impl KeyStore {
    pub(crate) async fn fetch_public_blog_runtime_data(
        &self,
        metric_start: i64,
        history_start: i64,
        now: i64,
        historical_quota_timestamps: &[i64],
    ) -> Result<PublicBlogRuntimeData, ProxyError> {
        let mut tx = self.pool.begin().await?;

        let hour_rows = sqlx::query(
            r#"
            SELECT
                ((bucket_start + 28800) / 3600) * 3600 - 28800 AS local_hour_start,
                COALESCE(SUM(total_requests), 0) AS requests,
                COALESCE(SUM(local_estimated_credits), 0) AS credits
            FROM dashboard_request_rollup_buckets
            WHERE bucket_secs = 60
              AND bucket_start >= ?
              AND bucket_start <= ?
            GROUP BY local_hour_start
            ORDER BY local_hour_start ASC
            "#,
        )
        .bind(metric_start)
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
        let hours = hour_rows
            .into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(PublicBlogRuntimeHour {
                    local_hour_start: row.try_get("local_hour_start")?,
                    requests: row.try_get("requests")?,
                    credits: row.try_get("credits")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let day_rows = sqlx::query(
            r#"
            SELECT
                strftime('%Y-%m-%d', bucket_start, 'unixepoch', '+8 hours') AS date,
                ((bucket_start + 28800) / 86400) * 86400 - 28800 AS local_day_start,
                COALESCE(SUM(total_requests), 0) AS requests,
                COALESCE(SUM(local_estimated_credits), 0) AS credits
            FROM dashboard_request_rollup_buckets
            WHERE bucket_secs = 60
              AND bucket_start >= ?
              AND bucket_start <= ?
            GROUP BY local_day_start, date
            ORDER BY local_day_start ASC
            "#,
        )
        .bind(history_start)
        .bind(now)
        .fetch_all(&mut *tx)
        .await?;
        let days = day_rows
            .into_iter()
            .map(|row| {
                Ok::<_, sqlx::Error>(PublicBlogRuntimeDay {
                    date: row.try_get("date")?,
                    local_day_start: row.try_get("local_day_start")?,
                    requests: row.try_get("requests")?,
                    credits: row.try_get("credits")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let current_quota_row = sqlx::query(
            r#"
            SELECT
                COUNT(*) AS eligible_keys,
                COALESCE(SUM(CASE
                    WHEN ak.quota_limit IS NULL
                      OR ak.quota_synced_at IS NULL
                      OR ak.quota_synced_at = 0
                    THEN 1 ELSE 0
                END), 0) AS unknown_keys,
                COALESCE(SUM(CASE
                    WHEN ak.quota_limit IS NULL
                      OR ak.quota_synced_at IS NULL
                      OR ak.quota_synced_at = 0
                    THEN 0 ELSE ak.quota_limit
                END), 0) AS total_quota_limit
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
            "#,
        )
        .fetch_one(&mut *tx)
        .await?;
        let eligible_keys: i64 = current_quota_row.try_get("eligible_keys")?;
        let unknown_keys: i64 = current_quota_row.try_get("unknown_keys")?;
        let current_quota_sum: i64 = current_quota_row.try_get("total_quota_limit")?;
        let total_quota_limit = if eligible_keys == 0 {
            Some(0)
        } else if unknown_keys > 0 {
            None
        } else {
            Some(current_quota_sum.max(0))
        };

        let lifecycle_tracked_from = sqlx::query_scalar::<_, i64>(
            "SELECT tracked_from FROM api_key_membership_history_state WHERE singleton = 1",
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut historical_quota_limits = Vec::with_capacity(historical_quota_timestamps.len());
        for timestamp in historical_quota_timestamps {
            if *timestamp < lifecycle_tracked_from {
                historical_quota_limits.push(None);
                continue;
            }

            let row = sqlx::query(
                r#"
                SELECT
                    COUNT(*) AS eligible_keys,
                    COALESCE(SUM(CASE WHEN quota_limit IS NULL THEN 1 ELSE 0 END), 0) AS unknown_keys,
                    COALESCE(SUM(CASE WHEN quota_limit IS NULL THEN 0 ELSE quota_limit END), 0) AS total_quota_limit
                FROM (
                    SELECT
                        ak.id,
                        COALESCE(
                            (
                                SELECT sample.quota_limit
                                FROM api_key_quota_sync_samples sample
                                WHERE sample.key_id = ak.id
                                  AND sample.captured_at >= membership.active_from
                                  AND sample.captured_at <= ?
                                ORDER BY sample.captured_at DESC, sample.id DESC
                                LIMIT 1
                            ),
                            CASE
                                WHEN ak.quota_synced_at >= membership.active_from
                                  AND ak.quota_synced_at <= ? THEN ak.quota_limit
                                ELSE NULL
                            END
                        ) AS quota_limit
                    FROM api_keys ak
                    JOIN api_key_membership_intervals membership ON membership.key_id = ak.id
                    WHERE membership.active_from <= ?
                      AND (membership.active_until IS NULL OR membership.active_until > ?)
                      AND NOT EXISTS (
                          SELECT 1
                          FROM api_key_quarantines aq
                          WHERE aq.key_id = ak.id
                            AND aq.created_at <= ?
                            AND (aq.cleared_at IS NULL OR aq.cleared_at > ?)
                      )
                ) eligible
                "#,
            )
            .bind(timestamp)
            .bind(timestamp)
            .bind(timestamp)
            .bind(timestamp)
            .bind(timestamp)
            .bind(timestamp)
            .fetch_one(&mut *tx)
            .await?;
            let eligible_keys: i64 = row.try_get("eligible_keys")?;
            let unknown_keys: i64 = row.try_get("unknown_keys")?;
            let total_quota_limit: i64 = row.try_get("total_quota_limit")?;
            historical_quota_limits.push(if eligible_keys == 0 {
                Some(0)
            } else if unknown_keys > 0 {
                None
            } else {
                Some(total_quota_limit.max(0))
            });
        }

        tx.commit().await?;

        Ok(PublicBlogRuntimeData {
            hours,
            days,
            total_quota_limit,
            historical_quota_limits,
        })
    }
}

/// PostgreSQL twins of the summary-window read helpers.
///
/// The SQLite implementations stay in `key_store_request_logs_summary_windows.rs` and keep
/// serving the request path until the cutover wave; these variants are reachable only
/// through the attached PostgreSQL pool.
impl KeyStore {
    pub(crate) async fn fetch_dashboard_rollup_freshness_signature_without_flush_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        range_start: i64,
    ) -> Result<[i64; 19], ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS bucket_count,
                COALESCE(MAX(updated_at), 0) AS max_updated_at,
                COALESCE(MAX(bucket_start), 0) AS max_bucket_start,
                COALESCE(SUM(bucket_secs), 0)::bigint AS bucket_secs_sum,
                COALESCE(SUM(total_requests), 0)::bigint AS total_requests_sum,
                COALESCE(SUM(success_count), 0)::bigint AS success_count_sum,
                COALESCE(SUM(error_count), 0)::bigint AS error_count_sum,
                COALESCE(SUM(quota_exhausted_count), 0)::bigint AS quota_exhausted_count_sum,
                COALESCE(SUM(valuable_success_count), 0)::bigint AS valuable_success_count_sum,
                COALESCE(SUM(valuable_failure_count), 0)::bigint AS valuable_failure_count_sum,
                COALESCE(SUM(valuable_failure_429_count), 0)::bigint AS valuable_failure_429_count_sum,
                COALESCE(SUM(other_success_count), 0)::bigint AS other_success_count_sum,
                COALESCE(SUM(other_failure_count), 0)::bigint AS other_failure_count_sum,
                COALESCE(SUM(unknown_count), 0)::bigint AS unknown_count_sum,
                COALESCE(SUM(mcp_non_billable), 0)::bigint AS mcp_non_billable_sum,
                COALESCE(SUM(mcp_billable), 0)::bigint AS mcp_billable_sum,
                COALESCE(SUM(api_non_billable), 0)::bigint AS api_non_billable_sum,
                COALESCE(SUM(api_billable), 0)::bigint AS api_billable_sum,
                COALESCE(SUM(local_estimated_credits), 0)::bigint AS local_estimated_credits_sum
            FROM observability.dashboard_request_rollup_buckets
            WHERE bucket_start >= $1
            "#,
        )
        .bind(range_start)
        .fetch_one(&mut **tx)
        .await?;
        Ok([
            row.try_get("bucket_count")?,
            row.try_get("max_updated_at")?,
            row.try_get("max_bucket_start")?,
            row.try_get("bucket_secs_sum")?,
            row.try_get("total_requests_sum")?,
            row.try_get("success_count_sum")?,
            row.try_get("error_count_sum")?,
            row.try_get("quota_exhausted_count_sum")?,
            row.try_get("valuable_success_count_sum")?,
            row.try_get("valuable_failure_count_sum")?,
            row.try_get("valuable_failure_429_count_sum")?,
            row.try_get("other_success_count_sum")?,
            row.try_get("other_failure_count_sum")?,
            row.try_get("unknown_count_sum")?,
            row.try_get("mcp_non_billable_sum")?,
            row.try_get("mcp_billable_sum")?,
            row.try_get("api_non_billable_sum")?,
            row.try_get("api_billable_sum")?,
            row.try_get("local_estimated_credits_sum")?,
        ])
    }

    async fn fetch_summary_windows_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        bounds: SummaryWindowBounds,
    ) -> Result<SummaryWindows, ProxyError> {
        let SummaryWindowBounds {
            today_start,
            today_end,
            today_period_end,
            yesterday_start,
            yesterday_end,
            month_start,
            month_quota_charge_start,
            month_period_end,
            previous_month_start,
            previous_month_end,
        } = bounds;
        let today_metrics = Self::fetch_dashboard_rollup_window_metrics_tx_pg(
            &mut *tx,
            SECS_PER_MINUTE,
            today_start,
            Some(today_end),
        )
        .await?;
        let yesterday_metrics = Self::fetch_dashboard_rollup_window_metrics_tx_pg(
            &mut *tx,
            SECS_PER_MINUTE,
            yesterday_start,
            Some(yesterday_end),
        )
        .await?;
        let month_metrics = Self::fetch_dashboard_rollup_month_metrics_tx_pg(
            &mut *tx,
            month_start,
            today_start,
            today_end,
        )
        .await?;
        let month_charge_metrics = Self::fetch_dashboard_rollup_month_metrics_tx_pg(
            &mut *tx,
            month_quota_charge_start,
            today_start,
            today_end,
        )
        .await?;

        let lifecycle_row = sqlx::query(
            r#"
            SELECT
                COUNT(DISTINCT CASE WHEN created_at >= $1 AND created_at < $2 THEN key_id END) AS today_upstream_exhausted_key_count,
                COUNT(DISTINCT CASE WHEN created_at >= $3 AND created_at < $4 THEN key_id END) AS yesterday_upstream_exhausted_key_count,
                COUNT(DISTINCT CASE WHEN created_at >= $5 AND created_at < $6 THEN key_id END) AS month_upstream_exhausted_key_count
            FROM core.api_key_maintenance_records
            WHERE source = $7
              AND operation_code = $8
              AND reason_code = $9
              AND created_at >= $10
              AND created_at < $11
            "#,
        )
        .bind(today_start)
        .bind(today_end)
        .bind(yesterday_start)
        .bind(yesterday_end)
        .bind(month_start)
        .bind(today_end)
        .bind(MAINTENANCE_SOURCE_SYSTEM)
        .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(yesterday_start.min(month_start))
        .bind(today_end)
        .fetch_one(&mut **tx)
        .await?;

        let month_lifecycle_row = sqlx::query(
            r#"
            SELECT
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM core.api_keys
                    WHERE created_at >= $1
                ) AS month_new_keys,
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM core.api_key_quarantines
                    WHERE created_at >= $2
                ) AS month_new_quarantines
            "#,
        )
        .bind(month_start)
        .bind(month_start)
        .fetch_one(&mut **tx)
        .await?;

        Ok(SummaryWindows {
            today: SummaryWindowMetrics {
                upstream_exhausted_key_count: lifecycle_row
                    .try_get("today_upstream_exhausted_key_count")?,
                ..today_metrics
            },
            yesterday: SummaryWindowMetrics {
                upstream_exhausted_key_count: lifecycle_row
                    .try_get("yesterday_upstream_exhausted_key_count")?,
                ..yesterday_metrics
            },
            month: SummaryWindowMetrics {
                upstream_exhausted_key_count: lifecycle_row
                    .try_get("month_upstream_exhausted_key_count")?,
                new_keys: month_lifecycle_row.try_get("month_new_keys")?,
                new_quarantines: month_lifecycle_row.try_get("month_new_quarantines")?,
                quota_charge: SummaryQuotaCharge {
                    local_estimated_credits: month_charge_metrics
                        .quota_charge
                        .local_estimated_credits,
                    ..month_metrics.quota_charge
                },
                ..month_metrics
            },
            today_start,
            today_end,
            today_period_end,
            yesterday_start,
            yesterday_end,
            month_start,
            month_end: today_end,
            month_period_end,
            previous_month_start,
            previous_month_end,
        })
    }

    async fn fetch_dashboard_hourly_request_window_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        current_bucket_start: i64,
        bucket_seconds: i64,
        visible_buckets: i64,
        retained_buckets: i64,
    ) -> Result<DashboardHourlyRequestWindow, ProxyError> {
        if bucket_seconds <= 0 || visible_buckets <= 0 || retained_buckets <= 0 {
            return Ok(DashboardHourlyRequestWindow {
                bucket_seconds,
                visible_buckets,
                retained_buckets,
                buckets: Vec::new(),
                unverified_bucket_starts: Vec::new(),
            });
        }

        let series_start = current_bucket_start
            .saturating_sub(bucket_seconds.saturating_mul(retained_buckets.saturating_sub(1)));
        let series_end_exclusive = current_bucket_start.saturating_add(bucket_seconds);
        let bucket_alignment_offset = current_bucket_start.rem_euclid(bucket_seconds);
        let rows = sqlx::query(
            r#"
            WITH RECURSIVE hour_series(bucket_start) AS (
                SELECT $1::bigint AS bucket_start
                UNION ALL
                SELECT bucket_start + $2
                FROM hour_series
                WHERE bucket_start + $3 < $4
            ),
            aggregated AS (
                SELECT
                    ((bucket_start - $5) / $6) * $7 + $8 AS hour_bucket_start,
                    COALESCE(SUM(other_success_count), 0)::bigint AS secondary_success,
                    COALESCE(SUM(valuable_success_count), 0)::bigint AS primary_success,
                    COALESCE(SUM(other_failure_count), 0)::bigint AS secondary_failure,
                    COALESCE(SUM(valuable_failure_429_count), 0)::bigint AS primary_failure_429,
                    COALESCE(
                        SUM(
                            CASE
                                WHEN valuable_failure_count > valuable_failure_429_count
                                    THEN valuable_failure_count - valuable_failure_429_count
                                ELSE 0
                            END
                        ),
                        0
                    )::bigint AS primary_failure_other,
                    COALESCE(SUM(unknown_count), 0)::bigint AS unknown_count,
                    COALESCE(SUM(mcp_non_billable), 0)::bigint AS mcp_non_billable,
                    COALESCE(SUM(mcp_billable), 0)::bigint AS mcp_billable,
                    COALESCE(SUM(api_non_billable), 0)::bigint AS api_non_billable,
                    COALESCE(SUM(api_billable), 0)::bigint AS api_billable,
                    COALESCE(SUM(local_estimated_credits), 0)::bigint AS local_estimated_credits
                FROM observability.dashboard_request_rollup_buckets
                WHERE bucket_secs = $9
                  AND bucket_start >= $10
                  AND bucket_start < $11
                GROUP BY hour_bucket_start
            )
            SELECT
                hour_series.bucket_start,
                COALESCE(aggregated.secondary_success, 0) AS secondary_success,
                COALESCE(aggregated.primary_success, 0) AS primary_success,
                COALESCE(aggregated.secondary_failure, 0) AS secondary_failure,
                COALESCE(aggregated.primary_failure_429, 0) AS primary_failure_429,
                COALESCE(aggregated.primary_failure_other, 0) AS primary_failure_other,
                COALESCE(aggregated.unknown_count, 0) AS unknown_count,
                COALESCE(aggregated.mcp_non_billable, 0) AS mcp_non_billable,
                COALESCE(aggregated.mcp_billable, 0) AS mcp_billable,
                COALESCE(aggregated.api_non_billable, 0) AS api_non_billable,
                COALESCE(aggregated.api_billable, 0) AS api_billable,
                COALESCE(aggregated.local_estimated_credits, 0) AS local_estimated_credits
            FROM hour_series
            LEFT JOIN aggregated ON aggregated.hour_bucket_start = hour_series.bucket_start
            ORDER BY hour_series.bucket_start ASC
            "#,
        )
        .bind(series_start)
        .bind(bucket_seconds)
        .bind(bucket_seconds)
        .bind(series_end_exclusive)
        .bind(bucket_alignment_offset)
        .bind(bucket_seconds)
        .bind(bucket_seconds)
        .bind(bucket_alignment_offset)
        .bind(SECS_PER_MINUTE)
        .bind(series_start)
        .bind(series_end_exclusive)
        .fetch_all(&mut **tx)
        .await?;

        let sample_rows = sqlx::query(
            r#"
            WITH window_rows AS (
                SELECT id, key_id, quota_remaining, captured_at
                FROM core.api_key_quota_sync_samples
                WHERE captured_at >= $1
                  AND captured_at < $2
            ),
            sampled_keys AS (
                SELECT DISTINCT key_id FROM window_rows
            ),
            baseline_rows AS (
                SELECT s.id, s.key_id, s.quota_remaining, s.captured_at
                FROM core.api_key_quota_sync_samples s
                INNER JOIN (
                    SELECT key_id, MAX(captured_at) AS captured_at
                    FROM core.api_key_quota_sync_samples
                    WHERE captured_at < $3
                      AND key_id IN (SELECT key_id FROM sampled_keys)
                    GROUP BY key_id
                ) latest
                    ON latest.key_id = s.key_id
                   AND latest.captured_at = s.captured_at
            )
            SELECT id, key_id, quota_remaining, captured_at
            FROM window_rows
            UNION ALL
            SELECT id, key_id, quota_remaining, captured_at
            FROM baseline_rows
            ORDER BY key_id ASC, captured_at ASC, id ASC
            "#,
        )
        .bind(series_start)
        .bind(series_end_exclusive)
        .bind(series_start)
        .fetch_all(&mut **tx)
        .await?;

        let mut upstream_actual_by_bucket = std::collections::HashMap::<i64, i64>::new();
        let mut current_key: Option<String> = None;
        let mut previous_remaining: Option<i64> = None;
        for row in sample_rows {
            let key_id: String = row.try_get("key_id")?;
            if current_key.as_deref() != Some(key_id.as_str()) {
                current_key = Some(key_id);
                previous_remaining = None;
            }

            let quota_remaining: i64 = row.try_get("quota_remaining")?;
            let captured_at: i64 = row.try_get("captured_at")?;
            if captured_at >= series_start
                && captured_at < series_end_exclusive
                && let Some(previous) = previous_remaining
            {
                let bucket_start = ((captured_at - bucket_alignment_offset) / bucket_seconds)
                    * bucket_seconds
                    + bucket_alignment_offset;
                *upstream_actual_by_bucket.entry(bucket_start).or_default() +=
                    (previous - quota_remaining).max(0);
            }
            previous_remaining = Some(quota_remaining);
        }

        let buckets = rows
            .into_iter()
            .map(|row| {
                Ok(DashboardHourlyRequestBucket {
                    bucket_start: row.try_get("bucket_start")?,
                    secondary_success: row.try_get("secondary_success")?,
                    primary_success: row.try_get("primary_success")?,
                    secondary_failure: row.try_get("secondary_failure")?,
                    primary_failure_429: row.try_get("primary_failure_429")?,
                    primary_failure_other: row.try_get("primary_failure_other")?,
                    unknown: row.try_get("unknown_count")?,
                    mcp_non_billable: row.try_get("mcp_non_billable")?,
                    mcp_billable: row.try_get("mcp_billable")?,
                    api_non_billable: row.try_get("api_non_billable")?,
                    api_billable: row.try_get("api_billable")?,
                    local_estimated_credits: row.try_get("local_estimated_credits")?,
                    upstream_actual_credits: upstream_actual_by_bucket
                        .get(&row.try_get::<i64, _>("bucket_start")?)
                        .copied(),
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        let gap_rows = sqlx::query(
            r#"
            SELECT range_start, range_end FROM observability.dashboard_rollup_integrity_gaps
            WHERE range_start < $1 AND range_end > $2
            UNION
            SELECT range_start, range_end FROM observability.dashboard_rollup_integrity_work_items
            WHERE status = 'pending' AND range_start < $3 AND range_end > $4
            UNION
            SELECT hot_cursor AS range_start, hot_fence AS range_end
            FROM observability.dashboard_rollup_integrity_state
            WHERE hot_cursor < hot_fence AND hot_cursor < $5 AND hot_fence > $6
            UNION
            SELECT MIN(log.created_at) AS range_start, state.history_cursor AS range_end
            FROM observability.dashboard_rollup_integrity_state AS state
            JOIN observability.request_logs AS log
                ON log.visibility = $7 AND log.created_at < state.history_cursor
            WHERE state.history_cursor > $8 AND log.created_at < $9
            UNION
            SELECT bucket_start AS range_start, bucket_end AS range_end
            FROM observability.dashboard_rollup_integrity_day_reaudits
            WHERE status = 'pending' AND bucket_start < $10 AND bucket_end > $11
            UNION
            SELECT $12::bigint AS range_start, $13::bigint AS range_end
            WHERE NOT EXISTS (
                SELECT 1 FROM observability.dashboard_rollup_integrity_state WHERE id = 1
            )
            ORDER BY range_start ASC
            "#,
        )
        .bind(series_end_exclusive)
        .bind(series_start)
        .bind(series_end_exclusive)
        .bind(series_start)
        .bind(series_end_exclusive)
        .bind(series_start)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(series_start)
        .bind(series_end_exclusive)
        .bind(series_end_exclusive)
        .bind(series_start)
        .bind(series_start)
        .bind(series_end_exclusive)
        .fetch_all(&mut **tx)
        .await?;
        let mut unverified_bucket_starts = Vec::new();
        for bucket_start in (series_start..series_end_exclusive).step_by(bucket_seconds as usize) {
            if gap_rows.iter().any(|gap| {
                let gap_start: i64 = gap.try_get("range_start").unwrap_or_default();
                let gap_end: i64 = gap.try_get("range_end").unwrap_or_default();
                gap_start < bucket_start.saturating_add(bucket_seconds) && gap_end > bucket_start
            }) {
                unverified_bucket_starts.push(bucket_start);
            }
        }

        Ok(DashboardHourlyRequestWindow {
            bucket_seconds,
            visible_buckets,
            retained_buckets,
            buckets,
            unverified_bucket_starts,
        })
    }

}

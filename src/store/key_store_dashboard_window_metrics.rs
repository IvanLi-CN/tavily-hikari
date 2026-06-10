impl KeyStore {
    async fn fetch_dashboard_rollup_window_metrics_tx(
        tx: &mut Transaction<'_, Sqlite>,
        bucket_secs: i64,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        if let Some(bucket_start_before) = bucket_start_before
            && bucket_secs == SECS_PER_MINUTE
        {
            if bucket_start_at_least >= bucket_start_before {
                return Ok(SummaryWindowMetrics::default());
            }

            let full_bucket_end = bucket_start_before.div_euclid(bucket_secs) * bucket_secs;
            let mut metrics = Self::fetch_dashboard_rollup_aligned_window_metrics_tx(
                tx,
                bucket_secs,
                bucket_start_at_least,
                Some(full_bucket_end),
            )
            .await?;

            if full_bucket_end < bucket_start_before {
                add_summary_window_metrics(
                    &mut metrics,
                    &Self::fetch_dashboard_request_log_window_metrics_tx(
                        tx,
                        full_bucket_end.max(bucket_start_at_least),
                        bucket_start_before,
                    )
                    .await?,
                );
            }

            return Ok(metrics);
        }

        Self::fetch_dashboard_rollup_aligned_window_metrics_tx(
            tx,
            bucket_secs,
            bucket_start_at_least,
            bucket_start_before,
        )
        .await
    }

    async fn fetch_dashboard_rollup_aligned_window_metrics_tx(
        tx: &mut Transaction<'_, Sqlite>,
        bucket_secs: i64,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let row = if let Some(bucket_start_before) = bucket_start_before {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0) AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0) AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count,
                    COALESCE(SUM(local_estimated_credits), 0) AS local_estimated_credits
                FROM dashboard_request_rollup_buckets
                WHERE bucket_secs = ?
                  AND bucket_start >= ?
                  AND bucket_start < ?
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(&mut **tx)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT
                    COALESCE(SUM(total_requests), 0) AS total_requests,
                    COALESCE(SUM(success_count), 0) AS success_count,
                    COALESCE(SUM(error_count), 0) AS error_count,
                    COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count,
                    COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count,
                    COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count,
                    COALESCE(SUM(other_success_count), 0) AS other_success_count,
                    COALESCE(SUM(other_failure_count), 0) AS other_failure_count,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count,
                    COALESCE(SUM(local_estimated_credits), 0) AS local_estimated_credits
                FROM dashboard_request_rollup_buckets
                WHERE bucket_secs = ?
                  AND bucket_start >= ?
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_start_at_least)
            .fetch_one(&mut **tx)
            .await?
        };

        Ok(SummaryWindowMetrics {
            total_requests: row.try_get("total_requests")?,
            success_count: row.try_get("success_count")?,
            error_count: row.try_get("error_count")?,
            quota_exhausted_count: row.try_get("quota_exhausted_count")?,
            valuable_success_count: row.try_get("valuable_success_count")?,
            valuable_failure_count: row.try_get("valuable_failure_count")?,
            other_success_count: row.try_get("other_success_count")?,
            other_failure_count: row.try_get("other_failure_count")?,
            unknown_count: row.try_get("unknown_count")?,
            upstream_exhausted_key_count: 0,
            new_keys: 0,
            new_quarantines: 0,
            quota_charge: SummaryQuotaCharge {
                local_estimated_credits: row.try_get("local_estimated_credits")?,
                ..SummaryQuotaCharge::default()
            },
        })
    }

    async fn fetch_dashboard_request_log_window_metrics_tx(
        tx: &mut Transaction<'_, Sqlite>,
        start: i64,
        end: i64,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        if start >= end {
            return Ok(SummaryWindowMetrics::default());
        }

        let mut rows = sqlx::query(
            r#"
            SELECT result_status, failure_kind, request_kind_key, request_body, path, business_credits, counts_business_quota
            FROM request_logs
            WHERE visibility = ?
              AND created_at >= ?
              AND created_at < ?
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(start)
        .bind(end)
        .fetch(&mut **tx);

        let mut counts = DashboardRequestRollupCounts::default();
        while let Some(row) = rows.try_next().await? {
            let result_status: String = row.try_get("result_status")?;
            let failure_kind: Option<String> = row.try_get("failure_kind")?;
            let stored_request_kind_key: Option<String> = row.try_get("request_kind_key")?;
            let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
            let path: String = row.try_get("path")?;
            let business_credits: Option<i64> = row.try_get("business_credits")?;
            let stored_counts_business_quota: Option<i64> =
                row.try_get("counts_business_quota")?;
            let request_kind_key = canonicalize_request_log_request_kind(
                &path,
                request_body.as_deref(),
                stored_request_kind_key,
                None,
                None,
            )
            .key;
            let counts_business_quota = stored_counts_business_quota
                .map(|value| value != 0)
                .unwrap_or_else(|| {
                    request_log_counts_business_quota(&request_kind_key, request_body.as_deref())
                });

            counts.add(Self::dashboard_rollup_counts_for_request(
                &request_kind_key,
                request_body.as_deref(),
                &result_status,
                failure_kind.as_deref(),
                business_credits.unwrap_or_default(),
                counts_business_quota,
            ));
        }

        Ok(summary_window_metrics_from_dashboard_counts(counts))
    }
}

/// PostgreSQL twins of the dashboard month-series helpers.
///
/// The SQLite implementations stay in `key_store_dashboard_month_series.rs`; these variants
/// are reachable only through the attached PostgreSQL pool.
impl KeyStore {
    async fn fetch_dashboard_month_lifecycle_daily_counts_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        range_start: i64,
        range_end: i64,
    ) -> Result<
        (
            std::collections::HashMap<i64, i64>,
            std::collections::HashMap<i64, i64>,
            std::collections::HashMap<i64, i64>,
        ),
        ProxyError,
    > {
        if range_end <= range_start {
            return Ok((
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
            ));
        }

        let count_local_days = |timestamps: Vec<i64>| -> std::collections::HashMap<i64, i64> {
            let mut counts = std::collections::HashMap::new();
            for created_at in timestamps {
                *counts
                    .entry(local_day_bucket_start_utc_ts(created_at))
                    .or_insert(0) += 1;
            }
            counts
        };

        let new_keys_created_at = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT created_at
            FROM core.api_keys
            WHERE created_at >= $1
              AND created_at < $2
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&mut **tx)
        .await?;

        let new_quarantines_created_at = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT created_at
            FROM core.api_key_quarantines
            WHERE created_at >= $1
              AND created_at < $2
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&mut **tx)
        .await?;

        let upstream_exhausted_first_created_at = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT MIN(created_at) AS first_created_at
            FROM core.api_key_maintenance_records
            WHERE source = $1
              AND operation_code = $2
              AND reason_code = $3
              AND created_at >= $4
              AND created_at < $5
            GROUP BY key_id
            "#,
        )
        .bind(MAINTENANCE_SOURCE_SYSTEM)
        .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&mut **tx)
        .await?;

        Ok((
            count_local_days(upstream_exhausted_first_created_at),
            count_local_days(new_keys_created_at),
            count_local_days(new_quarantines_created_at),
        ))
    }

    async fn fetch_dashboard_month_series_points_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        range_start: i64,
        range_end: i64,
        now_cutoff: i64,
    ) -> Result<Vec<DashboardMonthSeriesPoint>, ProxyError> {
        if range_end <= range_start {
            return Ok(Vec::new());
        }

        let mut points = Vec::new();
        let mut running_total = SummaryWindowMetrics::default();
        let current_local_day_start = local_day_bucket_start_utc_ts(now_cutoff.saturating_sub(1));
        let lifecycle_end = range_end.min(now_cutoff);
        let (
            upstream_exhausted_by_day,
            new_keys_by_day,
            new_quarantines_by_day,
        ) = Self::fetch_dashboard_month_lifecycle_daily_counts_tx_pg(tx, range_start, lifecycle_end)
            .await?;
        let mut running_upstream_exhausted = 0;
        let mut running_new_keys = 0;
        let mut running_new_quarantines = 0;

        for (bucket_start, bucket_end) in
            Self::collect_bucket_ranges(range_start, range_end, next_local_day_start_utc_ts)
        {
            let point = if !Self::should_populate_dashboard_month_series_bucket(
                bucket_start,
                bucket_end,
                now_cutoff,
                current_local_day_start,
            ) {
                DashboardMonthSeriesPoint {
                    bucket_start,
                    display_bucket_start: Some(bucket_start),
                    ..DashboardMonthSeriesPoint::default()
                }
            } else {
                let bucket_metrics = if bucket_start == current_local_day_start {
                    Self::fetch_dashboard_rollup_window_metrics_tx_pg(
                        tx,
                        SECS_PER_MINUTE,
                        bucket_start,
                        Some(bucket_end.min(now_cutoff)),
                    )
                    .await?
                } else {
                    Self::fetch_dashboard_rollup_bucket_metrics_in_range_tx_pg(
                        tx,
                        SECS_PER_DAY,
                        bucket_start,
                        bucket_end,
                    )
                    .await?
                };
                add_summary_window_metrics(&mut running_total, &bucket_metrics);
                running_upstream_exhausted += upstream_exhausted_by_day
                    .get(&bucket_start)
                    .copied()
                    .unwrap_or_default();
                running_new_keys += new_keys_by_day.get(&bucket_start).copied().unwrap_or_default();
                running_new_quarantines += new_quarantines_by_day
                    .get(&bucket_start)
                    .copied()
                    .unwrap_or_default();
                DashboardMonthSeriesPoint {
                    bucket_start,
                    display_bucket_start: Some(bucket_start),
                    total: Some(running_total.total_requests),
                    valuable_success: Some(running_total.valuable_success_count),
                    valuable_failure: Some(running_total.valuable_failure_count),
                    other_success: Some(running_total.other_success_count),
                    other_failure: Some(running_total.other_failure_count),
                    unknown: Some(running_total.unknown_count),
                    upstream_exhausted: Some(running_upstream_exhausted),
                    new_keys: Some(running_new_keys),
                    new_quarantines: Some(running_new_quarantines),
                }
            };
            points.push(point);
        }

        Ok(points)
    }

    async fn dashboard_month_series_has_retained_data_tx_pg(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        range_start: i64,
        range_end: i64,
    ) -> Result<bool, ProxyError> {
        if range_end <= range_start {
            return Ok(false);
        }

        let retained = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT CASE
                WHEN EXISTS(
                    SELECT 1
                    FROM observability.dashboard_request_rollup_buckets
                    WHERE bucket_start >= $1
                      AND bucket_start < $2
                ) THEN 1
                WHEN EXISTS(
                    SELECT 1
                    FROM core.api_keys
                    WHERE created_at >= $3
                      AND created_at < $4
                ) THEN 1
                WHEN EXISTS(
                    SELECT 1
                    FROM core.api_key_quarantines
                    WHERE created_at >= $5
                      AND created_at < $6
                ) THEN 1
                WHEN EXISTS(
                    SELECT 1
                    FROM core.api_key_maintenance_records
                    WHERE source = $7
                      AND operation_code = $8
                      AND reason_code = $9
                      AND created_at >= $10
                      AND created_at < $11
                ) THEN 1
                ELSE 0
            END
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .bind(range_start)
        .bind(range_end)
        .bind(range_start)
        .bind(range_end)
        .bind(MAINTENANCE_SOURCE_SYSTEM)
        .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(range_start)
        .bind(range_end)
        .fetch_one(&mut **tx)
        .await?;

        Ok(retained != 0)
    }

}

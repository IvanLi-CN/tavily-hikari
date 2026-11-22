impl KeyStore {
    async fn dashboard_month_series_has_retained_data_tx(
        tx: &mut Transaction<'_, Sqlite>,
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
                    FROM dashboard_request_rollup_buckets
                    WHERE bucket_start >= ?
                      AND bucket_start < ?
                ) THEN 1
                WHEN EXISTS(
                    SELECT 1
                    FROM api_keys
                    WHERE created_at >= ?
                      AND created_at < ?
                ) THEN 1
                WHEN EXISTS(
                    SELECT 1
                    FROM api_key_quarantines
                    WHERE created_at >= ?
                      AND created_at < ?
                ) THEN 1
                WHEN EXISTS(
                    SELECT 1
                    FROM api_key_maintenance_records
                    WHERE source = ?
                      AND operation_code = ?
                      AND reason_code = ?
                      AND created_at >= ?
                      AND created_at < ?
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

    pub(crate) fn collect_bucket_ranges<F>(
        range_start: i64,
        range_end: i64,
        mut next_bucket_start: F,
    ) -> Vec<(i64, i64)>
    where
        F: FnMut(i64) -> i64,
    {
        let mut ranges = Vec::new();
        let mut bucket_start = range_start;
        while bucket_start < range_end {
            let bucket_end = next_bucket_start(bucket_start);
            let clamped_bucket_end = bucket_end.min(range_end);
            if clamped_bucket_end <= bucket_start {
                break;
            }
            ranges.push((bucket_start, clamped_bucket_end));
            bucket_start = clamped_bucket_end;
        }
        ranges
    }

    pub(crate) fn should_populate_dashboard_month_series_bucket(
        bucket_start: i64,
        bucket_end: i64,
        now_cutoff: i64,
        current_partial_bucket_start: i64,
    ) -> bool {
        if bucket_start >= now_cutoff {
            return false;
        }
        if bucket_end > now_cutoff && bucket_start != current_partial_bucket_start {
            return false;
        }
        true
    }

    async fn fetch_dashboard_month_lifecycle_daily_counts_tx(
        tx: &mut Transaction<'_, Sqlite>,
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
            FROM api_keys
            WHERE created_at >= ?
              AND created_at < ?
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&mut **tx)
        .await?;

        let new_quarantines_created_at = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT created_at
            FROM api_key_quarantines
            WHERE created_at >= ?
              AND created_at < ?
            "#,
        )
        .bind(range_start)
        .bind(range_end)
        .fetch_all(&mut **tx)
        .await?;

        let upstream_exhausted_first_created_at = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT MIN(created_at) AS first_created_at
            FROM api_key_maintenance_records
            WHERE source = ?
              AND operation_code = ?
              AND reason_code = ?
              AND created_at >= ?
              AND created_at < ?
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

    async fn fetch_dashboard_month_series_points_tx(
        tx: &mut Transaction<'_, Sqlite>,
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
        ) = Self::fetch_dashboard_month_lifecycle_daily_counts_tx(tx, range_start, lifecycle_end)
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
                    Self::fetch_dashboard_rollup_window_metrics_tx(
                        tx,
                        SECS_PER_MINUTE,
                        bucket_start,
                        Some(bucket_end.min(now_cutoff)),
                    )
                    .await?
                } else {
                    Self::fetch_dashboard_rollup_bucket_metrics_in_range_tx(
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

    pub(crate) async fn fetch_dashboard_month_series(
        &self,
        summary_windows: &SummaryWindows,
    ) -> Result<DashboardMonthSeries, ProxyError> {
        let mut tx = self.pool.begin().await?;

        let current = Self::fetch_dashboard_month_series_points_tx(
            &mut tx,
            summary_windows.month_start,
            summary_windows.month_period_end,
            summary_windows.month_end,
        )
        .await?;
        let comparison_has_retained_data = Self::dashboard_month_series_has_retained_data_tx(
            &mut tx,
            summary_windows.previous_month_start,
            summary_windows.previous_month_end,
        )
        .await?;
        let mut comparison = Self::fetch_dashboard_month_series_points_tx(
            &mut tx,
            summary_windows.previous_month_start,
            summary_windows.previous_month_end,
            summary_windows.previous_month_end,
        )
        .await?;
        for (index, point) in comparison.iter_mut().enumerate() {
            point.display_bucket_start = current
                .get(index)
                .map(|current_point| {
                    current_point
                        .display_bucket_start
                        .unwrap_or(current_point.bucket_start)
                });
        }
        if !comparison_has_retained_data {
            comparison.clear();
        }

        tx.commit().await?;

        Ok(DashboardMonthSeries {
            current,
            comparison,
        })
    }
}

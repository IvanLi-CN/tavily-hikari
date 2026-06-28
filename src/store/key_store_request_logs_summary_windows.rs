impl KeyStore {
    pub(crate) async fn fetch_dashboard_quota_charge_token(
        &self,
        stale_key_count: i64,
        month_quota_charge_start: i64,
        today_end: i64,
    ) -> Result<[i64; 6], ProxyError> {
        let latest_sync_at = self
            .fetch_latest_dashboard_quota_sync_sample_at()
            .await?
            .unwrap_or_default();
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS sample_count,
                COALESCE(SUM(captured_at), 0) AS captured_at_sum,
                COALESCE(SUM(quota_remaining), 0) AS quota_remaining_sum,
                COALESCE(COUNT(DISTINCT key_id), 0) AS sampled_key_count
            FROM api_key_quota_sync_samples
            WHERE captured_at >= ?
              AND captured_at < ?
            "#,
        )
        .bind(month_quota_charge_start)
        .bind(today_end)
        .fetch_one(&self.pool)
        .await?;
        Ok([
            latest_sync_at,
            row.try_get("sample_count")?,
            row.try_get("captured_at_sum")?,
            row.try_get("quota_remaining_sum")?,
            row.try_get("sampled_key_count")?,
            stale_key_count,
        ])
    }

    pub(crate) async fn fetch_dashboard_quota_charge_snapshot(
        &self,
        bounds: SummaryWindowBounds,
    ) -> Result<DashboardQuotaChargeSnapshot, ProxyError> {
        let SummaryWindowBounds {
            today_start,
            today_end,
            yesterday_start,
            yesterday_end,
            month_quota_charge_start,
            ..
        } = bounds;
        let sample_window_start = yesterday_start.min(month_quota_charge_start);
        let now_ts = today_end.saturating_sub(1);
        let hot_active_since = now_ts.saturating_sub(2 * 60 * 60);
        let hot_stale_before = now_ts.saturating_sub(15 * 60);
        let cold_stale_before = now_ts.saturating_sub(24 * 60 * 60);

        let sample_rows = sqlx::query(
            r#"
            WITH window_rows AS (
                SELECT key_id, quota_remaining, captured_at
                FROM api_key_quota_sync_samples
                WHERE captured_at >= ?
                  AND captured_at < ?
            ),
            sampled_keys AS (
                SELECT DISTINCT key_id FROM window_rows
            ),
            baseline_rows AS (
                SELECT s.key_id, s.quota_remaining, s.captured_at
                FROM api_key_quota_sync_samples s
                INNER JOIN (
                    SELECT key_id, MAX(captured_at) AS captured_at
                    FROM api_key_quota_sync_samples
                    WHERE captured_at < ?
                      AND key_id IN (SELECT key_id FROM sampled_keys)
                    GROUP BY key_id
                ) latest
                    ON latest.key_id = s.key_id
                   AND latest.captured_at = s.captured_at
            )
            SELECT key_id, quota_remaining, captured_at
            FROM window_rows
            UNION ALL
            SELECT key_id, quota_remaining, captured_at
            FROM baseline_rows
            ORDER BY key_id ASC, captured_at ASC
            "#,
        )
        .bind(sample_window_start)
        .bind(today_end)
        .bind(sample_window_start)
        .fetch_all(&self.pool)
        .await?;

        let stale_key_count = self
            .fetch_dashboard_stale_key_count(hot_active_since, hot_stale_before, cold_stale_before)
            .await?;

        let mut today_charge = QuotaChargeAccumulator::default();
        let mut yesterday_charge = QuotaChargeAccumulator::default();
        let mut month_charge = QuotaChargeAccumulator::default();
        let mut today_sampled_keys = std::collections::HashSet::new();
        let mut yesterday_sampled_keys = std::collections::HashSet::new();
        let mut month_sampled_keys = std::collections::HashSet::new();
        let mut current_key: Option<String> = None;
        let mut previous_sample: Option<QuotaSyncSampleRow> = None;

        for row in sample_rows {
            let key_id: String = row.try_get("key_id")?;
            if current_key.as_deref() != Some(key_id.as_str()) {
                current_key = Some(key_id.clone());
                previous_sample = None;
            }

            let sample = QuotaSyncSampleRow {
                quota_remaining: row.try_get("quota_remaining")?,
                captured_at: row.try_get("captured_at")?,
            };
            let delta = previous_sample
                .map(|previous| (previous.quota_remaining - sample.quota_remaining).max(0))
                .unwrap_or(0);

            if sample.captured_at >= month_quota_charge_start && sample.captured_at < today_end {
                month_charge.upstream_actual_credits += delta;
                month_sampled_keys.insert(key_id.clone());
                if month_charge
                    .latest_sync_at
                    .map(|latest| sample.captured_at > latest)
                    .unwrap_or(true)
                {
                    month_charge.latest_sync_at = Some(sample.captured_at);
                }
            }
            if sample.captured_at >= today_start && sample.captured_at < today_end {
                today_charge.upstream_actual_credits += delta;
                today_sampled_keys.insert(key_id.clone());
                if today_charge
                    .latest_sync_at
                    .map(|latest| sample.captured_at > latest)
                    .unwrap_or(true)
                {
                    today_charge.latest_sync_at = Some(sample.captured_at);
                }
            }
            if sample.captured_at >= yesterday_start && sample.captured_at < yesterday_end {
                yesterday_charge.upstream_actual_credits += delta;
                yesterday_sampled_keys.insert(key_id.clone());
                if yesterday_charge
                    .latest_sync_at
                    .map(|latest| sample.captured_at > latest)
                    .unwrap_or(true)
                {
                    yesterday_charge.latest_sync_at = Some(sample.captured_at);
                }
            }

            previous_sample = Some(sample);
        }

        today_charge.sampled_key_count = today_sampled_keys.len() as i64;
        today_charge.stale_key_count = stale_key_count;
        yesterday_charge.sampled_key_count = yesterday_sampled_keys.len() as i64;
        yesterday_charge.stale_key_count = stale_key_count;
        month_charge.sampled_key_count = month_sampled_keys.len() as i64;
        month_charge.stale_key_count = stale_key_count;

        Ok(DashboardQuotaChargeSnapshot {
            today: SummaryQuotaCharge {
                upstream_actual_credits: today_charge.upstream_actual_credits,
                sampled_key_count: today_charge.sampled_key_count,
                stale_key_count: today_charge.stale_key_count,
                latest_sync_at: today_charge.latest_sync_at,
                ..Default::default()
            },
            yesterday: SummaryQuotaCharge {
                upstream_actual_credits: yesterday_charge.upstream_actual_credits,
                sampled_key_count: yesterday_charge.sampled_key_count,
                stale_key_count: yesterday_charge.stale_key_count,
                latest_sync_at: yesterday_charge.latest_sync_at,
                ..Default::default()
            },
            month: SummaryQuotaCharge {
                upstream_actual_credits: month_charge.upstream_actual_credits,
                sampled_key_count: month_charge.sampled_key_count,
                stale_key_count: month_charge.stale_key_count,
                latest_sync_at: month_charge.latest_sync_at,
                ..Default::default()
            },
        })
    }

    pub(crate) async fn fetch_dashboard_rollup_freshness_signature_without_flush(
        &self,
        range_start: i64,
    ) -> Result<[i64; 19], ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS bucket_count,
                COALESCE(MAX(updated_at), 0) AS max_updated_at,
                COALESCE(MAX(bucket_start), 0) AS max_bucket_start,
                COALESCE(SUM(bucket_secs), 0) AS bucket_secs_sum,
                COALESCE(SUM(total_requests), 0) AS total_requests_sum,
                COALESCE(SUM(success_count), 0) AS success_count_sum,
                COALESCE(SUM(error_count), 0) AS error_count_sum,
                COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count_sum,
                COALESCE(SUM(valuable_success_count), 0) AS valuable_success_count_sum,
                COALESCE(SUM(valuable_failure_count), 0) AS valuable_failure_count_sum,
                COALESCE(SUM(valuable_failure_429_count), 0) AS valuable_failure_429_count_sum,
                COALESCE(SUM(other_success_count), 0) AS other_success_count_sum,
                COALESCE(SUM(other_failure_count), 0) AS other_failure_count_sum,
                COALESCE(SUM(unknown_count), 0) AS unknown_count_sum,
                COALESCE(SUM(mcp_non_billable), 0) AS mcp_non_billable_sum,
                COALESCE(SUM(mcp_billable), 0) AS mcp_billable_sum,
                COALESCE(SUM(api_non_billable), 0) AS api_non_billable_sum,
                COALESCE(SUM(api_billable), 0) AS api_billable_sum,
                COALESCE(SUM(local_estimated_credits), 0) AS local_estimated_credits_sum
            FROM dashboard_request_rollup_buckets
            WHERE bucket_start >= ?
            "#,
        )
        .bind(range_start)
        .fetch_one(&self.pool)
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

    pub(crate) async fn fetch_latest_dashboard_quota_sync_sample_at(
        &self,
    ) -> Result<Option<i64>, ProxyError> {
        sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(captured_at) FROM api_key_quota_sync_samples",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn fetch_dashboard_rollup_freshness_signature(
        &self,
        range_start: i64,
    ) -> Result<[i64; 19], ProxyError> {
        self.flush_request_stats_writes().await?;
        self.fetch_dashboard_rollup_freshness_signature_without_flush(range_start)
            .await
    }

    pub(crate) async fn fetch_dashboard_api_key_lifecycle_signature(
        &self,
        range_start: i64,
    ) -> Result<[i64; 3], ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS key_count,
                COALESCE(MAX(created_at), 0) AS max_created_at,
                COALESCE(SUM(created_at), 0) AS created_at_sum
            FROM api_keys
            WHERE created_at >= ?
            "#,
        )
        .bind(range_start)
        .fetch_one(&self.pool)
        .await?;
        Ok([
            row.try_get("key_count")?,
            row.try_get("max_created_at")?,
            row.try_get("created_at_sum")?,
        ])
    }

    pub(crate) async fn fetch_dashboard_quarantine_lifecycle_signature(
        &self,
        range_start: i64,
    ) -> Result<[i64; 3], ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS quarantine_count,
                COALESCE(MAX(created_at), 0) AS max_created_at,
                COALESCE(SUM(created_at), 0) AS created_at_sum
            FROM api_key_quarantines
            WHERE created_at >= ?
            "#,
        )
        .bind(range_start)
        .fetch_one(&self.pool)
        .await?;
        Ok([
            row.try_get("quarantine_count")?,
            row.try_get("max_created_at")?,
            row.try_get("created_at_sum")?,
        ])
    }

    pub(crate) async fn fetch_dashboard_exhausted_lifecycle_signature(
        &self,
        range_start: i64,
        range_end: i64,
    ) -> Result<[i64; 3], ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(COUNT(*), 0) AS exhausted_count,
                COALESCE(MAX(created_at), 0) AS max_created_at,
                COALESCE(SUM(created_at), 0) AS created_at_sum
            FROM api_key_maintenance_records
            WHERE source = ?
              AND operation_code = ?
              AND reason_code = ?
              AND created_at >= ?
              AND created_at < ?
            "#,
        )
        .bind(MAINTENANCE_SOURCE_SYSTEM)
        .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
        .bind(OUTCOME_QUOTA_EXHAUSTED)
        .bind(range_start)
        .bind(range_end)
        .fetch_one(&self.pool)
        .await?;
        Ok([
            row.try_get("exhausted_count")?,
            row.try_get("max_created_at")?,
            row.try_get("created_at_sum")?,
        ])
    }

    pub(crate) async fn fetch_dashboard_quota_sample_signature(
        &self,
        window_start: i64,
        window_end: i64,
    ) -> Result<[i64; 4], ProxyError> {
        if window_end <= window_start {
            return Ok([0, 0, 0, 0]);
        }
        let row = sqlx::query(
            r#"
            WITH window_rows AS (
                SELECT key_id, quota_remaining, captured_at
                FROM api_key_quota_sync_samples
                WHERE captured_at >= ?
                  AND captured_at < ?
            ),
            sampled_keys AS (
                SELECT DISTINCT key_id FROM window_rows
            ),
            baseline_rows AS (
                SELECT s.key_id, s.quota_remaining, s.captured_at
                FROM api_key_quota_sync_samples s
                INNER JOIN (
                    SELECT key_id, MAX(captured_at) AS captured_at
                    FROM api_key_quota_sync_samples
                    WHERE captured_at < ?
                      AND key_id IN (SELECT key_id FROM sampled_keys)
                    GROUP BY key_id
                ) latest
                    ON latest.key_id = s.key_id
                   AND latest.captured_at = s.captured_at
            ),
            signature_rows AS (
                SELECT key_id, quota_remaining, captured_at
                FROM window_rows
                UNION ALL
                SELECT key_id, quota_remaining, captured_at
                FROM baseline_rows
            )
            SELECT
                COALESCE(COUNT(*), 0) AS sample_count,
                COALESCE(MAX(captured_at), 0) AS max_captured_at,
                COALESCE(SUM(captured_at), 0) AS captured_at_sum,
                COALESCE(SUM(quota_remaining), 0) AS remaining_sum
            FROM signature_rows
            "#,
        )
        .bind(window_start)
        .bind(window_end)
        .bind(window_start)
        .fetch_one(&self.pool)
        .await?;
        Ok([
            row.try_get("sample_count")?,
            row.try_get("max_captured_at")?,
            row.try_get("captured_at_sum")?,
            row.try_get("remaining_sum")?,
        ])
    }

    pub(crate) async fn fetch_dashboard_stale_key_count(
        &self,
        hot_active_since: i64,
        hot_stale_before: i64,
        cold_stale_before: i64,
    ) -> Result<i64, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT COALESCE(COUNT(*), 0)
            FROM api_keys
            WHERE deleted_at IS NULL
              AND status <> ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND CASE
                  WHEN last_used_at >= ? THEN (
                      quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
                  )
                  ELSE (
                      quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
                  )
              END
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(hot_active_since)
        .bind(hot_stale_before)
        .bind(cold_stale_before)
        .fetch_one(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn fetch_summary_windows(
        &self,
        bounds: SummaryWindowBounds,
    ) -> Result<SummaryWindows, ProxyError> {
        self.flush_request_stats_writes().await?;
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
        let mut tx = self.pool.begin().await?;
        let today_metrics = Self::fetch_dashboard_rollup_window_metrics_tx(
            &mut tx,
            SECS_PER_MINUTE,
            today_start,
            Some(today_end),
        )
        .await?;
        let yesterday_metrics = Self::fetch_dashboard_rollup_window_metrics_tx(
            &mut tx,
            SECS_PER_MINUTE,
            yesterday_start,
            Some(yesterday_end),
        )
        .await?;
        let month_metrics = Self::fetch_dashboard_rollup_month_metrics_tx(
            &mut tx,
            month_start,
            today_start,
            today_end,
        )
        .await?;
        let month_charge_metrics = Self::fetch_dashboard_rollup_month_metrics_tx(
            &mut tx,
            month_quota_charge_start,
            today_start,
            today_end,
        )
        .await?;

        let lifecycle_row = sqlx::query(
            r#"
            SELECT
                COUNT(DISTINCT CASE WHEN created_at >= ? AND created_at < ? THEN key_id END) AS today_upstream_exhausted_key_count,
                COUNT(DISTINCT CASE WHEN created_at >= ? AND created_at < ? THEN key_id END) AS yesterday_upstream_exhausted_key_count,
                COUNT(DISTINCT CASE WHEN created_at >= ? AND created_at < ? THEN key_id END) AS month_upstream_exhausted_key_count
            FROM api_key_maintenance_records
            WHERE source = ?
              AND operation_code = ?
              AND reason_code = ?
              AND created_at >= ?
              AND created_at < ?
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
        .fetch_one(&mut *tx)
        .await?;

        let month_lifecycle_row = sqlx::query(
            r#"
            SELECT
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM api_keys
                    WHERE created_at >= ?
                ) AS month_new_keys,
                (
                    SELECT COALESCE(COUNT(*), 0)
                    FROM api_key_quarantines
                    WHERE created_at >= ?
                ) AS month_new_quarantines
            "#,
        )
        .bind(month_start)
        .bind(month_start)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

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

    pub(crate) async fn fetch_dashboard_hourly_request_window(
        &self,
        current_bucket_start: i64,
        bucket_seconds: i64,
        visible_buckets: i64,
        retained_buckets: i64,
    ) -> Result<DashboardHourlyRequestWindow, ProxyError> {
        self.flush_request_stats_writes().await?;
        if bucket_seconds <= 0 || visible_buckets <= 0 || retained_buckets <= 0 {
            return Ok(DashboardHourlyRequestWindow {
                bucket_seconds,
                visible_buckets,
                retained_buckets,
                buckets: Vec::new(),
            });
        }

        let series_start = current_bucket_start
            .saturating_sub(bucket_seconds.saturating_mul(retained_buckets.saturating_sub(1)));
        let series_end_exclusive = current_bucket_start.saturating_add(bucket_seconds);
        let bucket_alignment_offset = current_bucket_start.rem_euclid(bucket_seconds);
        let rows = sqlx::query(
            r#"
            WITH RECURSIVE hour_series(bucket_start) AS (
                SELECT ? AS bucket_start
                UNION ALL
                SELECT bucket_start + ?
                FROM hour_series
                WHERE bucket_start + ? < ?
            ),
            aggregated AS (
                SELECT
                    ((bucket_start - ?) / ?) * ? + ? AS hour_bucket_start,
                    COALESCE(SUM(other_success_count), 0) AS secondary_success,
                    COALESCE(SUM(valuable_success_count), 0) AS primary_success,
                    COALESCE(SUM(other_failure_count), 0) AS secondary_failure,
                    COALESCE(SUM(valuable_failure_429_count), 0) AS primary_failure_429,
                    COALESCE(
                        SUM(
                            CASE
                                WHEN valuable_failure_count > valuable_failure_429_count
                                    THEN valuable_failure_count - valuable_failure_429_count
                                ELSE 0
                            END
                        ),
                        0
                    ) AS primary_failure_other,
                    COALESCE(SUM(unknown_count), 0) AS unknown_count,
                    COALESCE(SUM(mcp_non_billable), 0) AS mcp_non_billable,
                    COALESCE(SUM(mcp_billable), 0) AS mcp_billable,
                    COALESCE(SUM(api_non_billable), 0) AS api_non_billable,
                    COALESCE(SUM(api_billable), 0) AS api_billable
                FROM dashboard_request_rollup_buckets
                WHERE bucket_secs = ?
                  AND bucket_start >= ?
                  AND bucket_start < ?
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
                COALESCE(aggregated.api_billable, 0) AS api_billable
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
        .fetch_all(&self.pool)
        .await?;

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
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;

        Ok(DashboardHourlyRequestWindow {
            bucket_seconds,
            visible_buckets,
            retained_buckets,
            buckets,
        })
    }

    #[cfg(test)]
    pub(crate) async fn fetch_success_breakdown(
        &self,
        month_since: i64,
        day_start: i64,
        day_end: i64,
    ) -> Result<SuccessBreakdown, ProxyError> {
        let month_request_log_floor = self
            .fetch_visible_request_log_floor_since(month_since)
            .await?;
        let bucket_month_success = self
            .fetch_utc_month_gap_bucket_metrics(
                month_since,
                month_request_log_floor,
                Utc::now().timestamp(),
            )
            .await?
            .success_count;
        let scan_floor = month_since.min(day_start);
        let row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(CASE WHEN created_at >= ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS monthly_success,
              COALESCE(SUM(CASE WHEN created_at >= ? AND created_at < ? AND result_status = ? THEN 1 ELSE 0 END), 0) AS daily_success
            FROM observability.request_logs
            WHERE visibility = ?
              AND created_at >= ?
            "#,
        )
        .bind(month_since)
        .bind(OUTCOME_SUCCESS)
        .bind(day_start)
        .bind(day_end)
        .bind(OUTCOME_SUCCESS)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(scan_floor)
        .fetch_one(&self.pool)
        .await?;

        Ok(SuccessBreakdown {
            monthly_success: bucket_month_success + row.try_get::<i64, _>("monthly_success")?,
            daily_success: row.try_get("daily_success")?,
        })
    }

}

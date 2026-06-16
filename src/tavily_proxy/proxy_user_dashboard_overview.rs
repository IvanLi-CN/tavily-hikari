const USER_OVERVIEW_REQUEST_RATE_BUCKET_SECS: i64 = 5;

fn fallback_limit_values(
    limit_values: Vec<Option<i64>>,
    fallback_limit: i64,
) -> Vec<Option<i64>> {
    if limit_values.iter().any(Option::is_some) {
        limit_values
    } else {
        vec![Some(fallback_limit); limit_values.len()]
    }
}

fn build_user_overview_request_rate_points(
    bucket_starts: Vec<i64>,
    timestamps: &[i64],
    user_created_at: Option<i64>,
    now_ts: i64,
    limit_values: Vec<Option<i64>>,
) -> Vec<UserDashboardOverviewSeriesPoint> {
    let mut seen = 0usize;
    bucket_starts
        .iter()
        .copied()
        .enumerate()
        .map(|(index, bucket_start)| {
            let bucket_end = bucket_starts
                .get(index + 1)
                .copied()
                .unwrap_or(now_ts)
                .max(bucket_start);
            while seen < timestamps.len() && timestamps[seen] <= bucket_end {
                seen += 1;
            }
            let value = if user_created_at.is_some_and(|created_at| bucket_end < created_at) {
                None
            } else {
                Some(seen as i64)
            };
            UserDashboardOverviewSeriesPoint {
                bucket_start,
                display_bucket_start: None,
                value,
                limit_value: limit_values.get(index).copied().unwrap_or(None),
            }
        })
        .collect()
}

fn build_user_overview_current_period_points(
    bucket_starts: Vec<i64>,
    values: &HashMap<i64, i64>,
    available_bucket_start: i64,
    current_bucket_start: i64,
    bucket_start_before: i64,
    limit_values: Vec<Option<i64>>,
) -> Vec<UserDashboardOverviewSeriesPoint> {
    let mut running_total = 0_i64;
    bucket_starts
        .iter()
        .copied()
        .enumerate()
        .map(|(index, bucket_start)| {
            let value = if bucket_start < available_bucket_start
                || bucket_start >= bucket_start_before
                || bucket_start > current_bucket_start
            {
                None
            } else {
                running_total += values.get(&bucket_start).copied().unwrap_or(0);
                Some(running_total)
            };
            UserDashboardOverviewSeriesPoint {
                bucket_start,
                display_bucket_start: None,
                value,
                limit_value: limit_values.get(index).copied().unwrap_or(None),
            }
        })
        .collect()
}

impl TavilyProxy {
    pub async fn user_dashboard_overview(
        &self,
        user_id: &str,
        daily_window: Option<TimeRangeUtc>,
    ) -> Result<UserDashboardOverviewSnapshot, ProxyError> {
        let summary = self.user_dashboard_summary(user_id, daily_window).await?;
        let now = self.backend_time.now_utc();
        let now_ts = now.timestamp();
        let user_created_at = self.key_store.fetch_user_created_at(user_id).await?;

        let request_rate = self
            .build_user_dashboard_request_rate_progress(
                user_id,
                user_created_at,
                summary.request_rate.used,
                summary.request_rate.limit,
                now_ts,
            )
            .await?;
        let quota_hourly = self
            .build_user_dashboard_quota_hourly_progress(
                user_id,
                user_created_at,
                summary.quota_hourly_used,
                summary.quota_hourly_limit,
                now_ts,
            )
            .await?;
        let quota_daily = self
            .build_user_dashboard_quota_daily_progress(
                user_id,
                user_created_at,
                summary.quota_daily_used,
                summary.quota_daily_limit,
                now,
            )
            .await?;
        let quota_monthly = self
            .build_user_dashboard_quota_monthly_progress(
                user_id,
                user_created_at,
                summary.quota_monthly_used,
                summary.quota_monthly_limit,
                now,
            )
            .await?;

        Ok(UserDashboardOverviewSnapshot {
            summary,
            progress: UserDashboardOverviewProgress {
                request_rate,
                quota_hourly,
                quota_daily,
                quota_monthly,
            },
        })
    }

    async fn build_user_dashboard_request_rate_progress(
        &self,
        user_id: &str,
        user_created_at: Option<i64>,
        used: i64,
        limit: i64,
        now_ts: i64,
    ) -> Result<UserDashboardProgressCard, ProxyError> {
        let bucket_secs = USER_OVERVIEW_REQUEST_RATE_BUCKET_SECS.max(1);
        let bucket_count = (request_rate_limit_window_secs() / bucket_secs).max(1);
        let window_start = now_ts.saturating_sub(bucket_count * bucket_secs);
        let bucket_starts: Vec<i64> = (0..bucket_count)
            .map(|index| window_start + index * bucket_secs)
            .collect();
        let timestamps = self.user_request_rate_recent_timestamps(user_id).await;
        let limit_values = fallback_limit_values(
            resolve_bucket_limit_values(
                &bucket_starts,
                now_ts.saturating_add(1),
                &self
                    .key_store
                    .fetch_request_rate_limit_snapshots_for_window(window_start, now_ts.saturating_add(1))
                    .await?,
                |snapshot| snapshot.changed_at,
                |snapshot| snapshot.limit_value,
            ),
            limit,
        );

        Ok(UserDashboardProgressCard {
            used,
            limit,
            points: build_user_overview_request_rate_points(
                bucket_starts,
                &timestamps,
                user_created_at,
                now_ts,
                limit_values,
            ),
        })
    }

    async fn build_user_dashboard_quota_hourly_progress(
        &self,
        user_id: &str,
        user_created_at: Option<i64>,
        used: i64,
        limit: i64,
        now_ts: i64,
    ) -> Result<UserDashboardProgressCard, ProxyError> {
        let current_hour_start = now_ts - now_ts.rem_euclid(SECS_PER_HOUR);
        let current_bucket_start = now_ts - now_ts.rem_euclid(SECS_PER_FIVE_MINUTES);
        let bucket_start_before = current_hour_start + SECS_PER_HOUR;
        let bucket_starts: Vec<i64> = (0..(SECS_PER_HOUR / SECS_PER_FIVE_MINUTES))
            .map(|index| current_hour_start + index * SECS_PER_FIVE_MINUTES)
            .collect();
        let values = self
            .key_store
            .fetch_account_usage_rollup_values(
                user_id,
                AccountUsageRollupMetricKind::BusinessCredits,
                AccountUsageRollupBucketKind::FiveMinute,
                current_hour_start,
                bucket_start_before,
            )
            .await?;
        let limit_values = fallback_limit_values(
            resolve_bucket_limit_values(
                &bucket_starts,
                bucket_start_before,
                &self
                    .key_store
                    .fetch_account_quota_limit_snapshots_for_window(
                        user_id,
                        current_hour_start,
                        bucket_start_before,
                    )
                    .await?,
                |snapshot| snapshot.changed_at,
                |snapshot| snapshot.select(AccountQuotaLimitSnapshotField::Hourly),
            ),
            limit,
        );
        let available_bucket_start = user_created_at
            .map(|created_at| created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES))
            .unwrap_or(current_hour_start)
            .max(current_hour_start);

        Ok(UserDashboardProgressCard {
            used,
            limit,
            points: build_user_overview_current_period_points(
                bucket_starts,
                &values,
                available_bucket_start,
                current_bucket_start,
                bucket_start_before,
                limit_values,
            ),
        })
    }

    async fn build_user_dashboard_quota_daily_progress(
        &self,
        user_id: &str,
        user_created_at: Option<i64>,
        used: i64,
        limit: i64,
        now: chrono::DateTime<Utc>,
    ) -> Result<UserDashboardProgressCard, ProxyError> {
        let local_now = now.with_timezone(&Local);
        let day_window = server_local_day_window_utc(local_now);
        let current_bucket_start = start_of_local_hour_utc_ts(local_now);
        let mut bucket_starts = Vec::new();
        let mut cursor = day_window.start;
        while cursor < day_window.end {
            bucket_starts.push(cursor);
            cursor = cursor.saturating_add(SECS_PER_HOUR);
        }
        let values = self
            .key_store
            .fetch_account_usage_rollup_values(
                user_id,
                AccountUsageRollupMetricKind::BusinessCredits,
                AccountUsageRollupBucketKind::Hour,
                day_window.start,
                day_window.end,
            )
            .await?;
        let limit_values = fallback_limit_values(
            resolve_bucket_limit_values(
                &bucket_starts,
                day_window.end,
                &self
                    .key_store
                    .fetch_account_quota_limit_snapshots_for_window(
                        user_id,
                        day_window.start,
                        day_window.end,
                    )
                    .await?,
                |snapshot| snapshot.changed_at,
                |snapshot| snapshot.select(AccountQuotaLimitSnapshotField::Daily),
            ),
            limit,
        );
        let available_bucket_start = user_created_at
            .map(|created_at| created_at - created_at.rem_euclid(SECS_PER_HOUR))
            .unwrap_or(day_window.start)
            .max(day_window.start);

        Ok(UserDashboardProgressCard {
            used,
            limit,
            points: build_user_overview_current_period_points(
                bucket_starts,
                &values,
                available_bucket_start,
                current_bucket_start,
                day_window.end,
                limit_values,
            ),
        })
    }

    async fn build_user_dashboard_quota_monthly_progress(
        &self,
        user_id: &str,
        user_created_at: Option<i64>,
        used: i64,
        limit: i64,
        now: chrono::DateTime<Utc>,
    ) -> Result<UserDashboardProgressCard, ProxyError> {
        let month_start = start_of_month(now).timestamp();
        let month_end = shift_month_start_utc_ts(month_start, 1);
        let current_bucket_start = utc_day_bucket_start_utc_ts(now.timestamp());
        let mut bucket_starts = Vec::new();
        let mut cursor = month_start;
        while cursor < month_end {
            bucket_starts.push(cursor);
            cursor = cursor.saturating_add(SECS_PER_DAY);
        }
        let values = self
            .key_store
            .fetch_account_usage_rollup_values(
                user_id,
                AccountUsageRollupMetricKind::BusinessCredits,
                AccountUsageRollupBucketKind::UtcDay,
                month_start,
                month_end,
            )
            .await?;
        let limit_values = fallback_limit_values(
            resolve_bucket_limit_values(
                &bucket_starts,
                month_end,
                &self
                    .key_store
                    .fetch_account_quota_limit_snapshots_for_window(user_id, month_start, month_end)
                    .await?,
                |snapshot| snapshot.changed_at,
                |snapshot| snapshot.select(AccountQuotaLimitSnapshotField::Monthly),
            ),
            limit,
        );
        let available_bucket_start = user_created_at
            .map(utc_day_bucket_start_utc_ts)
            .unwrap_or(month_start)
            .max(month_start);

        Ok(UserDashboardProgressCard {
            used,
            limit,
            points: build_user_overview_current_period_points(
                bucket_starts,
                &values,
                available_bucket_start,
                current_bucket_start,
                month_end,
                limit_values,
            ),
        })
    }
}

fn resolve_bucket_limit_values<T>(
    bucket_starts: &[i64],
    bucket_start_before: i64,
    snapshots: &[T],
    snapshot_changed_at: impl Fn(&T) -> i64,
    snapshot_limit_value: impl Fn(&T) -> i64,
) -> Vec<Option<i64>> {
    if bucket_starts.is_empty() {
        return Vec::new();
    }

    let mut resolved = Vec::with_capacity(bucket_starts.len());
    let mut latest_limit = None;
    let mut snapshot_index = 0usize;

    for (index, bucket_start) in bucket_starts.iter().copied().enumerate() {
        let bucket_end = bucket_starts
            .get(index + 1)
            .copied()
            .unwrap_or(bucket_start_before)
            .max(bucket_start);
        while snapshot_index < snapshots.len()
            && snapshot_changed_at(&snapshots[snapshot_index]) < bucket_end
        {
            latest_limit = Some(snapshot_limit_value(&snapshots[snapshot_index]));
            snapshot_index += 1;
        }
        resolved.push(latest_limit);
    }

    resolved
}

fn build_admin_user_usage_series_points(
    series: AdminUserUsageSeriesKind,
    bucket_starts: Vec<i64>,
    _bucket_start_before: i64,
    values: &HashMap<i64, i64>,
    coverage_start: Option<i64>,
    user_created_at: Option<i64>,
    limit_values: Vec<Option<i64>>,
) -> Vec<AdminUserUsageSeriesPoint> {
    let coverage_floor = coverage_start
        .unwrap_or(i64::MIN)
        .max(user_created_at.unwrap_or(i64::MIN));
    bucket_starts
        .iter()
        .copied()
        .enumerate()
        .map(|(index, bucket_start)| {
            let value = values
                .get(&bucket_start)
                .copied()
                .map(Some)
                .unwrap_or_else(|| {
                    if bucket_start < coverage_floor {
                        None
                    } else {
                        Some(0)
                    }
                });
            let display_bucket_start = match series {
                AdminUserUsageSeriesKind::Quota24h => Local
                    .timestamp_opt(bucket_start, 0)
                    .single()
                    .map(|local_dt| local_dt.naive_local().and_utc().timestamp())
                    .or(Some(bucket_start)),
                AdminUserUsageSeriesKind::QuotaMonth => Some(bucket_start),
                AdminUserUsageSeriesKind::Rate5m | AdminUserUsageSeriesKind::Quota1h => None,
            };
            AdminUserUsageSeriesPoint {
                bucket_start,
                display_bucket_start,
                value,
                limit_value: limit_values.get(index).copied().unwrap_or(None),
            }
        })
        .collect()
}

impl TavilyProxy {
    pub async fn admin_user_usage_series(
        &self,
        user_id: &str,
        series: AdminUserUsageSeriesKind,
    ) -> Result<AdminUserUsageSeries, ProxyError> {
        let now = Utc::now();
        let (
            metric_kind,
            bucket_kind,
            bucket_starts,
            bucket_start_before,
            coverage_key,
            limit,
            quota_limit_field,
        ) = match series {
            AdminUserUsageSeriesKind::Rate5m => {
                let current_bucket_start =
                    now.timestamp() - now.timestamp().rem_euclid(SECS_PER_FIVE_MINUTES);
                let start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
                let bucket_starts = (0..288)
                    .map(|index| start + index * SECS_PER_FIVE_MINUTES)
                    .collect();
                (
                    AccountUsageRollupMetricKind::RequestCount,
                    AccountUsageRollupBucketKind::FiveMinute,
                    bucket_starts,
                    current_bucket_start + SECS_PER_FIVE_MINUTES,
                    META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START,
                    self.current_request_rate_limit(),
                    None,
                )
            }
            AdminUserUsageSeriesKind::Quota1h => {
                let current_bucket_start =
                    now.timestamp() - now.timestamp().rem_euclid(SECS_PER_HOUR);
                let start = current_bucket_start - 71 * SECS_PER_HOUR;
                let bucket_starts = (0..72)
                    .map(|index| start + index * SECS_PER_HOUR)
                    .collect();
                let limit = self
                    .key_store
                    .resolve_account_quota_resolution(user_id)
                    .await?
                    .effective
                    .hourly_limit;
                (
                    AccountUsageRollupMetricKind::BusinessCredits,
                    AccountUsageRollupBucketKind::Hour,
                    bucket_starts,
                    current_bucket_start + SECS_PER_HOUR,
                    META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START,
                    limit,
                    Some(AccountQuotaLimitSnapshotField::Hourly),
                )
            }
            AdminUserUsageSeriesKind::Quota24h => {
                let current_bucket_start =
                    server_local_day_window_utc(now.with_timezone(&Local)).start;
                let start = shift_local_day_start_utc_ts(current_bucket_start, -6);
                let mut bucket_starts = Vec::with_capacity(7);
                let mut cursor = start;
                for _ in 0..7 {
                    bucket_starts.push(cursor);
                    cursor = shift_local_day_start_utc_ts(cursor, 1);
                }
                let limit = self
                    .key_store
                    .resolve_account_quota_resolution(user_id)
                    .await?
                    .effective
                    .daily_limit;
                (
                    AccountUsageRollupMetricKind::BusinessCredits,
                    AccountUsageRollupBucketKind::Day,
                    bucket_starts,
                    cursor,
                    META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA24H_COVERAGE_START,
                    limit,
                    Some(AccountQuotaLimitSnapshotField::Daily),
                )
            }
            AdminUserUsageSeriesKind::QuotaMonth => {
                let current_bucket_start = start_of_month(now).timestamp();
                let start = shift_month_start_utc_ts(current_bucket_start, -11);
                let mut bucket_starts = Vec::with_capacity(12);
                let mut cursor = start;
                for _ in 0..12 {
                    bucket_starts.push(cursor);
                    cursor = shift_month_start_utc_ts(cursor, 1);
                }
                let limit = self
                    .key_store
                    .resolve_account_quota_resolution(user_id)
                    .await?
                    .effective
                    .monthly_limit;
                (
                    AccountUsageRollupMetricKind::BusinessCredits,
                    AccountUsageRollupBucketKind::Month,
                    bucket_starts,
                    cursor,
                    META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA_MONTH_COVERAGE_START,
                    limit,
                    Some(AccountQuotaLimitSnapshotField::Monthly),
                )
            }
        };

        let coverage_start = self.key_store.get_meta_i64(coverage_key).await?;
        let user_created_at = self.key_store.fetch_user_created_at(user_id).await?;
        let bucket_start_at_least = bucket_starts.first().copied().unwrap_or(bucket_start_before);
        let first_bucket_end = bucket_starts
            .get(1)
            .copied()
            .unwrap_or(bucket_start_before)
            .max(bucket_start_at_least);
        let values = self
            .key_store
            .fetch_account_usage_rollup_values(
                user_id,
                metric_kind,
                bucket_kind,
                bucket_start_at_least,
                bucket_start_before,
            )
            .await?;

        let limit_values = match quota_limit_field {
            None => resolve_bucket_limit_values(
                &bucket_starts,
                bucket_start_before,
                &self
                    .key_store
                    .fetch_request_rate_limit_snapshots_for_window(first_bucket_end, bucket_start_before)
                    .await?,
                |snapshot| snapshot.changed_at,
                |snapshot| snapshot.limit_value,
            ),
            Some(field) => resolve_bucket_limit_values(
                &bucket_starts,
                bucket_start_before,
                &self
                    .key_store
                    .fetch_account_quota_limit_snapshots_for_window(
                        user_id,
                        first_bucket_end,
                        bucket_start_before,
                    )
                    .await?,
                |snapshot| snapshot.changed_at,
                |snapshot| snapshot.select(field),
            ),
        };

        Ok(AdminUserUsageSeries {
            limit,
            points: build_admin_user_usage_series_points(
                series,
                bucket_starts,
                bucket_start_before,
                &values,
                coverage_start,
                user_created_at,
                limit_values,
            ),
        })
    }
}

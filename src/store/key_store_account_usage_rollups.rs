const ACCOUNT_USAGE_ROLLUP_METRIC_REQUEST_COUNT: &str = "request_count";
const ACCOUNT_USAGE_ROLLUP_METRIC_BUSINESS_CREDITS: &str = "business_credits";
const ACCOUNT_USAGE_ROLLUP_BUCKET_FIVE_MINUTE: &str = "five_minute";
const ACCOUNT_USAGE_ROLLUP_BUCKET_HOUR: &str = "hour";
const ACCOUNT_USAGE_ROLLUP_BUCKET_DAY: &str = "day";
const ACCOUNT_USAGE_ROLLUP_BUCKET_MONTH: &str = "month";
const ACCOUNT_USAGE_ROLLUP_INSERT_CHUNK_SIZE: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountUsageRollupMetricKind {
    RequestCount,
    BusinessCredits,
}

impl AccountUsageRollupMetricKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RequestCount => ACCOUNT_USAGE_ROLLUP_METRIC_REQUEST_COUNT,
            Self::BusinessCredits => ACCOUNT_USAGE_ROLLUP_METRIC_BUSINESS_CREDITS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccountUsageRollupBucketKind {
    FiveMinute,
    Hour,
    Day,
    Month,
}

impl AccountUsageRollupBucketKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::FiveMinute => ACCOUNT_USAGE_ROLLUP_BUCKET_FIVE_MINUTE,
            Self::Hour => ACCOUNT_USAGE_ROLLUP_BUCKET_HOUR,
            Self::Day => ACCOUNT_USAGE_ROLLUP_BUCKET_DAY,
            Self::Month => ACCOUNT_USAGE_ROLLUP_BUCKET_MONTH,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AccountUsageRollupRecord {
    pub(crate) user_id: String,
    pub(crate) bucket_start: i64,
    pub(crate) value: i64,
}

async fn upsert_account_usage_rollup_executor<'e, E>(
    executor: E,
    user_id: &str,
    metric_kind: AccountUsageRollupMetricKind,
    bucket_kind: AccountUsageRollupBucketKind,
    bucket_start: i64,
    delta: i64,
    updated_at: i64,
) -> Result<(), ProxyError>
where
    E: Executor<'e, Database = Sqlite>,
{
    if delta <= 0 {
        return Ok(());
    }

    sqlx::query(
        r#"
        INSERT INTO account_usage_rollup_buckets (
            user_id,
            metric_kind,
            bucket_kind,
            bucket_start,
            value,
            updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(user_id, metric_kind, bucket_kind, bucket_start)
        DO UPDATE SET
            value = account_usage_rollup_buckets.value + excluded.value,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(user_id)
    .bind(metric_kind.as_str())
    .bind(bucket_kind.as_str())
    .bind(bucket_start)
    .bind(delta)
    .bind(updated_at)
    .execute(executor)
    .await?;

    Ok(())
}

async fn replace_account_usage_rollup_records(
    tx: &mut Transaction<'_, Sqlite>,
    metric_kind: AccountUsageRollupMetricKind,
    bucket_kind: AccountUsageRollupBucketKind,
    records: &[AccountUsageRollupRecord],
    updated_at: i64,
) -> Result<(), ProxyError> {
    if records.is_empty() {
        return Ok(());
    }

    for chunk in records.chunks(ACCOUNT_USAGE_ROLLUP_INSERT_CHUNK_SIZE) {
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO account_usage_rollup_buckets (user_id, metric_kind, bucket_kind, bucket_start, value, updated_at) ",
        );
        query.push_values(chunk, |mut row, record| {
            row.push_bind(&record.user_id)
                .push_bind(metric_kind.as_str())
                .push_bind(bucket_kind.as_str())
                .push_bind(record.bucket_start)
                .push_bind(record.value)
                .push_bind(updated_at);
        });
        query.push(
            " ON CONFLICT(user_id, metric_kind, bucket_kind, bucket_start) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        );
        query.build().execute(&mut **tx).await?;
    }

    Ok(())
}

impl KeyStore {
    pub(crate) async fn record_account_request_rollup_for_user_id(
        &self,
        user_id: Option<&str>,
        created_at: i64,
    ) -> Result<(), ProxyError> {
        let Some(user_id) = user_id else {
            return Ok(());
        };
        let bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
        upsert_account_usage_rollup_executor(
            &self.pool,
            user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            1,
            created_at,
        )
        .await?;

        Ok(())
    }

    pub(crate) async fn record_account_business_credit_rollups(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        created_at: i64,
        credits: i64,
    ) -> Result<(), ProxyError> {
        if credits <= 0 {
            return Ok(());
        }

        let hour_bucket = created_at - created_at.rem_euclid(SECS_PER_HOUR);
        let day_bucket = local_day_bucket_start_utc_ts(created_at);
        let month_bucket = Utc
            .timestamp_opt(created_at, 0)
            .single()
            .map(start_of_month)
            .unwrap_or_else(Utc::now)
            .timestamp();

        for (bucket_kind, bucket_start) in [
            (AccountUsageRollupBucketKind::Hour, hour_bucket),
            (AccountUsageRollupBucketKind::Day, day_bucket),
            (AccountUsageRollupBucketKind::Month, month_bucket),
        ] {
            upsert_account_usage_rollup_executor(
                &mut **tx,
                user_id,
                AccountUsageRollupMetricKind::BusinessCredits,
                bucket_kind,
                bucket_start,
                credits,
                created_at,
            )
            .await?;
        }

        Ok(())
    }

    pub(crate) async fn fetch_account_usage_rollup_values(
        &self,
        user_id: &str,
        metric_kind: AccountUsageRollupMetricKind,
        bucket_kind: AccountUsageRollupBucketKind,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<HashMap<i64, i64>, ProxyError> {
        let rows = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT bucket_start, value
            FROM account_usage_rollup_buckets
            WHERE user_id = ?
              AND metric_kind = ?
              AND bucket_kind = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            ORDER BY bucket_start ASC
            "#,
        )
        .bind(user_id)
        .bind(metric_kind.as_str())
        .bind(bucket_kind.as_str())
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn delete_old_account_usage_rollup_buckets(
        &self,
        metric_kind: AccountUsageRollupMetricKind,
        bucket_kind: AccountUsageRollupBucketKind,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM account_usage_rollup_buckets
            WHERE metric_kind = ?
              AND bucket_kind = ?
              AND bucket_start < ?
            "#,
        )
        .bind(metric_kind.as_str())
        .bind(bucket_kind.as_str())
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn rebuild_account_usage_rollup_buckets_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now();
        let now_ts = now.timestamp();
        let request_backfill_start = now_ts.saturating_sub(ACCOUNT_USAGE_ROLLUP_REQUEST_BACKFILL_SECS);
        let business_backfill_start = now_ts.saturating_sub(ACCOUNT_USAGE_ROLLUP_BUSINESS_BACKFILL_SECS);
        let monthly_coverage_start =
            shift_month_start_utc_ts(start_of_month(now).timestamp(), -(ACCOUNT_USAGE_ROLLUP_MONTH_CHART_MONTHS - 1));

        let existing_quota1h_coverage = self
            .get_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START)
            .await?;
        let existing_quota24h_coverage = self
            .get_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA24H_COVERAGE_START)
            .await?;
        let existing_quota_month_coverage = self
            .get_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA_MONTH_COVERAGE_START)
            .await?;

        let request_rows = sqlx::query_as::<_, (String, i64, i64)>(
            r#"
            SELECT
                COALESCE(
                    l.request_user_id,
                    CASE
                        WHEN l.billing_subject LIKE 'account:%' THEN SUBSTR(l.billing_subject, 9)
                        ELSE NULL
                    END,
                    b.user_id
                ) AS user_id,
                (l.created_at / ?) * ? AS bucket_start,
                COUNT(*) AS total
            FROM auth_token_logs l
            LEFT JOIN user_token_bindings b ON b.token_id = l.token_id
            WHERE l.created_at >= ?
              AND COALESCE(
                    l.request_user_id,
                    CASE
                        WHEN l.billing_subject LIKE 'account:%' THEN SUBSTR(l.billing_subject, 9)
                        ELSE NULL
                    END,
                    b.user_id
                ) IS NOT NULL
            GROUP BY user_id, bucket_start
            ORDER BY user_id ASC, bucket_start ASC
            "#,
        )
        .bind(SECS_PER_FIVE_MINUTES)
        .bind(SECS_PER_FIVE_MINUTES)
        .bind(request_backfill_start)
        .fetch_all(&self.pool)
        .await?;
        let request_records: Vec<AccountUsageRollupRecord> = request_rows
            .into_iter()
            .map(|(user_id, bucket_start, value)| AccountUsageRollupRecord {
                user_id,
                bucket_start,
                value,
            })
            .collect();

        let business_rows = sqlx::query_as::<_, (String, i64, i64)>(
            r#"
            SELECT billing_subject, created_at, COALESCE(business_credits, 0) AS business_credits
            FROM auth_token_logs
            WHERE billing_state = ?
              AND COALESCE(business_credits, 0) > 0
              AND billing_subject LIKE 'account:%'
              AND created_at >= ?
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(BILLING_STATE_CHARGED)
        .bind(monthly_coverage_start.min(business_backfill_start))
        .fetch_all(&self.pool)
        .await?;

        let mut hourly_rollups: HashMap<(String, i64), i64> = HashMap::new();
        let mut daily_rollups: HashMap<(String, i64), i64> = HashMap::new();
        let mut monthly_rollups: HashMap<(String, i64), i64> = HashMap::new();

        for (billing_subject, created_at, credits) in business_rows {
            let Some(user_id) = billing_subject.strip_prefix("account:") else {
                continue;
            };
            if credits <= 0 {
                continue;
            }
            let user_id = user_id.to_string();
            if created_at >= business_backfill_start {
                let hour_bucket = created_at - created_at.rem_euclid(SECS_PER_HOUR);
                *hourly_rollups.entry((user_id.clone(), hour_bucket)).or_default() += credits;

                let day_bucket = local_day_bucket_start_utc_ts(created_at);
                *daily_rollups.entry((user_id.clone(), day_bucket)).or_default() += credits;
            }

            if created_at >= monthly_coverage_start {
                let month_bucket = Utc
                    .timestamp_opt(created_at, 0)
                    .single()
                    .map(start_of_month)
                    .unwrap_or_else(Utc::now)
                    .timestamp();
                *monthly_rollups.entry((user_id, month_bucket)).or_default() += credits;
            }
        }

        let mut hourly_records: Vec<AccountUsageRollupRecord> = hourly_rollups
            .into_iter()
            .map(|((user_id, bucket_start), value)| AccountUsageRollupRecord {
                user_id,
                bucket_start,
                value,
            })
            .collect();
        hourly_records.sort_by(|left, right| {
            left.user_id
                .cmp(&right.user_id)
                .then_with(|| left.bucket_start.cmp(&right.bucket_start))
        });

        let mut daily_records: Vec<AccountUsageRollupRecord> = daily_rollups
            .into_iter()
            .map(|((user_id, bucket_start), value)| AccountUsageRollupRecord {
                user_id,
                bucket_start,
                value,
            })
            .collect();
        daily_records.sort_by(|left, right| {
            left.user_id
                .cmp(&right.user_id)
                .then_with(|| left.bucket_start.cmp(&right.bucket_start))
        });

        let mut monthly_records: Vec<AccountUsageRollupRecord> = monthly_rollups
            .into_iter()
            .map(|((user_id, bucket_start), value)| AccountUsageRollupRecord {
                user_id,
                bucket_start,
                value,
            })
            .collect();
        monthly_records.sort_by(|left, right| {
            left.user_id
                .cmp(&right.user_id)
                .then_with(|| left.bucket_start.cmp(&right.bucket_start))
        });

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            DELETE FROM account_usage_rollup_buckets
            WHERE metric_kind = ?
              AND bucket_kind = ?
              AND bucket_start >= ?
            "#,
        )
        .bind(AccountUsageRollupMetricKind::RequestCount.as_str())
        .bind(AccountUsageRollupBucketKind::FiveMinute.as_str())
        .bind(request_backfill_start)
        .execute(&mut *tx)
        .await?;

        for bucket_kind in [AccountUsageRollupBucketKind::Hour, AccountUsageRollupBucketKind::Day] {
            sqlx::query(
                r#"
                DELETE FROM account_usage_rollup_buckets
                WHERE metric_kind = ?
                  AND bucket_kind = ?
                  AND bucket_start >= ?
                "#,
            )
            .bind(AccountUsageRollupMetricKind::BusinessCredits.as_str())
            .bind(bucket_kind.as_str())
            .bind(business_backfill_start)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            r#"
            DELETE FROM account_usage_rollup_buckets
            WHERE metric_kind = ?
              AND bucket_kind = ?
              AND bucket_start >= ?
            "#,
        )
        .bind(AccountUsageRollupMetricKind::BusinessCredits.as_str())
        .bind(AccountUsageRollupBucketKind::Month.as_str())
        .bind(monthly_coverage_start)
        .execute(&mut *tx)
        .await?;

        replace_account_usage_rollup_records(
            &mut tx,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            &request_records,
            now_ts,
        )
        .await?;
        replace_account_usage_rollup_records(
            &mut tx,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::Hour,
            &hourly_records,
            now_ts,
        )
        .await?;
        replace_account_usage_rollup_records(
            &mut tx,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::Day,
            &daily_records,
            now_ts,
        )
        .await?;
        replace_account_usage_rollup_records(
            &mut tx,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::Month,
            &monthly_records,
            now_ts,
        )
        .await?;

        let rate5m_coverage = request_backfill_start;
        let quota1h_coverage = existing_quota1h_coverage
            .map(|value| value.min(business_backfill_start))
            .unwrap_or(business_backfill_start);
        let quota24h_coverage = existing_quota24h_coverage
            .map(|value| value.min(business_backfill_start))
            .unwrap_or(business_backfill_start);
        let quota_month_coverage = existing_quota_month_coverage
            .map(|value| value.min(monthly_coverage_start))
            .unwrap_or(monthly_coverage_start);

        set_meta_i64_executor(&mut *tx, META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START, rate5m_coverage).await?;
        set_meta_i64_executor(&mut *tx, META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START, quota1h_coverage).await?;
        set_meta_i64_executor(&mut *tx, META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA24H_COVERAGE_START, quota24h_coverage).await?;
        set_meta_i64_executor(&mut *tx, META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA_MONTH_COVERAGE_START, quota_month_coverage).await?;
        set_meta_i64_executor(&mut *tx, META_KEY_ACCOUNT_USAGE_ROLLUP_V1_DONE, now_ts).await?;

        tx.commit().await?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestStatsReadFreshness {
    Fresh,
    DurableFallback,
}

impl KeyStore {
    pub(crate) async fn flush_request_stats_writes(&self) -> Result<(), ProxyError> {
        self.flush_request_stats_writes_with_wait_policy(&self.pool, Duration::from_secs(10), None)
            .await
    }

    pub(crate) async fn best_effort_flush_request_stats_writes_for_read(
        &self,
        read_operation: &'static str,
    ) -> Result<RequestStatsReadFreshness, ProxyError> {
        const RETRY_BUDGET: Duration = Duration::from_millis(250);
        let inflight_wait_deadline = self.backend_time.instant_now() + RETRY_BUDGET;
        match self
            .flush_request_stats_writes_with_wait_policy(
                &self.read_flush_pool,
                RETRY_BUDGET,
                Some(inflight_wait_deadline),
            )
            .await
        {
            Ok(()) => Ok(RequestStatsReadFreshness::Fresh),
            Err(err)
                if is_transient_sqlite_write_error(&err)
                    || is_request_stats_flush_wait_budget_exhausted(&err) =>
            {
                warn!(
                    component = "admin_read",
                    operation = read_operation,
                    retry_budget_ms = RETRY_BUDGET.as_millis() as u64,
                    error = %err,
                    "serving durable stats without flushing pending request stats"
                );
                Ok(RequestStatsReadFreshness::DurableFallback)
            }
            Err(err) => Err(err),
        }
    }

    async fn flush_request_stats_writes_with_wait_policy(
        &self,
        pool: &SqlitePool,
        retry_budget: Duration,
        inflight_wait_deadline: Option<Instant>,
    ) -> Result<(), ProxyError> {
        loop {
            let pending = {
                let mut state = self.request_stats_coalescer.state.lock().await;
                if state.flushing {
                    None
                } else if state.pending_dashboard_rollups.is_empty()
                    && state.pending_api_key_usage.is_empty()
                    && state.pending_auth_token_activity.is_empty()
                    && state.pending_account_request_rollups.is_empty()
                    && state.pending_request_log_catalog.is_empty()
                {
                    return Ok(());
                } else {
                    state.flushing = true;
                    state.flushing_oldest_created_at = state.oldest_pending_created_at.take();
                    state.flushing_newest_created_at = state.newest_pending_created_at.take();
                    Some((
                        std::mem::take(&mut state.pending_dashboard_rollups),
                        std::mem::take(&mut state.pending_api_key_usage),
                        std::mem::take(&mut state.pending_auth_token_activity),
                        std::mem::take(&mut state.pending_account_request_rollups),
                        std::mem::take(&mut state.pending_request_log_catalog),
                        state.flushing_oldest_created_at,
                        state.flushing_newest_created_at,
                    ))
                }
            };
            let Some((
                pending_dashboard_rollups,
                pending_api_key_usage,
                pending_auth_token_activity,
                pending_account_request_rollups,
                pending_request_log_catalog,
                drained_oldest_pending_created_at,
                drained_newest_pending_created_at,
            )) = pending
            else {
                if let Some(deadline) = inflight_wait_deadline {
                    let remaining = deadline.saturating_duration_since(self.backend_time.instant_now());
                    if remaining.is_zero() {
                        return Err(request_stats_flush_wait_budget_exhausted_error());
                    }
                    if tokio::time::timeout(
                        remaining,
                        self.request_stats_coalescer.wait_until_not_flushing(),
                    )
                    .await
                    .is_err()
                    {
                        return Err(request_stats_flush_wait_budget_exhausted_error());
                    }
                } else {
                    self.request_stats_coalescer.wait_until_not_flushing().await;
                }
                continue;
            };

            let pending_batch_counts = format!(
                "dashboard={},api_key={},auth_token={},account_rollup={},request_catalog={}",
                pending_dashboard_rollups.len(),
                pending_api_key_usage.len(),
                pending_auth_token_activity.len(),
                pending_account_request_rollups.len(),
                pending_request_log_catalog.len(),
            );
            let log_fields = SqliteContentionLogFields {
                operation: "flush_request_stats_writes",
                request_path: "/internal/request-stats-flush",
                request_kind: "internal:request-stats-flush",
                billing_subject_kind: "unknown",
                retry_budget_ms: retry_budget.as_millis() as u64,
                pending_batch_counts: pending_batch_counts.as_str(),
                oldest_pending_created_at: drained_oldest_pending_created_at,
                newest_pending_created_at: drained_newest_pending_created_at,
            };
            let retry_deadline = inflight_wait_deadline
                .unwrap_or_else(|| self.backend_time.instant_now() + retry_budget);
            let operation_started = Instant::now();
            let mut retry_attempt = 0usize;
            let result = loop {
                match self
                    .flush_request_stats_writes_once(
                        pool,
                        &pending_dashboard_rollups,
                        &pending_api_key_usage,
                        &pending_auth_token_activity,
                        &pending_account_request_rollups,
                        &pending_request_log_catalog,
                    )
                    .await
                {
                    Ok(()) => break Ok(()),
                    Err(err) => {
                        if !is_transient_sqlite_write_error(&err) {
                            break Err(err);
                        }
                        let now = self.backend_time.instant_now();
                        if now >= retry_deadline {
                            log_sqlite_transient_write_exhaustion_with_fields(
                                log_fields,
                                retry_attempt + 1,
                                operation_started.elapsed(),
                                &err,
                            );
                            break Err(err);
                        }
                        let remaining = retry_deadline.saturating_duration_since(now);
                        let backoff = sqlite_transient_write_retry_delay(retry_attempt).min(remaining);
                        log_sqlite_transient_write_retry_with_fields(
                            log_fields,
                            retry_attempt + 1,
                            backoff,
                            operation_started.elapsed(),
                            &err,
                        );
                        self.backend_time.sleep(backoff).await;
                        retry_attempt += 1;
                    }
                }
            };

            {
                let mut state = self.request_stats_coalescer.state.lock().await;
                state.flushing = false;
                state.flush_deadline = None;
                if let Err(err) = result {
                    state.flushing_oldest_created_at = None;
                    state.flushing_newest_created_at = None;
                    for (key, counts) in pending_dashboard_rollups {
                        state.pending_dashboard_rollups.entry(key).or_default().add(counts);
                    }
                    for (key, delta) in pending_api_key_usage {
                        state.pending_api_key_usage.entry(key).or_default().add(delta);
                    }
                    for (token_id, delta) in pending_auth_token_activity {
                        state
                            .pending_auth_token_activity
                            .entry(token_id)
                            .or_default()
                            .add(delta);
                    }
                    for (key, delta) in pending_account_request_rollups {
                        state
                            .pending_account_request_rollups
                            .entry(key)
                            .or_default()
                            .add(delta);
                    }
                    for (key, delta) in pending_request_log_catalog {
                        *state.pending_request_log_catalog.entry(key).or_default() += delta;
                    }
                    if let Some(created_at) = drained_oldest_pending_created_at {
                        state.oldest_pending_created_at = Some(
                            state
                                .oldest_pending_created_at
                                .map(|current| current.min(created_at))
                                .unwrap_or(created_at),
                        );
                    }
                    if let Some(created_at) = drained_newest_pending_created_at {
                        state.newest_pending_created_at = Some(
                            state
                                .newest_pending_created_at
                                .map(|current| current.max(created_at))
                                .unwrap_or(created_at),
                        );
                    }
                    RequestStatsCoalescer::mark_flush_deadline_if_pending(&mut state);
                    self.request_stats_coalescer.flushed.notify_waiters();
                    return Err(err);
                }
                state.flushing_oldest_created_at = None;
                state.flushing_newest_created_at = None;
                if RequestStatsCoalescer::pending_key_count(&state) == 0 {
                    state.oldest_pending_created_at = None;
                    state.newest_pending_created_at = None;
                } else {
                    RequestStatsCoalescer::mark_flush_deadline_if_pending(&mut state);
                }
                self.request_stats_coalescer.flushed.notify_waiters();
            }
            #[cfg(test)]
            self.request_stats_coalescer
                .wait_for_post_flush_pause_if_installed()
                .await;
        }
    }

    async fn flush_request_stats_writes_once(
        &self,
        pool: &SqlitePool,
        pending_dashboard_rollups: &HashMap<(i64, i64), DashboardRequestRollupCounts>,
        pending_api_key_usage: &HashMap<(String, i64), ApiKeyUsageBucketDelta>,
        pending_auth_token_activity: &HashMap<String, AuthTokenActivityDelta>,
        pending_account_request_rollups: &HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
        pending_request_log_catalog: &HashMap<RequestLogCatalogRollupKey, i64>,
    ) -> Result<(), ProxyError> {
        let updated_at = Utc::now().timestamp();
        let mut tx = pool.begin().await?;
        let mut dashboard_entries = pending_dashboard_rollups
            .iter()
            .map(|(key, counts)| (*key, *counts))
            .collect::<Vec<_>>();
        dashboard_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for ((bucket_start, bucket_secs), counts) in dashboard_entries {
            Self::upsert_dashboard_request_rollup_bucket(
                &mut tx,
                bucket_start,
                bucket_secs,
                counts,
                updated_at,
            )
            .await?;
        }

        let mut api_key_usage_entries = pending_api_key_usage
            .iter()
            .map(|(key, delta)| (key.clone(), *delta))
            .collect::<Vec<_>>();
        api_key_usage_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for ((key_id, bucket_start), delta) in api_key_usage_entries {
            Self::upsert_api_key_usage_bucket_delta(
                &mut tx,
                &key_id,
                bucket_start,
                delta,
                updated_at,
            )
            .await?;
        }

        let mut auth_token_activity_entries = pending_auth_token_activity
            .iter()
            .map(|(token_id, delta)| (token_id.clone(), delta.clone()))
            .collect::<Vec<_>>();
        auth_token_activity_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for (token_id, delta) in auth_token_activity_entries {
            Self::upsert_auth_token_activity_delta(&mut tx, &token_id, delta).await?;
        }

        let mut account_request_rollup_entries = pending_account_request_rollups
            .iter()
            .map(|(key, delta)| (key.clone(), *delta))
            .collect::<Vec<_>>();
        account_request_rollup_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for (key, delta) in account_request_rollup_entries {
            let user_id = key.user_id;
            let bucket_start = key.five_minute_bucket_start;
            let day_bucket_start = key.day_bucket_start;
            if delta.request_count > 0 {
                for (bucket_kind, rollup_bucket_start) in [
                    (AccountUsageRollupBucketKind::FiveMinute, bucket_start),
                    (AccountUsageRollupBucketKind::Day, day_bucket_start),
                ] {
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
                    .bind(&user_id)
                    .bind(AccountUsageRollupMetricKind::RequestCount.as_str())
                    .bind(bucket_kind.as_str())
                    .bind(rollup_bucket_start)
                    .bind(delta.request_count)
                    .bind(updated_at)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            if delta.primary_success > 0 {
                for (bucket_kind, rollup_bucket_start) in [
                    (AccountUsageRollupBucketKind::FiveMinute, bucket_start),
                    (AccountUsageRollupBucketKind::Day, day_bucket_start),
                ] {
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
                    .bind(&user_id)
                    .bind(AccountUsageRollupMetricKind::PrimarySuccess.as_str())
                    .bind(bucket_kind.as_str())
                    .bind(rollup_bucket_start)
                    .bind(delta.primary_success)
                    .bind(updated_at)
                    .execute(&mut *tx)
                    .await?;
                }
            }
            if delta.secondary_success > 0 {
                for (bucket_kind, rollup_bucket_start) in [
                    (AccountUsageRollupBucketKind::FiveMinute, bucket_start),
                    (AccountUsageRollupBucketKind::Day, day_bucket_start),
                ] {
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
                    .bind(&user_id)
                    .bind(AccountUsageRollupMetricKind::SecondarySuccess.as_str())
                    .bind(bucket_kind.as_str())
                    .bind(rollup_bucket_start)
                    .bind(delta.secondary_success)
                    .bind(updated_at)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }

        let mut request_log_catalog_entries = pending_request_log_catalog
            .iter()
            .map(|(key, delta)| (key.clone(), *delta))
            .collect::<Vec<_>>();
        request_log_catalog_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for (key, delta) in request_log_catalog_entries {
            Self::upsert_request_log_catalog_rollup_delta(&mut tx, &key, delta, updated_at).await?;
        }

        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1)
        .bind(updated_at.to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn fetch_api_key_usage_bucket_success_count(
        &self,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<i64, ProxyError> {
        if let Some(bucket_start_before) = bucket_start_before {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COALESCE(SUM(success_count), 0)
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                  AND bucket_start >= ?
                  AND bucket_start < ?
                "#,
            )
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::Database)
        } else {
            sqlx::query_scalar::<_, i64>(
                r#"
                SELECT COALESCE(SUM(success_count), 0)
                FROM api_key_usage_buckets
                WHERE bucket_secs = 86400
                  AND bucket_start >= ?
                "#,
            )
            .bind(bucket_start_at_least)
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::Database)
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    async fn fetch_utc_month_gap_bucket_metrics(
        &self,
        month_start: i64,
        month_request_log_floor: Option<i64>,
        gap_fallback_end: i64,
    ) -> Result<SummaryWindowMetrics, ProxyError> {
        let gap_end = match month_request_log_floor {
            Some(floor) if floor > month_start => floor,
            Some(_) => return Ok(SummaryWindowMetrics::default()),
            None => gap_fallback_end,
        };
        if gap_end <= month_start {
            return Ok(SummaryWindowMetrics::default());
        }

        let first_bucket_start = local_day_bucket_start_utc_ts(month_start);
        let first_exact_bucket_start = if first_bucket_start == month_start {
            month_start
        } else {
            next_local_day_start_utc_ts(first_bucket_start)
        };
        let last_gap_bucket_start = local_day_bucket_start_utc_ts(gap_end);

        let mut backfill = SummaryWindowMetrics::default();
        if first_exact_bucket_start < last_gap_bucket_start {
            add_summary_window_metrics(
                &mut backfill,
                &self
                    .fetch_api_key_usage_bucket_window_metrics(
                        first_exact_bucket_start,
                        Some(last_gap_bucket_start),
                    )
                    .await?,
            );
        }

        if gap_end > last_gap_bucket_start && last_gap_bucket_start >= month_start {
            let last_gap_bucket_end = next_local_day_start_utc_ts(last_gap_bucket_start);
            let full_day_bucket = self
                .fetch_api_key_usage_bucket_window_metrics(
                    last_gap_bucket_start,
                    Some(last_gap_bucket_end),
                )
                .await?;
            let retained_tail = self
                .fetch_visible_request_log_window_metrics(gap_end, last_gap_bucket_end)
                .await?;
            add_summary_window_metrics(
                &mut backfill,
                &subtract_summary_window_metrics(&full_day_bucket, &retained_tail),
            );
        }

        Ok(backfill)
    }

    async fn fetch_utc_month_gap_success_count(
        &self,
        month_start: i64,
        month_request_log_floor: Option<i64>,
        gap_fallback_end: i64,
    ) -> Result<i64, ProxyError> {
        let gap_end = match month_request_log_floor {
            Some(floor) if floor > month_start => floor,
            Some(_) => return Ok(0),
            None => gap_fallback_end,
        };
        if gap_end <= month_start {
            return Ok(0);
        }

        let first_bucket_start = local_day_bucket_start_utc_ts(month_start);
        let first_exact_bucket_start = if first_bucket_start == month_start {
            month_start
        } else {
            next_local_day_start_utc_ts(first_bucket_start)
        };
        let last_gap_bucket_start = local_day_bucket_start_utc_ts(gap_end);
        let mut success_count = 0;

        if first_exact_bucket_start < last_gap_bucket_start {
            success_count += self
                .fetch_api_key_usage_bucket_success_count(
                    first_exact_bucket_start,
                    Some(last_gap_bucket_start),
                )
                .await?;
        }

        if gap_end > last_gap_bucket_start && last_gap_bucket_start >= month_start {
            let last_gap_bucket_end = next_local_day_start_utc_ts(last_gap_bucket_start);
            let full_day_success = self
                .fetch_api_key_usage_bucket_success_count(
                    last_gap_bucket_start,
                    Some(last_gap_bucket_end),
                )
                .await?;
            let mut tx = self.pool.begin().await?;
            let retained_tail_success = Self::fetch_visible_request_log_success_count_tx(
                &mut tx,
                gap_end,
                last_gap_bucket_end,
            )
            .await?;
            tx.commit().await?;
            success_count += subtract_nonnegative(full_day_success, retained_tail_success);
        }

        Ok(success_count)
    }
}

fn request_stats_flush_wait_budget_exhausted_error() -> ProxyError {
    ProxyError::Other("request stats flush wait budget exhausted".to_string())
}

fn is_request_stats_flush_wait_budget_exhausted(err: &ProxyError) -> bool {
    matches!(err, ProxyError::Other(message) if message == "request stats flush wait budget exhausted")
}

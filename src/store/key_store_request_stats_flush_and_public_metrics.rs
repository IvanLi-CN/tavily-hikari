#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestStatsReadFreshness {
    Fresh,
    DurableFallback,
}

const REQUEST_STATS_FLUSH_BATCH_MARKER_RETENTION_SECS: i64 = 7 * 24 * 60 * 60;

impl KeyStore {
    pub(crate) async fn flush_request_stats_writes(&self) -> Result<(), ProxyError> {
        self.request_stats_pipeline.flush_request_stats_writes().await
    }

    #[cfg(test)]
    pub(crate) async fn flush_request_stats_writes_with_wait_policy_for_test(
        &self,
        retry_budget: Duration,
        inflight_wait_deadline: Option<Instant>,
    ) -> Result<(), ProxyError> {
        self.request_stats_pipeline
            .flush_request_stats_writes_with_wait_policy_for_test(
                retry_budget,
                inflight_wait_deadline,
            )
            .await
    }

    pub(crate) async fn best_effort_flush_request_stats_writes_for_read(
        &self,
        read_operation: &'static str,
    ) -> Result<RequestStatsReadFreshness, ProxyError> {
        const RETRY_BUDGET: Duration = Duration::from_millis(250);
        let inflight_wait_deadline = self.request_stats_pipeline.backend_time().instant_now()
            + RETRY_BUDGET;
        match self
            .request_stats_pipeline
            .flush_request_stats_writes_with_wait_policy(
                true,
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

    pub(crate) async fn best_effort_flush_request_stats_writes_for_maintenance(
        &self,
        operation: &'static str,
    ) -> Result<RequestStatsReadFreshness, ProxyError> {
        const RETRY_BUDGET: Duration = Duration::from_millis(100);
        let inflight_wait_deadline = self.request_stats_pipeline.backend_time().instant_now()
            + RETRY_BUDGET;
        match self
            .request_stats_pipeline
            .flush_request_stats_writes_with_wait_policy(
                true,
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
                tracing::debug!(
                    component = "dashboard_rollup_integrity",
                    operation,
                    retry_budget_ms = RETRY_BUDGET.as_millis() as u64,
                    error = %err,
                    "deferring integrity work until request statistics are durable"
                );
                Ok(RequestStatsReadFreshness::DurableFallback)
            }
            Err(err) => Err(err),
        }
    }

}

async fn flush_request_stats_writes_with_wait_policy(
    request_stats_pipeline: &RequestStatsPipeline,
    use_read_flush_pool: bool,
    retry_budget: Duration,
    inflight_wait_deadline: Option<Instant>,
) -> Result<(), ProxyError> {
    loop {
            let pending = match request_stats_pipeline.claim_flush_batch().await {
                RequestStatsFlushClaim::Empty => return Ok(()),
                RequestStatsFlushClaim::InFlight => None,
                RequestStatsFlushClaim::Batch(batch) => Some(RequestStatsFlushBatchGuard::new(
                    request_stats_pipeline.clone(),
                    *batch,
                )),
            };
            let Some(drained) = pending else {
                if let Some(deadline) = inflight_wait_deadline {
                    let remaining = deadline.saturating_duration_since(
                        request_stats_pipeline.backend_time().instant_now(),
                    );
                    if remaining.is_zero() {
                        return Err(request_stats_flush_wait_budget_exhausted_error());
                    }
                    if tokio::time::timeout(
                        remaining,
                        request_stats_pipeline.wait_until_not_flushing(),
                    )
                    .await
                    .is_err()
                    {
                        return Err(request_stats_flush_wait_budget_exhausted_error());
                    }
                } else {
                    request_stats_pipeline.wait_until_not_flushing().await;
                }
                continue;
            };

            let flush_task = RequestStatsPipeline::spawn_request_stats_flush_drained_batch_task(
                request_stats_pipeline.clone(),
                use_read_flush_pool,
                retry_budget,
                drained,
            );
            if let Some(deadline) = inflight_wait_deadline {
                let remaining = deadline.saturating_duration_since(
                        request_stats_pipeline.backend_time().instant_now(),
                );
                if remaining.is_zero() {
                    std::mem::drop(flush_task);
                    return Err(request_stats_flush_wait_budget_exhausted_error());
                }
                match tokio::time::timeout(remaining, flush_task).await {
                    Ok(Ok(Ok(()))) => continue,
                    Ok(Ok(Err(err))) => return Err(err),
                    Ok(Err(err)) => {
                        return Err(ProxyError::Other(format!(
                        "request stats flush task failed: {err}"
                    )))
                    }
                    Err(_) => return Err(request_stats_flush_wait_budget_exhausted_error()),
                }
            } else {
                match flush_task.await {
                    Ok(Ok(())) => continue,
                    Ok(Err(err)) => return Err(err),
                    Err(err) => {
                        return Err(ProxyError::Other(format!(
                            "request stats flush task failed: {err}"
                        )))
                    }
                }
            }
        }
    }

impl RequestStatsPipeline {
    fn spawn_request_stats_flush_drained_batch_task(
        request_stats_pipeline: RequestStatsPipeline,
        use_read_flush_pool: bool,
        retry_budget: Duration,
        drained: RequestStatsFlushBatchGuard,
    ) -> tokio::task::JoinHandle<Result<(), ProxyError>> {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        // The persistence task owns the transaction and batch. The returned task only waits for
        // its result, so a read deadline or caller cancellation cannot requeue a batch after an
        // acknowledged-or-unacknowledged SQLite COMMIT.
        tokio::spawn(async move {
            let result = Self::flush_request_stats_writes_drained_batch(
                request_stats_pipeline,
                use_read_flush_pool,
                retry_budget,
                drained,
            )
            .await;
            let _ = result_tx.send(result);
        });
        tokio::spawn(async move {
            result_rx.await.map_err(|_| {
                ProxyError::Other("request stats persistence task stopped unexpectedly".to_string())
            })?
        })
    }

    async fn flush_request_stats_writes_drained_batch(
        request_stats_pipeline: RequestStatsPipeline,
        use_read_flush_pool: bool,
        retry_budget: Duration,
        mut drained: RequestStatsFlushBatchGuard,
    ) -> Result<(), ProxyError> {
        let (pending_batch_counts, oldest_pending_created_at, newest_pending_created_at) = {
            let batch = drained.batch();
            (
                format!(
                    "dashboard={},api_key={},auth_token={},account_rollup={},request_catalog={}",
                    batch.pending_dashboard_rollups.len(),
                    batch.pending_api_key_usage.len(),
                    batch.pending_auth_token_activity.len(),
                    batch.pending_account_request_rollups.len(),
                    batch.pending_request_log_catalog.len(),
                ),
                batch.drained_oldest_pending_created_at,
                batch.drained_newest_pending_created_at,
            )
        };
        let log_fields = SqliteContentionLogFields {
            operation: "flush_request_stats_writes",
            request_path: "/internal/request-stats-flush",
            request_kind: "internal:request-stats-flush",
            billing_subject_kind: "unknown",
            retry_budget_ms: retry_budget.as_millis() as u64,
            pending_batch_counts: pending_batch_counts.as_str(),
            oldest_pending_created_at,
            newest_pending_created_at,
        };
        let retry_deadline = request_stats_pipeline.backend_time().instant_now() + retry_budget;
        let operation_started = Instant::now();
        let mut retry_attempt = 0usize;
        let flush_result = loop {
            let batch = drained.batch().clone();
            let flush_result = Self::persist_request_stats_flush_batch(
                &request_stats_pipeline,
                use_read_flush_pool,
                &batch,
            )
            .await;
            match flush_result {
                Ok(()) => break Ok(()),
                Err(err) => {
                    if !is_transient_sqlite_write_error(&err) {
                        break Err(err);
                    }
                    let now = request_stats_pipeline.backend_time().instant_now();
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
                    request_stats_pipeline.backend_time().sleep(backoff).await;
                    retry_attempt += 1;
                }
            }
        };

        request_stats_pipeline.finish_flush_batch(&mut drained, flush_result)?;
        #[cfg(test)]
        request_stats_pipeline
            .wait_for_post_flush_pause_if_installed()
            .await;
        Ok(())
    }

    async fn persist_request_stats_flush_batch(
        request_stats_pipeline: &RequestStatsPipeline,
        use_read_flush_pool: bool,
        batch: &RequestStatsFlushBatch,
    ) -> Result<(), ProxyError> {
        let mut tx = if use_read_flush_pool {
            request_stats_pipeline.begin_read_flush_transaction().await?
        } else {
            request_stats_pipeline.begin_primary_transaction().await?
        };
        SqliteRequestStatsTransaction::run_bounded_operation(tx.operation_budget(), async {
            let applied_at = request_stats_pipeline.backend_time().now_ts();
            let marker_inserted = sqlx::query(
                r#"
                INSERT INTO request_stats_flush_batches (batch_id, applied_at)
                VALUES (?, ?)
                ON CONFLICT(batch_id) DO NOTHING
                "#,
            )
            .bind(&batch.batch_id)
            .bind(applied_at)
            .execute(&mut **tx)
            .await
            .map_err(ProxyError::Database)?
            .rows_affected()
                > 0;

            if marker_inserted {
                request_stats_pipeline.flush_request_stats_writes_once(
                    &mut tx,
                    &batch.pending_dashboard_rollups,
                    &batch.pending_api_key_usage,
                    &batch.pending_auth_token_activity,
                    &batch.pending_account_request_rollups,
                    &batch.pending_request_log_catalog,
                )
                .await?;
            }

            sqlx::query("DELETE FROM request_stats_flush_batches WHERE applied_at < ?")
                .bind(applied_at.saturating_sub(REQUEST_STATS_FLUSH_BATCH_MARKER_RETENTION_SECS))
                .execute(&mut **tx)
                .await
                .map_err(ProxyError::Database)?;
            Ok::<(), ProxyError>(())
        })
        .await?;
        tx.commit().await.map_err(ProxyError::Database)
    }

    async fn flush_request_stats_writes_once(
        &self,
        tx: &mut SqliteRequestStatsTransaction<'_>,
        pending_dashboard_rollups: &HashMap<(i64, i64), DashboardRequestRollupCounts>,
        pending_api_key_usage: &HashMap<(String, i64), ApiKeyUsageBucketDelta>,
        pending_auth_token_activity: &HashMap<String, AuthTokenActivityDelta>,
        pending_account_request_rollups: &HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
        pending_request_log_catalog: &HashMap<RequestLogCatalogRollupKey, i64>,
    ) -> Result<(), ProxyError> {
        let updated_at = self.backend_time.now_ts();
        let mut dashboard_entries = pending_dashboard_rollups
            .iter()
            .map(|(key, counts)| (*key, *counts))
            .collect::<Vec<_>>();
        dashboard_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for ((bucket_start, bucket_secs), counts) in dashboard_entries {
            KeyStore::upsert_dashboard_request_rollup_bucket(
                tx,
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
            KeyStore::upsert_api_key_usage_bucket_delta(
                tx,
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
            KeyStore::upsert_auth_token_activity_delta(tx, &token_id, delta).await?;
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
                    .execute(&mut ***tx)
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
                    .execute(&mut ***tx)
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
                    .execute(&mut ***tx)
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
            KeyStore::upsert_request_log_catalog_rollup_delta(tx, &key, delta, updated_at)
                .await?;
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
        .execute(&mut ***tx)
        .await?;

        Ok(())
    }

}

impl KeyStore {
    async fn fetch_api_key_usage_bucket_success_count(
        &self,
        bucket_start_at_least: i64,
        bucket_start_before: Option<i64>,
    ) -> Result<i64, ProxyError> {
        if let Some(bucket_start_before) = bucket_start_before {
            request_stats_primary_fetch_scalar_one!(
                self.request_stats_pipeline,
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
            )
        } else {
            request_stats_primary_fetch_scalar_one!(
                self.request_stats_pipeline,
                sqlx::query_scalar::<_, i64>(
                    r#"
                    SELECT COALESCE(SUM(success_count), 0)
                    FROM api_key_usage_buckets
                    WHERE bucket_secs = 86400
                      AND bucket_start >= ?
                    "#,
                )
                .bind(bucket_start_at_least)
            )
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
            let mut tx = self.request_stats_pipeline.begin_primary_transaction().await?;
            let retained_tail_success = SqliteRequestStatsTransaction::run_bounded_operation(
                tx.operation_budget(),
                Self::fetch_visible_request_log_success_count_tx(
                    &mut tx,
                    gap_end,
                    last_gap_bucket_end,
                ),
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

#[cfg(test)]
mod request_stats_flush_tests {
    use super::*;

    #[tokio::test]
    async fn durable_batch_marker_prevents_additive_replay() {
        let temp_dir = tempfile::tempdir().expect("create request stats flush temp directory");
        let database_path = temp_dir.path().join("request-stats-flush.db");
        let database_path = database_path.to_string_lossy().into_owned();
        let store = KeyStore::new(&database_path)
            .await
            .expect("create request stats flush store");
        let bucket_start = store.backend_time.now_ts() - 300;
        let counts = DashboardRequestRollupCounts {
            total_requests: 1,
            success_count: 1,
            ..DashboardRequestRollupCounts::default()
        };
        let batch = RequestStatsFlushBatch {
            batch_id: "request-stats-flush-marker-test".to_string(),
            pending_dashboard_rollups: std::collections::HashMap::from([(
                (bucket_start, 300),
                counts,
            )]),
            pending_api_key_usage: std::collections::HashMap::new(),
            pending_auth_token_activity: std::collections::HashMap::new(),
            pending_account_request_rollups: std::collections::HashMap::new(),
            pending_request_log_catalog: std::collections::HashMap::new(),
            drained_oldest_pending_created_at: Some(bucket_start),
            drained_newest_pending_created_at: Some(bucket_start),
        };

        RequestStatsPipeline::persist_request_stats_flush_batch(
            &store.request_stats_pipeline,
            false,
            &batch,
        )
        .await
        .expect("persist first request stats batch");
        RequestStatsPipeline::persist_request_stats_flush_batch(
            &store.request_stats_pipeline,
            false,
            &batch,
        )
        .await
        .expect("replay request stats batch");

        let total: i64 = sqlx::query_scalar(
            "SELECT total_requests FROM dashboard_request_rollup_buckets WHERE bucket_start = ? AND bucket_secs = ?",
        )
        .bind(bucket_start)
        .bind(300)
        .fetch_one(&store.pool)
        .await
        .expect("read persisted request stats batch");
        assert_eq!(total, 1);
    }
}

/// PostgreSQL twins of the request-stats flush helpers.
///
/// The SQLite implementations stay in `key_store_request_stats_flush_and_public_metrics.rs`
/// and keep serving the request path until the cutover wave; these variants are reachable
/// only through the attached PostgreSQL pool.
impl KeyStore {
#[allow(clippy::too_many_arguments)]
    async fn flush_request_stats_writes_drained_batch_pg(
        request_stats_coalescer: RequestStatsCoalescer,
        backend_time: BackendTime,
        pool: sqlx::PgPool,
        _sqlite_runtime: SqliteRuntime,
        retry_budget: Duration,
        drained: DrainedRequestStatsFlushBatch,
        log_transient_exhaustion: bool,
        max_committed_chunks: Option<usize>,
    ) -> Result<(), ProxyError> {
        let retry_deadline = backend_time.instant_now() + retry_budget;
        let operation_started = Instant::now();
        let mut uncommitted = drained;
        // Start conservatively because one logical key can fan out into several
        // rollup writes. Increase only after a transaction stays within budget.
        let mut chunk_size = REQUEST_STATS_FLUSH_MIN_LOGICAL_KEYS;
        let mut committed_chunks = 0usize;
        let flush_result = loop {
            if uncommitted.is_empty() {
                break Ok(());
            }
            if backend_time.instant_now() >= retry_deadline {
                break Err(request_stats_flush_wait_budget_exhausted_error());
            }

            let chunk = uncommitted.take_chunk(chunk_size);
            let chunk_started = Instant::now();
            match Self::flush_request_stats_chunk_with_retry_pg(
                &pool,
                &_sqlite_runtime,
                &backend_time,
                retry_deadline,
                operation_started,
                &chunk,
                log_transient_exhaustion,
            )
            .await
            {
                Ok(()) => {
                    committed_chunks = committed_chunks.saturating_add(1);
                    if chunk_started.elapsed() > REQUEST_STATS_FLUSH_SLOW_TRANSACTION {
                        chunk_size = (chunk_size / 2).max(REQUEST_STATS_FLUSH_MIN_LOGICAL_KEYS);
                    } else {
                        chunk_size = (chunk_size + REQUEST_STATS_FLUSH_MIN_LOGICAL_KEYS)
                            .min(REQUEST_STATS_FLUSH_MAX_LOGICAL_KEYS);
                    }
                    if max_committed_chunks.is_some_and(|limit| committed_chunks >= limit) {
                        break Ok(());
                    }
                }
                Err(err) => {
                    uncommitted.absorb(chunk);
                    break Err(err);
                }
            }
        };

        if flush_result.is_ok() {
            #[cfg(test)]
            request_stats_coalescer
                .wait_for_post_flush_pause_if_installed()
                .await;
        }
        {
            let mut state = request_stats_coalescer.state.lock().await;
            state.flushing = false;
            state.flush_deadline = None;
            if let Err(err) = flush_result {
                state.flushing_oldest_created_at = None;
                state.flushing_newest_created_at = None;
                state.flushing_dashboard_rollup_ranges.clear();
                uncommitted.requeue_into(&mut state);
                RequestStatsCoalescer::mark_flush_deadline_if_pending(&mut state);
                request_stats_coalescer.flushed.notify_waiters();
                return Err(err);
            }
            state.flushing_oldest_created_at = None;
            state.flushing_newest_created_at = None;
            state.flushing_dashboard_rollup_ranges.clear();
            // A background admission owns a small, wall-clock-bounded group
            // of committed transactions. Return every remaining key before
            // releasing the coalescer so the next nominal tick can resume.
            uncommitted.requeue_into(&mut state);
            if RequestStatsCoalescer::pending_key_count(&state) == 0 {
                state.oldest_pending_created_at = None;
                state.newest_pending_created_at = None;
            } else {
                RequestStatsCoalescer::mark_flush_deadline_if_pending(&mut state);
            }
            request_stats_coalescer.flushed.notify_waiters();
        }
        Ok(())
    }

    async fn flush_request_stats_chunk_with_retry_pg(
        pool: &sqlx::PgPool,
        sqlite_runtime: &SqliteRuntime,
        backend_time: &BackendTime,
        retry_deadline: Instant,
        operation_started: Instant,
        chunk: &DrainedRequestStatsFlushBatch,
        log_transient_exhaustion: bool,
    ) -> Result<(), ProxyError> {
        let pending_batch_counts = format!(
            "dashboard={},api_key={},auth_token={},account_rollup={},request_catalog={}",
            chunk.pending_dashboard_rollups.len(),
            chunk.pending_api_key_usage.len(),
            chunk.pending_auth_token_activity.len(),
            chunk.pending_account_request_rollups.len(),
            chunk.pending_request_log_catalog.len(),
        );
        let log_fields = SqliteContentionLogFields {
            operation: "flush_request_stats_writes",
            request_path: "/internal/request-stats-flush",
            request_kind: "internal:request-stats-flush",
            billing_subject_kind: "unknown",
            retry_budget_ms: retry_deadline
                .saturating_duration_since(backend_time.instant_now())
                .as_millis() as u64,
            pending_batch_counts: pending_batch_counts.as_str(),
            oldest_pending_created_at: chunk.drained_oldest_pending_created_at,
            newest_pending_created_at: chunk.drained_newest_pending_created_at,
        };
        let mut retry_attempt = 0usize;
        loop {
            match Self::flush_request_stats_writes_once_pg(
                pool,
                sqlite_runtime,
                backend_time,
                retry_deadline,
                chunk,
            )
            .await
            {
                Ok(()) => return Ok(()),
                Err(err) if is_transient_sqlite_write_error(&err) => {
                    let now = backend_time.instant_now();
                    if now >= retry_deadline {
                        if log_transient_exhaustion {
                            log_sqlite_transient_write_exhaustion_with_fields(
                                log_fields,
                                retry_attempt + 1,
                                operation_started.elapsed(),
                                &err,
                            );
                        }
                        return Err(err);
                    }
                    let backoff = sqlite_transient_write_retry_delay(retry_attempt)
                        .min(retry_deadline.saturating_duration_since(now));
                    sqlite_runtime.record_retry(SqliteOperation::RequestStatsFlush);
                    if log_transient_exhaustion {
                        log_sqlite_transient_write_retry_with_fields(
                            log_fields,
                            retry_attempt + 1,
                            backoff,
                            operation_started.elapsed(),
                            &err,
                        );
                    }
                    backend_time.sleep(backoff).await;
                    retry_attempt += 1;
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn flush_request_stats_writes_once_pg(
        pool: &sqlx::PgPool,
        sqlite_runtime: &SqliteRuntime,
        backend_time: &BackendTime,
        retry_deadline: Instant,
        chunk: &DrainedRequestStatsFlushBatch,
    ) -> Result<(), ProxyError> {
        let remaining = retry_deadline.saturating_duration_since(backend_time.instant_now());
        if remaining.is_zero() {
            sqlite_runtime.record_deferred(
                SqliteOperation::RequestStatsFlush,
                SqliteAdmissionDeferReason::RecentContention,
            );
            return Err(ProxyError::Database(sqlx::Error::PoolTimedOut));
        }
        // The remaining retry budget bounds both the transaction's lock wait and the
        // flush itself. The batch commits or rolls back inside this call, so a cancelled
        // wait can never leave a half-applied delta for the coalescer to requeue.
        Self::flush_request_stats_writes_once_within_deadline_pg(
            pool,
            sqlite_runtime,
            backend_time,
            retry_deadline,
            chunk,
        )
        .await
    }

    async fn flush_request_stats_writes_once_within_deadline_pg(
        pool: &sqlx::PgPool,
        _sqlite_runtime: &SqliteRuntime,
        backend_time: &BackendTime,
        retry_deadline: Instant,
        chunk: &DrainedRequestStatsFlushBatch,
    ) -> Result<(), ProxyError> {
        let updated_at = backend_time.now_ts();
        // The coalescer's `flushing` flag serializes flushes inside one process; the
        // advisory lock keeps two processes from interleaving the same bucket deltas the
        // way the SQLite immediate-write lock used to. The deadline becomes the lock wait
        // budget so a blocked batch fails inside its retry window.
        let mut tx = pool.begin().await?;
        let remaining = retry_deadline.saturating_duration_since(backend_time.instant_now());
        sqlx::query("SELECT set_config('lock_timeout', $1, true)")
            .bind(format!("{}ms", (remaining.as_millis() as i64).max(1)))
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('tavily_hikari.request_stats_flush'))")
            .execute(&mut *tx)
            .await?;
        let write_result = async {
        let mut dashboard_entries = chunk
            .pending_dashboard_rollups
            .iter()
            .map(|(key, counts)| (*key, *counts))
            .collect::<Vec<_>>();
        dashboard_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for ((bucket_start, bucket_secs), counts) in dashboard_entries {
            Self::upsert_dashboard_request_rollup_bucket_pg(
                &mut tx,
                bucket_start,
                bucket_secs,
                counts,
                updated_at,
            )
            .await?;
        }

        let mut api_key_usage_entries = chunk
            .pending_api_key_usage
            .iter()
            .map(|(key, delta)| (key.clone(), *delta))
            .collect::<Vec<_>>();
        api_key_usage_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for ((key_id, bucket_start), delta) in api_key_usage_entries {
            Self::upsert_api_key_usage_bucket_delta_pg(
                &mut tx,
                &key_id,
                bucket_start,
                delta,
                updated_at,
            )
            .await?;
        }

        let mut auth_token_activity_entries = chunk
            .pending_auth_token_activity
            .iter()
            .map(|(token_id, delta)| (token_id.clone(), delta.clone()))
            .collect::<Vec<_>>();
        auth_token_activity_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for (token_id, delta) in auth_token_activity_entries {
            Self::upsert_auth_token_activity_delta_pg(&mut tx, &token_id, delta).await?;
        }

        let mut account_request_rollup_entries = chunk
            .pending_account_request_rollups
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
                        INSERT INTO core.account_usage_rollup_buckets (
                            user_id,
                            metric_kind,
                            bucket_kind,
                            bucket_start,
                            value,
                            updated_at
                        )
                        VALUES ($1, $2, $3, $4, $5, $6)
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
                        INSERT INTO core.account_usage_rollup_buckets (
                            user_id,
                            metric_kind,
                            bucket_kind,
                            bucket_start,
                            value,
                            updated_at
                        )
                        VALUES ($1, $2, $3, $4, $5, $6)
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
                        INSERT INTO core.account_usage_rollup_buckets (
                            user_id,
                            metric_kind,
                            bucket_kind,
                            bucket_start,
                            value,
                            updated_at
                        )
                        VALUES ($1, $2, $3, $4, $5, $6)
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

        let mut request_log_catalog_entries = chunk
            .pending_request_log_catalog
            .iter()
            .map(|(key, delta)| (key.clone(), *delta))
            .collect::<Vec<_>>();
        request_log_catalog_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for (key, delta) in request_log_catalog_entries {
            Self::upsert_request_log_catalog_rollup_delta_pg(&mut tx, &key, delta, updated_at)
                .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO core.meta (key, value)
            VALUES ($1, $2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(META_KEY_REQUEST_STATS_LAST_FLUSHED_AT_V1)
        .bind(updated_at.to_string())
        .execute(&mut *tx)
        .await?;

        Ok(())
        }
        .await;
        match write_result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(err) => {
                tx.rollback().await?;
                Err(err)
            }
        }
    }

}

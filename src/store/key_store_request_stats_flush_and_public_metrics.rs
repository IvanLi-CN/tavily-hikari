#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestStatsReadFreshness {
    Fresh,
    DurableFallback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestStatsBackgroundFlushOutcome {
    Flushed,
    Deferred(SqliteAdmissionDeferReason),
}

#[derive(Debug)]
struct DrainedRequestStatsFlushBatch {
    pending_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    pending_api_key_usage: HashMap<(String, i64), ApiKeyUsageBucketDelta>,
    pending_auth_token_activity: HashMap<String, AuthTokenActivityDelta>,
    pending_account_request_rollups: HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
    pending_request_log_catalog: HashMap<RequestLogCatalogRollupKey, i64>,
    drained_oldest_pending_created_at: Option<i64>,
    drained_newest_pending_created_at: Option<i64>,
}

const REQUEST_STATS_FLUSH_MAX_LOGICAL_KEYS: usize = 250;
const REQUEST_STATS_FLUSH_MIN_LOGICAL_KEYS: usize = 25;
const REQUEST_STATS_FLUSH_SLOW_TRANSACTION: Duration = Duration::from_millis(50);
const REQUEST_STATS_BACKGROUND_MAX_COMMITTED_CHUNKS: usize = 4;

impl DrainedRequestStatsFlushBatch {
    fn is_empty(&self) -> bool {
        self.pending_dashboard_rollups.is_empty()
            && self.pending_api_key_usage.is_empty()
            && self.pending_auth_token_activity.is_empty()
            && self.pending_account_request_rollups.is_empty()
            && self.pending_request_log_catalog.is_empty()
    }

    fn take_chunk(&mut self, max_keys: usize) -> Self {
        let mut remaining = max_keys;
        let mut chunk = Self {
            pending_dashboard_rollups: HashMap::new(),
            pending_api_key_usage: HashMap::new(),
            pending_auth_token_activity: HashMap::new(),
            pending_account_request_rollups: HashMap::new(),
            pending_request_log_catalog: HashMap::new(),
            drained_oldest_pending_created_at: self.drained_oldest_pending_created_at,
            drained_newest_pending_created_at: self.drained_newest_pending_created_at,
        };
        take_request_stats_chunk_entries(
            &mut self.pending_dashboard_rollups,
            &mut chunk.pending_dashboard_rollups,
            &mut remaining,
        );
        take_request_stats_chunk_entries(
            &mut self.pending_api_key_usage,
            &mut chunk.pending_api_key_usage,
            &mut remaining,
        );
        take_request_stats_chunk_entries(
            &mut self.pending_auth_token_activity,
            &mut chunk.pending_auth_token_activity,
            &mut remaining,
        );
        take_request_stats_chunk_entries(
            &mut self.pending_account_request_rollups,
            &mut chunk.pending_account_request_rollups,
            &mut remaining,
        );
        take_request_stats_chunk_entries(
            &mut self.pending_request_log_catalog,
            &mut chunk.pending_request_log_catalog,
            &mut remaining,
        );
        chunk
    }

    fn absorb(&mut self, other: Self) {
        for (key, counts) in other.pending_dashboard_rollups {
            self.pending_dashboard_rollups.entry(key).or_default().add(counts);
        }
        for (key, delta) in other.pending_api_key_usage {
            self.pending_api_key_usage.entry(key).or_default().add(delta);
        }
        for (token_id, delta) in other.pending_auth_token_activity {
            self.pending_auth_token_activity
                .entry(token_id)
                .or_default()
                .add(delta);
        }
        for (key, delta) in other.pending_account_request_rollups {
            self.pending_account_request_rollups
                .entry(key)
                .or_default()
                .add(delta);
        }
        for (key, delta) in other.pending_request_log_catalog {
            *self.pending_request_log_catalog.entry(key).or_default() += delta;
        }
    }

    fn requeue_into(self, state: &mut RequestStatsCoalescerState) {
        for (key, counts) in self.pending_dashboard_rollups {
            state.pending_dashboard_rollups.entry(key).or_default().add(counts);
        }
        for (key, delta) in self.pending_api_key_usage {
            state.pending_api_key_usage.entry(key).or_default().add(delta);
        }
        for (token_id, delta) in self.pending_auth_token_activity {
            state
                .pending_auth_token_activity
                .entry(token_id)
                .or_default()
                .add(delta);
        }
        for (key, delta) in self.pending_account_request_rollups {
            state
                .pending_account_request_rollups
                .entry(key)
                .or_default()
                .add(delta);
        }
        for (key, delta) in self.pending_request_log_catalog {
            *state.pending_request_log_catalog.entry(key).or_default() += delta;
        }
        if let Some(created_at) = self.drained_oldest_pending_created_at {
            state.oldest_pending_created_at = Some(
                state
                    .oldest_pending_created_at
                    .map(|current| current.min(created_at))
                    .unwrap_or(created_at),
            );
        }
        if let Some(created_at) = self.drained_newest_pending_created_at {
            state.newest_pending_created_at = Some(
                state
                    .newest_pending_created_at
                    .map(|current| current.max(created_at))
                    .unwrap_or(created_at),
            );
        }
    }
}

fn take_request_stats_chunk_entries<K, V>(
    source: &mut HashMap<K, V>,
    target: &mut HashMap<K, V>,
    remaining: &mut usize,
) where
    K: std::cmp::Eq + std::hash::Hash,
{
    if *remaining == 0 {
        return;
    }
    for (key, value) in std::mem::take(source) {
        if *remaining == 0 {
            source.insert(key, value);
        } else {
            target.insert(key, value);
            *remaining -= 1;
        }
    }
}

impl KeyStore {
    #[cfg(debug_assertions)]
    pub(crate) async fn flush_request_stats_writes(&self) -> Result<(), ProxyError> {
        self.flush_request_stats_writes_with_wait_policy(
            Duration::from_secs(10),
            None,
            true,
            None,
        )
        .await
    }

    pub(crate) async fn flush_request_stats_writes_in_background(
        &self,
    ) -> Result<RequestStatsBackgroundFlushOutcome, ProxyError> {
        let permit = match self
            .sqlite_runtime
            .try_admit_maintenance_bulk(SqliteOperation::RequestStatsFlush)
        {
            Ok(permit) => permit,
            Err(reason) => return Ok(RequestStatsBackgroundFlushOutcome::Deferred(reason)),
        };
        let result = self
            .flush_request_stats_writes_with_wait_policy(
                // A background slice must yield before a foreground control
                // transaction can spend its own 100ms admission budget. Four
                // adaptive chunks can cover at most 250 logical keys while
                // the wall-clock budget remains the primary bound.
                Duration::from_millis(50),
                Some(self.backend_time.instant_now() + Duration::from_millis(50)),
                false,
                Some(REQUEST_STATS_BACKGROUND_MAX_COMMITTED_CHUNKS),
            )
            .await;
        drop(permit);
        match result {
            Ok(()) => Ok(RequestStatsBackgroundFlushOutcome::Flushed),
            Err(err) if is_transient_sqlite_write_error(&err) => Ok(
                RequestStatsBackgroundFlushOutcome::Deferred(
                    SqliteAdmissionDeferReason::RecentContention,
                ),
            ),
            Err(err) if is_request_stats_flush_wait_budget_exhausted(&err) => {
                self.sqlite_runtime.record_deferred(
                    SqliteOperation::RequestStatsFlush,
                    SqliteAdmissionDeferReason::RecentContention,
                );
                Ok(RequestStatsBackgroundFlushOutcome::Deferred(
                    SqliteAdmissionDeferReason::RecentContention,
                ))
            }
            Err(err) => Err(err),
        }
    }

    pub(crate) fn request_stats_read_freshness(&self) -> RequestStatsReadFreshness {
        if self
            .request_stats_coalescer
            .try_has_pending_or_flushing_work()
        {
            RequestStatsReadFreshness::DurableFallback
        } else {
            RequestStatsReadFreshness::Fresh
        }
    }

    pub(crate) fn request_stats_durable_freshness_for_maintenance(
        &self,
    ) -> RequestStatsReadFreshness {
        self.request_stats_read_freshness()
    }

    async fn flush_request_stats_writes_with_wait_policy(
        &self,
        retry_budget: Duration,
        inflight_wait_deadline: Option<Instant>,
        log_transient_exhaustion: bool,
        max_committed_chunks: Option<usize>,
    ) -> Result<(), ProxyError> {
        loop {
            if inflight_wait_deadline.is_some_and(|deadline| {
                self.backend_time.instant_now() >= deadline
            }) {
                return Err(request_stats_flush_wait_budget_exhausted_error());
            }
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
                    Some(DrainedRequestStatsFlushBatch {
                        pending_dashboard_rollups: std::mem::take(&mut state.pending_dashboard_rollups),
                        pending_api_key_usage: std::mem::take(&mut state.pending_api_key_usage),
                        pending_auth_token_activity: std::mem::take(&mut state.pending_auth_token_activity),
                        pending_account_request_rollups: std::mem::take(
                            &mut state.pending_account_request_rollups,
                        ),
                        pending_request_log_catalog: std::mem::take(
                            &mut state.pending_request_log_catalog,
                        ),
                        drained_oldest_pending_created_at: state.flushing_oldest_created_at,
                        drained_newest_pending_created_at: state.flushing_newest_created_at,
                    })
                }
            };
            let Some(drained) = pending else {
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

            // Once this worker owns a drained batch, keep its admission permit
            // until the bounded flush returns the batch to the coalescer or
            // commits it. A detached flush would outlive the bulk permit and
            // could reacquire the SQLite writer under foreground pressure.
            Self::flush_request_stats_writes_drained_batch(
                self.request_stats_coalescer.clone(),
                self.backend_time.clone(),
                self.sqlite_runtime.clone(),
                retry_budget,
                drained,
                log_transient_exhaustion,
                max_committed_chunks,
            )
            .await?;
            if max_committed_chunks.is_some() {
                return Ok(());
            }
        }
    }

    #[cfg(test)]
    pub(crate) async fn flush_request_stats_writes_with_wait_policy_for_test(
        &self,
        retry_budget: Duration,
        inflight_wait_deadline: Option<Instant>,
    ) -> Result<(), ProxyError> {
        self.flush_request_stats_writes_with_wait_policy(
            retry_budget,
            inflight_wait_deadline,
            true,
            None,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn flush_request_stats_background_slice_for_test(
        &self,
    ) -> Result<(), ProxyError> {
        // CI can spend a few scheduler quanta opening the test database. Keep
        // the production 50ms slice unchanged while giving this deterministic
        // contract test enough bounded time to observe one finite slice.
        const TEST_SLICE_BUDGET: Duration = Duration::from_millis(250);
        self.flush_request_stats_writes_with_wait_policy(
            TEST_SLICE_BUDGET,
            Some(self.backend_time.instant_now() + TEST_SLICE_BUDGET),
            false,
            Some(REQUEST_STATS_BACKGROUND_MAX_COMMITTED_CHUNKS),
        )
        .await
    }

    async fn flush_request_stats_writes_drained_batch(
        request_stats_coalescer: RequestStatsCoalescer,
        backend_time: BackendTime,
        sqlite_runtime: SqliteRuntime,
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
            match Self::flush_request_stats_chunk_with_retry(
                &sqlite_runtime,
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

        {
            let mut state = request_stats_coalescer.state.lock().await;
            state.flushing = false;
            state.flush_deadline = None;
            if let Err(err) = flush_result {
                state.flushing_oldest_created_at = None;
                state.flushing_newest_created_at = None;
                uncommitted.requeue_into(&mut state);
                RequestStatsCoalescer::mark_flush_deadline_if_pending(&mut state);
                request_stats_coalescer.flushed.notify_waiters();
                return Err(err);
            }
            state.flushing_oldest_created_at = None;
            state.flushing_newest_created_at = None;
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
        #[cfg(test)]
        request_stats_coalescer
            .wait_for_post_flush_pause_if_installed()
            .await;
        Ok(())
    }

    async fn flush_request_stats_chunk_with_retry(
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
            match Self::flush_request_stats_writes_once(
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

    async fn flush_request_stats_writes_once(
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
        match tokio::time::timeout(
            remaining,
            Self::flush_request_stats_writes_once_within_deadline(
                sqlite_runtime,
                backend_time,
                retry_deadline,
                chunk,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                sqlite_runtime.record_deferred(
                    SqliteOperation::RequestStatsFlush,
                    SqliteAdmissionDeferReason::RecentContention,
                );
                Err(ProxyError::Database(sqlx::Error::PoolTimedOut))
            }
        }
    }

    async fn flush_request_stats_writes_once_within_deadline(
        sqlite_runtime: &SqliteRuntime,
        backend_time: &BackendTime,
        retry_deadline: Instant,
        chunk: &DrainedRequestStatsFlushBatch,
    ) -> Result<(), ProxyError> {
        let updated_at = backend_time.now_ts();
        let mut tx = sqlite_runtime
            .begin_immediate_before(
                SqliteOperation::RequestStatsFlush,
                retry_deadline.into(),
            )
            .await?;
        let write_result = async {
        let mut dashboard_entries = chunk
            .pending_dashboard_rollups
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

        let mut api_key_usage_entries = chunk
            .pending_api_key_usage
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

        let mut auth_token_activity_entries = chunk
            .pending_auth_token_activity
            .iter()
            .map(|(token_id, delta)| (token_id.clone(), delta.clone()))
            .collect::<Vec<_>>();
        auth_token_activity_entries.sort_by(|left, right| left.0.cmp(&right.0));
        for (token_id, delta) in auth_token_activity_entries {
            Self::upsert_auth_token_activity_delta(&mut tx, &token_id, delta).await?;
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

        let mut request_log_catalog_entries = chunk
            .pending_request_log_catalog
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

        Ok(())
        }
        .await;
        tx.finish(write_result).await
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

fn is_request_stats_flush_wait_budget_exhausted(error: &ProxyError) -> bool {
    matches!(error, ProxyError::Other(message) if message == "request stats flush wait budget exhausted")
}

#[derive(Debug, Default)]
pub(crate) struct RequestStatsPipelineState {
    pending_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    dashboard_rollup_repairs: BTreeMap<i64, DashboardRollupRepairBarrier>,
    pending_api_key_usage: HashMap<(String, i64), ApiKeyUsageBucketDelta>,
    pending_auth_token_activity: HashMap<String, AuthTokenActivityDelta>,
    pending_account_request_rollups:
        HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
    pending_request_log_catalog: HashMap<RequestLogCatalogRollupKey, i64>,
    retry_flush_batches: std::collections::VecDeque<RequestStatsFlushBatch>,
    request_stats_version: u64,
    dashboard_rollup_source_versions: BTreeMap<i64, i64>,
    oldest_pending_created_at: Option<i64>,
    newest_pending_created_at: Option<i64>,
    flushing_oldest_created_at: Option<i64>,
    flushing_newest_created_at: Option<i64>,
    reserved_pending_keys: usize,
    flush_deadline: Option<Instant>,
    flushing: bool,
    shutdown: bool,
    worker_stopped: bool,
}

#[derive(Debug)]
pub(crate) struct DashboardRollupRepairBarrier {
    range_end: i64,
    source_fence_id: i64,
    source_included_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    post_fence_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
}

#[derive(Debug, Clone)]
pub(crate) struct RequestStatsPipeline {
    sqlite_runtime: SqliteRuntime,
    backend_time: BackendTime,
    state: Arc<StdMutex<RequestStatsPipelineState>>,
    dashboard_rollup_source_updates: Arc<StdMutex<DashboardRollupSourceUpdates>>,
    wake: Arc<Notify>,
    flushed: Arc<Notify>,
    #[cfg(test)]
    post_flush_pause: Arc<Mutex<Option<RequestStatsPipelinePause>>>,
}

#[derive(Debug, Default)]
pub(crate) struct DashboardRollupSourceUpdates {
    in_flight: BTreeMap<i64, u64>,
    cancelled_versions: BTreeMap<i64, i64>,
}

pub(crate) struct RequestLogRollupInput<'a> {
    pub(crate) api_key_id: Option<&'a str>,
    pub(crate) auth_token_id: &'a str,
    pub(crate) request_user_id: Option<&'a str>,
    pub(crate) request_log_id: Option<i64>,
    pub(crate) created_at: i64,
    pub(crate) dashboard_counts: DashboardRequestRollupCounts,
    pub(crate) request_log_catalog_key: Option<RequestLogCatalogRollupKey>,
}

#[derive(Debug, Clone)]
pub(crate) struct RequestStatsFlushBatch {
    pub(crate) batch_id: String,
    pub(crate) pending_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    pub(crate) pending_api_key_usage: HashMap<(String, i64), ApiKeyUsageBucketDelta>,
    pub(crate) pending_auth_token_activity: HashMap<String, AuthTokenActivityDelta>,
    pub(crate) pending_account_request_rollups: HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
    pub(crate) pending_request_log_catalog: HashMap<RequestLogCatalogRollupKey, i64>,
    pub(crate) drained_oldest_pending_created_at: Option<i64>,
    pub(crate) drained_newest_pending_created_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestStatsSnapshot {
    pub(crate) version: u64,
    pub(crate) has_pending_work: bool,
    pub(crate) oldest_pending_created_at: Option<i64>,
    pub(crate) newest_pending_created_at: Option<i64>,
}

#[derive(Debug)]
pub(crate) enum RequestStatsFlushClaim {
    Empty,
    InFlight,
    Batch(Box<RequestStatsFlushBatch>),
}

#[derive(Debug)]
pub(crate) struct RequestStatsFlushBatchGuard {
    pipeline: RequestStatsPipeline,
    batch: Option<RequestStatsFlushBatch>,
}

#[must_use]
#[derive(Debug)]
struct RequestStatsPendingCapacityReservation {
    pipeline: RequestStatsPipeline,
    keys: usize,
}

impl Drop for RequestStatsPendingCapacityReservation {
    fn drop(&mut self) {
        let mut state = self
            .pipeline
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.reserved_pending_keys = state.reserved_pending_keys.saturating_sub(self.keys);
    }
}

impl RequestStatsFlushBatchGuard {
    pub(crate) fn new(pipeline: RequestStatsPipeline, batch: RequestStatsFlushBatch) -> Self {
        Self {
            pipeline,
            batch: Some(batch),
        }
    }

    pub(crate) fn batch(&self) -> &RequestStatsFlushBatch {
        self.batch
            .as_ref()
            .expect("request stats flush batch guard is complete")
    }

    fn take_batch(&mut self) -> RequestStatsFlushBatch {
        self.batch
            .take()
            .expect("request stats flush batch guard is complete")
    }
}

impl Drop for RequestStatsFlushBatchGuard {
    fn drop(&mut self) {
        let Some(batch) = self.batch.take() else {
            return;
        };
        self.pipeline.requeue_flush_batch(batch);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestStatsWorkerWait {
    pub(crate) flush_now: bool,
    pub(crate) wait_duration: Duration,
}

#[derive(Debug, Clone)]
pub struct RequestStatsPipelinePause {
    pub(crate) arrived: Arc<Notify>,
    pub(crate) release: Arc<Notify>,
    pub(crate) released: Arc<std::sync::atomic::AtomicBool>,
}

pub type RequestStatsPostFlushPause = RequestStatsPipelinePause;

impl RequestStatsPipelinePause {
    #[doc(hidden)]
    pub async fn wait_until_arrived(&self) {
        self.arrived.notified().await;
    }

    #[doc(hidden)]
    pub fn release(&self) {
        self.released
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.release.notify_waiters();
    }
}
impl RequestStatsPipeline {
    pub(crate) fn new(sqlite_runtime: SqliteRuntime, backend_time: BackendTime) -> Self {
        Self {
            sqlite_runtime,
            backend_time,
            state: Arc::new(StdMutex::new(RequestStatsPipelineState::default())),
            dashboard_rollup_source_updates: Arc::new(StdMutex::new(
                DashboardRollupSourceUpdates::default(),
            )),
            wake: Arc::new(Notify::new()),
            flushed: Arc::new(Notify::new()),
            #[cfg(test)]
            post_flush_pause: Arc::new(Mutex::new(None)),
        }
    }

    async fn fetch_request_stats_one<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> Result<sqlx::sqlite::SqliteRow, ProxyError> {
        self.sqlite_runtime.fetch_request_stats_one(query).await
    }

    async fn fetch_request_stats_scalar_one<'q, O>(
        &self,
        query: sqlx::query::QueryScalar<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> Result<O, ProxyError>
    where
        O: Send + Unpin + 'q,
        (O,): Send + Unpin + for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow>,
    {
        self.sqlite_runtime
            .fetch_request_stats_scalar_one(query)
            .await
    }

    async fn fetch_request_stats_optional<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> Result<Option<sqlx::sqlite::SqliteRow>, ProxyError> {
        self.sqlite_runtime.fetch_request_stats_optional(query).await
    }

    async fn fetch_request_stats_scalar_optional<'q, O>(
        &self,
        query: sqlx::query::QueryScalar<'q, Sqlite, O, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> Result<Option<O>, ProxyError>
    where
        O: Send + Unpin + 'q,
        (O,): Send + Unpin + for<'r> sqlx::FromRow<'r, sqlx::sqlite::SqliteRow>,
    {
        self.sqlite_runtime
            .fetch_request_stats_scalar_optional(query)
            .await
    }

    async fn fetch_request_stats_all<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> Result<Vec<sqlx::sqlite::SqliteRow>, ProxyError> {
        self.sqlite_runtime.fetch_request_stats_all(query).await
    }

    async fn execute_request_stats<'q>(
        &self,
        query: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> Result<sqlx::sqlite::SqliteQueryResult, ProxyError> {
        self.sqlite_runtime.execute_request_stats(query).await
    }

    pub(crate) async fn begin_primary_transaction(
        &self,
    ) -> Result<SqliteRequestStatsTransaction<'_>, ProxyError> {
        self.sqlite_runtime.begin_primary_transaction().await
    }

    pub(crate) async fn begin_read_flush_transaction(
        &self,
    ) -> Result<SqliteRequestStatsTransaction<'_>, ProxyError> {
        self.sqlite_runtime.begin_read_flush_transaction().await
    }

    pub(crate) async fn acquire_primary_connection(
        &self,
    ) -> Result<SqliteRequestStatsConnection, ProxyError> {
        self.sqlite_runtime.acquire_primary_connection().await
    }

    pub(crate) fn backend_time(&self) -> &BackendTime {
        &self.backend_time
    }

    pub(crate) async fn flush_request_stats_writes(&self) -> Result<(), ProxyError> {
        self.flush_request_stats_writes_with_wait_policy(
            false,
            Duration::from_secs(10),
            None,
        )
        .await
    }

    pub(crate) async fn flush_request_stats_writes_with_wait_policy(
        &self,
        use_read_flush_pool: bool,
        retry_budget: Duration,
        inflight_wait_deadline: Option<Instant>,
    ) -> Result<(), ProxyError> {
        flush_request_stats_writes_with_wait_policy(
            self,
            use_read_flush_pool,
            retry_budget,
            inflight_wait_deadline,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn flush_request_stats_writes_with_wait_policy_for_test(
        &self,
        retry_budget: Duration,
        inflight_wait_deadline: Option<Instant>,
    ) -> Result<(), ProxyError> {
        self.flush_request_stats_writes_with_wait_policy(
            true,
            retry_budget,
            inflight_wait_deadline,
        )
        .await
    }
}

macro_rules! request_stats_primary_fetch_one {
    ($pipeline:expr, $query:expr) => {{
        $pipeline.fetch_request_stats_one($query).await
    }};
}

macro_rules! request_stats_primary_fetch_optional {
    ($pipeline:expr, $query:expr) => {{
        $pipeline.fetch_request_stats_optional($query).await
    }};
}

macro_rules! request_stats_primary_fetch_scalar_one {
    ($pipeline:expr, $query:expr) => {{
        $pipeline.fetch_request_stats_scalar_one($query).await
    }};
}

macro_rules! request_stats_primary_fetch_scalar_optional {
    ($pipeline:expr, $query:expr) => {{
        $pipeline.fetch_request_stats_scalar_optional($query).await
    }};
}

macro_rules! request_stats_primary_fetch_all {
    ($pipeline:expr, $query:expr) => {{
        $pipeline.fetch_request_stats_all($query).await
    }};
}

macro_rules! request_stats_primary_execute {
    ($pipeline:expr, $query:expr) => {{
        $pipeline.execute_request_stats($query).await
    }};
}

#[cfg(test)]
impl Default for RequestStatsPipeline {
    fn default() -> Self {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("create lazy sqlite pool for request stats pipeline test");
        Self::new(
            SqliteRuntime::new(pool.clone(), pool, 1),
            BackendTime::system(),
        )
    }
}

impl RequestStatsPipeline {
    pub(crate) const MAX_PENDING_KEYS: usize = 100;
    const MAX_REQUEST_LOG_PENDING_KEYS: usize = 6;
    pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(1);
    pub(crate) const BACKFILL_PAGE_ROWS: i64 = 500;

    fn pending_key_count(state: &RequestStatsPipelineState) -> usize {
        state.pending_dashboard_rollups.len()
            + state.pending_api_key_usage.len()
            + state.pending_auth_token_activity.len()
            + state.pending_account_request_rollups.len()
            + state.pending_request_log_catalog.len()
            + state
                .retry_flush_batches
                .iter()
                .map(|batch| {
                    batch.pending_dashboard_rollups.len()
                        + batch.pending_api_key_usage.len()
                        + batch.pending_auth_token_activity.len()
                        + batch.pending_account_request_rollups.len()
                        + batch.pending_request_log_catalog.len()
                })
                .sum::<usize>()
    }

    async fn reserve_pending_capacity(
        &self,
        additional_keys: usize,
    ) -> RequestStatsPendingCapacityReservation {
        debug_assert!(additional_keys <= Self::MAX_PENDING_KEYS);
        loop {
            let notified = self.flushed.notified();
            let has_capacity = {
                let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let reserved_total = Self::pending_key_count(&state)
                    .saturating_add(state.reserved_pending_keys)
                    .saturating_add(additional_keys);
                if reserved_total <= Self::MAX_PENDING_KEYS {
                    state.reserved_pending_keys = state
                        .reserved_pending_keys
                        .saturating_add(additional_keys);
                    true
                } else {
                    false
                }
            };
            if has_capacity {
                return RequestStatsPendingCapacityReservation {
                    pipeline: self.clone(),
                    keys: additional_keys,
                };
            }
            self.wake.notify_one();
            notified.await;
        }
    }

    fn bump_request_stats_version(state: &mut RequestStatsPipelineState) {
        state.request_stats_version = state.request_stats_version.wrapping_add(1);
    }

    fn snapshot_from_state(state: &RequestStatsPipelineState) -> RequestStatsSnapshot {
        let retry_oldest_created_at = state
            .retry_flush_batches
            .iter()
            .filter_map(|batch| batch.drained_oldest_pending_created_at)
            .min();
        let retry_newest_created_at = state
            .retry_flush_batches
            .iter()
            .filter_map(|batch| batch.drained_newest_pending_created_at)
            .max();
        RequestStatsSnapshot {
            version: state.request_stats_version,
            has_pending_work: state.flushing
                || Self::pending_key_count(state) > 0
                || !state.dashboard_rollup_repairs.is_empty(),
            oldest_pending_created_at: [
                state.oldest_pending_created_at,
                state.flushing_oldest_created_at,
                retry_oldest_created_at,
            ]
            .into_iter()
            .flatten()
            .min(),
            newest_pending_created_at: [
                state.newest_pending_created_at,
                state.flushing_newest_created_at,
                retry_newest_created_at,
            ]
            .into_iter()
            .flatten()
            .max(),
        }
    }

    pub(crate) async fn snapshot(&self) -> RequestStatsSnapshot {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::snapshot_from_state(&state)
    }

    pub(crate) fn try_snapshot(&self) -> Option<RequestStatsSnapshot> {
        self.state
            .try_lock()
            .ok()
            .map(|state| Self::snapshot_from_state(&state))
    }

    pub(crate) async fn claim_flush_batch(&self) -> RequestStatsFlushClaim {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.flushing {
            return RequestStatsFlushClaim::InFlight;
        }
        if let Some(batch) = state.retry_flush_batches.pop_front() {
            state.flushing = true;
            state.flushing_oldest_created_at = batch.drained_oldest_pending_created_at;
            state.flushing_newest_created_at = batch.drained_newest_pending_created_at;
            return RequestStatsFlushClaim::Batch(Box::new(batch));
        }
        if Self::pending_key_count(&state) == 0 {
            return RequestStatsFlushClaim::Empty;
        }

        state.flushing = true;
        state.flushing_oldest_created_at = state.oldest_pending_created_at.take();
        state.flushing_newest_created_at = state.newest_pending_created_at.take();
        RequestStatsFlushClaim::Batch(Box::new(RequestStatsFlushBatch {
            batch_id: uuid::Uuid::new_v4().to_string(),
            pending_dashboard_rollups: std::mem::take(&mut state.pending_dashboard_rollups),
            pending_api_key_usage: std::mem::take(&mut state.pending_api_key_usage),
            pending_auth_token_activity: std::mem::take(&mut state.pending_auth_token_activity),
            pending_account_request_rollups: std::mem::take(
                &mut state.pending_account_request_rollups,
            ),
            pending_request_log_catalog: std::mem::take(&mut state.pending_request_log_catalog),
            drained_oldest_pending_created_at: state.flushing_oldest_created_at,
            drained_newest_pending_created_at: state.flushing_newest_created_at,
        }))
    }

    fn requeue_flush_batch(&self, batch: RequestStatsFlushBatch) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.flushing = false;
        state.flush_deadline = None;
        state.flushing_oldest_created_at = None;
        state.flushing_newest_created_at = None;
        state.retry_flush_batches.push_front(batch);
        self.flushed.notify_waiters();
        drop(state);
        self.wake.notify_one();
    }

    pub(crate) fn finish_flush_batch(
        &self,
        guard: &mut RequestStatsFlushBatchGuard,
        result: Result<(), ProxyError>,
    ) -> Result<(), ProxyError> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let batch = guard.take_batch();
        state.flushing = false;
        state.flush_deadline = None;
        match result {
            Ok(()) => {
                state.flushing_oldest_created_at = None;
                state.flushing_newest_created_at = None;
                if Self::pending_key_count(&state) == 0 {
                    state.oldest_pending_created_at = None;
                    state.newest_pending_created_at = None;
                } else {
                    Self::mark_flush_deadline_if_pending(&mut state);
                }
                self.flushed.notify_waiters();
                Ok(())
            }
            Err(err) => {
                state.flushing_oldest_created_at = None;
                state.flushing_newest_created_at = None;
                state.retry_flush_batches.push_front(batch);
                self.flushed.notify_waiters();
                drop(state);
                self.wake.notify_one();
                Err(err)
            }
        }
    }

    pub(crate) async fn enqueue_account_request_rollup(
        &self,
        user_id: &str,
        created_at: i64,
        primary_success_delta: i64,
    ) {
        let _capacity = self.reserve_pending_capacity(1).await;
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = state
            .pending_account_request_rollups
            .entry(AccountRequestRollupKey {
                user_id: user_id.to_string(),
                five_minute_bucket_start: created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES),
                day_bucket_start: local_day_bucket_start_utc_ts(created_at),
            })
            .or_default();
        entry.request_count += 1;
        entry.primary_success += primary_success_delta.max(0);
        Self::bump_request_stats_version(&mut state);
        Self::note_pending_created_at(&mut state, created_at);
        Self::mark_flush_deadline_if_pending(&mut state);
        drop(state);
        self.wake.notify_one();
    }

    pub(crate) fn begin_dashboard_rollup_source_mutation(
        &self,
        created_at: i64,
    ) -> DashboardRollupSourceMutation {
        let range_start = created_at.div_euclid(SECS_PER_FIVE_MINUTES) * SECS_PER_FIVE_MINUTES;
        let mut updates = self
            .dashboard_rollup_source_updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let retention_floor = range_start.saturating_sub(92 * SECS_PER_DAY);
        updates
            .cancelled_versions
            .retain(|bucket_start, _| *bucket_start >= retention_floor);
        *updates.in_flight.entry(range_start).or_default() += 1;
        DashboardRollupSourceMutation {
            pipeline: self.clone(),
            range_start,
            committed: false,
        }
    }

    pub(crate) async fn dashboard_rollup_source_version_is_stable(
        &self,
        range_start: i64,
        expected_version: i64,
    ) -> bool {
        let current_version = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .dashboard_rollup_source_versions
            .get(&range_start)
            .copied()
            .unwrap_or_default();
        let updates = self
            .dashboard_rollup_source_updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        current_version.wrapping_add(
            updates
                .cancelled_versions
                .get(&range_start)
                .copied()
                .unwrap_or_default(),
        ) == expected_version
            && updates
                .in_flight
                .get(&range_start)
                .copied()
                .unwrap_or_default()
                == 0
    }

    pub(crate) async fn dashboard_rollup_source_version(&self, range_start: i64) -> i64 {
        let current_version = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .dashboard_rollup_source_versions
            .get(&range_start)
            .copied()
            .unwrap_or_default();
        let updates = self
            .dashboard_rollup_source_updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        current_version.wrapping_add(
            updates
                .cancelled_versions
                .get(&range_start)
                .copied()
                .unwrap_or_default(),
        )
    }

    fn mark_flush_deadline_if_pending(state: &mut RequestStatsPipelineState) {
        if Self::pending_key_count(state) > 0 && state.flush_deadline.is_none() {
            state.flush_deadline = Some(Instant::now() + Self::FLUSH_INTERVAL);
        }
    }

    fn note_pending_created_at(state: &mut RequestStatsPipelineState, created_at: i64) {
        state.oldest_pending_created_at = Some(
            state
                .oldest_pending_created_at
                .map(|current| current.min(created_at))
                .unwrap_or(created_at),
        );
        state.newest_pending_created_at = Some(
            state
                .newest_pending_created_at
                .map(|current| current.max(created_at))
                .unwrap_or(created_at),
        );
    }

    pub(crate) async fn enqueue_request_log_rollups(&self, input: RequestLogRollupInput<'_>) {
        let _capacity = self
            .reserve_pending_capacity(Self::MAX_REQUEST_LOG_PENDING_KEYS)
            .await;
        let RequestLogRollupInput {
            api_key_id,
            auth_token_id,
            request_user_id,
            request_log_id,
            created_at,
            dashboard_counts,
            request_log_catalog_key,
        } = input;
        {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let day_bucket_start = local_day_bucket_start_utc_ts(created_at);
            Self::enqueue_dashboard_rollup_delta_locked(
                &mut state,
                created_at,
                request_log_id,
                dashboard_counts,
                false,
            );
            if let Some(api_key_id) = api_key_id {
                state
                    .pending_api_key_usage
                    .entry((api_key_id.to_string(), day_bucket_start))
                    .or_default()
                    .add(ApiKeyUsageBucketDelta {
                        total_requests: dashboard_counts.total_requests,
                        success_count: dashboard_counts.success_count,
                        error_count: dashboard_counts.error_count,
                        quota_exhausted_count: dashboard_counts.quota_exhausted_count,
                        valuable_success_count: dashboard_counts.valuable_success_count,
                        valuable_failure_count: dashboard_counts.valuable_failure_count,
                        other_success_count: dashboard_counts.other_success_count,
                        other_failure_count: dashboard_counts.other_failure_count,
                        unknown_count: dashboard_counts.unknown_count,
                    });
            }
            Self::enqueue_auth_token_activity_locked(
                &mut state,
                auth_token_id,
                request_user_id,
                created_at,
                dashboard_counts.valuable_success_count,
                dashboard_counts.other_success_count,
            );
            if let Some(request_log_catalog_key) = request_log_catalog_key {
                *state
                    .pending_request_log_catalog
                    .entry(request_log_catalog_key)
                    .or_default() += 1;
            }
            Self::bump_request_stats_version(&mut state);
            Self::note_pending_created_at(&mut state, created_at);
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    pub(crate) async fn enqueue_auth_token_activity(
        &self,
        auth_token_id: &str,
        request_user_id: Option<&str>,
        created_at: i64,
    ) {
        let _capacity = self.reserve_pending_capacity(2).await;
        {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Self::enqueue_auth_token_activity_locked(
                &mut state,
                auth_token_id,
                request_user_id,
                created_at,
                0,
                0,
            );
            Self::bump_request_stats_version(&mut state);
            Self::note_pending_created_at(&mut state, created_at);
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    fn enqueue_auth_token_activity_locked(
        state: &mut RequestStatsPipelineState,
        auth_token_id: &str,
        request_user_id: Option<&str>,
        created_at: i64,
        primary_success_delta: i64,
        secondary_success_delta: i64,
    ) {
        state
            .pending_auth_token_activity
            .entry(auth_token_id.to_string())
            .or_default()
            .add_request(created_at);
        if let Some(user_id) = request_user_id {
            let five_minute_bucket_start =
                created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
            let day_bucket_start = local_day_bucket_start_utc_ts(created_at);
            let entry = state
                .pending_account_request_rollups
                .entry(AccountRequestRollupKey {
                    user_id: user_id.to_string(),
                    five_minute_bucket_start,
                    day_bucket_start,
                })
                .or_default();
            entry.request_count += 1;
            entry.primary_success += primary_success_delta.max(0);
            entry.secondary_success += secondary_success_delta.max(0);
        }
    }

    pub(crate) async fn enqueue_dashboard_credit_rollups(&self, created_at: i64, credits: i64) {
        self.enqueue_dashboard_credit_rollups_for_request_log(created_at, credits, None)
            .await;
    }

    pub(crate) async fn enqueue_dashboard_credit_rollups_for_request_log(
        &self,
        created_at: i64,
        credits: i64,
        request_log_id: Option<i64>,
    ) {
        if credits <= 0 {
            return;
        }
        let _capacity = self.reserve_pending_capacity(2).await;
        {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let counts = DashboardRequestRollupCounts {
                local_estimated_credits: credits,
                ..DashboardRequestRollupCounts::default()
            };
            // Credits can settle after a request log was first observed. Treat a
            // credit delta as post-fence during a repair so that the next slice
            // re-reads the amended source row before declaring the bucket valid.
            Self::enqueue_dashboard_rollup_delta_locked(
                &mut state,
                created_at,
                request_log_id,
                counts,
                true,
            );
            Self::bump_request_stats_version(&mut state);
            Self::note_pending_created_at(&mut state, created_at);
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_one();
    }

    fn enqueue_dashboard_rollup_delta_locked(
        state: &mut RequestStatsPipelineState,
        created_at: i64,
        request_log_id: Option<i64>,
        dashboard_counts: DashboardRequestRollupCounts,
        force_post_fence: bool,
    ) {
        let minute_bucket_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
        let day_bucket_start = local_day_bucket_start_utc_ts(created_at);
        let keys = [
            (minute_bucket_start, SECS_PER_MINUTE),
            (day_bucket_start, SECS_PER_DAY),
        ];
        let repair = state
            .dashboard_rollup_repairs
            .range_mut(..=minute_bucket_start)
            .next_back()
            .filter(|(_, repair)| minute_bucket_start < repair.range_end)
            .map(|(_, repair)| repair);
        if let Some(repair) = repair {
            let destination = if force_post_fence
                || request_log_id
                    .map(|id| id > repair.source_fence_id)
                    .unwrap_or(true)
            {
                &mut repair.post_fence_dashboard_rollups
            } else {
                &mut repair.source_included_dashboard_rollups
            };
            for key in keys {
                destination.entry(key).or_default().add(dashboard_counts);
            }
            return;
        }
        for key in keys {
            state
                .pending_dashboard_rollups
                .entry(key)
                .or_default()
                .add(dashboard_counts);
        }
    }

    pub(crate) async fn begin_dashboard_rollup_repair(
        &self,
        range_start: i64,
        range_end: i64,
        source_fence_id: i64,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.dashboard_rollup_repairs.insert(
            range_start,
            DashboardRollupRepairBarrier {
                range_end,
                source_fence_id,
                source_included_dashboard_rollups: HashMap::new(),
                post_fence_dashboard_rollups: HashMap::new(),
            },
        );
    }

    pub(crate) async fn finish_dashboard_rollup_repair(
        &self,
        range_start: i64,
        replacement_committed: bool,
    ) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(repair) = state.dashboard_rollup_repairs.remove(&range_start) else {
            return false;
        };
        let has_post_fence_changes = !repair.post_fence_dashboard_rollups.is_empty();
        let deferred = if replacement_committed {
            repair.post_fence_dashboard_rollups
        } else {
            repair
                .source_included_dashboard_rollups
                .into_iter()
                .chain(repair.post_fence_dashboard_rollups)
                .fold(
                    HashMap::<(i64, i64), DashboardRequestRollupCounts>::new(),
                    |mut merged, (key, counts)| {
                        merged.entry(key).or_default().add(counts);
                        merged
                    },
                )
        };
        for (key, counts) in deferred {
            state
                .pending_dashboard_rollups
                .entry(key)
                .or_default()
                .add(counts);
        }
        Self::mark_flush_deadline_if_pending(&mut state);
        drop(state);
        self.wake.notify_one();
        has_post_fence_changes
    }

    #[cfg(test)]
    pub(crate) async fn pending_oldest_created_at(&self) -> Option<i64> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.oldest_pending_created_at
    }

    #[cfg(test)]
    pub(crate) async fn pending_newest_created_at(&self) -> Option<i64> {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.newest_pending_created_at
    }

    pub(crate) async fn freshness_created_at_bounds(&self) -> Option<(i64, i64)> {
        let snapshot = self.snapshot().await;
        snapshot
            .oldest_pending_created_at
            .map(|oldest| (oldest, snapshot.newest_pending_created_at.unwrap_or(oldest)))
    }

    pub(crate) async fn pending_dashboard_freshness_signature(&self) -> [i64; 10] {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut entries = state
            .pending_dashboard_rollups
            .iter()
            .map(|(&(bucket_start, bucket_secs), counts)| (bucket_start, bucket_secs, *counts))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(bucket_start, bucket_secs, _)| (*bucket_start, *bucket_secs));

        let mut signature = [0_i64; 10];
        signature[0] = entries.len() as i64;
        signature[1] = state.oldest_pending_created_at.unwrap_or_default();
        signature[2] = state.newest_pending_created_at.unwrap_or_default();
        signature[3] = state.flushing_oldest_created_at.unwrap_or_default();
        signature[4] = state.flushing_newest_created_at.unwrap_or_default();

        for (bucket_start, bucket_secs, counts) in entries {
            signature[5] += bucket_start;
            signature[6] += bucket_secs;
            signature[7] += counts.total_requests
                + counts.success_count
                + counts.error_count
                + counts.quota_exhausted_count
                + counts.valuable_success_count
                + counts.valuable_failure_count
                + counts.valuable_failure_429_count
                + counts.other_success_count
                + counts.other_failure_count
                + counts.unknown_count;
            signature[8] += counts.local_estimated_credits;
            signature[9] += counts.mcp_non_billable
                + counts.mcp_billable
                + counts.api_non_billable
                + counts.api_billable;
        }

        signature
    }

    pub(crate) async fn wait_until_not_flushing(&self) {
        loop {
            let notified = {
                let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if !state.flushing {
                    return;
                }
                self.flushed.clone().notified_owned()
            };
            notified.await;
        }
    }

    pub(crate) async fn begin_shutdown(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.shutdown = true;
            state.flush_deadline = Some(Instant::now());
        }
        self.wake.notify_waiters();
    }

    pub(crate) async fn nudge_flush(&self) {
        {
            let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_waiters();
    }

    pub(crate) async fn worker_wait(&self) -> RequestStatsWorkerWait {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let pending_key_count = Self::pending_key_count(&state);
        let repairs_pending = !state.dashboard_rollup_repairs.is_empty();
        let deadline_due = pending_key_count > 0
            && state
                .flush_deadline
                .map(|deadline| Instant::now() >= deadline)
                .unwrap_or(false);
        let flush_now = (state.shutdown && state.dashboard_rollup_repairs.is_empty())
            || pending_key_count >= Self::MAX_PENDING_KEYS
            || deadline_due;
        let wait_duration = if pending_key_count == 0 && (repairs_pending || state.shutdown) {
            Self::FLUSH_INTERVAL
        } else {
            state
                .flush_deadline
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
                .unwrap_or(Self::FLUSH_INTERVAL)
        };
        RequestStatsWorkerWait {
            flush_now,
            wait_duration,
        }
    }

    async fn should_stop_after_flush(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.shutdown
            && Self::pending_key_count(&state) == 0
            && state.dashboard_rollup_repairs.is_empty()
    }

    async fn mark_worker_started(&self) {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).worker_stopped = false;
    }

    async fn mark_worker_stopped(&self) {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).worker_stopped = true;
        self.flushed.notify_waiters();
    }

    pub(crate) async fn run_worker(&self) {
        self.mark_worker_started().await;
        loop {
            let wait = self.worker_wait().await;
            if !wait.flush_now {
                tokio::select! {
                    _ = self.wake.notified() => {}
                    _ = tokio::time::sleep(wait.wait_duration) => {}
                }
                continue;
            }

            let should_flush = {
                let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Self::pending_key_count(&state) > 0 || state.shutdown
            };
            if !should_flush {
                continue;
            }

            let flush_started = Instant::now();
            if let Err(err) = self.flush_request_stats_writes().await {
                log_db_operation_error(
                    "request stats persist",
                    flush_started.elapsed(),
                    Some("component=request-stats-pipeline"),
                    &err,
                );
                tracing::debug!(
                    component = "request_stats",
                    event = "persist_retry",
                    elapsed_ms = flush_started.elapsed().as_millis() as u64,
                    err = %err,
                    "request stats persist deferred after structured database error"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            } else {
                log_slow_db_operation(
                    "request stats persist",
                    flush_started.elapsed(),
                    Some("component=request-stats-pipeline"),
                );
            }

            if self.should_stop_after_flush().await {
                self.mark_worker_stopped().await;
                break;
            }
        }
    }

    pub(crate) async fn wait_until_worker_stopped(&self) {
        loop {
            let notified = {
                let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if state.worker_stopped {
                    return;
                }
                self.flushed.clone().notified_owned()
            };
            notified.await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn is_flushing_only_created_at(&self, created_at: i64) -> bool {
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.flushing
            && state.oldest_pending_created_at.is_none()
            && state.flushing_oldest_created_at == Some(created_at)
            && state.flushing_newest_created_at == Some(created_at)
    }

    #[cfg(test)]
    pub(crate) async fn is_flushing(&self) -> bool {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).flushing
    }

    #[cfg(test)]
    pub(crate) async fn pending_dashboard_total(&self) -> i64 {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending_dashboard_rollups
            .values()
            .map(|counts| counts.total_requests)
            .sum()
    }

    #[cfg(test)]
    pub(crate) async fn pending_dashboard_rollups_are_empty(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending_dashboard_rollups
            .is_empty()
    }

    #[cfg(test)]
    pub(crate) async fn install_post_flush_pause(&self) -> RequestStatsPipelinePause {
        let pause = new_request_stats_test_pause();
        let mut slot = self.post_flush_pause.lock().await;
        *slot = Some(pause.clone());
        pause
    }

    #[cfg(test)]
    pub(crate) async fn wait_for_post_flush_pause_if_installed(&self) {
        wait_for_request_stats_test_pause_if_installed(&self.post_flush_pause).await;
    }
}

pub(crate) struct DashboardRollupSourceMutation {
    pipeline: RequestStatsPipeline,
    range_start: i64,
    committed: bool,
}

impl DashboardRollupSourceMutation {
    pub(crate) async fn commit(mut self) {
        {
            let mut state = self.pipeline.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let retention_floor = self.range_start.saturating_sub(92 * SECS_PER_DAY);
            state
                .dashboard_rollup_source_versions
                .retain(|bucket_start, _| *bucket_start >= retention_floor);
            let version = state
                .dashboard_rollup_source_versions
                .entry(self.range_start)
                .or_default();
            *version = version.wrapping_add(1);
        }
        let mut updates = self
            .pipeline
            .dashboard_rollup_source_updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(count) = updates.in_flight.get_mut(&self.range_start) {
            *count -= 1;
            if *count == 0 {
                updates.in_flight.remove(&self.range_start);
            }
        }
        self.committed = true;
    }
}

impl Drop for DashboardRollupSourceMutation {
    fn drop(&mut self) {
        if !self.committed {
            let mut updates = self
                .pipeline
                .dashboard_rollup_source_updates
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let version = updates
                .cancelled_versions
                .entry(self.range_start)
                .or_default();
            *version = version.wrapping_add(1);
            if let Some(count) = updates.in_flight.get_mut(&self.range_start) {
                *count -= 1;
                if *count == 0 {
                    updates.in_flight.remove(&self.range_start);
                }
            }
        }
    }
}

#[cfg(test)]
mod request_stats_pipeline_capacity_tests {
    use super::*;

    #[tokio::test]
    async fn pending_capacity_reservation_waits_for_flush_notification() {
        let pipeline = RequestStatsPipeline::default();
        {
            let mut state = pipeline.state.lock().expect("lock request stats state");
            state.reserved_pending_keys = RequestStatsPipeline::MAX_PENDING_KEYS;
        }

        let pipeline_for_reservation = pipeline.clone();
        let mut waiting = tokio::spawn(async move {
            pipeline_for_reservation
                .reserve_pending_capacity(1)
                .await
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "reservation should wait at capacity");

        {
            let mut state = pipeline.state.lock().expect("lock request stats state");
            state.reserved_pending_keys = 0;
        }
        pipeline.flushed.notify_waiters();

        let reservation = tokio::time::timeout(Duration::from_secs(1), &mut waiting)
            .await
            .expect("reservation should wake after flush")
            .expect("reservation task should not panic");
        assert_eq!(
            pipeline
                .state
                .lock()
                .expect("lock request stats state")
                .reserved_pending_keys,
            1
        );

        drop(reservation);
        assert_eq!(
            pipeline
                .state
                .lock()
                .expect("lock request stats state")
                .reserved_pending_keys,
            0
        );
    }

    #[tokio::test]
    async fn concurrent_pending_capacity_reservations_are_bounded_and_released() {
        let pipeline = RequestStatsPipeline::default();
        let (first, second, third, fourth) = tokio::join!(
            pipeline.reserve_pending_capacity(25),
            pipeline.reserve_pending_capacity(25),
            pipeline.reserve_pending_capacity(25),
            pipeline.reserve_pending_capacity(25),
        );

        assert_eq!(
            pipeline
                .state
                .lock()
                .expect("lock request stats state")
                .reserved_pending_keys,
            RequestStatsPipeline::MAX_PENDING_KEYS
        );

        drop((first, second, third, fourth));
        assert_eq!(
            pipeline
                .state
                .lock()
                .expect("lock request stats state")
                .reserved_pending_keys,
            0
        );
    }
}

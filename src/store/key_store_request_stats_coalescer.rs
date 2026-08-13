#[derive(Debug, Default)]
pub(crate) struct RequestStatsCoalescerState {
    pub(crate) pending_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    pub(crate) dashboard_rollup_repairs: BTreeMap<i64, DashboardRollupRepairBarrier>,
    pub(crate) pending_api_key_usage: HashMap<(String, i64), ApiKeyUsageBucketDelta>,
    pub(crate) pending_auth_token_activity: HashMap<String, AuthTokenActivityDelta>,
    pub(crate) pending_account_request_rollups:
        HashMap<AccountRequestRollupKey, AccountUsageRollupDelta>,
    pub(crate) pending_request_log_catalog: HashMap<RequestLogCatalogRollupKey, i64>,
    pub(crate) request_stats_version: u64,
    pub(crate) dashboard_rollup_source_versions: BTreeMap<i64, i64>,
    pub(crate) oldest_pending_created_at: Option<i64>,
    pub(crate) newest_pending_created_at: Option<i64>,
    pub(crate) flushing_oldest_created_at: Option<i64>,
    pub(crate) flushing_newest_created_at: Option<i64>,
    pub(crate) flush_deadline: Option<Instant>,
    pub(crate) flushing: bool,
    pub(crate) shutdown: bool,
    pub(crate) worker_stopped: bool,
}

#[derive(Debug)]
pub(crate) struct DashboardRollupRepairBarrier {
    pub(crate) range_end: i64,
    pub(crate) source_fence_id: i64,
    pub(crate) source_included_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
    pub(crate) post_fence_dashboard_rollups: HashMap<(i64, i64), DashboardRequestRollupCounts>,
}

#[derive(Debug, Clone)]
pub(crate) struct RequestStatsCoalescer {
    pub(crate) state: Arc<Mutex<RequestStatsCoalescerState>>,
    pub(crate) dashboard_rollup_source_updates: Arc<StdMutex<DashboardRollupSourceUpdates>>,
    pub(crate) wake: Arc<Notify>,
    pub(crate) flushed: Arc<Notify>,
    #[cfg(test)]
    pub(crate) post_flush_pause: Arc<Mutex<Option<RequestStatsPostFlushPause>>>,
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

#[cfg(any(test, debug_assertions))]
#[derive(Debug, Clone)]
pub struct RequestStatsPostFlushPause {
    pub(crate) arrived: Arc<Notify>,
    pub(crate) release: Arc<Notify>,
    pub(crate) released: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(any(test, debug_assertions))]
impl RequestStatsPostFlushPause {
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
impl Default for RequestStatsCoalescer {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(RequestStatsCoalescerState::default())),
            dashboard_rollup_source_updates: Arc::new(StdMutex::new(
                DashboardRollupSourceUpdates::default(),
            )),
            wake: Arc::new(Notify::new()),
            flushed: Arc::new(Notify::new()),
            #[cfg(test)]
            post_flush_pause: Arc::new(Mutex::new(None)),
        }
    }
}

impl RequestStatsCoalescer {
    pub(crate) const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

    pub(crate) fn pending_key_count(state: &RequestStatsCoalescerState) -> usize {
        state.pending_dashboard_rollups.len()
            + state.pending_api_key_usage.len()
            + state.pending_auth_token_activity.len()
            + state.pending_account_request_rollups.len()
            + state.pending_request_log_catalog.len()
    }

    pub(crate) fn bump_request_stats_version(state: &mut RequestStatsCoalescerState) {
        state.request_stats_version = state.request_stats_version.wrapping_add(1);
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
            coalescer: self.clone(),
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
            .await
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
            .await
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

    fn mark_flush_deadline_if_pending(state: &mut RequestStatsCoalescerState) {
        if Self::pending_key_count(state) > 0 && state.flush_deadline.is_none() {
            state.flush_deadline = Some(Instant::now() + Self::FLUSH_INTERVAL);
        }
    }

    fn note_pending_created_at(state: &mut RequestStatsCoalescerState, created_at: i64) {
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
            let mut state = self.state.lock().await;
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
        {
            let mut state = self.state.lock().await;
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
        state: &mut RequestStatsCoalescerState,
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
        {
            let mut state = self.state.lock().await;
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
        state: &mut RequestStatsCoalescerState,
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
        let mut state = self.state.lock().await;
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
        let mut state = self.state.lock().await;
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
        let state = self.state.lock().await;
        state.oldest_pending_created_at
    }

    #[cfg(test)]
    pub(crate) async fn pending_newest_created_at(&self) -> Option<i64> {
        let state = self.state.lock().await;
        state.newest_pending_created_at
    }

    pub(crate) async fn pending_dashboard_freshness_signature(&self) -> [i64; 10] {
        let state = self.state.lock().await;
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
                let state = self.state.lock().await;
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
            let mut state = self.state.lock().await;
            state.shutdown = true;
            state.flush_deadline = Some(Instant::now());
        }
        self.wake.notify_waiters();
    }

    pub(crate) async fn nudge_flush(&self) {
        {
            let mut state = self.state.lock().await;
            Self::mark_flush_deadline_if_pending(&mut state);
        }
        self.wake.notify_waiters();
    }

    pub(crate) async fn wait_until_worker_stopped(&self) {
        loop {
            let notified = {
                let state = self.state.lock().await;
                if state.worker_stopped {
                    return;
                }
                self.flushed.clone().notified_owned()
            };
            notified.await;
        }
    }

    pub(crate) fn try_has_pending_or_flushing_work(&self) -> bool {
        self.state
            .try_lock()
            .map(|state| {
                state.flushing
                    || Self::pending_key_count(&state) > 0
                    || !state.dashboard_rollup_repairs.is_empty()
            })
            .unwrap_or(true)
    }

    pub(crate) fn try_request_stats_version(&self) -> Option<u64> {
        self.state
            .try_lock()
            .ok()
            .map(|state| state.request_stats_version)
    }

    #[cfg(test)]
    pub(crate) async fn install_post_flush_pause(&self) -> RequestStatsPostFlushPause {
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
    coalescer: RequestStatsCoalescer,
    range_start: i64,
    committed: bool,
}

impl DashboardRollupSourceMutation {
    pub(crate) async fn commit(mut self) {
        {
            let mut state = self.coalescer.state.lock().await;
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
            .coalescer
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
                .coalescer
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

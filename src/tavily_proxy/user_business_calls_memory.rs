use super::*;

impl MemoryUserBusinessCalls1hBackend {
    pub(super) async fn begin_backfill(
        &self,
        upper_bound_request_log_id: i64,
        now_ts: i64,
        retention_secs: i64,
    ) {
        let mut state = self.state.lock().await;
        Self::maybe_gc(
            &mut state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
        drop(state);
        let mut backfill = self.backfill.lock().await;
        *backfill = Some(MemoryUserBusinessCalls1hBackfill {
            state: MemoryUserBusinessCalls1hState::default(),
            live_buckets: HashMap::new(),
            upper_bound_request_log_id,
        });
    }

    pub(super) async fn append_backfill_page(
        &self,
        rows: &[UserBusinessCalls1hBackfillRow],
        now_ts: i64,
        retention_secs: i64,
    ) {
        let mut backfill = self.backfill.lock().await;
        let Some(backfill) = backfill.as_mut() else {
            return;
        };
        let state = &mut backfill.state;
        for row in rows {
            if row.created_at
                <= now_ts.saturating_sub(UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS)
            {
                Self::aggregate_event(state, &row.user_id, row.created_at, row.outcome);
            } else {
                let queue = state.entries.entry(row.user_id.clone()).or_default();
                Self::insert_event_sorted(
                    queue,
                    UserBusinessCallEvent {
                        request_log_id: row.request_log_id,
                        created_at: row.created_at,
                        outcome: row.outcome,
                    },
                );
            }
        }
        Self::maybe_gc(
            state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
    }

    pub(super) async fn finish_backfill(&self, now_ts: i64, retention_secs: i64) {
        let mut state = self.state.lock().await;
        let mut backfill = self.backfill.lock().await;
        let Some(mut backfill) = backfill.take() else {
            return;
        };
        // Merge requests that arrived while the staged snapshot was being
        // built. This keeps the old serving snapshot intact on query failure
        // and only copies the bounded live tail at the final swap.
        for (user_id, queue) in &state.entries {
            for event in queue {
                if event.request_log_id.is_none_or(|request_log_id| {
                    request_log_id > backfill.upper_bound_request_log_id
                }) {
                    let target = backfill.state.entries.entry(user_id.clone()).or_default();
                    Self::insert_event_sorted(target, event.clone());
                }
            }
        }
        for (user_id, buckets) in &backfill.live_buckets {
            let target = backfill.state.buckets.entry(user_id.clone()).or_default();
            for (bucket_start, counts) in buckets {
                target.entry(*bucket_start).or_default().add(counts);
            }
        }
        Self::maybe_gc(
            &mut backfill.state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
        state.entries = backfill.state.entries;
        state.buckets = backfill.state.buckets;
        state.next_gc_at = backfill.state.next_gc_at;
    }

    pub(super) async fn abort_backfill(&self) {
        let mut backfill = self.backfill.lock().await;
        backfill.take();
    }

    pub(super) async fn replace_from_backfill(
        &self,
        rows: &[UserBusinessCalls1hBackfillRow],
        upper_bound_request_log_id: i64,
        now_ts: i64,
        retention_secs: i64,
    ) {
        self.begin_backfill(upper_bound_request_log_id, now_ts, retention_secs)
            .await;
        self.append_backfill_page(rows, now_ts, retention_secs)
            .await;
        self.finish_backfill(now_ts, retention_secs).await;
    }

    pub(super) async fn record_event(
        &self,
        user_id: &str,
        event: UserBusinessCallEvent,
        now_ts: i64,
        retention_secs: i64,
    ) {
        let mut state = self.state.lock().await;
        Self::maybe_gc(
            &mut state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
        let event_created_at = event.created_at;
        let aggregate_into_bucket = event_created_at
            <= now_ts.saturating_sub(UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS);
        let event_outcome = event.outcome;
        if aggregate_into_bucket {
            Self::aggregate_event(&mut state, user_id, event.created_at, event.outcome);
        } else {
            let queue = state.entries.entry(user_id.to_string()).or_default();
            Self::insert_event_sorted(queue, event);
        }
        if let Some(queue) = state.entries.get_mut(user_id) {
            Self::prune_queue(
                queue,
                now_ts,
                UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
            );
        }
        if let Some(buckets) = state.buckets.get_mut(user_id) {
            Self::prune_buckets(buckets, now_ts, retention_secs);
        }
        if state.entries.get(user_id).is_some_and(VecDeque::is_empty) {
            state.entries.remove(user_id);
        }
        if state.buckets.get(user_id).is_some_and(BTreeMap::is_empty) {
            state.buckets.remove(user_id);
        }
        if aggregate_into_bucket {
            let mut backfill = self.backfill.lock().await;
            if let Some(backfill) = backfill.as_mut() {
                let bucket_start =
                    event_created_at - event_created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
                backfill
                    .live_buckets
                    .entry(user_id.to_string())
                    .or_default()
                    .entry(bucket_start)
                    .or_default()
                    .record(event_outcome);
            }
        }
    }

    pub(super) async fn snapshot_many(
        &self,
        user_ids: &[String],
        now_ts: i64,
        rolling_window_secs: i64,
        retention_secs: i64,
    ) -> HashMap<String, UserBusinessCallCounts> {
        let mut state = self.state.lock().await;
        Self::maybe_gc(
            &mut state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
        let mut out = HashMap::with_capacity(user_ids.len());
        let mut empty_keys = Vec::new();
        for user_id in user_ids {
            let counts = if let Some(queue) = state.entries.get_mut(user_id) {
                Self::prune_queue(
                    queue,
                    now_ts,
                    UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
                );
                let counts = Self::rolling_counts(queue, now_ts, rolling_window_secs);
                if queue.is_empty() {
                    empty_keys.push(user_id.clone());
                }
                counts
            } else {
                UserBusinessCallCounts::default()
            };
            out.insert(user_id.clone(), counts);
        }
        for key in empty_keys {
            state.entries.remove(&key);
        }
        out
    }

    pub(super) async fn enforcement_counts(
        &self,
        user_id: &str,
        now_ts: i64,
        rolling_window_secs: i64,
        retention_secs: i64,
        reservation_ttl_secs: i64,
    ) -> UserBusinessCallEnforcementCounts {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, retention_secs, reservation_ttl_secs);
        Self::enforcement_counts_for_user(
            &mut state,
            user_id,
            now_ts,
            rolling_window_secs,
            retention_secs,
            reservation_ttl_secs,
        )
    }

    pub(super) async fn reserve(
        &self,
        user_id: &str,
        request: UserBusinessCallReserveRequest,
    ) -> BusinessCalls1hReservationOutcome {
        let mut state = self.state.lock().await;
        Self::maybe_gc(
            &mut state,
            request.now_ts,
            request.retention_secs,
            request.reservation_ttl_secs,
        );
        let counts = Self::enforcement_counts_for_user(
            &mut state,
            user_id,
            request.now_ts,
            request.rolling_window_secs,
            request.retention_secs,
            request.reservation_ttl_secs,
        );
        let summary = BusinessCalls1hSummary {
            success_count: counts.completed.success_count,
            failure_count: counts.completed.failure_count,
            total_count: counts.total_count(),
            limit: request.limit.max(0),
            window_minutes: request.window_minutes,
        };
        let verdict = BusinessCalls1hLimitVerdict::new(summary);
        if !verdict.allowed {
            return BusinessCalls1hReservationOutcome::Denied(verdict);
        }

        state.next_reservation_id = state.next_reservation_id.saturating_add(1);
        let reservation = UserBusinessCallReservation {
            user_id: user_id.to_string(),
            reservation_id: state.next_reservation_id,
            created_at: request.now_ts,
        };
        let queue = state.reservations.entry(user_id.to_string()).or_default();
        Self::insert_reservation_sorted(
            queue,
            UserBusinessCallReservationEntry {
                reservation_id: reservation.reservation_id,
                created_at: reservation.created_at,
                expires_at: request
                    .now_ts
                    .saturating_add(request.reservation_ttl_secs.max(1)),
            },
        );
        BusinessCalls1hReservationOutcome::Reserved(reservation)
    }

    pub(super) async fn finalize_reservation(
        &self,
        reservation: UserBusinessCallReservation,
        request_log_id: Option<i64>,
        outcome: UserBusinessCallOutcome,
        now_ts: i64,
        retention_secs: i64,
        reservation_ttl_secs: i64,
    ) {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, retention_secs, reservation_ttl_secs);
        let created_at = Self::remove_reservation_entry(
            &mut state,
            &reservation.user_id,
            reservation.reservation_id,
            now_ts,
        )
        .unwrap_or(reservation.created_at);
        if created_at <= now_ts.saturating_sub(UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS) {
            Self::aggregate_event(&mut state, &reservation.user_id, created_at, outcome);
        } else {
            let queue = state
                .entries
                .entry(reservation.user_id.clone())
                .or_default();
            Self::insert_event_sorted(
                queue,
                UserBusinessCallEvent {
                    request_log_id,
                    created_at,
                    outcome,
                },
            );
            Self::prune_queue(
                queue,
                now_ts,
                UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
            );
        }
        if state
            .entries
            .get(&reservation.user_id)
            .is_some_and(VecDeque::is_empty)
        {
            state.entries.remove(&reservation.user_id);
        }
    }

    pub(super) async fn release_reservation(
        &self,
        reservation: UserBusinessCallReservation,
        now_ts: i64,
        retention_secs: i64,
        reservation_ttl_secs: i64,
    ) {
        let mut state = self.state.lock().await;
        Self::maybe_gc(&mut state, now_ts, retention_secs, reservation_ttl_secs);
        let _ = Self::remove_reservation_entry(
            &mut state,
            &reservation.user_id,
            reservation.reservation_id,
            now_ts,
        );
    }

    pub(super) async fn series_data_for_user(
        &self,
        user_id: &str,
        now_ts: i64,
        retention_secs: i64,
    ) -> UserBusinessCallSeriesData {
        let mut state = self.state.lock().await;
        Self::maybe_gc(
            &mut state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
        let raw_events = state
            .entries
            .get_mut(user_id)
            .map(|queue| {
                Self::prune_queue(
                    queue,
                    now_ts,
                    UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
                );
                queue.iter().cloned().collect()
            })
            .unwrap_or_default();
        let buckets = state.buckets.get(user_id).cloned().unwrap_or_default();
        UserBusinessCallSeriesData {
            raw_events,
            buckets,
        }
    }

    pub(super) async fn snapshot_all(
        &self,
        now_ts: i64,
        rolling_window_secs: i64,
        retention_secs: i64,
    ) -> HashMap<String, UserBusinessCallCounts> {
        let mut state = self.state.lock().await;
        Self::maybe_gc(
            &mut state,
            now_ts,
            retention_secs,
            UserBusinessCalls1hWindow::RESERVATION_TTL_SECS,
        );
        let mut empty_keys = Vec::new();
        let mut out = HashMap::with_capacity(state.entries.len());
        for (user_id, queue) in &mut state.entries {
            Self::prune_queue(
                queue,
                now_ts,
                UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
            );
            let counts = Self::rolling_counts(queue, now_ts, rolling_window_secs);
            if queue.is_empty() {
                empty_keys.push(user_id.clone());
            } else if counts.total_count() > 0 {
                out.insert(user_id.clone(), counts);
            }
        }
        for key in empty_keys {
            state.entries.remove(&key);
        }
        out
    }

    fn maybe_gc(
        state: &mut MemoryUserBusinessCalls1hState,
        now_ts: i64,
        retention_secs: i64,
        reservation_ttl_secs: i64,
    ) {
        if now_ts < state.next_gc_at {
            return;
        }
        state.entries.retain(|_, queue| {
            Self::prune_queue(
                queue,
                now_ts,
                UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
            );
            !queue.is_empty()
        });
        state.buckets.retain(|_, buckets| {
            Self::prune_buckets(buckets, now_ts, retention_secs);
            !buckets.is_empty()
        });
        state.reservations.retain(|_, queue| {
            Self::prune_reservation_queue(queue, now_ts);
            !queue.is_empty()
        });
        state.next_gc_at = now_ts.saturating_add(
            retention_secs
                .min(reservation_ttl_secs.max(1))
                .clamp(60, SECS_PER_HOUR),
        );
    }

    fn prune_queue(queue: &mut VecDeque<UserBusinessCallEvent>, now_ts: i64, retention_secs: i64) {
        let expires_at = now_ts - retention_secs;
        while queue
            .front()
            .is_some_and(|event| event.created_at <= expires_at)
        {
            queue.pop_front();
        }
    }

    fn prune_buckets(
        buckets: &mut BTreeMap<i64, UserBusinessCallCounts>,
        now_ts: i64,
        retention_secs: i64,
    ) {
        let expires_at = now_ts.saturating_sub(retention_secs);
        buckets.retain(|bucket_start, _| {
            bucket_start.saturating_add(SECS_PER_FIVE_MINUTES) > expires_at
        });
    }

    fn aggregate_event(
        state: &mut MemoryUserBusinessCalls1hState,
        user_id: &str,
        created_at: i64,
        outcome: UserBusinessCallOutcome,
    ) {
        let bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
        state
            .buckets
            .entry(user_id.to_string())
            .or_default()
            .entry(bucket_start)
            .or_default()
            .record(outcome);
    }

    fn prune_reservation_queue(
        queue: &mut VecDeque<UserBusinessCallReservationEntry>,
        now_ts: i64,
    ) {
        while queue
            .front()
            .is_some_and(|reservation| reservation.expires_at <= now_ts)
        {
            queue.pop_front();
        }
    }

    fn insert_event_sorted(
        queue: &mut VecDeque<UserBusinessCallEvent>,
        event: UserBusinessCallEvent,
    ) {
        if let Some(request_log_id) = event.request_log_id
            && queue
                .iter()
                .any(|existing| existing.request_log_id == Some(request_log_id))
        {
            return;
        }
        let insert_at = queue
            .iter()
            .position(|existing| existing.created_at > event.created_at)
            .unwrap_or(queue.len());
        queue.insert(insert_at, event);
    }

    fn insert_reservation_sorted(
        queue: &mut VecDeque<UserBusinessCallReservationEntry>,
        reservation: UserBusinessCallReservationEntry,
    ) {
        let insert_at = queue
            .iter()
            .position(|existing| existing.created_at > reservation.created_at)
            .unwrap_or(queue.len());
        queue.insert(insert_at, reservation);
    }

    fn rolling_counts(
        queue: &VecDeque<UserBusinessCallEvent>,
        now_ts: i64,
        rolling_window_secs: i64,
    ) -> UserBusinessCallCounts {
        let cutoff = now_ts - rolling_window_secs;
        let mut counts = UserBusinessCallCounts::default();
        for event in queue.iter().filter(|event| event.created_at > cutoff) {
            counts.record(event.outcome);
        }
        counts
    }

    fn enforcement_counts_for_user(
        state: &mut MemoryUserBusinessCalls1hState,
        user_id: &str,
        now_ts: i64,
        rolling_window_secs: i64,
        _retention_secs: i64,
        _reservation_ttl_secs: i64,
    ) -> UserBusinessCallEnforcementCounts {
        let (completed, remove_entry) = if let Some(queue) = state.entries.get_mut(user_id) {
            Self::prune_queue(
                queue,
                now_ts,
                UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS,
            );
            (
                Self::rolling_counts(queue, now_ts, rolling_window_secs),
                queue.is_empty(),
            )
        } else {
            (UserBusinessCallCounts::default(), false)
        };
        if remove_entry {
            state.entries.remove(user_id);
        }

        let (reservation_count, remove_reservation) =
            if let Some(queue) = state.reservations.get_mut(user_id) {
                Self::prune_reservation_queue(queue, now_ts);
                (queue.len() as i64, queue.is_empty())
            } else {
                (0, false)
            };
        if remove_reservation {
            state.reservations.remove(user_id);
        }
        UserBusinessCallEnforcementCounts {
            completed,
            reservation_count,
        }
    }

    fn remove_reservation_entry(
        state: &mut MemoryUserBusinessCalls1hState,
        user_id: &str,
        reservation_id: u64,
        now_ts: i64,
    ) -> Option<i64> {
        let (removed, should_remove) = {
            let queue = state.reservations.get_mut(user_id)?;
            Self::prune_reservation_queue(queue, now_ts);
            let removed = queue
                .iter()
                .position(|reservation| reservation.reservation_id == reservation_id)
                .and_then(|index| queue.remove(index))
                .map(|reservation| reservation.created_at);
            (removed, queue.is_empty())
        };
        if should_remove {
            state.reservations.remove(user_id);
        }
        removed
    }
}

#[cfg(test)]
mod memory_window_regression_tests {
    use super::*;

    #[tokio::test]
    async fn backfill_does_not_keep_all_25_hours_as_raw_events() {
        let backend = MemoryUserBusinessCalls1hBackend::default();
        let now = 1_750_000_000;
        let rows = (0..(25 * 60))
            .map(|index| UserBusinessCalls1hBackfillRow {
                request_log_id: Some(index + 1),
                user_id: "user-1".to_string(),
                created_at: now - index * 60,
                outcome: if index % 2 == 0 {
                    UserBusinessCallOutcome::Success
                } else {
                    UserBusinessCallOutcome::Failure
                },
            })
            .collect::<Vec<_>>();
        backend
            .replace_from_backfill(
                &rows,
                rows.len() as i64,
                now,
                UserBusinessCalls1hWindow::RETENTION_SECS,
            )
            .await;

        let state = backend.state.lock().await;
        let raw_events = state.entries.values().map(VecDeque::len).sum::<usize>();
        assert!(
            raw_events <= 90,
            "only the recent one-hour window may remain event-granular; got {raw_events} raw events"
        );
    }

    #[tokio::test]
    async fn aborted_staged_backfill_preserves_serving_snapshot() {
        let backend = MemoryUserBusinessCalls1hBackend::default();
        let now = 1_750_000_000;
        backend
            .record_event(
                "serving-user",
                UserBusinessCallEvent {
                    request_log_id: Some(101),
                    created_at: now - 30,
                    outcome: UserBusinessCallOutcome::Success,
                },
                now,
                UserBusinessCalls1hWindow::RETENTION_SECS,
            )
            .await;
        backend
            .begin_backfill(100, now, UserBusinessCalls1hWindow::RETENTION_SECS)
            .await;
        backend
            .append_backfill_page(
                &[UserBusinessCalls1hBackfillRow {
                    request_log_id: Some(99),
                    user_id: "staged-user".to_string(),
                    created_at: now - 60,
                    outcome: UserBusinessCallOutcome::Failure,
                }],
                now,
                UserBusinessCalls1hWindow::RETENTION_SECS,
            )
            .await;
        backend.abort_backfill().await;

        let state = backend.state.lock().await;
        assert_eq!(
            state.entries.get("serving-user").map(VecDeque::len),
            Some(1)
        );
        assert!(!state.entries.contains_key("staged-user"));
    }

    #[tokio::test]
    async fn backfill_preserves_live_bucketed_events() {
        let backend = MemoryUserBusinessCalls1hBackend::default();
        let now = 1_750_000_000;
        backend
            .begin_backfill(100, now, UserBusinessCalls1hWindow::RETENTION_SECS)
            .await;
        backend
            .record_event(
                "live-user",
                UserBusinessCallEvent {
                    request_log_id: Some(101),
                    created_at: now - UserBusinessCalls1hWindow::ROLLING_WINDOW_SECS - 1,
                    outcome: UserBusinessCallOutcome::Success,
                },
                now,
                UserBusinessCalls1hWindow::RETENTION_SECS,
            )
            .await;
        backend
            .finish_backfill(now, UserBusinessCalls1hWindow::RETENTION_SECS)
            .await;

        let state = backend.state.lock().await;
        let total = state
            .buckets
            .get("live-user")
            .into_iter()
            .flat_map(|buckets| buckets.values())
            .map(UserBusinessCallCounts::total_count)
            .sum::<i64>();
        assert_eq!(
            total, 1,
            "the backfill swap must retain live bucketed events"
        );
    }
}

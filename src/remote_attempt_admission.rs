use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

#[cfg(test)]
use futures_util::FutureExt;
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

/// Coordinates the one-at-a-time lease that protects an actual outbound
/// upstream request and an aged reconciliation scheduling turn. Local SQLite
/// preparation never acquires the outbound lease.
#[derive(Debug)]
#[doc(hidden)]
pub struct RemoteAttemptAdmissionController {
    attempt_slot: Arc<Semaphore>,
    reconciliation_turn: Mutex<ReconciliationTurnState>,
    next_reconciliation_turn_id: AtomicU64,
    reconciliation_turn_cleared: Notify,
    active_attempts: AtomicUsize,
    peak_active_attempts: AtomicUsize,
    total_wait_ms: AtomicU64,
    total_hold_ms: AtomicU64,
}

#[derive(Debug, Default)]
struct ReconciliationTurnState {
    id: u64,
    kind: Option<ReconciliationTurnKind>,
    resumable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub struct RemoteAttemptMetrics {
    pub(crate) active_attempts: usize,
    pub(crate) peak_active_attempts: usize,
    pub(crate) total_wait_ms: u64,
    pub(crate) total_hold_ms: u64,
}

#[derive(Debug)]
#[doc(hidden)]
pub struct RemoteAttemptLease {
    controller: Arc<RemoteAttemptAdmissionController>,
    _permit: OwnedSemaphorePermit,
    reconciliation_turn_id: AtomicU64,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum ReconciliationTurnKind {
    Main,
    ResearchDrain,
}

/// A durable reason for pausing a Research drain before its result transaction
/// is accepted. The serialized value is part of the scheduled-job contract;
/// callers must not hand-roll it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum ResearchDrainDeferReason {
    ForegroundPressure,
    RemoteLease,
    ReadBudget,
    ControlDefer,
    KeyCooldown,
}

impl ResearchDrainDeferReason {
    #[doc(hidden)]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ForegroundPressure => "foreground_pressure",
            Self::RemoteLease => "remote_lease",
            Self::ReadBudget => "read_budget",
            Self::ControlDefer => "control_defer",
            Self::KeyCooldown => "key_cooldown",
        }
    }

    #[doc(hidden)]
    pub const fn scheduled_job_message(self) -> &'static str {
        match self {
            Self::ForegroundPressure => "deferred=foreground_pressure",
            Self::RemoteLease => "deferred=remote_lease",
            Self::ReadBudget => "deferred=read_budget",
            Self::ControlDefer => "deferred=control_defer",
            Self::KeyCooldown => "deferred=key_cooldown",
        }
    }

    #[doc(hidden)]
    pub fn from_scheduled_job_message(value: &str) -> Option<Self> {
        match value.strip_prefix("deferred=") {
            Some("foreground_pressure") => Some(Self::ForegroundPressure),
            Some("remote_lease") => Some(Self::RemoteLease),
            Some("read_budget") => Some(Self::ReadBudget),
            Some("control_defer") => Some(Self::ControlDefer),
            Some("key_cooldown") => Some(Self::KeyCooldown),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub const fn preserves_research_wait_anchor(self) -> bool {
        matches!(
            self,
            Self::ForegroundPressure | Self::RemoteLease | Self::ReadBudget | Self::ControlDefer
        )
    }
}

/// Owns an aged reconciliation fairness turn without reserving an outbound
/// request lease. The generation fence prevents an older claimed run from
/// clearing a newer turn after it has started its request.
#[derive(Debug)]
#[doc(hidden)]
pub struct ReconciliationTurn {
    controller: Arc<RemoteAttemptAdmissionController>,
    turn_id: u64,
    kind: ReconciliationTurnKind,
    clear_on_drop: AtomicBool,
}

impl ReconciliationTurn {
    #[doc(hidden)]
    pub fn kind(&self) -> ReconciliationTurnKind {
        self.kind
    }

    /// Preserve an aged Research reservation after its claim-fenced
    /// continuation was accepted. The following eligible run resumes this
    /// reservation, so a lease-only defer cannot let another automatic job
    /// consume the next outbound request opportunity.
    #[doc(hidden)]
    pub fn retain_for_continuation(&self) {
        let mut state = self
            .controller
            .reconciliation_turn
            .lock()
            .expect("reconciliation turn state lock is not poisoned");
        if state.id == self.turn_id {
            state.resumable = true;
            self.clear_on_drop.store(false, Ordering::Release);
        }
    }

    #[doc(hidden)]
    pub async fn acquire_attempt(&self) -> Result<RemoteAttemptLease, &'static str> {
        let active_turn_id = self.controller.reconciliation_turn_id();
        if active_turn_id == 0 {
            return self.controller.acquire_reconciliation_attempt().await;
        }
        if active_turn_id != self.turn_id {
            return Err("reconciliation_turn_stale");
        }
        self.controller
            .acquire_reconciliation_attempt_for_turn(self.turn_id)
            .await
    }

    #[doc(hidden)]
    pub fn try_acquire_attempt(&self) -> Result<RemoteAttemptLease, &'static str> {
        let active_turn_id = self.controller.reconciliation_turn_id();
        if active_turn_id == 0 {
            return self.controller.try_acquire_automatic_attempt();
        }
        if active_turn_id != self.turn_id {
            return Err("reconciliation_turn_stale");
        }
        self.controller
            .try_acquire_reconciliation_attempt_for_turn(self.turn_id)
    }
}

impl Default for RemoteAttemptAdmissionController {
    fn default() -> Self {
        Self {
            attempt_slot: Arc::new(Semaphore::new(1)),
            reconciliation_turn: Mutex::new(ReconciliationTurnState::default()),
            next_reconciliation_turn_id: AtomicU64::new(1),
            reconciliation_turn_cleared: Notify::new(),
            active_attempts: AtomicUsize::new(0),
            peak_active_attempts: AtomicUsize::new(0),
            total_wait_ms: AtomicU64::new(0),
            total_hold_ms: AtomicU64::new(0),
        }
    }
}

impl RemoteAttemptAdmissionController {
    pub fn reserve_aged_reconciliation_turn(self: &Arc<Self>) -> Option<ReconciliationTurn> {
        self.reserve_aged_turn(ReconciliationTurnKind::Main)
    }

    pub fn reserve_aged_research_drain_turn(self: &Arc<Self>) -> Option<ReconciliationTurn> {
        self.reserve_aged_turn(ReconciliationTurnKind::ResearchDrain)
    }

    fn reserve_aged_turn(
        self: &Arc<Self>,
        kind: ReconciliationTurnKind,
    ) -> Option<ReconciliationTurn> {
        let turn_id = {
            let mut state = self
                .reconciliation_turn
                .lock()
                .expect("reconciliation turn state lock is not poisoned");
            if state.id != 0 {
                if state.kind == Some(kind) && state.resumable {
                    state.resumable = false;
                    state.id
                } else {
                    return None;
                }
            } else {
                let turn_id = self
                    .next_reconciliation_turn_id
                    .fetch_add(1, Ordering::AcqRel)
                    .max(1);
                state.id = turn_id;
                state.kind = Some(kind);
                state.resumable = false;
                turn_id
            }
        };
        Some(ReconciliationTurn {
            controller: self.clone(),
            turn_id,
            kind,
            clear_on_drop: AtomicBool::new(true),
        })
    }

    fn reconciliation_turn_id(&self) -> u64 {
        self.reconciliation_turn
            .lock()
            .expect("reconciliation turn state lock is not poisoned")
            .id
    }

    /// Returns a fairness reservation that has been durably continued after a
    /// no-request defer. It is intentionally unavailable while an active run
    /// or a pending request lease owns the turn.
    #[doc(hidden)]
    pub fn resumable_reconciliation_turn_kind(&self) -> Option<ReconciliationTurnKind> {
        let state = self
            .reconciliation_turn
            .lock()
            .expect("reconciliation turn state lock is not poisoned");
        state
            .resumable
            .then_some(state.kind?)
            .filter(|_| state.id != 0)
    }

    pub fn reconciliation_turn_required(&self) -> bool {
        self.reconciliation_turn_id() != 0
    }

    fn clear_reconciliation_turn(&self, turn_id: u64) {
        let cleared = {
            let mut state = self
                .reconciliation_turn
                .lock()
                .expect("reconciliation turn state lock is not poisoned");
            if state.id != turn_id {
                false
            } else {
                *state = ReconciliationTurnState::default();
                true
            }
        };
        if cleared {
            self.reconciliation_turn_cleared.notify_waiters();
        }
    }

    pub async fn acquire_attempt(self: &Arc<Self>) -> Result<RemoteAttemptLease, &'static str> {
        loop {
            while self.reconciliation_turn_required() {
                let cleared = self.reconciliation_turn_cleared.notified();
                if self.reconciliation_turn_required() {
                    cleared.await;
                }
            }
            let lease = self.acquire_attempt_inner().await?;
            if !self.reconciliation_turn_required() {
                return Ok(lease);
            }
            drop(lease);
        }
    }

    /// Foreground-triggered work bypasses an automatic reconciliation turn but
    /// still shares the single actual-request lease.
    pub async fn acquire_manual_attempt(
        self: &Arc<Self>,
    ) -> Result<RemoteAttemptLease, &'static str> {
        self.acquire_attempt_inner().await
    }

    pub async fn acquire_reconciliation_attempt(
        self: &Arc<Self>,
    ) -> Result<RemoteAttemptLease, &'static str> {
        self.acquire_attempt().await
    }

    /// Tries an automatic request lease without waiting behind another
    /// request. Research drain uses this to turn lease contention into a
    /// durable short defer instead of spending its whole preparation budget.
    pub fn try_acquire_automatic_attempt(
        self: &Arc<Self>,
    ) -> Result<RemoteAttemptLease, &'static str> {
        if self.reconciliation_turn_required() {
            return Err("remote_lease_unavailable");
        }
        self.try_acquire_attempt_inner()
    }

    async fn acquire_reconciliation_attempt_for_turn(
        self: &Arc<Self>,
        turn_id: u64,
    ) -> Result<RemoteAttemptLease, &'static str> {
        let waiting_started_at = Instant::now();
        let permit = self
            .attempt_slot
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "remote_attempt_admission_closed")?;
        if self.reconciliation_turn_id() != turn_id {
            drop(permit);
            return Err("reconciliation_turn_stale");
        }
        self.total_wait_ms.fetch_add(
            waiting_started_at
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        let active_attempts = self.active_attempts.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak_active_attempts
            .fetch_max(active_attempts, Ordering::AcqRel);
        Ok(RemoteAttemptLease {
            controller: self.clone(),
            _permit: permit,
            reconciliation_turn_id: AtomicU64::new(turn_id),
            started_at: Instant::now(),
        })
    }

    fn try_acquire_reconciliation_attempt_for_turn(
        self: &Arc<Self>,
        turn_id: u64,
    ) -> Result<RemoteAttemptLease, &'static str> {
        let permit = self
            .attempt_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| "remote_lease_unavailable")?;
        if self.reconciliation_turn_id() != turn_id {
            drop(permit);
            return Err("reconciliation_turn_stale");
        }
        Ok(self.make_reconciliation_turn_lease(permit, turn_id))
    }

    fn try_acquire_attempt_inner(self: &Arc<Self>) -> Result<RemoteAttemptLease, &'static str> {
        let permit = self
            .attempt_slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| "remote_lease_unavailable")?;
        Ok(self.make_lease(permit))
    }

    fn make_lease(self: &Arc<Self>, permit: OwnedSemaphorePermit) -> RemoteAttemptLease {
        let active_attempts = self.active_attempts.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak_active_attempts
            .fetch_max(active_attempts, Ordering::AcqRel);
        RemoteAttemptLease {
            controller: self.clone(),
            _permit: permit,
            reconciliation_turn_id: AtomicU64::new(0),
            started_at: Instant::now(),
        }
    }

    fn make_reconciliation_turn_lease(
        self: &Arc<Self>,
        permit: OwnedSemaphorePermit,
        turn_id: u64,
    ) -> RemoteAttemptLease {
        let lease = self.make_lease(permit);
        lease
            .reconciliation_turn_id
            .store(turn_id, Ordering::Release);
        lease
    }

    async fn acquire_attempt_inner(self: &Arc<Self>) -> Result<RemoteAttemptLease, &'static str> {
        let waiting_started_at = Instant::now();
        let permit = self
            .attempt_slot
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "remote_attempt_admission_closed")?;
        self.total_wait_ms.fetch_add(
            waiting_started_at
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        let active_attempts = self.active_attempts.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak_active_attempts
            .fetch_max(active_attempts, Ordering::AcqRel);
        Ok(RemoteAttemptLease {
            controller: self.clone(),
            _permit: permit,
            reconciliation_turn_id: AtomicU64::new(0),
            started_at: Instant::now(),
        })
    }

    pub fn metrics(&self) -> RemoteAttemptMetrics {
        RemoteAttemptMetrics {
            active_attempts: self.active_attempts.load(Ordering::Acquire),
            peak_active_attempts: self.peak_active_attempts.load(Ordering::Acquire),
            total_wait_ms: self.total_wait_ms.load(Ordering::Relaxed),
            total_hold_ms: self.total_hold_ms.load(Ordering::Relaxed),
        }
    }
}

impl Drop for RemoteAttemptLease {
    fn drop(&mut self) {
        self.controller.total_hold_ms.fetch_add(
            self.started_at.elapsed().as_millis().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        self.controller
            .active_attempts
            .fetch_sub(1, Ordering::AcqRel);
    }
}

impl RemoteAttemptLease {
    /// Consumes an aged fairness turn at the first actual request build, not
    /// while local preparation or lease acquisition is still reversible.
    #[doc(hidden)]
    pub fn mark_request_started(&self) {
        let turn_id = self.reconciliation_turn_id.swap(0, Ordering::AcqRel);
        if turn_id != 0 {
            self.controller.clear_reconciliation_turn(turn_id);
        }
    }
}

impl Drop for ReconciliationTurn {
    fn drop(&mut self) {
        if self.clear_on_drop.load(Ordering::Acquire) {
            self.controller.clear_reconciliation_turn(self.turn_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[tokio::test]
    async fn remote_attempt_controller_serves_aged_reconciliation() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        let turn = controller
            .reserve_aged_reconciliation_turn()
            .expect("aged reconciliation reserves a fairness turn");
        let manual_attempt = controller
            .acquire_manual_attempt()
            .await
            .expect("manual attempt");
        assert!(
            controller.reconciliation_turn_required(),
            "a manual attempt must not consume the automatic reconciliation turn"
        );
        drop(manual_attempt);
        // `scheduled_job_mark_running` can lose a claim race after dispatch
        // selection. Dropping its turn must release the fairness turn.
        drop(turn);
        assert!(
            !controller.reconciliation_turn_required(),
            "a claimed run that defers before HTTP releases its fairness turn"
        );
        let old_turn = controller
            .reserve_aged_reconciliation_turn()
            .expect("reconciliation owns the next automatic turn");
        assert!(
            controller.acquire_attempt().now_or_never().is_none(),
            "ordinary automatic work waits while reconciliation owns the turn"
        );
        let lease = old_turn.acquire_attempt().await.expect("attempt lease");
        assert!(
            controller.reconciliation_turn_required(),
            "acquiring a lease alone must not consume the fairness turn"
        );
        lease.mark_request_started();
        assert!(
            !controller.reconciliation_turn_required(),
            "the first actual request build consumes the fairness turn"
        );
        let next_turn = controller
            .reserve_aged_reconciliation_turn()
            .expect("a completed request permits a later turn");
        drop(old_turn);
        assert!(
            controller.reconciliation_turn_required(),
            "an old turn guard cannot clear a newer reconciliation turn"
        );
        drop(next_turn);
        assert_eq!(controller.metrics().active_attempts, 1);
        assert!(controller.acquire_attempt().now_or_never().is_none());
        drop(lease);
        assert_eq!(controller.metrics().active_attempts, 0);
        assert_eq!(controller.metrics().peak_active_attempts, 1);
    }

    #[tokio::test]
    async fn reconciliation_remote_lease_is_request_scoped() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());

        // Candidate selection and projection run before an outbound request.
        assert_eq!(controller.metrics().active_attempts, 0);
        let request = controller
            .acquire_reconciliation_attempt()
            .await
            .expect("outbound request acquires the only remote lease");
        assert_eq!(controller.metrics().active_attempts, 1);

        // Response parsing and durable finalization begin only after the HTTP
        // lease is released, so they cannot block another outbound attempt.
        drop(request);
        assert_eq!(controller.metrics().active_attempts, 0);
        let other_request = controller
            .acquire_attempt()
            .await
            .expect("another remote request starts after response handling");
        assert_eq!(controller.metrics().peak_active_attempts, 1);
        drop(other_request);
    }

    #[tokio::test]
    async fn remote_attempt_controller_fairly_serves_aged_research() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        let turn = controller
            .reserve_aged_research_drain_turn()
            .expect("aged Research reserves the next automatic turn");
        assert_eq!(turn.kind(), ReconciliationTurnKind::ResearchDrain);

        let manual = controller
            .acquire_manual_attempt()
            .await
            .expect("manual work stays ahead of an automatic turn");
        assert!(
            controller.acquire_attempt().now_or_never().is_none(),
            "ordinary automatic work waits for the aged Research turn"
        );
        drop(manual);

        let lease = turn
            .acquire_attempt()
            .await
            .expect("aged Research receives the next automatic request");
        assert_eq!(controller.metrics().peak_active_attempts, 1);
        drop(lease);
    }

    #[tokio::test]
    async fn research_turn_reports_busy_lease_without_consuming_its_turn() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        let held = controller
            .acquire_manual_attempt()
            .await
            .expect("manual request occupies the only lease");
        let turn = controller
            .reserve_aged_research_drain_turn()
            .expect("Research owns the next automatic turn");

        assert!(matches!(
            turn.try_acquire_attempt(),
            Err("remote_lease_unavailable")
        ));
        assert!(
            controller.reconciliation_turn_required(),
            "a lease defer must not consume the aged Research turn"
        );
        drop(held);
        let lease = turn
            .try_acquire_attempt()
            .expect("the same turn acquires the lease once it is free");
        assert!(controller.reconciliation_turn_required());
        lease.mark_request_started();
        assert!(!controller.reconciliation_turn_required());
        drop(lease);
    }

    #[tokio::test]
    async fn retained_research_turn_blocks_other_automatic_work_until_request_starts() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        let held = controller
            .acquire_manual_attempt()
            .await
            .expect("manual request occupies the only lease");
        let turn = controller
            .reserve_aged_research_drain_turn()
            .expect("aged Research owns the next automatic turn");

        assert!(matches!(
            turn.try_acquire_attempt(),
            Err("remote_lease_unavailable")
        ));
        // This mirrors the scheduler after its claim-fenced defer and unique
        // five-second continuation were accepted.
        turn.retain_for_continuation();
        drop(turn);
        assert_eq!(
            controller.resumable_reconciliation_turn_kind(),
            Some(ReconciliationTurnKind::ResearchDrain)
        );

        drop(held);
        assert!(matches!(
            controller.try_acquire_automatic_attempt(),
            Err("remote_lease_unavailable")
        ));
        let resumed = controller
            .reserve_aged_research_drain_turn()
            .expect("the durable continuation reclaims its aged reservation");
        let lease = resumed
            .try_acquire_attempt()
            .expect("Research gets the next automatic lease");
        assert!(controller.reconciliation_turn_required());
        lease.mark_request_started();
        assert!(!controller.reconciliation_turn_required());
        drop(lease);
    }

    #[tokio::test]
    async fn lease_without_request_does_not_consume_an_aged_turn() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        let turn = controller
            .reserve_aged_research_drain_turn()
            .expect("aged Research owns a turn");
        let lease = turn
            .try_acquire_attempt()
            .expect("local preparation acquires a request-scoped lease");

        assert!(controller.reconciliation_turn_required());
        drop(lease);
        assert!(
            controller.reconciliation_turn_required(),
            "a pre-request deadline cannot consume the fairness turn"
        );
        drop(turn);
        assert!(
            !controller.reconciliation_turn_required(),
            "a no-request path that was not durably continued releases its turn"
        );
    }

    #[test]
    fn concurrent_turn_clear_and_reserve_keeps_new_research_state_intact() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        let old_turn = controller
            .reserve_aged_reconciliation_turn()
            .expect("the old Main turn reserves a generation");
        let old_turn_id = old_turn.turn_id;
        let start = Arc::new(Barrier::new(2));

        std::thread::scope(|scope| {
            let clearer = controller.clone();
            let clear_start = start.clone();
            scope.spawn(move || {
                clear_start.wait();
                clearer.clear_reconciliation_turn(old_turn_id);
            });

            let reserver = controller.clone();
            scope.spawn(move || {
                start.wait();
                loop {
                    if let Some(turn) = reserver.reserve_aged_research_drain_turn() {
                        turn.retain_for_continuation();
                        break;
                    }
                    std::thread::yield_now();
                }
            });
        });

        drop(old_turn);
        assert_eq!(
            controller.resumable_reconciliation_turn_kind(),
            Some(ReconciliationTurnKind::ResearchDrain),
            "clearing an old turn cannot reset a newly reserved Research kind"
        );
        let resumed = controller
            .reserve_aged_research_drain_turn()
            .expect("the new Research reservation remains resumable");
        drop(resumed);
    }
}

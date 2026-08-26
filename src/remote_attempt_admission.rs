use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
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
    reconciliation_turn_id: AtomicU64,
    next_reconciliation_turn_id: AtomicU64,
    reconciliation_turn_cleared: Notify,
    active_attempts: AtomicUsize,
    peak_active_attempts: AtomicUsize,
    total_wait_ms: AtomicU64,
    total_hold_ms: AtomicU64,
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
    started_at: Instant,
}

/// Owns an aged reconciliation fairness turn without reserving an outbound
/// request lease. The generation fence prevents an older claimed run from
/// clearing a newer turn after it has started its request.
#[derive(Debug)]
#[doc(hidden)]
pub struct ReconciliationTurn {
    controller: Arc<RemoteAttemptAdmissionController>,
    turn_id: u64,
}

impl ReconciliationTurn {
    #[doc(hidden)]
    pub async fn acquire_attempt(&self) -> Result<RemoteAttemptLease, &'static str> {
        let active_turn_id = self
            .controller
            .reconciliation_turn_id
            .load(Ordering::Acquire);
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
}

impl Default for RemoteAttemptAdmissionController {
    fn default() -> Self {
        Self {
            attempt_slot: Arc::new(Semaphore::new(1)),
            reconciliation_turn_id: AtomicU64::new(0),
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
        let turn_id = self
            .next_reconciliation_turn_id
            .fetch_add(1, Ordering::AcqRel)
            .max(1);
        self.reconciliation_turn_id
            .compare_exchange(0, turn_id, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(ReconciliationTurn {
            controller: self.clone(),
            turn_id,
        })
    }

    pub fn reconciliation_turn_required(&self) -> bool {
        self.reconciliation_turn_id.load(Ordering::Acquire) != 0
    }

    fn clear_reconciliation_turn(&self, turn_id: u64) {
        if self
            .reconciliation_turn_id
            .compare_exchange(turn_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
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
        self.total_wait_ms.fetch_add(
            waiting_started_at
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
        self.reconciliation_turn_id
            .compare_exchange(turn_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "reconciliation_turn_stale")?;
        self.reconciliation_turn_cleared.notify_waiters();
        let active_attempts = self.active_attempts.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak_active_attempts
            .fetch_max(active_attempts, Ordering::AcqRel);
        Ok(RemoteAttemptLease {
            controller: self.clone(),
            _permit: permit,
            started_at: Instant::now(),
        })
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

impl Drop for ReconciliationTurn {
    fn drop(&mut self) {
        self.controller.clear_reconciliation_turn(self.turn_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!controller.reconciliation_turn_required());
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
}

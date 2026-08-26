use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

#[cfg(test)]
use futures_util::FutureExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Separates one-at-a-time maintenance dispatch from the lease that protects an
/// actual outbound upstream request. Local SQLite preparation must not consume
/// the latter.
#[derive(Debug)]
#[doc(hidden)]
pub struct RemoteAttemptAdmissionController {
    dispatch_slot: Arc<Semaphore>,
    attempt_slot: Arc<Semaphore>,
    reconciliation_turn_required: AtomicBool,
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

/// Bounds scheduler task dispatch without representing an outbound request.
/// The reconciliation variant owns the fairness turn, so every early return
/// after a claim releases that turn automatically.
#[derive(Debug)]
#[doc(hidden)]
pub enum RemoteJobDispatchPermit {
    Standard(OwnedSemaphorePermit),
    Reconciliation(ReconciliationDispatchPermit),
}

#[derive(Debug)]
#[doc(hidden)]
pub struct ReconciliationDispatchPermit {
    controller: Arc<RemoteAttemptAdmissionController>,
    _permit: OwnedSemaphorePermit,
}

impl Default for RemoteAttemptAdmissionController {
    fn default() -> Self {
        Self {
            dispatch_slot: Arc::new(Semaphore::new(1)),
            attempt_slot: Arc::new(Semaphore::new(1)),
            reconciliation_turn_required: AtomicBool::new(false),
            active_attempts: AtomicUsize::new(0),
            peak_active_attempts: AtomicUsize::new(0),
            total_wait_ms: AtomicU64::new(0),
            total_hold_ms: AtomicU64::new(0),
        }
    }
}

impl RemoteAttemptAdmissionController {
    pub fn try_acquire_manual_dispatch(&self) -> Option<RemoteJobDispatchPermit> {
        self.dispatch_slot
            .clone()
            .try_acquire_owned()
            .ok()
            .map(RemoteJobDispatchPermit::Standard)
    }

    pub fn try_acquire_reconciliation_dispatch(&self) -> Option<RemoteJobDispatchPermit> {
        self.dispatch_slot
            .clone()
            .try_acquire_owned()
            .ok()
            .map(RemoteJobDispatchPermit::Standard)
    }

    pub fn try_acquire_aged_reconciliation_dispatch(
        self: &Arc<Self>,
    ) -> Option<RemoteJobDispatchPermit> {
        let permit = self.dispatch_slot.clone().try_acquire_owned().ok()?;
        self.require_reconciliation_turn();
        Some(RemoteJobDispatchPermit::Reconciliation(
            ReconciliationDispatchPermit {
                controller: self.clone(),
                _permit: permit,
            },
        ))
    }

    pub fn try_acquire_nonmanual_dispatch(&self) -> Option<RemoteJobDispatchPermit> {
        if self.reconciliation_turn_required() {
            return None;
        }
        self.dispatch_slot
            .clone()
            .try_acquire_owned()
            .ok()
            .map(RemoteJobDispatchPermit::Standard)
    }

    pub fn require_reconciliation_turn(&self) {
        self.reconciliation_turn_required
            .store(true, Ordering::Release);
    }

    pub fn reconciliation_turn_required(&self) -> bool {
        self.reconciliation_turn_required.load(Ordering::Acquire)
    }

    pub fn clear_reconciliation_turn(&self) {
        self.reconciliation_turn_required
            .store(false, Ordering::Release);
    }

    pub async fn acquire_attempt(self: &Arc<Self>) -> Result<RemoteAttemptLease, &'static str> {
        self.acquire_attempt_inner(false).await
    }

    pub async fn acquire_reconciliation_attempt(
        self: &Arc<Self>,
    ) -> Result<RemoteAttemptLease, &'static str> {
        self.acquire_attempt_inner(true).await
    }

    async fn acquire_attempt_inner(
        self: &Arc<Self>,
        reconciliation_attempt: bool,
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
        if reconciliation_attempt {
            self.reconciliation_turn_required
                .store(false, Ordering::Release);
        }
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

impl Drop for ReconciliationDispatchPermit {
    fn drop(&mut self) {
        self.controller.clear_reconciliation_turn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remote_attempt_controller_serves_aged_reconciliation() {
        let controller = Arc::new(RemoteAttemptAdmissionController::default());
        controller.require_reconciliation_turn();
        assert!(
            controller.try_acquire_nonmanual_dispatch().is_none(),
            "an aged reconciliation turn blocks later automatic remote dispatch"
        );
        let manual_dispatch = controller
            .try_acquire_manual_dispatch()
            .expect("manual work keeps dispatch priority");
        drop(manual_dispatch);
        let manual_attempt = controller.acquire_attempt().await.expect("manual attempt");
        assert!(
            controller.reconciliation_turn_required(),
            "a manual attempt must not consume the automatic reconciliation turn"
        );
        drop(manual_attempt);
        let claim_race_dispatch = controller
            .try_acquire_aged_reconciliation_dispatch()
            .expect("reconciliation owns the next automatic dispatch");
        assert!(controller.reconciliation_turn_required());
        // `scheduled_job_mark_running` can lose a claim race after dispatch
        // admission. Dropping its permit must release the fairness turn.
        drop(claim_race_dispatch);
        assert!(
            !controller.reconciliation_turn_required(),
            "a claimed run that defers before HTTP releases its fairness turn"
        );
        controller.require_reconciliation_turn();
        let lease = controller
            .acquire_reconciliation_attempt()
            .await
            .expect("attempt lease");
        assert!(!controller.reconciliation_turn_required());
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

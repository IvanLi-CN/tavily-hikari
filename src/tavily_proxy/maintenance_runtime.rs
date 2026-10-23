/// Owns the bounded reconciliation continuation loop for one proxy runtime.
///
/// Request paths publish only a generation hint. This owner coalesces those
/// hints for one second, rebuilds the durable representatives at startup and
/// every watchdog interval, and wakes the existing maintenance worker only
/// after the control enqueue has completed.
#[derive(Clone, Debug)]
pub(crate) struct MaintenanceRuntime {
    state: std::sync::Arc<MaintenanceRuntimeState>,
    store: std::sync::Weak<KeyStore>,
    hint: std::sync::Arc<UpstreamReconciliationMaintenanceHint>,
    backend_time: BackendTime,
}

#[derive(Debug)]
struct MaintenanceRuntimeState {
    active: std::sync::atomic::AtomicBool,
    shutdown: std::sync::atomic::AtomicBool,
    worker_started: std::sync::atomic::AtomicBool,
    wake: tokio::sync::Notify,
    worker_wake: std::sync::Mutex<Option<std::sync::Arc<tokio::sync::Notify>>>,
}

impl MaintenanceRuntime {
    pub(crate) fn new(
        store: std::sync::Weak<KeyStore>,
        hint: std::sync::Arc<UpstreamReconciliationMaintenanceHint>,
        backend_time: BackendTime,
    ) -> Self {
        Self {
            state: std::sync::Arc::new(MaintenanceRuntimeState {
                active: std::sync::atomic::AtomicBool::new(false),
                shutdown: std::sync::atomic::AtomicBool::new(false),
                worker_started: std::sync::atomic::AtomicBool::new(false),
                wake: tokio::sync::Notify::new(),
                worker_wake: std::sync::Mutex::new(None),
            }),
            store,
            hint,
            backend_time,
        }
    }

    pub(crate) fn start_for_writable_tenure(
        &self,
        worker_wake: std::sync::Arc<tokio::sync::Notify>,
    ) {
        if self.state.shutdown.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        *self
            .state
            .worker_wake
            .lock()
            .expect("maintenance runtime worker wake lock") = Some(worker_wake);
        self.state
            .active
            .store(true, std::sync::atomic::Ordering::Release);
        if self
            .state
            .worker_started
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok()
        {
            let runtime = self.clone();
            tokio::spawn(async move { runtime.run().await });
        }
        self.state.wake.notify_one();
    }

    pub(crate) fn stop_for_writable_tenure(&self) {
        self.state
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        self.state.wake.notify_waiters();
    }

    pub(crate) fn shutdown(&self) {
        self.state
            .active
            .store(false, std::sync::atomic::Ordering::Release);
        self.state
            .shutdown
            .store(true, std::sync::atomic::Ordering::Release);
        self.state.wake.notify_waiters();
    }

    pub(crate) fn mark_dirty(&self) -> u64 {
        self.hint.mark_dirty()
    }

    async fn run(self) {
        let mut seen_generation = self.hint.generation();
        let mut force_rebuild = true;
        let mut retry_pending = false;
        loop {
            if self
                .state
                .shutdown
                .load(std::sync::atomic::Ordering::Acquire)
            {
                break;
            }
            if !self
                .state
                .active
                .load(std::sync::atomic::Ordering::Acquire)
            {
                tokio::select! {
                    _ = self.state.wake.notified() => {}
                    _ = self.hint.wait() => {}
                }
                continue;
            }

            let observed_generation = self.hint.generation();
            if force_rebuild || retry_pending || observed_generation != seen_generation {
                if !force_rebuild {
                    self
                        .backend_time
                        .sleep(std::time::Duration::from_secs(1))
                        .await;
                    if !self
                        .state
                        .active
                        .load(std::sync::atomic::Ordering::Acquire)
                    {
                        continue;
                    }
                }
                let Some(store) = self.store.upgrade() else {
                    break;
                };
                let enqueue_result = store
                    .ensure_upstream_reconciliation_maintenance_jobs()
                    .await;
                let enqueue_ok = enqueue_result.is_ok();
                if let Err(err) = enqueue_result {
                    tracing::debug!(
                        component = "reconciliation",
                        event = "maintenance_hint_enqueue_failed",
                        err = %err,
                    );
                }
                apply_enqueue_result(
                    &mut seen_generation,
                    &mut retry_pending,
                    observed_generation,
                    enqueue_ok,
                );
                force_rebuild = false;
                if enqueue_ok
                    && self
                        .state
                        .active
                        .load(std::sync::atomic::Ordering::Acquire)
                    && let Some(worker_wake) = self
                        .state
                        .worker_wake
                        .lock()
                        .expect("maintenance runtime worker wake lock")
                        .as_ref()
                {
                    worker_wake.notify_one();
                }
                continue;
            }

            tokio::select! {
                _ = self.state.wake.notified() => {
                    force_rebuild = true;
                }
                _ = self.hint.wait() => {}
                _ = self
                    .backend_time
                    .sleep(std::time::Duration::from_secs(30)) => {
                    force_rebuild = true;
                }
            }
        }
        self.state
            .worker_started
            .store(false, std::sync::atomic::Ordering::Release);
    }
}

fn apply_enqueue_result(
    seen_generation: &mut u64,
    retry_pending: &mut bool,
    observed_generation: u64,
    enqueue_ok: bool,
) {
    if enqueue_ok {
        // A generation published while the enqueue was in flight must remain dirty for
        // the next coalesced pass. Only acknowledge the generation captured before await.
        *seen_generation = observed_generation;
        *retry_pending = false;
    } else {
        // Keep the owner eligible for a bounded retry even when no newer hint arrives.
        *retry_pending = true;
    }
}

#[cfg(test)]
mod maintenance_runtime_tests {
    use super::*;

    #[test]
    fn enqueue_success_preserves_generation_published_during_await() {
        let hint = std::sync::Arc::new(UpstreamReconciliationMaintenanceHint::default());
        let mut seen_generation = hint.mark_dirty();
        let observed_generation = seen_generation;
        assert_eq!(hint.mark_dirty(), observed_generation + 1);
        let mut retry_pending = false;

        apply_enqueue_result(
            &mut seen_generation,
            &mut retry_pending,
            observed_generation,
            true,
        );

        assert_eq!(seen_generation, observed_generation);
        assert!(hint.generation() > seen_generation);
        assert!(!retry_pending);
    }

    #[test]
    fn enqueue_failure_remains_pending_without_a_new_generation() {
        let hint = std::sync::Arc::new(UpstreamReconciliationMaintenanceHint::default());
        let mut seen_generation = hint.mark_dirty();
        let observed_generation = seen_generation;
        let mut retry_pending = false;

        apply_enqueue_result(
            &mut seen_generation,
            &mut retry_pending,
            observed_generation,
            false,
        );

        assert_eq!(seen_generation, observed_generation);
        assert!(retry_pending);
    }
}

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

            let generation = self.hint.generation();
            if force_rebuild || generation != seen_generation {
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
                seen_generation = self.hint.generation();
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

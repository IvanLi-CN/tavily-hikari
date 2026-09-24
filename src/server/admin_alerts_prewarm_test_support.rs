#[derive(Debug, Clone)]
pub(crate) struct AdminAlertsWarmPause {
    arrived: Arc<std::sync::atomic::AtomicBool>,
    arrived_notify: Arc<Notify>,
    released: Arc<std::sync::atomic::AtomicBool>,
    release: Arc<Notify>,
}

impl AdminAlertsWarmPause {
    pub(crate) fn new() -> Self {
        Self {
            arrived: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            arrived_notify: Arc::new(Notify::new()),
            released: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            release: Arc::new(Notify::new()),
        }
    }

    pub(crate) async fn wait_until_arrived(&self) {
        loop {
            if self.arrived.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            let notified = self.arrived_notify.notified();
            if self.arrived.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    pub(crate) fn release(&self) {
        self.released
            .store(true, std::sync::atomic::Ordering::Release);
        self.release.notify_waiters();
    }
}

pub(crate) async fn install_admin_alerts_warm_after_catalog_pause_for_test(
    state: &AppState,
) -> AdminAlertsWarmPause {
    let pause = AdminAlertsWarmPause::new();
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_after_catalog_pause = Some(pause.clone());
    pause
}
#[cfg(test)]
pub(crate) async fn install_admin_alerts_warm_after_groups_pause_for_test(
    state: &AppState,
) -> AdminAlertsWarmPause {
    let pause = AdminAlertsWarmPause::new();
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_after_groups_pause = Some(pause.clone());
    pause
}

#[cfg(test)]
pub(crate) async fn install_admin_alerts_warm_before_projection_fence_pause_for_test(
    state: &AppState,
) -> AdminAlertsWarmPause {
    let pause = AdminAlertsWarmPause::new();
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_before_projection_fence_pause = Some(pause.clone());
    pause
}

#[cfg(test)]
pub(crate) async fn install_admin_alerts_warm_after_projection_fence_pause_for_test(
    state: &AppState,
) -> AdminAlertsWarmPause {
    let pause = AdminAlertsWarmPause::new();
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_after_projection_fence_pause = Some(pause.clone());
    pause
}

#[cfg(test)]
pub(crate) async fn install_admin_alerts_warm_after_groups_defer_pause_for_test(
    state: &AppState,
) -> AdminAlertsWarmPause {
    let pause = AdminAlertsWarmPause::new();
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_after_groups_defer_pause = Some(pause.clone());
    pause
}

#[cfg(test)]
pub(crate) async fn install_admin_alerts_warm_after_groups_source_fence_changed_pause_for_test(
    state: &AppState,
) -> AdminAlertsWarmPause {
    let pause = AdminAlertsWarmPause::new();
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_after_groups_source_fence_changed_pause = Some(pause.clone());
    pause
}

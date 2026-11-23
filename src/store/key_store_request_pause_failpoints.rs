// Request-stats pause failpoints shared by the store and its integration tests.
//
// The helpers are compiled away outside test and debug builds; they live here to hold the
// module hub within its line budget.

#[cfg(any(test, debug_assertions))]
fn new_request_stats_test_pause() -> RequestStatsPostFlushPause {
    RequestStatsPostFlushPause {
        arrived: Arc::new(Notify::new()),
        arrival_observed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        release: Arc::new(Notify::new()),
        released: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }
}

#[cfg(any(test, debug_assertions))]
async fn wait_for_request_stats_test_pause_if_installed(
    slot: &Arc<Mutex<Option<RequestStatsPostFlushPause>>>,
) {
    if let Some(pause) = slot.lock().await.take() {
        // Retain the arrival signal when the request returns before the test waits.
        pause
            .arrival_observed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        pause.arrived.notify_one();
        while !pause.released.load(std::sync::atomic::Ordering::SeqCst) {
            pause.release.notified().await;
        }
    }
}

impl KeyStore {
    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    pub(crate) async fn install_dashboard_overview_read_pause(&self) -> RequestStatsPostFlushPause {
        let pause = new_request_stats_test_pause();
        let mut slot = self.dashboard_overview_read_pause.lock().await;
        *slot = Some(pause.clone());
        pause
    }

    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    pub(crate) async fn wait_for_dashboard_overview_read_pause_if_installed(&self) {
        wait_for_request_stats_test_pause_if_installed(&self.dashboard_overview_read_pause).await;
    }

    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    pub(crate) async fn install_admin_privacy_read_pause(&self) -> RequestStatsPostFlushPause {
        let pause = new_request_stats_test_pause();
        let mut slot = self.admin_privacy_read_pause.lock().await;
        *slot = Some(pause.clone());
        pause
    }

    #[cfg(debug_assertions)]
    #[allow(dead_code)]
    pub(crate) async fn wait_for_admin_privacy_read_pause_if_installed(&self) {
        wait_for_request_stats_test_pause_if_installed(&self.admin_privacy_read_pause).await;
    }
}

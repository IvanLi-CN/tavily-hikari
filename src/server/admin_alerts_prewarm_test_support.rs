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

async fn wait_for_admin_alerts_shutdown_or(
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
    shutdown_notify: &Arc<tokio::sync::Notify>,
    delay: std::time::Duration,
) -> bool {
    let notified = shutdown_notify.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    if admin_alerts_shutdown_requested(cache).await {
        return true;
    }
    tokio::select! {
        _ = tokio::time::sleep(delay) => admin_alerts_shutdown_requested(cache).await,
        _ = &mut notified => true,
    }
}

async fn admin_alerts_shutdown_requested(
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
) -> bool {
    cache.lock().await.admin_alerts_shutting_down
}

async fn wait_for_admin_alerts_projection_or_shutdown(
    state: &AppState,
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
    shutdown_notify: &Arc<tokio::sync::Notify>,
    fallback_delay: std::time::Duration,
) -> bool {
    let notified = shutdown_notify.notified();
    tokio::pin!(notified);
    notified.as_mut().enable();
    let projection_done = state
        .proxy
        .wait_for_admin_alerts_cache_warm_projection_turn();
    tokio::pin!(projection_done);
    if admin_alerts_shutdown_requested(cache).await {
        return true;
    }
    tokio::select! {
        _ = &mut projection_done => false,
        _ = tokio::time::sleep(fallback_delay) => false,
        _ = &mut notified => true,
    }
}

async fn admin_alerts_canonical_groups_for_warm_liveness_stage(
    state: &AppState,
) -> Result<(PaginatedAlertGroups, i64, i64, i64), tavily_hikari::ProxyError> {
    loop {
        match admin_alerts_canonical_groups_for_warm(state).await {
            Err(tavily_hikari::ProxyError::Deferred { reason, .. })
                if reason == "groups_build_in_progress" =>
            {
                // Coverage was established before the Groups snapshot started.
                // Keep the liveness stage owned across its bounded slices so a
                // projection advance cannot invalidate the staged generation
                // before the canonical three-key publish. Projection resumes
                // after the stage publishes or yields for a real retry reason.
                #[cfg(test)]
                pause_admin_alerts_warm_after_groups_defer_for_test(state).await;
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}

async fn admin_alert_catalog_for_canonical_snapshot_liveness_stage(
    state: &AppState,
    build_generation: i64,
) -> Result<tavily_hikari::AlertCatalog, tavily_hikari::ProxyError> {
    loop {
        match state
            .proxy
            .admin_alert_catalog_for_canonical_snapshot(build_generation)
            .await
        {
            Err(tavily_hikari::ProxyError::Deferred { reason, .. })
                if reason == "catalog_build_in_progress"
                    || reason == "catalog_payload_build_in_progress" =>
            {
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}

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

async fn reacquire_admin_alerts_liveness_stage_or_shutdown(
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
    shutdown_notify: &Arc<tokio::sync::Notify>,
    proxy: &TavilyProxy,
) -> bool {
    if wait_for_admin_alerts_shutdown_or(
        cache,
        shutdown_notify,
        std::time::Duration::from_secs(5),
    )
    .await
    {
        return true;
    }
    proxy.set_admin_alerts_cache_warm_liveness(true);
    proxy.begin_admin_alerts_cache_warm_liveness_stage();
    false
}

async fn admin_alerts_shutdown_requested(
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
) -> bool {
    cache.lock().await.admin_alerts_shutting_down
}

async fn admin_alerts_canonical_groups_for_warm_liveness_stage(
    state: &AppState,
) -> Result<(PaginatedAlertGroups, i64, i64, i64), tavily_hikari::ProxyError> {
    loop {
        match admin_alerts_canonical_groups_for_warm(state).await {
            Err(tavily_hikari::ProxyError::Deferred { reason, .. })
                if reason == "groups_build_in_progress" =>
            {
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

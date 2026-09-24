fn admin_alerts_warm_deferred(reason: &'static str) -> tavily_hikari::ProxyError {
    tavily_hikari::ProxyError::Deferred {
        operation: "admin_alerts_warm",
        reason: reason.to_string(),
    }
}

fn admin_alerts_warm_retry_delay(
    liveness_slot: bool,
    delay: std::time::Duration,
) -> std::time::Duration {
    if liveness_slot {
        delay.min(ADMIN_ALERTS_LIVENESS_RETRY_MAX_INTERVAL)
    } else {
        delay
    }
}

fn admin_alerts_warm_error_reason(error: &tavily_hikari::ProxyError) -> &'static str {
    let tavily_hikari::ProxyError::Deferred { reason, .. } = error else {
        return "sqlite_pressure";
    };
    match reason.as_str() {
        "foreground_pressure" => "foreground_pressure",
        "pool_pressure" => "pool_pressure",
        "recent_contention" => "recent_contention",
        "projection_fence_changed" => "projection_fence_changed",
        "projection_generation_changed" => "projection_generation_changed",
        "read_budget" => "read_budget",
        _ => "deferred",
    }
}

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

#[cfg(test)]
async fn pause_admin_alerts_warm_after_projection_fence_for_test(state: &AppState) {
    let pause = dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .admin_alerts_warm_after_projection_fence_pause
        .take();
    let Some(pause) = pause else {
        return;
    };
    pause_admin_alerts_warm_for_test(pause).await;
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
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
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
                if admin_alerts_shutdown_requested(cache).await {
                    return Err(admin_alerts_warm_deferred("shutdown"));
                }
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}

async fn admin_alert_catalog_for_canonical_snapshot_liveness_stage(
    state: &AppState,
    build_generation: i64,
    cache: &Arc<Mutex<DashboardOverviewCacheState>>,
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
                if admin_alerts_shutdown_requested(cache).await {
                    return Err(admin_alerts_warm_deferred("shutdown"));
                }
                tokio::task::yield_now().await;
            }
            result => return result,
        }
    }
}

async fn admin_alerts_canonical_groups_for_warm(
    state: &AppState,
) -> Result<(PaginatedAlertGroups, i64, i64, i64), tavily_hikari::ProxyError> {
    let cache = dashboard_overview_cache_for_state(state);
    let owner = {
        let mut cache_state = cache.lock().await;
        let Some(owner) = cache_state.start_admin_alerts_groups_build() else {
            return Err(admin_alerts_warm_deferred("groups_reclaim_busy"));
        };
        owner
    };
    let mut flight_guard = AdminAlertsFlightGuard::new(
        cache.clone(),
        AdminAlertsFlightKind::GroupsBuild,
        owner,
        None,
    );
    let result = state.proxy.admin_alert_canonical_groups_page_for_warm().await;
    // Clear the owner before disarming so cancellation still releases the flag safely.
    cache
        .lock()
        .await
        .finish_admin_alerts_groups_build(owner);
    flight_guard.disarm();
    result
}

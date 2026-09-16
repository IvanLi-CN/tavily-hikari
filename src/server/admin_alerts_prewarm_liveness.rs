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

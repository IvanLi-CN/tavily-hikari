async fn admin_alerts_last_good(
    state: &AppState,
    key: &str,
) -> Option<(AdminAlertsReadCacheValue, i64)> {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    let position = cache
        .admin_alerts
        .entries
        .iter()
        .position(|entry| entry.key == key && entry.stored_at.elapsed() <= ADMIN_ALERTS_CACHE_TTL)?;
    let entry = cache.admin_alerts.entries.remove(position)?;
    let observed_at = entry.observed_at;
    let value = entry.value.clone();
    cache.admin_alerts.entries.push_front(entry);
    Some((value, observed_at))
}

async fn admin_alerts_canonical_last_good(
    state: &AppState,
    key: &str,
) -> Option<(AdminAlertsReadCacheValue, i64, u64, u64)> {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    let position = cache.admin_alerts.entries.iter().position(|entry| {
        entry.key == key && entry.canonical && entry.stored_at.elapsed() <= ADMIN_ALERTS_CACHE_TTL
    })?;
    let entry = cache.admin_alerts.entries.remove(position)?;
    let result = (
        entry.value.clone(),
        entry.observed_at,
        entry.generation,
        cache.alert_projection_generation,
    );
    cache.admin_alerts.entries.push_front(entry);
    Some(result)
}

async fn record_admin_alerts_last_good(
    state: &AppState,
    key: String,
    value: AdminAlertsReadCacheValue,
) {
    record_admin_alerts_last_good_at_generation(
        state,
        key,
        value,
        current_admin_alerts_generation(state).await,
    )
    .await;
}

async fn current_admin_alerts_generation(state: &AppState) -> u64 {
    dashboard_overview_cache_for_state(state)
        .lock()
        .await
        .alert_projection_generation
}

async fn record_admin_alerts_last_good_at_generation(
    state: &AppState,
    key: String,
    value: AdminAlertsReadCacheValue,
    generation: u64,
) {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    let canonical = key == "catalog"
        || key == default_admin_alert_cache_key("events")
        || key == default_admin_alert_cache_key("groups");
    cache.admin_alerts.entries.retain(|entry| entry.key != key);
    cache.admin_alerts.entries.push_front(AdminAlertsReadCacheEntry {
        key,
        value,
        generation,
        canonical,
        observed_at: state.proxy.backend_time().now_ts(),
        stored_at: tokio::time::Instant::now(),
    });
    while cache.admin_alerts.entries.len() > ADMIN_ALERTS_CACHE_CAPACITY {
        if let Some(index) = cache
            .admin_alerts
            .entries
            .iter()
            .rposition(|entry| !entry.canonical)
        {
            cache.admin_alerts.entries.remove(index);
        } else {
            break;
        }
    }
}

#[cfg(test)]
pub(crate) async fn expire_admin_alerts_canonical_last_good_for_test(
    state: &AppState,
    key: &str,
) {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    if let Some(entry) = cache
        .admin_alerts
        .entries
        .iter_mut()
        .find(|entry| entry.key == key && entry.canonical)
    {
        entry.stored_at = tokio::time::Instant::now() - ADMIN_ALERTS_CACHE_TTL;
    }
}

async fn publish_admin_alerts_canonical(
    state: &AppState,
    generation: u64,
    catalog: AlertCatalog,
    events: PaginatedAlertEvents,
    groups: PaginatedAlertGroups,
) -> bool {
    let cache = dashboard_overview_cache_for_state(state);
    let mut cache = cache.lock().await;
    publish_admin_alerts_canonical_into_cache(
        &mut cache,
        generation,
        state.proxy.backend_time().now_ts(),
        tokio::time::Instant::now(),
        catalog,
        events,
        groups,
    )
}

fn publish_admin_alerts_canonical_into_cache(
    cache: &mut DashboardOverviewCacheState,
    generation: u64,
    observed_at: i64,
    stored_at: tokio::time::Instant,
    catalog: AlertCatalog,
    events: PaginatedAlertEvents,
    groups: PaginatedAlertGroups,
) -> bool {
    // The controller may stage each key through independently-admitted slices,
    // but all three values must still belong to the cache generation captured
    // at the start of the flight. A projection advance invalidates the staged
    // payload; the prior complete last-good remains available to handlers.
    if cache.alert_projection_generation != generation {
        return false;
    }
    for (key, value) in [
        ("catalog".to_string(), AdminAlertsReadCacheValue::Catalog(catalog)),
        (
            default_admin_alert_cache_key("events"),
            AdminAlertsReadCacheValue::Events(events),
        ),
        (
            default_admin_alert_cache_key("groups"),
            AdminAlertsReadCacheValue::Groups(groups),
        ),
    ] {
        cache.admin_alerts.entries.retain(|entry| entry.key != key);
        cache.admin_alerts.entries.push_front(AdminAlertsReadCacheEntry {
            key,
            value,
            generation,
            canonical: true,
            observed_at,
            stored_at,
        });
    }
    while cache.admin_alerts.entries.len() > ADMIN_ALERTS_CACHE_CAPACITY {
        if let Some(index) = cache
            .admin_alerts
            .entries
            .iter()
            .rposition(|entry| !entry.canonical)
        {
            cache.admin_alerts.entries.remove(index);
        } else {
            break;
        }
    }
    true
}

async fn rehydrate_admin_alerts_canonical_last_good(
    state: &AppState,
) -> Result<bool, tavily_hikari::ProxyError> {
    let cache = dashboard_overview_cache_for_state(state);
    let keys = [
        "catalog".to_string(),
        default_admin_alert_cache_key("events"),
        default_admin_alert_cache_key("groups"),
    ];
    let needs_rehydrate = {
        let cache = cache.lock().await;
        !keys.iter().all(|key| {
            cache
                .admin_alerts
                .entries
                .iter()
                .any(|entry| entry.canonical && entry.key == *key)
        })
    };
    if !needs_rehydrate {
        return Ok(false);
    }

    let Some(restored) = state
        .proxy
        .admin_alerts_canonical_last_good_for_rehydrate()
        .await?
    else {
        return Ok(false);
    };
    let current_fence = state
        .proxy
        .admin_alerts_canonical_warm_projection_fence()
        .await?;
    let (catalog, events, groups, active_fence) = restored;
    let mut cache = cache.lock().await;
    if keys.iter().all(|key| {
        cache
            .admin_alerts
            .entries
            .iter()
            .any(|entry| entry.canonical && entry.key == *key)
    }) {
        return Ok(false);
    }
    let generation = cache.alert_projection_generation;
    if !publish_admin_alerts_canonical_into_cache(
        &mut cache,
        generation,
        state.proxy.backend_time().now_ts(),
        tokio::time::Instant::now(),
        catalog,
        events,
        groups,
    ) {
        return Ok(false);
    }
    let stale = active_fence != current_fence;
    if stale {
        let next_generation = cache.alert_projection_generation.wrapping_add(1);
        cache.alert_projection_generation = next_generation.max(1);
    }
    tracing::info!(
        component = "admin_read",
        event = "alerts_canonical_last_good_rehydrated",
        active_recent_generation = active_fence.0,
        active_history_generation = active_fence.1,
        current_recent_generation = current_fence.0,
        current_history_generation = current_fence.1,
        stale,
        "rehydrated durable canonical Alerts last-good cache"
    );
    Ok(true)
}

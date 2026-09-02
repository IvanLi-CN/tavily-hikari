fn parse_alert_timestamp_filter(value: Option<&str>) -> Result<Option<i64>, StatusCode> {
    match value {
        Some(raw) if !raw.trim().is_empty() => parse_iso_timestamp(raw).ok_or(StatusCode::BAD_REQUEST).map(Some),
        _ => Ok(None),
    }
}

fn normalize_alert_type_filter(value: Option<&str>) -> Result<Option<&str>, StatusCode> {
    let normalized = normalize_optional_filter(value);
    if let Some(alert_type) = normalized
        && !tavily_hikari::is_supported_alert_type(alert_type)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(normalized)
}

fn resolve_alert_query_window(
    since: Option<&str>,
    until: Option<&str>,
) -> Result<(Option<i64>, Option<i64>), StatusCode> {
    let parsed_since = parse_alert_timestamp_filter(since)?;
    let parsed_until = parse_alert_timestamp_filter(until)?;
    if let (Some(since), Some(until)) = (parsed_since, parsed_until)
        && since > until
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok((parsed_since, parsed_until))
}

async fn get_alert_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<axum::response::Response, axum::response::Response> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN.into_response());
    }

    let key = "catalog";
    if let Some((AdminAlertsReadCacheValue::Catalog(catalog), observed_at, entry_generation, generation)) =
        admin_alerts_canonical_last_good(state.as_ref(), key).await
    {
        let pressure_reason = state.proxy.admin_alerts_cache_warm_pressure_reason();
        let stale_reason = pressure_reason.unwrap_or("projection_refresh");
        if entry_generation == generation && pressure_reason.is_none() {
            tracing::debug!(
                component = "admin_read",
                event = "alerts_last_good_served",
                route = "/api/alerts/catalog",
                coverage = "fresh",
                "served canonical Alerts catalog from last-good cache"
            );
            return Ok(Json(AlertCatalogView::from(catalog)).into_response());
        }
        tracing::debug!(
            component = "admin_read",
            event = "alerts_last_good_served",
            route = "/api/alerts/catalog",
            coverage = "stale",
            stale_reason,
            "served canonical Alerts catalog from prior projection generation"
        );
        return Ok(
            Json(AlertCatalogView::from(catalog).stale_with_reason(observed_at, stale_reason))
            .into_response(),
        );
    }
    // The canonical catalog is owned by the background warm controller. A
    // cold or expired key must never make an administrator request rebuild it.
    state.proxy.record_admin_alerts_warm_cold_miss();
    tracing::debug!(
        component = "admin_read",
        event = "alerts_cold_pressure",
        route = "/api/alerts/catalog",
        "canonical Alerts catalog cache is cold"
    );
    Err(alerts_sqlite_pressure_response())
}

fn alerts_sqlite_pressure_response() -> axum::response::Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [("retry-after", "1")],
    )
        .into_response()
}

struct AlertReadCacheQuery<'a> {
    alert_type: Option<&'a str>,
    since: Option<i64>,
    until: Option<i64>,
    user_id: Option<&'a str>,
    token_id: Option<&'a str>,
    key_id: Option<&'a str>,
    request_kinds: &'a [String],
    page: i64,
    per_page: i64,
}

fn alert_read_cache_key(kind: &str, query: &AlertReadCacheQuery<'_>) -> String {
    let mut request_kinds = query.request_kinds.to_vec();
    request_kinds.sort();
    request_kinds.dedup();
    serde_json::to_string(&(
        kind,
        query.alert_type,
        query.since,
        query.until,
        query.user_id,
        query.token_id,
        query.key_id,
        request_kinds,
        query.page,
        query.per_page,
    ))
    .expect("alert read cache key fields are serializable")
}

async fn get_alert_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<AlertsQuery>,
) -> Result<axum::response::Response, axum::response::Response> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN.into_response());
    }

    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let alert_type = normalize_alert_type_filter(q.alert_type.as_deref())
        .map_err(|status| status.into_response())?;
    let (since, until) = resolve_alert_query_window(q.since.as_deref(), q.until.as_deref())
        .map_err(|status| status.into_response())?;
    let user_id = normalize_optional_filter(q.user_id.as_deref());
    let token_id = normalize_optional_filter(q.token_id.as_deref());
    let key_id = normalize_optional_filter(q.key_id.as_deref());
    let cache_query = AlertReadCacheQuery {
        alert_type,
        since,
        until,
        user_id,
        token_id,
        key_id,
        request_kinds: &request_kinds,
        page,
        per_page,
    };
    let cache_key = alert_read_cache_key("events", &cache_query);
    if let Some((AdminAlertsReadCacheValue::Events(events), observed_at, entry_generation, generation)) =
        admin_alerts_canonical_last_good(state.as_ref(), &cache_key).await
    {
        let pressure_reason = state.proxy.admin_alerts_cache_warm_pressure_reason();
        let stale_reason = pressure_reason.unwrap_or("projection_refresh");
        if entry_generation == generation && pressure_reason.is_none() {
            tracing::debug!(
                component = "admin_read",
                event = "alerts_last_good_served",
                route = "/api/alerts/events",
                coverage = "fresh",
                "served canonical Alerts events from last-good cache"
            );
            return Ok(Json(PaginatedAlertEventsView::from(events)).into_response());
        }
        tracing::debug!(
            component = "admin_read",
            event = "alerts_last_good_served",
            route = "/api/alerts/events",
            coverage = "stale",
            stale_reason,
            "served canonical Alerts events from prior projection generation"
        );
        return Ok(
            Json(PaginatedAlertEventsView::from(events).stale_with_reason(observed_at, stale_reason))
            .into_response(),
        );
    }
    if cache_key == default_admin_alert_cache_key("events") {
        state.proxy.record_admin_alerts_warm_cold_miss();
        tracing::debug!(
            component = "admin_read",
            event = "alerts_cold_pressure",
            route = "/api/alerts/events",
            "canonical Alerts events cache is cold"
        );
        return Err(alerts_sqlite_pressure_response());
    }
    match state.proxy.admin_alert_events_page(
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            &request_kinds,
            page,
            per_page,
        )
        .await
    {
        Ok(events) => {
            record_admin_alerts_last_good(
                state.as_ref(),
                cache_key,
                AdminAlertsReadCacheValue::Events(events.clone()),
            )
            .await;
            Ok(Json(PaginatedAlertEventsView::from(events)).into_response())
        }
        Err(error) if tavily_hikari::is_transient_sqlite_write_error(&error) || error.is_deferred() => {
            match admin_alerts_last_good(state.as_ref(), &cache_key).await {
                Some((AdminAlertsReadCacheValue::Events(events), observed_at)) => {
                    Ok(Json(PaginatedAlertEventsView::from(events).stale(observed_at)).into_response())
                }
                _ => Err(alerts_sqlite_pressure_response()),
            }
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

async fn get_alert_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<AlertsQuery>,
) -> Result<axum::response::Response, axum::response::Response> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN.into_response());
    }

    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let alert_type = normalize_alert_type_filter(q.alert_type.as_deref())
        .map_err(|status| status.into_response())?;
    let (since, until) = resolve_alert_query_window(q.since.as_deref(), q.until.as_deref())
        .map_err(|status| status.into_response())?;
    let user_id = normalize_optional_filter(q.user_id.as_deref());
    let token_id = normalize_optional_filter(q.token_id.as_deref());
    let key_id = normalize_optional_filter(q.key_id.as_deref());
    let cache_query = AlertReadCacheQuery {
        alert_type,
        since,
        until,
        user_id,
        token_id,
        key_id,
        request_kinds: &request_kinds,
        page,
        per_page,
    };
    let cache_key = alert_read_cache_key("groups", &cache_query);
    if let Some((AdminAlertsReadCacheValue::Groups(groups), observed_at, entry_generation, generation)) =
        admin_alerts_canonical_last_good(state.as_ref(), &cache_key).await
    {
        let pressure_reason = state.proxy.admin_alerts_cache_warm_pressure_reason();
        let stale_reason = pressure_reason.unwrap_or("projection_refresh");
        if entry_generation == generation && pressure_reason.is_none() {
            tracing::debug!(
                component = "admin_read",
                event = "alerts_last_good_served",
                route = "/api/alerts/groups",
                coverage = "fresh",
                "served canonical Alerts groups from last-good cache"
            );
            return Ok(Json(PaginatedAlertGroupsView::from(groups)).into_response());
        }
        tracing::debug!(
            component = "admin_read",
            event = "alerts_last_good_served",
            route = "/api/alerts/groups",
            coverage = "stale",
            stale_reason,
            "served canonical Alerts groups from prior projection generation"
        );
        return Ok(
            Json(PaginatedAlertGroupsView::from(groups).stale_with_reason(observed_at, stale_reason))
            .into_response(),
        );
    }
    if cache_key == default_admin_alert_cache_key("groups") {
        state.proxy.record_admin_alerts_warm_cold_miss();
        tracing::debug!(
            component = "admin_read",
            event = "alerts_cold_pressure",
            route = "/api/alerts/groups",
            "canonical Alerts groups cache is cold"
        );
        return Err(alerts_sqlite_pressure_response());
    }
    match state.proxy.admin_alert_groups_page(
            alert_type,
            since,
            until,
            user_id,
            token_id,
            key_id,
            &request_kinds,
            page,
            per_page,
        )
        .await
    {
        Ok(groups) => {
            record_admin_alerts_last_good(
                state.as_ref(),
                cache_key,
                AdminAlertsReadCacheValue::Groups(groups.clone()),
            )
            .await;
            Ok(Json(PaginatedAlertGroupsView::from(groups)).into_response())
        }
        Err(error) if tavily_hikari::is_transient_sqlite_write_error(&error) || error.is_deferred() => {
            match admin_alerts_last_good(state.as_ref(), &cache_key).await {
                Some((AdminAlertsReadCacheValue::Groups(groups), observed_at)) => {
                    Ok(Json(PaginatedAlertGroupsView::from(groups).stale(observed_at)).into_response())
                }
                _ => Err(alerts_sqlite_pressure_response()),
            }
        }
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR.into_response()),
    }
}

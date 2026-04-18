const ALERTS_DEFAULT_WINDOW_HOURS: i64 = 24;

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
    let mut parsed_since = parse_alert_timestamp_filter(since)?;
    let parsed_until = parse_alert_timestamp_filter(until)?;
    if parsed_since.is_none() && parsed_until.is_none() {
        parsed_since = Some(
            Utc::now()
                .timestamp()
                .saturating_sub(ALERTS_DEFAULT_WINDOW_HOURS * 3600),
        );
    }
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
) -> Result<Json<AlertCatalogView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .alert_catalog()
        .await
        .map(AlertCatalogView::from)
        .map(Json)
        .map_err(|err| {
            eprintln!("alert catalog error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_alert_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<AlertsQuery>,
) -> Result<Json<PaginatedAlertEventsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let alert_type = normalize_alert_type_filter(q.alert_type.as_deref())?;
    let (since, until) = resolve_alert_query_window(q.since.as_deref(), q.until.as_deref())?;

    state
        .proxy
        .alert_events_page(
            alert_type,
            since,
            until,
            normalize_optional_filter(q.user_id.as_deref()),
            normalize_optional_filter(q.token_id.as_deref()),
            normalize_optional_filter(q.key_id.as_deref()),
            &request_kinds,
            page,
            per_page,
        )
        .await
        .map(PaginatedAlertEventsView::from)
        .map(Json)
        .map_err(|err| {
            eprintln!("alert events error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_alert_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<AlertsQuery>,
) -> Result<Json<PaginatedAlertGroupsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100);
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let alert_type = normalize_alert_type_filter(q.alert_type.as_deref())?;
    let (since, until) = resolve_alert_query_window(q.since.as_deref(), q.until.as_deref())?;

    state
        .proxy
        .alert_groups_page(
            alert_type,
            since,
            until,
            normalize_optional_filter(q.user_id.as_deref()),
            normalize_optional_filter(q.token_id.as_deref()),
            normalize_optional_filter(q.key_id.as_deref()),
            &request_kinds,
            page,
            per_page,
        )
        .await
        .map(PaginatedAlertGroupsView::from)
        .map(Json)
        .map_err(|err| {
            eprintln!("alert groups error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

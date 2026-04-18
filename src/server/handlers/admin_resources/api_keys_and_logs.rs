async fn post_validate_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ValidateKeysRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let ValidateKeysRequest { api_keys, items } = payload;
    let raw_items = if items.is_empty() {
        api_keys
            .into_iter()
            .map(|api_key| ValidateKeyItemInput {
                api_key,
                registration_ip: None,
            })
            .collect::<Vec<_>>()
    } else {
        items
    };

    let mut summary = ValidateKeysSummary {
        input_lines: raw_items.len() as u64,
        ..Default::default()
    };

    let mut trimmed = Vec::<NormalizedValidateKeyItem>::with_capacity(raw_items.len());
    let mut geo_lookup_ips = Vec::<String>::new();
    for item in raw_items {
        let api_key = item.api_key.trim();
        if api_key.is_empty() {
            continue;
        }
        let registration_ip = item
            .registration_ip
            .as_deref()
            .and_then(normalize_global_registration_ip);
        if let Some(ip) = registration_ip.as_ref() {
            geo_lookup_ips.push(ip.clone());
        }
        trimmed.push(NormalizedValidateKeyItem {
            api_key: api_key.to_string(),
            registration_ip,
        });
    }
    summary.valid_lines = trimmed.len() as u64;

    if trimmed.len() > API_KEYS_BATCH_LIMIT {
        let body = Json(json!({
            "error": "too_many_items",
            "detail": format!("api_keys exceeds limit (max {})", API_KEYS_BATCH_LIMIT),
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }

    let region_by_ip = resolve_registration_regions(&state.api_key_ip_geo_origin, &geo_lookup_ips).await;
    let mut results = Vec::<ValidateKeyResult>::with_capacity(trimmed.len());
    let mut pending = Vec::<(usize, String, Option<String>, Option<String>)>::new();
    let mut seen = HashSet::<String>::new();

    for item in trimmed {
        let registration_region = item
            .registration_ip
            .as_ref()
            .and_then(|ip| region_by_ip.get(ip).cloned());
        if !seen.insert(item.api_key.clone()) {
            summary.duplicate_in_input += 1;
            results.push(ValidateKeyResult {
                api_key: item.api_key,
                status: "duplicate_in_input".to_string(),
                registration_ip: item.registration_ip,
                registration_region,
                assigned_proxy_key: None,
                assigned_proxy_label: None,
                assigned_proxy_match_kind: None,
                quota_limit: None,
                quota_remaining: None,
                detail: None,
            });
            continue;
        }

        let pos = results.len();
        results.push(ValidateKeyResult {
            api_key: item.api_key.clone(),
            status: "pending".to_string(),
            registration_ip: item.registration_ip.clone(),
            registration_region: registration_region.clone(),
            assigned_proxy_key: None,
            assigned_proxy_label: None,
            assigned_proxy_match_kind: None,
            quota_limit: None,
            quota_remaining: None,
            detail: None,
        });
        pending.push((pos, item.api_key, item.registration_ip, registration_region));
    }

    summary.unique_in_input = seen.len() as u64;

    let proxy = state.proxy.clone();
    let usage_base = state.usage_base.clone();
    let geo_origin = state.api_key_ip_geo_origin.clone();
    let checked = futures_stream::iter(pending.into_iter())
        .map(|(pos, api_key, registration_ip, registration_region)| {
            let proxy = proxy.clone();
            let usage_base = usage_base.clone();
            let geo_origin = geo_origin.clone();
            async move {
                let (result, kind) = validate_single_key(
                    proxy,
                    usage_base,
                    geo_origin,
                    api_key,
                    registration_ip,
                    registration_region,
                )
                .await;
                (pos, result, kind)
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;

    for (pos, result, kind) in checked {
        if let Some(slot) = results.get_mut(pos) {
            *slot = result;
        }
        match kind {
            "ok" => summary.ok += 1,
            "exhausted" => summary.exhausted += 1,
            "invalid" => summary.invalid += 1,
            _ => summary.error += 1,
        }
    }

    Ok((
        StatusCode::OK,
        Json(ValidateKeysResponse { summary, results }),
    )
        .into_response())
}

async fn create_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let CreateKeyRequest {
        api_key,
        group: group_raw,
        registration_ip: registration_ip_raw,
        assigned_proxy_key,
    } = payload;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let group = group_raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let registration_ip = registration_ip_raw
        .as_deref()
        .and_then(normalize_global_registration_ip);
    let registration_region = if let Some(registration_ip) = registration_ip.as_ref() {
        resolve_registration_regions(&state.api_key_ip_geo_origin, std::slice::from_ref(registration_ip))
            .await
            .remove(registration_ip)
    } else {
        None
    };

    match state
        .proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            api_key,
            group,
            registration_ip.as_deref(),
            registration_region.as_deref(),
            &state.api_key_ip_geo_origin,
            assigned_proxy_key.as_deref(),
        )
        .await
    {
        Ok((id, _)) => Ok((StatusCode::CREATED, Json(CreateKeyResponse { id }))),
        Err(err) => {
            eprintln!("create api key error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn create_api_keys_batch(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BatchCreateKeysRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let BatchCreateKeysRequest {
        api_keys,
        items,
        group: group_raw,
        exhausted_api_keys,
    } = payload;
    let raw_items = BatchCreateKeysRequest {
        api_keys,
        items,
        group: None,
        exhausted_api_keys: None,
    }
    .into_items();
    let group = group_raw
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let exhausted_set: HashSet<String> = exhausted_api_keys
        .unwrap_or_default()
        .into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();

    let mut summary = BatchCreateKeysSummary {
        input_lines: raw_items.len() as u64,
        ..Default::default()
    };

    let mut trimmed = Vec::<NormalizedBatchCreateKeyItem>::with_capacity(raw_items.len());
    let mut geo_lookup_ips = Vec::<String>::new();
    for item in raw_items {
        let api_key = item.api_key.trim();
        if api_key.is_empty() {
            summary.ignored_empty += 1;
            continue;
        }
        let registration_ip = item
            .registration_ip
            .as_deref()
            .and_then(normalize_global_registration_ip);
        if let Some(ip) = registration_ip.as_ref() {
            geo_lookup_ips.push(ip.clone());
        }
        let assigned_proxy_key = item
            .assigned_proxy_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        trimmed.push(NormalizedBatchCreateKeyItem {
            api_key: api_key.to_string(),
            registration_ip,
            assigned_proxy_key,
        });
    }
    summary.valid_lines = trimmed.len() as u64;

    if trimmed.len() > API_KEYS_BATCH_LIMIT {
        let body = Json(json!({
            "error": "too_many_items",
            "detail": format!("api_keys exceeds limit (max {})", API_KEYS_BATCH_LIMIT),
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }

    let mut results = Vec::with_capacity(trimmed.len());
    let mut seen = HashSet::<String>::new();
    let region_by_ip = resolve_registration_regions(&state.api_key_ip_geo_origin, &geo_lookup_ips).await;
    let maintenance_actor = admin_maintenance_actor(state.as_ref(), &headers, None).await;

    for item in trimmed {
        if !seen.insert(item.api_key.clone()) {
            summary.duplicate_in_input += 1;
            results.push(BatchCreateKeysResult {
                api_key: item.api_key,
                status: "duplicate_in_input".to_string(),
                id: None,
                error: None,
                marked_exhausted: None,
            });
            continue;
        }

        let registration_region = item
            .registration_ip
            .as_ref()
            .and_then(|ip| region_by_ip.get(ip).cloned());

        match state
            .proxy
            .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
                &item.api_key,
                group,
                item.registration_ip.as_deref(),
                registration_region.as_deref(),
                &state.api_key_ip_geo_origin,
                item.assigned_proxy_key.as_deref(),
            )
            .await
        {
            Ok((id, status)) => {
                match status.as_str() {
                    "created" => summary.created += 1,
                    "undeleted" => summary.undeleted += 1,
                    "existed" => summary.existed += 1,
                    _ => {}
                }
                let mut marked_exhausted = None;
                if exhausted_set.contains(&item.api_key) {
                    marked_exhausted = match state
                        .proxy
                        .mark_key_quota_exhausted_by_secret_with_actor(
                            &item.api_key,
                            maintenance_actor.clone(),
                        )
                        .await
                    {
                        Ok(changed) => Some(changed),
                        Err(err) => {
                            eprintln!("mark exhausted failed for key: {err}");
                            Some(false)
                        }
                    };
                }
                results.push(BatchCreateKeysResult {
                    api_key: item.api_key,
                    status: status.as_str().to_string(),
                    id: Some(id),
                    error: None,
                    marked_exhausted,
                });
            }
            Err(err) => {
                summary.failed += 1;
                results.push(BatchCreateKeysResult {
                    api_key: item.api_key,
                    status: "failed".to_string(),
                    id: None,
                    error: Some(err.to_string()),
                    marked_exhausted: None,
                });
            }
        }
    }

    summary.unique_in_input = seen.len() as u64;

    Ok((
        StatusCode::OK,
        Json(BatchCreateKeysResponse { summary, results }),
    )
        .into_response())
}

async fn post_api_key_bulk_actions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<BulkApiKeyActionRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let Some(action) = BulkApiKeyActionKind::parse(&payload.action) else {
        let body = Json(json!({
            "error": "invalid_action",
            "detail": "action must be one of delete, clear_quarantine, sync_usage",
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    };

    let mut normalized_ids = Vec::with_capacity(payload.key_ids.len());
    let mut seen = HashSet::<String>::new();
    for raw_id in payload.key_ids {
        let key_id = raw_id.trim();
        if key_id.is_empty() {
            continue;
        }
        if seen.insert(key_id.to_string()) {
            normalized_ids.push(key_id.to_string());
        }
    }

    if normalized_ids.is_empty() {
        let body = Json(json!({
            "error": "empty_key_ids",
            "detail": "key_ids must contain at least one non-empty value",
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }

    if normalized_ids.len() > API_KEYS_BATCH_LIMIT {
        let body = Json(json!({
            "error": "too_many_items",
            "detail": format!("key_ids exceeds limit (max {})", API_KEYS_BATCH_LIMIT),
        }));
        return Ok((StatusCode::BAD_REQUEST, body).into_response());
    }

    if matches!(action, BulkApiKeyActionKind::SyncUsage) && request_accepts_event_stream(&headers) {
        let state = state.clone();
        let total = normalized_ids.len() as u64;
        let stream = stream! {
            let prepare = BulkApiKeySyncProgressEvent::Phase {
                phase_key: "prepare_request",
                label: "Preparing request",
                detail: Some(format!("Queued {total} key(s) for manual quota sync")),
                current: Some(0),
                total: Some(total),
            };
            match serde_json::to_string(&prepare) {
                Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                Err(err) => {
                    let fallback = BulkApiKeySyncProgressEvent::Error {
                        message: "failed to encode bulk sync prepare event".to_string(),
                        phase_key: Some("prepare_request"),
                        detail: Some(err.to_string()),
                    };
                    if let Ok(json) = serde_json::to_string(&fallback) {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(json));
                    }
                    return;
                }
            }

            let sync_phase = BulkApiKeySyncProgressEvent::Phase {
                phase_key: "sync_usage",
                label: "Syncing selected keys",
                detail: Some("Waiting for each manual quota sync result as keys finish".to_string()),
                current: Some(0),
                total: Some(total),
            };
            match serde_json::to_string(&sync_phase) {
                Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                Err(err) => {
                    let fallback = BulkApiKeySyncProgressEvent::Error {
                        message: "failed to encode bulk sync phase event".to_string(),
                        phase_key: Some("sync_usage"),
                        detail: Some(err.to_string()),
                    };
                    if let Ok(json) = serde_json::to_string(&fallback) {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(json));
                    }
                    return;
                }
            }

            let mut summary = BulkApiKeyActionSummary {
                requested: total,
                ..Default::default()
            };
            let mut results = Vec::with_capacity(total as usize);

            for (index, key_id) in normalized_ids.into_iter().enumerate() {
                let result = match run_manual_key_quota_sync(state.as_ref(), &key_id).await {
                    Ok(()) => BulkApiKeyActionResult {
                        key_id,
                        status: "success".to_string(),
                        detail: None,
                    },
                    Err(err) => BulkApiKeyActionResult {
                        key_id,
                        status: "failed".to_string(),
                        detail: Some(err.detail),
                    },
                };

                match result.status.as_str() {
                    "success" => summary.succeeded += 1,
                    "skipped" => summary.skipped += 1,
                    _ => summary.failed += 1,
                }

                results.push(result.clone());

                let item_event = BulkApiKeySyncProgressEvent::Item {
                    key_id: result.key_id.clone(),
                    status: result.status.clone(),
                    current: index as u64 + 1,
                    total,
                    summary: summary.clone(),
                    detail: result.detail.clone(),
                };

                match serde_json::to_string(&item_event) {
                    Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                    Err(err) => {
                        let fallback = BulkApiKeySyncProgressEvent::Error {
                            message: "failed to encode bulk sync item event".to_string(),
                            phase_key: Some("sync_usage"),
                            detail: Some(err.to_string()),
                        };
                        if let Ok(json) = serde_json::to_string(&fallback) {
                            yield Ok::<Event, axum::http::Error>(Event::default().data(json));
                        }
                        return;
                    }
                }
            }

            let refresh_phase = BulkApiKeySyncProgressEvent::Phase {
                phase_key: "refresh_ui",
                label: "Refreshing list",
                detail: Some("Server-side sync finished; refresh the admin keys list now".to_string()),
                current: Some(total),
                total: Some(total),
            };
            match serde_json::to_string(&refresh_phase) {
                Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                Err(err) => {
                    let fallback = BulkApiKeySyncProgressEvent::Error {
                        message: "failed to encode bulk sync refresh event".to_string(),
                        phase_key: Some("refresh_ui"),
                        detail: Some(err.to_string()),
                    };
                    if let Ok(json) = serde_json::to_string(&fallback) {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(json));
                    }
                    return;
                }
            }

            let complete = BulkApiKeySyncProgressEvent::Complete {
                payload: BulkApiKeyActionResponse { summary, results },
            };
            match serde_json::to_string(&complete) {
                Ok(json) => yield Ok::<Event, axum::http::Error>(Event::default().data(json)),
                Err(err) => {
                    let fallback = BulkApiKeySyncProgressEvent::Error {
                        message: "failed to encode bulk sync completion event".to_string(),
                        phase_key: Some("refresh_ui"),
                        detail: Some(err.to_string()),
                    };
                    if let Ok(json) = serde_json::to_string(&fallback) {
                        yield Ok::<Event, axum::http::Error>(Event::default().data(json));
                    }
                }
            }
        };

        return Ok(
            Sse::new(stream)
                .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text(""))
                .into_response(),
        );
    }

    let maintenance_actor = if matches!(action, BulkApiKeyActionKind::ClearQuarantine) {
        Some(admin_maintenance_actor(state.as_ref(), &headers, None).await)
    } else {
        None
    };

    let mut summary = BulkApiKeyActionSummary {
        requested: normalized_ids.len() as u64,
        ..Default::default()
    };
    let mut results = Vec::with_capacity(normalized_ids.len());

    for key_id in normalized_ids {
        let result = match action {
            BulkApiKeyActionKind::Delete => match state.proxy.soft_delete_key_by_id(&key_id).await {
                Ok(()) => BulkApiKeyActionResult {
                    key_id,
                    status: "success".to_string(),
                    detail: None,
                },
                Err(err) => BulkApiKeyActionResult {
                    key_id,
                    status: "failed".to_string(),
                    detail: Some(err.to_string()),
                },
            },
            BulkApiKeyActionKind::ClearQuarantine => match state
                .proxy
                .clear_key_quarantine_by_id_with_actor(
                    &key_id,
                    maintenance_actor
                        .clone()
                        .expect("maintenance actor for bulk clear quarantine"),
                )
                .await
            {
                Ok(true) => BulkApiKeyActionResult {
                    key_id,
                    status: "success".to_string(),
                    detail: None,
                },
                Ok(false) => BulkApiKeyActionResult {
                    key_id,
                    status: "skipped".to_string(),
                    detail: Some("no active quarantine".to_string()),
                },
                Err(err) => BulkApiKeyActionResult {
                    key_id,
                    status: "failed".to_string(),
                    detail: Some(err.to_string()),
                },
            },
            BulkApiKeyActionKind::SyncUsage => match run_manual_key_quota_sync(state.as_ref(), &key_id).await {
                Ok(()) => BulkApiKeyActionResult {
                    key_id,
                    status: "success".to_string(),
                    detail: None,
                },
                Err(err) => BulkApiKeyActionResult {
                    key_id,
                    status: "failed".to_string(),
                    detail: Some(err.detail),
                },
            },
        };

        match result.status.as_str() {
            "success" => summary.succeeded += 1,
            "skipped" => summary.skipped += 1,
            _ => summary.failed += 1,
        }
        results.push(result);
    }

    Ok((
        StatusCode::OK,
        Json(BulkApiKeyActionResponse { summary, results }),
    )
        .into_response())
}

async fn delete_api_key(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.soft_delete_key_by_id(&id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(err) => {
            eprintln!("delete api key error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Deserialize)]
struct UpdateKeyStatus {
    status: String,
}

async fn update_api_key_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<UpdateKeyStatus>,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let status = payload.status.trim().to_ascii_lowercase();
    match status.as_str() {
        "disabled" => match state.proxy.disable_key_by_id(&id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(err) => {
                eprintln!("disable api key error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        "active" => match state.proxy.enable_key_by_id(&id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(err) => {
                eprintln!("enable api key error: {err}");
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn get_api_key_secret(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeySecretView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    match state.proxy.get_api_key_secret(&id).await {
        Ok(Some(secret)) => Ok(Json(ApiKeySecretView { api_key: secret })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(err) => {
            eprintln!("fetch api key secret error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedLogsView {
    items: Vec<RequestLogView>,
    total: i64,
    page: i64,
    per_page: i64,
    request_kind_options: Vec<TokenRequestKindOptionView>,
    facets: RequestLogFacetsView,
}

async fn list_logs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(params): Query<LogsQuery>,
) -> Result<Json<PaginatedLogsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page = params.page.unwrap_or(1).max(1);
    let per_page = params.per_page.unwrap_or(20).clamp(1, 200);

    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(params.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(params.key_effect.as_deref());
    let binding_effect_code = normalize_binding_effect_filter(params.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(params.selection_effect.as_deref());
    let include_bodies = params.include_bodies.unwrap_or(false);
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let auth_token_id = normalize_optional_filter(params.auth_token_id.as_deref());
    let key_id = normalize_optional_filter(params.key_id.as_deref());
    let operational_class = normalize_operational_class_filter(params.operational_class.as_deref());
    if params
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .request_logs_page(
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            key_id,
            operational_class,
            page,
            per_page,
        )
        .await
        .map(|logs| {
            let view_items = logs
                .items
                .into_iter()
                .map(|record| RequestLogView::from_request_record(record, include_bodies))
                .collect();
            Json(PaginatedLogsView {
                items: view_items,
                total: logs.total,
                page,
                per_page,
                request_kind_options: logs
                    .request_kind_options
                    .into_iter()
                    .map(TokenRequestKindOptionView::from)
                    .collect(),
                facets: RequestLogFacetsView {
                    results: logs
                        .facets
                        .results
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    key_effects: logs
                        .facets
                        .key_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    binding_effects: logs
                        .facets
                        .binding_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    selection_effects: logs
                        .facets
                        .selection_effects
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    tokens: logs
                        .facets
                        .tokens
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                    keys: logs
                        .facets
                        .keys
                        .into_iter()
                        .map(LogFacetOptionView::from)
                        .collect(),
                },
            })
        })
        .map_err(|err| {
            eprintln!("list logs error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn list_logs_cursor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(params): Query<CursorLogsQuery>,
) -> Result<Json<RequestLogsCursorPageView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    let page_size = params.limit.unwrap_or(20).clamp(1, 200);
    let cursor = parse_request_logs_cursor(params.cursor.as_deref())?;
    let direction = normalize_request_logs_cursor_direction(params.direction.as_deref())?;
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(params.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(params.key_effect.as_deref());
    let binding_effect_code = normalize_binding_effect_filter(params.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(params.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let auth_token_id = normalize_optional_filter(params.auth_token_id.as_deref());
    let key_id = normalize_optional_filter(params.key_id.as_deref());
    let operational_class = normalize_operational_class_filter(params.operational_class.as_deref());
    if params
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .request_logs_list(
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            key_id,
            operational_class,
            cursor.as_ref(),
            direction,
            page_size,
        )
        .await
        .map(build_request_logs_cursor_page_view)
        .map(Json)
        .map_err(|err| {
            eprintln!("list logs cursor error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn get_logs_catalog(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
    Query(q): Query<CursorLogsQuery>,
) -> Result<Json<RequestLogsCatalogView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let request_kinds = parse_request_kind_filters(raw_query.as_deref());
    let result_status = normalize_result_status_filter(q.result.as_deref());
    let key_effect_code = normalize_key_effect_filter(q.key_effect.as_deref());
    let binding_effect_code = normalize_binding_effect_filter(q.binding_effect.as_deref());
    let selection_effect_code = normalize_selection_effect_filter(q.selection_effect.as_deref());
    validate_logs_effect_filters(
        result_status,
        key_effect_code,
        binding_effect_code,
        selection_effect_code,
    )?;
    let auth_token_id = normalize_optional_filter(q.auth_token_id.as_deref());
    let operational_class = normalize_operational_class_filter(q.operational_class.as_deref());
    if q
        .operational_class
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && operational_class.is_none()
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    state
        .proxy
        .request_logs_catalog(
            &request_kinds,
            result_status,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            auth_token_id,
            normalize_optional_filter(q.key_id.as_deref()),
            operational_class,
        )
        .await
        .map(RequestLogsCatalogView::from)
        .map(Json)
        .map_err(|err| {
            eprintln!("request logs catalog error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Deserialize)]
struct ListUsersQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    q: Option<String>,
    #[serde(rename = "tagId")]
    tag_id: Option<String>,
    sort: Option<AdminUsersSortField>,
    order: Option<AdminUsersSortDirection>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum AdminUsersSortField {
    HourlyAnyUsed,
    QuotaHourlyUsed,
    QuotaDailyUsed,
    QuotaMonthlyUsed,
    DailySuccessRate,
    MonthlySuccessRate,
    MonthlyBrokenCount,
    LastActivity,
    LastLoginAt,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum AdminUsersSortDirection {
    Asc,
    Desc,
}


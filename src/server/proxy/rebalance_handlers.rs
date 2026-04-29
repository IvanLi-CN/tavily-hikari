#[allow(clippy::too_many_arguments)]
async fn handle_rebalance_mcp_single_message(
    state: &Arc<AppState>,
    method: &Method,
    path: &str,
    headers: &ReqHeaderMap,
    message: &Value,
    message_body: &[u8],
    token_id: Option<&str>,
    proxy_session_id: Option<&str>,
    incoming_protocol_version: Option<&str>,
    routing_subject_hash: Option<&str>,
) -> Result<ProxyResponse, StatusCode> {
    let response_id = message.get("id").filter(|value| !value.is_null());
    let method_name = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(method_name) = method_name else {
        let body = build_rebalance_mcp_error_body(response_id, -32600, "Invalid Request");
        let request_log_id = log_rebalance_local_control_plane_response(
            state,
            token_id,
            method,
            path,
            message_body,
            StatusCode::BAD_REQUEST,
            &body,
            proxy_session_id,
            routing_subject_hash,
            Some("invalid_jsonrpc_request"),
        )
        .await;
        return Ok(proxy_response_with_mcp_body(
            StatusCode::BAD_REQUEST,
            body,
            request_log_id,
        ));
    };

    match method_name {
        "initialize" => {
            let protocol_version =
                rebalance_initialize_protocol_version(message, incoming_protocol_version);
            let body = build_rebalance_mcp_initialize_body(response_id, &protocol_version);
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::OK,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::OK,
                body,
                request_log_id,
            ))
        }
        "notifications/initialized" => {
            let body = Vec::new();
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::ACCEPTED,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::ACCEPTED,
                body,
                request_log_id,
            ))
        }
        "ping" => {
            let body = build_rebalance_mcp_ping_body(response_id);
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::OK,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::OK,
                body,
                request_log_id,
            ))
        }
        "tools/list" => {
            let body = build_rebalance_mcp_tools_list_body(response_id);
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::OK,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::OK,
                body,
                request_log_id,
            ))
        }
        "prompts/list" => {
            let body = build_rebalance_mcp_prompts_list_body(response_id);
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::OK,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::OK,
                body,
                request_log_id,
            ))
        }
        "resources/list" => {
            let body = build_rebalance_mcp_resources_list_body(response_id);
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::OK,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::OK,
                body,
                request_log_id,
            ))
        }
        "resources/templates/list" => {
            let body = build_rebalance_mcp_resource_templates_list_body(response_id);
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::OK,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::OK,
                body,
                request_log_id,
            ))
        }
        "tools/call" => {
            let params = message.get("params").and_then(Value::as_object);
            let Some(params) = params else {
                let body = build_rebalance_mcp_tool_error_result_body(
                    response_id,
                    "Invalid tools/call params",
                );
                let request_log_id = log_rebalance_local_control_plane_response(
                    state,
                    token_id,
                    method,
                    path,
                    message_body,
                    StatusCode::OK,
                    &body,
                    proxy_session_id,
                    routing_subject_hash,
                    Some("invalid_tool_params"),
                )
                .await;
                return Ok(proxy_response_with_mcp_body(
                    StatusCode::OK,
                    body,
                    request_log_id,
                ));
            };
            let tool_name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(tool) = rebalance_mcp_tool_definition_by_name(tool_name) else {
                let message = format!("Not found: Unknown tool: '{tool_name}'");
                let body = build_rebalance_mcp_tool_error_result_body(response_id, &message);
                let request_log_id = log_rebalance_local_control_plane_response(
                    state,
                    token_id,
                    method,
                    path,
                    message_body,
                    StatusCode::OK,
                    &body,
                    proxy_session_id,
                    routing_subject_hash,
                    Some("unknown_tool"),
                )
                .await;
                return Ok(proxy_response_with_mcp_body(
                    StatusCode::OK,
                    body,
                    request_log_id,
                ));
            };
            let options = match validate_rebalance_mcp_tool_arguments(tool, params.get("arguments")) {
                Ok(arguments) => arguments,
                Err(message) => {
                    let body = build_rebalance_mcp_tool_error_result_body(response_id, &message);
                    let request_log_id = log_rebalance_local_control_plane_response(
                        state,
                        token_id,
                        method,
                        path,
                        message_body,
                        StatusCode::OK,
                        &body,
                        proxy_session_id,
                        routing_subject_hash,
                        Some("invalid_tool_arguments"),
                    )
                    .await;
                    return Ok(proxy_response_with_mcp_body(
                        StatusCode::OK,
                        body,
                        request_log_id,
                    ));
                }
            };
            match tool.upstream_tool {
                "search" => state
                    .proxy
                    .proxy_rebalance_mcp_http_json_endpoint(
                        &state.usage_base,
                        "/search",
                        token_id,
                        method,
                        path,
                        message_body,
                        headers,
                        response_id,
                        options,
                        proxy_session_id,
                        routing_subject_hash,
                        "http_search",
                    )
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY),
                "extract" => state
                    .proxy
                    .proxy_rebalance_mcp_http_json_endpoint(
                        &state.usage_base,
                        "/extract",
                        token_id,
                        method,
                        path,
                        message_body,
                        headers,
                        response_id,
                        options,
                        proxy_session_id,
                        routing_subject_hash,
                        "http_extract",
                    )
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY),
                "crawl" => state
                    .proxy
                    .proxy_rebalance_mcp_http_json_endpoint(
                        &state.usage_base,
                        "/crawl",
                        token_id,
                        method,
                        path,
                        message_body,
                        headers,
                        response_id,
                        options,
                        proxy_session_id,
                        routing_subject_hash,
                        "http_crawl",
                    )
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY),
                "map" => state
                    .proxy
                    .proxy_rebalance_mcp_http_json_endpoint(
                        &state.usage_base,
                        "/map",
                        token_id,
                        method,
                        path,
                        message_body,
                        headers,
                        response_id,
                        options,
                        proxy_session_id,
                        routing_subject_hash,
                        "http_map",
                    )
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY),
                "research" => state
                    .proxy
                    .proxy_rebalance_mcp_http_research(
                        &state.usage_base,
                        token_id,
                        method,
                        path,
                        message_body,
                        headers,
                        response_id,
                        options,
                        proxy_session_id,
                        routing_subject_hash,
                        "http_research",
                    )
                    .await
                    .map_err(|_| StatusCode::BAD_GATEWAY),
                _ => unreachable!(),
            }
        }
        _ if method_name.starts_with("notifications/") => {
            let body = Vec::new();
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::ACCEPTED,
                &body,
                proxy_session_id,
                routing_subject_hash,
                None,
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::ACCEPTED,
                body,
                request_log_id,
            ))
        }
        _ => {
            let body = build_rebalance_mcp_error_body(response_id, -32601, "Method not found");
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                message_body,
                StatusCode::NOT_FOUND,
                &body,
                proxy_session_id,
                routing_subject_hash,
                Some("method_not_found"),
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::NOT_FOUND,
                body,
                request_log_id,
            ))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_rebalance_mcp_request_body(
    state: &Arc<AppState>,
    method: &Method,
    path: &str,
    headers: &ReqHeaderMap,
    body_bytes: &[u8],
    token_id: Option<&str>,
    proxy_session_id: Option<&str>,
    incoming_protocol_version: Option<&str>,
    routing_subject_hash: Option<&str>,
) -> Result<ProxyResponse, StatusCode> {
    let parsed = serde_json::from_slice::<Value>(body_bytes).map_err(|_| StatusCode::BAD_REQUEST)?;
    let summary = summarize_mcp_jsonrpc_body(body_bytes).map_err(|_| StatusCode::BAD_REQUEST)?;

    match parsed {
        Value::Object(_) => {
            handle_rebalance_mcp_single_message(
                state,
                method,
                path,
                headers,
                &parsed,
                body_bytes,
                token_id,
                proxy_session_id,
                incoming_protocol_version,
                routing_subject_hash,
            )
            .await
        }
        Value::Array(items) => {
            if summary.is_empty_batch || summary.is_response_only_batch() {
                let body = build_rebalance_mcp_error_body(None, -32600, "Invalid Request");
                let request_log_id = log_rebalance_local_control_plane_response(
                    state,
                    token_id,
                    method,
                    path,
                    body_bytes,
                    StatusCode::BAD_REQUEST,
                    &body,
                    proxy_session_id,
                    routing_subject_hash,
                    Some("invalid_jsonrpc_request"),
                )
                .await;
                return Ok(proxy_response_with_mcp_body(
                    StatusCode::BAD_REQUEST,
                    body,
                    request_log_id,
                ));
            }

            let mut responses: Vec<Value> = Vec::new();
            let mut last_response: Option<ProxyResponse> = None;

            for item in items {
                let item_body = serde_json::to_vec(&item).map_err(|_| StatusCode::BAD_REQUEST)?;
                let response = handle_rebalance_mcp_single_message(
                    state,
                    method,
                    path,
                    headers,
                    &item,
                    &item_body,
                    token_id,
                    proxy_session_id,
                    incoming_protocol_version,
                    routing_subject_hash,
                )
                .await?;

                if !response.body.is_empty()
                    && let Some(value) = parse_mcp_sse_message_body(&response.body)
                        .or_else(|| serde_json::from_slice::<Value>(&response.body).ok())
                {
                    responses.push(value);
                }
                last_response = Some(response);
            }

            if responses.is_empty() {
                let request_log_id = last_response
                    .as_ref()
                    .and_then(|response| response.request_log_id);
                return Ok(proxy_response_with_mcp_body(
                    StatusCode::ACCEPTED,
                    Vec::new(),
                    request_log_id,
                ));
            }

            let mut aggregated = proxy_response_with_mcp_body(
                StatusCode::OK,
                serde_json::to_vec(&Value::Array(responses))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                last_response
                    .as_ref()
                    .and_then(|response| response.request_log_id),
            );
            if let Some(last_response) = last_response {
                aggregated.api_key_id = last_response.api_key_id;
                aggregated.key_effect_code = last_response.key_effect_code;
                aggregated.key_effect_summary = last_response.key_effect_summary;
                aggregated.binding_effect_code = last_response.binding_effect_code;
                aggregated.binding_effect_summary = last_response.binding_effect_summary;
                aggregated.selection_effect_code = last_response.selection_effect_code;
                aggregated.selection_effect_summary = last_response.selection_effect_summary;
            }
            Ok(aggregated)
        }
        _ => {
            let body = build_rebalance_mcp_error_body(None, -32600, "Invalid Request");
            let request_log_id = log_rebalance_local_control_plane_response(
                state,
                token_id,
                method,
                path,
                body_bytes,
                StatusCode::BAD_REQUEST,
                &body,
                proxy_session_id,
                routing_subject_hash,
                Some("invalid_jsonrpc_request"),
            )
            .await;
            Ok(proxy_response_with_mcp_body(
                StatusCode::BAD_REQUEST,
                body,
                request_log_id,
            ))
        }
    }
}

async fn mcp_subpath_reject_handler(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    let (query, query_token) = extract_token_from_query(parts.uri.query());
    let authenticated = match authenticate_request_token(&state, &parts.headers, query_token).await
    {
        Ok(authenticated) => authenticated,
        Err(response) => return Ok(response),
    };
    if authenticated.using_dev_open_admin_fallback {
        return mcp_session_response(
            StatusCode::UNAUTHORIZED,
            "explicit_token_required",
            "MCP requests must provide an explicit token when --dev-open-admin is enabled.",
        );
    }
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let request_kind = classify_token_request_kind(&path, Some(body_bytes.as_ref()));
    let response_body = b"Not Found".as_slice();
    let empty_headers: [String; 0] = [];

    let request_log_id = match state
        .proxy
        .record_local_request_log_without_key(
            authenticated.token_id.as_deref(),
            &method,
            &path,
            query.as_deref(),
            StatusCode::NOT_FOUND,
            Some(StatusCode::NOT_FOUND.as_u16() as i64),
            &body_bytes,
            response_body,
            "error",
            Some("mcp_path_404"),
            &empty_headers,
            &empty_headers,
        )
        .await
    {
        Ok(log_id) => Some(log_id),
        Err(err) => {
            eprintln!("local MCP subpath reject request_log failed for {path}: {err}");
            None
        }
    };

    if let Some(token_id) = authenticated.token_id.as_deref() {
        let _ = state
            .proxy
            .record_token_attempt_with_kind_request_log_metadata(
                token_id,
                &method,
                &path,
                query.as_deref(),
                Some(StatusCode::NOT_FOUND.as_u16() as i64),
                Some(StatusCode::NOT_FOUND.as_u16() as i64),
                false,
                "error",
                None,
                &request_kind,
                Some("mcp_path_404"),
                Some("none"),
                None,
                None,
                None,
                None,
                None,
                request_log_id,
            )
            .await;
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(response_body.to_vec()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

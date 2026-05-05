impl TavilyProxy {

    /// 将请求透传到 Tavily upstream 并记录日志。
    pub async fn proxy_request(&self, request: ProxyRequest) -> Result<ProxyResponse, ProxyError> {
        let mut mcp_session_init_effect = KeyEffect::none();
        let lease = if let Some(key_id) = request.pinned_api_key_id.as_deref() {
            let Some(lease) = self.key_store.try_acquire_specific_key(key_id).await? else {
                return Err(ProxyError::PinnedMcpSessionUnavailable);
            };
            lease
        } else if request.prefer_mcp_session_affinity {
            let selection = self
                .acquire_key_for_mcp_session_init(request.auth_token_id.as_deref())
                .await?;
            mcp_session_init_effect = selection.key_effect;
            selection.lease
        } else {
            self.acquire_key_for(request.auth_token_id.as_deref())
                .await?
        };

        let mut url = build_mcp_upstream_url(&self.upstream, request.path.as_str());

        {
            let mut pairs = url.query_pairs_mut();
            if let Some(existing) = request.query.as_ref() {
                for (key, value) in form_urlencoded::parse(existing.as_bytes()) {
                    pairs.append_pair(&key, &value);
                }
            }
            pairs.append_pair("tavilyApiKey", lease.secret.as_str());
        }

        drop(url.query_pairs_mut());

        let sanitized_headers = self.sanitize_headers(&request.headers, &request.path);
        let gateway_mode = request.gateway_mode.as_deref().or_else(|| {
            request
                .path
                .starts_with("/mcp")
                .then_some(MCP_GATEWAY_MODE_UPSTREAM)
        });
        let experiment_variant = request
            .experiment_variant
            .as_deref()
            .or_else(|| gateway_mode.map(|_| MCP_EXPERIMENT_VARIANT_CONTROL));
        let upstream_operation = request
            .upstream_operation
            .as_deref()
            .or_else(|| request.path.starts_with("/mcp").then_some("mcp"));
        let request_method = request.method.clone();
        let request_body = request.body.clone();
        let request_url = url.clone();
        let tavily_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "mcp", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder
                    .header("Tavily-Api-Key", tavily_secret.as_str())
                    .body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                let outcome = analyze_attempt(status, &body_bytes);

                log_success(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    status,
                );

                let mut key_effect = self
                    .reconcile_key_health(
                        &lease,
                        request.path.as_str(),
                        &outcome,
                        request.auth_token_id.as_deref(),
                    )
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && outcome.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(
                            &lease.id,
                            request.path.as_str(),
                            request.auth_token_id.as_deref(),
                        )
                        .await?;
                }
                let armed_mcp_init_backoff = self
                    .maybe_arm_mcp_session_init_backoff(&lease.id, &headers, &outcome)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && armed_mcp_init_backoff {
                    key_effect =
                        if outcome.failure_kind.as_deref() == Some(FAILURE_KIND_UPSTREAM_UNKNOWN_403)
                        {
                            Self::transient_backoff_set_effect()
                        } else {
                            Self::mcp_session_init_backoff_effect()
                        };
                }
                let selection_effect = mcp_session_init_effect.clone();

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: Some(status),
                        tavily_status_code: outcome.tavily_status_code,
                        error: None,
                        request_body: &request.body,
                        response_body: &body_bytes,
                        outcome: outcome.status,
                        failure_kind: outcome.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: selection_effect.code.as_str(),
                        selection_effect_summary: selection_effect.summary.as_deref(),
                        gateway_mode,
                        experiment_variant,
                        proxy_session_id: request.proxy_session_id.as_deref(),
                        routing_subject_hash: request.routing_subject_hash.as_deref(),
                        upstream_operation,
                        fallback_reason: request.fallback_reason.as_deref(),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                self.link_transient_backoff_clear_request_log(
                    &key_effect,
                    &lease.id,
                    request_log_id,
                )
                .await?;
                if armed_mcp_init_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            MCP_SESSION_INIT_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }

                Ok(ProxyResponse {
                    status,
                    headers,
                    body: body_bytes,
                    api_key_id: Some(lease.id.clone()),
                    request_log_id: Some(request_log_id),
                    key_effect_code: key_effect.code,
                    key_effect_summary: key_effect.summary,
                    binding_effect_code: KEY_EFFECT_NONE.to_string(),
                    binding_effect_summary: None,
                    selection_effect_code: selection_effect.code,
                    selection_effect_summary: selection_effect.summary,
                })
            }
            Err(err) => {
                log_proxy_error(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    &err,
                );
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &request.body,
                        response_body: &[],
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode,
                        experiment_variant,
                        proxy_session_id: request.proxy_session_id.as_deref(),
                        routing_subject_hash: request.routing_subject_hash.as_deref(),
                        upstream_operation,
                        fallback_reason: request.fallback_reason.as_deref(),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Generic helper to proxy a Tavily HTTP JSON endpoint (e.g. `/search`, `/extract`).
    /// It injects the Tavily key into the `api_key` field, performs header sanitization,
    /// records request logs with sensitive fields redacted, and updates key quota state.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_json_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        api_routing_key: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        let selection = self
            .acquire_key_for_api_route(auth_token_id, api_routing_key)
            .await?;
        let api_route_binding_effect = selection.binding_effect;
        let api_route_selection_effect = selection.selection_effect;
        let lease = selection.lease;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let url = build_path_prefixed_url(&base, upstream_path);

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        // Build upstream request body by injecting Tavily key into api_key field.
        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            // Remove any existing api_key field (case-insensitive) before inserting the Tavily key.
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            // Unexpected payload shape; wrap it so we still send a valid JSON object upstream.
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        // Force Tavily to return usage for predictable endpoints so we can charge credits 1:1.
        // Tavily does not document/support this on `/research`; local Research billing uses
        // model-based estimates instead.
        if matches!(upstream_path, "/search" | "/extract" | "/crawl" | "/map")
            && let Value::Object(ref mut map) = upstream_options
        {
            map.insert("include_usage".to_string(), Value::Bool(true));
        }

        let request_body =
            serde_json::to_vec(&upstream_options).map_err(|e| ProxyError::Other(e.to_string()))?;
        let redacted_request_body = redact_api_key_bytes(&request_body);

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, upstream_path.trim_start_matches('/'), |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(status, &body_bytes);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(status.as_u16() as i64),
                        analysis.tavily_status_code,
                        None,
                        &body_bytes,
                    );
                }
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && upstream_path == "/research"
                    && let Some(request_id) = extract_research_request_id(&body_bytes)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                let mut key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(&lease.id, display_path, auth_token_id)
                        .await?;
                }
                let armed_api_rebalance_backoff = self
                    .maybe_arm_api_rebalance_backoff(&lease.id, &headers, &analysis)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && armed_api_rebalance_backoff {
                    key_effect = Self::transient_backoff_set_effect();
                }
                let primary_effect = Self::primary_request_effect(
                    &key_effect,
                    &api_route_binding_effect,
                    &api_route_selection_effect,
                );

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        failure_kind: analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        binding_effect_code: api_route_binding_effect.code.as_str(),
                        binding_effect_summary: api_route_binding_effect.summary.as_deref(),
                        selection_effect_code: api_route_selection_effect.code.as_str(),
                        selection_effect_summary: api_route_selection_effect.summary.as_deref(),
                        gateway_mode: None,
                        experiment_variant: None,
                        proxy_session_id: None,
                        routing_subject_hash: None,
                        upstream_operation: None,
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                self.link_transient_backoff_clear_request_log(
                    &key_effect,
                    &lease.id,
                    request_log_id,
                )
                .await?;
                if armed_api_rebalance_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            API_REBALANCE_HTTP_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }
                analysis.key_effect = primary_effect;

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                        api_key_id: Some(lease.id.clone()),
                        request_log_id: Some(request_log_id),
                        key_effect_code: key_effect.code,
                        key_effect_summary: key_effect.summary,
                        binding_effect_code: api_route_binding_effect.code,
                        binding_effect_summary: api_route_binding_effect.summary,
                        selection_effect_code: api_route_selection_effect.code,
                        selection_effect_summary: api_route_selection_effect.summary,
                    },
                    analysis,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: None,
                        experiment_variant: None,
                        proxy_session_id: None,
                        routing_subject_hash: None,
                        upstream_operation: None,
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    fn build_rebalance_mcp_tool_result_body(
        response_id: Option<&Value>,
        upstream_status: StatusCode,
        upstream_body: &[u8],
        usage_credits_override: Option<i64>,
    ) -> Vec<u8> {
        let raw_text = String::from_utf8_lossy(upstream_body).into_owned();
        let mut structured_content = match serde_json::from_slice::<Value>(upstream_body) {
            Ok(Value::Object(map)) => Value::Object(map),
            Ok(other) => serde_json::json!({ "data": other }),
            Err(_) => serde_json::json!({
                "raw_body": raw_text,
            }),
        };

        let mut is_error = !upstream_status.is_success();
        if let Value::Object(ref mut map) = structured_content {
            map.entry("status".to_string())
                .or_insert(Value::from(i64::from(upstream_status.as_u16())));
            is_error |= map.get("isError").and_then(Value::as_bool).unwrap_or(false);
            map.remove("isError");
            if let Some(usage_credits) = usage_credits_override {
                map.insert(
                    "usage".to_string(),
                    serde_json::json!({ "credits": usage_credits }),
                );
            }
        }

        let mut result = serde_json::Map::new();
        result.insert("content".to_string(), Value::Array(Vec::new()));
        result.insert("structuredContent".to_string(), structured_content);
        if !raw_text.trim().is_empty() {
            result.insert(
                "content".to_string(),
                serde_json::json!([{ "type": "text", "text": raw_text }]),
            );
        }
        if is_error {
            result.insert("isError".to_string(), Value::Bool(true));
        }

        let mut envelope = serde_json::Map::new();
        envelope.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
        if let Some(id) = response_id {
            envelope.insert("id".to_string(), id.clone());
        }
        envelope.insert("result".to_string(), Value::Object(result));

        serde_json::to_vec(&Value::Object(envelope)).unwrap_or_else(|_| {
            br#"{"jsonrpc":"2.0","result":{"content":[],"isError":true,"structuredContent":{"status":500}}}"#
                .to_vec()
        })
    }

    fn wrap_rebalance_mcp_sse_message_body(body: &[u8]) -> Bytes {
        if body.is_empty() {
            return Bytes::new();
        }
        let payload = String::from_utf8_lossy(body);
        Bytes::from(format!("event: message\ndata: {payload}\n\n"))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_rebalance_mcp_http_json_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        original_request_body: &[u8],
        original_headers: &HeaderMap,
        response_id: Option<&Value>,
        options: Value,
        proxy_session_id: Option<&str>,
        routing_subject_hash: Option<&str>,
        upstream_operation: &str,
    ) -> Result<ProxyResponse, ProxyError> {
        let lease = self.acquire_key_for_rebalance_mcp_http_call().await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let url = build_path_prefixed_url(&base, upstream_path);
        let sanitized_headers = sanitize_rebalance_mcp_http_headers_inner(original_headers);

        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|key| key.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("include_usage".to_string(), Value::Bool(true));
        } else {
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("include_usage".to_string(), Value::Bool(true));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        let request_body = serde_json::to_vec(&upstream_options)
            .map_err(|err| ProxyError::Other(err.to_string()))?;

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, upstream_path.trim_start_matches('/'), |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder
                    .header("Authorization", format!("Bearer {}", upstream_secret))
                    .body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let upstream_status = response.status();
                let upstream_headers = response.headers().clone();
                let upstream_body = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(upstream_status, &upstream_body);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(i64::from(upstream_status.as_u16())),
                        analysis.tavily_status_code,
                        None,
                        &upstream_body,
                    );
                }

                let mut key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(&lease.id, display_path, auth_token_id)
                        .await?;
                }
                let armed_backoff = self
                    .maybe_arm_rebalance_mcp_http_backoff(&lease.id, &upstream_headers, &analysis)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && armed_backoff {
                    key_effect = Self::transient_backoff_set_effect();
                }
                let response_body = Self::build_rebalance_mcp_tool_result_body(
                    response_id,
                    upstream_status,
                    &upstream_body,
                    None,
                );
                let mcp_analysis = analyze_mcp_attempt(StatusCode::OK, &response_body);
                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(StatusCode::OK),
                        tavily_status_code: mcp_analysis.tavily_status_code,
                        error: None,
                        request_body: original_request_body,
                        response_body: &response_body,
                        outcome: mcp_analysis.status,
                        failure_kind: mcp_analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: Some(MCP_GATEWAY_MODE_REBALANCE),
                        experiment_variant: Some(MCP_EXPERIMENT_VARIANT_REBALANCE),
                        proxy_session_id,
                        routing_subject_hash,
                        upstream_operation: Some(upstream_operation),
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                self.link_transient_backoff_clear_request_log(
                    &key_effect,
                    &lease.id,
                    request_log_id,
                )
                .await?;
                if armed_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            REBALANCE_MCP_HTTP_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }

                let mut headers = HeaderMap::new();
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
                let response_body = Self::wrap_rebalance_mcp_sse_message_body(&response_body);

                Ok(ProxyResponse {
                    status: StatusCode::OK,
                    headers,
                    body: response_body,
                    api_key_id: Some(lease.id),
                    request_log_id: Some(request_log_id),
                    key_effect_code: key_effect.code,
                    key_effect_summary: key_effect.summary,
                    binding_effect_code: KEY_EFFECT_NONE.to_string(),
                    binding_effect_summary: None,
                    selection_effect_code: KEY_EFFECT_NONE.to_string(),
                    selection_effect_summary: None,
                })
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let response_body = Self::build_rebalance_mcp_tool_result_body(
                    response_id,
                    StatusCode::BAD_GATEWAY,
                    err.to_string().as_bytes(),
                    None,
                );
                let mcp_analysis = analyze_mcp_attempt(StatusCode::OK, &response_body);
                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(StatusCode::OK),
                        tavily_status_code: mcp_analysis.tavily_status_code,
                        error: Some(&err.to_string()),
                        request_body: original_request_body,
                        response_body: &response_body,
                        outcome: mcp_analysis.status,
                        failure_kind: mcp_analysis.failure_kind.as_deref(),
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: Some(MCP_GATEWAY_MODE_REBALANCE),
                        experiment_variant: Some(MCP_EXPERIMENT_VARIANT_REBALANCE),
                        proxy_session_id,
                        routing_subject_hash,
                        upstream_operation: Some(upstream_operation),
                        fallback_reason: Some("upstream_http_error"),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                let mut headers = HeaderMap::new();
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
                let response_body = Self::wrap_rebalance_mcp_sse_message_body(&response_body);
                Ok(ProxyResponse {
                    status: StatusCode::OK,
                    headers,
                    body: response_body,
                    api_key_id: Some(lease.id),
                    request_log_id: Some(request_log_id),
                    key_effect_code: KEY_EFFECT_NONE.to_string(),
                    key_effect_summary: None,
                    binding_effect_code: KEY_EFFECT_NONE.to_string(),
                    binding_effect_summary: None,
                    selection_effect_code: KEY_EFFECT_NONE.to_string(),
                    selection_effect_summary: None,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_rebalance_mcp_http_research(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        original_request_body: &[u8],
        original_headers: &HeaderMap,
        response_id: Option<&Value>,
        options: Value,
        proxy_session_id: Option<&str>,
        routing_subject_hash: Option<&str>,
        upstream_operation: &str,
    ) -> Result<ProxyResponse, ProxyError> {
        let lease = self.acquire_key_for_rebalance_mcp_http_call().await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let url = build_path_prefixed_url(&base, "/research");
        let sanitized_headers = sanitize_rebalance_mcp_http_headers_inner(original_headers);

        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|key| key.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        let request_body = serde_json::to_vec(&upstream_options)
            .map_err(|err| ProxyError::Other(err.to_string()))?;

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder
                    .header("Authorization", format!("Bearer {}", upstream_secret))
                    .body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let upstream_status = response.status();
                let upstream_headers = response.headers().clone();
                let upstream_body = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(upstream_status, &upstream_body);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(i64::from(upstream_status.as_u16())),
                        analysis.tavily_status_code,
                        None,
                        &upstream_body,
                    );
                }
                if upstream_status.is_success()
                    && let Some(request_id) = extract_research_request_id(&upstream_body)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                let mut key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(&lease.id, display_path, auth_token_id)
                        .await?;
                }
                let armed_backoff = self
                    .maybe_arm_rebalance_mcp_http_backoff(&lease.id, &upstream_headers, &analysis)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && armed_backoff {
                    key_effect = Self::transient_backoff_set_effect();
                }

                let response_body = Self::build_rebalance_mcp_tool_result_body(
                    response_id,
                    upstream_status,
                    &upstream_body,
                    None,
                );
                let mcp_analysis = analyze_mcp_attempt(StatusCode::OK, &response_body);
                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(StatusCode::OK),
                        tavily_status_code: mcp_analysis.tavily_status_code,
                        error: None,
                        request_body: original_request_body,
                        response_body: &response_body,
                        outcome: mcp_analysis.status,
                        failure_kind: mcp_analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: Some(MCP_GATEWAY_MODE_REBALANCE),
                        experiment_variant: Some(MCP_EXPERIMENT_VARIANT_REBALANCE),
                        proxy_session_id,
                        routing_subject_hash,
                        upstream_operation: Some(upstream_operation),
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                self.link_transient_backoff_clear_request_log(
                    &key_effect,
                    &lease.id,
                    request_log_id,
                )
                .await?;
                if armed_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            REBALANCE_MCP_HTTP_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }

                let mut headers = HeaderMap::new();
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
                let response_body = Self::wrap_rebalance_mcp_sse_message_body(&response_body);

                Ok(ProxyResponse {
                    status: StatusCode::OK,
                    headers,
                    body: response_body,
                    api_key_id: Some(lease.id),
                    request_log_id: Some(request_log_id),
                    key_effect_code: key_effect.code,
                    key_effect_summary: key_effect.summary,
                    binding_effect_code: KEY_EFFECT_NONE.to_string(),
                    binding_effect_summary: None,
                    selection_effect_code: KEY_EFFECT_NONE.to_string(),
                    selection_effect_summary: None,
                })
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let response_body = Self::build_rebalance_mcp_tool_result_body(
                    response_id,
                    StatusCode::BAD_GATEWAY,
                    err.to_string().as_bytes(),
                    None,
                );
                let mcp_analysis = analyze_mcp_attempt(StatusCode::OK, &response_body);
                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(StatusCode::OK),
                        tavily_status_code: mcp_analysis.tavily_status_code,
                        error: Some(&err.to_string()),
                        request_body: original_request_body,
                        response_body: &response_body,
                        outcome: mcp_analysis.status,
                        failure_kind: mcp_analysis.failure_kind.as_deref(),
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: Some(MCP_GATEWAY_MODE_REBALANCE),
                        experiment_variant: Some(MCP_EXPERIMENT_VARIANT_REBALANCE),
                        proxy_session_id,
                        routing_subject_hash,
                        upstream_operation: Some(upstream_operation),
                        fallback_reason: Some("upstream_http_error"),
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                let mut headers = HeaderMap::new();
                headers.insert(
                    reqwest::header::CONTENT_TYPE,
                    HeaderValue::from_static("text/event-stream"),
                );
                let response_body = Self::wrap_rebalance_mcp_sse_message_body(&response_body);
                Ok(ProxyResponse {
                    status: StatusCode::OK,
                    headers,
                    body: response_body,
                    api_key_id: Some(lease.id),
                    request_log_id: Some(request_log_id),
                    key_effect_code: KEY_EFFECT_NONE.to_string(),
                    key_effect_summary: None,
                    binding_effect_code: KEY_EFFECT_NONE.to_string(),
                    binding_effect_summary: None,
                    selection_effect_code: KEY_EFFECT_NONE.to_string(),
                    selection_effect_summary: None,
                })
            }
        }
    }

    /// Proxy Tavily `/research`.
    ///
    /// Tavily research responses do not include per-request `usage.credits`, and shared upstream
    /// keys make `/usage.research_usage` deltas unsafe to attribute to a single Hikari user. The
    /// caller charges model-based estimated credits instead.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_research(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        api_routing_key: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis, Option<i64>), ProxyError> {
        let selection = self
            .acquire_key_for_api_route(auth_token_id, api_routing_key)
            .await?;
        let api_route_binding_effect = selection.binding_effect;
        let api_route_selection_effect = selection.selection_effect;
        let lease = selection.lease;
        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let url = build_path_prefixed_url(&base, "/research");

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        // Build upstream request body by injecting Tavily key into api_key field.
        let mut upstream_options = options;
        if let Value::Object(ref mut map) = upstream_options {
            let keys_to_remove: Vec<String> = map
                .keys()
                .filter(|k| k.eq_ignore_ascii_case("api_key"))
                .cloned()
                .collect();
            for key in keys_to_remove {
                map.remove(&key);
            }
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
        } else {
            let mut map = serde_json::Map::new();
            map.insert("api_key".to_string(), Value::String(lease.secret.clone()));
            map.insert("payload".to_string(), upstream_options);
            upstream_options = Value::Object(map);
        }

        let request_body =
            serde_json::to_vec(&upstream_options).map_err(|e| ProxyError::Other(e.to_string()))?;
        let redacted_request_body = redact_api_key_bytes(&request_body);

        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(status, &body_bytes);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(status.as_u16() as i64),
                        analysis.tavily_status_code,
                        None,
                        &body_bytes,
                    );
                }
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && let Some(request_id) = extract_research_request_id(&body_bytes)
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(&request_id, &lease.id, token_id)
                        .await?;
                }

                let mut key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(&lease.id, display_path, auth_token_id)
                        .await?;
                }
                let armed_api_rebalance_backoff = self
                    .maybe_arm_api_rebalance_backoff(&lease.id, &headers, &analysis)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && armed_api_rebalance_backoff {
                    key_effect = Self::transient_backoff_set_effect();
                }
                let primary_effect = Self::primary_request_effect(
                    &key_effect,
                    &api_route_binding_effect,
                    &api_route_selection_effect,
                );

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        failure_kind: analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        binding_effect_code: api_route_binding_effect.code.as_str(),
                        binding_effect_summary: api_route_binding_effect.summary.as_deref(),
                        selection_effect_code: api_route_selection_effect.code.as_str(),
                        selection_effect_summary: api_route_selection_effect.summary.as_deref(),
                        gateway_mode: None,
                        experiment_variant: None,
                        proxy_session_id: None,
                        routing_subject_hash: None,
                        upstream_operation: None,
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                self.link_transient_backoff_clear_request_log(
                    &key_effect,
                    &lease.id,
                    request_log_id,
                )
                .await?;
                if armed_api_rebalance_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            API_REBALANCE_HTTP_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }
                analysis.key_effect = primary_effect;

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                        api_key_id: Some(lease.id.clone()),
                        request_log_id: Some(request_log_id),
                        key_effect_code: key_effect.code,
                        key_effect_summary: key_effect.summary,
                        binding_effect_code: api_route_binding_effect.code,
                        binding_effect_summary: api_route_binding_effect.summary,
                        selection_effect_code: api_route_selection_effect.code,
                        selection_effect_summary: api_route_selection_effect.summary,
                    },
                    analysis,
                    None,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: None,
                        experiment_variant: None,
                        proxy_session_id: None,
                        routing_subject_hash: None,
                        upstream_operation: None,
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Generic helper to proxy a Tavily HTTP endpoint with no request body
    /// (for example `GET /research/{request_id}`).
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_get_endpoint(
        &self,
        usage_base: &str,
        upstream_path: &str,
        auth_token_id: Option<&str>,
        method: &Method,
        display_path: &str,
        original_headers: &HeaderMap,
        inject_upstream_bearer_auth: bool,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        let research_request_id = extract_research_request_id_from_path(upstream_path);
        let lease = self
            .acquire_key_for_research_request(auth_token_id, research_request_id.as_deref())
            .await?;

        let base = Url::parse(usage_base).map_err(|source| ProxyError::InvalidEndpoint {
            endpoint: usage_base.to_owned(),
            source,
        })?;
        let origin = origin_from_url(&base);

        let url = build_path_prefixed_url(&base, upstream_path);

        let sanitized_headers = sanitize_headers_inner(original_headers, &base, &origin);

        let redacted_request_body: Vec<u8> = Vec::new();
        let request_method = method.clone();
        let request_url = url.clone();
        let upstream_secret = lease.secret.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, "research_result", |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                if inject_upstream_bearer_auth {
                    builder =
                        builder.header("Authorization", format!("Bearer {}", upstream_secret));
                }
                builder
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;

                let mut analysis = analyze_http_attempt(status, &body_bytes);
                analysis.api_key_id = Some(lease.id.clone());
                if analysis.failure_kind.is_none() && analysis.status == OUTCOME_ERROR {
                    analysis.failure_kind = classify_failure_kind(
                        display_path,
                        Some(status.as_u16() as i64),
                        analysis.tavily_status_code,
                        None,
                        &body_bytes,
                    );
                }
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                if status.is_success()
                    && let Some(request_id) = research_request_id.as_deref()
                    && let Some(token_id) = auth_token_id
                {
                    self.record_research_request_affinity(request_id, &lease.id, token_id)
                        .await?;
                }

                let mut key_effect = self
                    .reconcile_key_health(&lease, display_path, &analysis, auth_token_id)
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(&lease.id, display_path, auth_token_id)
                        .await?;
                }
                let armed_api_rebalance_backoff = self
                    .maybe_arm_api_rebalance_backoff(&lease.id, &headers, &analysis)
                    .await?;
                let armed_http_global_backoff = self
                    .maybe_arm_http_global_backoff(&lease.id, &headers, &analysis)
                    .await?;
                let armed_mcp_init_backoff = if analysis.failure_kind.as_deref()
                    == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
                {
                    self.maybe_arm_mcp_session_init_backoff(&lease.id, &headers, &analysis)
                        .await?
                } else {
                    false
                };
                if key_effect.code == KEY_EFFECT_NONE && armed_api_rebalance_backoff {
                    key_effect = Self::transient_backoff_set_effect();
                } else if key_effect.code == KEY_EFFECT_NONE && armed_mcp_init_backoff {
                    key_effect = Self::mcp_session_init_backoff_effect();
                } else if key_effect.code == KEY_EFFECT_NONE && armed_http_global_backoff {
                    key_effect = Self::transient_backoff_set_effect();
                }

                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: Some(status),
                        tavily_status_code: analysis.tavily_status_code,
                        error: None,
                        request_body: &redacted_request_body,
                        response_body: &redacted_response_body,
                        outcome: analysis.status,
                        failure_kind: analysis.failure_kind.as_deref(),
                        key_effect_code: key_effect.code.as_str(),
                        key_effect_summary: key_effect.summary.as_deref(),
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: None,
                        experiment_variant: None,
                        proxy_session_id: None,
                        routing_subject_hash: None,
                        upstream_operation: None,
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                self.link_transient_backoff_clear_request_log(
                    &key_effect,
                    &lease.id,
                    request_log_id,
                )
                .await?;
                if armed_http_global_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            HTTP_GLOBAL_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }
                if armed_api_rebalance_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            API_REBALANCE_HTTP_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }
                if armed_mcp_init_backoff {
                    self.key_store
                        .set_api_key_transient_backoff_request_log_id(
                            &lease.id,
                            MCP_SESSION_INIT_BACKOFF_SCOPE,
                            request_log_id,
                            Utc::now().timestamp(),
                        )
                        .await?;
                }
                analysis.key_effect = key_effect.clone();

                Ok((
                    ProxyResponse {
                        status,
                        headers,
                        body: body_bytes,
                        api_key_id: Some(lease.id.clone()),
                        request_log_id: Some(request_log_id),
                        key_effect_code: key_effect.code,
                        key_effect_summary: key_effect.summary,
                        binding_effect_code: KEY_EFFECT_NONE.to_string(),
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE.to_string(),
                        selection_effect_summary: None,
                    },
                    analysis,
                ))
            }
            Err(err) => {
                log_proxy_error(&lease.secret, method, display_path, None, &err);
                let redacted_empty: Vec<u8> = Vec::new();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id,
                        method,
                        path: display_path,
                        query: None,
                        status: None,
                        tavily_status_code: None,
                        error: Some(&err.to_string()),
                        request_body: &redacted_request_body,
                        response_body: &redacted_empty,
                        outcome: OUTCOME_ERROR,
                        failure_kind: None,
                        key_effect_code: KEY_EFFECT_NONE,
                        key_effect_summary: None,
                        binding_effect_code: KEY_EFFECT_NONE,
                        binding_effect_summary: None,
                        selection_effect_code: KEY_EFFECT_NONE,
                        selection_effect_summary: None,
                        gateway_mode: None,
                        experiment_variant: None,
                        proxy_session_id: None,
                        routing_subject_hash: None,
                        upstream_operation: None,
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                    })
                    .await?;
                Err(err)
            }
        }
    }

    /// Proxy a Tavily HTTP `/search` call via the usage base URL, performing key rotation
    /// and recording request logs with sensitive fields redacted.
    #[allow(clippy::too_many_arguments)]
    pub async fn proxy_http_search(
        &self,
        usage_base: &str,
        auth_token_id: Option<&str>,
        api_routing_key: Option<&str>,
        method: &Method,
        display_path: &str,
        options: Value,
        original_headers: &HeaderMap,
    ) -> Result<(ProxyResponse, AttemptAnalysis), ProxyError> {
        self.proxy_http_json_endpoint(
            usage_base,
            "/search",
            auth_token_id,
            api_routing_key,
            method,
            display_path,
            options,
            original_headers,
            true,
        )
        .await
    }

    /// 获取全部 API key 的统计信息，按状态与最近使用时间排序。
    pub async fn list_api_key_metrics(&self) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metrics(false).await
    }

    pub async fn list_dashboard_exhausted_key_metrics(
        &self,
        limit: usize,
    ) -> Result<Vec<ApiKeyMetrics>, ProxyError> {
        self.key_store
            .fetch_dashboard_exhausted_api_key_metrics(limit)
            .await
    }

    pub async fn list_dashboard_exhausted_key_ids(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        self.key_store
            .fetch_dashboard_exhausted_api_key_ids(limit)
            .await
    }

    /// Admin: list API key metrics with pagination and optional filters.
    pub async fn list_api_key_metrics_paged(
        &self,
        page: i64,
        per_page: i64,
        groups: &[String],
        statuses: &[String],
        registration_ip: Option<&str>,
        regions: &[String],
    ) -> Result<PaginatedApiKeyMetrics, ProxyError> {
        self.key_store
            .fetch_api_key_metrics_page(page, per_page, groups, statuses, registration_ip, regions)
            .await
    }

    /// 获取单个 API key 的完整统计信息，包含隔离详情。
    pub async fn get_api_key_metric(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyMetrics>, ProxyError> {
        self.key_store.fetch_api_key_metric_by_id(key_id).await
    }

    /// 获取最近的请求日志，按时间倒序排列。
    pub async fn recent_request_logs(
        &self,
        limit: usize,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_recent_logs(limit).await
    }

    pub async fn latest_visible_request_log_id(&self) -> Result<Option<i64>, ProxyError> {
        self.key_store.fetch_latest_visible_request_log_id().await
    }

    /// Admin: recent request logs with simple pagination and optional result_status filter.
    pub async fn recent_request_logs_page(
        &self,
        result_status: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<RequestLogRecord>, i64), ProxyError> {
        self.key_store
            .fetch_recent_logs_page(result_status, operational_class, page, per_page)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn request_logs_page(
        &self,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        page: i64,
        per_page: i64,
        include_bodies: bool,
    ) -> Result<RequestLogsPage, ProxyError> {
        let since = Some(request_logs_retention_threshold_utc_ts(
            effective_request_logs_retention_days(),
        ));
        self.key_store
            .fetch_request_logs_page(
                None,
                since,
                request_kinds,
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                key_id,
                operational_class,
                page,
                per_page,
                true,
                true,
                include_bodies,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn request_logs_list(
        &self,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
        page_size: i64,
    ) -> Result<RequestLogsCursorPage, ProxyError> {
        let since = Some(request_logs_retention_threshold_utc_ts(
            effective_request_logs_retention_days(),
        ));
        self.key_store
            .fetch_request_logs_cursor_page(
                None,
                since,
                request_kinds,
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                key_id,
                operational_class,
                cursor,
                direction,
                page_size,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn request_logs_catalog(
        &self,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        key_id: Option<&str>,
        operational_class: Option<&str>,
    ) -> Result<RequestLogsCatalog, ProxyError> {
        let since = Some(request_logs_retention_threshold_utc_ts(
            effective_request_logs_retention_days(),
        ));
        self.key_store
            .fetch_request_logs_catalog(
                None,
                since,
                true,
                true,
                RequestLogsCatalogFilters {
                    request_kinds,
                    result_status,
                    key_effect_code,
                    binding_effect_code,
                    selection_effect_code,
                    auth_token_id,
                    key_id,
                    operational_class,
                },
            )
            .await
    }

    pub async fn request_log_bodies(
        &self,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        self.key_store.fetch_request_log_bodies(log_id).await
    }

    /// Rebuild API-key request buckets from visible request logs.
    pub async fn rebuild_api_key_usage_buckets(&self) -> Result<(), ProxyError> {
        self.key_store.rebuild_api_key_usage_buckets().await
    }

    /// 获取指定 key 在起始时间以来的汇总。
    pub async fn key_summary_since(
        &self,
        key_id: &str,
        since: i64,
    ) -> Result<ProxySummary, ProxyError> {
        self.key_store.fetch_key_summary_since(key_id, since).await
    }

    /// 获取指定 key 的最近日志（可选起始时间过滤）。
    pub async fn key_recent_logs(
        &self,
        key_id: &str,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        self.key_store.fetch_key_logs(key_id, limit, since).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn key_logs_page(
        &self,
        key_id: &str,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        page: i64,
        per_page: i64,
    ) -> Result<RequestLogsPage, ProxyError> {
        let since = Some(since.unwrap_or_else(|| {
            request_logs_retention_threshold_utc_ts(effective_request_logs_retention_days())
        }));
        self.key_store
            .fetch_request_logs_page(
                Some(key_id),
                since,
                request_kinds,
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                None,
                None,
                page,
                per_page,
                true,
                false,
                false,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn key_logs_list(
        &self,
        key_id: &str,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        operational_class: Option<&str>,
        cursor: Option<&RequestLogsCursor>,
        direction: RequestLogsCursorDirection,
        page_size: i64,
    ) -> Result<RequestLogsCursorPage, ProxyError> {
        let since = Some(since.unwrap_or_else(|| {
            request_logs_retention_threshold_utc_ts(effective_request_logs_retention_days())
        }));
        self.key_store
            .fetch_request_logs_cursor_page(
                Some(key_id),
                since,
                request_kinds,
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                auth_token_id,
                None,
                operational_class,
                cursor,
                direction,
                page_size,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn key_logs_catalog(
        &self,
        key_id: &str,
        since: Option<i64>,
        request_kinds: &[String],
        result_status: Option<&str>,
        key_effect_code: Option<&str>,
        binding_effect_code: Option<&str>,
        selection_effect_code: Option<&str>,
        auth_token_id: Option<&str>,
        operational_class: Option<&str>,
    ) -> Result<RequestLogsCatalog, ProxyError> {
        let since = Some(since.unwrap_or_else(|| {
            request_logs_retention_threshold_utc_ts(effective_request_logs_retention_days())
        }));
        self.key_store
            .fetch_request_logs_catalog(
                Some(key_id),
                since,
                true,
                false,
                RequestLogsCatalogFilters {
                    request_kinds,
                    result_status,
                    key_effect_code,
                    binding_effect_code,
                    selection_effect_code,
                    auth_token_id,
                    key_id: None,
                    operational_class,
                },
            )
            .await
    }

    pub async fn key_request_log_bodies(
        &self,
        key_id: &str,
        log_id: i64,
    ) -> Result<Option<RequestLogBodiesRecord>, ProxyError> {
        self.key_store
            .fetch_key_request_log_bodies(key_id, log_id)
            .await
    }

    pub async fn key_sticky_users_paged(
        &self,
        key_id: &str,
        page: i64,
        per_page: i64,
    ) -> Result<PaginatedApiKeyStickyUsers, ProxyError> {
        self.key_store
            .fetch_key_sticky_users_page(key_id, page, per_page)
            .await
    }

    pub async fn key_sticky_nodes(
        &self,
        key_id: &str,
    ) -> Result<ApiKeyStickyNodesResponse, ProxyError> {
        let record = self.resolve_proxy_affinity_record(key_id, false).await?;
        let manager = self.forward_proxy.lock().await.clone();
        let live =
            forward_proxy::build_forward_proxy_live_stats_response(&self.key_store.pool, &manager)
                .await?;
        let mut nodes = Vec::new();
        for (role, proxy_key) in [
            ("primary", record.primary_proxy_key.as_deref()),
            ("secondary", record.secondary_proxy_key.as_deref()),
        ] {
            let Some(proxy_key) = proxy_key else {
                continue;
            };
            if let Some(node) = live.nodes.iter().find(|node| node.key == proxy_key) {
                nodes.push(ApiKeyStickyNode {
                    role,
                    node: node.clone(),
                });
            }
        }
        Ok(ApiKeyStickyNodesResponse {
            range_start: live.range_start,
            range_end: live.range_end,
            bucket_seconds: live.bucket_seconds,
            nodes,
        })
    }

}

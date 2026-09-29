//! Exa LOCAL MCP facade execution layer (plan todo 17; amendments M12/M19/M22).
//!
//! `/exa/mcp` is a Hikari-local facade that translates `tools/call` into
//! pooled-key Exa REST POSTs. This module is the lib-side executor for that
//! translation: it mirrors the brave local-MCP executor
//! (`proxy_provider_facade_brave_mcp`) with the POST shape — translated JSON
//! body on the wire — and re-uses the ENTIRE committed todo-13 Exa record path
//! ([`TavilyProxy::record_exa_rest_facade_attempt`]) so classification →
//! reconcile_key_health (402 budget tags → exhausted with SuccessOnly
//! recovery, 401 → auth-fail quarantine, 429 → per-key Retry-After-sized
//! shared HTTP backoff — M12: no new global limiter, Exa QPS is team-wide),
//! and the `provider='exa'` request_logs row are IDENTICAL to the REST facade.
//! Deeper alerting/health wiring across providers remains todo 18.
//!
//! request_logs keying (todo-17 contract): the audit identity carried in the
//! [`ProviderRestFacadeRequest`] is the CLIENT-facing MCP request — method
//! POST, path `/exa/mcp`, `kind_path` `/exa/mcp`, `request_body` = the
//! client's `tools/call` JSON — so the store derives
//! `mcp:exa/{web_search,web_fetch,answer}` from (path, body) via
//! `classify_token_request_kind` (the committed todo-3 kinds), while the
//! record path stamps `provider='exa'`. The wire POST (translated REST body +
//! injected `x-api-key`) is decoupled from that audit body.
//!
//! Tavily/SerpApi/Brave never reach this seam (provider-gated). Tavily
//! rebalance keeps its own executors (`proxy_rebalance_mcp_http_json_endpoint`)
//! byte-stable; acquisition is the generalized todo-9
//! `acquire_key_for_provider` (never the Tavily-pinned rebalance acquire).
//! Client key slots were stripped at the server boundary (todo 11 / M17), so
//! no client-supplied provider auth is ever relayed; the pooled key rides the
//! ProviderSpec `x-api-key` header slot.

use super::*;

impl TavilyProxy {
    /// Execute one Exa local-MCP `tools/call` as a pooled-key Exa REST POST.
    ///
    /// `request` carries the CLIENT-facing audit identity (provider = Exa,
    /// method POST, path/kind_path `/exa/mcp`, body = the tools/call JSON,
    /// stripped client headers, upstream_path + upstream_operation for the
    /// translated call). `translated_body` is the Exa REST JSON body built by
    /// the bin-side translate step (T13 stream-gate + api_key-strip already
    /// applied). Returns the RAW upstream response (native status + body) —
    /// the bin side wraps it into the MCP tool-result envelope; billing +
    /// envelope stay bin-side (the rebalance split).
    pub async fn proxy_provider_mcp_exa_facade_call(
        &self,
        request: ProviderRestFacadeRequest,
        translated_body: Vec<u8>,
    ) -> Result<ProxyResponse, ProxyError> {
        if request.provider != providers::ProviderId::Exa {
            return Err(ProxyError::Other(format!(
                "{} MCP surface is not the exa local facade",
                request.provider
            )));
        }
        let base = self.provider_rest_base(request.provider)?;
        let Some(sanitize) = provider_rest_header_sanitizer(request.provider) else {
            return Err(ProxyError::Other(
                "exa MCP facade requires the exa REST header sanitizer".to_string(),
            ));
        };
        let spec = providers::spec_for(request.provider);

        // Provider-scoped acquire through the generalized seam (plan todo 9
        // `acquire_key_for_provider`). Tavily rebalance keeps its own pinned
        // acquire — byte-stable and never reached here.
        let lease = self
            .acquire_key_for_provider(request.provider, request.auth_token_id.as_deref())
            .await?;

        // M1: path-prefix-compatible URL building against the base parsed once
        // at construction. The translated call is a POST — no query pairs.
        let url = build_path_prefixed_url(&base, request.upstream_path);

        let mut sanitized_headers = sanitize(&request.headers);
        // M2: x-project-id is a Tavily-only upstream identity policy; the
        // provider gate strips forged client values for third-party providers.
        self.apply_upstream_project_id_header_for_provider(
            request.provider,
            &mut sanitized_headers,
            &request.headers,
            request.auth_token_id.as_deref(),
        )
        .await?;

        // The wire body is the bin-side translated REST JSON (never the
        // client's JSON-RPC envelope), so its content type is declared HERE,
        // independent of whatever the MCP POST carried for the envelope.
        sanitized_headers.headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        if !sanitized_headers
            .forwarded
            .iter()
            .any(|name| name == "content-type")
        {
            sanitized_headers.forwarded.push("content-type".to_string());
        }

        // Pooled upstream key into Exa's native header slot (x-api-key). The
        // client slot was stripped at the server boundary (M17), so no
        // client-supplied provider auth is ever relayed.
        let upstream_secret = lease.secret.clone();
        match provider_rest_key_injection(request.provider, spec) {
            providers::KeyAuthInjection::Header { name } => {
                let header_name = HeaderName::try_from(name)
                    .map_err(|_| ProxyError::Other(format!("invalid exa header name: {name}")))?;
                let header_value = HeaderValue::from_str(upstream_secret.as_str())
                    .map_err(|_| ProxyError::Other("invalid exa key header value".to_string()))?;
                sanitized_headers.headers.insert(header_name, header_value);
                sanitized_headers.forwarded.push(name.to_ascii_lowercase());
            }
            providers::KeyAuthInjection::QueryParam { .. }
            | providers::KeyAuthInjection::Bearer => {
                return Err(ProxyError::Other(
                    "exa MCP facade injects the pooled key via header only".to_string(),
                ));
            }
        }

        // Wire call: POST with the translated body. The audit `request.body`
        // carries the client MCP JSON for request_kind derivation and is NEVER
        // sent upstream.
        let request_url = url;
        let upstream_operation = request.upstream_operation;
        let response = self
            .send_with_forward_proxy(&lease.id, upstream_operation, |client| {
                let mut builder = client.request(Method::POST, request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder.body(translated_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                // Todo-3 classification feeding the ENTIRE committed todo-13
                // Exa record path: reconcile_key_health (402 budget tags →
                // exhausted, 401 → quarantine), Retry-After-sized 429 backoff
                // arming (M12 per-key), and the provider='exa' request_logs
                // row keyed on (/exa/mcp, tools/call body) → mcp:exa/*.
                let signal = classify_provider_failure(
                    request.provider,
                    &ProviderFailureEvidence {
                        status: status.as_u16(),
                        headers: Some(&headers),
                        body: &body_bytes,
                    },
                );
                let attempt_ctx = ProviderFacadeAttemptContext {
                    request: &request,
                    lease: &lease,
                    sanitized_headers: &sanitized_headers,
                    status,
                    body_bytes: &body_bytes,
                    signal,
                };
                let recorded = self.record_exa_rest_facade_attempt(&attempt_ctx).await?;
                Ok(ProxyResponse {
                    status,
                    headers,
                    body: body_bytes,
                    api_key_id: Some(lease.id.clone()),
                    request_log_id: recorded.request_log_id,
                    key_effect_code: recorded.key_effect.code,
                    key_effect_summary: recorded.key_effect.summary,
                    binding_effect_code: KEY_EFFECT_NONE.to_string(),
                    binding_effect_summary: None,
                    selection_effect_code: KEY_EFFECT_NONE.to_string(),
                    selection_effect_summary: None,
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
                // Transport failure persists an audit row (keyed on /exa/mcp +
                // the MCP body) exactly like the REST facade transport path.
                self.log_exa_rest_facade_transport_error(
                    &request,
                    &lease,
                    &sanitized_headers,
                    &err,
                )
                .await?;
                Err(err)
            }
        }
    }
}

//! Brave LOCAL MCP facade execution layer (plan todo 16; amendments M19/M22).
//!
//! Brave hosts NO MCP server, so `/brave/mcp` is a Hikari-local facade that
//! translates `tools/call` into pooled-key Brave REST GETs. This module is the
//! lib-side executor for that translation: it mirrors the generic REST seam
//! (`proxy_provider_rest_facade`) MINUS client-body forwarding (a GET has an
//! empty wire body) and re-uses the ENTIRE committed todo-14 Brave record path
//! ([`TavilyProxy::record_brave_rest_facade_attempt`]) so classification →
//! reconcile, second-scale 429 backoff arming, M11 passive quota harvest, and
//! the `provider='brave'` request_logs row are IDENTICAL to the REST facade.
//!
//! request_logs keying (todo-16 contract): the audit identity carried in the
//! [`ProviderRestFacadeRequest`] is the CLIENT-facing MCP request — method POST,
//! path `/brave/mcp`, `request_body` = the client's `tools/call` JSON — so the
//! store derives `mcp:brave/{web_search,news_search,llm_context}` from
//! (path, body) via `classify_token_request_kind` (the committed todo-3 kinds),
//! while the record path stamps `provider='brave'`. The wire GET (translated
//! query params + injected `X-Subscription-Token`, empty body) is decoupled from
//! that audit body.
//!
//! Tavily/SerpApi/Exa never reach this seam (provider-gated). Tavily rebalance
//! keeps its own executors (`proxy_rebalance_mcp_http_json_endpoint`)
//! byte-stable; the generalized acquire seam is todo-9 `acquire_key_for_provider`
//! (never the Tavily-pinned `acquire_key_for_rebalance_mcp_http_call`).

use super::*;

impl TavilyProxy {
    /// Execute one Brave local-MCP `tools/call` as a pooled-key Brave REST GET.
    ///
    /// `request` carries the CLIENT-facing audit identity (provider = Brave,
    /// method POST, path/kind_path `/brave/mcp`, body = the tools/call JSON,
    /// stripped client headers, upstream_path + upstream_operation for the
    /// translated call). `translated_query` is the Brave GET query string built
    /// by the bin-side translate step. Returns the RAW upstream response (native
    /// status + body) — the bin side wraps it into the MCP tool-result envelope;
    /// billing + envelope stay bin-side (the rebalance split).
    pub async fn proxy_provider_mcp_brave_facade_call(
        &self,
        request: ProviderRestFacadeRequest,
        translated_query: Option<String>,
    ) -> Result<ProxyResponse, ProxyError> {
        if request.provider != providers::ProviderId::Brave {
            return Err(ProxyError::Other(format!(
                "{} MCP surface is not the brave local facade",
                request.provider
            )));
        }
        let base = self.provider_rest_base(request.provider)?;
        let Some(sanitize) = provider_rest_header_sanitizer(request.provider) else {
            return Err(ProxyError::Other(
                "brave MCP facade requires the brave REST header sanitizer".to_string(),
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
        // at construction, then the translated query pairs appended.
        let mut url = build_path_prefixed_url(&base, request.upstream_path);
        if let Some(query) = translated_query.as_deref() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                pairs.append_pair(&key, &value);
            }
            drop(pairs);
        }

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

        // Pooled upstream key into Brave's native header slot
        // (X-Subscription-Token). The client slot was stripped at the server
        // boundary (M17), so no client-supplied provider auth is ever relayed.
        let upstream_secret = lease.secret.clone();
        match provider_rest_key_injection(request.provider, spec) {
            providers::KeyAuthInjection::Header { name } => {
                let header_name = HeaderName::try_from(name)
                    .map_err(|_| ProxyError::Other(format!("invalid brave header name: {name}")))?;
                let header_value = HeaderValue::from_str(upstream_secret.as_str())
                    .map_err(|_| ProxyError::Other("invalid brave key header value".to_string()))?;
                sanitized_headers.headers.insert(header_name, header_value);
                sanitized_headers.forwarded.push(name.to_ascii_lowercase());
            }
            providers::KeyAuthInjection::QueryParam { .. }
            | providers::KeyAuthInjection::Bearer => {
                return Err(ProxyError::Other(
                    "brave MCP facade injects the pooled key via header only".to_string(),
                ));
            }
        }

        // Wire call: GET with an EMPTY body. The audit `request.body` carries the
        // client MCP JSON for request_kind derivation and is NEVER sent upstream.
        let request_url = url.clone();
        let upstream_operation = request.upstream_operation;
        let response = self
            .send_with_forward_proxy(&lease.id, upstream_operation, |client| {
                let mut builder = client.request(Method::GET, request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                // Todo-3 classification feeding the ENTIRE committed todo-14
                // Brave record path: reconcile_key_health (402→exhausted,
                // 422-token→quarantine), second-scale 429 backoff arming, M11
                // passive quota harvest, and the provider='brave' request_logs
                // row keyed on (/brave/mcp, tools/call body) → mcp:brave/*.
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
                let recorded = self
                    .record_brave_rest_facade_attempt(&attempt_ctx, &headers)
                    .await?;
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
                // Transport failure persists an audit row (keyed on /brave/mcp +
                // the MCP body) exactly like the REST facade transport path.
                self.log_brave_rest_facade_transport_error(&request, &lease, &sanitized_headers, &err)
                    .await?;
                Err(err)
            }
        }
    }
}

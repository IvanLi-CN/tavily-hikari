//! SerpApi hosted MCP passthrough execution layer (plan todo 15; amendments
//! M2/M6/M7/M17).
//!
//! Layered over the T11 ingress-aware URL builders and the T12 SerpApi
//! classification contract:
//! * upstream URL via [`build_provider_mcp_upstream_url`] so a path-prefixed
//!   `SERPAPI_MCP_UPSTREAM` (e.g. `http://host/gw/mcp`) is targeted exactly
//!   (M1 acceptance vector);
//! * `Authorization: Bearer <pooled key secret>` injection per the ProviderSpec
//!   registry — NEVER the Tavily-only `tavilyApiKey` query slot or
//!   `Tavily-Api-Key` header (high-anonymity contract; absence asserted at the
//!   mock in tests/provider_mcp_e2e.rs);
//! * header allowlist + UA policy IDENTICAL to Tavily Control mode
//!   ([`sanitize_mcp_headers_inner`] reused unchanged, per the T4 note);
//! * x-project-id is never emitted (M2 gate, shared provider-gated stripper);
//! * byte-passthrough request/response bodies (SSE frames included — M22
//!   transformation ban);
//! * failure classification through the committed todo-3 classifier
//!   ([`classify_provider_failure`]) projected onto [`AttemptAnalysis`] so the
//!   EXISTING `reconcile_key_health` machinery applies the acceptance health
//!   actions: 401 → auth-fail quarantine, run-out 429 (raw OR SSE-wrapped —
//!   both flavors covered by the classifier's envelope parser) → exhausted,
//!   hourly-flavor 429 → transient backoff on the shared HTTP scope;
//! * request_logs rows stamped `provider='serpapi'`,
//!   `gateway_mode='upstream_mcp'`, the additive provider-scoped
//!   `upstream_operation` value `serpapi_mcp` (M6: todo 27 ratifies the closed
//!   -enum SPEC amendment), and the proxy session id for follow-up audits;
//! * provider-scoped mcp_sessions stamping/resolution wrappers (M6) so Tavily
//!   rows keep their byte-stable NULL provider default.
//!
//! Tavily MCP never reaches this seam (legacy executors stay untouched), and
//! Exa/Brave MCP surfaces are local facades owned by todos 16-17: the
//! executor's provider gate rejects them until their owner todo lands.

use super::*;

/// Additive provider-scoped `upstream_operation` vocabulary for hosted MCP
/// passthrough (amendment M6). The rebalance-mcp-gateway SPEC enum is closed;
/// todo 27 records these values as the additive amendment. Tavily keeps its
/// legacy `"mcp"` value (owned by `mcp_upstream_operation_default`); Exa/Brave
/// are local facades (todos 16-17) and have no hosted-passthrough operation.
pub fn provider_mcp_upstream_operation(provider: providers::ProviderId) -> Option<&'static str> {
    match provider {
        providers::ProviderId::Serpapi => Some("serpapi_mcp"),
        providers::ProviderId::Tavily
        | providers::ProviderId::Exa
        | providers::ProviderId::Brave => None,
    }
}

/// Client-facing description of one hosted-MCP passthrough call. The server
/// layer owns auth extraction, client-slot stripping, session resolution, and
/// quota gates BEFORE building this; the executor owns the upstream URL, key
/// injection, header policy, classification, and audit persistence.
#[derive(Debug, Clone)]
pub struct ProviderMcpPassthroughRequest {
    pub provider: providers::ProviderId,
    pub method: Method,
    /// Client-facing ingress path (`/serpapi/mcp`) — audit identity and the
    /// ingress prefix stripped by the upstream URL builder.
    pub path: String,
    /// Client query with every provider client-auth slot already stripped.
    pub query: Option<String>,
    /// Client headers with Authorization and native key slots removed; the
    /// MCP allowlist is applied inside the executor.
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub auth_token_id: Option<String>,
    /// Pinned pooled key from the resolved proxy session (follow-ups).
    pub pinned_api_key_id: Option<String>,
    /// Upstream protocol session id to present on follow-ups; the executor
    /// replaces any client-visible `mcp-session-id` with this value.
    pub upstream_session_id: Option<String>,
    /// Proxy session id stamped on request_logs rows (audit correlation).
    pub proxy_session_id: Option<String>,
    pub client_ip: Option<ClientIpInfo>,
}

/// Classify + project one buffered hosted-MCP upstream response onto the
/// shared [`AttemptAnalysis`] contract (mirrors the T12 REST projection so
/// `reconcile_key_health` applies the identical health actions; the
/// REST-side helper stays in its byte-frozen module).
fn provider_mcp_attempt_analysis(
    provider: providers::ProviderId,
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> AttemptAnalysis {
    let (result_status, key_health_action) = match signal {
        ProviderFailureSignal::Ok => (OUTCOME_SUCCESS, KeyHealthAction::None),
        ProviderFailureSignal::Exhausted => {
            (OUTCOME_QUOTA_EXHAUSTED, KeyHealthAction::MarkExhausted)
        }
        // Auth-fail feeds the EXISTING quarantine machinery (T12 precedent);
        // provider-tagged alert payloads and recovery policy stay todo 18.
        // M8: new providers never write api_key_low_quota_depletions.
        ProviderFailureSignal::AuthFailed => (
            OUTCOME_ERROR,
            KeyHealthAction::Quarantine(QuarantineDecision {
                reason_code: BLOCKED_KEY_REASON_INVALID_API_KEY.to_string(),
                reason_summary: format!(
                    "{provider} rejected the pooled API key as invalid (HTTP {})",
                    status.as_u16()
                ),
                reason_detail: truncate_text(
                    String::from_utf8_lossy(body).trim(),
                    QUARANTINE_REASON_DETAIL_MAX_LEN,
                ),
            }),
        ),
        ProviderFailureSignal::RateLimited { .. }
        | ProviderFailureSignal::ClientError
        | ProviderFailureSignal::ServerTransient => (OUTCOME_ERROR, KeyHealthAction::None),
    };
    AttemptAnalysis {
        status: result_status,
        // The legacy column carries the upstream HTTP status verbatim for
        // provider rows (todo 12 logging contract).
        tavily_status_code: Some(i64::from(status.as_u16())),
        key_health_action,
        failure_kind: signal.failure_kind().map(str::to_string),
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

impl TavilyProxy {
    /// Hosted MCP passthrough executor (plan todo 15). Byte-passthrough with
    /// pooled-key Bearer injection, provider-scoped session affinity via the
    /// pinned key, T3 classification feeding the existing key-health
    /// reconcile, and provider/gateway-stamped request_logs rows.
    pub async fn proxy_provider_mcp_passthrough(
        &self,
        request: ProviderMcpPassthroughRequest,
    ) -> Result<ProxyResponse, ProxyError> {
        if request.provider != providers::ProviderId::Serpapi {
            return Err(ProxyError::Other(format!(
                "{} hosted MCP passthrough is not wired yet",
                request.provider
            )));
        }
        let operation = provider_mcp_upstream_operation(request.provider).ok_or_else(|| {
            ProxyError::Other(format!(
                "{} has no hosted MCP upstream operation",
                request.provider
            ))
        })?;

        // Provider-scoped acquire. MCP passthrough affinity lives in the
        // session pin (follow-ups re-lease `pinned_api_key_id`), so the
        // initialize acquire uses the provider-scoped global LRU lane and
        // deliberately does NOT write the token primary affinity (M6):
        // that table has no provider dimension, and a cross-provider primary
        // write makes the next Tavily acquire rebind + revoke sessions
        // pinned to the previous key — entangling the two provider surfaces.
        // (The pre-existing REST-facade instance of this gap is documented
        // for todo 18's provider-dimension work.)
        let lease = if let Some(key_id) = request.pinned_api_key_id.as_deref() {
            let Some(lease) = self
                .key_store
                .try_acquire_specific_key_for_provider(request.provider, key_id)
                .await?
            else {
                return Err(ProxyError::PinnedMcpSessionUnavailable);
            };
            lease
        } else {
            self.key_store
                .acquire_key_avoiding_transient_backoff(request.provider, HTTP_GLOBAL_BACKOFF_SCOPE)
                .await?
        };

        // M1: ingress-aware upstream URL — SERPAPI_MCP_UPSTREAM is honored
        // including path-prefixed values; the client query (already stripped
        // of every client auth slot) is appended verbatim.
        let base = self.provider_upstreams.serpapi_mcp.clone();
        let mut url = build_provider_mcp_upstream_url(request.provider, &base, &request.path);
        if let Some(query) = request.query.as_deref() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                pairs.append_pair(&key, &value);
            }
        }
        drop(url.query_pairs_mut());

        // Header allowlist + UA policy identical to Tavily Control mode (T4).
        let settings = self.key_store.get_system_settings().await?;
        let mut sanitized_headers =
            sanitize_mcp_headers_inner(&request.headers, Some(&settings.upstream_mcp_user_agent));
        // M2 GUARD: x-project-id is a Tavily-only upstream identity policy —
        // never emitted on third-party outbound, forged client values removed.
        self.apply_upstream_project_id_header_for_provider(
            request.provider,
            &mut sanitized_headers,
            &request.headers,
            request.auth_token_id.as_deref(),
        )
        .await?;
        // The client-visible mcp-session-id header carries the PROXY session
        // id; it must never leak upstream. Follow-ups present the translated
        // upstream session id instead.
        sanitized_headers.headers.remove("mcp-session-id");
        sanitized_headers
            .forwarded
            .retain(|name| name != "mcp-session-id");
        if let Some(upstream_session_id) = request.upstream_session_id.as_deref() {
            let value = HeaderValue::from_str(upstream_session_id)
                .map_err(|_| ProxyError::Other("invalid upstream mcp session id".to_string()))?;
            sanitized_headers
                .headers
                .insert(HeaderName::from_static("mcp-session-id"), value);
            sanitized_headers
                .forwarded
                .push("mcp-session-id".to_string());
        }
        // Pooled upstream key into the Bearer slot (ProviderSpec
        // upstream_key_injection; client Authorization was stripped at the
        // server boundary — M17 never relayed, never persisted).
        let bearer_value = HeaderValue::from_str(&format!("Bearer {}", lease.secret.as_str()))
            .map_err(|_| ProxyError::Other("invalid provider bearer value".to_string()))?;
        sanitized_headers
            .headers
            .insert(AUTHORIZATION, bearer_value);
        sanitized_headers
            .forwarded
            .push("authorization".to_string());

        let request_method = request.method.clone();
        let request_body = request.body.clone();
        let request_url = url.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, operation, |client| {
                let mut builder = client.request(request_method.clone(), request_url.clone());
                for (name, value) in sanitized_headers.headers.iter() {
                    if name == HOST || name == CONTENT_LENGTH {
                        continue;
                    }
                    builder = builder.header(name, value);
                }
                builder.body(request_body.clone())
            })
            .await;

        match response {
            Ok((response, _relay_lease)) => {
                let status = response.status();
                let headers = response.headers().clone();
                let body_bytes = response.bytes().await.map_err(ProxyError::Http)?;
                // Todo-3 classifier (M7): run-out 429 in BOTH the raw-HTTP and
                // SSE-envelope flavors exhausts; hourly-flavor 429 stays a
                // transient backoff and NEVER exhausts.
                let signal = classify_provider_failure(
                    request.provider,
                    &ProviderFailureEvidence {
                        status: status.as_u16(),
                        headers: Some(&headers),
                        body: &body_bytes,
                    },
                );
                let analysis =
                    provider_mcp_attempt_analysis(request.provider, status, &body_bytes, signal);
                tracing::debug!(
                    component = "provider_facade",
                    event = "provider_mcp_upstream_response",
                    provider = %request.provider,
                    operation = operation,
                    status = status.as_u16(),
                    signal = ?signal,
                    api_key_id = %lease.id,
                    "provider mcp upstream response classified"
                );

                let mut key_effect = self
                    .reconcile_key_health(
                        &lease,
                        request.path.as_str(),
                        &analysis,
                        request.auth_token_id.as_deref(),
                    )
                    .await?;
                if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
                    key_effect = self
                        .clear_transient_backoffs_after_success(
                            &lease.id,
                            request.path.as_str(),
                            request.auth_token_id.as_deref(),
                        )
                        .await?;
                }

                // Hourly-flavor 429: arm the shared HTTP backoff scope that
                // provider-scoped acquisition avoids (T12 precedent).
                let armed_backoff = match signal {
                    ProviderFailureSignal::RateLimited { .. } => {
                        let now = self.backend_time.now_ts();
                        match Self::effective_provider_rate_limited_backoff_secs(
                            request.provider,
                            signal,
                            now,
                        ) {
                            Some(retry_after_secs) => self
                                .key_store
                                .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                                    key_id: &lease.id,
                                    scope: HTTP_GLOBAL_BACKOFF_SCOPE,
                                    cooldown_until: now + retry_after_secs,
                                    retry_after_secs,
                                    reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
                                    source_request_log_id: None,
                                    now,
                                })
                                .await?
                                .is_some(),
                            // Unreachable for RateLimited; exhaustive match
                            // without an escape hatch.
                            None => false,
                        }
                    }
                    ProviderFailureSignal::Ok
                    | ProviderFailureSignal::Exhausted
                    | ProviderFailureSignal::AuthFailed
                    | ProviderFailureSignal::ClientError
                    | ProviderFailureSignal::ServerTransient => false,
                };
                if armed_backoff && key_effect.code == KEY_EFFECT_NONE {
                    key_effect = Self::transient_backoff_set_effect();
                }

                // Secrets redacted before persistence (M17): the pooled key
                // rides the Authorization header only (names persisted, never
                // values) and client slots were stripped at the boundary.
                log_success(
                    &lease.secret,
                    &request.method,
                    &request.path,
                    request.query.as_deref(),
                    status,
                );
                let redacted_request_body = redact_api_key_bytes(&request.body);
                let redacted_response_body = redact_api_key_bytes(&body_bytes);
                let request_log_id = self
                    .key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
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
                        gateway_mode: Some(MCP_GATEWAY_MODE_UPSTREAM),
                        experiment_variant: Some(MCP_EXPERIMENT_VARIANT_CONTROL),
                        provider: Some(request.provider.as_str()),
                        proxy_session_id: request.proxy_session_id.as_deref(),
                        routing_subject_hash: None,
                        upstream_operation: Some(operation),
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                        client_ip: request.client_ip.as_ref(),
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
                            HTTP_GLOBAL_BACKOFF_SCOPE,
                            request_log_id,
                            self.backend_time.now_ts(),
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
                let redacted_request_body = redact_api_key_bytes(&request.body);
                let redacted_empty: Vec<u8> = Vec::new();
                let error_message = err.to_string();
                self.key_store
                    .log_attempt(AttemptLog {
                        key_id: Some(&lease.id),
                        auth_token_id: request.auth_token_id.as_deref(),
                        method: &request.method,
                        path: request.path.as_str(),
                        query: request.query.as_deref(),
                        status: None,
                        tavily_status_code: None,
                        error: Some(&error_message),
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
                        gateway_mode: Some(MCP_GATEWAY_MODE_UPSTREAM),
                        experiment_variant: Some(MCP_EXPERIMENT_VARIANT_CONTROL),
                        provider: Some(request.provider.as_str()),
                        proxy_session_id: request.proxy_session_id.as_deref(),
                        routing_subject_hash: None,
                        upstream_operation: Some(operation),
                        fallback_reason: None,
                        forwarded_headers: &sanitized_headers.forwarded,
                        dropped_headers: &sanitized_headers.dropped,
                        client_ip: request.client_ip.as_ref(),
                    })
                    .await?;
                Err(err)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Provider-scoped mcp_sessions wrappers (plan todo 15; amendment M6).
    // Tavily rows keep their byte-stable NULL provider default: the store
    // predicates read NULL as `tavily`, so every pre-provider behavior is
    // preserved while serpapi sessions resolve ONLY on the serpapi surface.
    // -----------------------------------------------------------------------

    /// Active session resolution scoped to one provider surface (M6): a
    /// session created on another surface never resolves here.
    pub async fn get_active_mcp_session_for_provider(
        &self,
        proxy_session_id: &str,
        provider: providers::ProviderId,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        self.key_store
            .get_active_mcp_session_for_provider(
                proxy_session_id,
                provider.as_str(),
                self.backend_time.now_ts(),
            )
            .await
    }

    /// Raw provider tag of a session row (None tag = legacy/Tavily default);
    /// outer Option distinguishes "row exists" from "no row". Used by the
    /// Tavily `/mcp` pipeline to reject foreign-provider proxy session ids
    /// without disturbing its HA peer-lookup flow.
    pub async fn mcp_session_provider_tag(
        &self,
        proxy_session_id: &str,
    ) -> Result<Option<Option<String>>, ProxyError> {
        self.key_store
            .mcp_session_provider_tag(proxy_session_id)
            .await
    }

    /// Provider-scoped session-mode gating (M6): `rebalance_mcp_enabled`
    /// keeps its Tavily-only meaning, and foreign-provider sessions never
    /// suppress this provider's control-session behavior.
    pub async fn token_has_active_non_rebalance_mcp_session_for_provider(
        &self,
        token_id: &str,
        provider: providers::ProviderId,
    ) -> Result<bool, ProxyError> {
        self.key_store
            .has_active_non_rebalance_mcp_session_for_token_in_provider(
                token_id,
                provider.as_str(),
                self.backend_time.now_ts(),
            )
            .await
    }

    /// Provider-scoped stateless-rebalance lookup (M6): a serpapi passthrough
    /// session never shadows the token's Tavily rebalance session.
    pub async fn latest_active_mcp_session_for_token_in_provider(
        &self,
        token_id: &str,
        provider: providers::ProviderId,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        self.key_store
            .get_latest_active_mcp_session_for_token_in_provider(
                token_id,
                provider.as_str(),
                self.backend_time.now_ts(),
            )
            .await
    }

    /// Persist a provider-stamped passthrough session binding: the translated
    /// proxy session id, the pinned pooled key, and the upstream protocol
    /// session id, with `gateway_mode='upstream_mcp'` +
    /// `experiment_variant='control'` (M6 additive values).
    #[allow(clippy::too_many_arguments)]
    pub async fn create_provider_mcp_session_record(
        &self,
        proxy_session_id: &str,
        upstream_session_id: &str,
        upstream_key_id: &str,
        auth_token_id: Option<&str>,
        user_id: Option<&str>,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
        provider: providers::ProviderId,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        self.key_store
            .create_or_replace_mcp_session_with_provider(
                &McpSessionBinding {
                    proxy_session_id: proxy_session_id.to_string(),
                    upstream_session_id: Some(upstream_session_id.to_string()),
                    upstream_key_id: Some(upstream_key_id.to_string()),
                    auth_token_id: auth_token_id.map(str::to_string),
                    user_id: user_id.map(str::to_string),
                    protocol_version: protocol_version.map(str::to_string),
                    last_event_id: last_event_id.map(str::to_string),
                    gateway_mode: MCP_GATEWAY_MODE_UPSTREAM.to_string(),
                    experiment_variant: MCP_EXPERIMENT_VARIANT_CONTROL.to_string(),
                    ab_bucket: None,
                    routing_subject_hash: None,
                    fallback_reason: None,
                    rate_limited_until: None,
                    last_rate_limited_at: None,
                    last_rate_limit_reason: None,
                    created_at: now,
                    updated_at: now,
                    expires_at: now + MCP_SESSION_RETENTION_SECS,
                    revoked_at: None,
                    revoke_reason: None,
                },
                provider.as_str(),
            )
            .await
    }
}

#[cfg(test)]
mod serpapi_mcp_passthrough_tests {
    use super::*;

    #[test]
    fn hosted_mcp_operation_vocabulary_is_provider_scoped() {
        // Given/When/Then: only the hosted-passthrough provider carries an
        // additive operation label; Tavily keeps its legacy "mcp" default and
        // the local-facade providers have none until todos 16-17.
        assert_eq!(
            provider_mcp_upstream_operation(providers::ProviderId::Serpapi),
            Some("serpapi_mcp")
        );
        assert_eq!(
            provider_mcp_upstream_operation(providers::ProviderId::Tavily),
            None
        );
        assert_eq!(
            provider_mcp_upstream_operation(providers::ProviderId::Exa),
            None
        );
        assert_eq!(
            provider_mcp_upstream_operation(providers::ProviderId::Brave),
            None
        );
    }

    #[test]
    fn mcp_attempt_analysis_maps_dual_429_and_auth_fail_onto_health_actions() {
        // Given the committed todo-3 classifier signals; When projected onto
        // the AttemptAnalysis contract; Then the health actions match the T12
        // REST precedent (exhaust / quarantine / transient-none).
        let exhausted = provider_mcp_attempt_analysis(
            providers::ProviderId::Serpapi,
            StatusCode::TOO_MANY_REQUESTS,
            b"data: {\"error\":\"Your account has run out of searches.\"}\n\n",
            ProviderFailureSignal::Exhausted,
        );
        assert_eq!(exhausted.status, OUTCOME_QUOTA_EXHAUSTED);
        assert!(matches!(
            exhausted.key_health_action,
            KeyHealthAction::MarkExhausted
        ));
        assert_eq!(exhausted.tavily_status_code, Some(429));

        let hourly = provider_mcp_attempt_analysis(
            providers::ProviderId::Serpapi,
            StatusCode::TOO_MANY_REQUESTS,
            b"{}",
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None,
            },
        );
        assert_eq!(hourly.status, OUTCOME_ERROR);
        assert!(matches!(hourly.key_health_action, KeyHealthAction::None));
        assert_eq!(
            hourly.failure_kind.as_deref(),
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        );

        let auth = provider_mcp_attempt_analysis(
            providers::ProviderId::Serpapi,
            StatusCode::UNAUTHORIZED,
            b"{\"error\":\"Invalid API key.\"}",
            ProviderFailureSignal::AuthFailed,
        );
        let KeyHealthAction::Quarantine(decision) = auth.key_health_action else {
            panic!("auth-fail must quarantine via the existing machinery");
        };
        assert_eq!(decision.reason_code, BLOCKED_KEY_REASON_INVALID_API_KEY);
        assert!(decision.reason_summary.contains("HTTP 401"));
    }
}

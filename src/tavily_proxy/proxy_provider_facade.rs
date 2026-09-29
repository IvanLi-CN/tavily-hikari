// Generic provider facade execution seam (plan todo 11; amendments
// M1/M2/M10).
//
// This fragment carries the provider-aware pieces the route plumbing needs:
// the ingress/URL builders that replace the hardcoded Tavily `/mcp` literals,
// the provider-scoped REST passthrough executor, and the per-provider header
// allowlist / key-injection dispatch. Provider-specific execution lands on
// top of this seam in todos 12-17: billing reserve/settle and request_logs
// kind detail (12-14), hosted MCP passthrough sessions (15), local MCP
// facades (16-17), key-health reconcile wiring (18). Nothing here rewires
// the Tavily request path (byte-stability rule).

use reqwest::header::{AUTHORIZATION, HeaderName};

// SerpApi REST execution layer (plan todo 12): response classification onto
// the shared AttemptAnalysis contract, reconcile/backoff/request_logs
// bookkeeping, and the bin-side outcome projection. Declared here so the
// provider facade family keeps ONE lib seam; todos 13 (Exa) and 14 (Brave)
// extend the same family below.
mod proxy_provider_facade_serpapi;
pub use proxy_provider_facade_serpapi::{SerpapiFacadeOutcome, serpapi_facade_outcome};
pub(crate) use proxy_provider_facade_serpapi::{
    ProviderFacadeAttemptContext, ProviderFacadeAttemptRecord,
};

// Exa REST execution layer (plan todo 13): the same bookkeeping contract over
// the todo-3 exa classifier — 402 budget tags exhaust (SuccessOnly recovery),
// 429 Retry-After arms the shared HTTP backoff scope, 401 quarantines — plus
// the bin-side outcome projection driving the search=1/contents=1/answer=2
// credit tiers. The SerpApi record path stays byte-identical behind its own
// gate; the executor below dispatches per provider.
mod proxy_provider_facade_exa;
pub use proxy_provider_facade_exa::{ExaFacadeOutcome, exa_facade_outcome};

// Brave REST execution layer (plan todo 14; amendment M11): the same
// bookkeeping contract over the todo-3 brave classifier — 422
// SUBSCRIPTION_TOKEN_INVALID (and VALIDATION whose loc is the token header)
// auth-fails into quarantine, 402 QUOTA_LIMITED exhausts (RateLimitResetWindow
// recovery owned by T9's reset_monthly), 429 RATE_LIMITED arms the SECOND-scale
// Brave backoff — PLUS the M11 passive quota harvest that parses the response's
// X-RateLimit-* month window (positional pairing) and persists it on material
// change via record_quota_sync_sample with source `brave_passive` (zero extra
// requests, best-effort, never fails the request). The bin-side outcome
// projection drives the web=1/news=1/llm_context=2 credit tiers. SerpApi/Exa
// record paths stay byte-identical behind their own gates.
mod proxy_provider_facade_brave;
pub use proxy_provider_facade_brave::{BraveFacadeOutcome, brave_facade_outcome};

/// Client-facing description of one provider REST facade call (the M1
/// "ProviderId + resolved upstream base on the request path" seam: the
/// provider selects its parsed base inside the executor, so no state field
/// and no `ProxyRequest` change is needed).
///
/// The server layer owns auth extraction, client-slot stripping, and quota
/// gates BEFORE building this; the executor owns upstream URL, key placement,
/// and header policy.
#[derive(Debug)]
pub struct ProviderRestFacadeRequest {
    pub provider: providers::ProviderId,
    pub method: Method,
    /// Client-facing facade path (audit identity; request_logs wiring is
    /// todos 12-14).
    pub path: String,
    /// Canonical request-kind path (the exact `classify_token_request_kind`
    /// input committed in the todo-3 static table). Identical to `path` for
    /// SerpApi/Exa; for Brave it is the canonical short form of the native
    /// `/res/v1` mirroring (`/api/brave/web/search`). Provider record paths
    /// whose store-derived request_kind must match the bin-boundary kind use
    /// THIS value (todo 14): the store classifies request_logs rows from the
    /// path column, and the value-bucket/retention bucketing keys off it.
    pub kind_path: &'static str,
    /// Native upstream path under the provider REST base (SerpApi `/search`,
    /// Brave `/res/v1/web/search`).
    pub upstream_path: &'static str,
    /// Client query with every provider client-auth slot already stripped.
    pub query: Option<String>,
    /// Client headers with Authorization, native key slots, and
    /// X-Hikari-Routing-Key already removed; the provider allowlist is
    /// applied inside the executor.
    pub headers: HeaderMap,
    pub body: Vec<u8>,
    pub auth_token_id: Option<String>,
    /// Additive provider-scoped upstream_operation label
    /// (`provider_rest_upstream_operation`; todo 27 ratifies the vocabulary
    /// into the closed-enum SPEC).
    pub upstream_operation: &'static str,
    pub client_ip: Option<ClientIpInfo>,
}

impl TavilyProxy {
    /// Generic provider REST facade passthrough (plan todo 11).
    ///
    /// Order of operations mirrors the Tavily executor skeleton
    /// (`proxy_request` / `proxy_http_json_endpoint`) minus the Tavily-only
    /// identity policy: provider-scoped acquire (M10, legacy token/user
    /// affinity v1 — the api-rebalance switch never gates this path),
    /// path-prefixed URL building against the provider's resolved base (M1),
    /// T4 per-provider header allowlists with the provider-gated
    /// x-project-id strip (M2), native key-slot injection, verbatim upstream
    /// response passthrough in every status.
    ///
    /// Documented boundary: SerpApi (todo 12) records request_logs, applies
    /// key-health reconcile, and exposes its outcome for bin-side billing
    /// settle; Exa (todo 13) books the same set over its own record path
    /// (`record_exa_rest_facade_attempt`); Brave (todo 14) books the same set
    /// over `record_brave_rest_facade_attempt` AND passively harvests its
    /// month-window quota from the already-buffered response headers (M11 —
    /// zero extra requests). Tavily REST never reaches this seam.
    pub async fn proxy_provider_rest_facade(
        &self,
        request: ProviderRestFacadeRequest,
    ) -> Result<ProxyResponse, ProxyError> {
        let base = self.provider_rest_base(request.provider)?;
        let Some(sanitize) = provider_rest_header_sanitizer(request.provider) else {
            return Err(ProxyError::Other(
                "tavily REST keeps its legacy executors".to_string(),
            ));
        };
        let spec = providers::spec_for(request.provider);

        // M10: provider-scoped acquire through the legacy token/user affinity
        // strategy; every tier filters WHERE provider = the requested one.
        let lease = self
            .acquire_key_for_provider(request.provider, request.auth_token_id.as_deref())
            .await?;

        // M1: path-prefix-compatible URL building against the base parsed
        // once at construction (todo 7 ProviderUpstreams).
        let mut url = build_path_prefixed_url(&base, request.upstream_path);
        if let Some(query) = request.query.as_deref() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in form_urlencoded::parse(query.as_bytes()) {
                pairs.append_pair(&key, &value);
            }
        }
        drop(url.query_pairs_mut());

        let mut sanitized_headers = sanitize(&request.headers);
        // M2: x-project-id is a Tavily-only upstream identity policy; the
        // provider gate strips forged client values and never even reads the
        // system-settings policy for third-party providers.
        self.apply_upstream_project_id_header_for_provider(
            request.provider,
            &mut sanitized_headers,
            &request.headers,
            request.auth_token_id.as_deref(),
        )
        .await?;

        // Pooled upstream key into the provider's native REST slot. Client
        // slots were stripped at the server boundary before reaching here
        // (M17), so no client-supplied provider auth value is ever relayed.
        let upstream_secret = lease.secret.clone();
        match provider_rest_key_injection(request.provider, spec) {
            providers::KeyAuthInjection::QueryParam { name } => {
                {
                    let mut pairs = url.query_pairs_mut();
                    pairs.append_pair(name, upstream_secret.as_str());
                }
                drop(url.query_pairs_mut());
            }
            providers::KeyAuthInjection::Header { name } => {
                let header_name = HeaderName::try_from(name).map_err(|_| {
                    ProxyError::Other(format!("invalid provider header name: {name}"))
                })?;
                let header_value = HeaderValue::from_str(upstream_secret.as_str())
                    .map_err(|_| {
                        ProxyError::Other("invalid provider key header value".to_string())
                    })?;
                sanitized_headers.headers.insert(header_name, header_value);
                sanitized_headers.forwarded.push(name.to_ascii_lowercase());
            }
            providers::KeyAuthInjection::Bearer => {
                let header_value =
                    HeaderValue::from_str(&format!("Bearer {}", upstream_secret.as_str()))
                        .map_err(|_| {
                            ProxyError::Other("invalid provider bearer value".to_string())
                        })?;
                sanitized_headers.headers.insert(AUTHORIZATION, header_value);
                sanitized_headers.forwarded.push("authorization".to_string());
            }
        }

        // SerpApi upstream is GET-only (plan todo 12): the convenience POST's
        // JSON body arrives pre-mapped into `request.query` by the bin
        // boundary, so the upstream call is always GET with an empty body
        // while `request.method`/`request.body` stay the audit identity.
        // Exa/Brave keep their T11 verbatim method+body forwarding.
        let (request_method, request_body) = if request.provider == providers::ProviderId::Serpapi {
            (Method::GET, Vec::new())
        } else {
            (request.method.clone(), request.body.clone())
        };
        let request_url = url.clone();
        let response = self
            .send_with_forward_proxy(&lease.id, request.upstream_operation, |client| {
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
                // Failure-signal classification (todo 3). SerpApi (todo 12),
                // Exa (todo 13), and Brave (todo 14) book the full attempt
                // (reconcile + request_logs + backoff) through their
                // per-provider record paths; Brave's additionally harvests the
                // month window from the live response headers (M11).
                let signal = classify_provider_failure(
                    request.provider,
                    &ProviderFailureEvidence {
                        status: status.as_u16(),
                        headers: Some(&headers),
                        body: &body_bytes,
                    },
                );
                tracing::debug!(
                    component = "provider_facade",
                    event = "provider_upstream_response",
                    provider = %request.provider,
                    operation = request.upstream_operation,
                    status = status.as_u16(),
                    signal = ?signal,
                    api_key_id = %lease.id,
                    "provider facade upstream response classified"
                );
                let attempt_ctx = ProviderFacadeAttemptContext {
                    request: &request,
                    lease: &lease,
                    sanitized_headers: &sanitized_headers,
                    status,
                    body_bytes: &body_bytes,
                    signal,
                };
                let recorded = match request.provider {
                    providers::ProviderId::Exa => {
                        self.record_exa_rest_facade_attempt(&attempt_ctx).await?
                    }
                    // Brave (todo 14): books the attempt AND harvests the
                    // month window from the already-buffered response headers
                    // (M11 — zero extra outbound requests).
                    providers::ProviderId::Brave => {
                        self.record_brave_rest_facade_attempt(&attempt_ctx, &headers)
                            .await?
                    }
                    providers::ProviderId::Tavily
                    | providers::ProviderId::Serpapi => {
                        self.record_provider_rest_facade_attempt(&attempt_ctx)
                            .await?
                    }
                };
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
                // SerpApi (todo 12), Exa (todo 13), and Brave (todo 14)
                // transport failures persist an audit row (Tavily executor
                // parity); Tavily keeps its legacy no-op gate here.
                match request.provider {
                    providers::ProviderId::Exa => {
                        self.log_exa_rest_facade_transport_error(
                            &request,
                            &lease,
                            &sanitized_headers,
                            &err,
                        )
                        .await?;
                    }
                    providers::ProviderId::Brave => {
                        self.log_brave_rest_facade_transport_error(
                            &request,
                            &lease,
                            &sanitized_headers,
                            &err,
                        )
                        .await?;
                    }
                    providers::ProviderId::Tavily
                    | providers::ProviderId::Serpapi => {
                        self.log_provider_rest_facade_transport_error(
                            &request,
                            &lease,
                            &sanitized_headers,
                            &err,
                        )
                        .await?;
                    }
                }
                Err(err)
            }
        }
    }

    /// Resolved provider REST base. Tavily REST keeps its legacy executors
    /// (`proxy_http_json_endpoint` family) and never reaches this seam; the
    /// typed error makes that boundary explicit instead of accidental.
    fn provider_rest_base(&self, provider: providers::ProviderId) -> Result<Url, ProxyError> {
        match provider {
            providers::ProviderId::Tavily => Err(ProxyError::Other(
                "tavily REST keeps its legacy executors".to_string(),
            )),
            providers::ProviderId::Serpapi => Ok(self.provider_upstreams.serpapi_rest.clone()),
            providers::ProviderId::Exa => Ok(self.provider_upstreams.exa_rest.clone()),
            providers::ProviderId::Brave => Ok(self.provider_upstreams.brave_rest.clone()),
        }
    }
}

/// Provider-aware ingress check (M1) replacing the hardcoded
/// `path.starts_with("/mcp")` tests. The Tavily branch yields exactly the
/// legacy result; provider branches match their own ingress prefixes.
pub fn is_mcp_ingress_path(provider: providers::ProviderId, path: &str) -> bool {
    path.starts_with(provider_mcp_ingress_path(provider))
}

/// Provider-aware MCP upstream URL (M1, acceptance C1): strips the provider's
/// own ingress prefix instead of the Tavily-literal `/mcp`, so a
/// path-prefixed upstream (`http://host/gw/mcp`) is the exact target.
/// Unit-tested here; consumed by the hosted passthrough executor (todo 15).
// Consumed by the SerpApi hosted passthrough as it lands (plan todo 15).
#[allow(dead_code)]
pub fn build_provider_mcp_upstream_url(
    provider: providers::ProviderId,
    base: &Url,
    request_path: &str,
) -> Url {
    build_mcp_upstream_url_with_ingress(base, request_path, provider_mcp_ingress_path(provider))
}

/// Provider-aware default for the recorded `gateway_mode` when a request
/// omits one (M1: the literal-`/mcp` default becomes provider-scoped; the
/// Tavily branch is byte-identical).
pub(crate) fn mcp_gateway_mode_default(
    provider: providers::ProviderId,
    path: &str,
) -> Option<&'static str> {
    is_mcp_ingress_path(provider, path).then_some(MCP_GATEWAY_MODE_UPSTREAM)
}

/// Provider-aware default for the recorded `upstream_operation` (M1; Tavily
/// branch byte-identical).
pub(crate) fn mcp_upstream_operation_default(
    provider: providers::ProviderId,
    path: &str,
) -> Option<&'static str> {
    is_mcp_ingress_path(provider, path).then_some("mcp")
}

/// Additive provider-scoped `upstream_operation` vocabulary for the REST
/// facades. The rebalance-mcp-gateway SPEC enum is closed; todo 27 records
/// these values as the additive amendment. Tavily REST keeps its existing
/// `http_upstream_operation` vocabulary and returns None here.
#[allow(dead_code)]
pub fn provider_rest_upstream_operation(
    provider: providers::ProviderId,
    upstream_path: &str,
) -> Option<&'static str> {
    match provider {
        providers::ProviderId::Serpapi => match upstream_path {
            "/search" => Some("serpapi_http_search"),
            _ => None,
        },
        providers::ProviderId::Exa => match upstream_path {
            "/search" => Some("exa_http_search"),
            "/contents" => Some("exa_http_contents"),
            "/answer" => Some("exa_http_answer"),
            _ => None,
        },
        providers::ProviderId::Brave => match upstream_path {
            "/res/v1/web/search" => Some("brave_http_web_search"),
            "/res/v1/news/search" => Some("brave_http_news_search"),
            "/res/v1/llm/context" => Some("brave_http_llm_context"),
            _ => None,
        },
        providers::ProviderId::Tavily => None,
    }
}

/// REST key-slot placement, deliberately distinct from
/// `ProviderSpec::upstream_key_injection`: the registry field records the
/// provider's HOSTED-MCP surface injection (SerpApi = Bearer, per the T2
/// registry decision), while the SerpApi REST contract carries the pooled
/// key in its native `api_key` query slot (pinned by the todo 12 facade
/// contract). Exa/Brave use their registry header slots on both surfaces.
fn provider_rest_key_injection(
    provider: providers::ProviderId,
    spec: &'static providers::ProviderSpec,
) -> providers::KeyAuthInjection {
    match provider {
        providers::ProviderId::Serpapi => providers::KeyAuthInjection::QueryParam { name: "api_key" },
        providers::ProviderId::Exa | providers::ProviderId::Brave => spec.upstream_key_injection,
        // Unreachable: provider_rest_base rejects Tavily before injection.
        providers::ProviderId::Tavily => spec.upstream_key_injection,
    }
}

/// T4 per-provider REST header allowlist dispatch (wired into live routing
/// here per its documented consumption plan). None = no provider allowlist
/// exists (Tavily keeps its legacy sanitizers).
fn provider_rest_header_sanitizer(
    provider: providers::ProviderId,
) -> Option<fn(&HeaderMap) -> SanitizedHeaders> {
    match provider {
        providers::ProviderId::Tavily => None,
        providers::ProviderId::Serpapi => Some(sanitize_serpapi_http_headers_inner),
        providers::ProviderId::Exa => Some(sanitize_exa_http_headers_inner),
        providers::ProviderId::Brave => Some(sanitize_brave_http_headers_inner),
    }
}

#[cfg(test)]
mod provider_facade_tests {
    use super::*;

    #[test]
    fn serpapi_mcp_upstream_with_path_prefix_targets_exact_configured_path() {
        // M1 acceptance vector: SERPAPI_MCP_UPSTREAM=http://127.0.0.1:P/gw/mcp
        let base = Url::parse("http://127.0.0.1:58091/gw/mcp").expect("base url");
        let url = build_provider_mcp_upstream_url(
            providers::ProviderId::Serpapi,
            &base,
            "/serpapi/mcp",
        );
        assert_eq!(url.as_str(), "http://127.0.0.1:58091/gw/mcp");
    }

    #[test]
    fn provider_mcp_upstream_url_keeps_relative_suffix_under_prefix() {
        let base = Url::parse("http://127.0.0.1:58091/gw/mcp").expect("base url");
        let url = build_provider_mcp_upstream_url(
            providers::ProviderId::Serpapi,
            &base,
            "/serpapi/mcp/messages",
        );
        assert_eq!(url.as_str(), "http://127.0.0.1:58091/gw/mcp/messages");
    }

    #[test]
    fn tavily_mcp_upstream_url_vectors_unchanged() {
        // Byte-stability anchors for the legacy Tavily builder (untouched fn;
        // these are the pre-existing pinned vectors).
        let prefixed = Url::parse("https://example.com/gw/mcp").expect("base url");
        assert_eq!(
            build_mcp_upstream_url(&prefixed, "/mcp").as_str(),
            "https://example.com/gw/mcp"
        );
        assert_eq!(
            build_mcp_upstream_url(&prefixed, "/mcp/messages").as_str(),
            "https://example.com/gw/mcp/messages"
        );
        let root = Url::parse("https://mcp.example.com").expect("base url");
        assert_eq!(
            build_mcp_upstream_url(&root, "/mcp").as_str(),
            "https://mcp.example.com/mcp"
        );
    }

    #[test]
    fn is_mcp_ingress_path_tavily_branch_is_byte_identical() {
        for path in ["/mcp", "/mcp/", "/mcp/messages", "/mcpx", "/api/tavily/search", "/", ""] {
            assert_eq!(
                is_mcp_ingress_path(providers::ProviderId::Tavily, path),
                path.starts_with("/mcp"),
                "{path}"
            );
        }
        assert!(is_mcp_ingress_path(providers::ProviderId::Serpapi, "/serpapi/mcp"));
        assert!(is_mcp_ingress_path(providers::ProviderId::Exa, "/exa/mcp/x"));
        assert!(is_mcp_ingress_path(providers::ProviderId::Brave, "/brave/mcp"));
        assert!(!is_mcp_ingress_path(providers::ProviderId::Serpapi, "/mcp"));
        assert!(!is_mcp_ingress_path(
            providers::ProviderId::Brave,
            "/api/brave/res/v1/web/search"
        ));
    }

    #[test]
    fn mcp_default_helpers_keep_tavily_gateway_semantics() {
        for path in ["/mcp", "/mcp/x", "/api/tavily/search"] {
            assert_eq!(
                mcp_gateway_mode_default(providers::ProviderId::Tavily, path),
                path.starts_with("/mcp").then_some(MCP_GATEWAY_MODE_UPSTREAM),
                "{path}"
            );
            assert_eq!(
                mcp_upstream_operation_default(providers::ProviderId::Tavily, path),
                path.starts_with("/mcp").then_some("mcp"),
                "{path}"
            );
        }
        assert_eq!(
            mcp_gateway_mode_default(providers::ProviderId::Serpapi, "/serpapi/mcp"),
            Some(MCP_GATEWAY_MODE_UPSTREAM)
        );
        assert_eq!(
            mcp_upstream_operation_default(providers::ProviderId::Exa, "/exa/mcp"),
            Some("mcp")
        );
        assert_eq!(
            mcp_upstream_operation_default(providers::ProviderId::Serpapi, "/api/serpapi/search"),
            None
        );
    }

    #[test]
    fn provider_rest_upstream_operation_labels_are_provider_scoped() {
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Serpapi, "/search"),
            Some("serpapi_http_search")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Exa, "/search"),
            Some("exa_http_search")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Exa, "/contents"),
            Some("exa_http_contents")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Exa, "/answer"),
            Some("exa_http_answer")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Brave, "/res/v1/web/search"),
            Some("brave_http_web_search")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Brave, "/res/v1/news/search"),
            Some("brave_http_news_search")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Brave, "/res/v1/llm/context"),
            Some("brave_http_llm_context")
        );
        assert_eq!(
            provider_rest_upstream_operation(providers::ProviderId::Tavily, "/search"),
            None
        );
    }

    #[test]
    fn sanitize_provider_rest_headers_dispatch_uses_provider_allowlists() {
        let mut headers = HeaderMap::new();
        headers.insert("x-project-id", HeaderValue::from_static("forged"));
        headers.insert("x-api-key", HeaderValue::from_static("client-slot"));
        headers.insert(
            "x-subscription-token",
            HeaderValue::from_static("client-slot"),
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("api-version", HeaderValue::from_static("2023-10-16"));
        headers.insert("user-agent", HeaderValue::from_static("client/1.0"));
        headers.insert("x-hikari-routing-key", HeaderValue::from_static("rk"));

        assert!(provider_rest_header_sanitizer(providers::ProviderId::Tavily).is_none());

        for provider in [providers::ProviderId::Serpapi, providers::ProviderId::Exa] {
            let sanitize = provider_rest_header_sanitizer(provider).expect("sanitizer");
            let sanitized = sanitize(&headers);
            assert!(!sanitized.headers.contains_key("x-project-id"));
            assert!(!sanitized.headers.contains_key("x-api-key"));
            assert!(!sanitized.headers.contains_key("x-subscription-token"));
            assert!(!sanitized.headers.contains_key("user-agent"));
            assert!(!sanitized.headers.contains_key("x-hikari-routing-key"));
            assert!(sanitized.headers.contains_key("content-type"));
            assert!(!sanitized.headers.contains_key("api-version"));
        }

        let sanitize = provider_rest_header_sanitizer(providers::ProviderId::Brave).expect("brave");
        let brave = sanitize(&headers);
        assert!(brave.headers.contains_key("api-version"));
        assert!(brave.headers.contains_key("content-type"));
        assert!(!brave.headers.contains_key("x-subscription-token"));
        assert!(!brave.headers.contains_key("x-project-id"));
        assert!(!brave.headers.contains_key("user-agent"));
    }
}

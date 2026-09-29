//! Provider facade routing, resolver, and client-auth extraction (plan
//! todo 11; amendments M1/M2/M10/M16/M17).
//!
//! Boundary contract (documented in the task-11 evidence): this module owns
//! route registration targets, the table-driven path→provider resolver, the
//! universal Bearer + provider-native-slot client auth with
//! strip-before-forward/strip-before-log semantics, and the uniform quota
//! gates (hourly limit, reserved-credit peek, billing lock, business-call
//! reservation) at the same seams as `/api/tavily`. The generic upstream
//! passthrough lives in `TavilyProxy::proxy_provider_rest_facade` (lib).
//! Provider-specific execution lands on top of these seams in todos 12-17:
//! billing reserve/settle and request_logs kind detail (12-14), hosted MCP
//! passthrough (15), local MCP facades (16-17). No temporary stubs: every
//! response here is the documented boundary behavior until its owner todo
//! replaces the call behind the same seam.

use super::*;
use tavily_hikari::{
    BraveFacadeOutcome, ExaFacadeOutcome, ProviderRestFacadeRequest, SerpapiFacadeOutcome,
    TokenBillingGuard, TokenRequestKind,
    providers::{self, ProviderId},
    brave_facade_outcome, exa_facade_outcome, serpapi_facade_outcome,
};

// SerpApi hosted MCP passthrough pipeline (plan todo 15): session-provider
// tagging + translation, tool billing matrix, retry-ladder follow-ups, and
// provider-scoped session gating (M6). Replaces the typed todo-11
// `provider_mcp_pending` boundary for the serpapi surface only; Exa/Brave
// MCP facades keep that boundary until todos 16-17.
#[path = "provider_mcp_serpapi.rs"]
mod provider_mcp_serpapi;

// Provider-parameterized LOCAL MCP facade framework (plan todo 16; M19) + the
// Brave facade built on it. The framework generalizes the rebalance local-facade
// plumbing (envelope/batch/SSE/dispatch/billing) into `LocalMcpFacade` without
// touching the Tavily golden path; `provider_mcp_brave` supplies the brave
// catalog DATA + tools/call→REST translation and replaces the typed
// `provider_mcp_pending` boundary for the brave surface only. Exa keeps that
// boundary until todo 17 (it will add an analogous module on this framework).
#[path = "local_mcp_facade.rs"]
mod local_mcp_facade;
#[path = "provider_mcp_brave.rs"]
mod provider_mcp_brave;

/// Client-facing surface deciding which native client key slots are accepted
/// (plan todo 11): the Exa MCP surface additionally honors the `exaApiKey`
/// query slot, mirroring Tavily's `tavilyApiKey` query parity on `/mcp`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderFacadeSurface {
    Rest,
    Mcp,
}

/// Exa hosted-style MCP client query slot (plan todo 11 auth matrix).
const EXA_MCP_CLIENT_QUERY_SLOT: &str = "exaApiKey";

/// One registered provider REST facade route (table-driven resolver entry).
pub(super) struct ProviderRestRoute {
    /// Registered router path (also the audit path in logs).
    pub(super) path: &'static str,
    /// Canonical path handed to `classify_token_request_kind`; matches the
    /// committed todo-3 static table (identical to `path` except for Brave's
    /// native `/res/v1` mirroring, whose kind keys predate the full-path
    /// registration).
    pub(super) kind_path: &'static str,
    pub(super) provider: ProviderId,
    // Method columns mirror the axum registration and are consumed by the
    // table-driven resolver contract tests (provider_facade_routing).
    #[allow(dead_code)]
    pub(super) allow_get: bool,
    #[allow(dead_code)]
    pub(super) allow_post: bool,
    /// Native upstream path under the provider REST base.
    pub(super) upstream_path: &'static str,
    /// Operation key for the ProviderSpec credits table (plan todo 2); the
    /// reserved-credit gate consumes it, the settle lands with todos 12-14.
    pub(super) credits_op: &'static str,
    /// Additive provider-scoped upstream_operation label; must equal
    /// `tavily_hikari::provider_rest_upstream_operation(provider, upstream_path)`
    /// (todo 27 ratifies the vocabulary into the closed-enum SPEC).
    pub(super) upstream_operation: &'static str,
}

/// Full native path mirroring under the provider prefix so base-URL-override
/// clients work unchanged (plan todo 11).
const PROVIDER_REST_ROUTES: &[ProviderRestRoute] = &[
    ProviderRestRoute {
        path: "/api/serpapi/search",
        kind_path: "/api/serpapi/search",
        provider: ProviderId::Serpapi,
        allow_get: true,
        allow_post: true,
        upstream_path: "/search",
        credits_op: "search",
        upstream_operation: "serpapi_http_search",
    },
    ProviderRestRoute {
        path: "/api/exa/search",
        kind_path: "/api/exa/search",
        provider: ProviderId::Exa,
        allow_get: false,
        allow_post: true,
        upstream_path: "/search",
        credits_op: "search",
        upstream_operation: "exa_http_search",
    },
    ProviderRestRoute {
        path: "/api/exa/contents",
        kind_path: "/api/exa/contents",
        provider: ProviderId::Exa,
        allow_get: false,
        allow_post: true,
        upstream_path: "/contents",
        credits_op: "contents",
        upstream_operation: "exa_http_contents",
    },
    ProviderRestRoute {
        path: "/api/exa/answer",
        kind_path: "/api/exa/answer",
        provider: ProviderId::Exa,
        allow_get: false,
        allow_post: true,
        upstream_path: "/answer",
        credits_op: "answer",
        upstream_operation: "exa_http_answer",
    },
    ProviderRestRoute {
        path: "/api/brave/res/v1/web/search",
        kind_path: "/api/brave/web/search",
        provider: ProviderId::Brave,
        allow_get: true,
        allow_post: false,
        upstream_path: "/res/v1/web/search",
        credits_op: "web",
        upstream_operation: "brave_http_web_search",
    },
    ProviderRestRoute {
        path: "/api/brave/res/v1/news/search",
        kind_path: "/api/brave/news/search",
        provider: ProviderId::Brave,
        allow_get: true,
        allow_post: false,
        upstream_path: "/res/v1/news/search",
        credits_op: "news",
        upstream_operation: "brave_http_news_search",
    },
    ProviderRestRoute {
        path: "/api/brave/res/v1/llm/context",
        kind_path: "/api/brave/llm/context",
        provider: ProviderId::Brave,
        allow_get: true,
        allow_post: false,
        upstream_path: "/res/v1/llm/context",
        credits_op: "llm_context",
        upstream_operation: "brave_http_llm_context",
    },
];

/// Provider MCP ingress registrations. Tavily's `/mcp` keeps its pinned
/// registration and never appears here.
const PROVIDER_MCP_INGRESS_ROUTES: &[(ProviderId, &str)] = &[
    (ProviderId::Serpapi, "/serpapi/mcp"),
    (ProviderId::Exa, "/exa/mcp"),
    (ProviderId::Brave, "/brave/mcp"),
];

pub(super) fn resolve_provider_rest_route(path: &str) -> Option<&'static ProviderRestRoute> {
    PROVIDER_REST_ROUTES.iter().find(|route| route.path == path)
}

pub(super) fn resolve_provider_mcp_route(path: &str) -> Option<ProviderId> {
    PROVIDER_MCP_INGRESS_ROUTES
        .iter()
        .find(|(_, ingress)| *ingress == path)
        .map(|(provider, _)| *provider)
}

/// Ingress prefix registered for a provider MCP surface. Tavily's literal
/// `/mcp` stays owned by its pinned registration.
// Consumed by the resolver contract tests; todo 15's hosted passthrough uses
// the same mapping to resolve provider MCP upstream bases.
#[allow(dead_code)]
pub(super) fn resolve_provider_mcp_ingress(provider: ProviderId) -> &'static str {
    match PROVIDER_MCP_INGRESS_ROUTES
        .iter()
        .find(|(candidate, _)| *candidate == provider)
    {
        Some((_, ingress)) => ingress,
        None => "/mcp",
    }
}

/// Table-driven path→ProviderId resolver used by the handlers below. Covers
/// ONLY the new provider surfaces (REST routes, MCP ingresses, and MCP
/// subpaths); legacy Tavily paths resolve to None and keep their pinned
/// registrations (byte-stability rule).
pub(super) fn resolve_provider_for_path(path: &str) -> Option<ProviderId> {
    if let Some(route) = resolve_provider_rest_route(path) {
        return Some(route.provider);
    }
    if let Some(provider) = resolve_provider_mcp_route(path) {
        return Some(provider);
    }
    PROVIDER_MCP_INGRESS_ROUTES
        .iter()
        .find(|(_, ingress)| {
            path.strip_prefix(*ingress)
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .map(|(provider, _)| *provider)
}

/// Production registration merged into the serve.rs router. Keeping the
/// whole table here means the bin tests exercise the exact same route set
/// the server registers (no test-only duplication).
pub(super) fn provider_facade_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/serpapi/search",
            get(provider_rest_facade_entry).post(provider_rest_facade_entry),
        )
        .route("/api/exa/search", post(provider_rest_facade_entry))
        .route("/api/exa/contents", post(provider_rest_facade_entry))
        .route("/api/exa/answer", post(provider_rest_facade_entry))
        .route(
            "/api/brave/res/v1/web/search",
            get(provider_rest_facade_entry),
        )
        .route(
            "/api/brave/res/v1/news/search",
            get(provider_rest_facade_entry),
        )
        .route(
            "/api/brave/res/v1/llm/context",
            get(provider_rest_facade_entry),
        )
        .route("/serpapi/mcp", any(provider_mcp_facade_entry))
        .route("/serpapi/mcp/*path", any(provider_mcp_subpath_reject_entry))
        .route("/exa/mcp", any(provider_mcp_facade_entry))
        .route("/exa/mcp/*path", any(provider_mcp_subpath_reject_entry))
        .route("/brave/mcp", any(provider_mcp_facade_entry))
        .route("/brave/mcp/*path", any(provider_mcp_subpath_reject_entry))
}

async fn provider_rest_facade_entry(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let Some(route) = resolve_provider_rest_route(req.uri().path()) else {
        return Err(StatusCode::NOT_FOUND);
    };
    provider_rest_facade(state, req, route).await
}

async fn provider_mcp_facade_entry(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let Some(provider) = resolve_provider_mcp_route(req.uri().path()) else {
        return Err(StatusCode::NOT_FOUND);
    };
    provider_mcp_facade(state, req, provider).await
}

/// Authenticated state after universal + native client-slot extraction. Both
/// the query and the header set are already STRIPPED of every client auth
/// slot and of X-Hikari-Routing-Key (M17 strip-before-forward/strip-before-log;
/// M10 never-stored/never-forwarded).
struct ProviderFacadeAuth {
    token_id: Option<String>,
    using_dev_open_admin_fallback: bool,
    query: Option<String>,
    headers: HeaderMap,
}

/// Generalized client auth extraction (plan todo 11): universal
/// `Authorization: Bearer th-...` PLUS the provider-native client slots
/// carrying the Hikari token — SerpApi query `api_key` (facade + MCP), Exa
/// header `x-api-key` + MCP query `exaApiKey`, Brave header
/// `X-Subscription-Token`. Every slot validates through the existing
/// validate_access_token and yields the SAME typed Hikari 401 shapes as
/// `/mcp` and `/api/tavily` — never a provider-shaped error.
async fn authenticate_provider_facade_request(
    state: &Arc<AppState>,
    provider: ProviderId,
    surface: ProviderFacadeSurface,
    headers: &HeaderMap,
    raw_query: Option<&str>,
) -> Result<ProviderFacadeAuth, Response<Body>> {
    let spec = providers::spec_for(provider);

    let bearer_token = extract_bearer_token(headers);
    let native_header_token = spec.client_auth_header_slot.and_then(|slot| {
        headers
            .get(slot)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    });
    // The generalized slot strip returns the cleaned query (first non-empty
    // occurrence wins — Tavily semantics preserved by todo 4).
    let (query, native_query_token) = match spec.client_auth_query_slot {
        Some(slot) => extract_token_from_query_slot(raw_query, slot),
        None => (raw_query.map(str::to_owned), None),
    };
    let (query, exa_mcp_query_token) =
        if surface == ProviderFacadeSurface::Mcp && provider == ProviderId::Exa {
            extract_token_from_query_slot(query.as_deref(), EXA_MCP_CLIENT_QUERY_SLOT)
        } else {
            (query, None)
        };

    let Some(token_resolution) = resolve_request_token(
        state.dev_open_admin,
        vec![
            bearer_token,
            native_header_token,
            native_query_token,
            exa_mcp_query_token,
        ],
    ) else {
        return Err(missing_token_response().unwrap_or_else(|status| {
            Response::builder()
                .status(status)
                .body(Body::empty())
                .unwrap()
        }));
    };

    let valid = if token_resolution.using_dev_open_admin_fallback {
        true
    } else {
        state
            .proxy
            .validate_access_token(&token_resolution.token)
            .await
            .map_err(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap()
            })?
    };
    if !valid {
        return Err(invalid_token_response().unwrap_or_else(|status| {
            Response::builder()
                .status(status)
                .body(Body::empty())
                .unwrap()
        }));
    }

    // M10: X-Hikari-Routing-Key is accepted at parity with /api/tavily
    // (handlers/tavily.rs), but the facades use the legacy v1 token/user
    // affinity — the key is inert here, never stored and never forwarded
    // (upstream-agnostic-api-rebalance SPEC: routing keys strip before
    // upstream and never persist raw).
    let _routing_key_accepted_at_parity = extract_hikari_routing_key(headers);

    let mut client_headers = clone_headers(headers);
    client_headers.remove(axum::http::header::AUTHORIZATION);
    if let Some(slot) = spec.client_auth_header_slot {
        client_headers.remove(slot);
    }
    client_headers.remove(HIKARI_ROUTING_KEY_HEADER);

    Ok(ProviderFacadeAuth {
        token_id: token_resolution.auth_token_id,
        using_dev_open_admin_fallback: token_resolution.using_dev_open_admin_fallback,
        query,
        headers: client_headers,
    })
}

/// Uniform gate stack for the provider REST facades, mirroring the
/// `/api/tavily` seams (handlers/tavily.rs:839-1057): HA business fence,
/// hourly any-request limit, reserved-credit quota peek, billing lock, and
/// the business-call 1h reservation. Credit settle and request_logs rows are
/// todos 12-14 (documented boundary).
async fn provider_rest_facade(
    state: Arc<AppState>,
    req: Request<Body>,
    route: &'static ProviderRestRoute,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    if let Err(response) = ensure_ha_allows_basic_business(&state, &path).await {
        return Ok(response);
    }
    let remote_addr = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0);

    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let request_kind = classify_token_request_kind(route.kind_path, Some(body_bytes.as_ref()));

    let auth = match authenticate_provider_facade_request(
        &state,
        route.provider,
        ProviderFacadeSurface::Rest,
        &parts.headers,
        parts.uri.query(),
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return Ok(response),
    };
    let token_id = auth.token_id;
    let using_dev_open_admin_fallback = auth.using_dev_open_admin_fallback;
    // The stripped query is the ONLY query text forwarded, logged, or recorded.
    let stripped_query = auth.query.clone();

    // SerpApi convenience POST (plan todo 12): map the JSON body fields onto
    // upstream query params BEFORE the gates so a malformed body is a cheap
    // typed 400 with zero upstream cost. The lib executor completes the
    // translation (SerpApi upstream is GET-only with an empty body); the
    // merged query is what gets forwarded AND persisted.
    let forward_query = if route.provider == ProviderId::Serpapi && method == Method::POST {
        match merge_serpapi_post_query(auth.query.as_deref(), &body_bytes) {
            Ok(merged) => merged,
            Err(message) => return invalid_tavily_http_request_response(message),
        }
    } else {
        auth.query.clone()
    };

    // Exa POST passthrough prep (plan todo 13): a truthy `stream` value is a
    // typed 400 Hikari rejection BEFORE the gates and before any upstream
    // call (zero upstream cost, mirroring the /api/tavily free-account
    // streaming boundary handlers/tavily.rs:794-798), and client api_key-like
    // body fields are stripped so no client-supplied provider auth is ever
    // relayed or persisted. Every other body byte forwards verbatim — Exa
    // owns request validation (M22) and answers native 4xx passthrough.
    let forward_body = if route.provider == ProviderId::Exa {
        match prepare_exa_post_body(&body_bytes) {
            Ok(body) => body,
            Err(message) => return invalid_tavily_http_request_response(message),
        }
    } else {
        body_bytes.to_vec()
    };

    let system_settings = state.proxy.get_system_settings().await.map_err(|err| {
        eprintln!("get system settings failed for {path}: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let client_ip = resolve_client_ip_info(
        remote_addr,
        &parts.headers,
        &TrustedClientIpSettings {
            trusted_proxy_cidrs: system_settings.trusted_proxy_cidrs.clone(),
            trusted_client_ip_headers: system_settings.trusted_client_ip_headers.clone(),
        },
    );

    // Reserved credits from the committed todo-2 ProviderSpec table.
    let spec = providers::spec_for(route.provider);
    let reserved_credits = i64::from((spec.credits)(route.credits_op));

    let token_billing_guard = if !using_dev_open_admin_fallback {
        if let Some(tid) = token_id.as_deref() {
            Some(state.proxy.lock_token_billing(tid).await.map_err(|err| {
                eprintln!("token billing lock failed for {path}: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?)
        } else {
            None
        }
    } else {
        None
    };
    let billing_subject = token_billing_guard
        .as_ref()
        .map(|guard| guard.billing_subject().to_string());
    if let Some(guard) = token_billing_guard.as_ref() {
        guard.ensure_live().map_err(|err| {
            eprintln!("token billing lock lost before precheck for {path}: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(tid) = token_id.as_deref()
        && !using_dev_open_admin_fallback
    {
        match state.proxy.check_token_hourly_requests(tid).await {
            Ok(verdict) => {
                if !verdict.allowed {
                    let message = build_request_limit_error_message(&verdict);
                    let _ = state
                        .proxy
                        .record_token_attempt_with_kind(
                            tid,
                            &method,
                            &path,
                            stripped_query.as_deref(),
                            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                            None,
                            false,
                            "quota_exhausted",
                            Some(&message),
                            &request_kind,
                        )
                        .await;
                    let response = request_limit_exceeded_response(&verdict)?;
                    return Ok(response);
                }
            }
            Err(err) => {
                eprintln!("hourly request limit check failed for {path}: {err}");
                let msg = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        &method,
                        &path,
                        stripped_query.as_deref(),
                        Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64),
                        None,
                        false,
                        "error",
                        Some(msg.as_str()),
                        &request_kind,
                    )
                    .await;
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    if let Some(tid) = token_id.as_deref() {
        match if let Some(subject) = billing_subject.as_deref() {
            state.proxy.peek_token_quota_for_subject(subject).await
        } else {
            state.proxy.peek_token_quota(tid).await
        } {
            Ok(verdict) => {
                if !using_dev_open_admin_fallback && quota_would_exceed(&verdict, reserved_credits)
                {
                    let message = "credits quota exhausted for this token";
                    let _ = state
                        .proxy
                        .record_token_attempt_with_kind(
                            tid,
                            &method,
                            &path,
                            stripped_query.as_deref(),
                            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                            None,
                            true,
                            "quota_exhausted",
                            Some(message),
                            &request_kind,
                        )
                        .await;
                    let payload = json!({
                        "error": "quota_exhausted",
                        "message": message,
                    });
                    let resp = Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .header(CONTENT_TYPE, "application/json; charset=utf-8")
                        .body(Body::from(payload.to_string()))
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    return Ok(resp);
                }
            }
            Err(err) => {
                eprintln!("quota peek failed for {path}: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    let mut business_calls_reservation = None;
    if let Some(tid) = token_id.as_deref()
        && !using_dev_open_admin_fallback
    {
        let reservation = if let Some(subject) = billing_subject.as_deref() {
            state
                .proxy
                .reserve_token_business_calls_1h_limit_for_subject(subject)
                .await
        } else {
            state.proxy.reserve_token_business_calls_1h_limit(tid).await
        };
        match reservation {
            Ok(tavily_hikari::BusinessCalls1hReservationOutcome::Reserved(reservation)) => {
                business_calls_reservation = Some(reservation);
            }
            Ok(tavily_hikari::BusinessCalls1hReservationOutcome::Denied(verdict)) => {
                let message = build_business_calls_1h_limit_error_message(&verdict);
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        &method,
                        &path,
                        stripped_query.as_deref(),
                        Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                        None,
                        false,
                        "quota_exhausted",
                        Some(&message),
                        &request_kind,
                    )
                    .await;
                let response = business_calls_1h_limit_exceeded_response(&verdict)?;
                return Ok(response);
            }
            Ok(tavily_hikari::BusinessCalls1hReservationOutcome::NotApplicable) => {}
            Err(err) => {
                eprintln!("business calls 1h reserve failed for {path}: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    if let Some(guard) = token_billing_guard.as_ref()
        && let Err(err) = guard.ensure_live()
    {
        state
            .proxy
            .release_business_calls_1h_reservation(business_calls_reservation.take())
            .await;
        eprintln!("token billing lock lost before provider facade send for {path}: {err}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Generic execution seam (lib): provider-scoped acquire, path-prefix URL
    // building, native key-slot injection, provider allowlists, verbatim
    // upstream passthrough. Todo 12 layers the SerpApi bookkeeping (attempt
    // rows, reconcile, backoff) inside this seam; billing settle for SerpApi
    // lands below. Todos 13/14 extend the same seam for Exa/Brave.
    let method_for_logs = method.clone();
    let path_for_logs = path.clone();
    let token_id_for_logs = token_id.clone();
    let result = state
        .proxy
        .proxy_provider_rest_facade(ProviderRestFacadeRequest {
            provider: route.provider,
            method,
            path,
            kind_path: route.kind_path,
            upstream_path: route.upstream_path,
            query: forward_query,
            headers: auth.headers,
            body: forward_body,
            auth_token_id: token_id,
            upstream_operation: route.upstream_operation,
            client_ip: Some(client_ip),
        })
        .await;

    match result {
        Ok(response) => {
            // Todos 12-14 (SerpApi, Exa, Brave): the deterministic lib-side
            // outcome projections drive token-log/billing rows and the
            // business-call finalization. Brave (todo 14) also carries the M11
            // passive harvest lib-side; only Tavily keeps the untouched
            // success/error fallback (it never reaches this seam).
            let facade_outcome = match route.provider {
                ProviderId::Serpapi => Some(ProviderFacadeOutcomeView::from(
                    serpapi_facade_outcome(response.status, &response.body),
                )),
                ProviderId::Exa => Some(ProviderFacadeOutcomeView::from(exa_facade_outcome(
                    response.status,
                    &response.body,
                ))),
                ProviderId::Brave => Some(ProviderFacadeOutcomeView::from(brave_facade_outcome(
                    response.status,
                    &response.body,
                ))),
                ProviderId::Tavily => None,
            };
            let outcome_status = match &facade_outcome {
                Some(outcome) => outcome.result_status,
                None if response.status.is_success() => "success",
                None => "error",
            };
            if let Some(outcome) = &facade_outcome {
                record_provider_facade_billing(ProviderFacadeBilling {
                    state: &state,
                    method: &method_for_logs,
                    path: &path_for_logs,
                    query: stripped_query.as_deref(),
                    token_id: token_id_for_logs.as_deref(),
                    using_dev_open_admin_fallback,
                    billing_subject: billing_subject.as_deref(),
                    billing_guard: token_billing_guard.as_ref(),
                    request_kind: &request_kind,
                    credits: reserved_credits,
                    outcome,
                    response: &response,
                })
                .await;
            }
            state
                .proxy
                .finalize_business_calls_1h_reservation_from_status(
                    business_calls_reservation.take(),
                    outcome_status,
                    response.request_log_id,
                )
                .await;
            Ok(build_response(response))
        }
        Err(err) => {
            state
                .proxy
                .release_business_calls_1h_reservation(business_calls_reservation.take())
                .await;
            eprintln!("provider facade {} proxy error: {err}", route.path);
            if let Some(tid) = token_id_for_logs.as_deref() {
                let msg = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        &method_for_logs,
                        &path_for_logs,
                        stripped_query.as_deref(),
                        None,
                        None,
                        false,
                        "error",
                        Some(msg.as_str()),
                        &request_kind,
                    )
                    .await;
            }
            let status = match err {
                ProxyError::Http(_)
                | ProxyError::NoAvailableKeys
                | ProxyError::PinnedMcpSessionUnavailable
                | ProxyError::QuotaDataMissing { .. }
                | ProxyError::UsageHttp { .. } => StatusCode::BAD_GATEWAY,
                ProxyError::Database(_)
                | ProxyError::InvalidEndpoint { .. }
                | ProxyError::LastAdminLoginMethod
                | ProxyError::StaleClaim { .. }
                | ProxyError::Deferred { .. }
                | ProxyError::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let payload = json!({
                "error": "proxy_error",
                "message": "upstream unavailable",
            });
            let resp = Response::builder()
                .status(status)
                .header(CONTENT_TYPE, "application/json; charset=utf-8")
                .body(Body::from(payload.to_string()))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(resp)
        }
    }
}

/// Provider MCP ingress handler (plan todo 11). Auth (universal Bearer +
/// native slots incl. the Exa `exaApiKey` MCP query slot) and the hourly
/// any-request gate are final here. SerpApi dispatches to the hosted
/// passthrough pipeline (todo 15); the execution bodies for the Exa/Brave
/// LOCAL MCP facades stay the documented boundary until todos 16-17 replace
/// them behind the same seam.
async fn provider_mcp_facade(
    state: Arc<AppState>,
    req: Request<Body>,
    provider: ProviderId,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    if let Err(response) = ensure_ha_allows_basic_business(&state, &path).await {
        return Ok(response);
    }
    let remote_addr = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0);
    let body_bytes = body::to_bytes(body, BODY_LIMIT)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let request_kind = classify_token_request_kind(&path, Some(body_bytes.as_ref()));

    let auth = match authenticate_provider_facade_request(
        &state,
        provider,
        ProviderFacadeSurface::Mcp,
        &parts.headers,
        parts.uri.query(),
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return Ok(response),
    };
    if auth.using_dev_open_admin_fallback {
        return mcp_session_response(
            StatusCode::UNAUTHORIZED,
            "explicit_token_required",
            "MCP requests must provide an explicit token when --dev-open-admin is enabled.",
        );
    }

    // Hourly any-request gate at parity with the /mcp handler.
    if let Some(tid) = auth.token_id.as_deref() {
        match state.proxy.check_token_hourly_requests(tid).await {
            Ok(verdict) => {
                if !verdict.allowed {
                    let message = build_request_limit_error_message(&verdict);
                    let _ = state
                        .proxy
                        .record_token_attempt_with_kind(
                            tid,
                            &method,
                            &path,
                            auth.query.as_deref(),
                            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                            None,
                            false,
                            "quota_exhausted",
                            Some(&message),
                            &request_kind,
                        )
                        .await;
                    return request_limit_exceeded_response(&verdict);
                }
            }
            Err(err) => {
                eprintln!("hourly request limit check failed for {path}: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    // SerpApi hosted passthrough (todo 15) and the Brave LOCAL facade (todo 16)
    // each replace the typed boundary behind the same seam; Exa keeps the todo-17
    // boundary below. Both build the SHARED facade context (the serpapi module
    // owns it as the first consumer of the seam).
    if provider == ProviderId::Serpapi {
        return provider_mcp_serpapi::provider_mcp_passthrough(
            state,
            provider_mcp_serpapi::ProviderMcpFacadeContext {
                provider,
                method: &method,
                path: &path,
                request_kind: &request_kind,
                body: &body_bytes,
                auth: &auth,
                original_headers: &parts.headers,
                remote_addr,
            },
        )
        .await;
    }
    if provider == ProviderId::Brave {
        return provider_mcp_brave::provider_mcp_brave_local_facade(
            state,
            provider_mcp_serpapi::ProviderMcpFacadeContext {
                provider,
                method: &method,
                path: &path,
                request_kind: &request_kind,
                body: &body_bytes,
                auth: &auth,
                original_headers: &parts.headers,
                remote_addr,
            },
        )
        .await;
    }

    provider_mcp_execution_pending_response(provider)
}

/// Typed Hikari boundary response for provider MCP surfaces whose execution
/// bodies are owned by todos 15-17. Never a provider-shaped error and never
/// an upstream call (acceptance C1 keeps every mock counter at zero until
/// the owner todo replaces this body behind the same seam).
fn provider_mcp_execution_pending_response(
    provider: ProviderId,
) -> Result<Response<Body>, StatusCode> {
    mcp_session_response(
        StatusCode::NOT_IMPLEMENTED,
        "provider_mcp_pending",
        &format!("{provider} MCP surface is not wired yet"),
    )
}

async fn provider_mcp_subpath_reject_entry(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response<Body>, StatusCode> {
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let path = parts.uri.path().to_owned();
    if let Err(response) = ensure_ha_allows_basic_business(&state, &path).await {
        return Ok(response);
    }
    let Some(provider) = resolve_provider_for_path(&path) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let auth = match authenticate_provider_facade_request(
        &state,
        provider,
        ProviderFacadeSurface::Mcp,
        &parts.headers,
        parts.uri.query(),
    )
    .await
    {
        Ok(auth) => auth,
        Err(response) => return Ok(response),
    };
    if auth.using_dev_open_admin_fallback {
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

    // Amendment M16: provider-tagged null-key mcp_path_404 raw logs; this
    // registration guarantees provider MCP subpaths NEVER reach the SPA HTML
    // 302 fallback (mirrors mcp_subpath_reject_handler).
    let request_log_id = match state
        .proxy
        .record_local_provider_request_log_without_key(
            provider,
            auth.token_id.as_deref(),
            &method,
            &path,
            auth.query.as_deref(),
            StatusCode::NOT_FOUND,
            Some(StatusCode::NOT_FOUND.as_u16() as i64),
            &body_bytes,
            response_body,
            "error",
            Some("mcp_path_404"),
            &empty_headers,
            &empty_headers,
            None,
        )
        .await
    {
        Ok(log_id) => Some(log_id),
        Err(err) => {
            eprintln!("provider MCP subpath reject request_log failed for {path}: {err}");
            None
        }
    };

    if let Some(token_id) = auth.token_id.as_deref() {
        let _ = state
            .proxy
            .record_token_attempt_with_kind_request_log_metadata(
                token_id,
                &method,
                &path,
                auth.query.as_deref(),
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

/// SerpApi convenience-POST translation (plan todo 12): every JSON body field
/// becomes an upstream query parameter. Documented policy: `api_key`-slotted
/// fields are DROPPED (the pooled key is injected by the lib executor and a
/// client slot is never relayed nor persisted raw); null fields are skipped;
/// scalars pass through verbatim; structured values (arrays/objects) are
/// JSON-encoded into the parameter value; explicit URL query parameters win
/// over same-name body fields. NO `q` re-validation — upstream answers a free
/// 400 that passes through natively (transformation ban, M22).
fn merge_serpapi_post_query(
    client_query: Option<&str>,
    body: &[u8],
) -> Result<Option<String>, &'static str> {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    let mut query_keys = HashSet::new();
    if let Some(query) = client_query {
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            serializer.append_pair(&key, &value);
            query_keys.insert(key.into_owned());
        }
    }
    if !body.is_empty() {
        let parsed: Value =
            serde_json::from_slice(body).map_err(|_| "request body must be valid JSON")?;
        let Value::Object(map) = parsed else {
            return Err("POST body must be a JSON object of SerpApi query parameters");
        };
        for (key, value) in &map {
            if key.eq_ignore_ascii_case("api_key") || query_keys.contains(key) {
                continue;
            }
            let rendered = match value {
                Value::Null => continue,
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            serializer.append_pair(key, &rendered);
        }
    }
    let merged = serializer.finish();
    Ok((!merged.is_empty()).then_some(merged))
}

/// Documented Exa streaming boundary message (plan todo 13): the typed 400
/// names the rule exactly like the /api/tavily free-account boundary helper
/// (handlers/tavily.rs:254-275,794-798).
const EXA_STREAM_REJECT_MESSAGE: &str = "streaming not supported; omit stream or set false";

/// Client api_key-like JSON body field names stripped from Exa POST bodies:
/// the pooled key rides the x-api-key header (ProviderSpec injection), so a
/// client-supplied provider auth field is never relayed nor persisted.
fn is_exa_client_auth_body_field(key: &str) -> bool {
    ["api_key", "apikey", "x-api-key", "x_api_key"]
        .iter()
        .any(|candidate| key.eq_ignore_ascii_case(candidate))
}

/// Truthy-`stream` detector. Exa's `stream` accepts booleans and streaming
/// mode strings ("auto"/"always"/"sources-first") — anything except an
/// explicit `false` (or null) would make the upstream answer an SSE stream,
/// which this facade never supports, so every other present value rejects
/// BEFORE any upstream call.
fn exa_stream_value_is_truthy(value: &Value) -> bool {
    !matches!(value, Value::Bool(false) | Value::Null)
}

/// Exa POST body preparation (plan todo 13). Policy:
/// * a truthy `stream` value → Err (typed 400 Hikari rejection naming the
///   documented streaming boundary) before any gate or upstream call;
/// * client api_key-like fields removed from JSON object bodies;
/// * every other byte — including non-JSON and non-object payloads — forwards
///   UNCHANGED: Exa owns request validation beyond the stream rejection and
///   answers native 4xx that pass through verbatim (transformation ban, M22).
fn prepare_exa_post_body(body: &[u8]) -> Result<Vec<u8>, &'static str> {
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let Ok(parsed) = serde_json::from_slice::<Value>(body) else {
        // Not JSON: verbatim passthrough for Exa's own free 400.
        return Ok(body.to_vec());
    };
    let Value::Object(map) = parsed else {
        // Arrays/scalars pass through untouched (Exa owns the schema).
        return Ok(body.to_vec());
    };
    if map.get("stream").is_some_and(exa_stream_value_is_truthy) {
        return Err(EXA_STREAM_REJECT_MESSAGE);
    }
    let original_len = map.len();
    let stripped: serde_json::Map<String, Value> = map
        .into_iter()
        .filter(|(key, _)| !is_exa_client_auth_body_field(key))
        .collect();
    if stripped.len() == original_len {
        // Nothing removed: the client bytes forward byte-identical.
        return Ok(body.to_vec());
    }
    serde_json::to_vec(&Value::Object(stripped))
        .map_err(|_| "request body must be JSON-serializable after key-slot removal")
}

/// Classifier-driven token-log outcome shared by the settling REST facades
/// (SerpApi todo 12, Exa todo 13); both lib-side projections fold onto this
/// view so the billing recorder stays single-sourced.
#[derive(Clone, Copy)]
struct ProviderFacadeOutcomeView {
    /// `success` / `quota_exhausted` / `error` (shared OUTCOME_* vocabulary).
    result_status: &'static str,
    failure_kind: Option<&'static str>,
}

impl From<SerpapiFacadeOutcome> for ProviderFacadeOutcomeView {
    fn from(outcome: SerpapiFacadeOutcome) -> Self {
        Self {
            result_status: outcome.result_status,
            failure_kind: outcome.failure_kind,
        }
    }
}

impl From<ExaFacadeOutcome> for ProviderFacadeOutcomeView {
    fn from(outcome: ExaFacadeOutcome) -> Self {
        Self {
            result_status: outcome.result_status,
            failure_kind: outcome.failure_kind,
        }
    }
}

impl From<BraveFacadeOutcome> for ProviderFacadeOutcomeView {
    fn from(outcome: BraveFacadeOutcome) -> Self {
        Self {
            result_status: outcome.result_status,
            failure_kind: outcome.failure_kind,
        }
    }
}

/// Everything the provider REST facade billing/attempt recording needs,
/// grouped so the facade flow stays readable (mirrors the handlers/tavily.rs
/// settle block).
struct ProviderFacadeBilling<'a> {
    state: &'a Arc<AppState>,
    method: &'a Method,
    path: &'a str,
    query: Option<&'a str>,
    token_id: Option<&'a str>,
    using_dev_open_admin_fallback: bool,
    billing_subject: Option<&'a str>,
    billing_guard: Option<&'a TokenBillingGuard>,
    request_kind: &'a TokenRequestKind,
    credits: i64,
    outcome: &'a ProviderFacadeOutcomeView,
    response: &'a ProxyResponse,
}

/// Reserve -> settle billing and token-log recording for the settling
/// provider REST facades (plan todo 12 SerpApi; todo 13 Exa extends the same
/// recorder with its credit tiers search=1/contents=1/answer=2; todo 14 Brave
/// adds web=1/news=1/llm_context=2 — all from the committed todo-2
/// ProviderSpec table, mirroring handlers/tavily.rs:1345-1524).
/// M22: provider responses are NEVER inspected for billing — business success
/// is 2xx and the credit amount is the registry table value. Local persistence
/// failures never replace the verbatim upstream response (duplicate-charge
/// protection, handlers/tavily.rs:1512-1514 rule).
async fn record_provider_facade_billing(ctx: ProviderFacadeBilling<'_>) {
    if ctx.using_dev_open_admin_fallback {
        return;
    }
    let Some(token_id) = ctx.token_id else {
        return;
    };
    let http_code = i64::from(ctx.response.status.as_u16());

    if ctx.response.status.is_success() {
        if ctx.credits <= 0 {
            return;
        }
        let recorded = match ctx.billing_subject {
            Some(subject) => {
                ctx.state
                    .proxy
                    .record_pending_billing_attempt_for_subject_with_kind_request_log(
                        token_id,
                        ctx.method,
                        ctx.path,
                        ctx.query,
                        Some(http_code),
                        Some(http_code),
                        true,
                        ctx.outcome.result_status,
                        None,
                        ctx.credits,
                        subject,
                        ctx.request_kind,
                        ctx.response.api_key_id.as_deref(),
                        ctx.outcome.failure_kind,
                        Some(ctx.response.key_effect_code.as_str()),
                        ctx.response.key_effect_summary.as_deref(),
                        Some(ctx.response.binding_effect_code.as_str()),
                        ctx.response.binding_effect_summary.as_deref(),
                        Some(ctx.response.selection_effect_code.as_str()),
                        ctx.response.selection_effect_summary.as_deref(),
                        ctx.response.request_log_id,
                    )
                    .await
            }
            None => {
                ctx.state
                    .proxy
                    .record_pending_billing_attempt_with_kind_request_log_metadata(
                        token_id,
                        ctx.method,
                        ctx.path,
                        ctx.query,
                        Some(http_code),
                        Some(http_code),
                        true,
                        ctx.outcome.result_status,
                        None,
                        ctx.credits,
                        ctx.request_kind,
                        ctx.response.api_key_id.as_deref(),
                        ctx.outcome.failure_kind,
                        Some(ctx.response.key_effect_code.as_str()),
                        ctx.response.key_effect_summary.as_deref(),
                        Some(ctx.response.binding_effect_code.as_str()),
                        ctx.response.binding_effect_summary.as_deref(),
                        Some(ctx.response.selection_effect_code.as_str()),
                        ctx.response.selection_effect_summary.as_deref(),
                        ctx.response.request_log_id,
                    )
                    .await
            }
        };
        let log_id = match recorded {
            Ok(log_id) => log_id,
            Err(err) => {
                eprintln!(
                    "record_pending_billing_attempt failed for {}: {err}",
                    ctx.path
                );
                return;
            }
        };
        let lock_lost_msg = ctx
            .billing_guard
            .and_then(|guard| guard.ensure_live().err())
            .map(|err| {
                format!(
                    "charge_token_quota deferred for {}: {err}; pending billing will retry",
                    ctx.path
                )
            });
        if let Some(msg) = lock_lost_msg {
            eprintln!("{msg}");
            let _ = ctx
                .state
                .proxy
                .annotate_pending_billing_attempt(log_id, &msg)
                .await;
            return;
        }
        match ctx.state.proxy.settle_pending_billing_attempt(log_id).await {
            Ok(PendingBillingSettleOutcome::Charged)
            | Ok(PendingBillingSettleOutcome::AlreadySettled) => {}
            Ok(PendingBillingSettleOutcome::RetryLater) => {
                let msg = format!(
                    "charge_token_quota delayed for {}: pending billing claim miss; will retry",
                    ctx.path
                );
                eprintln!("{msg}");
                let _ = ctx
                    .state
                    .proxy
                    .annotate_pending_billing_attempt(log_id, &msg)
                    .await;
            }
            Err(err) => {
                let msg = format!("charge_token_quota failed for {}: {err}", ctx.path);
                eprintln!("{msg}");
                let _ = ctx
                    .state
                    .proxy
                    .annotate_pending_billing_attempt(log_id, &msg)
                    .await;
            }
        }
        return;
    }

    // Non-2xx passthrough: error attempt row, never billable (Tavily parity).
    let _ = ctx
        .state
        .proxy
        .record_token_attempt_with_kind_request_log_metadata(
            token_id,
            ctx.method,
            ctx.path,
            ctx.query,
            Some(http_code),
            Some(http_code),
            true,
            ctx.outcome.result_status,
            None,
            ctx.request_kind,
            ctx.outcome.failure_kind,
            Some(ctx.response.key_effect_code.as_str()),
            ctx.response.key_effect_summary.as_deref(),
            Some(ctx.response.binding_effect_code.as_str()),
            ctx.response.binding_effect_summary.as_deref(),
            Some(ctx.response.selection_effect_code.as_str()),
            ctx.response.selection_effect_summary.as_deref(),
            ctx.response.request_log_id,
        )
        .await;
}

#[cfg(test)]
mod exa_post_body_tests {
    use super::*;

    #[test]
    fn truthy_stream_values_reject_with_the_documented_boundary_message() {
        // Given an Exa POST body carrying any present stream value other than
        // an explicit false/null; When the boundary prepares the body; Then
        // it rejects with the typed streaming-boundary message BEFORE any
        // gate or upstream call (plan todo 13 zero-cost rule).
        for stream in [
            json!(true),
            json!("true"),
            json!("always"),
            json!("auto"),
            json!("sources-first"),
            json!(1),
            json!(2.5),
            json!({ "mode": "on" }),
        ] {
            let body = serde_json::to_vec(&json!({ "query": "x", "stream": stream }))
                .expect("encode body");
            assert_eq!(
                prepare_exa_post_body(&body),
                Err(EXA_STREAM_REJECT_MESSAGE),
                "stream={stream} must reject"
            );
        }
    }

    #[test]
    fn falsy_and_absent_stream_pass_bytes_through_unchanged() {
        // Given stream:false, stream:null, or no stream field; Then the
        // original bytes forward verbatim (only truthy values reject).
        for body in [
            br#"{"query":"x","stream":false}"#.to_vec(),
            br#"{"query":"x","stream":null}"#.to_vec(),
            br#"{"query":"x"}"#.to_vec(),
        ] {
            assert_eq!(prepare_exa_post_body(&body), Ok(body.clone()));
        }
    }

    #[test]
    fn client_api_key_like_body_fields_are_stripped_and_rest_verbatim() {
        // Given client api_key-like fields mixed into an object body; When
        // prepared; Then only those fields disappear (never relayed, never
        // persisted) and the remaining fields survive.
        let body =
            br#"{"query":"x","api_key":"th-a","APIKEY":"th-b","x-api-key":"th-c","x_api_key":"th-d","keep":1}"#;
        let prepared = prepare_exa_post_body(body).expect("prepared");
        let parsed: Value = serde_json::from_slice(&prepared).expect("json");
        assert_eq!(parsed, json!({ "query": "x", "keep": 1 }));
        let text = String::from_utf8_lossy(&prepared);
        for slot in ["th-a", "th-b", "th-c", "th-d"] {
            assert!(!text.contains(slot), "{text}");
        }
    }

    #[test]
    fn non_json_and_non_object_bodies_forward_unchanged() {
        // Given payloads Exa itself must validate; Then they pass through
        // byte-identical for a native upstream 4xx (no request-body schema
        // validation beyond the stream rejection, M22).
        for body in [
            br#"not json at all"#.to_vec(),
            br#"["array","body"]"#.to_vec(),
            br#""scalar""#.to_vec(),
        ] {
            assert_eq!(prepare_exa_post_body(&body), Ok(body.clone()));
        }
        assert_eq!(prepare_exa_post_body(b""), Ok(Vec::new()));
    }
}

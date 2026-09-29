//! SerpApi hosted MCP passthrough pipeline (plan todo 15; amendments M6/M7).
//!
//! Bin-side orchestration layered over the lib executor
//! (`TavilyProxy::proxy_provider_mcp_passthrough`): provider-scoped session
//! resolution + translation (client proxy session id <-> upstream protocol
//! session id), `mcp_sessions.provider='serpapi'` stamping, the MCP tool
//! billing matrix (search/search_table/search_dashboard = 1 credit each;
//! method-level non-billable gating shared with the Tavily pipeline via
//! `is_non_billable_mcp_method`), the follow-up retry ladder mirroring
//! `proxy_mcp_follow_up_with_retry`, and reserve→settle billing through the
//! REAL persistence seams (billing_ledger + auth_token_logs rows).
//!
//! M6: `rebalance_mcp_enabled` is never consulted here (the switch does not
//! gate the passthrough) and session gating is provider-scoped, so a Tavily
//! control session never suppresses or entangles a serpapi session.
//! HA dual-active boundary (documented, todo 20/27): peer session copies
//! travel the legacy provider-less DTO, so serpapi follow-ups resolve from
//! the local session store.

use super::*;
use tavily_hikari::{
    ProviderMcpPassthroughRequest, extract_mcp_has_error_by_id_from_bytes,
    mcp_response_has_any_error, mcp_response_has_any_success, serpapi_facade_outcome,
};

/// Billable SerpApi hosted tools (amendment M7): each consumes a real
/// upstream search — "only successful searches counted" — 1 credit each,
/// flat. Any other tools/call name is a proven non-business call (T3 kind
/// `mcp:third-party-tool` is non-billable in every persistence seam).
const SERPAPI_MCP_BILLABLE_TOOLS: &[&str] = &["search", "search_table", "search_dashboard"];
const SERPAPI_MCP_TOOL_CREDITS: i64 = 1;

/// Everything the serpapi hosted-MCP pipeline needs from the shared facade
/// entry (auth + gates already applied there, exactly like the Tavily `/mcp`
/// and provider REST seams).
pub(super) struct ProviderMcpFacadeContext<'a> {
    pub(super) provider: ProviderId,
    pub(super) method: &'a Method,
    pub(super) path: &'a str,
    pub(super) request_kind: &'a TokenRequestKind,
    pub(super) body: &'a [u8],
    pub(super) auth: &'a ProviderFacadeAuth,
    pub(super) original_headers: &'a HeaderMap,
    pub(super) remote_addr: Option<SocketAddr>,
}

/// Billing plan for one serpapi MCP request body (M7 matrix). Control-plane
/// methods and unknown tools count nothing; unparseable bodies count nothing
/// (byte-passthrough forwards them for the upstream's own native error —
/// local billing only ever attaches to a PROVEN successful search call).
struct SerpapiMcpBillingPlan {
    billable: bool,
    expected_credits: i64,
    /// JSON-RPC ids of the billable calls, for per-id response attribution.
    billable_ids: Vec<String>,
    /// Billable calls without a non-null JSON-RPC id (response attribution
    /// falls back to the whole-body success/error scan).
    billable_without_id: i64,
}

impl SerpapiMcpBillingPlan {
    fn empty() -> Self {
        Self {
            billable: false,
            expected_credits: 0,
            billable_ids: Vec::new(),
            billable_without_id: 0,
        }
    }
}

fn serpapi_mcp_billing_plan(body: &[u8]) -> SerpapiMcpBillingPlan {
    let mut plan = SerpapiMcpBillingPlan::empty();
    let Ok(parsed) = serde_json::from_slice::<Value>(body) else {
        return plan;
    };

    let mut record_message = |message: &Value| {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if method == "tools/call" {
            let tool = message
                .get("params")
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if SERPAPI_MCP_BILLABLE_TOOLS.contains(&tool.as_str()) {
                plan.billable = true;
                plan.expected_credits += SERPAPI_MCP_TOOL_CREDITS;
                match message
                    .get("id")
                    .filter(|value| !value.is_null())
                    .map(|value| value.to_string())
                {
                    Some(id) => plan.billable_ids.push(id),
                    None => plan.billable_without_id += 1,
                }
            }
        }
        // Every other method — initialize/ping/tools/list plus the
        // resources/prompts/notifications families (the shared
        // is_non_billable_mcp_method gate) AND unknown methods — is
        // non-billable in the T3 persistence seams, so it plans zero.
    };

    match parsed {
        Value::Object(_) => record_message(&parsed),
        Value::Array(items) => {
            for item in &items {
                if item.is_object() {
                    record_message(item);
                }
            }
        }
        _ => {}
    }
    plan
}

/// Credits actually observed in a SUCCESSFUL upstream response: a billable id
/// counts only when the response proves a non-error result for that id
/// (missing id attribution defaults to error — never guess), and without-id
/// calls fall back to the whole-body success scan.
fn observed_serpapi_mcp_credits(plan: &SerpapiMcpBillingPlan, response_body: &[u8]) -> i64 {
    let errors_by_id = extract_mcp_has_error_by_id_from_bytes(response_body);
    let mut total = plan
        .billable_ids
        .iter()
        .filter(|id| errors_by_id.get(id.as_str()) == Some(&false))
        .count() as i64
        * SERPAPI_MCP_TOOL_CREDITS;
    if plan.billable_without_id > 0
        && plan.billable_ids.is_empty()
        && mcp_response_has_any_success(response_body)
        && !mcp_response_has_any_error(response_body)
    {
        total += plan.billable_without_id * SERPAPI_MCP_TOOL_CREDITS;
    }
    total
}

/// Hourly-flavor 429 detector over the T3 classifier vocabulary (the run-out
/// flavor classifies Exhausted and never retries — only RateLimited arms the
/// session retry window).
fn provider_mcp_response_rate_limited(response: &tavily_hikari::ProxyResponse) -> bool {
    serpapi_facade_outcome(response.status, &response.body).failure_kind
        == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429_CODE)
}

/// Follow-up retry ladder mirroring `proxy_mcp_follow_up_with_retry`
/// (mcp_helpers.rs) over the provider executor and the T3 rate-limit
/// detection: wait out the armed session window, send, and on hourly-flavor
/// 429 arm the window, sleep Retry-After, and retry ONCE.
async fn provider_mcp_follow_up_with_retry(
    state: &Arc<AppState>,
    proxy_session_id: &str,
    request: ProviderMcpPassthroughRequest,
) -> Result<tavily_hikari::ProxyResponse, ProxyError> {
    let waited_before_send = wait_for_mcp_session_retry_window(state, proxy_session_id).await?;
    let mut first_response = state
        .proxy
        .proxy_provider_mcp_passthrough(request.clone())
        .await?;
    let first_rate_limited = provider_mcp_response_rate_limited(&first_response);

    if !first_rate_limited {
        if waited_before_send {
            annotate_request_log_key_effect_if_none(
                state,
                first_response.request_log_id,
                KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
                Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
            )
            .await;
            set_proxy_response_key_effect_if_none(
                &mut first_response,
                KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
                Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
            );
        }
        let _ = state
            .proxy
            .clear_mcp_session_rate_limit(proxy_session_id)
            .await;
        return Ok(first_response);
    }

    let now = state.proxy.backend_time().now_ts();
    let retry_after_secs = mcp_session_retry_after_secs(&first_response.headers, now);
    state
        .proxy
        .mark_mcp_session_rate_limited(
            proxy_session_id,
            now + retry_after_secs,
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429_CODE),
        )
        .await?;
    annotate_request_log_key_effect_if_none(
        state,
        first_response.request_log_id,
        KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED_CODE,
        Some(KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED_SUMMARY),
    )
    .await;

    state
        .proxy
        .backend_time()
        .sleep(Duration::from_secs(retry_after_secs.max(0) as u64))
        .await;

    let mut retry_response = state.proxy.proxy_provider_mcp_passthrough(request).await?;
    if provider_mcp_response_rate_limited(&retry_response) {
        let now = state.proxy.backend_time().now_ts();
        let retry_after_secs = mcp_session_retry_after_secs(&retry_response.headers, now);
        state
            .proxy
            .mark_mcp_session_rate_limited(
                proxy_session_id,
                now + retry_after_secs,
                Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429_CODE),
            )
            .await?;
    } else {
        annotate_request_log_key_effect_if_none(
            state,
            retry_response.request_log_id,
            KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
            Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
        )
        .await;
        set_proxy_response_key_effect_if_none(
            &mut retry_response,
            KEY_EFFECT_MCP_SESSION_RETRY_WAITED_CODE,
            Some(KEY_EFFECT_MCP_SESSION_RETRY_WAITED_SUMMARY),
        );
        let _ = state
            .proxy
            .clear_mcp_session_rate_limit(proxy_session_id)
            .await;
    }

    Ok(retry_response)
}

/// SerpApi hosted MCP passthrough body (replaces the todo-11 typed
/// `provider_mcp_pending` boundary for the serpapi surface only).
pub(super) async fn provider_mcp_passthrough(
    state: Arc<AppState>,
    ctx: ProviderMcpFacadeContext<'_>,
) -> Result<Response<Body>, StatusCode> {
    // Tavily Control-mode parity (/mcp handler): a GET opening an event
    // stream is rejected locally — the hosted passthrough buffers responses,
    // so a stream-open GET would hang the request instead of serving it.
    if *ctx.method == Method::GET && accepts_event_stream(ctx.original_headers) {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }
    let system_settings = state.proxy.get_system_settings().await.map_err(|err| {
        eprintln!("get system settings failed for {}: {err}", ctx.path);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let client_ip = resolve_client_ip_info(
        ctx.remote_addr,
        ctx.original_headers,
        &TrustedClientIpSettings {
            trusted_proxy_cidrs: system_settings.trusted_proxy_cidrs.clone(),
            trusted_client_ip_headers: system_settings.trusted_client_ip_headers.clone(),
        },
    );

    // Body summary drives ONLY the initialize/session planning + billing
    // plan; protocol errors stay upstream-owned (byte-passthrough: the
    // native MCP error comes back from the provider verbatim).
    let summary = summarize_mcp_jsonrpc_body(ctx.body).ok();
    let is_initialize = summary.as_ref().is_some_and(|summary| {
        summary.contains_initialize && !summary.is_batch && summary.request_count == 1
    });

    let token_id = ctx.auth.token_id.clone();
    let token_user_id = if let Some(tid) = token_id.as_deref() {
        state
            .proxy
            .find_user_id_by_token(tid)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        None
    };

    let incoming_proxy_session_id = header_string(&ctx.auth.headers, "mcp-session-id");
    let incoming_protocol_version = header_string(&ctx.auth.headers, "mcp-protocol-version");
    let incoming_last_event_id = header_string(&ctx.auth.headers, "last-event-id");

    // ---- Provider-scoped session resolution (M6) ------------------------
    // Only serpapi-stamped rows resolve here; a Tavily (legacy NULL-tag)
    // session id presented on this surface is treated as unknown.
    let mut forward_headers = ctx.auth.headers.clone();
    let mut pinned_api_key_id: Option<String> = None;
    let mut upstream_session_id: Option<String> = None;
    let mut session_request_guard = None;
    if let Some(proxy_session_id) = incoming_proxy_session_id.as_deref() {
        let session = state
            .proxy
            .get_active_mcp_session_for_provider(proxy_session_id, ctx.provider)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let Some(session) = session else {
            return mcp_session_response(
                StatusCode::NOT_FOUND,
                "session_unavailable",
                "MCP session is unavailable, please reconnect to initialize a new session.",
            );
        };
        if session.auth_token_id.as_deref() != token_id.as_deref() {
            return mcp_session_response(
                StatusCode::FORBIDDEN,
                "session_forbidden",
                "This MCP session belongs to a different token. Please reconnect.",
            );
        };
        if session.gateway_mode != tavily_hikari::MCP_GATEWAY_MODE_UPSTREAM {
            return mcp_session_response(
                StatusCode::NOT_FOUND,
                "session_unavailable",
                "MCP session is unavailable, please reconnect to initialize a new session.",
            );
        }
        let (Some(pinned_upstream_session_id), Some(pinned_key_id)) = (
            session.upstream_session_id.clone(),
            session.upstream_key_id.clone(),
        ) else {
            return mcp_session_response(
                StatusCode::NOT_FOUND,
                "session_unavailable",
                "MCP session is unavailable, please reconnect to initialize a new session.",
            );
        };
        upstream_session_id = Some(pinned_upstream_session_id);
        pinned_api_key_id = Some(pinned_key_id);

        // Effective protocol version (Tavily control-path parity).
        let effective_protocol_version = incoming_protocol_version
            .clone()
            .or(session.protocol_version.clone());
        if let Some(protocol_version) = effective_protocol_version.as_deref() {
            forward_headers.insert(
                HeaderName::from_static("mcp-protocol-version"),
                ReqHeaderValue::from_str(protocol_version).map_err(|_| StatusCode::BAD_REQUEST)?,
            );
        }

        let guard = state
            .proxy
            .lock_mcp_session_request(proxy_session_id)
            .await
            .map_err(|err| {
                eprintln!("mcp session request lock failed: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        guard.ensure_live().map_err(|err| {
            eprintln!("mcp session request lock lost before provider follow-up: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        session_request_guard = Some(guard);
    }

    // ---- Initialize planning (M6: rebalance_mcp_enabled never consulted) -
    let mut planned_initialize_proxy_session_id: Option<String> = None;
    let mut session_init_guard = None;
    if is_initialize && incoming_proxy_session_id.is_none() {
        planned_initialize_proxy_session_id = Some(nanoid::nanoid!(24));
        let guard = state
            .proxy
            .lock_mcp_session_init(token_id.as_deref(), token_user_id.as_deref())
            .await
            .map_err(|err| {
                eprintln!("mcp session init lock failed: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        if let Some(guard) = guard.as_ref()
            && let Err(err) = guard.ensure_live()
        {
            eprintln!("mcp session init lock lost before provider initialize: {err}");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
        session_init_guard = guard;
    }

    // ---- Billing plan + uniform gates (REST facade order) ---------------
    let plan = serpapi_mcp_billing_plan(ctx.body);
    let token_billing_guard = if plan.billable
        && let Some(tid) = token_id.as_deref()
    {
        Some(state.proxy.lock_token_billing(tid).await.map_err(|err| {
            eprintln!("token billing lock failed for {}: {err}", ctx.path);
            StatusCode::INTERNAL_SERVER_ERROR
        })?)
    } else {
        None
    };
    let billing_subject = token_billing_guard
        .as_ref()
        .map(|guard| guard.billing_subject().to_string());
    if let Some(guard) = token_billing_guard.as_ref() {
        guard.ensure_live().map_err(|err| {
            eprintln!(
                "token billing lock lost before precheck for {}: {err}",
                ctx.path
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if plan.billable
        && let Some(tid) = token_id.as_deref()
    {
        let verdict = match billing_subject.as_deref() {
            Some(subject) => state.proxy.peek_token_quota_for_subject(subject).await,
            None => state.proxy.peek_token_quota(tid).await,
        }
        .map_err(|err| {
            eprintln!("quota peek failed for {}: {err}", ctx.path);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        if quota_would_exceed(&verdict, plan.expected_credits) {
            let message = build_quota_error_message(&verdict, Some(plan.expected_credits));
            let _ = state
                .proxy
                .record_token_attempt_with_kind(
                    tid,
                    ctx.method,
                    ctx.path,
                    ctx.auth.query.as_deref(),
                    Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                    None,
                    true,
                    "quota_exhausted",
                    Some(&message),
                    ctx.request_kind,
                )
                .await;
            return quota_exceeded_response(&verdict, Some(plan.expected_credits));
        }
    }

    let mut business_calls_reservation = None;
    if plan.billable
        && let Some(tid) = token_id.as_deref()
    {
        let reservation = match billing_subject.as_deref() {
            Some(subject) => {
                state
                    .proxy
                    .reserve_token_business_calls_1h_limit_for_subject(subject)
                    .await
            }
            None => state.proxy.reserve_token_business_calls_1h_limit(tid).await,
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
                        ctx.method,
                        ctx.path,
                        ctx.auth.query.as_deref(),
                        Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                        None,
                        false,
                        "quota_exhausted",
                        Some(&message),
                        ctx.request_kind,
                    )
                    .await;
                return business_calls_1h_limit_exceeded_response(&verdict);
            }
            Ok(tavily_hikari::BusinessCalls1hReservationOutcome::NotApplicable) => {}
            Err(err) => {
                eprintln!("business calls 1h reserve failed for {}: {err}", ctx.path);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    if let Some(guard) = session_request_guard.as_ref()
        && let Err(err) = guard.ensure_live()
    {
        state
            .proxy
            .release_business_calls_1h_reservation(business_calls_reservation.take())
            .await;
        eprintln!("mcp session request lock lost before provider send: {err}");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ---- Execute (follow-ups reuse the retry ladder) --------------------
    let passthrough_request = ProviderMcpPassthroughRequest {
        provider: ctx.provider,
        method: ctx.method.clone(),
        path: ctx.path.to_string(),
        query: ctx.auth.query.clone(),
        headers: forward_headers,
        body: ctx.body.to_vec(),
        auth_token_id: token_id.clone(),
        pinned_api_key_id,
        upstream_session_id,
        proxy_session_id: incoming_proxy_session_id
            .clone()
            .or_else(|| planned_initialize_proxy_session_id.clone()),
        client_ip: Some(client_ip.clone()),
    };
    let proxy_result = if let Some(proxy_session_id) = incoming_proxy_session_id.as_deref() {
        provider_mcp_follow_up_with_retry(&state, proxy_session_id, passthrough_request).await
    } else {
        state
            .proxy
            .proxy_provider_mcp_passthrough(passthrough_request)
            .await
    };

    match proxy_result {
        Ok(mut resp) => {
            let outcome = serpapi_facade_outcome(resp.status, &resp.body);

            // ---- Session persistence + header translation ---------------
            if is_initialize && resp.status.is_success() && incoming_proxy_session_id.is_none() {
                if let Some(guard) = session_init_guard.as_ref()
                    && let Err(err) = guard.ensure_live()
                {
                    state
                        .proxy
                        .finalize_business_calls_1h_reservation_from_status(
                            business_calls_reservation.take(),
                            outcome.result_status,
                            resp.request_log_id,
                        )
                        .await;
                    eprintln!("mcp session init lock lost before provider session bind: {err}");
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
                let upstream_header_session_id = resp
                    .headers
                    .get("mcp-session-id")
                    .and_then(|value| value.to_str().ok())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                if let (Some(proxy_session_id), Some(upstream_session_id), Some(upstream_key_id)) = (
                    planned_initialize_proxy_session_id.as_deref(),
                    upstream_header_session_id.as_deref(),
                    resp.api_key_id.as_deref(),
                ) {
                    let response_protocol_version = resp
                        .headers
                        .get("mcp-protocol-version")
                        .and_then(|value| value.to_str().ok())
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    if state
                        .proxy
                        .create_provider_mcp_session_record(
                            proxy_session_id,
                            upstream_session_id,
                            upstream_key_id,
                            token_id.as_deref(),
                            token_user_id.as_deref(),
                            response_protocol_version.or(incoming_protocol_version.as_deref()),
                            incoming_last_event_id.as_deref(),
                            ctx.provider,
                        )
                        .await
                        .is_err()
                    {
                        state
                            .proxy
                            .finalize_business_calls_1h_reservation_from_status(
                                business_calls_reservation.take(),
                                outcome.result_status,
                                resp.request_log_id,
                            )
                            .await;
                        return Err(StatusCode::INTERNAL_SERVER_ERROR);
                    }
                    // Session id translation: the client only ever sees the
                    // proxy session id.
                    if let Ok(proxy_header) = ReqHeaderValue::from_str(proxy_session_id) {
                        resp.headers
                            .insert(HeaderName::from_static("mcp-session-id"), proxy_header);
                    }
                }
            } else if let Some(proxy_session_id) = incoming_proxy_session_id.as_deref() {
                if mcp_response_requires_reconnect(resp.status, &resp.body) {
                    let _ = state
                        .proxy
                        .revoke_mcp_session(proxy_session_id, "upstream_session_invalid")
                        .await;
                    resp.status = StatusCode::NOT_FOUND;
                    resp.headers = ReqHeaderMap::new();
                    resp.headers.insert(
                        CONTENT_TYPE,
                        ReqHeaderValue::from_static("application/json; charset=utf-8"),
                    );
                    resp.body = mcp_session_body(
                        "session_unavailable",
                        "Upstream MCP session expired or became unavailable. Please reconnect.",
                    );
                } else {
                    let response_protocol_version = resp
                        .headers
                        .get("mcp-protocol-version")
                        .and_then(|value| value.to_str().ok())
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    let _ = state
                        .proxy
                        .touch_mcp_session(
                            proxy_session_id,
                            response_protocol_version.or(incoming_protocol_version.as_deref()),
                            incoming_last_event_id.as_deref(),
                        )
                        .await;
                    if let Some(rotated_upstream_session_id) = resp
                        .headers
                        .get("mcp-session-id")
                        .and_then(|value| value.to_str().ok())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .filter(|value| *value != proxy_session_id)
                    {
                        let _ = state
                            .proxy
                            .update_mcp_session_upstream_identity(
                                proxy_session_id,
                                rotated_upstream_session_id,
                                response_protocol_version.or(incoming_protocol_version.as_deref()),
                            )
                            .await;
                    }
                    // Session id translation is total on follow-ups: the
                    // client only ever sees its proxy session id (any
                    // upstream-echoed id is replaced, an omitted one is
                    // supplied — the upstream protocol id never leaks).
                    if let Ok(proxy_header) = ReqHeaderValue::from_str(proxy_session_id) {
                        resp.headers
                            .insert(HeaderName::from_static("mcp-session-id"), proxy_header);
                    }
                }
            }

            // ---- Billing settle through the REAL seams ------------------
            // Flat M7 credits observed against per-id response attribution;
            // duplicate-charge protection: local persistence failures never
            // replace the verbatim upstream response.
            let mut billing_error: Option<String> = None;
            if let Some(tid) = token_id.as_deref() {
                let http_code = i64::from(resp.status.as_u16());
                let api_key_id = resp.api_key_id.as_deref();
                let credits = if plan.billable && resp.status.is_success() {
                    observed_serpapi_mcp_credits(&plan, &resp.body)
                } else {
                    0
                };
                let mut attempt_logged = false;
                if credits > 0 {
                    let recorded =
                        match billing_subject.as_deref() {
                            Some(subject) => state
                                .proxy
                                .record_pending_billing_attempt_for_subject_with_kind_request_log(
                                    tid,
                                    ctx.method,
                                    ctx.path,
                                    ctx.auth.query.as_deref(),
                                    Some(http_code),
                                    Some(http_code),
                                    plan.billable,
                                    outcome.result_status,
                                    None,
                                    credits,
                                    subject,
                                    ctx.request_kind,
                                    api_key_id,
                                    outcome.failure_kind,
                                    Some(resp.key_effect_code.as_str()),
                                    resp.key_effect_summary.as_deref(),
                                    Some(resp.binding_effect_code.as_str()),
                                    resp.binding_effect_summary.as_deref(),
                                    Some(resp.selection_effect_code.as_str()),
                                    resp.selection_effect_summary.as_deref(),
                                    resp.request_log_id,
                                )
                                .await,
                            None => {
                                state
                                    .proxy
                                    .record_pending_billing_attempt_with_kind_request_log_metadata(
                                        tid,
                                        ctx.method,
                                        ctx.path,
                                        ctx.auth.query.as_deref(),
                                        Some(http_code),
                                        Some(http_code),
                                        plan.billable,
                                        outcome.result_status,
                                        None,
                                        credits,
                                        ctx.request_kind,
                                        api_key_id,
                                        outcome.failure_kind,
                                        Some(resp.key_effect_code.as_str()),
                                        resp.key_effect_summary.as_deref(),
                                        Some(resp.binding_effect_code.as_str()),
                                        resp.binding_effect_summary.as_deref(),
                                        Some(resp.selection_effect_code.as_str()),
                                        resp.selection_effect_summary.as_deref(),
                                        resp.request_log_id,
                                    )
                                    .await
                            }
                        };
                    match recorded {
                        Ok(log_id) => {
                            attempt_logged = true;
                            let lock_lost_msg = token_billing_guard
                                .as_ref()
                                .and_then(|guard| guard.ensure_live().err())
                                .map(|err| {
                                    format!(
                                        "charge_token_quota deferred for {}: {err}; pending billing will retry",
                                        ctx.path
                                    )
                                });
                            if let Some(msg) = lock_lost_msg {
                                eprintln!("{msg}");
                                let _ = state
                                    .proxy
                                    .annotate_pending_billing_attempt(log_id, &msg)
                                    .await;
                                billing_error = Some(msg);
                            } else {
                                match state.proxy.settle_pending_billing_attempt(log_id).await {
                                    Ok(tavily_hikari::PendingBillingSettleOutcome::Charged)
                                    | Ok(
                                        tavily_hikari::PendingBillingSettleOutcome::AlreadySettled,
                                    ) => {}
                                    Ok(tavily_hikari::PendingBillingSettleOutcome::RetryLater) => {
                                        let msg = format!(
                                            "charge_token_quota delayed for {}: pending billing claim miss; will retry",
                                            ctx.path
                                        );
                                        eprintln!("{msg}");
                                        let _ = state
                                            .proxy
                                            .annotate_pending_billing_attempt(log_id, &msg)
                                            .await;
                                        billing_error = Some(msg);
                                    }
                                    Err(err) => {
                                        let msg = format!(
                                            "charge_token_quota failed for {}: {err}",
                                            ctx.path
                                        );
                                        eprintln!("{msg}");
                                        let _ = state
                                            .proxy
                                            .annotate_pending_billing_attempt(log_id, &msg)
                                            .await;
                                        billing_error = Some(msg);
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            let msg = format!(
                                "record_pending_billing_attempt failed for {}: {err}",
                                ctx.path
                            );
                            eprintln!("{msg}");
                            billing_error = Some(msg);
                        }
                    }
                }
                if !attempt_logged {
                    let _ = state
                        .proxy
                        .record_token_attempt_with_kind_request_log_metadata(
                            tid,
                            ctx.method,
                            ctx.path,
                            ctx.auth.query.as_deref(),
                            Some(http_code),
                            Some(http_code),
                            plan.billable,
                            outcome.result_status,
                            billing_error.as_deref(),
                            ctx.request_kind,
                            outcome.failure_kind,
                            Some(resp.key_effect_code.as_str()),
                            resp.key_effect_summary.as_deref(),
                            Some(resp.binding_effect_code.as_str()),
                            resp.binding_effect_summary.as_deref(),
                            Some(resp.selection_effect_code.as_str()),
                            resp.selection_effect_summary.as_deref(),
                            resp.request_log_id,
                        )
                        .await;
                }
            }
            state
                .proxy
                .finalize_business_calls_1h_reservation_from_status(
                    business_calls_reservation.take(),
                    outcome.result_status,
                    resp.request_log_id,
                )
                .await;
            // Always return the upstream response verbatim, even if local
            // billing persistence failed (duplicate-charge protection).
            Ok(build_response(resp))
        }
        Err(err) => {
            state
                .proxy
                .release_business_calls_1h_reservation(business_calls_reservation.take())
                .await;
            eprintln!("provider mcp passthrough error for {}: {err}", ctx.path);
            if let Some(tid) = token_id.as_deref() {
                let err_str = err.to_string();
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        ctx.method,
                        ctx.path,
                        ctx.auth.query.as_deref(),
                        None,
                        None,
                        plan.billable,
                        "error",
                        Some(err_str.as_str()),
                        ctx.request_kind,
                    )
                    .await;
            }
            if matches!(&err, ProxyError::PinnedMcpSessionUnavailable)
                && let Some(proxy_session_id) = incoming_proxy_session_id.as_deref()
            {
                let _ = state
                    .proxy
                    .revoke_mcp_session(proxy_session_id, "pinned_key_unavailable")
                    .await;
                return mcp_session_response(
                    StatusCode::NOT_FOUND,
                    "session_unavailable",
                    "The pinned MCP session key is unavailable. Please reconnect.",
                );
            }
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

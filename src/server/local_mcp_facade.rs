//! Provider-parameterized LOCAL MCP facade framework (plan todo 16; M19/M22).
//!
//! Generalizes the rebalance local-facade machinery — JSON-RPC envelope, batch
//! aggregation, SSE transport, method dispatch, and the reserve→settle billing
//! path — into a provider-parameterized [`LocalMcpFacade`] WITHOUT touching the
//! Tavily rebalance golden path. Tavily keeps [`handle_rebalance_mcp_request_body`]
//! (rebalance_handlers.rs) byte-stable; this framework is a NEW module composed
//! of the SAME shared envelope/plumbing helpers (mcp_helpers.rs) plus per-provider
//! catalog DATA ([`LocalMcpToolDefinition`]) and a translate hook.
//!
//! M19: the catalog is per-provider DATA with validation rules (the single-
//! required-field model is expressed as a per-tool validate fn so todo-17 can add
//! an XOR variant); identity constants are parameterized (`server_name`,
//! `ingress_path`); provider translation lives in NEW modules (never editing the
//! Tavily translation). The framework carries TWO small provider seams — the
//! lib-executor dispatch and the outcome projection — that todo-17 extends with
//! an Exa arm.
//!
//! M22: local validation is the minimal contract guard (required fields only);
//! the brave upstream enforces bounds. No streaming, no hosted passthrough.

use super::*;
use tavily_hikari::{BusinessCalls1hReservationOutcome, TokenBillingGuard};

/// Version stamped on every local facade's `serverInfo` (mirrors the rebalance
/// constant; a local facade advertises the Hikari build, not an upstream).
const LOCAL_MCP_SERVER_VERSION: &str = REBALANCE_MCP_SERVER_VERSION;

/// The translated upstream REST request a tool call maps to. Brave is a GET with
/// query params; todo-17 Exa is a POST with a JSON body — the enum lets the one
/// framework express both without an escape hatch.
pub(super) enum LocalMcpUpstreamRequest {
    Get { query: Option<String> },
    #[allow(dead_code)] // consumed by todo-17 (Exa local facade)
    Post { body: Vec<u8> },
}

/// One local-facade tool: advertised identity, the object inputSchema builder,
/// the minimal contract guard (M22), the credit tier, and the upstream REST
/// target. Catalog DATA — no provider logic here.
pub(super) struct LocalMcpToolDefinition {
    pub advertised_name: &'static str,
    pub description: &'static str,
    /// Builds the advertised JSON Schema object (provenance cited by the
    /// provider module that defines the tool, per M19).
    pub input_schema: fn() -> Value,
    /// Minimal contract guard (M22): validates the tools/call `arguments`,
    /// returning the validated arguments object or a user-facing error message.
    pub validate: fn(&LocalMcpToolDefinition, Option<&Value>) -> Result<Value, String>,
    /// Required field name (surfaced by the shared validators; also the schema's
    /// single `required` entry for the minimal-guard tools).
    pub required_field: &'static str,
    /// Operation key for the ProviderSpec credits table (plan todo 2); the
    /// credit tier is resolved from the registry (never hardcoded) so the
    /// committed table stays the single source of truth.
    pub credits_op: &'static str,
    /// Native upstream path under the provider REST base.
    pub upstream_path: &'static str,
    /// Additive provider-scoped upstream_operation (REST vocabulary reused).
    pub upstream_operation: &'static str,
}

/// Resolve a tool's credit tier from the committed todo-2 ProviderSpec table
/// (`spec.credits(op)`), mirroring the REST facade's `reserved_credits`.
fn local_mcp_tool_credits(
    provider: tavily_hikari::providers::ProviderId,
    tool: &LocalMcpToolDefinition,
) -> i64 {
    i64::from((tavily_hikari::providers::spec_for(provider).credits)(tool.credits_op))
}

/// Translate hook: (tool, validated arguments object) → upstream REST request.
pub(super) type LocalMcpTranslate =
    fn(&LocalMcpToolDefinition, &Value) -> Result<LocalMcpUpstreamRequest, String>;

/// A provider-parameterized local MCP facade (the M19 generalization).
pub(super) struct LocalMcpFacade {
    pub provider: tavily_hikari::providers::ProviderId,
    /// serverInfo.name advertised on initialize (M19 parameterized identity).
    pub server_name: &'static str,
    /// Client-facing ingress path (also the request_logs `path`/`kind_path`),
    /// e.g. `/brave/mcp`.
    pub ingress_path: &'static str,
    /// Per-provider tool catalog DATA.
    pub tools: &'static [LocalMcpToolDefinition],
    /// Per-provider tools/call → REST translation.
    pub translate: LocalMcpTranslate,
}

impl LocalMcpFacade {
    fn tool_by_name(&self, name: &str) -> Option<&'static LocalMcpToolDefinition> {
        let normalized = name.trim().to_ascii_lowercase();
        self.tools
            .iter()
            .find(|tool| tool.advertised_name.to_ascii_lowercase() == normalized)
            // Tool defs are &'static (from a &'static [..] slice) so the borrow
            // outlives &self; map to a 'static reference.
            .map(|tool| tool as &'static LocalMcpToolDefinition)
    }

    fn build_initialize_body(&self, response_id: Option<&Value>, protocol_version: &str) -> Vec<u8> {
        build_rebalance_mcp_success_body(
            response_id,
            json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": { "listChanged": false },
                    "prompts": { "listChanged": false },
                    "resources": {}
                },
                "serverInfo": {
                    "name": self.server_name,
                    "version": LOCAL_MCP_SERVER_VERSION
                }
            }),
        )
    }

    fn build_tools_list_body(&self, response_id: Option<&Value>) -> Vec<u8> {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.advertised_name,
                    "description": tool.description,
                    "inputSchema": (tool.input_schema)(),
                })
            })
            .collect();
        build_rebalance_mcp_success_body(response_id, json!({ "tools": tools }))
    }
}

/// Shared tool-result envelope builder mirroring the rebalance precedent
/// (`build_rebalance_mcp_tool_result_body`): the native upstream body rides as
/// text content AND structuredContent, the upstream HTTP status is projected
/// into `structuredContent.status`, and any non-2xx (or a native isError) sets
/// `isError=true` carrying the provider error body verbatim (M22: transform
/// nothing beyond the envelope).
fn build_local_mcp_tool_result_body(
    response_id: Option<&Value>,
    upstream_status: StatusCode,
    upstream_body: &[u8],
) -> Vec<u8> {
    let raw_text = String::from_utf8_lossy(upstream_body).into_owned();
    let mut structured_content = match serde_json::from_slice::<Value>(upstream_body) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(other) => json!({ "data": other }),
        Err(_) => json!({ "raw_body": raw_text }),
    };

    let mut is_error = !upstream_status.is_success();
    if let Value::Object(ref mut map) = structured_content {
        map.entry("status".to_string())
            .or_insert(Value::from(i64::from(upstream_status.as_u16())));
        is_error |= map.get("isError").and_then(Value::as_bool).unwrap_or(false);
        map.remove("isError");
    }

    let mut result = serde_json::Map::new();
    result.insert("content".to_string(), Value::Array(Vec::new()));
    result.insert("structuredContent".to_string(), structured_content);
    if !raw_text.trim().is_empty() {
        result.insert(
            "content".to_string(),
            json!([{ "type": "text", "text": raw_text }]),
        );
    }
    if is_error {
        result.insert("isError".to_string(), Value::Bool(true));
    }
    build_rebalance_mcp_success_body(response_id, Value::Object(result))
}

/// Observable outcome of one billable tools/call execution (drives settle).
struct LocalMcpBusinessCall {
    /// Credits actually observed (tool tier on success, 0 otherwise).
    observed_credits: i64,
    /// success / quota_exhausted / error (shared OUTCOME_* vocabulary).
    result_status: &'static str,
    failure_kind: Option<&'static str>,
}

/// Everything one dispatch needs, grouped (mirrors the ProviderMcpFacadeContext
/// hand-off so the dispatcher signature stays readable).
struct LocalMcpDispatchCtx<'a> {
    facade: &'a LocalMcpFacade,
    state: &'a Arc<AppState>,
    method: &'a Method,
    path: &'a str,
    request_kind: &'a TokenRequestKind,
    client_headers: &'a HeaderMap,
    token_id: Option<&'a str>,
    auth_query: Option<&'a str>,
    client_ip: &'a ClientIpInfo,
}

/// Provider-tagged local control-plane log + SSE/JSON response build (the brave
/// analogue of `build_and_log_local_mcp_protocol_response`, but stamping
/// `request_logs.provider` so the dimension never coalesces into the legacy
/// tavily default).
async fn build_and_log_local_facade_response(
    ctx: &LocalMcpDispatchCtx<'_>,
    message_body: &[u8],
    response_status: StatusCode,
    response_body: &[u8],
    failure_kind: Option<&str>,
) -> Result<ProxyResponse, StatusCode> {
    let analysis = analyze_mcp_attempt(response_status, response_body);
    let empty_headers: [String; 0] = [];
    let request_log_id = ctx
        .state
        .proxy
        .record_local_provider_request_log_without_key(
            ctx.facade.provider,
            ctx.token_id,
            ctx.method,
            ctx.path,
            ctx.auth_query,
            response_status,
            analysis.tavily_status_code,
            message_body,
            response_body,
            analysis.status,
            failure_kind.or(analysis.failure_kind.as_deref()),
            &empty_headers,
            &empty_headers,
            Some(ctx.client_ip),
        )
        .await
        .ok();
    Ok(proxy_response_with_mcp_body(
        response_status,
        response_body.to_vec(),
        request_log_id,
    ))
}

/// Build a typed local MCP tool-error result (isError envelope) + log it. Used
/// for unknown-tool, invalid-arguments, and no-available-keys cases — all local
/// (zero upstream calls, zero billing).
async fn local_tool_error_response(
    ctx: &LocalMcpDispatchCtx<'_>,
    message_body: &[u8],
    response_id: Option<&Value>,
    message: &str,
    failure_kind: Option<&str>,
) -> Result<ProxyResponse, StatusCode> {
    let body = build_rebalance_mcp_tool_error_result_body(response_id, message);
    build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, failure_kind).await
}

/// Dispatch the provider lib executor for one translated tools/call and project
/// the outcome. The provider match is the small seam todo-17 extends for Exa.
async fn execute_local_mcp_upstream(
    ctx: &LocalMcpDispatchCtx<'_>,
    tool: &LocalMcpToolDefinition,
    message_body: &[u8],
    upstream: LocalMcpUpstreamRequest,
) -> Result<(ProxyResponse, Option<&'static str>, Option<&'static str>), ProxyError> {
    let audit_request = ProviderRestFacadeRequest {
        provider: ctx.facade.provider,
        method: ctx.method.clone(),
        path: ctx.path.to_string(),
        kind_path: ctx.facade.ingress_path,
        upstream_path: tool.upstream_path,
        query: ctx.auth_query.map(str::to_string),
        headers: ctx.client_headers.clone(),
        // The client tools/call JSON drives request_kind derivation
        // (mcp:brave/*); it is NEVER sent upstream.
        body: message_body.to_vec(),
        auth_token_id: ctx.token_id.map(str::to_string),
        upstream_operation: tool.upstream_operation,
        client_ip: Some(ctx.client_ip.clone()),
    };

    let resp = match (ctx.facade.provider, upstream) {
        (tavily_hikari::providers::ProviderId::Brave, LocalMcpUpstreamRequest::Get { query }) => {
            ctx.state
                .proxy
                .proxy_provider_mcp_brave_facade_call(audit_request, query)
                .await?
        }
        (provider, _) => {
            return Err(ProxyError::Other(format!(
                "{provider} local MCP facade executor is not wired yet"
            )));
        }
    };

    let outcome = match ctx.facade.provider {
        tavily_hikari::providers::ProviderId::Brave => {
            let outcome = brave_facade_outcome(resp.status, &resp.body);
            (outcome.result_status, outcome.failure_kind)
        }
        provider => {
            return Err(ProxyError::Other(format!(
                "{provider} local MCP facade outcome is not wired yet"
            )));
        }
    };
    Ok((resp, Some(outcome.0), outcome.1))
}

/// Handle one JSON-RPC message. Returns the client-facing (SSE-wrapped) response
/// plus the billable-tool observation when a tools/call actually executed.
async fn handle_local_mcp_single_message(
    ctx: &LocalMcpDispatchCtx<'_>,
    message: &Value,
    message_body: &[u8],
    incoming_protocol_version: Option<&str>,
) -> Result<(ProxyResponse, Option<LocalMcpBusinessCall>), StatusCode> {
    let facade = ctx.facade;
    let response_id = message.get("id").filter(|value| !value.is_null());
    let method_name = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let Some(method_name) = method_name else {
        let body = build_rebalance_mcp_error_body(response_id, -32600, "Invalid Request");
        let resp = build_and_log_local_facade_response(
            ctx,
            message_body,
            StatusCode::BAD_REQUEST,
            &body,
            Some("invalid_jsonrpc_request"),
        )
        .await?;
        return Ok((resp, None));
    };

    match method_name {
        "initialize" => {
            let protocol_version =
                rebalance_initialize_protocol_version(message, incoming_protocol_version);
            let body = facade.build_initialize_body(response_id, &protocol_version);
            let resp =
                build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, None)
                    .await?;
            Ok((resp, None))
        }
        "ping" => {
            let body = build_rebalance_mcp_ping_body(response_id);
            let resp =
                build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, None)
                    .await?;
            Ok((resp, None))
        }
        "tools/list" => {
            let body = facade.build_tools_list_body(response_id);
            let resp =
                build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, None)
                    .await?;
            Ok((resp, None))
        }
        "prompts/list" => {
            let body = build_rebalance_mcp_prompts_list_body(response_id);
            let resp =
                build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, None)
                    .await?;
            Ok((resp, None))
        }
        "resources/list" => {
            let body = build_rebalance_mcp_resources_list_body(response_id);
            let resp =
                build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, None)
                    .await?;
            Ok((resp, None))
        }
        "resources/templates/list" => {
            let body = build_rebalance_mcp_resource_templates_list_body(response_id);
            let resp =
                build_and_log_local_facade_response(ctx, message_body, StatusCode::OK, &body, None)
                    .await?;
            Ok((resp, None))
        }
        "notifications/initialized" => {
            let resp = build_and_log_local_facade_response(
                ctx,
                message_body,
                StatusCode::ACCEPTED,
                &[],
                None,
            )
            .await?;
            Ok((resp, None))
        }
        "tools/call" => handle_local_mcp_tool_call(ctx, message, message_body, response_id).await,
        _ if method_name.starts_with("notifications/") => {
            let resp = build_and_log_local_facade_response(
                ctx,
                message_body,
                StatusCode::ACCEPTED,
                &[],
                None,
            )
            .await?;
            Ok((resp, None))
        }
        _ => {
            let body = build_rebalance_mcp_error_body(response_id, -32601, "Method not found");
            let resp = build_and_log_local_facade_response(
                ctx,
                message_body,
                StatusCode::NOT_FOUND,
                &body,
                Some("method_not_found"),
            )
            .await?;
            Ok((resp, None))
        }
    }
}

/// tools/call: resolve tool, apply the minimal contract guard, translate, execute
/// via the provider lib executor, wrap the native body into the MCP result
/// envelope, and surface the billable observation.
async fn handle_local_mcp_tool_call(
    ctx: &LocalMcpDispatchCtx<'_>,
    message: &Value,
    message_body: &[u8],
    response_id: Option<&Value>,
) -> Result<(ProxyResponse, Option<LocalMcpBusinessCall>), StatusCode> {
    let facade = ctx.facade;
    let Some(params) = message.get("params").and_then(Value::as_object) else {
        let resp = local_tool_error_response(
            ctx,
            message_body,
            response_id,
            "Invalid tools/call params",
            Some("invalid_tool_params"),
        )
        .await?;
        return Ok((resp, None));
    };
    let tool_name = params.get("name").and_then(Value::as_str).unwrap_or_default();
    let Some(tool) = facade.tool_by_name(tool_name) else {
        let message = format!("Not found: Unknown tool: '{tool_name}'");
        let resp =
            local_tool_error_response(ctx, message_body, response_id, &message, Some("unknown_tool"))
                .await?;
        return Ok((resp, None));
    };
    let arguments = match (tool.validate)(tool, params.get("arguments")) {
        Ok(arguments) => arguments,
        Err(message) => {
            let resp = local_tool_error_response(
                ctx,
                message_body,
                response_id,
                &message,
                Some("invalid_tool_arguments"),
            )
            .await?;
            return Ok((resp, None));
        }
    };
    let upstream = match (facade.translate)(tool, &arguments) {
        Ok(upstream) => upstream,
        Err(message) => {
            let resp = local_tool_error_response(
                ctx,
                message_body,
                response_id,
                &message,
                Some("invalid_tool_arguments"),
            )
            .await?;
            return Ok((resp, None));
        }
    };

    match execute_local_mcp_upstream(ctx, tool, message_body, upstream).await {
        Ok((resp, result_status, failure_kind)) => {
            let envelope = build_local_mcp_tool_result_body(response_id, resp.status, &resp.body);
            let mut sse_resp =
                proxy_response_with_mcp_body(StatusCode::OK, envelope, resp.request_log_id);
            sse_resp.api_key_id = resp.api_key_id;
            sse_resp.key_effect_code = resp.key_effect_code;
            sse_resp.key_effect_summary = resp.key_effect_summary;
            sse_resp.binding_effect_code = resp.binding_effect_code;
            sse_resp.binding_effect_summary = resp.binding_effect_summary;
            sse_resp.selection_effect_code = resp.selection_effect_code;
            sse_resp.selection_effect_summary = resp.selection_effect_summary;
            let success = result_status == Some("success");
            let business = LocalMcpBusinessCall {
                observed_credits: if success {
                    local_mcp_tool_credits(facade.provider, tool)
                } else {
                    0
                },
                result_status: result_status.unwrap_or("error"),
                failure_kind,
            };
            Ok((sse_resp, Some(business)))
        }
        Err(err) => {
            // Acquire failure (NoAvailableKeys) or transport failure: a typed
            // isError result carrying the local error text, ZERO upstream calls
            // on the exhaustion path, ZERO billing.
            let message = err.to_string();
            let resp = local_tool_error_response(
                ctx,
                message_body,
                response_id,
                &message,
                Some("upstream_unavailable"),
            )
            .await?;
            Ok((resp, None))
        }
    }
}

/// Billing plan expected credits (the reservation amount for the quota peek +
/// business-call reservation), summed over billable tools/call messages.
fn local_mcp_expected_credits(facade: &LocalMcpFacade, parsed: &Value) -> i64 {
    let mut expected = 0i64;
    let mut record = |message: &Value| {
        if message.get("method").and_then(Value::as_str).map(str::trim) == Some("tools/call") {
            let tool_name = message
                .get("params")
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(tool) = facade.tool_by_name(tool_name) {
                expected += local_mcp_tool_credits(facade.provider, tool);
            }
        }
    };
    match parsed {
        Value::Object(_) => record(parsed),
        Value::Array(items) => {
            for item in items {
                if item.is_object() {
                    record(item);
                }
            }
        }
        _ => {}
    }
    expected
}

/// Reserve → settle billing + token-log recording for a local facade request
/// (mirrors the serpapi hosted-passthrough settle so local persistence failures
/// never block the client response — duplicate-charge protection).
#[allow(clippy::too_many_arguments)]
async fn settle_local_mcp_billing(
    ctx: &LocalMcpDispatchCtx<'_>,
    response: &ProxyResponse,
    token_billing_guard: Option<&TokenBillingGuard>,
    billing_subject: Option<&str>,
    observed_credits: i64,
    result_status: &'static str,
    failure_kind: Option<&'static str>,
) {
    let Some(token_id) = ctx.token_id else {
        return;
    };
    let http_code = i64::from(response.status.as_u16());
    let api_key_id = response.api_key_id.as_deref();

    if observed_credits > 0 {
        let recorded = match billing_subject {
            Some(subject) => {
                ctx.state
                    .proxy
                    .record_pending_billing_attempt_for_subject_with_kind_request_log(
                        token_id,
                        ctx.method,
                        ctx.path,
                        ctx.auth_query,
                        Some(http_code),
                        Some(http_code),
                        true,
                        result_status,
                        None,
                        observed_credits,
                        subject,
                        ctx.request_kind,
                        api_key_id,
                        failure_kind,
                        Some(response.key_effect_code.as_str()),
                        response.key_effect_summary.as_deref(),
                        Some(response.binding_effect_code.as_str()),
                        response.binding_effect_summary.as_deref(),
                        Some(response.selection_effect_code.as_str()),
                        response.selection_effect_summary.as_deref(),
                        response.request_log_id,
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
                        ctx.auth_query,
                        Some(http_code),
                        Some(http_code),
                        true,
                        result_status,
                        None,
                        observed_credits,
                        ctx.request_kind,
                        api_key_id,
                        failure_kind,
                        Some(response.key_effect_code.as_str()),
                        response.key_effect_summary.as_deref(),
                        Some(response.binding_effect_code.as_str()),
                        response.binding_effect_summary.as_deref(),
                        Some(response.selection_effect_code.as_str()),
                        response.selection_effect_summary.as_deref(),
                        response.request_log_id,
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
        let lock_lost_msg = token_billing_guard
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
            Ok(tavily_hikari::PendingBillingSettleOutcome::Charged)
            | Ok(tavily_hikari::PendingBillingSettleOutcome::AlreadySettled) => {}
            Ok(tavily_hikari::PendingBillingSettleOutcome::RetryLater) => {
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

    // Non-billable / errored: a plain attempt row, never a ledger charge.
    let _ = ctx
        .state
        .proxy
        .record_token_attempt_with_kind_request_log_metadata(
            token_id,
            ctx.method,
            ctx.path,
            ctx.auth_query,
            Some(http_code),
            Some(http_code),
            observed_credits == 0 && result_status != "success",
            result_status,
            None,
            ctx.request_kind,
            failure_kind,
            Some(response.key_effect_code.as_str()),
            response.key_effect_summary.as_deref(),
            Some(response.binding_effect_code.as_str()),
            response.binding_effect_summary.as_deref(),
            Some(response.selection_effect_code.as_str()),
            response.selection_effect_summary.as_deref(),
            response.request_log_id,
        )
        .await;
}

/// Dispatch a full request body (single or batch), applying the billing gates
/// and settle. The envelope/batch/SSE structure mirrors
/// `handle_rebalance_mcp_request_body`; the billing gates mirror the serpapi
/// hosted passthrough. Returns the client-facing response.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_local_mcp_request_body(
    facade: &LocalMcpFacade,
    state: &Arc<AppState>,
    method: &Method,
    path: &str,
    request_kind: &TokenRequestKind,
    body_bytes: &[u8],
    client_headers: &HeaderMap,
    token_id: Option<&str>,
    auth_query: Option<&str>,
    client_ip: &ClientIpInfo,
    incoming_protocol_version: Option<&str>,
) -> Result<Response<Body>, StatusCode> {
    let ctx = LocalMcpDispatchCtx {
        facade,
        state,
        method,
        path,
        request_kind,
        client_headers,
        token_id,
        auth_query,
        client_ip,
    };
    let protocol_version = incoming_protocol_version;

    let parsed = match serde_json::from_slice::<Value>(body_bytes) {
        Ok(parsed) => parsed,
        Err(_) => {
            let body = build_rebalance_mcp_error_body(None, -32700, "Parse error");
            let resp = build_and_log_local_facade_response(
                &ctx,
                body_bytes,
                StatusCode::BAD_REQUEST,
                &body,
                Some("parse_error"),
            )
            .await?;
            return Ok(build_response(resp));
        }
    };
    let summary = match summarize_mcp_jsonrpc_body(body_bytes) {
        Ok(summary) => summary,
        Err(_) => {
            let body = build_rebalance_mcp_error_body(None, -32700, "Parse error");
            let resp = build_and_log_local_facade_response(
                &ctx,
                body_bytes,
                StatusCode::BAD_REQUEST,
                &body,
                Some("parse_error"),
            )
            .await?;
            return Ok(build_response(resp));
        }
    };

    // ---- Billing gates (mirror the serpapi hosted passthrough) ------------
    // No `expect`: billing only ever runs when a token is present, expressed as
    // an Option so the gates stay token-optional like the rebalance path.
    let expected_credits = local_mcp_expected_credits(facade, &parsed);
    let billing_token = token_id.filter(|_| expected_credits > 0);
    let token_billing_guard = if let Some(tid) = billing_token {
        Some(state.proxy.lock_token_billing(tid).await.map_err(|err| {
            eprintln!("token billing lock failed for {path}: {err}");
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
            eprintln!("token billing lock lost before precheck for {path}: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    if let Some(tid) = billing_token {
        let verdict = match billing_subject.as_deref() {
            Some(subject) => state.proxy.peek_token_quota_for_subject(subject).await,
            None => state.proxy.peek_token_quota(tid).await,
        }
        .map_err(|err| {
            eprintln!("quota peek failed for {path}: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        if quota_would_exceed(&verdict, expected_credits) {
            let message = build_quota_error_message(&verdict, Some(expected_credits));
            let _ = state
                .proxy
                .record_token_attempt_with_kind(
                    tid,
                    method,
                    path,
                    auth_query,
                    Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                    None,
                    true,
                    "quota_exhausted",
                    Some(&message),
                    request_kind,
                )
                .await;
            return quota_exceeded_response(&verdict, Some(expected_credits));
        }
    }

    let mut business_calls_reservation = None;
    if let Some(tid) = billing_token {
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
            Ok(BusinessCalls1hReservationOutcome::Reserved(reservation)) => {
                business_calls_reservation = Some(reservation);
            }
            Ok(BusinessCalls1hReservationOutcome::Denied(verdict)) => {
                let message = build_business_calls_1h_limit_error_message(&verdict);
                let _ = state
                    .proxy
                    .record_token_attempt_with_kind(
                        tid,
                        method,
                        path,
                        auth_query,
                        Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                        None,
                        false,
                        "quota_exhausted",
                        Some(&message),
                        request_kind,
                    )
                    .await;
                return business_calls_1h_limit_exceeded_response(&verdict);
            }
            Ok(BusinessCalls1hReservationOutcome::NotApplicable) => {}
            Err(err) => {
                eprintln!("business calls 1h reserve failed for {path}: {err}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    // ---- Dispatch (single or batch) ---------------------------------------
    let dispatch =
        dispatch_local_mcp_messages(&ctx, &parsed, body_bytes, &summary, protocol_version).await?;

    // ---- Settle + finalize reservation ------------------------------------
    let finalize_status = if billing_token.is_some() {
        dispatch.business_result_status.unwrap_or("error")
    } else {
        "success"
    };
    settle_local_mcp_billing(
        &ctx,
        &dispatch.response,
        token_billing_guard.as_ref(),
        billing_subject.as_deref(),
        dispatch.observed_credits,
        dispatch.settle_result_status,
        dispatch.failure_kind,
    )
    .await;
    state
        .proxy
        .finalize_business_calls_1h_reservation_from_status(
            business_calls_reservation.take(),
            finalize_status,
            dispatch.response.request_log_id,
        )
        .await;

    Ok(build_response(dispatch.response))
}

/// Aggregated dispatch result across one request body.
struct LocalMcpDispatchResult {
    response: ProxyResponse,
    observed_credits: i64,
    /// Result status used for settle (business outcome if any business call ran,
    /// else analysis of the aggregated control response).
    settle_result_status: &'static str,
    /// Result status used for the business-call reservation finalize.
    business_result_status: Option<&'static str>,
    failure_kind: Option<&'static str>,
}

/// Dispatch all messages (single object or batch array), aggregating responses
/// exactly like `handle_rebalance_mcp_request_body`. `body_bytes` is the original
/// request body (logged verbatim for the single-message case; batch items are
/// re-serialized per item, mirroring rebalance).
async fn dispatch_local_mcp_messages(
    ctx: &LocalMcpDispatchCtx<'_>,
    parsed: &Value,
    body_bytes: &[u8],
    summary: &McpJsonRpcBodySummary,
    protocol_version: Option<&str>,
) -> Result<LocalMcpDispatchResult, StatusCode> {
    match parsed {
        Value::Object(_) => {
            let (response, business) =
                handle_local_mcp_single_message(ctx, parsed, body_bytes, protocol_version).await?;
            Ok(finalize_dispatch(response, business.into_iter().collect()))
        }
        Value::Array(items) => {
            if summary.is_empty_batch || summary.is_response_only_batch() {
                let body = build_rebalance_mcp_error_body(None, -32600, "Invalid Request");
                let response = build_and_log_local_facade_response(
                    ctx,
                    body_bytes,
                    StatusCode::BAD_REQUEST,
                    &body,
                    Some("invalid_jsonrpc_request"),
                )
                .await?;
                return Ok(LocalMcpDispatchResult {
                    settle_result_status: "error",
                    business_result_status: None,
                    failure_kind: Some("invalid_jsonrpc_request"),
                    observed_credits: 0,
                    response,
                });
            }

            let mut responses: Vec<Value> = Vec::new();
            let mut last_response: Option<ProxyResponse> = None;
            let mut business_calls: Vec<LocalMcpBusinessCall> = Vec::new();

            for item in items {
                let item_body =
                    serde_json::to_vec(item).map_err(|_| StatusCode::BAD_REQUEST)?;
                let (response, business) =
                    handle_local_mcp_single_message(ctx, item, &item_body, protocol_version)
                        .await?;
                if let Some(business) = business {
                    business_calls.push(business);
                }
                if !response.body.is_empty()
                    && let Some(value) = parse_mcp_sse_message_body(&response.body)
                        .or_else(|| serde_json::from_slice::<Value>(&response.body).ok())
                {
                    responses.push(value);
                }
                last_response = Some(response);
            }

            if responses.is_empty() {
                let response = last_response.unwrap_or_else(|| {
                    proxy_response_with_mcp_body(StatusCode::ACCEPTED, Vec::new(), None)
                });
                return Ok(finalize_dispatch(response, business_calls));
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
            Ok(finalize_dispatch(aggregated, business_calls))
        }
        _ => {
            let body = build_rebalance_mcp_error_body(None, -32600, "Invalid Request");
            let response = build_and_log_local_facade_response(
                ctx,
                body_bytes,
                StatusCode::BAD_REQUEST,
                &body,
                Some("invalid_jsonrpc_request"),
            )
            .await?;
            Ok(LocalMcpDispatchResult {
                settle_result_status: "error",
                business_result_status: None,
                failure_kind: Some("invalid_jsonrpc_request"),
                observed_credits: 0,
                response,
            })
        }
    }
}

/// Fold per-message business observations into the settle/finalize inputs.
fn finalize_dispatch(
    response: ProxyResponse,
    business_calls: Vec<LocalMcpBusinessCall>,
) -> LocalMcpDispatchResult {
    let observed_credits: i64 = business_calls.iter().map(|call| call.observed_credits).sum();
    let last = business_calls.last();
    let business_result_status = last.map(|call| call.result_status);
    let failure_kind = last.and_then(|call| call.failure_kind);
    // Settle status: the business outcome when a tools/call executed; else the
    // control-plane analysis of the aggregated response.
    let settle_result_status = match last {
        Some(call) => call.result_status,
        None => analyze_mcp_attempt(response.status, &response.body).status,
    };
    LocalMcpDispatchResult {
        response,
        observed_credits,
        settle_result_status,
        business_result_status,
        failure_kind,
    }
}

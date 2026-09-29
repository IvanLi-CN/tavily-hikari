//! Mock SerpAPI upstream (HTTP search + account + streamable MCP) for offline tests.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{Query, RawQuery, State};
use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::Parser;
use serde::Deserialize;
use serde_json::{Value, json};

const MONTHLY_SEARCHES: u64 = 30_000;
const KEY_EXHAUSTED: &str = "key-exhausted";
const KEY_THROTTLED: &str = "key-throttled";

#[derive(Parser)]
#[command(
    name = "mock_serpapi",
    about = "Mock SerpAPI upstream for offline tests"
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:58091")]
    bind: String,
    #[arg(
        long,
        env = "MOCK_SERPAPI_KEYS",
        value_delimiter = ',',
        default_value = "mock-serpapi-key"
    )]
    keys: Vec<String>,
    #[arg(
        long,
        env = "MOCK_SERPAPI_PLAN_RENEWAL_DATE",
        default_value = "2026-10-23"
    )]
    plan_renewal_date: String,
}

#[derive(Debug, PartialEq, Eq)]
enum KeyStatus {
    Valid,
    Invalid,
    Exhausted,
    Throttled,
}

struct MockState {
    keys: Vec<String>,
    searches_left: AtomicU64,
    plan_renewal_date: String,
    sse_errors: bool,
    // Additive request observability counters (plan todo 15; D1 precedent).
    // Response behavior is untouched — these only back GET /mock-metrics.
    mcp_requests: AtomicU64,
    mcp_initialize_requests: AtomicU64,
    mcp_tools_call_requests: AtomicU64,
    tavily_slot_seen: AtomicU64,
    last_authorization: Mutex<Option<String>>,
    last_session_header: Mutex<Option<String>>,
}

impl MockState {
    fn new(keys: Vec<String>, plan_renewal_date: String, sse_errors: bool) -> Self {
        Self {
            keys,
            searches_left: AtomicU64::new(MONTHLY_SEARCHES),
            plan_renewal_date,
            sse_errors,
            mcp_requests: AtomicU64::new(0),
            mcp_initialize_requests: AtomicU64::new(0),
            mcp_tools_call_requests: AtomicU64::new(0),
            tavily_slot_seen: AtomicU64::new(0),
            last_authorization: Mutex::new(None),
            last_session_header: Mutex::new(None),
        }
    }

    /// Record what an inbound request carried (additive; never changes the
    /// response): the last Authorization bearer value, the last
    /// `mcp-session-id` header, and whether ANY Tavily-only client slot
    /// (`Tavily-Api-Key` header or `tavilyApiKey` query) was observed — the
    /// passthrough contract says it never should be on serpapi outbound.
    fn note_outbound_observation(&self, headers: &HeaderMap, raw_query: Option<&str>) {
        if let Some(value) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            *self.last_authorization.lock().expect("auth lock") = Some(value.to_string());
        }
        if let Some(value) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            *self.last_session_header.lock().expect("session lock") = Some(value.to_string());
        }
        let header_slot = headers
            .keys()
            .any(|name| name.as_str().eq_ignore_ascii_case("tavily-api-key"));
        let query_slot = raw_query
            .map(|query| query.to_ascii_lowercase().contains("tavilyapikey"))
            .unwrap_or(false);
        if header_slot || query_slot {
            self.tavily_slot_seen.store(1, Ordering::Relaxed);
        }
    }

    fn classify(&self, key: &str) -> KeyStatus {
        match key {
            KEY_EXHAUSTED => KeyStatus::Exhausted,
            KEY_THROTTLED => KeyStatus::Throttled,
            _ if self.keys.iter().any(|known| known == key) => KeyStatus::Valid,
            _ => KeyStatus::Invalid,
        }
    }

    fn decrement_searches(&self) {
        let _ = self
            .searches_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            });
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    api_key: Option<String>,
    q: Option<String>,
    sse_error: Option<String>,
}

fn invalid_key_body() -> Value {
    json!({
        "error": "Invalid API key. Your API key should be here: https://serpapi.com/manage-api-key"
    })
}

fn run_out_body() -> Value {
    json!({ "error": "Your account has run out of searches." })
}

fn throttled_body() -> Value {
    json!({ "error": "Your account has exceeded its hourly throughput limit." })
}

fn search_success_body() -> Value {
    json!({
        "search_metadata": { "id": "mock", "status": "Success" },
        "search_parameters": {},
        "organic_results": [
            {
                "position": 1,
                "title": "Mock Result",
                "link": "https://example.com/1",
                "snippet": "mock snippet"
            }
        ]
    })
}

fn error_response(status: StatusCode, body: Value, sse: bool) -> Response {
    if sse {
        (
            status,
            [(header::CONTENT_TYPE, "text/event-stream")],
            format!("data: {}\n\n", body),
        )
            .into_response()
    } else {
        (status, Json(body)).into_response()
    }
}

async fn handle_search(
    State(state): State<Arc<MockState>>,
    Query(query): Query<SearchQuery>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
) -> Response {
    state.note_outbound_observation(&headers, raw_query.as_deref());
    let sse = query.sse_error.as_deref() == Some("1") || state.sse_errors;
    let Some(key) = query.api_key.as_deref().filter(|key| !key.is_empty()) else {
        return error_response(StatusCode::UNAUTHORIZED, invalid_key_body(), sse);
    };
    match state.classify(key) {
        KeyStatus::Valid => {}
        KeyStatus::Invalid => {
            return error_response(StatusCode::UNAUTHORIZED, invalid_key_body(), sse);
        }
        KeyStatus::Exhausted => {
            return error_response(StatusCode::TOO_MANY_REQUESTS, run_out_body(), sse);
        }
        KeyStatus::Throttled => {
            return error_response(StatusCode::TOO_MANY_REQUESTS, throttled_body(), sse);
        }
    }
    if query.q.as_deref().unwrap_or_default().trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            json!({ "error": "Missing query `q` parameter." }),
            sse,
        );
    }
    state.decrement_searches();
    Json(search_success_body()).into_response()
}

async fn handle_account(State(state): State<Arc<MockState>>) -> Json<Value> {
    let remaining = state.searches_left.load(Ordering::Relaxed);
    Json(json!({
        "account_status": "Active",
        "plan_id": "bigdata",
        "searches_per_month": MONTHLY_SEARCHES,
        "total_searches_left": remaining,
        "plan_searches_left": remaining,
        "plan_renewal_date": state.plan_renewal_date,
        "this_hour_searches": 1,
        "account_rate_limit_per_hour": 6000
    }))
}

fn jsonrpc_result(id: Value, result: Value) -> Json<Value> {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn mcp_tool(name: &str) -> Value {
    json!({
        "name": name,
        "description": format!("Mock SerpAPI {name}"),
        "inputSchema": {
            "type": "object",
            "properties": { "q": { "type": "string", "description": "search query" } },
            "required": ["q"]
        }
    })
}

/// Additive MCP query knob mirroring /search's `sse_error=1`: error responses
/// wrap in an SSE `data:` frame (M7 requires BOTH run-out flavors be
/// emittable on /mcp). Absent knob + absent env keeps the raw-JSON behavior
/// byte-identical.
#[derive(Deserialize)]
struct McpQuery {
    sse_error: Option<String>,
}

async fn handle_mcp(
    State(state): State<Arc<MockState>>,
    Query(query): Query<McpQuery>,
    RawQuery(raw_query): RawQuery,
    headers: HeaderMap,
    body: Option<String>,
) -> Response {
    state.mcp_requests.fetch_add(1, Ordering::Relaxed);
    state.note_outbound_observation(&headers, raw_query.as_deref());
    let sse = query.sse_error.as_deref() == Some("1") || state.sse_errors;
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    match state.classify(bearer) {
        KeyStatus::Valid => {}
        KeyStatus::Invalid => {
            return error_response(StatusCode::UNAUTHORIZED, invalid_key_body(), sse);
        }
        KeyStatus::Exhausted => {
            return error_response(StatusCode::TOO_MANY_REQUESTS, run_out_body(), sse);
        }
        KeyStatus::Throttled => {
            return error_response(StatusCode::TOO_MANY_REQUESTS, throttled_body(), sse);
        }
    }

    let raw = body.unwrap_or_default();
    let Ok(request) = serde_json::from_str::<Value>(&raw) else {
        return Json(json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": { "code": -32700, "message": "Parse error" }
        }))
        .into_response();
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match method {
        "initialize" => {
            state
                .mcp_initialize_requests
                .fetch_add(1, Ordering::Relaxed);
            (
                StatusCode::OK,
                [(
                    HeaderName::from_static("mcp-session-id"),
                    "mock-serpapi-mcp-session",
                )],
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "mock-serpapi", "version": "0.1.0" }
                    }
                })),
            )
                .into_response()
        }
        "tools/list" => jsonrpc_result(
            id,
            json!({
                "tools": [mcp_tool("search"), mcp_tool("search_table"), mcp_tool("search_dashboard")]
            }),
        )
        .into_response(),
        "tools/call" => {
            state
                .mcp_tools_call_requests
                .fetch_add(1, Ordering::Relaxed);
            let encoded = serde_json::to_string(&search_success_body()).unwrap_or_default();
            jsonrpc_result(id, json!({ "structuredContent": { "result": encoded } })).into_response()
        }
        "ping" => jsonrpc_result(id, json!({})).into_response(),
        notification if notification.starts_with("notifications/") => {
            StatusCode::ACCEPTED.into_response()
        }
        _ => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "Method not found" }
        }))
        .into_response(),
    }
}

/// Additive observability endpoint (plan todo 15): request counters and the
/// last-seen auth/session evidence for passthrough contract assertions.
async fn handle_mock_metrics(State(state): State<Arc<MockState>>) -> Json<Value> {
    Json(json!({
        "mcp_requests": state.mcp_requests.load(Ordering::Relaxed),
        "mcp_initialize_requests": state.mcp_initialize_requests.load(Ordering::Relaxed),
        "mcp_tools_call_requests": state.mcp_tools_call_requests.load(Ordering::Relaxed),
        "tavily_slot_seen": state.tavily_slot_seen.load(Ordering::Relaxed) != 0,
        "last_authorization": state.last_authorization.lock().expect("auth lock").clone(),
        "last_session_header": state
            .last_session_header
            .lock()
            .expect("session lock")
            .clone(),
    }))
}

fn build_router(state: Arc<MockState>) -> Router {
    Router::new()
        .route("/search", get(handle_search))
        .route("/account", get(handle_account))
        .route("/mcp", get(handle_mcp).post(handle_mcp))
        .route("/mock-metrics", get(handle_mock_metrics))
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let sse_errors = std::env::var("MOCK_SERPAPI_SSE_ERRORS").is_ok_and(|value| value == "1");
    let state = Arc::new(MockState::new(cli.keys, cli.plan_renewal_date, sse_errors));
    let app = build_router(state);
    println!("Mock SerpAPI upstream listening on http://{}", cli.bind);
    axum::serve(tokio::net::TcpListener::bind(cli.bind).await?, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> Arc<MockState> {
        Arc::new(MockState::new(
            vec!["mock-serpapi-key".to_string()],
            "2026-10-23".to_string(),
            false,
        ))
    }

    async fn spawn_mock(state: Arc<MockState>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let base = format!("http://{}", listener.local_addr().expect("local addr"));
        tokio::spawn(async move {
            axum::serve(listener, build_router(state))
                .await
                .expect("mock server runs");
        });
        base
    }

    #[tokio::test]
    async fn search_missing_api_key_returns_401_invalid_key_body() {
        // Given a running mock SerpAPI upstream
        let base = spawn_mock(test_state()).await;
        // When /search is called without api_key
        let response = reqwest::get(format!("{base}/search?q=hello"))
            .await
            .expect("request completes");
        // Then it answers 401 with the SerpAPI invalid-key envelope
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, invalid_key_body());
    }

    #[tokio::test]
    async fn search_missing_q_returns_400() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::get(format!("{base}/search?api_key=mock-serpapi-key"))
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, json!({ "error": "Missing query `q` parameter." }));
    }

    #[tokio::test]
    async fn search_exhausted_key_returns_429_run_out() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::get(format!("{base}/search?api_key=key-exhausted&q=hello"))
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, run_out_body());
    }

    #[tokio::test]
    async fn search_throttled_key_returns_429_hourly_limit() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::get(format!("{base}/search?api_key=key-throttled&q=hello"))
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, throttled_body());
    }

    #[tokio::test]
    async fn search_success_returns_organic_results_and_decrements_account() {
        // Given a running mock with a full monthly quota
        let base = spawn_mock(test_state()).await;
        let client = reqwest::Client::new();
        // When a valid search succeeds
        let response = client
            .get(format!("{base}/search"))
            .query(&[("api_key", "mock-serpapi-key"), ("q", "hello")])
            .send()
            .await
            .expect("request completes");
        // Then the organic-results envelope is returned
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["search_metadata"]["status"], json!("Success"));
        assert_eq!(body["organic_results"][0]["title"], json!("Mock Result"));
        assert_eq!(
            body["organic_results"][0]["link"],
            json!("https://example.com/1")
        );
        // And /account reflects one consumed search plus the plan fields
        let account: Value = client
            .get(format!("{base}/account"))
            .send()
            .await
            .expect("request completes")
            .json()
            .await
            .expect("json body");
        assert_eq!(account["account_status"], json!("Active"));
        assert_eq!(account["plan_id"], json!("bigdata"));
        assert_eq!(account["searches_per_month"], json!(MONTHLY_SEARCHES));
        assert_eq!(account["total_searches_left"], json!(MONTHLY_SEARCHES - 1));
        assert_eq!(account["plan_searches_left"], json!(MONTHLY_SEARCHES - 1));
        assert_eq!(account["plan_renewal_date"], json!("2026-10-23"));
        assert_eq!(account["this_hour_searches"], json!(1));
        assert_eq!(account["account_rate_limit_per_hour"], json!(6000));
    }

    #[tokio::test]
    async fn sse_error_switch_wraps_429_in_event_stream_frame_with_same_payload() {
        let base = spawn_mock(test_state()).await;
        // When the exhausted-key 429 is requested with sse_error=1
        let response = reqwest::get(format!(
            "{base}/search?api_key=key-exhausted&q=hello&sse_error=1"
        ))
        .await
        .expect("request completes");
        // Then the envelope is text/event-stream and the data frame decodes to the JSON 429 body
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let text = response.text().await.expect("text body");
        let frame = text
            .strip_prefix("data: ")
            .expect("sse data frame")
            .trim_end();
        let payload: Value = serde_json::from_str(frame).expect("frame decodes to json");
        assert_eq!(payload, run_out_body());
    }

    #[tokio::test]
    async fn mcp_missing_or_invalid_bearer_returns_401() {
        let base = spawn_mock(test_state()).await;
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{base}/mcp"))
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }))
            .send()
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, invalid_key_body());
    }

    #[tokio::test]
    async fn mcp_exhausted_key_returns_429_run_out() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .bearer_auth(KEY_EXHAUSTED)
            .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
            .send()
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body, run_out_body());
    }

    #[tokio::test]
    async fn mcp_initialize_returns_session_id_header() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .bearer_auth("mock-serpapi-key")
            .json(&json!({ "jsonrpc": "2.0", "id": 7, "method": "initialize" }))
            .send()
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
        let session = response.headers()[HeaderName::from_static("mcp-session-id")]
            .to_str()
            .expect("ascii header");
        assert!(!session.is_empty());
        let body: Value = response.json().await.expect("json body");
        assert_eq!(body["jsonrpc"], json!("2.0"));
        assert_eq!(body["id"], json!(7));
        assert_eq!(body["result"]["serverInfo"]["name"], json!("mock-serpapi"));
    }

    #[tokio::test]
    async fn mcp_tools_list_returns_three_search_tools() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .bearer_auth("mock-serpapi-key")
            .json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .send()
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        let tools = body["result"]["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0]["name"], json!("search"));
        assert_eq!(tools[1]["name"], json!("search_table"));
        assert_eq!(tools[2]["name"], json!("search_dashboard"));
        assert_eq!(tools[0]["inputSchema"]["type"], json!("object"));
    }

    #[tokio::test]
    async fn mcp_tools_call_returns_structured_content_json_string() {
        let base = spawn_mock(test_state()).await;
        let response = reqwest::Client::new()
            .post(format!("{base}/mcp"))
            .bearer_auth("mock-serpapi-key")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "search", "arguments": { "q": "hello" } }
            }))
            .send()
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.expect("json body");
        let encoded = body["result"]["structuredContent"]["result"]
            .as_str()
            .expect("result is a json string");
        let inner: Value = serde_json::from_str(encoded).expect("inner json decodes");
        assert_eq!(inner, search_success_body());
    }
}

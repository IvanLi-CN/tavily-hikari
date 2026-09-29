//! Provider hosted-MCP passthrough E2E suite (plan todo 15; amendments M6/M7).
//!
//! SerpApi `/serpapi/mcp` hosted passthrough against mock_serpapi:
//! Bearer key injection (never tavilyApiKey/Tavily-Api-Key), session id
//! translation with `mcp_sessions.provider='serpapi'` stamping, tool billing
//! (search/search_table/search_dashboard = 1 credit each; initialize/ping/
//! tools-list/resources/prompts/notifications non-billable), 401 auth-fail and
//! dual-flavor (raw + SSE-wrapped) 429 run-out exhaustion with native MCP
//! error passthrough, and the M6 provider-scoped session gating that keeps a
//! Tavily control session from suppressing or entangling serpapi sessions.
//!
//! Real server binary + real mock_serpapi binary + direct sqlite assertions
//! (pattern proven by tests/provider_facades_contract.rs and
//! tests/mcp_billing_regression.rs). Every upstream is a stub — production
//! endpoints are never contacted.

use std::{
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router, extract::Request as AxumRequest, http::StatusCode, response::IntoResponse, routing::any,
};
use nanoid::nanoid;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[path = "common/support_binaries.rs"]
mod support_binaries;

// ---------------------------------------------------------------------------
// Shared harness utilities.
// ---------------------------------------------------------------------------

fn temp_db_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.db", nanoid!(8)))
}

fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind random port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

fn sqlite_observability_db_path(db_path: &Path) -> PathBuf {
    let parent = db_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = db_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("sqlite");
    parent.join(format!("{stem}-observability.db"))
}

async fn connect_sqlite_test_pool(db_path: &str) -> sqlx::SqlitePool {
    let observability_db_path = sqlite_observability_db_path(Path::new(db_path));
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .after_connect(move |conn, _meta| {
            let observability_db_path = observability_db_path.clone();
            Box::pin(async move {
                let attach_sql = format!(
                    "ATTACH DATABASE '{}' AS observability",
                    observability_db_path.to_string_lossy().replace('\'', "''")
                );
                sqlx::query(&attach_sql).execute(conn).await?;
                Ok(())
            })
        })
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("connect sqlite pool")
}

fn remove_sqlite_files(db_path: &Path) {
    let _ = std::fs::remove_file(sqlite_observability_db_path(db_path));
    for suffix in ["", "-wal", "-shm"] {
        let mut path = db_path.as_os_str().to_os_string();
        path.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(path));
    }
}

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn health_ready_within(port: u16, timeout_secs: u64) -> bool {
    let client = Client::new();
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if Instant::now() > deadline {
            return false;
        }
        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            && response.status().is_success()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Wait until the startup key sync has stamped the expected number of
/// provider rows (SERPAPI_API_KEYS env bootstrap).
async fn wait_for_provider_keys(db_str: &str, provider: &str, expected: i64) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let pool = connect_sqlite_test_pool(db_str).await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM api_keys WHERE provider = ? AND deleted_at IS NULL",
        )
        .bind(provider)
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
        pool.close().await;
        if count >= expected {
            return;
        }
        if Instant::now() > deadline {
            panic!("provider '{provider}' keys never reached {expected} rows (saw {count})");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn create_test_token(base_url: &str) -> (String, String) {
    let payload = Client::new()
        .post(format!("{base_url}/api/tokens"))
        .json(&json!({}))
        .send()
        .await
        .expect("create token request")
        .error_for_status()
        .expect("create token status")
        .json::<Value>()
        .await
        .expect("decode token payload");
    let token = payload["token"].as_str().expect("token secret").to_string();
    let token_id = token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(id, _)| id.to_string()))
        .expect("token id embedded in secret");
    (token, token_id)
}

// ---------------------------------------------------------------------------
// In-process Tavily upstream mock (M6 entanglement test only).
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct TavilyMockCounters {
    initialize: Arc<AtomicU64>,
    tools_list: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
}

/// Minimal Tavily-shaped MCP upstream: initialize gets a session id header,
/// tools/list answers a canonical result, everything else answers `{}`.
async fn spawn_tavily_mock_upstream() -> (String, TavilyMockCounters) {
    let counters = TavilyMockCounters::default();
    let handler_counters = counters.clone();
    let app = Router::new().route(
        "/mcp",
        any(move |request: AxumRequest| {
            let counters = handler_counters.clone();
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), 4 * 1024 * 1024)
                    .await
                    .unwrap_or_default();
                counters.total.fetch_add(1, Ordering::SeqCst);
                let parsed: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                let id = parsed.get("id").cloned().unwrap_or(Value::Null);
                let method = parsed
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                match method.as_str() {
                    "initialize" => {
                        counters.initialize.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::OK,
                            [("mcp-session-id", "mock-tavily-upstream-session")],
                            axum::Json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "protocolVersion": "2025-03-26",
                                    "capabilities": { "tools": {} },
                                    "serverInfo": { "name": "mock-tavily", "version": "0.1.0" }
                                }
                            })),
                        )
                            .into_response()
                    }
                    "tools/list" => {
                        counters.tools_list.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::OK,
                            [("mcp-session-id", "mock-tavily-upstream-session")],
                            axum::Json(json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "tools": [{ "name": "tavily-search" }] }
                            })),
                        )
                            .into_response()
                    }
                    _ if method.starts_with("notifications/") => {
                        StatusCode::ACCEPTED.into_response()
                    }
                    _ => (
                        StatusCode::OK,
                        axum::Json(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
                    )
                        .into_response(),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind tavily mock");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("tavily mock runs");
    });
    (format!("http://{addr}/mcp"), counters)
}

// ---------------------------------------------------------------------------
// SerpApi MCP harness (real server binary + real mock_serpapi binary).
// ---------------------------------------------------------------------------

struct SerpapiMcpHarness {
    base_url: String,
    mock_base: String,
    token: String,
    token_id: String,
    db_path: PathBuf,
    db_str: String,
    tavily_counters: Option<TavilyMockCounters>,
    _server: ChildGuard,
    _mock: ChildGuard,
}

impl Drop for SerpapiMcpHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

#[derive(Default, Clone, Copy)]
struct HarnessOptions {
    /// MOCK_SERPAPI_SSE_ERRORS=1: /mcp + /search errors wrap in SSE frames.
    sse_errors: bool,
    /// Spawn the in-process Tavily mock and point TAVILY_UPSTREAM at it.
    tavily_mock: bool,
}

async fn spawn_harness(prefix: &str, pool_keys: &[&str], mock_keys: &[&str]) -> SerpapiMcpHarness {
    spawn_harness_with_options(prefix, pool_keys, mock_keys, HarnessOptions::default()).await
}

async fn spawn_harness_with_options(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
    options: HarnessOptions,
) -> SerpapiMcpHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_harness(prefix, pool_keys, mock_keys, options).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    panic!("serpapi mcp harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_harness(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
    options: HarnessOptions,
) -> Option<SerpapiMcpHarness> {
    let mock_port = reserve_local_port();
    let mut mock_cmd = Command::new(support_binaries::resolve_support_binary(
        "MOCK_SERPAPI_TEST_BIN",
        env!("CARGO_BIN_EXE_mock_serpapi"),
    ));
    mock_cmd
        .arg("--bind")
        .arg(format!("127.0.0.1:{mock_port}"))
        .arg("--keys")
        .arg(mock_keys.join(","))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if options.sse_errors {
        mock_cmd.env("MOCK_SERPAPI_SSE_ERRORS", "1");
    }
    let mock = mock_cmd.spawn().expect("spawn mock_serpapi");

    let port = reserve_local_port();
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    // Tavily surfaces must never contact production endpoints: default to a
    // dead local port unless the test needs the in-process Tavily mock.
    let dead_port = reserve_local_port();
    let (tavily_upstream, tavily_usage_base, tavily_counters) = if options.tavily_mock {
        let (base, counters) = spawn_tavily_mock_upstream().await;
        let usage_base = base.trim_end_matches("/mcp").to_string();
        (base, usage_base, Some(counters))
    } else {
        (
            format!("http://127.0.0.1:{dead_port}/mcp"),
            format!("http://127.0.0.1:{dead_port}"),
            None,
        )
    };
    let server = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ))
    .env("TAVILY_API_KEYS", "tvly-mcp-e2e-key")
    .env("TAVILY_UPSTREAM", &tavily_upstream)
    .env("TAVILY_USAGE_BASE", &tavily_usage_base)
    .env("SERPAPI_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
    // Path suffix under the mock host: exercises the ingress-aware upstream
    // URL builder (SERPAPI_MCP_UPSTREAM honored with a /mcp path).
    .env(
        "SERPAPI_MCP_UPSTREAM",
        format!("http://127.0.0.1:{mock_port}/mcp"),
    )
    .env("SERPAPI_API_KEYS", pool_keys.join(","))
    .env("PROXY_BIND", "127.0.0.1")
    .env("PROXY_PORT", port.to_string())
    .env("PROXY_DB_PATH", &db_str)
    .env("DEV_OPEN_ADMIN", "true")
    .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn tavily-hikari");

    if !health_ready_within(port, 25).await {
        drop(ChildGuard { child: server });
        drop(ChildGuard { child: mock });
        remove_sqlite_files(&db_path);
        return None;
    }
    wait_for_provider_keys(&db_str, "serpapi", i64::try_from(pool_keys.len()).unwrap()).await;
    let base_url = format!("http://127.0.0.1:{port}");
    let (token, token_id) = create_test_token(&base_url).await;

    Some(SerpapiMcpHarness {
        base_url,
        mock_base: format!("http://127.0.0.1:{mock_port}"),
        token,
        token_id,
        db_path,
        db_str,
        tavily_counters,
        _server: ChildGuard { child: server },
        _mock: ChildGuard { child: mock },
    })
}

/// One raw MCP-over-HTTP response as the client sees it.
struct McpCallResult {
    status: StatusCode,
    session_header: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl McpCallResult {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

impl SerpapiMcpHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }

    /// POST one JSON-RPC message to `/serpapi/mcp` with the Hikari Bearer
    /// token and (optionally) the proxy mcp-session-id header.
    async fn serpapi_mcp_call(&self, session_id: Option<&str>, message: &Value) -> McpCallResult {
        self.mcp_call("/serpapi/mcp", session_id, message).await
    }

    async fn mcp_call(
        &self,
        path: &str,
        session_id: Option<&str>,
        message: &Value,
    ) -> McpCallResult {
        let mut request = Client::new()
            .post(format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json");
        if let Some(session_id) = session_id {
            request = request.header("mcp-session-id", session_id);
        }
        let response = request.json(message).send().await.expect("mcp request");
        McpCallResult {
            status: response.status(),
            session_header: response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            content_type: response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: response.bytes().await.expect("response body").to_vec(),
        }
    }

    fn initialize_message(id: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "provider-mcp-e2e", "version": "0.1.0" }
            }
        })
    }

    fn tools_call_message(id: u64, tool: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": { "q": "hello" } }
        })
    }

    /// Run initialize and return the proxy session id the gateway handed back.
    async fn initialize_session(&self, id: u64) -> String {
        let response = self
            .serpapi_mcp_call(None, &Self::initialize_message(id))
            .await;
        assert_eq!(
            response.status,
            StatusCode::OK,
            "initialize must pass through: {}",
            response.body_text()
        );
        let session = response
            .session_header
            .clone()
            .expect("initialize must return the translated proxy mcp-session-id");
        assert_ne!(
            session, "mock-serpapi-mcp-session",
            "client must never see the raw upstream session id"
        );
        // notifications/initialized completes the handshake (non-billable).
        let notified = self
            .serpapi_mcp_call(
                Some(&session),
                &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            )
            .await;
        assert_eq!(notified.status, StatusCode::ACCEPTED, "notification 202");
        session
    }

    /// Snapshot of the mock's additive metrics endpoint.
    async fn mock_metrics(&self) -> Value {
        Client::new()
            .get(format!("{}/mock-metrics", self.mock_base))
            .send()
            .await
            .expect("mock metrics request")
            .json::<Value>()
            .await
            .expect("mock metrics json")
    }

    async fn mcp_session_row(&self, proxy_session_id: &str) -> sqlx::sqlite::SqliteRow {
        let pool = self.pool().await;
        let row = sqlx::query(
            "SELECT proxy_session_id, upstream_session_id, upstream_key_id, auth_token_id, \
             gateway_mode, experiment_variant, provider \
             FROM mcp_sessions WHERE proxy_session_id = ? LIMIT 1",
        )
        .bind(proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("mcp_sessions row for proxy session id");
        pool.close().await;
        row
    }

    async fn charged_ledger_rows(&self) -> Vec<(String, i64, Option<i64>)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT billing_state, business_credits, request_log_id FROM billing_ledger \
             WHERE token_id = ? ORDER BY auth_token_log_id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("ledger rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("billing_state"),
                    row.get::<i64, _>("business_credits"),
                    row.get::<Option<i64>, _>("request_log_id"),
                )
            })
            .collect()
    }

    async fn token_log_rows(&self) -> Vec<(Option<String>, Option<i64>, Option<String>)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT request_kind_key, business_credits, provider FROM auth_token_logs \
             WHERE token_id = ? ORDER BY id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("token log rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<Option<String>, _>("request_kind_key"),
                    row.get::<Option<i64>, _>("business_credits"),
                    row.get::<Option<String>, _>("provider"),
                )
            })
            .collect()
    }
}

fn mock_tools_list_body(id: u64) -> Value {
    fn tool(name: &str) -> Value {
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
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [tool("search"), tool("search_table"), tool("search_dashboard")]
        }
    })
}

fn mock_run_out_body() -> Value {
    json!({ "error": "Your account has run out of searches." })
}

fn mock_invalid_key_body() -> Value {
    json!({
        "error": "Invalid API key. Your API key should be here: https://serpapi.com/manage-api-key"
    })
}

// ---------------------------------------------------------------------------
// Acceptance tests (plan L210, each named criterion).
// ---------------------------------------------------------------------------

/// Criterion: initialize goes upstream, returns a TRANSLATED proxy session id,
/// persists an mcp_sessions row with provider='serpapi' + upstream identity +
/// gateway_mode='upstream_mcp'; the mock never sees a Tavily auth slot and
/// the pooled key rides `Authorization: Bearer` (mock echo assertion).
#[tokio::test]
async fn initialize_translates_session_and_tags_provider_row() {
    let harness = spawn_harness("mcp-e2e-init", &["sp-mcp-pool-1"], &["sp-mcp-pool-1"]).await;
    let session = harness.initialize_session(1).await;

    let row = harness.mcp_session_row(&session).await;
    assert_eq!(row.get::<String, _>("proxy_session_id"), session);
    assert_eq!(
        row.get::<Option<String>, _>("provider").as_deref(),
        Some("serpapi"),
        "mcp_sessions row must be provider-stamped"
    );
    assert_eq!(
        row.get::<Option<String>, _>("upstream_session_id")
            .as_deref(),
        Some("mock-serpapi-mcp-session"),
        "upstream session id translated and persisted"
    );
    assert_eq!(
        row.get::<String, _>("gateway_mode"),
        "upstream_mcp",
        "additive gateway mode value recorded"
    );
    assert_eq!(row.get::<String, _>("experiment_variant"), "control");
    assert_eq!(
        row.get::<Option<String>, _>("auth_token_id").as_deref(),
        Some(harness.token_id.as_str())
    );
    // The pinned upstream key is the pooled SERPAPI key row (provider-scoped).
    let upstream_key_id = row
        .get::<Option<String>, _>("upstream_key_id")
        .expect("pinned upstream key id");
    let pool = harness.pool().await;
    let key_provider: String = sqlx::query_scalar("SELECT provider FROM api_keys WHERE id = ?")
        .bind(&upstream_key_id)
        .fetch_one(&pool)
        .await
        .expect("pinned key row");
    assert_eq!(key_provider, "serpapi");

    // request_logs stamping: provider + gateway_mode + additive operation.
    let log = sqlx::query(
        "SELECT provider, gateway_mode, upstream_operation, proxy_session_id, path, \
         request_kind_key, forwarded_headers \
         FROM observability.request_logs WHERE path = '/serpapi/mcp' ORDER BY id ASC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(
        log.get::<Option<String>, _>("provider").as_deref(),
        Some("serpapi")
    );
    assert_eq!(
        log.get::<Option<String>, _>("gateway_mode").as_deref(),
        Some("upstream_mcp")
    );
    assert_eq!(
        log.get::<Option<String>, _>("upstream_operation")
            .as_deref(),
        Some("serpapi_mcp"),
        "additive provider-scoped upstream_operation value"
    );
    assert_eq!(
        log.get::<Option<String>, _>("proxy_session_id").as_deref(),
        Some(session.as_str())
    );
    assert_eq!(
        log.get::<Option<String>, _>("request_kind_key").as_deref(),
        Some("mcp:initialize")
    );
    pool.close().await;

    // Mock-side proof: Bearer carries the pooled secret; NO Tavily auth slot
    // (Tavily-Api-Key header or tavilyApiKey query) was ever observed; the
    // mock saw the upstream protocol session lifecycle exactly once.
    let metrics = harness.mock_metrics().await;
    assert_eq!(
        metrics["tavily_slot_seen"],
        json!(false),
        "mock must never see tavilyApiKey/Tavily-Api-Key on serpapi outbound"
    );
    assert_eq!(
        metrics["last_authorization"],
        json!("Bearer sp-mcp-pool-1"),
        "pooled key rides Authorization: Bearer"
    );
    assert_eq!(metrics["mcp_initialize_requests"], json!(1));
}

/// Criterion: tools/list passthrough is NON-BILLABLE — mock catalog bytes
/// reach the client verbatim and no billing_ledger row exists.
#[tokio::test]
async fn tools_list_passthrough_is_non_billable() {
    let harness = spawn_harness("mcp-e2e-toolslist", &["sp-mcp-pool-2"], &["sp-mcp-pool-2"]).await;
    let session = harness.initialize_session(1).await;

    let response = harness
        .serpapi_mcp_call(
            Some(&session),
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.json(),
        mock_tools_list_body(2),
        "mock tools catalog passes through unchanged"
    );

    let charged = harness.charged_ledger_rows().await;
    assert!(
        charged.is_empty(),
        "tools/list must not create ledger rows: {charged:?}"
    );
    let logs = harness.token_log_rows().await;
    assert!(
        logs.iter()
            .any(|(kind, _, _)| kind.as_deref() == Some("mcp:tools/list")),
        "tools/list attempt row recorded with its kind: {logs:?}"
    );
    assert!(
        logs.iter().all(|(_, credits, _)| credits.unwrap_or(0) == 0),
        "no business credits on non-billable rows: {logs:?}"
    );
}

/// Criterion: tools/call search bills exactly 1 credit with ledger row and
/// kind mcp:serpapi/search; the upstream result body passes through.
#[tokio::test]
async fn tools_call_search_bills_one_credit_with_serpapi_kind() {
    let harness = spawn_harness("mcp-e2e-bill", &["sp-mcp-pool-3"], &["sp-mcp-pool-3"]).await;
    let session = harness.initialize_session(1).await;

    let response = harness
        .serpapi_mcp_call(
            Some(&session),
            &SerpapiMcpHarness::tools_call_message(3, "search"),
        )
        .await;
    assert_eq!(response.status, StatusCode::OK);
    let body = response.json();
    assert_eq!(body["jsonrpc"], json!("2.0"));
    assert_eq!(body["id"], json!(3));
    assert!(body["result"]["structuredContent"]["result"].is_string());

    let charged = harness.charged_ledger_rows().await;
    assert_eq!(charged.len(), 1, "exactly one ledger row: {charged:?}");
    assert_eq!(charged[0].0, "charged");
    assert_eq!(charged[0].1, 1, "1 credit for one successful search");

    let pool = harness.pool().await;
    let (kind, credits): (Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT request_kind_key, business_credits FROM auth_token_logs \
         WHERE token_id = ? AND business_credits > 0 ORDER BY id DESC LIMIT 1",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("billed token log row");
    assert_eq!(kind.as_deref(), Some("mcp:serpapi/search"));
    assert_eq!(credits, Some(1));

    let log_kind: Option<String> = sqlx::query_scalar(
        "SELECT request_kind_key FROM observability.request_logs \
         WHERE path = '/serpapi/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(log_kind.as_deref(), Some("mcp:serpapi/search"));

    let month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1), 0)",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("token quota row");
    assert_eq!(month_count, 1);
    pool.close().await;
}

/// Criterion: follow-ups carrying the proxy session reuse the pinned upstream
/// session — the mock sees the SAME upstream session id and no re-initialize.
#[tokio::test]
async fn follow_up_with_proxy_session_reuses_upstream_session() {
    let harness = spawn_harness("mcp-e2e-followup", &["sp-mcp-pool-4"], &["sp-mcp-pool-4"]).await;
    let session = harness.initialize_session(1).await;

    for id in 2..=4u64 {
        let response = harness
            .serpapi_mcp_call(
                Some(&session),
                &json!({ "jsonrpc": "2.0", "id": id, "method": "tools/list" }),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "follow-up #{id}");
        assert_eq!(response.json(), mock_tools_list_body(id));
        assert_eq!(
            response.session_header.as_deref(),
            Some(session.as_str()),
            "translated proxy session id echoes back, never the upstream id"
        );
    }

    let metrics = harness.mock_metrics().await;
    assert_eq!(
        metrics["mcp_initialize_requests"],
        json!(1),
        "no re-initialize on follow-ups"
    );
    assert_eq!(
        metrics["last_session_header"],
        json!("mock-serpapi-mcp-session"),
        "mock sees the pinned upstream session id on every follow-up"
    );
    assert!(
        metrics["mcp_requests"].as_u64().expect("counter") >= 5,
        "all lifecycle requests reached the mock: {metrics}"
    );
}

/// Criterion: billing matrix — search_table and search_dashboard each bill
/// 1 credit under their own mcp:serpapi/* kinds (never mcp:search, never
/// mcp:third-party-tool; amendment M7 collision guard).
#[tokio::test]
async fn billing_matrix_covers_all_three_tools_one_credit_each() {
    let harness = spawn_harness("mcp-e2e-matrix", &["sp-mcp-pool-5"], &["sp-mcp-pool-5"]).await;
    let session = harness.initialize_session(1).await;

    for (id, tool) in [
        (2u64, "search"),
        (3, "search_table"),
        (4, "search_dashboard"),
    ] {
        let response = harness
            .serpapi_mcp_call(
                Some(&session),
                &SerpapiMcpHarness::tools_call_message(id, tool),
            )
            .await;
        assert_eq!(response.status, StatusCode::OK, "{tool} call");
        assert_eq!(response.json()["id"], json!(id));
    }

    let pool = harness.pool().await;
    let rows = sqlx::query(
        "SELECT request_kind_key, business_credits FROM auth_token_logs \
         WHERE token_id = ? AND business_credits > 0 ORDER BY id ASC",
    )
    .bind(&harness.token_id)
    .fetch_all(&pool)
    .await
    .expect("billed rows");
    let billed: Vec<(Option<String>, Option<i64>)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<Option<String>, _>("request_kind_key"),
                row.get::<Option<i64>, _>("business_credits"),
            )
        })
        .collect();
    assert_eq!(
        billed,
        vec![
            (Some("mcp:serpapi/search".to_string()), Some(1)),
            (Some("mcp:serpapi/search_table".to_string()), Some(1)),
            (Some("mcp:serpapi/search_dashboard".to_string()), Some(1)),
        ],
        "each SerpApi tool bills 1 credit under its own serpapi-prefixed kind"
    );

    let charged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("ledger scan");
    assert_eq!(charged, 3);
    let credits_total: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1), 0)",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("quota row");
    assert_eq!(credits_total, 3);
    pool.close().await;
}

/// Criterion: non-billable methods (initialize/ping/tools/list/resources/*)
/// assert ZERO ledger rows — only the successful search family bills.
#[tokio::test]
async fn non_billable_methods_record_zero_ledger_rows() {
    let harness = spawn_harness(
        "mcp-e2e-nonbillable",
        &["sp-mcp-pool-6"],
        &["sp-mcp-pool-6"],
    )
    .await;
    let session = harness.initialize_session(1).await;

    let messages = [
        json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
        json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/list" }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ];
    for message in &messages {
        let response = harness.serpapi_mcp_call(Some(&session), message).await;
        assert!(
            response.status.is_success() || response.status == StatusCode::ACCEPTED,
            "non-billable passthrough status: {} {}",
            response.status,
            response.body_text()
        );
    }

    let charged = harness.charged_ledger_rows().await;
    assert!(
        charged.is_empty(),
        "initialize/ping/tools-list/resources/prompts/notifications bill nothing: {charged:?}"
    );
    let logs = harness.token_log_rows().await;
    assert!(
        logs.iter().all(|(_, credits, _)| credits.unwrap_or(0) == 0),
        "no business credits recorded: {logs:?}"
    );
    let kinds: Vec<&str> = logs
        .iter()
        .filter_map(|(kind, _, _)| kind.as_deref())
        .collect();
    assert!(kinds.contains(&"mcp:ping"), "{kinds:?}");
    assert!(kinds.contains(&"mcp:resources/list"), "{kinds:?}");
    assert!(
        kinds.contains(&"mcp:notifications/initialized"),
        "{kinds:?}"
    );
}

/// Criterion: invalid pooled key → upstream 401 passes through natively and
/// the auth-fail health action quarantines the key (existing machinery).
#[tokio::test]
async fn invalid_key_401_applies_auth_fail_and_passes_native_error() {
    let harness = spawn_harness(
        "mcp-e2e-401",
        &["sp-invalid-at-mock"],
        &["sp-some-other-key"],
    )
    .await;

    let response = harness
        .serpapi_mcp_call(None, &SerpapiMcpHarness::initialize_message(1))
        .await;
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);
    assert_eq!(response.json(), mock_invalid_key_body(), "native 401 body");

    let pool = harness.pool().await;
    let log = sqlx::query(
        "SELECT result_status, failure_kind, key_effect_code, status_code, provider, \
         request_kind_key FROM observability.request_logs \
         WHERE path = '/serpapi/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(log.get::<String, _>("result_status"), "error");
    assert_eq!(
        log.get::<Option<String>, _>("failure_kind").as_deref(),
        Some("upstream_account_deactivated_401")
    );
    assert_eq!(log.get::<String, _>("key_effect_code"), "quarantined");
    assert_eq!(
        log.get::<Option<String>, _>("provider").as_deref(),
        Some("serpapi")
    );

    let quarantined: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines q \
         JOIN api_keys k ON k.id = q.key_id \
         WHERE k.api_key = 'sp-invalid-at-mock' AND q.cleared_at IS NULL \
         AND q.reason_code = 'invalid_api_key'",
    )
    .fetch_one(&pool)
    .await
    .expect("quarantine scan");
    assert_eq!(quarantined, 1, "401 must quarantine the pooled key");

    // No proxy session row was persisted for the failed initialize.
    let sessions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mcp_sessions WHERE provider = 'serpapi'")
            .fetch_one(&pool)
            .await
            .expect("session scan");
    assert_eq!(sessions, 0);

    let charged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("ledger scan");
    assert_eq!(charged, 0);
    pool.close().await;
}

/// Criterion: exhausted key with RAW-HTTP 429 run-out → key marked exhausted
/// in the DB and the native MCP error body passes through to the client.
#[tokio::test]
async fn exhausted_key_raw_429_marks_key_and_passes_native_error() {
    let harness = spawn_harness("mcp-e2e-429raw", &["key-exhausted"], &[]).await;

    let response = harness
        .serpapi_mcp_call(None, &SerpapiMcpHarness::initialize_message(1))
        .await;
    assert_eq!(response.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.json(), mock_run_out_body(), "native run-out body");

    let pool = harness.pool().await;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM api_keys WHERE provider = 'serpapi' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("key row");
    assert_eq!(status, "exhausted", "run-out 429 marks the pooled key");

    let log = sqlx::query(
        "SELECT result_status, status_code, provider FROM observability.request_logs \
         WHERE path = '/serpapi/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(log.get::<String, _>("result_status"), "quota_exhausted");
    assert_eq!(log.get::<i64, _>("status_code"), 429);
    pool.close().await;
}

/// Criterion (M7): SSE-envelope-wrapped 429 run-out exhausts the key exactly
/// like the raw flavor and the SSE frame passes through to the client.
#[tokio::test]
async fn exhausted_key_sse_wrapped_429_marks_key_and_passes_sse_frame() {
    let harness = spawn_harness_with_options(
        "mcp-e2e-429sse",
        &["key-exhausted"],
        &[],
        HarnessOptions {
            sse_errors: true,
            tavily_mock: false,
        },
    )
    .await;

    let response = harness
        .serpapi_mcp_call(None, &SerpapiMcpHarness::initialize_message(1))
        .await;
    assert_eq!(response.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        response.content_type.as_deref(),
        Some("text/event-stream"),
        "SSE envelope passes through"
    );
    let text = response.body_text();
    let frame = text
        .strip_prefix("data: ")
        .expect("sse data frame")
        .trim_end();
    let payload: Value = serde_json::from_str(frame).expect("frame decodes");
    assert_eq!(payload, mock_run_out_body());

    let pool = harness.pool().await;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM api_keys WHERE provider = 'serpapi' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("key row");
    assert_eq!(
        status, "exhausted",
        "SSE-wrapped run-out 429 exhausts through the envelope parser"
    );
    pool.close().await;
}

/// Criterion (M6): session-mode gating is PROVIDER-SCOPED — a Tavily control
/// session never suppresses/entangles serpapi passthrough sessions and a
/// serpapi session never trips the Tavily requires-session gate; the
/// rebalance_mcp_enabled switch stays OFF the whole time and does NOT gate
/// the serpapi passthrough; proxy session ids never cross provider surfaces.
#[tokio::test]
async fn tavily_control_session_never_suppresses_or_entangles_serpapi() {
    let harness = spawn_harness_with_options(
        "mcp-e2e-m6",
        &["sp-mcp-pool-m6"],
        &["sp-mcp-pool-m6"],
        HarnessOptions {
            sse_errors: false,
            tavily_mock: true,
        },
    )
    .await;
    let tavily_counters = harness.tavily_counters.clone().expect("tavily mock");

    // A serpapi session exists while rebalance_mcp_enabled stays false.
    let serpapi_session = harness.initialize_session(1).await;

    // Tavily /mcp follow-up WITHOUT a session header must NOT be rejected by
    // the requires-session gate just because the token has a serpapi session
    // (pre-provider-scoping this returned 400 session_required).
    let tavily_tools = harness
        .mcp_call(
            "/mcp",
            None,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        )
        .await;
    assert_eq!(
        tavily_tools.status,
        StatusCode::OK,
        "serpapi session must not trip the Tavily requires-session gate: {}",
        tavily_tools.body_text()
    );
    assert_eq!(
        tavily_counters
            .tools_list
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the request reached the Tavily upstream"
    );

    // Now create a Tavily control session on the SAME token.
    let tavily_init = harness
        .mcp_call("/mcp", None, &SerpapiMcpHarness::initialize_message(10))
        .await;
    assert_eq!(tavily_init.status, StatusCode::OK);
    let tavily_session = tavily_init
        .session_header
        .clone()
        .expect("tavily control session id");
    assert_ne!(tavily_session, serpapi_session);

    // The Tavily control session is provider-untagged (byte-stable default).
    let pool = harness.pool().await;
    let tavily_provider: Option<String> =
        sqlx::query_scalar("SELECT provider FROM mcp_sessions WHERE proxy_session_id = ?")
            .bind(&tavily_session)
            .fetch_one(&pool)
            .await
            .expect("tavily session row");
    assert!(
        tavily_provider.is_none() || tavily_provider.as_deref() == Some("tavily"),
        "Tavily rows keep the byte-stable provider default: {tavily_provider:?}"
    );
    pool.close().await;

    // An active Tavily control session does not suppress the serpapi surface:
    // serpapi follow-ups keep working on the pinned upstream session.
    let serpapi_follow_up = harness
        .serpapi_mcp_call(
            Some(&serpapi_session),
            &json!({ "jsonrpc": "2.0", "id": 11, "method": "tools/list" }),
        )
        .await;
    assert_eq!(serpapi_follow_up.status, StatusCode::OK);
    assert_eq!(serpapi_follow_up.json(), mock_tools_list_body(11));

    // Proxy session ids never cross provider surfaces.
    let crossed_to_tavily = harness
        .mcp_call(
            "/mcp",
            Some(&serpapi_session),
            &json!({ "jsonrpc": "2.0", "id": 12, "method": "ping" }),
        )
        .await;
    assert_eq!(
        crossed_to_tavily.status,
        StatusCode::NOT_FOUND,
        "serpapi proxy session must not resolve on the Tavily surface"
    );
    let crossed_to_serpapi = harness
        .serpapi_mcp_call(
            Some(&tavily_session),
            &json!({ "jsonrpc": "2.0", "id": 13, "method": "ping" }),
        )
        .await;
    assert_eq!(
        crossed_to_serpapi.status,
        StatusCode::NOT_FOUND,
        "tavily proxy session must not resolve on the serpapi surface"
    );
}

// ===========================================================================
// Brave LOCAL MCP facade (`/brave/mcp`) — plan todo 16.
//
// The local facade answers initialize/tools-list/ping/notifications LOCALLY
// (session-less, SSE-wrapped, server_name `brave-search-hikari`, EXACTLY the 3
// v2.1.4-mirror tools) and translates tools/call into a pooled-key brave REST
// GET, returning the native body as the MCP tool-result content. Billing runs
// the reserve→settle path (web=1, news=1, llm_context=2); provider errors
// surface an isError result carrying the NATIVE body and emit the todo-3 health
// signals (402→exhausted, 422 token→quarantine, 429→backoff). Forbidden tools
// and pool exhaustion are typed MCP errors with ZERO upstream calls.
//
// Real server binary + real mock_brave binary + direct sqlite assertions (the
// proven provider_facades_contract / serpapi-section pattern). Every upstream
// is a stub — production endpoints are never contacted.
// ===========================================================================

/// Extract the single JSON-RPC payload from an `event: message\ndata: {..}`
/// SSE frame (the local facade's session-less transport, rebalance-shaped).
fn parse_sse_json(body: &[u8]) -> Value {
    let text = String::from_utf8_lossy(body);
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .unwrap_or_else(|| panic!("expected an SSE data frame, got: {text:?}"));
    serde_json::from_str(data.trim()).unwrap_or_else(|err| panic!("sse data decodes: {err}"))
}

struct BraveMcpHarness {
    base_url: String,
    mock_base: String,
    token: String,
    token_id: String,
    db_path: PathBuf,
    db_str: String,
    _server: ChildGuard,
    _mock: ChildGuard,
}

impl Drop for BraveMcpHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

async fn spawn_brave_harness(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
) -> BraveMcpHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_brave_harness(prefix, pool_keys, mock_keys).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    panic!("brave mcp harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_brave_harness(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
) -> Option<BraveMcpHarness> {
    let mock_port = reserve_local_port();
    let mock = Command::new(support_binaries::resolve_support_binary(
        "MOCK_BRAVE_TEST_BIN",
        env!("CARGO_BIN_EXE_mock_brave"),
    ))
    .arg("--bind")
    .arg(format!("127.0.0.1:{mock_port}"))
    .arg("--keys")
    .arg(mock_keys.join(","))
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn mock_brave");

    let port = reserve_local_port();
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    // Every non-Brave provider upstream points at a dead local port so the
    // production endpoints are unreachable by construction (incident rule).
    let dead_port = reserve_local_port();
    let server = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ))
    .env("TAVILY_API_KEYS", "tvly-mcp-e2e-key")
    .env(
        "TAVILY_UPSTREAM",
        format!("http://127.0.0.1:{dead_port}/mcp"),
    )
    .env("TAVILY_USAGE_BASE", format!("http://127.0.0.1:{dead_port}"))
    .env("BRAVE_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
    .env("BRAVE_API_KEYS", pool_keys.join(","))
    .env(
        "SERPAPI_UPSTREAM",
        format!("http://127.0.0.1:{dead_port}"),
    )
    .env(
        "SERPAPI_MCP_UPSTREAM",
        format!("http://127.0.0.1:{dead_port}/mcp"),
    )
    .env("EXA_UPSTREAM", format!("http://127.0.0.1:{dead_port}"))
    .env("PROXY_BIND", "127.0.0.1")
    .env("PROXY_PORT", port.to_string())
    .env("PROXY_DB_PATH", &db_str)
    .env("DEV_OPEN_ADMIN", "true")
    .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn tavily-hikari");

    if !health_ready_within(port, 25).await {
        drop(ChildGuard { child: server });
        drop(ChildGuard { child: mock });
        remove_sqlite_files(&db_path);
        return None;
    }
    wait_for_provider_keys(&db_str, "brave", i64::try_from(pool_keys.len()).unwrap()).await;
    let base_url = format!("http://127.0.0.1:{port}");
    let (token, token_id) = create_test_token(&base_url).await;

    Some(BraveMcpHarness {
        base_url,
        mock_base: format!("http://127.0.0.1:{mock_port}"),
        token,
        token_id,
        db_path,
        db_str,
        _server: ChildGuard { child: server },
        _mock: ChildGuard { child: mock },
    })
}

impl BraveMcpHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }

    /// POST one JSON-RPC message to `/brave/mcp` with the Hikari Bearer token.
    async fn brave_mcp_call(&self, message: &Value) -> McpCallResult {
        let response = Client::new()
            .post(format!("{}/brave/mcp", self.base_url))
            .bearer_auth(&self.token)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .json(message)
            .send()
            .await
            .expect("brave mcp request");
        McpCallResult {
            status: response.status(),
            session_header: response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            content_type: response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: response.bytes().await.expect("response body").to_vec(),
        }
    }

    /// POST + decode the SSE-wrapped JSON-RPC payload in one step.
    async fn brave_mcp_json(&self, message: &Value) -> Value {
        let result = self.brave_mcp_call(message).await;
        assert_eq!(
            result.status,
            StatusCode::OK,
            "local facade answers tools/call + methods at HTTP 200: {}",
            result.body_text()
        );
        parse_sse_json(&result.body)
    }

    fn initialize_message(id: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "provider-mcp-e2e", "version": "0.1.0" }
            }
        })
    }

    fn tools_call(id: u64, tool: &str, arguments: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        })
    }

    /// POST initialize (session-less local response — no mcp-session-id).
    async fn initialize(&self, id: u64) -> Value {
        let result = self.brave_mcp_call(&Self::initialize_message(id)).await;
        assert_eq!(result.status, StatusCode::OK, "initialize 200");
        assert!(
            result.session_header.is_none(),
            "session-less local facade must not mint an mcp-session-id: {:?}",
            result.session_header
        );
        parse_sse_json(&result.body)
    }

    async fn mock_metrics(&self) -> Value {
        Client::new()
            .get(format!("{}/metrics", self.mock_base))
            .send()
            .await
            .expect("mock metrics request")
            .json::<Value>()
            .await
            .expect("mock metrics json")
    }

    /// (billing_state, business_credits) for every ledger row on the token.
    async fn charged_ledger_rows(&self) -> Vec<(String, i64)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT billing_state, business_credits FROM billing_ledger \
             WHERE token_id = ? ORDER BY auth_token_log_id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("ledger rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("billing_state"),
                    row.get::<i64, _>("business_credits"),
                )
            })
            .collect()
    }

    /// (request_kind_key, business_credits, provider) per auth_token_logs row.
    async fn token_log_rows(&self) -> Vec<(Option<String>, Option<i64>, Option<String>)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT request_kind_key, business_credits, provider FROM auth_token_logs \
             WHERE token_id = ? ORDER BY id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("token log rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<Option<String>, _>("request_kind_key"),
                    row.get::<Option<i64>, _>("business_credits"),
                    row.get::<Option<String>, _>("provider"),
                )
            })
            .collect()
    }
}

fn brave_tool_names(catalog: &Value) -> Vec<String> {
    catalog["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Criterion (plan L217): initialize then tools/list shows EXACTLY the three
/// v2.1.4-mirror tools, each with an object inputSchema; non-billable (zero
/// ledger) and zero upstream calls.
#[tokio::test]
async fn brave_initialize_and_tools_list_advertise_exactly_three_tools() {
    let harness = spawn_brave_harness("mcp-e2e-brave-catalog", &["mock-brave-key"], &["mock-brave-key"]).await;

    let init = harness.initialize(1).await;
    assert_eq!(init["jsonrpc"], json!("2.0"));
    assert_eq!(init["id"], json!(1));
    assert_eq!(
        init["result"]["serverInfo"]["name"],
        json!("brave-search-hikari"),
        "server name is the brave facade identity"
    );
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let listed = harness
        .brave_mcp_json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    let mut names = brave_tool_names(&listed);
    names.sort();
    assert_eq!(
        names,
        vec![
            "brave_llm_context".to_string(),
            "brave_news_search".to_string(),
            "brave_web_search".to_string(),
        ],
        "tools/list advertises EXACTLY the three brave tools (no summarizer/place/images/videos/suggest/spellcheck)"
    );
    for tool in listed["result"]["tools"].as_array().expect("tools") {
        assert_eq!(
            tool["inputSchema"]["type"],
            json!("object"),
            "each tool carries an object inputSchema"
        );
        assert!(
            tool["inputSchema"]["properties"]["query"].is_object(),
            "each brave tool exposes a query property"
        );
        assert_eq!(
            tool["inputSchema"]["required"],
            json!(["query"]),
            "query is the minimal required-field contract guard"
        );
    }

    // initialize + tools/list are non-billable and never touch brave upstream.
    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "control-plane methods create zero ledger rows"
    );
    let metrics = harness.mock_metrics().await;
    assert_eq!(
        metrics,
        json!({ "web": 0, "news": 0, "llm_context": 0 }),
        "initialize/tools-list answered locally, zero upstream calls"
    );
}

/// Criterion (plan L217): tools/call brave_web_search bills EXACTLY 1 credit,
/// returns the mock native body as MCP content, records provider='brave' +
/// kind mcp:brave/web_search, and hits web/search once.
#[tokio::test]
async fn brave_web_search_bills_one_credit_and_returns_native_body() {
    let harness = spawn_brave_harness("mcp-e2e-brave-web", &["mock-brave-key"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_web_search",
            json!({ "query": "hikari test", "count": 5 }),
        ))
        .await;
    assert_eq!(result["id"], json!(2));
    assert!(
        result["result"]["isError"].is_null() || result["result"]["isError"] == json!(false),
        "successful web search is not an error: {result}"
    );
    // Native mock body as text content + structuredContent projection.
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let parsed: Value = serde_json::from_str(content_text).expect("native body text is json");
    assert_eq!(parsed["type"], json!("search"));
    assert_eq!(parsed["web"]["results"][0]["title"], json!("Mock"));
    assert_eq!(
        result["result"]["structuredContent"]["web"]["results"][0]["url"],
        json!("https://example.com/1"),
        "structuredContent mirrors the native body"
    );

    let charged = harness.charged_ledger_rows().await;
    assert_eq!(charged.len(), 1, "exactly one ledger row: {charged:?}");
    assert_eq!(charged[0].0, "charged");
    assert_eq!(charged[0].1, 1, "brave_web_search bills 1 credit");

    let pool = harness.pool().await;
    let log = sqlx::query(
        "SELECT provider, request_kind_key, tavily_status_code, result_status \
         FROM observability.request_logs WHERE path = '/brave/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(
        log.get::<Option<String>, _>("provider").as_deref(),
        Some("brave")
    );
    assert_eq!(
        log.get::<Option<String>, _>("request_kind_key").as_deref(),
        Some("mcp:brave/web_search"),
        "request_logs kind is the committed T3 mcp:brave/web_search"
    );
    assert_eq!(log.get::<String, _>("result_status"), "success");
    pool.close().await;

    let metrics = harness.mock_metrics().await;
    assert_eq!(metrics["web"], json!(1), "exactly one upstream web/search call");
}

/// Criterion (plan L217): brave_llm_context bills 2 credits (llm_context tier)
/// and reaches the llm/context endpoint once.
#[tokio::test]
async fn brave_llm_context_bills_two_credits() {
    let harness = spawn_brave_harness("mcp-e2e-brave-llm", &["mock-brave-key"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_llm_context",
            json!({ "query": "context test", "count": 3 }),
        ))
        .await;
    assert_eq!(result["id"], json!(2));
    assert!(
        result["result"]["structuredContent"]["grounding"].is_object(),
        "native grounding body projected into structuredContent: {result}"
    );

    let charged = harness.charged_ledger_rows().await;
    assert_eq!(charged.len(), 1, "one ledger row: {charged:?}");
    assert_eq!(charged[0].1, 2, "brave_llm_context bills 2 credits");

    let pool = harness.pool().await;
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT request_kind_key FROM observability.request_logs \
         WHERE path = '/brave/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("log row");
    assert_eq!(kind.as_deref(), Some("mcp:brave/llm_context"));
    pool.close().await;

    assert_eq!(harness.mock_metrics().await["llm_context"], json!(1));
}

/// Criterion: brave_news_search bills 1 credit and reaches news/search once.
#[tokio::test]
async fn brave_news_search_bills_one_credit() {
    let harness = spawn_brave_harness("mcp-e2e-brave-news", &["mock-brave-key"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_news_search",
            json!({ "query": "news test" }),
        ))
        .await;
    assert_eq!(
        result["result"]["structuredContent"]["type"],
        json!("news"),
        "native news body projected: {result}"
    );
    let charged = harness.charged_ledger_rows().await;
    assert_eq!(charged.len(), 1);
    assert_eq!(charged[0].1, 1, "brave_news_search bills 1 credit");
    assert_eq!(harness.mock_metrics().await["news"], json!(1));
}

/// Criterion (plan L217): 402 QUOTA_LIMITED surfaces an isError MCP result
/// carrying the NATIVE error body, marks the pooled key exhausted, and bills 0.
#[tokio::test]
async fn brave_402_quota_limited_is_error_marks_exhausted_and_bills_zero() {
    let harness = spawn_brave_harness("mcp-e2e-brave-402", &["key-exhausted"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_web_search",
            json!({ "query": "quota probe" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true), "402 → isError true");
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    assert!(
        content_text.contains("QUOTA_LIMITED"),
        "native error body carried in content: {content_text}"
    );
    assert_eq!(
        result["result"]["structuredContent"]["error"]["code"],
        json!("QUOTA_LIMITED"),
        "structuredContent carries the native error body"
    );

    let pool = harness.pool().await;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM api_keys WHERE provider = 'brave' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("key row");
    assert_eq!(status, "exhausted", "402 QUOTA_LIMITED marks the pooled key exhausted");
    let log = sqlx::query(
        "SELECT provider, result_status, request_kind_key FROM observability.request_logs \
         WHERE path = '/brave/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("log row");
    assert_eq!(log.get::<String, _>("result_status"), "quota_exhausted");
    assert_eq!(
        log.get::<Option<String>, _>("provider").as_deref(),
        Some("brave")
    );
    pool.close().await;

    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "provider error bills nothing"
    );
}

/// Criterion (plan L217): 422 SUBSCRIPTION_TOKEN_INVALID surfaces an isError
/// result and quarantines the pooled key through the existing machinery.
#[tokio::test]
async fn brave_422_invalid_token_is_error_and_quarantines() {
    // Pooled key is NOT in the mock's valid key list → 422 SUBSCRIPTION_TOKEN_INVALID.
    let harness =
        spawn_brave_harness("mcp-e2e-brave-422", &["bogus-at-mock"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_web_search",
            json!({ "query": "auth probe" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true), "422 → isError true");
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.contains("SUBSCRIPTION_TOKEN_INVALID"),
        "native 422 body carried: {content_text}"
    );

    let pool = harness.pool().await;
    let quarantined: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines q \
         JOIN api_keys k ON k.id = q.key_id \
         WHERE k.api_key = 'bogus-at-mock' AND q.cleared_at IS NULL \
         AND q.reason_code = 'invalid_api_key'",
    )
    .fetch_one(&pool)
    .await
    .expect("quarantine scan");
    assert_eq!(
        quarantined, 1,
        "422 SUBSCRIPTION_TOKEN_INVALID quarantines the pooled key"
    );
    pool.close().await;

    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (plan L217): 429 RATE_LIMITED surfaces an isError result and arms
/// the shared second-scale transient backoff (key stays active), bills 0.
#[tokio::test]
async fn brave_429_rate_limited_is_error_and_arms_backoff() {
    let harness = spawn_brave_harness("mcp-e2e-brave-429", &["key-throttled"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_web_search",
            json!({ "query": "throttle probe" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true), "429 → isError true");
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.contains("RATE_LIMITED"),
        "native 429 body carried: {content_text}"
    );

    let pool = harness.pool().await;
    let status: String =
        sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-throttled'")
            .fetch_one(&pool)
            .await
            .expect("key row");
    assert_eq!(status, "active", "429 RATE_LIMITED must NOT exhaust the key");
    let backoff: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_transient_backoffs b \
         JOIN api_keys k ON k.id = b.key_id \
         WHERE k.api_key = 'key-throttled' AND b.scope = 'http_global' \
         AND b.reason_code = 'upstream_rate_limited_429'",
    )
    .fetch_one(&pool)
    .await
    .expect("backoff scan");
    assert_eq!(backoff, 1, "429 arms the shared HTTP transient backoff");
    pool.close().await;

    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (plan L217 / gate 1): non-billable methods (initialize, ping,
/// tools/list, resources/prompts lists, notifications) record ZERO ledger rows
/// and ZERO upstream calls.
#[tokio::test]
async fn brave_non_billable_methods_record_zero_ledger_and_zero_upstream() {
    let harness =
        spawn_brave_harness("mcp-e2e-brave-nonbillable", &["mock-brave-key"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    for message in [
        json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
        json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/list" }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ] {
        let result = harness.brave_mcp_call(&message).await;
        assert!(
            result.status == StatusCode::OK || result.status == StatusCode::ACCEPTED,
            "non-billable method status {}: {}",
            result.status,
            result.body_text()
        );
    }

    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "control-plane methods bill nothing"
    );
    let logs = harness.token_log_rows().await;
    assert!(
        logs.iter().all(|(_, credits, _)| credits.unwrap_or(0) == 0),
        "no business credits recorded: {logs:?}"
    );
    assert_eq!(
        harness.mock_metrics().await,
        json!({ "web": 0, "news": 0, "llm_context": 0 }),
        "control-plane answered locally, zero upstream calls"
    );
}

/// Criterion (gate 1): an unknown tool is a TYPED MCP error result with ZERO
/// upstream calls and ZERO billing.
#[tokio::test]
async fn brave_unknown_tool_is_typed_error_without_upstream_call() {
    let harness =
        spawn_brave_harness("mcp-e2e-brave-unknown", &["mock-brave-key"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_summarizer",
            json!({ "query": "should never dispatch" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true));
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.contains("Unknown tool"),
        "typed unknown-tool error: {content_text}"
    );

    assert_eq!(
        harness.mock_metrics().await,
        json!({ "web": 0, "news": 0, "llm_context": 0 }),
        "unknown tool never reaches brave upstream"
    );
    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (M22 minimal contract guard): a tools/call missing the required
/// `query` is a local invalid-arguments error — no upstream call, no billing.
#[tokio::test]
async fn brave_missing_required_query_is_local_invalid_arguments() {
    let harness =
        spawn_brave_harness("mcp-e2e-brave-noargs", &["mock-brave-key"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_web_search",
            json!({ "count": 5 }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true));
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.contains("query"),
        "invalid-arguments message names the missing required field: {content_text}"
    );
    assert_eq!(
        harness.mock_metrics().await,
        json!({ "web": 0, "news": 0, "llm_context": 0 }),
        "local validation rejects before any upstream call"
    );
    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (plan L218 QA failure): once the whole brave pool is quarantined
/// (the only key auth-fails 422 → excluded at every acquire tier), a further
/// tools/call returns an isError NoAvailableKeys-shaped MCP result with ZERO
/// additional upstream calls (mock counter unchanged) and ZERO billing.
/// (Exhaustion alone is NOT this state: acquire re-leases a lone exhausted key
/// as a fallback, whereas quarantine is excluded at every tier — the faithful
/// "pool unavailable" trigger.)
#[tokio::test]
async fn brave_exhausted_pool_returns_no_available_keys_without_upstream_call() {
    // Pooled key is invalid at the mock → 422 SUBSCRIPTION_TOKEN_INVALID → the
    // T3 classifier auth-fails it into quarantine.
    let harness =
        spawn_brave_harness("mcp-e2e-brave-nopool", &["bogus-at-mock"], &["mock-brave-key"]).await;
    harness.initialize(1).await;

    // First call hits upstream once (422) and quarantines the only pooled key.
    let first = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            2,
            "brave_web_search",
            json!({ "query": "quarantine trigger" }),
        ))
        .await;
    assert_eq!(first["result"]["isError"], json!(true), "422 → isError");
    let counter_after_first = harness.mock_metrics().await;
    assert_eq!(counter_after_first["web"], json!(1), "one upstream 422 call");
    let pool = harness.pool().await;
    let quarantined: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines q \
         JOIN api_keys k ON k.id = q.key_id \
         WHERE k.api_key = 'bogus-at-mock' AND q.cleared_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("quarantine scan");
    assert_eq!(quarantined, 1, "the pooled key is quarantined");
    pool.close().await;

    // Second call: the quarantined pool is unavailable → NoAvailableKeys-shaped
    // local MCP error, ZERO additional upstream calls (counter unchanged).
    let second = harness
        .brave_mcp_json(&BraveMcpHarness::tools_call(
            3,
            "brave_web_search",
            json!({ "query": "should not dispatch" }),
        ))
        .await;
    assert_eq!(
        second["result"]["isError"],
        json!(true),
        "unavailable pool is an isError result"
    );
    let content_text = second["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.to_ascii_lowercase().contains("available"),
        "NoAvailableKeys-shaped message: {content_text}"
    );
    let counter_after_second = harness.mock_metrics().await;
    assert_eq!(
        counter_after_second["web"], counter_after_first["web"],
        "quarantined pool → zero additional upstream calls (counter unchanged)"
    );
}

// ===========================================================================
// Exa LOCAL MCP facade (`/exa/mcp`) — plan todo 17.
//
// The local facade answers initialize/tools-list/ping/notifications LOCALLY
// (session-less, SSE-wrapped, server_name `exa-search-hikari`, EXACTLY the 3
// exa-labs/exa-mcp-server-mirror tools) and translates tools/call into a
// pooled-key exa REST POST (x-api-key injection), returning the native body as
// the MCP tool-result content. Billing runs the reserve→settle path with the
// registry credits (search=1, contents=1, answer=2); provider errors surface an
// isError result carrying the NATIVE body and emit the todo-3 health signals
// (402 budget tags→exhausted, 401→quarantine, 429→Retry-After backoff).
// web_fetch_exa enforces the M19 urls-XOR-ids contract guard LOCALLY (typed
// invalid-params, ZERO upstream calls); exa_answer never carries `stream` on
// the wire (T13 semantics — a truthy smuggled stream rejects locally).
// Deprecated shims (crawling_exa, company_research_exa, deep_researcher_*)
// and agent_run are NOT in the catalog — unknown-tool typed errors.
//
// Real server binary + real mock_exa binary + direct sqlite assertions (the
// proven brave-section pattern). Every upstream is a stub — production
// endpoints are never contacted.
// ===========================================================================

struct ExaMcpHarness {
    base_url: String,
    mock_base: String,
    token: String,
    token_id: String,
    db_path: PathBuf,
    db_str: String,
    _server: ChildGuard,
    _mock: ChildGuard,
}

impl Drop for ExaMcpHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

async fn spawn_exa_harness(prefix: &str, pool_keys: &[&str], mock_keys: &[&str]) -> ExaMcpHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_exa_harness(prefix, pool_keys, mock_keys).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    panic!("exa mcp harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_exa_harness(
    prefix: &str,
    pool_keys: &[&str],
    mock_keys: &[&str],
) -> Option<ExaMcpHarness> {
    let mock_port = reserve_local_port();
    let mock = Command::new(support_binaries::resolve_support_binary(
        "MOCK_EXA_TEST_BIN",
        env!("CARGO_BIN_EXE_mock_exa"),
    ))
    .arg("--bind")
    .arg(format!("127.0.0.1:{mock_port}"))
    .arg("--keys")
    .arg(mock_keys.join(","))
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn mock_exa");

    let port = reserve_local_port();
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    // Every non-Exa provider upstream points at a dead local port so the
    // production endpoints are unreachable by construction (incident rule).
    let dead_port = reserve_local_port();
    let server = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ))
    .env("TAVILY_API_KEYS", "tvly-mcp-e2e-key")
    .env(
        "TAVILY_UPSTREAM",
        format!("http://127.0.0.1:{dead_port}/mcp"),
    )
    .env("TAVILY_USAGE_BASE", format!("http://127.0.0.1:{dead_port}"))
    .env("EXA_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
    .env("EXA_API_KEYS", pool_keys.join(","))
    .env(
        "SERPAPI_UPSTREAM",
        format!("http://127.0.0.1:{dead_port}"),
    )
    .env(
        "SERPAPI_MCP_UPSTREAM",
        format!("http://127.0.0.1:{dead_port}/mcp"),
    )
    .env("BRAVE_UPSTREAM", format!("http://127.0.0.1:{dead_port}"))
    .env("PROXY_BIND", "127.0.0.1")
    .env("PROXY_PORT", port.to_string())
    .env("PROXY_DB_PATH", &db_str)
    .env("DEV_OPEN_ADMIN", "true")
    .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn tavily-hikari");

    if !health_ready_within(port, 25).await {
        drop(ChildGuard { child: server });
        drop(ChildGuard { child: mock });
        remove_sqlite_files(&db_path);
        return None;
    }
    wait_for_provider_keys(&db_str, "exa", i64::try_from(pool_keys.len()).unwrap()).await;
    let base_url = format!("http://127.0.0.1:{port}");
    let (token, token_id) = create_test_token(&base_url).await;

    Some(ExaMcpHarness {
        base_url,
        mock_base: format!("http://127.0.0.1:{mock_port}"),
        token,
        token_id,
        db_path,
        db_str,
        _server: ChildGuard { child: server },
        _mock: ChildGuard { child: mock },
    })
}

impl ExaMcpHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }

    /// POST one JSON-RPC message to `/exa/mcp` with the Hikari Bearer token.
    async fn exa_mcp_call(&self, message: &Value) -> McpCallResult {
        let response = Client::new()
            .post(format!("{}/exa/mcp", self.base_url))
            .bearer_auth(&self.token)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .json(message)
            .send()
            .await
            .expect("exa mcp request");
        McpCallResult {
            status: response.status(),
            session_header: response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            content_type: response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
            body: response.bytes().await.expect("response body").to_vec(),
        }
    }

    /// POST + decode the SSE-wrapped JSON-RPC payload in one step.
    async fn exa_mcp_json(&self, message: &Value) -> Value {
        let result = self.exa_mcp_call(message).await;
        assert_eq!(
            result.status,
            StatusCode::OK,
            "local facade answers tools/call + methods at HTTP 200: {}",
            result.body_text()
        );
        parse_sse_json(&result.body)
    }

    fn initialize_message(id: u64) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "provider-mcp-e2e", "version": "0.1.0" }
            }
        })
    }

    fn tools_call(id: u64, tool: &str, arguments: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        })
    }

    /// POST initialize (session-less local response — no mcp-session-id).
    async fn initialize(&self, id: u64) -> Value {
        let result = self.exa_mcp_call(&Self::initialize_message(id)).await;
        assert_eq!(result.status, StatusCode::OK, "initialize 200");
        assert!(
            result.session_header.is_none(),
            "session-less local facade must not mint an mcp-session-id: {:?}",
            result.session_header
        );
        parse_sse_json(&result.body)
    }

    async fn mock_metrics(&self) -> Value {
        Client::new()
            .get(format!("{}/metrics", self.mock_base))
            .send()
            .await
            .expect("mock metrics request")
            .json::<Value>()
            .await
            .expect("mock metrics json")
    }

    /// Last request body the mock received per endpoint (todo-17 additive
    /// knob): proves the MCP→REST translation wire body.
    async fn mock_last_bodies(&self) -> Value {
        Client::new()
            .get(format!("{}/last-bodies", self.mock_base))
            .send()
            .await
            .expect("mock last-bodies request")
            .json::<Value>()
            .await
            .expect("mock last-bodies json")
    }

    /// (billing_state, business_credits) for every ledger row on the token.
    async fn charged_ledger_rows(&self) -> Vec<(String, i64)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT billing_state, business_credits FROM billing_ledger \
             WHERE token_id = ? ORDER BY auth_token_log_id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("ledger rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("billing_state"),
                    row.get::<i64, _>("business_credits"),
                )
            })
            .collect()
    }

    /// (request_kind_key, business_credits, provider) per auth_token_logs row.
    async fn token_log_rows(&self) -> Vec<(Option<String>, Option<i64>, Option<String>)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT request_kind_key, business_credits, provider FROM auth_token_logs \
             WHERE token_id = ? ORDER BY id ASC",
        )
        .bind(&self.token_id)
        .fetch_all(&pool)
        .await
        .expect("token log rows");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<Option<String>, _>("request_kind_key"),
                    row.get::<Option<i64>, _>("business_credits"),
                    row.get::<Option<String>, _>("provider"),
                )
            })
            .collect()
    }
}

fn exa_tool_names(catalog: &Value) -> Vec<String> {
    catalog["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn exa_tool_by_name<'a>(catalog: &'a Value, name: &str) -> &'a Value {
    catalog["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .find(|tool| tool["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("tool {name} missing from the catalog"))
}

/// Criterion (plan L224): initialize then tools/list shows EXACTLY the three
/// exa-labs/exa-mcp-server-mirror tools with their schemas (no deprecated
/// shims, no agent_run, no `stream` property anywhere); non-billable (zero
/// ledger) and zero upstream calls.
#[tokio::test]
async fn exa_initialize_and_tools_list_advertise_exactly_three_tools() {
    let harness = spawn_exa_harness("mcp-e2e-exa-catalog", &["mock-exa-key"], &["mock-exa-key"]).await;

    let init = harness.initialize(1).await;
    assert_eq!(init["jsonrpc"], json!("2.0"));
    assert_eq!(init["id"], json!(1));
    assert_eq!(
        init["result"]["serverInfo"]["name"],
        json!("exa-search-hikari"),
        "server name is the exa facade identity"
    );
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let listed = harness
        .exa_mcp_json(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await;
    let mut names = exa_tool_names(&listed);
    names.sort();
    assert_eq!(
        names,
        vec![
            "exa_answer".to_string(),
            "web_fetch_exa".to_string(),
            "web_search_exa".to_string(),
        ],
        "tools/list advertises EXACTLY the three exa tools (no crawling_exa/company_research_exa/deep_researcher_*/agent_run shims)"
    );
    for tool in listed["result"]["tools"].as_array().expect("tools") {
        assert_eq!(
            tool["inputSchema"]["type"],
            json!("object"),
            "each tool carries an object inputSchema"
        );
        for property in tool["inputSchema"]["properties"]
            .as_object()
            .expect("properties object")
            .keys()
        {
            assert_ne!(
                property, "stream",
                "the facade NEVER advertises a stream parameter (no SSE)"
            );
        }
    }

    // Schema-detail pins (M19 registry-mirror defaults).
    let search = exa_tool_by_name(&listed, "web_search_exa");
    assert_eq!(search["inputSchema"]["required"], json!(["query"]));
    assert_eq!(search["inputSchema"]["properties"]["numResults"]["minimum"], json!(1));
    assert_eq!(search["inputSchema"]["properties"]["numResults"]["maximum"], json!(100));
    for property in [
        "type",
        "category",
        "includeDomains",
        "excludeDomains",
        "startPublishedDate",
        "endPublishedDate",
        "contents",
    ] {
        assert!(
            search["inputSchema"]["properties"][property].is_object(),
            "web_search_exa advertises the registry `{property}` parameter"
        );
    }
    let fetch = exa_tool_by_name(&listed, "web_fetch_exa");
    assert_eq!(fetch["inputSchema"]["properties"]["urls"]["type"], json!("array"));
    assert_eq!(fetch["inputSchema"]["properties"]["ids"]["type"], json!("array"));
    assert_eq!(
        fetch["inputSchema"]["properties"]["urls"]["maxItems"],
        json!(100),
        "urls advertises the 1-100 bound"
    );
    let answer = exa_tool_by_name(&listed, "exa_answer");
    assert_eq!(answer["inputSchema"]["required"], json!(["query"]));
    assert_eq!(
        answer["inputSchema"]["properties"]["model"]["enum"],
        json!(["exa", "exa-pro", "exa-research", "exa-fast"]),
        "exa_answer advertises the model enum"
    );

    // initialize + tools/list are non-billable and never touch exa upstream.
    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "control-plane methods create zero ledger rows"
    );
    let metrics = harness.mock_metrics().await;
    assert_eq!(
        metrics,
        json!({ "search": 0, "contents": 0, "answer": 0 }),
        "initialize/tools-list answered locally, zero upstream calls"
    );
}

/// Criterion (plan L224): tools/call web_search_exa bills EXACTLY 1 credit,
/// returns the mock native body as MCP content, translates onto POST /search
/// (mock sees the REST body), and records provider='exa' + mcp:exa/web_search.
#[tokio::test]
async fn exa_web_search_bills_one_credit_and_returns_native_body() {
    let harness = spawn_exa_harness("mcp-e2e-exa-search", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            2,
            "web_search_exa",
            json!({ "query": "hikari exa test", "numResults": 5 }),
        ))
        .await;
    assert_eq!(result["id"], json!(2));
    assert!(
        result["result"]["isError"].is_null() || result["result"]["isError"] == json!(false),
        "successful web search is not an error: {result}"
    );
    // Native mock body as text content + structuredContent projection.
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let parsed: Value = serde_json::from_str(content_text).expect("native body text is json");
    assert_eq!(parsed["results"][0]["url"], json!("https://example.com/1"));
    assert_eq!(
        result["result"]["structuredContent"]["results"][0]["title"],
        json!("Mock"),
        "structuredContent mirrors the native body"
    );
    assert_eq!(
        result["result"]["structuredContent"]["status"],
        json!(200),
        "upstream status projected into structuredContent"
    );

    // The mock received the translated REST body on POST /search.
    let bodies = harness.mock_last_bodies().await;
    assert_eq!(
        bodies["search"],
        json!({ "query": "hikari exa test", "numResults": 5 }),
        "arguments translate verbatim onto the exa REST /search body"
    );

    let charged = harness.charged_ledger_rows().await;
    assert_eq!(charged.len(), 1, "exactly one ledger row: {charged:?}");
    assert_eq!(charged[0].0, "charged");
    assert_eq!(charged[0].1, 1, "web_search_exa bills 1 credit");

    let pool = harness.pool().await;
    let (kind, credits): (Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT request_kind_key, business_credits FROM auth_token_logs \
         WHERE token_id = ? AND business_credits > 0 ORDER BY id DESC LIMIT 1",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("billed token log row");
    assert_eq!(kind.as_deref(), Some("mcp:exa/web_search"));
    assert_eq!(credits, Some(1));

    let log = sqlx::query(
        "SELECT provider, request_kind_key, result_status FROM observability.request_logs \
         WHERE path = '/exa/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("request log row");
    assert_eq!(
        log.get::<Option<String>, _>("provider").as_deref(),
        Some("exa")
    );
    assert_eq!(
        log.get::<Option<String>, _>("request_kind_key").as_deref(),
        Some("mcp:exa/web_search"),
        "request_logs kind is the committed T3 mcp:exa/web_search"
    );
    assert_eq!(log.get::<String, _>("result_status"), "success");

    let month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1), 0)",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("quota row");
    assert_eq!(month_count, 1);
    pool.close().await;

    assert_eq!(
        harness.mock_metrics().await["search"],
        json!(1),
        "exactly one upstream /search call"
    );
}

/// Criterion (plan L224 + gate 1): web_fetch_exa maps urls→POST /contents (the
/// mock asserts the received body), the ids variant works identically, and
/// each call bills 1 credit under mcp:exa/web_fetch.
#[tokio::test]
async fn exa_web_fetch_maps_urls_and_ids_onto_contents() {
    let harness = spawn_exa_harness("mcp-e2e-exa-fetch", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    let urls_result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            2,
            "web_fetch_exa",
            json!({ "urls": ["https://example.com/1"] }),
        ))
        .await;
    assert_eq!(urls_result["id"], json!(2));
    assert!(
        urls_result["result"]["isError"].is_null()
            || urls_result["result"]["isError"] == json!(false),
        "urls variant succeeds: {urls_result}"
    );
    assert_eq!(
        urls_result["result"]["structuredContent"]["statuses"][0]["id"],
        json!("https://example.com/1"),
        "native /contents body projected"
    );
    let bodies = harness.mock_last_bodies().await;
    assert_eq!(
        bodies["contents"],
        json!({ "urls": ["https://example.com/1"] }),
        "urls variant lands on the REST /contents body verbatim"
    );

    let ids_result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            3,
            "web_fetch_exa",
            json!({ "ids": ["mock-content-id"] }),
        ))
        .await;
    assert_eq!(ids_result["id"], json!(3));
    assert!(
        ids_result["result"]["isError"].is_null() || ids_result["result"]["isError"] == json!(false),
        "ids variant succeeds: {ids_result}"
    );
    let bodies = harness.mock_last_bodies().await;
    assert_eq!(
        bodies["contents"],
        json!({ "ids": ["mock-content-id"] }),
        "ids variant lands on the REST /contents body verbatim"
    );

    let charged = harness.charged_ledger_rows().await;
    assert_eq!(
        charged,
        vec![("charged".to_string(), 1), ("charged".to_string(), 1)],
        "each successful web_fetch_exa bills 1 credit: {charged:?}"
    );
    let logs = harness.token_log_rows().await;
    let fetch_kinds: Vec<&str> = logs
        .iter()
        .filter(|(_, credits, _)| credits.unwrap_or(0) > 0)
        .map(|(kind, _, _)| kind.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(
        fetch_kinds,
        vec!["mcp:exa/web_fetch", "mcp:exa/web_fetch"],
        "both variants bill under the committed T3 kind"
    );

    let metrics = harness.mock_metrics().await;
    assert_eq!(metrics["contents"], json!(2), "two upstream /contents calls");
    assert_eq!(metrics["search"], json!(0));
    assert_eq!(metrics["answer"], json!(0));
}

/// Criterion (M19 XOR guard + gate 1): supplying BOTH urls and ids, or
/// NEITHER, is a typed local invalid-params MCP error with ZERO upstream calls
/// and ZERO billing.
#[tokio::test]
async fn exa_web_fetch_xor_violations_are_local_invalid_params() {
    let harness = spawn_exa_harness("mcp-e2e-exa-xor", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    for (id, arguments) in [
        // Both present.
        (
            2u64,
            json!({ "urls": ["https://example.com/1"], "ids": ["mock-id"] }),
        ),
        // Neither present.
        (3u64, json!({})),
        // Both present but empty (an empty array supplies no documents).
        (4u64, json!({ "urls": [], "ids": [] })),
    ] {
        let result = harness
            .exa_mcp_json(&ExaMcpHarness::tools_call(
                id,
                "web_fetch_exa",
                arguments.clone(),
            ))
            .await;
        assert_eq!(result["id"], json!(id));
        assert_eq!(
            result["result"]["isError"],
            json!(true),
            "XOR violation {arguments} is a typed isError result"
        );
        let content_text = result["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(
            content_text.contains("exactly one") && content_text.contains("urls"),
            "invalid-params message names the XOR contract: {content_text}"
        );
    }

    assert_eq!(
        harness.mock_metrics().await,
        json!({ "search": 0, "contents": 0, "answer": 0 }),
        "XOR violations never reach exa upstream"
    );
    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "local invalid-params bills nothing"
    );
}

/// Criterion (plan L224 + gate 1): exa_answer bills EXACTLY 2 credits, hits
/// POST /answer once, and the wire body NEVER carries `stream`.
#[tokio::test]
async fn exa_answer_bills_two_credits_and_never_streams() {
    let harness = spawn_exa_harness("mcp-e2e-exa-answer", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            2,
            "exa_answer",
            json!({ "query": "what changed?", "model": "exa-pro" }),
        ))
        .await;
    assert_eq!(result["id"], json!(2));
    assert!(
        result["result"]["isError"].is_null() || result["result"]["isError"] == json!(false),
        "successful answer is not an error: {result}"
    );
    assert_eq!(
        result["result"]["structuredContent"]["answer"],
        json!("mock answer"),
        "native /answer body projected"
    );

    // Stream-never proof: the REST body the mock received has no stream key.
    let bodies = harness.mock_last_bodies().await;
    assert_eq!(bodies["answer"]["query"], json!("what changed?"));
    assert_eq!(bodies["answer"]["model"], json!("exa-pro"));
    assert!(
        bodies["answer"].get("stream").is_none(),
        "stream is OMITTED from the exa REST /answer body: {}",
        bodies["answer"]
    );

    let charged = harness.charged_ledger_rows().await;
    assert_eq!(charged.len(), 1, "exactly one ledger row: {charged:?}");
    assert_eq!(charged[0].1, 2, "exa_answer bills 2 credits");

    let pool = harness.pool().await;
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT request_kind_key FROM observability.request_logs \
         WHERE path = '/exa/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("log row");
    assert_eq!(kind.as_deref(), Some("mcp:exa/answer"));
    let month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1), 0)",
    )
    .bind(&harness.token_id)
    .fetch_one(&pool)
    .await
    .expect("quota row");
    assert_eq!(month_count, 2);
    pool.close().await;

    let metrics = harness.mock_metrics().await;
    assert_eq!(metrics["answer"], json!(1), "exactly one upstream /answer call");
}

/// Criterion (T13 stream-gate reuse): a truthy `stream` smuggled through tool
/// arguments is a LOCAL typed rejection (the documented boundary message) with
/// ZERO upstream calls and ZERO billing.
#[tokio::test]
async fn exa_truthy_stream_arguments_reject_locally_without_upstream() {
    let harness = spawn_exa_harness("mcp-e2e-exa-stream", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    for (id, tool, arguments) in [
        (2u64, "exa_answer", json!({ "query": "q", "stream": true })),
        (3u64, "web_search_exa", json!({ "query": "q", "stream": "always" })),
    ] {
        let result = harness
            .exa_mcp_json(&ExaMcpHarness::tools_call(id, tool, arguments))
            .await;
        assert_eq!(result["id"], json!(id));
        assert_eq!(
            result["result"]["isError"],
            json!(true),
            "{tool} truthy stream must reject locally: {result}"
        );
        let content_text = result["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(
            content_text.contains("streaming not supported"),
            "rejection carries the documented T13 streaming boundary message: {content_text}"
        );
    }

    assert_eq!(
        harness.mock_metrics().await,
        json!({ "search": 0, "contents": 0, "answer": 0 }),
        "stream rejections never reach exa upstream"
    );
    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (plan L224 + gate 1): 402 NO_MORE_CREDITS surfaces an isError MCP
/// result carrying the NATIVE error body (tag included), marks the pooled key
/// exhausted in the DB, records result_status='quota_exhausted', and bills 0.
#[tokio::test]
async fn exa_402_no_more_credits_is_error_marks_exhausted_and_bills_zero() {
    let harness = spawn_exa_harness("mcp-e2e-exa-402", &["key-exhausted"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            2,
            "web_search_exa",
            json!({ "query": "quota probe" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true), "402 → isError true");
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    assert!(
        content_text.contains("NO_MORE_CREDITS"),
        "native error body carried in content: {content_text}"
    );
    assert_eq!(
        result["result"]["structuredContent"]["tag"],
        json!("NO_MORE_CREDITS"),
        "structuredContent carries the native error body"
    );

    let pool = harness.pool().await;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM api_keys WHERE provider = 'exa' AND deleted_at IS NULL LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("key row");
    assert_eq!(
        status, "exhausted",
        "402 NO_MORE_CREDITS marks the pooled key exhausted"
    );
    let log = sqlx::query(
        "SELECT provider, result_status, request_kind_key FROM observability.request_logs \
         WHERE path = '/exa/mcp' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("log row");
    assert_eq!(log.get::<String, _>("result_status"), "quota_exhausted");
    assert_eq!(
        log.get::<Option<String>, _>("provider").as_deref(),
        Some("exa")
    );
    pool.close().await;

    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "provider error bills nothing"
    );
    assert_eq!(
        harness.mock_metrics().await["search"],
        json!(1),
        "the 402 came from one real upstream call"
    );
}

/// Criterion (gate 1): 401 INVALID_API_KEY surfaces an isError result and
/// quarantines the pooled key through the existing auth-fail machinery; bills 0.
#[tokio::test]
async fn exa_401_invalid_key_is_error_and_quarantines() {
    // Pooled key is NOT in the mock's valid key list → 401 INVALID_API_KEY.
    let harness =
        spawn_exa_harness("mcp-e2e-exa-401", &["ex-invalid-at-mock"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            2,
            "web_search_exa",
            json!({ "query": "auth probe" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true), "401 → isError true");
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.contains("INVALID_API_KEY"),
        "native 401 body carried: {content_text}"
    );

    let pool = harness.pool().await;
    let quarantined: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines q \
         JOIN api_keys k ON k.id = q.key_id \
         WHERE k.api_key = 'ex-invalid-at-mock' AND q.cleared_at IS NULL \
         AND q.reason_code = 'invalid_api_key'",
    )
    .fetch_one(&pool)
    .await
    .expect("quarantine scan");
    assert_eq!(
        quarantined, 1,
        "401 INVALID_API_KEY quarantines the pooled key"
    );
    pool.close().await;

    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (M12 + gate 1): 429 RATE_LIMITED surfaces an isError result, the
/// key STAYS active, and the Retry-After-sized shared HTTP backoff is armed
/// (record path reused from the T13 REST facade); bills 0.
#[tokio::test]
async fn exa_429_rate_limited_is_error_and_arms_backoff() {
    let harness = spawn_exa_harness("mcp-e2e-exa-429", &["key-throttled"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    let result = harness
        .exa_mcp_json(&ExaMcpHarness::tools_call(
            2,
            "web_search_exa",
            json!({ "query": "throttle probe" }),
        ))
        .await;
    assert_eq!(result["result"]["isError"], json!(true), "429 → isError true");
    let content_text = result["result"]["content"][0]["text"]
        .as_str()
        .expect("text");
    assert!(
        content_text.contains("RATE_LIMIT_EXCEEDED"),
        "native 429 body carried: {content_text}"
    );

    let pool = harness.pool().await;
    let status: String =
        sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = 'key-throttled'")
            .fetch_one(&pool)
            .await
            .expect("key row");
    assert_eq!(status, "active", "429 must NOT exhaust the exa key");
    let backoff: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_transient_backoffs b \
         JOIN api_keys k ON k.id = b.key_id \
         WHERE k.api_key = 'key-throttled' AND b.scope = 'http_global' \
         AND b.reason_code = 'upstream_rate_limited_429'",
    )
    .fetch_one(&pool)
    .await
    .expect("backoff scan");
    assert_eq!(
        backoff, 1,
        "429 arms the shared HTTP transient backoff (Retry-After honored)"
    );
    pool.close().await;

    assert!(harness.charged_ledger_rows().await.is_empty());
}

/// Criterion (gate 1): non-billable methods (initialize, ping, tools/list,
/// resources/prompts lists, notifications) record ZERO ledger rows and ZERO
/// upstream calls.
#[tokio::test]
async fn exa_non_billable_methods_record_zero_ledger_and_zero_upstream() {
    let harness =
        spawn_exa_harness("mcp-e2e-exa-nonbillable", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    for message in [
        json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list" }),
        json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/list" }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    ] {
        let result = harness.exa_mcp_call(&message).await;
        assert!(
            result.status == StatusCode::OK || result.status == StatusCode::ACCEPTED,
            "non-billable method status {}: {}",
            result.status,
            result.body_text()
        );
    }

    assert!(
        harness.charged_ledger_rows().await.is_empty(),
        "control-plane methods bill nothing"
    );
    let logs = harness.token_log_rows().await;
    assert!(
        logs.iter().all(|(_, credits, _)| credits.unwrap_or(0) == 0),
        "no business credits recorded: {logs:?}"
    );
    assert!(
        logs.iter()
            .all(|(_, _, provider)| provider.as_deref().is_none_or(|value| value == "exa")),
        "token-log rows stay provider-stamped: {logs:?}"
    );
    assert_eq!(
        harness.mock_metrics().await,
        json!({ "search": 0, "contents": 0, "answer": 0 }),
        "control-plane answered locally, zero upstream calls"
    );
}

/// Criterion (gate 1 + MUST NOT list): an unknown tool — including every
/// deprecated shim name — is a TYPED MCP error with ZERO upstream calls and
/// ZERO billing.
#[tokio::test]
async fn exa_unknown_tool_and_deprecated_shims_are_typed_errors() {
    let harness =
        spawn_exa_harness("mcp-e2e-exa-unknown", &["mock-exa-key"], &["mock-exa-key"]).await;
    harness.initialize(1).await;

    for (id, tool) in [
        (2u64, "crawling_exa"),
        (3u64, "company_research_exa"),
        (4u64, "deep_researcher_start"),
        (5u64, "agent_run"),
    ] {
        let result = harness
            .exa_mcp_json(&ExaMcpHarness::tools_call(id, tool, json!({ "query": "nope" })))
            .await;
        assert_eq!(result["id"], json!(id));
        assert_eq!(
            result["result"]["isError"],
            json!(true),
            "{tool} must be an unknown-tool error"
        );
        let content_text = result["result"]["content"][0]["text"]
            .as_str()
            .expect("text");
        assert!(
            content_text.contains("Unknown tool"),
            "typed unknown-tool error for {tool}: {content_text}"
        );
    }

    assert_eq!(
        harness.mock_metrics().await,
        json!({ "search": 0, "contents": 0, "answer": 0 }),
        "unknown tools never reach exa upstream"
    );
    assert!(harness.charged_ledger_rows().await.is_empty());
}

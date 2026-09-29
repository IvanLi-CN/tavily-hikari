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

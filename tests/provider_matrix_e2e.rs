//! Plan todo 29 — cross-provider E2E consolidation matrix (shard-registered).
//!
//! This suite owns the CROSS-provider matrix cells that the per-provider
//! suites cannot express: one shared credit wallet spanning ALL FOUR
//! providers in a single run, the M24 shared-wallet pre-check parity
//! (exhausted wallet rejects the NEXT request of ANY provider with ZERO
//! upstream stub calls), the shared hourly business-call gate, the
//! per-token billing lock under parallel mixed-provider requests, persisted
//! log hygiene across every provider surface, and NoAvailableKeys isolation.
//!
//! Real server binary + the four real mock provider binaries
//! (`mock_tavily` / `mock_serpapi` / `mock_exa` / `mock_brave`) + direct
//! sqlite assertions (the proven `provider_facades_contract.rs` /
//! `provider_health_matrix_e2e.rs` harness pattern). Every upstream is a
//! stub: production endpoints are NEVER contacted (AGENTS.md incident rule).
//!
//! Cells already proven by named per-provider tests are cited in the
//! evidence coverage map instead of being duplicated here (retry/backoff,
//! M8 zero-depletion negative, MCP billing, quota surfaces).

use std::{
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use nanoid::nanoid;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[path = "common/support_binaries.rs"]
mod support_binaries;

// ---------------------------------------------------------------------------
// Pool secrets (one per provider; each mock accepts exactly its own).
// ---------------------------------------------------------------------------

const TAVILY_POOL_KEY: &str = "mx-tavily-pool-key";
const SERPAPI_POOL_KEY: &str = "mx-serpapi-pool-key";
const EXA_POOL_KEY: &str = "mx-exa-pool-key";
const BRAVE_POOL_KEY: &str = "mx-brave-pool-key";

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
// Free request helpers (usable from concurrent tokio tasks).
// ---------------------------------------------------------------------------

async fn tavily_search(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    let response = Client::new()
        .post(format!("{base}/api/tavily/search"))
        .bearer_auth(token)
        .json(&json!({ "query": query }))
        .send()
        .await
        .expect("tavily search request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn serpapi_search(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    let response = Client::new()
        .get(format!(
            "{base}/api/serpapi/search?api_key={token}&q={query}"
        ))
        .send()
        .await
        .expect("serpapi search request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn exa_post(base: &str, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
    let response = Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .expect("exa request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn exa_search(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    exa_post(base, token, "/api/exa/search", json!({ "query": query })).await
}

async fn exa_answer(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    exa_post(
        base,
        token,
        "/api/exa/answer",
        json!({ "query": query, "type": "fast" }),
    )
    .await
}

async fn brave_get(base: &str, token: &str, path: &str, query: &str) -> (StatusCode, Value) {
    let response = Client::new()
        .get(format!("{base}{path}?q={query}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("brave request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn brave_web_search(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    brave_get(base, token, "/api/brave/res/v1/web/search", query).await
}

/// Raw MCP-over-HTTP POST returning (status, session header, body bytes).
async fn mcp_post(
    base: &str,
    path: &str,
    token: &str,
    session: Option<&str>,
    message: &Value,
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut request = Client::new()
        .post(format!("{base}{path}"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json");
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = request.json(message).send().await.expect("mcp request");
    let status = response.status();
    let session_header = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.bytes().await.expect("mcp response body").to_vec();
    (status, session_header, body)
}

fn initialize_message(id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "provider-matrix-e2e", "version": "0.1.0" }
        }
    })
}

fn tools_call_message(id: u64, tool: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments }
    })
}

// ---------------------------------------------------------------------------
// Matrix harness: all four provider mocks + the real server in one process set.
// ---------------------------------------------------------------------------

struct MatrixHarness {
    base_url: String,
    token: String,
    token_id: String,
    db_path: PathBuf,
    db_str: String,
    serpapi_base: String,
    exa_base: String,
    brave_base: String,
    tavily_base: String,
    _server: ChildGuard,
    _mocks: Vec<ChildGuard>,
}

impl Drop for MatrixHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

async fn spawn_matrix_harness(prefix: &str, monthly_limit: Option<i64>) -> MatrixHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_matrix_harness(prefix, monthly_limit).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
    panic!("matrix harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_matrix_harness(
    prefix: &str,
    monthly_limit: Option<i64>,
) -> Option<MatrixHarness> {
    let mut mocks = Vec::new();

    let serpapi_port = reserve_local_port();
    mocks.push(ChildGuard {
        child: spawn_mock(
            "MOCK_SERPAPI_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_serpapi"),
            serpapi_port,
            SERPAPI_POOL_KEY,
        ),
    });
    let exa_port = reserve_local_port();
    mocks.push(ChildGuard {
        child: spawn_mock(
            "MOCK_EXA_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_exa"),
            exa_port,
            EXA_POOL_KEY,
        ),
    });
    let brave_port = reserve_local_port();
    mocks.push(ChildGuard {
        child: spawn_mock(
            "MOCK_BRAVE_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_brave"),
            brave_port,
            BRAVE_POOL_KEY,
        ),
    });
    let tavily_port = reserve_local_port();
    mocks.push(ChildGuard {
        child: spawn_mock(
            "MOCK_TAVILY_TEST_BIN",
            env!("CARGO_BIN_EXE_mock_tavily"),
            tavily_port,
            TAVILY_POOL_KEY,
        ),
    });

    let port = reserve_local_port();
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();

    let mut command = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ));
    command
        .env("TAVILY_API_KEYS", TAVILY_POOL_KEY)
        .env(
            "TAVILY_UPSTREAM",
            format!("http://127.0.0.1:{tavily_port}/mcp"),
        )
        .env(
            "TAVILY_USAGE_BASE",
            format!("http://127.0.0.1:{tavily_port}"),
        )
        .env(
            "SERPAPI_UPSTREAM",
            format!("http://127.0.0.1:{serpapi_port}"),
        )
        .env(
            "SERPAPI_MCP_UPSTREAM",
            format!("http://127.0.0.1:{serpapi_port}/mcp"),
        )
        .env("SERPAPI_API_KEYS", SERPAPI_POOL_KEY)
        .env("EXA_UPSTREAM", format!("http://127.0.0.1:{exa_port}"))
        .env("EXA_API_KEYS", EXA_POOL_KEY)
        .env("BRAVE_UPSTREAM", format!("http://127.0.0.1:{brave_port}"))
        .env("BRAVE_API_KEYS", BRAVE_POOL_KEY)
        .env("TAVILY_DISABLE_BACKGROUND_TASKS", "1")
        .env("PROXY_BIND", "127.0.0.1")
        .env("PROXY_PORT", port.to_string())
        .env("PROXY_DB_PATH", &db_str)
        .env("DEV_OPEN_ADMIN", "true")
        .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error");
    if let Some(limit) = monthly_limit {
        command.env("TOKEN_MONTHLY_LIMIT", limit.to_string());
    }
    let server = command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn tavily-hikari");

    if !health_ready_within(port, 30).await {
        drop(ChildGuard { child: server });
        drop(mocks);
        remove_sqlite_files(&db_path);
        return None;
    }
    for provider in ["tavily", "serpapi", "exa", "brave"] {
        wait_for_provider_keys(&db_str, provider, 1).await;
    }

    let base_url = format!("http://127.0.0.1:{port}");
    let (token, token_id) = create_test_token(&base_url).await;

    Some(MatrixHarness {
        base_url,
        token,
        token_id,
        db_path,
        db_str,
        serpapi_base: format!("http://127.0.0.1:{serpapi_port}"),
        exa_base: format!("http://127.0.0.1:{exa_port}"),
        brave_base: format!("http://127.0.0.1:{brave_port}"),
        tavily_base: format!("http://127.0.0.1:{tavily_port}"),
        _server: ChildGuard { child: server },
        _mocks: mocks,
    })
}

fn spawn_mock(env_var: &str, compile_time: &str, port: u16, key: &str) -> Child {
    let mut command = Command::new(support_binaries::resolve_support_binary(
        env_var,
        compile_time,
    ));
    command.arg("--bind").arg(format!("127.0.0.1:{port}"));
    if compile_time.contains("mock_tavily") {
        command.arg("--preseeded-keys").arg(key);
    } else {
        command.arg("--keys").arg(key);
    }
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|err| panic!("spawn mock {compile_time}: {err}"))
}

impl MatrixHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }

    async fn charged_ledger_count(&self) -> i64 {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM billing_ledger WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(&self.token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger count");
        pool.close().await;
        count
    }

    async fn charged_ledger_sum(&self) -> i64 {
        self.ledger_sum_for(&self.token_id).await
    }

    async fn ledger_sum_for(&self, token_id: &str) -> i64 {
        let pool = self.pool().await;
        let sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(business_credits), 0) FROM billing_ledger \
             WHERE token_id = ? AND billing_state = 'charged'",
        )
        .bind(token_id)
        .fetch_one(&pool)
        .await
        .expect("ledger sum");
        pool.close().await;
        sum
    }

    async fn month_count(&self) -> i64 {
        self.month_count_for(&self.token_id).await
    }

    async fn month_count_for(&self, token_id: &str) -> i64 {
        let pool = self.pool().await;
        let count: i64 = sqlx::query_scalar(
            "SELECT COALESCE((SELECT month_count FROM auth_token_quota \
              WHERE token_id = ? LIMIT 1), 0)",
        )
        .bind(token_id)
        .fetch_one(&pool)
        .await
        .expect("token quota row");
        pool.close().await;
        count
    }

    /// (provider, SUM(business_credits), COUNT(*)) for CHARGED rows only,
    /// taking the provider from the linked observability request_logs row
    /// (auth_token_logs.provider is unpopulated; request_logs.provider is the
    /// reliably stamped provider dimension).
    async fn charged_provider_breakdown(&self, token_id: &str) -> Vec<(String, i64, i64)> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT COALESCE(r.provider, '') AS provider, \
             COALESCE(SUM(l.business_credits), 0) AS credits, COUNT(*) AS rows \
             FROM auth_token_logs l \
             JOIN observability.request_logs r ON r.id = l.request_log_id \
             WHERE l.token_id = ? AND l.business_credits > 0 \
             GROUP BY r.provider ORDER BY r.provider",
        )
        .bind(token_id)
        .fetch_all(&pool)
        .await
        .expect("charged provider breakdown");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("provider"),
                    row.get::<i64, _>("credits"),
                    row.get::<i64, _>("rows"),
                )
            })
            .collect()
    }

    /// serpapi remaining searches (REST search decrements this on success) —
    /// the zero-upstream detector for the serpapi REST leg.
    async fn serpapi_searches_left(&self) -> i64 {
        mock_json(&self.serpapi_base, "/account")
            .await
            .get("total_searches_left")
            .and_then(Value::as_i64)
            .expect("total_searches_left")
    }

    /// exa `/metrics` — {search, contents, answer} received-call counters.
    async fn exa_metrics(&self) -> Value {
        mock_json(&self.exa_base, "/metrics").await
    }

    /// brave `/metrics` — {web, news, llm_context} received-call counters.
    async fn brave_metrics(&self) -> Value {
        mock_json(&self.brave_base, "/metrics").await
    }

    /// tavily upstream per-key `usage` (increments only on a successful
    /// upstream search) — the zero-upstream detector for the Tavily leg.
    async fn tavily_usage(&self) -> i64 {
        mock_json(&self.tavily_base, "/admin/state")
            .await
            .get("keys")
            .and_then(|keys| keys.get(TAVILY_POOL_KEY))
            .and_then(|record| record.get("usage"))
            .and_then(Value::as_i64)
            .expect("tavily mock key usage")
    }

    /// Seed one bound user + account quota + bound token through SQLite
    /// (mirrors tests/mcp_session_affinity_e2e.rs:712-790). Returns the full
    /// `th-<id>-<secret>` token. Used by the shared business-call gate cell.
    async fn seed_bound_account_token(
        &self,
        monthly_credits: i64,
        business_calls_1h: i64,
    ) -> String {
        let pool = self.pool().await;
        let now = chrono::Utc::now().timestamp();
        let user_id = "mx-matrix-user";
        let token_id = "mxm1";
        // validate_access_token requires a 4-char id and a 12/24-char secret.
        let secret = format!("matrix{:018}", 1u64);
        let secret = secret.as_str();
        sqlx::query(
            "INSERT INTO users (id, display_name, username, avatar_template, active, \
             created_at, updated_at, last_login_at) VALUES (?, ?, ?, NULL, 1, ?, ?, ?)",
        )
        .bind(user_id)
        .bind("Matrix User")
        .bind("matrix_user")
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert user");
        sqlx::query(
            "INSERT INTO account_quota_limits (user_id, business_calls_1h_limit, \
             daily_credits_limit, monthly_credits_limit, monthly_broken_limit, \
             inherits_defaults, created_at, updated_at) VALUES (?, ?, ?, ?, 5, 1, ?, ?)",
        )
        .bind(user_id)
        .bind(business_calls_1h)
        .bind(500i64)
        .bind(monthly_credits)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert account quota");
        sqlx::query(
            "INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, \
             created_at, last_used_at, deleted_at) VALUES (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)",
        )
        .bind(token_id)
        .bind(secret)
        .bind(format!("matrix bound {user_id}"))
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert bound auth token");
        sqlx::query(
            "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(token_id)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert user token binding");
        pool.close().await;
        format!("th-{token_id}-{secret}")
    }

    /// Persisted request-log text fields per row, for secret scanning:
    /// (query, request_body, response_body, error_message, forwarded_headers,
    /// dropped_headers).
    async fn persisted_request_log_fields(&self) -> Vec<[String; 6]> {
        let pool = self.pool().await;
        let rows = sqlx::query(
            "SELECT COALESCE(query, '') AS query, \
             COALESCE(CAST(request_body AS TEXT), '') AS rb, \
             COALESCE(CAST(response_body AS TEXT), '') AS respb, \
             COALESCE(error_message, '') AS em, \
             COALESCE(CAST(forwarded_headers AS TEXT), '') AS fh, \
             COALESCE(CAST(dropped_headers AS TEXT), '') AS dh \
             FROM observability.request_logs ORDER BY id ASC",
        )
        .fetch_all(&pool)
        .await
        .expect("request log text scan");
        pool.close().await;
        rows.into_iter()
            .map(|row| {
                [
                    row.get::<String, _>("query"),
                    row.get::<String, _>("rb"),
                    row.get::<String, _>("respb"),
                    row.get::<String, _>("em"),
                    row.get::<String, _>("fh"),
                    row.get::<String, _>("dh"),
                ]
            })
            .collect()
    }

    async fn soft_delete_provider_keys(&self, provider: &str) {
        let pool = self.pool().await;
        sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE provider = ? AND deleted_at IS NULL")
            .bind(chrono::Utc::now().timestamp())
            .bind(provider)
            .execute(&pool)
            .await
            .expect("soft delete provider keys");
        pool.close().await;
    }
}

async fn mock_json(base: &str, path: &str) -> Value {
    Client::new()
        .get(format!("{base}{path}"))
        .send()
        .await
        .unwrap_or_else(|err| panic!("mock request {path}: {err}"))
        .json::<Value>()
        .await
        .unwrap_or_else(|err| panic!("mock json {path}: {err}"))
}

// ---------------------------------------------------------------------------
// Cell 1 — shared wallet spans all four providers in one run.
// ---------------------------------------------------------------------------

/// Matrix cell 1: ONE token spends across ALL FOUR providers in one run;
/// billing_ledger charged rows + auth_token_logs provider buckets + the
/// monthly counter sum to the registry credit table (tavily search 1,
/// serpapi search 1, exa search 1, brave web search 1). Each provider is
/// reached exactly once (counter proof).
#[tokio::test]
async fn shared_wallet_bills_across_all_four_providers() {
    let harness = spawn_matrix_harness("matrix-wallet", None).await;
    let (base, token) = (harness.base_url.clone(), harness.token.clone());

    let (status, _) = tavily_search(&base, &token, "matrix-tavily").await;
    assert_eq!(status, StatusCode::OK, "tavily search via shared wallet");
    let (status, _) = serpapi_search(&base, &token, "matrix-serpapi").await;
    assert_eq!(status, StatusCode::OK, "serpapi search via shared wallet");
    let (status, _) = exa_search(&base, &token, "matrix-exa").await;
    assert_eq!(status, StatusCode::OK, "exa search via shared wallet");
    let (status, _) = brave_web_search(&base, &token, "matrix-brave").await;
    assert_eq!(status, StatusCode::OK, "brave search via shared wallet");

    assert_eq!(
        harness.charged_ledger_count().await,
        4,
        "one charged ledger row per provider op"
    );
    assert_eq!(
        harness.charged_ledger_sum().await,
        4,
        "ledger sum == registry total (1+1+1+1)"
    );
    assert_eq!(
        harness.month_count().await,
        4,
        "shared wallet monthly counter == sum of per-op credits"
    );
    assert_eq!(
        harness.charged_provider_breakdown(&harness.token_id).await,
        vec![
            ("brave".to_string(), 1, 1),
            ("exa".to_string(), 1, 1),
            ("serpapi".to_string(), 1, 1),
            ("tavily".to_string(), 1, 1),
        ],
        "per-provider token-log buckets each carry exactly one credit"
    );

    // One upstream call per provider (mocks prove the calls happened once).
    assert_eq!(
        harness.exa_metrics().await,
        json!({ "search": 1, "contents": 0, "answer": 0 }),
        "exa mock saw exactly one search"
    );
    assert_eq!(
        harness.brave_metrics().await,
        json!({ "web": 1, "news": 0, "llm_context": 0 }),
        "brave mock saw exactly one web search"
    );
    assert_eq!(harness.tavily_usage().await, 1, "tavily mock usage == 1");
}

// ---------------------------------------------------------------------------
// Cell 2 — M24 shared-wallet pre-check parity across ALL providers.
// ---------------------------------------------------------------------------

/// Matrix cell 2 (M24): a token holding 3 credits spends exa_answer(2) then
/// serpapi_search(1) — both settle to the window limit — and the NEXT request
/// of ANY provider (all four exercised) is locally rejected with 429
/// `quota_exhausted` and ZERO upstream stub calls. The reserved-credit
/// pre-check is shared across providers (docs/specs/upstream-credits-billing/
/// SPEC.md:54-55).
#[tokio::test]
async fn precheck_parity_rejects_every_provider_with_zero_upstream_calls() {
    let harness = spawn_matrix_harness("matrix-precheck", Some(3)).await;
    let (base, token) = (harness.base_url.clone(), harness.token.clone());

    // Spend exactly to the 3-credit limit: exa_answer(2) + serpapi_search(1).
    let (status, _) = exa_answer(&base, &token, "matrix-answer").await;
    assert_eq!(status, StatusCode::OK, "exa answer (2 credits) must pass");
    let (status, _) = serpapi_search(&base, &token, "matrix-serpapi").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "serpapi search (1 credit) must pass at exactly the limit"
    );
    assert_eq!(harness.month_count().await, 3, "wallet fully spent");

    // Snapshot the upstream observation surface for the four providers.
    let serpapi_left_before = harness.serpapi_searches_left().await;
    let exa_before = harness.exa_metrics().await;
    let brave_before = harness.brave_metrics().await;
    let tavily_usage_before = harness.tavily_usage().await;

    // Every provider's next request is rejected locally.
    let (tavily_status, tavily_body) = tavily_search(&base, &token, "matrix-next-tavily").await;
    assert_eq!(tavily_status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        tavily_body.get("error").and_then(Value::as_str),
        Some("quota_exhausted")
    );
    let (serpapi_status, serpapi_body) = serpapi_search(&base, &token, "matrix-next-serpapi").await;
    assert_eq!(serpapi_status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        serpapi_body.get("error").and_then(Value::as_str),
        Some("quota_exhausted")
    );
    let (exa_status, exa_body) = exa_search(&base, &token, "matrix-next-exa").await;
    assert_eq!(exa_status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        exa_body.get("error").and_then(Value::as_str),
        Some("quota_exhausted")
    );
    let (brave_status, brave_body) = brave_web_search(&base, &token, "matrix-next-brave").await;
    assert_eq!(brave_status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        brave_body.get("error").and_then(Value::as_str),
        Some("quota_exhausted")
    );

    // ZERO upstream stub calls: every counter identical to the snapshot.
    assert_eq!(harness.month_count().await, 3, "rejections never settle");
    assert_eq!(
        harness.serpapi_searches_left().await,
        serpapi_left_before,
        "serpapi mock was never reached by the rejected request"
    );
    assert_eq!(
        harness.exa_metrics().await,
        exa_before,
        "exa mock was never reached by the rejected request"
    );
    assert_eq!(
        harness.brave_metrics().await,
        brave_before,
        "brave mock was never reached by the rejected request"
    );
    assert_eq!(
        harness.tavily_usage().await,
        tavily_usage_before,
        "tavily mock was never reached by the rejected request"
    );
}

// ---------------------------------------------------------------------------
// Cell 3 — hourly business-call gate is shared across providers.
// ---------------------------------------------------------------------------

/// Matrix cell 3: for a user-bound token the hourly business-call window is
/// keyed by the billing subject (user), so consumption on ONE provider counts
/// against admission on ANOTHER. With business_calls_1h_limit=2, a serpapi
/// search then an exa search fill the window and the following brave search is
/// locally rejected with 429 before any upstream call.
#[tokio::test]
async fn shared_business_calls_1h_limit_blocks_across_providers() {
    let harness = spawn_matrix_harness("matrix-business-calls", None).await;
    let base = harness.base_url.clone();
    let token = harness.seed_bound_account_token(1000, 2).await;

    let (status, _) = serpapi_search(&base, &token, "matrix-bc-serpapi").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "first business call (serpapi) passes"
    );
    let (status, _) = exa_search(&base, &token, "matrix-bc-exa").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "second business call (exa, DIFFERENT provider) counts against the \
         shared user window and still passes under the limit"
    );

    let brave_before = harness.brave_metrics().await;
    let (status, body) = brave_web_search(&base, &token, "matrix-bc-brave").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "third cross-provider business call is rejected by the shared window"
    );
    assert_eq!(
        body.get("error").and_then(Value::as_str),
        Some("quota_exhausted")
    );
    assert_eq!(
        harness.brave_metrics().await,
        brave_before,
        "brave upstream never reached on the business-call rejection"
    );

    let bound_token_id = "mxm1";
    assert_eq!(
        harness.charged_provider_breakdown(bound_token_id).await,
        vec![("exa".to_string(), 1, 1), ("serpapi".to_string(), 1, 1),],
        "only the two admitted calls settled a credit (brave never charged)"
    );
    assert_eq!(
        harness.ledger_sum_for(bound_token_id).await,
        2,
        "ledger sum == admitted calls (no charge for the rejected one)"
    );
}

// ---------------------------------------------------------------------------
// Cell 4 — per-token billing lock under parallel mixed-provider requests.
// ---------------------------------------------------------------------------

/// Matrix cell 4: parallel mixed-provider requests must charge exactly once
/// each — no double-charge and no lost settle. The failure-detector is ledger
/// sum equality (8 charged rows / sum 8 / month_count 8 for 8 parallel ops).
#[tokio::test]
async fn concurrent_mixed_provider_billing_charges_exactly_once() {
    let harness = spawn_matrix_harness("matrix-concurrency", None).await;
    let base = harness.base_url.clone();
    let token = harness.token.clone();

    let mut handles = Vec::new();
    for _ in 0..2 {
        let (b, t) = (base.clone(), token.clone());
        handles.push(tokio::spawn(async move {
            tavily_search(&b, &t, "concurrent-tavily").await.0
        }));
        let (b, t) = (base.clone(), token.clone());
        handles.push(tokio::spawn(async move {
            serpapi_search(&b, &t, "concurrent-serpapi").await.0
        }));
        let (b, t) = (base.clone(), token.clone());
        handles.push(tokio::spawn(async move {
            exa_search(&b, &t, "concurrent-exa").await.0
        }));
        let (b, t) = (base.clone(), token.clone());
        handles.push(tokio::spawn(async move {
            brave_web_search(&b, &t, "concurrent-brave").await.0
        }));
    }
    for handle in handles {
        let status = handle.await.expect("concurrent request task joined");
        assert_eq!(status, StatusCode::OK, "all concurrent requests succeed");
    }

    assert_eq!(
        harness.charged_ledger_count().await,
        8,
        "exactly one charged ledger row per successful request (no lost settle)"
    );
    assert_eq!(
        harness.charged_ledger_sum().await,
        8,
        "ledger sum equality — the concurrency failure-detector (no double-charge)"
    );
    assert_eq!(
        harness.month_count().await,
        8,
        "monthly counter equals the summed per-op credits"
    );
    assert_eq!(
        harness.charged_provider_breakdown(&harness.token_id).await,
        vec![
            ("brave".to_string(), 2, 2),
            ("exa".to_string(), 2, 2),
            ("serpapi".to_string(), 2, 2),
            ("tavily".to_string(), 2, 2),
        ],
        "per-provider buckets reflect the two parallel ops each"
    );
}

// ---------------------------------------------------------------------------
// Cell 5 — persisted-row log hygiene across every provider surface (C6).
// ---------------------------------------------------------------------------

/// Matrix cell 5 (C6): after one REST call and one MCP tools/call per provider
/// in a single run, NO persisted `request_logs` row contains the Hikari
/// `th-` access token nor any provider pool secret in the request-side
/// columns (query / request_body / error_message / forwarded/dropped headers).
/// Response bodies are exempt for pool secrets because mock_tavily echoes the
/// bound key as `mock_bound_key` (an upstream artifact, not a Hikari leak).
#[tokio::test]
async fn persisted_rows_never_carry_tokens_or_provider_secrets() {
    let harness = spawn_matrix_harness("matrix-log-hygiene", None).await;
    let (base, token) = (harness.base_url.clone(), harness.token.clone());

    // REST surface: one spend per provider.
    assert_eq!(
        tavily_search(&base, &token, "hygiene-tavily").await.0,
        StatusCode::OK
    );
    assert_eq!(
        serpapi_search(&base, &token, "hygiene-serpapi").await.0,
        StatusCode::OK
    );
    assert_eq!(
        exa_search(&base, &token, "hygiene-exa").await.0,
        StatusCode::OK
    );
    assert_eq!(
        brave_web_search(&base, &token, "hygiene-brave").await.0,
        StatusCode::OK
    );

    // MCP surface: initialize (+ notification) then tools/call per provider.
    let exa_init = mcp_post(&base, "/exa/mcp", &token, None, &initialize_message(1)).await;
    assert_eq!(exa_init.0, StatusCode::OK, "exa mcp initialize");
    let (status, _, _) = mcp_post(
        &base,
        "/exa/mcp",
        &token,
        None,
        &tools_call_message(2, "web_search_exa", json!({ "query": "hygiene" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "exa mcp tools/call");

    let brave_init = mcp_post(&base, "/brave/mcp", &token, None, &initialize_message(3)).await;
    assert_eq!(brave_init.0, StatusCode::OK, "brave mcp initialize");
    let (status, _, _) = mcp_post(
        &base,
        "/brave/mcp",
        &token,
        None,
        &tools_call_message(4, "brave_web_search", json!({ "query": "hygiene" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "brave mcp tools/call");

    let serpapi_init = mcp_post(&base, "/serpapi/mcp", &token, None, &initialize_message(5)).await;
    assert_eq!(serpapi_init.0, StatusCode::OK, "serpapi mcp initialize");
    let session = serpapi_init.1.expect("serpapi mcp session header");
    let (status, _, _) = mcp_post(
        &base,
        "/serpapi/mcp",
        &token,
        Some(&session),
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "serpapi mcp initialized");
    let (status, _, _) = mcp_post(
        &base,
        "/serpapi/mcp",
        &token,
        Some(&session),
        &tools_call_message(6, "search", json!({ "q": "hygiene" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "serpapi mcp tools/call");

    let rows = harness.persisted_request_log_fields().await;
    assert!(rows.len() >= 7, "all provider surfaces persisted a row");
    let secret_tail = token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(_, secret)| secret.to_string()))
        .expect("token secret tail");
    for (index, fields) in rows.iter().enumerate() {
        // The access token must never appear in ANY persisted text field.
        for text in fields {
            assert!(
                !text.contains(&token),
                "persisted row {index} must never carry the full th- token"
            );
            assert!(
                !text.contains(&secret_tail),
                "persisted row {index} must never carry the th- secret tail"
            );
        }
        // Request-side columns must never carry any provider pool secret.
        // (Response bodies are exempt: mock_tavily echoes `mock_bound_key` as
        // an upstream artifact, not a Hikari leak.)
        for (field_index, text) in fields.iter().enumerate() {
            if field_index == 2 {
                continue;
            }
            for secret in [
                TAVILY_POOL_KEY,
                SERPAPI_POOL_KEY,
                EXA_POOL_KEY,
                BRAVE_POOL_KEY,
            ] {
                assert!(
                    !text.contains(secret),
                    "persisted request-side row {index} field {field_index} must never \
                     carry pool secret {secret}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cell 6 — NoAvailableKeys isolation per provider.
// ---------------------------------------------------------------------------

/// Matrix cell 6: an empty (soft-deleted) pool for ONE provider yields the
/// NoAvailableKeys 502 shape for that provider only — the OTHER providers
/// keep leasing their own keys and bill normally (no cross-provider starvation).
#[tokio::test]
async fn empty_provider_pool_does_not_starve_other_providers() {
    let harness = spawn_matrix_harness("matrix-nopool-isolation", None).await;
    let base = harness.base_url.clone();
    let token = harness.token.clone();

    harness.soft_delete_provider_keys("serpapi").await;

    let (status, body) = serpapi_search(&base, &token, "matrix-nopool").await;
    assert_eq!(
        status,
        StatusCode::BAD_GATEWAY,
        "empty serpapi pool returns the NoAvailableKeys 502 shape"
    );
    assert_eq!(
        body.get("error").and_then(Value::as_str),
        Some("proxy_error")
    );
    assert_eq!(
        body.get("message").and_then(Value::as_str),
        Some("upstream unavailable")
    );

    // Isolation: the other three providers still lease + bill.
    assert_eq!(
        exa_search(&base, &token, "matrix-nopool-exa").await.0,
        StatusCode::OK,
        "exa leasing is unaffected by the empty serpapi pool"
    );
    assert_eq!(
        brave_web_search(&base, &token, "matrix-nopool-brave")
            .await
            .0,
        StatusCode::OK,
        "brave leasing is unaffected by the empty serpapi pool"
    );
    assert_eq!(
        tavily_search(&base, &token, "matrix-nopool-tavily").await.0,
        StatusCode::OK,
        "tavily leasing is unaffected by the empty serpapi pool"
    );
    assert_eq!(
        harness.month_count().await,
        3,
        "only the three unaffected providers settle credits"
    );
    assert_eq!(
        harness.charged_provider_breakdown(&harness.token_id).await,
        vec![
            ("brave".to_string(), 1, 1),
            ("exa".to_string(), 1, 1),
            ("tavily".to_string(), 1, 1),
        ],
        "no serpapi bucket — the empty pool never charged"
    );
}

//! Plan todo 30 — migration / HA / regression hardening (shard-registered).
//!
//! This suite owns the PROCESS-LEVEL migration hardening for the
//! multi-provider (serpapi/exa/brave) plan that the in-process lib tests
//! under `src/tests/` cannot express:
//!
//!  * a genuinely seeded **v44 fixture** (44 ledger rows = 30 versioned +
//!    14 convergence, provider-less legacy table shapes, dangling-key row)
//!    is upgraded by the REAL server binary to v45; the v45 ledger row +
//!    checksum land, `migration_count` reaches 45, the sidecar
//!    `request_logs.provider` ALTER is applied exactly once, every legacy
//!    row stays readable, and legacy `api_keys` rows resolve to
//!    `provider='tavily'` (M23);
//!  * a **fresh bootstrap** records every column from `initialize_schema`
//!    DDL AND the full 1..=45 ledger (house rule);
//!  * the **provider facades stay functional post-upgrade** (one REST + one
//!    MCP call per new provider against stub mocks seeded by post-upgrade
//!    keys) with the M8 negative: zero `api_key_low_quota_depletions` rows
//!    for the new providers;
//!  * the **HA control baseline export** from the upgraded DB carries the
//!    provider column;
//!  * a **checksum tamper** on the upgraded DB is rejected at startup with
//!    the typed `schema migration checksum mismatch at version 45` error.
//!
//! Real server binary + the four real mock provider binaries
//! (`mock_tavily` / `mock_serpapi` / `mock_exa` / `mock_brave`) + direct
//! sqlite assertions — the proven `provider_matrix_e2e.rs` harness pattern.
//! Every upstream is a stub: production endpoints are NEVER contacted
//! (AGENTS.md incident rule).
//!
//! Committed coverage that is CITED, not duplicated: the v45 apply-path
//! idempotence + checksum rejection + legacy-insert defaulting lib tests and
//! the sidecar attach-path idempotence suite (`src/tests/schema_migrations.rs`
//! `provider_aware_v45_*`, `src/tests/observability_sidecar_provider.rs`) plus
//! the HA legacy-NDJSON import defaulting lib test.

use std::{
    io::Cursor,
    net::TcpListener as StdTcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use nanoid::nanoid;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

#[path = "common/support_binaries.rs"]
mod support_binaries;

// ---------------------------------------------------------------------------
// Pool secrets (one per provider; each mock accepts exactly its own).
// ---------------------------------------------------------------------------

const TAVILY_POOL_KEY: &str = "mh-tavily-pool-key";
const SERPAPI_POOL_KEY: &str = "mh-serpapi-pool-key";
const EXA_POOL_KEY: &str = "mh-exa-pool-key";
const BRAVE_POOL_KEY: &str = "mh-brave-pool-key";

const LEGACY_TOKEN_ID: &str = "legacy-token";
const LEGACY_SESSION_ID: &str = "legacy-session";
const DANGLING_KEY_ID: &str = "ghost-dangling-key";
const LEGACY_BILLING_LOG_ID: i64 = 900_001;

/// v45 recorded checksum (byte-identical contract; see
/// `key_store_schema_migrations_provider.rs`).
const PROVIDER_AWARE_V45_CHECKSUM: &str =
    "sha256:77201d3952329ffb3e54be6c2a1bdb08d595e3ecbc3e5279a6fb6f4895f4bd0f";

// ---------------------------------------------------------------------------
// Shared harness utilities (pattern: tests/provider_matrix_e2e.rs).
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
// Free request helpers.
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
        .get(format!("{base}/api/serpapi/search?api_key={token}&q={query}"))
        .send()
        .await
        .expect("serpapi search request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn exa_search(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    let response = Client::new()
        .post(format!("{base}/api/exa/search"))
        .bearer_auth(token)
        .json(&json!({ "query": query }))
        .send()
        .await
        .expect("exa search request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

async fn brave_web_search(base: &str, token: &str, query: &str) -> (StatusCode, Value) {
    let response = Client::new()
        .get(format!("{base}/api/brave/res/v1/web/search?q={query}"))
        .bearer_auth(token)
        .send()
        .await
        .expect("brave request");
    let status = response.status();
    (status, response.json().await.unwrap_or(Value::Null))
}

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
            "clientInfo": { "name": "provider-migration-hardening", "version": "0.1.0" }
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

fn base_server_command(db_str: &str, port: u16, tavily_upstream: &str, tavily_usage: &str) -> Command {
    let mut command = Command::new(support_binaries::resolve_support_binary(
        "TAVILY_HIKARI_TEST_BIN",
        env!("CARGO_BIN_EXE_tavily-hikari"),
    ));
    command
        .env("TAVILY_API_KEYS", TAVILY_POOL_KEY)
        .env("TAVILY_UPSTREAM", tavily_upstream)
        .env("TAVILY_USAGE_BASE", tavily_usage)
        .env("TAVILY_DISABLE_BACKGROUND_TASKS", "1")
        .env("PROXY_BIND", "127.0.0.1")
        .env("PROXY_PORT", port.to_string())
        .env("PROXY_DB_PATH", db_str)
        .env("DEV_OPEN_ADMIN", "true")
        .env("RUST_LOG", "warn,tavily_hikari=info,sqlx::query=error");
    command
}

// ---------------------------------------------------------------------------
// v44 fixture: bootstrap a fresh DB, then downgrade to the v44 shape and
// seed pre-provider legacy rows (M23: 30 versioned + 14 convergence rows).
// ---------------------------------------------------------------------------

async fn build_v44_fixture(prefix: &str) -> (PathBuf, String) {
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();

    // Phase 1: a fresh (v45) bootstrap run through the real binary, then stop
    // it. We then strip the v45 objects to reconstruct a genuine v44 database.
    let port = reserve_local_port();
    let dead_port = reserve_local_port();
    let child = base_server_command(
        &db_str,
        port,
        &format!("http://127.0.0.1:{dead_port}/mcp"),
        &format!("http://127.0.0.1:{dead_port}"),
    )
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn v44 bootstrap server");
    assert!(
        health_ready_within(port, 30).await,
        "v44 fixture bootstrap server never became healthy"
    );
    wait_for_provider_keys(&db_str, "tavily", 1).await;
    drop(ChildGuard { child });
    // Let the OS release the sqlite file handles before mutating offline.
    tokio::time::sleep(Duration::from_millis(300)).await;

    downgrade_and_seed_v44(&db_str).await;
    (db_path, db_str)
}

async fn downgrade_and_seed_v44(db_str: &str) {
    let pool = connect_sqlite_test_pool(db_str).await;

    // Ledger: drop the sole v45 row, leaving 30 versioned + 14 convergence.
    sqlx::query("DELETE FROM schema_migrations WHERE version = 45")
        .execute(&pool)
        .await
        .expect("drop v45 ledger row");
    // v45 objects: index before the column it references.
    sqlx::query("DROP INDEX IF EXISTS idx_api_keys_provider_status")
        .execute(&pool)
        .await
        .expect("drop provider index");
    for statement in [
        "ALTER TABLE api_keys DROP COLUMN provider",
        "ALTER TABLE api_keys DROP COLUMN quota_renews_at",
        "ALTER TABLE auth_token_logs DROP COLUMN provider",
        "ALTER TABLE mcp_sessions DROP COLUMN provider",
        "ALTER TABLE observability.request_logs DROP COLUMN provider",
    ] {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .unwrap_or_else(|err| panic!("restore v44 shape ({statement}): {err}"));
    }

    // Legacy rows in pre-provider shapes. `created_at`/required columns are
    // the minimum the v44 DDL demanded; every later column stays NULL.
    sqlx::query(
        "INSERT INTO auth_token_logs (token_id, method, path, result_status, created_at) \
         VALUES (?, 'POST', '/mcp', 'success', 1)",
    )
    .bind(LEGACY_TOKEN_ID)
    .execute(&pool)
    .await
    .expect("seed legacy auth_token_logs row");
    sqlx::query(
        "INSERT INTO mcp_sessions (proxy_session_id, created_at, updated_at, expires_at) \
         VALUES (?, 1, 1, 9999999999)",
    )
    .bind(LEGACY_SESSION_ID)
    .execute(&pool)
    .await
    .expect("seed legacy mcp_sessions row");
    sqlx::query(
        "INSERT INTO billing_ledger \
             (auth_token_log_id, token_id, result_status, created_at, updated_at) \
         VALUES (?, ?, 'success', 1, 1)",
    )
    .bind(LEGACY_BILLING_LOG_ID)
    .bind(LEGACY_TOKEN_ID)
    .execute(&pool)
    .await
    .expect("seed legacy billing_ledger row");
    // Dangling-key row: `api_key_id` points at no `api_keys` row. The T19
    // `NOT EXISTS (api_keys.provider <> 'tavily')` guard shape must keep it
    // selectable and coalesce it to tavily.
    sqlx::query(
        "INSERT INTO observability.request_logs (method, path, api_key_id, created_at) \
         VALUES ('POST', '/mcp', ?, 1)",
    )
    .bind(DANGLING_KEY_ID)
    .execute(&pool)
    .await
    .expect("seed dangling-key request_logs row");

    // Assert the fixture is genuinely v44 before the upgrade run.
    let ledger_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
        .fetch_one(&pool)
        .await
        .expect("count v44 ledger rows");
    assert_eq!(ledger_rows, 44, "fixture must present exactly 44 ledger rows");
    let v45_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version = 45")
            .fetch_one(&pool)
            .await
            .expect("count v45 rows");
    assert_eq!(v45_rows, 0, "fixture must not carry the v45 ledger row");
    let api_keys_provider: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('api_keys') WHERE name = 'provider'",
    )
    .fetch_one(&pool)
    .await
    .expect("probe fixture api_keys.provider");
    assert_eq!(api_keys_provider, 0, "fixture api_keys must be provider-less");
    let sidecar_provider: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('request_logs', 'observability') WHERE name = 'provider'",
    )
    .fetch_one(&pool)
    .await
    .expect("probe fixture request_logs.provider");
    assert_eq!(
        sidecar_provider, 0,
        "fixture sidecar request_logs must be provider-less"
    );

    pool.close().await;
}

// ---------------------------------------------------------------------------
// Upgrade / fresh harness: four mock providers + the real server binary.
// ---------------------------------------------------------------------------

struct MigrationHarness {
    base_url: String,
    token: String,
    db_path: PathBuf,
    db_str: String,
    _server: ChildGuard,
    _mocks: Vec<ChildGuard>,
}

impl Drop for MigrationHarness {
    fn drop(&mut self) {
        remove_sqlite_files(&self.db_path);
    }
}

impl MigrationHarness {
    async fn pool(&self) -> sqlx::SqlitePool {
        connect_sqlite_test_pool(&self.db_str).await
    }
}

async fn spawn_harness(prefix: &str, db_path: PathBuf, db_str: String) -> MigrationHarness {
    for attempt in 1..=5usize {
        if let Some(harness) = try_spawn_harness(&db_path, &db_str).await {
            return harness;
        }
        if attempt < 5 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }
    panic!("{prefix} migration harness never became healthy after 5 spawn attempts");
}

async fn try_spawn_harness(db_path: &Path, db_str: &str) -> Option<MigrationHarness> {
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
    let server = base_server_command(
        db_str,
        port,
        &format!("http://127.0.0.1:{tavily_port}/mcp"),
        &format!("http://127.0.0.1:{tavily_port}"),
    )
    .env("SERPAPI_UPSTREAM", format!("http://127.0.0.1:{serpapi_port}"))
    .env(
        "SERPAPI_MCP_UPSTREAM",
        format!("http://127.0.0.1:{serpapi_port}/mcp"),
    )
    .env("SERPAPI_API_KEYS", SERPAPI_POOL_KEY)
    .env("EXA_UPSTREAM", format!("http://127.0.0.1:{exa_port}"))
    .env("EXA_API_KEYS", EXA_POOL_KEY)
    .env("BRAVE_UPSTREAM", format!("http://127.0.0.1:{brave_port}"))
    .env("BRAVE_API_KEYS", BRAVE_POOL_KEY)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn tavily-hikari");

    if !health_ready_within(port, 30).await {
        drop(ChildGuard { child: server });
        drop(mocks);
        return None;
    }
    for provider in ["tavily", "serpapi", "exa", "brave"] {
        wait_for_provider_keys(db_str, provider, 1).await;
    }

    let base_url = format!("http://127.0.0.1:{port}");
    let (token, _token_id) = create_test_token(&base_url).await;

    Some(MigrationHarness {
        base_url,
        token,
        db_path: db_path.to_path_buf(),
        db_str: db_str.to_string(),
        _server: ChildGuard { child: server },
        _mocks: mocks,
    })
}

// ---------------------------------------------------------------------------
// Shared migration assertions.
// ---------------------------------------------------------------------------

async fn assert_v45_schema_applied(pool: &sqlx::SqlitePool, context: &str) {
    let ledger_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
        .fetch_one(pool)
        .await
        .expect("count ledger rows");
    assert_eq!(
        ledger_rows, 45,
        "{context}: migration_count must reach 45 (30 versioned + 14 convergence + v45)"
    );
    let versioned: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version <= 30")
            .fetch_one(pool)
            .await
            .expect("count versioned rows");
    let convergence: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations WHERE version BETWEEN 31 AND 44")
            .fetch_one(pool)
            .await
            .expect("count convergence rows");
    assert_eq!(
        (versioned, convergence),
        (30, 14),
        "{context}: 30 versioned + 14 convergence rows expected"
    );

    let (name, checksum): (String, String) =
        sqlx::query_as("SELECT name, checksum FROM schema_migrations WHERE version = 45")
            .fetch_one(pool)
            .await
            .expect("read v45 ledger row");
    assert_eq!(name, "provider-aware-schema-v1", "{context}: v45 name");
    assert_eq!(checksum, PROVIDER_AWARE_V45_CHECKSUM, "{context}: v45 checksum");

    let api_keys_cols: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('api_keys') \
         WHERE name IN ('provider', 'quota_renews_at')",
    )
    .fetch_one(pool)
    .await
    .expect("probe api_keys columns");
    assert_eq!(api_keys_cols, 2, "{context}: api_keys v45 columns");
    let auth_token_logs_cols: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('auth_token_logs') WHERE name = 'provider'",
    )
    .fetch_one(pool)
    .await
    .expect("probe auth_token_logs columns");
    assert_eq!(auth_token_logs_cols, 1, "{context}: auth_token_logs.provider");
    let mcp_sessions_cols: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('mcp_sessions') WHERE name = 'provider'",
    )
    .fetch_one(pool)
    .await
    .expect("probe mcp_sessions columns");
    assert_eq!(mcp_sessions_cols, 1, "{context}: mcp_sessions.provider");
    // Sidecar ALTER path: exactly one provider column (idempotent).
    let sidecar_provider: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('request_logs', 'observability') WHERE name = 'provider'",
    )
    .fetch_one(pool)
    .await
    .expect("probe sidecar request_logs columns");
    assert_eq!(
        sidecar_provider, 1,
        "{context}: sidecar request_logs.provider must exist exactly once"
    );
    let provider_index: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' \
         AND name = 'idx_api_keys_provider_status'",
    )
    .fetch_one(pool)
    .await
    .expect("probe provider index");
    assert_eq!(provider_index, 1, "{context}: provider index");
}

// ---------------------------------------------------------------------------
// Test 1 — warm v44 → v45 upgrade E2E (M23 core).
// ---------------------------------------------------------------------------

/// Legacy-row counts for the QA happy-path before/after identity check.
async fn legacy_row_counts(pool: &sqlx::SqlitePool) -> (i64, i64, i64, i64) {
    let auth: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ?")
            .bind(LEGACY_TOKEN_ID)
            .fetch_one(pool)
            .await
            .expect("count legacy auth rows");
    let session: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mcp_sessions WHERE proxy_session_id = ?")
            .bind(LEGACY_SESSION_ID)
            .fetch_one(pool)
            .await
            .expect("count legacy session rows");
    let billing: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM billing_ledger WHERE auth_token_log_id = ?")
            .bind(LEGACY_BILLING_LOG_ID)
            .fetch_one(pool)
            .await
            .expect("count legacy billing rows");
    let dangling: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs WHERE api_key_id = ?")
            .bind(DANGLING_KEY_ID)
            .fetch_one(pool)
            .await
            .expect("count dangling-key rows");
    (auth, session, billing, dangling)
}

/// Given a genuinely seeded v44 fixture (30 versioned + 14 convergence rows,
/// provider-less legacy shapes, a dangling-key row).
/// When the real server binary starts against it.
/// Then v45 is applied exactly once (ledger + checksum + migration_count=45,
/// sidecar ALTER), every legacy row stays readable, and legacy
/// api_keys/auth_token_logs/mcp_sessions rows resolve to `tavily` — never a
/// non-tavily provider.
#[tokio::test]
async fn warm_v44_upgrade_applies_v45_and_keeps_legacy_rows_tavily() {
    let (db_path, db_str) = build_v44_fixture("mh-warm-upgrade").await;
    let pre_counts = {
        let pool = connect_sqlite_test_pool(&db_str).await;
        let counts = legacy_row_counts(&pool).await;
        pool.close().await;
        counts
    };
    assert_eq!(
        pre_counts,
        (1, 1, 1, 1),
        "fixture must seed exactly one legacy row per surface"
    );
    let harness = spawn_harness("mh-warm-upgrade", db_path, db_str).await;
    let pool = harness.pool().await;

    assert_v45_schema_applied(&pool, "warm upgrade").await;

    // QA happy: the warm upgrade loses no legacy row — counts identical.
    let post_counts = legacy_row_counts(&pool).await;
    assert_eq!(
        pre_counts, post_counts,
        "warm upgrade must keep legacy row counts identical (no data loss)"
    );

    // api_keys: NOT NULL DEFAULT 'tavily' fills every legacy row literally.
    let legacy_key_provider: String =
        sqlx::query_scalar("SELECT provider FROM api_keys WHERE api_key = ?")
            .bind(TAVILY_POOL_KEY)
            .fetch_one(&pool)
            .await
            .expect("read legacy tavily key provider");
    assert_eq!(legacy_key_provider, "tavily");
    let null_api_keys: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE provider IS NULL")
            .fetch_one(&pool)
            .await
            .expect("count NULL api_keys providers");
    assert_eq!(null_api_keys, 0, "no api_keys row may carry a NULL provider");

    // auth_token_logs/mcp_sessions: the v45 ALTER is nullable (legacy rows
    // stay NULL and readers coalesce — T8/T29 contract), so assert zero
    // non-tavily contamination plus the coalesced read.
    for (table, token_column) in [
        ("auth_token_logs", "token_id"),
        ("mcp_sessions", "proxy_session_id"),
    ] {
        let contaminate: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE provider IS NOT NULL AND provider <> 'tavily'"
        ))
        .fetch_one(&pool)
        .await
        .expect("count non-tavily rows");
        assert_eq!(contaminate, 0, "{table}: no legacy row may become non-tavily");
        let legacy_provider: String = sqlx::query_scalar(&format!(
            "SELECT COALESCE(provider, 'tavily') FROM {table} WHERE {token_column} = ?"
        ))
        .bind(if table == "auth_token_logs" {
            LEGACY_TOKEN_ID
        } else {
            LEGACY_SESSION_ID
        })
        .fetch_one(&pool)
        .await
        .expect("read coalesced legacy provider");
        assert_eq!(legacy_provider, "tavily", "{table}: legacy row resolves to tavily");
    }

    // Every legacy row survives and stays readable.
    let legacy_auth_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = ?")
            .bind(LEGACY_TOKEN_ID)
            .fetch_one(&pool)
            .await
            .expect("count legacy auth rows");
    assert_eq!(legacy_auth_rows, 1, "legacy auth_token_logs row must survive");
    let legacy_session_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mcp_sessions WHERE proxy_session_id = ?")
            .bind(LEGACY_SESSION_ID)
            .fetch_one(&pool)
            .await
            .expect("count legacy mcp session rows");
    assert_eq!(legacy_session_rows, 1, "legacy mcp_sessions row must survive");
    let legacy_billing_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM billing_ledger WHERE auth_token_log_id = ?")
            .bind(LEGACY_BILLING_LOG_ID)
            .fetch_one(&pool)
            .await
            .expect("count legacy billing rows");
    assert_eq!(legacy_billing_rows, 1, "legacy billing_ledger row must survive");
    // Dangling-key row: still selectable, coalesces to tavily.
    let dangling_coalesced: String = sqlx::query_scalar(
        "SELECT COALESCE(provider, 'tavily') FROM observability.request_logs \
         WHERE api_key_id = ? LIMIT 1",
    )
    .bind(DANGLING_KEY_ID)
    .fetch_one(&pool)
    .await
    .expect("read dangling-key legacy row");
    assert_eq!(dangling_coalesced, "tavily");
    let request_log_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
        .fetch_one(&pool)
        .await
        .expect("count request logs");
    assert!(request_log_rows >= 1, "legacy request_logs row must survive");
}

// ---------------------------------------------------------------------------
// Test 2 — fresh bootstrap E2E (house rule).
// ---------------------------------------------------------------------------

/// Given a brand-new database.
/// When the real server binary bootstraps it.
/// Then columns come from the `initialize_schema` DDL AND the ledger holds
/// every recorded version 1..=45 (the src/tests/schema_migrations.rs golden
/// vector is cited, not duplicated).
#[tokio::test]
async fn fresh_bootstrap_records_provider_columns_and_full_ledger() {
    let db_path = temp_db_path("mh-fresh-bootstrap");
    let db_str = db_path.to_string_lossy().to_string();
    let harness = spawn_harness("mh-fresh-bootstrap", db_path, db_str).await;
    let pool = harness.pool().await;

    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .expect("read fresh ledger");
    assert_eq!(
        versions,
        (1..=45_i64).collect::<Vec<_>>(),
        "fresh bootstrap must record the golden vector 1..=45"
    );

    assert_v45_schema_applied(&pool, "fresh bootstrap").await;
}

// ---------------------------------------------------------------------------
// Test 3 — provider facades functional post-upgrade + M8 negative.
// ---------------------------------------------------------------------------

/// Given an upgraded v45 database with post-upgrade provider keys.
/// When one REST + one MCP call per new provider hits the stubs.
/// Then every facade answers 200, each stamps a provider-tagged request log,
/// and M8 holds: zero depletion rows for the new providers.
#[tokio::test]
async fn upgraded_database_serves_all_provider_facades_with_zero_new_depletions() {
    let (db_path, db_str) = build_v44_fixture("mh-facades").await;
    let harness = spawn_harness("mh-facades", db_path, db_str).await;
    let base = harness.base_url.clone();
    let token = harness.token.clone();

    // Tavily contract representative (REST).
    let (status, body) = tavily_search(&base, &token, "mh-tavily").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "tavily REST post-upgrade (body={body})"
    );
    assert_eq!(body["query"], json!("mh-tavily"), "tavily echoes the query");
    assert!(body["results"].is_array(), "tavily results array present");

    // New-provider REST facades.
    assert_eq!(
        serpapi_search(&base, &token, "mh-serpapi").await.0,
        StatusCode::OK,
        "serpapi REST post-upgrade"
    );
    assert_eq!(
        exa_search(&base, &token, "mh-exa").await.0,
        StatusCode::OK,
        "exa REST post-upgrade"
    );
    assert_eq!(
        brave_web_search(&base, &token, "mh-brave").await.0,
        StatusCode::OK,
        "brave REST post-upgrade"
    );

    // New-provider MCP facades. SerpApi is a hosted passthrough (session +
    // notifications/initialized); Exa/Brave are session-less local facades.
    let (init_status, session, _) =
        mcp_post(&base, "/serpapi/mcp", &token, None, &initialize_message(1)).await;
    assert_eq!(init_status, StatusCode::OK, "serpapi MCP initialize");
    let session = session.expect("serpapi MCP session header");
    let (notify_status, _, _) = mcp_post(
        &base,
        "/serpapi/mcp",
        &token,
        Some(&session),
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;
    assert_eq!(notify_status, StatusCode::ACCEPTED, "serpapi MCP initialized");
    assert_eq!(
        mcp_post(
            &base,
            "/serpapi/mcp",
            &token,
            Some(&session),
            &tools_call_message(2, "search", json!({ "q": "mh" })),
        )
        .await
        .0,
        StatusCode::OK,
        "serpapi MCP tools/call"
    );

    assert_eq!(
        mcp_post(&base, "/exa/mcp", &token, None, &initialize_message(3)).await.0,
        StatusCode::OK,
        "exa MCP initialize"
    );
    assert_eq!(
        mcp_post(
            &base,
            "/exa/mcp",
            &token,
            None,
            &tools_call_message(4, "web_search_exa", json!({ "query": "mh" })),
        )
        .await
        .0,
        StatusCode::OK,
        "exa MCP tools/call"
    );
    assert_eq!(
        mcp_post(&base, "/brave/mcp", &token, None, &initialize_message(5)).await.0,
        StatusCode::OK,
        "brave MCP initialize"
    );
    assert_eq!(
        mcp_post(
            &base,
            "/brave/mcp",
            &token,
            None,
            &tools_call_message(6, "brave_web_search", json!({ "query": "mh" })),
        )
        .await
        .0,
        StatusCode::OK,
        "brave MCP tools/call"
    );

    let pool = harness.pool().await;
    for provider in ["serpapi", "exa", "brave"] {
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.request_logs WHERE provider = ?",
        )
        .bind(provider)
        .fetch_one(&pool)
        .await
        .expect("count provider request logs");
        assert!(
            rows >= 1,
            "{provider} facade must stamp a provider request log post-upgrade"
        );
    }

    // M8 negative: low-quota depletion stays Tavily-only. Zero rows for any
    // non-tavily key after the upgrade + facade flows.
    let new_provider_depletions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_low_quota_depletions d \
         JOIN api_keys k ON k.id = d.key_id WHERE k.provider <> 'tavily'",
    )
    .fetch_one(&pool)
    .await
    .expect("count new-provider depletion rows");
    assert_eq!(
        new_provider_depletions, 0,
        "M8: new providers must never write api_key_low_quota_depletions rows"
    );
}

// ---------------------------------------------------------------------------
// Test 4 — HA control baseline export carries the provider column.
// ---------------------------------------------------------------------------

/// Given an upgraded v45 database.
/// When the admin HA control baseline is exported.
/// Then the api_keys resource rows carry `provider='tavily'` (the import-side
/// legacy-NDJSON defaulting is covered by the committed lib test
/// `provider_aware_v45_ha_baseline_legacy_ndjson_defaults_to_tavily`).
#[tokio::test]
async fn ha_baseline_export_from_upgraded_database_carries_provider() {
    let (db_path, db_str) = build_v44_fixture("mh-ha-baseline").await;
    let harness = spawn_harness("mh-ha-baseline", db_path, db_str).await;

    let response = Client::new()
        .get(format!(
            "{}/api/admin/ha/baseline?channel=control",
            harness.base_url
        ))
        .send()
        .await
        .expect("HA baseline export request");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "control baseline export must be servable post-upgrade"
    );
    let encoding = response
        .headers()
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(encoding, "zstd", "baseline export is zstd-encoded");
    let bytes = response.bytes().await.expect("baseline body bytes");
    let decoded = zstd::stream::decode_all(Cursor::new(bytes)).expect("decode zstd baseline");
    let text = String::from_utf8_lossy(&decoded);

    let mut found_api_keys = false;
    let mut found_legacy_tavily = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("kind").and_then(Value::as_str) != Some("resource")
            || value.get("resource").and_then(Value::as_str) != Some("api_keys")
        {
            continue;
        }
        let provider = value
            .get("data")
            .and_then(|data| data.get("provider"))
            .and_then(Value::as_str);
        assert!(
            provider.is_some(),
            "every exported api_keys row must carry the provider column: {line}"
        );
        if provider == Some("tavily") {
            found_legacy_tavily = true;
        }
        found_api_keys = true;
    }
    assert!(
        found_api_keys,
        "control baseline must export api_keys rows with the provider column"
    );
    assert!(
        found_legacy_tavily,
        "the upgraded legacy api_keys row must export as provider='tavily'"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — QA failure scenario: checksum tamper rejected at startup.
// ---------------------------------------------------------------------------

/// Given an upgraded v45 database whose v45 checksum is tampered.
/// When the real server binary starts against it.
/// Then startup is rejected with the typed checksum-mismatch error.
#[tokio::test]
async fn checksum_tamper_on_upgraded_database_is_rejected_at_startup() {
    let (db_path, db_str) = build_v44_fixture("mh-tamper").await;

    // Upgrade the fixture to v45 (minimal single-provider server; no mocks).
    let upgrade_port = reserve_local_port();
    let dead_port = reserve_local_port();
    let upgrade_child = base_server_command(
        &db_str,
        upgrade_port,
        &format!("http://127.0.0.1:{dead_port}/mcp"),
        &format!("http://127.0.0.1:{dead_port}"),
    )
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .expect("spawn warm upgrade server");
    assert!(
        health_ready_within(upgrade_port, 30).await,
        "warm upgrade server never became healthy"
    );
    drop(ChildGuard { child: upgrade_child });
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Tamper the recorded v45 checksum.
    {
        let pool = connect_sqlite_test_pool(&db_str).await;
        let updated = sqlx::query(
            "UPDATE schema_migrations \
             SET checksum = 'sha256:0000000000000000000000000000000000000000000000000000000000000000' \
             WHERE version = 45",
        )
        .execute(&pool)
        .await
        .expect("tamper v45 checksum");
        assert_eq!(updated.rows_affected(), 1, "v45 row must exist to tamper");
        pool.close().await;
    }

    // Startup must fail closed with the typed error.
    let fail_port = reserve_local_port();
    let child = base_server_command(
        &db_str,
        fail_port,
        &format!("http://127.0.0.1:{dead_port}/mcp"),
        &format!("http://127.0.0.1:{dead_port}"),
    )
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn tamper-check server");
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::task::spawn_blocking(move || child.wait_with_output()),
    )
    .await
    .expect("tampered startup must fail fast")
    .expect("join tamper wait")
    .expect("wait tamper child");
    assert!(
        !output.status.success(),
        "tampered v45 checksum must reject startup"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("schema migration checksum mismatch at version 45"),
        "startup must surface the typed checksum error; stderr was: {stderr}"
    );

    remove_sqlite_files(&db_path);
}
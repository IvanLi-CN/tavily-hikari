use std::{
    collections::{HashMap, HashSet},
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    Router,
    body::Body,
    extract::{Json, Query},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::any,
};
use chrono::Utc;
use nanoid::nanoid;
use reqwest::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tavily_hikari::{
    TOKEN_DAILY_LIMIT, TOKEN_HOURLY_LIMIT, TOKEN_HOURLY_REQUEST_LIMIT, TOKEN_MONTHLY_LIMIT,
};
use tokio::net::TcpListener;

fn temp_db_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}.db", nanoid!(8)))
}

fn reserve_local_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind random port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

struct BackendGuard {
    child: Child,
    db_path: PathBuf,
}

impl Drop for BackendGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.db_path);
        let _ = std::fs::remove_file(format!("{}-wal", self.db_path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", self.db_path.display()));
    }
}

fn spawn_backend_process(
    keys: &[String],
    upstream: &str,
    usage_base: &str,
    port: u16,
    db_path: &Path,
) -> BackendGuard {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tavily-hikari"));
    cmd.arg("--bind")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--db-path")
        .arg(db_path)
        .arg("--upstream")
        .arg(upstream)
        .arg("--usage-base")
        .arg(usage_base)
        .arg("--dev-open-admin");
    for key in keys {
        cmd.arg("--keys").arg(key);
    }

    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn tavily-hikari");

    BackendGuard {
        child,
        db_path: db_path.to_path_buf(),
    }
}

async fn wait_for_health(port: u16) {
    let client = Client::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() > deadline {
            panic!("proxy did not become healthy in time on port {port}");
        }

        if let Ok(response) = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            && response.status().is_success()
        {
            return;
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn connect_sqlite_test_pool(db_path: &Path) -> sqlx::SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
        .expect("connect sqlite pool")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MockMcpCall {
    method: String,
    upstream_api_key: String,
    upstream_session_id_header: Option<String>,
    issued_upstream_session_id: Option<String>,
}

struct MockMcpUpstream {
    addr: SocketAddr,
    calls: Arc<Mutex<Vec<MockMcpCall>>>,
}

impl MockMcpUpstream {
    async fn spawn(allowed_api_keys: Vec<String>) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let session_seq = Arc::new(AtomicUsize::new(1));
        let app = Router::new().route(
            "/mcp",
            any({
                let calls = calls.clone();
                let session_seq = session_seq.clone();
                move |headers: HeaderMap,
                      Query(params): Query<HashMap<String, String>>,
                      Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let session_seq = session_seq.clone();
                    let allowed_api_keys = allowed_api_keys.clone();
                    async move {
                        let upstream_api_key = params
                            .get("tavilyApiKey")
                            .cloned()
                            .expect("missing tavilyApiKey");
                        assert!(
                            allowed_api_keys.iter().any(|allowed| allowed == &upstream_api_key),
                            "unexpected upstream key {upstream_api_key}"
                        );

                        let method = body
                            .get("method")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let upstream_session_id_header = headers
                            .get("mcp-session-id")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        let issued_upstream_session_id = if method == "initialize" {
                            Some(format!(
                                "upstream-session-{}",
                                session_seq.fetch_add(1, Ordering::SeqCst)
                            ))
                        } else {
                            None
                        };

                        calls
                            .lock()
                            .expect("mock mcp calls lock poisoned")
                            .push(MockMcpCall {
                                method: method.clone(),
                                upstream_api_key: upstream_api_key.clone(),
                                upstream_session_id_header: upstream_session_id_header.clone(),
                                issued_upstream_session_id: issued_upstream_session_id.clone(),
                            });

                        match method.as_str() {
                            "initialize" => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .header(
                                    "mcp-session-id",
                                    issued_upstream_session_id
                                        .as_deref()
                                        .expect("initialize must issue session id"),
                                )
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "result": {
                                            "protocolVersion": "2025-03-26",
                                            "serverInfo": { "name": "mock-mcp", "version": "1.0.0" },
                                            "capabilities": {}
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build initialize response"),
                            "notifications/initialized" => Response::builder()
                                .status(StatusCode::ACCEPTED)
                                .body(Body::empty())
                                .expect("build initialized response"),
                            "tools/list" => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "result": {
                                            "tools": [
                                                { "name": "tavily_search", "description": "mock tool" }
                                            ]
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build tools/list response"),
                            other => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "result": {
                                            "echoMethod": other,
                                            "status": 200
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build generic response"),
                        }
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock mcp upstream");
        let addr = listener.local_addr().expect("mock mcp upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .expect("serve mock mcp upstream");
        });

        Self { addr, calls }
    }

    async fn spawn_with_hot_key_rate_limit(
        allowed_api_keys: Vec<String>,
        hot_key: Arc<Mutex<Option<String>>>,
        retry_after_secs: i64,
    ) -> Self {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let session_seq = Arc::new(AtomicUsize::new(1));
        let app = Router::new().route(
            "/mcp",
            any({
                let calls = calls.clone();
                let session_seq = session_seq.clone();
                move |headers: HeaderMap,
                      Query(params): Query<HashMap<String, String>>,
                      Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let session_seq = session_seq.clone();
                    let allowed_api_keys = allowed_api_keys.clone();
                    let hot_key = hot_key.clone();
                    async move {
                        let upstream_api_key = params
                            .get("tavilyApiKey")
                            .cloned()
                            .expect("missing tavilyApiKey");
                        assert!(
                            allowed_api_keys.iter().any(|allowed| allowed == &upstream_api_key),
                            "unexpected upstream key {upstream_api_key}"
                        );

                        let method = body
                            .get("method")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let upstream_session_id_header = headers
                            .get("mcp-session-id")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        let issued_upstream_session_id = if method == "initialize" {
                            Some(format!(
                                "upstream-session-{}",
                                session_seq.fetch_add(1, Ordering::SeqCst)
                            ))
                        } else {
                            None
                        };

                        calls
                            .lock()
                            .expect("mock mcp calls lock poisoned")
                            .push(MockMcpCall {
                                method: method.clone(),
                                upstream_api_key: upstream_api_key.clone(),
                                upstream_session_id_header: upstream_session_id_header.clone(),
                                issued_upstream_session_id: issued_upstream_session_id.clone(),
                            });

                        match method.as_str() {
                            "initialize" => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .header(
                                    "mcp-session-id",
                                    issued_upstream_session_id
                                        .as_deref()
                                        .expect("initialize must issue session id"),
                                )
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "result": {
                                            "protocolVersion": "2025-03-26",
                                            "serverInfo": { "name": "mock-mcp", "version": "1.0.0" },
                                            "capabilities": {}
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build initialize response"),
                            "notifications/initialized" => Response::builder()
                                .status(StatusCode::ACCEPTED)
                                .body(Body::empty())
                                .expect("build initialized response"),
                            "tools/list"
                                if hot_key
                                    .lock()
                                    .expect("hot key lock poisoned")
                                    .as_deref()
                                    == Some(upstream_api_key.as_str()) =>
                            {
                                Response::builder()
                                .status(StatusCode::TOO_MANY_REQUESTS)
                                .header("content-type", "application/json")
                                .header("retry-after", retry_after_secs.to_string())
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "error": {
                                            "code": 429,
                                            "message": "Your request has been blocked due to excessive requests."
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build rate-limited response")
                            }
                            "tools/list" => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "result": {
                                            "tools": [
                                                { "name": "tavily_search", "description": "mock tool" }
                                            ]
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build tools/list response"),
                            other => Response::builder()
                                .status(StatusCode::OK)
                                .header("content-type", "application/json")
                                .body(Body::from(
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                        "result": {
                                            "echoMethod": other,
                                            "status": 200
                                        }
                                    })
                                    .to_string(),
                                ))
                                .expect("build generic response"),
                        }
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock mcp upstream");
        let addr = listener.local_addr().expect("mock mcp upstream addr");
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .expect("serve mock mcp upstream");
        });

        Self { addr, calls }
    }
}

async fn create_test_token(base_url: &str) -> String {
    Client::new()
        .post(format!("{base_url}/api/tokens"))
        .json(&json!({}))
        .send()
        .await
        .expect("create token request")
        .error_for_status()
        .expect("create token status")
        .json::<Value>()
        .await
        .expect("decode token payload")
        .get("token")
        .and_then(|value| value.as_str())
        .expect("token string")
        .to_string()
}

fn token_id_from_secret(token: &str) -> &str {
    token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(token_id, _)| token_id))
        .expect("token id embedded in secret")
}

async fn get_settings(base_url: &str) -> Value {
    Client::new()
        .get(format!("{base_url}/api/settings"))
        .send()
        .await
        .expect("get settings")
        .error_for_status()
        .expect("settings status")
        .json::<Value>()
        .await
        .expect("decode settings payload")
}

async fn put_affinity_count(base_url: &str, count: i64) -> Value {
    Client::new()
        .put(format!("{base_url}/api/settings/system"))
        .json(&json!({
            "mcpSessionAffinityKeyCount": count,
        }))
        .send()
        .await
        .expect("put system settings")
        .error_for_status()
        .expect("put system settings status")
        .json::<Value>()
        .await
        .expect("decode system settings payload")
}

#[derive(Debug, Clone)]
struct McpSessionInit {
    proxy_session_id: String,
    protocol_version: String,
}

async fn initialize_mcp_session(base_url: &str, token: &str, label: &str) -> McpSessionInit {
    let response = Client::new()
        .post(format!("{base_url}/mcp?tavilyApiKey={token}"))
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": format!("init-{label}"),
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": { "name": "e2e", "version": "1.0.0" }
            }
        }))
        .send()
        .await
        .expect("initialize request");
    let status = response.status();
    let headers = response.headers().clone();
    let body_bytes = response.bytes().await.expect("initialize body");
    if !status.is_success() {
        panic!(
            "initialize status {status}: {}",
            String::from_utf8_lossy(&body_bytes)
        );
    }
    let proxy_session_id = headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .expect("proxy mcp-session-id header")
        .to_string();
    let body = serde_json::from_slice::<Value>(&body_bytes).expect("decode initialize payload");
    let protocol_version = body
        .get("result")
        .and_then(|value| value.get("protocolVersion"))
        .and_then(|value| value.as_str())
        .unwrap_or("2025-03-26")
        .to_string();
    McpSessionInit {
        proxy_session_id,
        protocol_version,
    }
}

async fn notify_initialized(base_url: &str, token: &str, session: &McpSessionInit) {
    let response = Client::new()
        .post(format!("{base_url}/mcp?tavilyApiKey={token}"))
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header("mcp-session-id", &session.proxy_session_id)
        .header("mcp-protocol-version", &session.protocol_version)
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .send()
        .await
        .expect("notifications/initialized request");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::ACCEPTED,
        "notifications/initialized should return 202"
    );
}

async fn list_tools(base_url: &str, token: &str, session: &McpSessionInit) -> Value {
    list_tools_raw(base_url, token, session)
        .await
        .error_for_status()
        .expect("tools/list status")
        .json::<Value>()
        .await
        .expect("decode tools/list payload")
}

async fn list_tools_raw(
    base_url: &str,
    token: &str,
    session: &McpSessionInit,
) -> reqwest::Response {
    Client::new()
        .post(format!("{base_url}/mcp?tavilyApiKey={token}"))
        .header("accept", "application/json, text/event-stream")
        .header("content-type", "application/json")
        .header("mcp-session-id", &session.proxy_session_id)
        .header("mcp-protocol-version", &session.protocol_version)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": format!("tools-{}", session.proxy_session_id),
            "method": "tools/list"
        }))
        .send()
        .await
        .expect("tools/list request")
}

#[derive(Debug, Clone)]
struct ApiKeyRecord {
    id: String,
    secret: String,
}

async fn fetch_api_key_records(db_path: &Path) -> Vec<ApiKeyRecord> {
    let pool = connect_sqlite_test_pool(db_path).await;
    sqlx::query("SELECT id, api_key FROM api_keys WHERE deleted_at IS NULL ORDER BY id ASC")
        .fetch_all(&pool)
        .await
        .expect("fetch api key records")
        .into_iter()
        .map(|row| ApiKeyRecord {
            id: row.try_get("id").expect("api key id"),
            secret: row.try_get("api_key").expect("api key secret"),
        })
        .collect()
}

#[derive(Debug, Clone)]
struct McpSessionRecord {
    proxy_session_id: String,
    upstream_session_id: String,
    upstream_key_id: String,
    revoked_at: Option<i64>,
}

async fn fetch_mcp_session_record(db_path: &Path, proxy_session_id: &str) -> McpSessionRecord {
    let pool = connect_sqlite_test_pool(db_path).await;
    let row = sqlx::query(
        r#"
        SELECT proxy_session_id, upstream_session_id, upstream_key_id, revoked_at
        FROM mcp_sessions
        WHERE proxy_session_id = ?
        LIMIT 1
        "#,
    )
    .bind(proxy_session_id)
    .fetch_one(&pool)
    .await
    .expect("fetch mcp session record");

    McpSessionRecord {
        proxy_session_id: row.try_get("proxy_session_id").expect("proxy session id"),
        upstream_session_id: row
            .try_get("upstream_session_id")
            .expect("upstream session id"),
        upstream_key_id: row.try_get("upstream_key_id").expect("upstream key id"),
        revoked_at: row.try_get("revoked_at").expect("revoked_at"),
    }
}

fn rank_top_key_ids(
    subject: &str,
    key_records: &[ApiKeyRecord],
    desired_count: usize,
) -> Vec<String> {
    let mut ranked = key_records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        affinity_score(subject, right)
            .cmp(&affinity_score(subject, left))
            .then_with(|| left.cmp(right))
    });
    ranked.truncate(desired_count.max(1).min(ranked.len()));
    ranked
}

fn affinity_score(subject: &str, key_id: &str) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(subject.as_bytes());
    digest.update(b":");
    digest.update(key_id.as_bytes());
    digest.finalize().into()
}

fn secret_for_key_id(key_records: &[ApiKeyRecord], key_id: &str) -> String {
    key_records
        .iter()
        .find(|record| record.id == key_id)
        .map(|record| record.secret.clone())
        .unwrap_or_else(|| panic!("missing secret for key id {key_id}"))
}

fn fixed_token_secret(seed: usize) -> String {
    format!("secret{seed:018}")
}

async fn insert_user(pool: &sqlx::SqlitePool, user_id: &str) {
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO users (id, display_name, username, avatar_template, active, created_at, updated_at, last_login_at)
        VALUES (?, ?, ?, NULL, 1, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(format!("User {user_id}"))
    .bind(format!("user_{user_id}"))
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert user");
}

async fn insert_default_account_quota_limits(pool: &sqlx::SqlitePool, user_id: &str) {
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO account_quota_limits (
            user_id,
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            monthly_broken_limit,
            inherits_defaults,
            created_at,
            updated_at
        )
        VALUES (?, ?, ?, ?, ?, 5, 1, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(TOKEN_HOURLY_REQUEST_LIMIT)
    .bind(TOKEN_HOURLY_LIMIT)
    .bind(TOKEN_DAILY_LIMIT)
    .bind(TOKEN_MONTHLY_LIMIT)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert default account quota limits");
}

async fn insert_bound_token(pool: &sqlx::SqlitePool, user_id: &str, token_id: &str, secret: &str) {
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO auth_tokens
            (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
        VALUES
            (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)
        "#,
    )
    .bind(token_id)
    .bind(secret)
    .bind(format!("bound:{user_id}:{token_id}"))
    .bind(now)
    .execute(pool)
    .await
    .expect("insert auth token");

    sqlx::query(
        r#"
        INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at)
        VALUES (?, ?, ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(token_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("insert user token binding");
}

fn choose_token_ids_different_from_user_pool(
    key_records: &[ApiKeyRecord],
    user_id: &str,
) -> Vec<(String, Vec<String>)> {
    let expected_user_top2 = rank_top_key_ids(&format!("user:{user_id}"), key_records, 2);
    let expected_user_top2_set = expected_user_top2.iter().cloned().collect::<HashSet<_>>();
    let mut found = Vec::new();
    for idx in 0..4096usize {
        let token_id = format!("{idx:04x}");
        let token_top2 = rank_top_key_ids(&format!("token:{token_id}"), key_records, 2);
        let token_top2_set = token_top2.iter().cloned().collect::<HashSet<_>>();
        if token_top2_set != expected_user_top2_set {
            found.push((token_id, token_top2));
            if found.len() == 2 {
                break;
            }
        }
    }
    found
}

fn count_by_secret(values: &[String]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value.clone()).or_insert(0) += 1;
    }
    counts
}

#[tokio::test]
async fn mcp_session_affinity_settings_rebalance_new_sessions_without_breaking_existing_ones() {
    let upstream_keys = vec![
        "tvly-e2e-affinity-a".to_string(),
        "tvly-e2e-affinity-b".to_string(),
        "tvly-e2e-affinity-c".to_string(),
        "tvly-e2e-affinity-d".to_string(),
    ];
    let upstream = MockMcpUpstream::spawn(upstream_keys.clone()).await;
    let db_path = temp_db_path("mcp-session-affinity-settings-e2e");
    let port = reserve_local_port();
    let upstream_url = format!("http://{}/mcp", upstream.addr);
    let usage_base = format!("http://{}", upstream.addr);
    let _backend =
        spawn_backend_process(&upstream_keys, &upstream_url, &usage_base, port, &db_path);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let settings = get_settings(&base_url).await;
    assert_eq!(
        settings["systemSettings"]["mcpSessionAffinityKeyCount"].as_i64(),
        Some(5),
        "default affinity pool size should stay 5"
    );
    assert_eq!(
        settings["systemSettings"]["requestRateLimit"].as_i64(),
        Some(100),
        "default request-rate limit should stay 100"
    );

    let token = create_test_token(&base_url).await;
    let token_id = token_id_from_secret(&token).to_string();
    let key_records = fetch_api_key_records(&db_path).await;
    let expected_top2_ids = rank_top_key_ids(&format!("token:{token_id}"), &key_records, 2);
    let expected_top3_ids = rank_top_key_ids(&format!("token:{token_id}"), &key_records, 3);
    let expected_top2_secrets = expected_top2_ids
        .iter()
        .map(|key_id| secret_for_key_id(&key_records, key_id))
        .collect::<HashSet<_>>();
    let expected_third_secret = secret_for_key_id(&key_records, &expected_top3_ids[2]);

    let updated = put_affinity_count(&base_url, 2).await;
    assert_eq!(
        updated["mcpSessionAffinityKeyCount"].as_i64(),
        Some(2),
        "system settings should apply immediately"
    );
    assert_eq!(
        updated["requestRateLimit"].as_i64(),
        Some(100),
        "legacy affinity-only payload should preserve request-rate limit"
    );

    let mut initial_secrets = Vec::new();
    for idx in 0..4 {
        let session = initialize_mcp_session(&base_url, &token, &format!("initial-{idx}")).await;
        notify_initialized(&base_url, &token, &session).await;
        let row = fetch_mcp_session_record(&db_path, &session.proxy_session_id).await;
        initial_secrets.push(secret_for_key_id(&key_records, &row.upstream_key_id));
    }

    assert_eq!(
        initial_secrets.iter().cloned().collect::<HashSet<_>>(),
        expected_top2_secrets,
        "new sessions should stay inside the token's top-2 affinity pool"
    );
    let initial_counts = count_by_secret(&initial_secrets);
    let initial_max = initial_counts.values().copied().max().unwrap_or(0);
    let initial_min = initial_counts.values().copied().min().unwrap_or(0);
    assert!(
        initial_max.saturating_sub(initial_min) <= 1,
        "top-2 pool should be balanced for new sessions: {initial_counts:?}"
    );

    let expanded = put_affinity_count(&base_url, 3).await;
    assert_eq!(
        expanded["mcpSessionAffinityKeyCount"].as_i64(),
        Some(3),
        "increasing affinity pool size should persist"
    );
    assert_eq!(expanded["requestRateLimit"].as_i64(), Some(100));

    let expanded_session = initialize_mcp_session(&base_url, &token, "expanded").await;
    notify_initialized(&base_url, &token, &expanded_session).await;
    let expanded_row = fetch_mcp_session_record(&db_path, &expanded_session.proxy_session_id).await;
    let expanded_secret = secret_for_key_id(&key_records, &expanded_row.upstream_key_id);
    assert_eq!(
        expanded_secret, expected_third_secret,
        "when the pool grows, the newly admitted affinity key should take the next session"
    );

    let shrunken = put_affinity_count(&base_url, 2).await;
    assert_eq!(
        shrunken["mcpSessionAffinityKeyCount"].as_i64(),
        Some(2),
        "shrinking affinity pool size should persist"
    );
    assert_eq!(shrunken["requestRateLimit"].as_i64(), Some(100));

    let mut shrunken_secrets = Vec::new();
    for idx in 0..4 {
        let session = initialize_mcp_session(&base_url, &token, &format!("shrunken-{idx}")).await;
        notify_initialized(&base_url, &token, &session).await;
        let row = fetch_mcp_session_record(&db_path, &session.proxy_session_id).await;
        shrunken_secrets.push(secret_for_key_id(&key_records, &row.upstream_key_id));
    }
    assert!(
        shrunken_secrets
            .iter()
            .all(|secret| expected_top2_secrets.contains(secret)),
        "after shrinking, new sessions should only use the preserved top-2 pool: {shrunken_secrets:?}"
    );
    assert!(
        shrunken_secrets
            .iter()
            .all(|secret| secret != &expected_third_secret),
        "after shrinking, newly created sessions must stop using the dropped third key"
    );

    let expanded_row_after_shrink =
        fetch_mcp_session_record(&db_path, &expanded_session.proxy_session_id).await;
    assert_eq!(
        expanded_row_after_shrink.proxy_session_id, expanded_session.proxy_session_id,
        "existing session row should still be present"
    );
    assert_eq!(
        expanded_row_after_shrink.revoked_at, None,
        "existing sessions must not be revoked by settings changes"
    );

    let tools = list_tools(&base_url, &token, &expanded_session).await;
    assert_eq!(
        tools["result"]["tools"].as_array().map(Vec::len),
        Some(1),
        "existing session should keep working after settings shrink"
    );

    let last_tools_call = upstream
        .calls
        .lock()
        .expect("mock calls lock poisoned")
        .iter()
        .rev()
        .find(|call| {
            call.method == "tools/list"
                && call.upstream_session_id_header.as_deref()
                    == Some(expanded_row_after_shrink.upstream_session_id.as_str())
        })
        .cloned()
        .expect("tools/list call for expanded session");
    assert_eq!(
        last_tools_call.upstream_api_key, expected_third_secret,
        "existing session should remain pinned to its original upstream key"
    );
}

#[tokio::test]
async fn mcp_session_affinity_prefers_user_subject_over_individual_token_subjects() {
    let upstream_keys = vec![
        "tvly-e2e-user-a".to_string(),
        "tvly-e2e-user-b".to_string(),
        "tvly-e2e-user-c".to_string(),
        "tvly-e2e-user-d".to_string(),
    ];
    let upstream = MockMcpUpstream::spawn(upstream_keys.clone()).await;
    let db_path = temp_db_path("mcp-session-affinity-user-e2e");
    let port = reserve_local_port();
    let upstream_url = format!("http://{}/mcp", upstream.addr);
    let usage_base = format!("http://{}", upstream.addr);
    let _backend =
        spawn_backend_process(&upstream_keys, &upstream_url, &usage_base, port, &db_path);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let updated = put_affinity_count(&base_url, 2).await;
    assert_eq!(
        updated["mcpSessionAffinityKeyCount"].as_i64(),
        Some(2),
        "user-priority E2E should run with a 2-key affinity pool"
    );
    assert_eq!(updated["requestRateLimit"].as_i64(), Some(100));

    let key_records = fetch_api_key_records(&db_path).await;
    let user_id = "e2e-user-priority";
    let expected_user_top2_ids = rank_top_key_ids(&format!("user:{user_id}"), &key_records, 2);
    let expected_user_top2_secrets = expected_user_top2_ids
        .iter()
        .map(|key_id| secret_for_key_id(&key_records, key_id))
        .collect::<HashSet<_>>();

    let chosen_tokens = choose_token_ids_different_from_user_pool(&key_records, user_id);
    assert_eq!(
        chosen_tokens.len(),
        2,
        "need two token ids whose token-based pools differ from the user pool"
    );

    let pool = connect_sqlite_test_pool(&db_path).await;
    insert_user(&pool, user_id).await;
    insert_default_account_quota_limits(&pool, user_id).await;
    let (token1_id, token1_top2_ids) = chosen_tokens[0].clone();
    let (token2_id, token2_top2_ids) = chosen_tokens[1].clone();
    let token1_secret = fixed_token_secret(1);
    let token2_secret = fixed_token_secret(2);
    insert_bound_token(&pool, user_id, &token1_id, &token1_secret).await;
    insert_bound_token(&pool, user_id, &token2_id, &token2_secret).await;
    drop(pool);

    let token1 = format!("th-{token1_id}-{token1_secret}");
    let token2 = format!("th-{token2_id}-{token2_secret}");
    let token1_top2_secrets = token1_top2_ids
        .iter()
        .map(|key_id| secret_for_key_id(&key_records, key_id))
        .collect::<HashSet<_>>();
    let token2_top2_secrets = token2_top2_ids
        .iter()
        .map(|key_id| secret_for_key_id(&key_records, key_id))
        .collect::<HashSet<_>>();

    let token1_session_a = initialize_mcp_session(&base_url, &token1, "token1-a").await;
    notify_initialized(&base_url, &token1, &token1_session_a).await;
    let token1_session_b = initialize_mcp_session(&base_url, &token1, "token1-b").await;
    notify_initialized(&base_url, &token1, &token1_session_b).await;
    let token1_secret_a = secret_for_key_id(
        &key_records,
        &fetch_mcp_session_record(&db_path, &token1_session_a.proxy_session_id)
            .await
            .upstream_key_id,
    );
    let token1_secret_b = secret_for_key_id(
        &key_records,
        &fetch_mcp_session_record(&db_path, &token1_session_b.proxy_session_id)
            .await
            .upstream_key_id,
    );
    let token1_used = HashSet::from([token1_secret_a.clone(), token1_secret_b.clone()]);
    assert_eq!(
        token1_used, expected_user_top2_secrets,
        "a bound token should inherit the user's affinity pool instead of its own token hash"
    );
    assert!(
        token1_used
            .iter()
            .any(|secret| !token1_top2_secrets.contains(secret)),
        "token1 sessions should prove the scheduler is using the user subject, not token:{token1_id}"
    );

    let token2_session_a = initialize_mcp_session(&base_url, &token2, "token2-a").await;
    notify_initialized(&base_url, &token2, &token2_session_a).await;
    let token2_session_b = initialize_mcp_session(&base_url, &token2, "token2-b").await;
    notify_initialized(&base_url, &token2, &token2_session_b).await;
    let token2_secret_a = secret_for_key_id(
        &key_records,
        &fetch_mcp_session_record(&db_path, &token2_session_a.proxy_session_id)
            .await
            .upstream_key_id,
    );
    let token2_secret_b = secret_for_key_id(
        &key_records,
        &fetch_mcp_session_record(&db_path, &token2_session_b.proxy_session_id)
            .await
            .upstream_key_id,
    );
    let token2_used = HashSet::from([token2_secret_a.clone(), token2_secret_b.clone()]);
    assert_eq!(
        token2_used, expected_user_top2_secrets,
        "all tokens bound to the same user should share the same user-level affinity pool"
    );
    assert!(
        token2_used
            .iter()
            .any(|secret| !token2_top2_secrets.contains(secret)),
        "token2 sessions should prove the scheduler is using the user subject, not token:{token2_id}"
    );
}

#[tokio::test]
async fn mcp_session_init_avoids_rate_limited_key_for_new_sessions_without_moving_existing_session()
{
    let upstream_keys = vec![
        "tvly-e2e-429-a".to_string(),
        "tvly-e2e-429-b".to_string(),
        "tvly-e2e-429-c".to_string(),
    ];
    let db_path = temp_db_path("mcp-session-init-backoff-e2e");
    let port = reserve_local_port();
    let hot_key = Arc::new(Mutex::new(None));
    let upstream =
        MockMcpUpstream::spawn_with_hot_key_rate_limit(upstream_keys.clone(), hot_key.clone(), 1)
            .await;
    let upstream_url = format!("http://{}/mcp", upstream.addr);
    let usage_base = format!("http://{}", upstream.addr);
    let _backend =
        spawn_backend_process(&upstream_keys, &upstream_url, &usage_base, port, &db_path);
    wait_for_health(port).await;

    let key_records = fetch_api_key_records(&db_path).await;
    let base_url = format!("http://127.0.0.1:{port}");
    let token = create_test_token(&base_url).await;
    let updated = put_affinity_count(&base_url, 2).await;
    assert_eq!(updated["mcpSessionAffinityKeyCount"].as_i64(), Some(2));
    assert_eq!(updated["requestRateLimit"].as_i64(), Some(100));

    let expected_top2_ids = rank_top_key_ids(
        &format!("token:{}", token_id_from_secret(&token)),
        &key_records,
        2,
    );
    let hot_key_id = expected_top2_ids[0].clone();
    let cool_key_id = expected_top2_ids[1].clone();
    let hot_key_secret = secret_for_key_id(&key_records, &hot_key_id);
    let cool_key_secret = secret_for_key_id(&key_records, &cool_key_id);
    *hot_key.lock().expect("set hot key lock poisoned") = Some(hot_key_secret.clone());

    let session_hot = initialize_mcp_session(&base_url, &token, "hot").await;
    notify_initialized(&base_url, &token, &session_hot).await;
    let hot_row = fetch_mcp_session_record(&db_path, &session_hot.proxy_session_id).await;
    assert_eq!(hot_row.upstream_key_id, hot_key_id);

    let first_rate_limited = list_tools_raw(&base_url, &token, &session_hot).await;
    let first_status = first_rate_limited.status();
    let first_body = first_rate_limited
        .text()
        .await
        .expect("read first 429 body");
    assert_eq!(first_status, reqwest::StatusCode::TOO_MANY_REQUESTS);
    assert!(
        first_body.contains("excessive requests"),
        "expected upstream 429 body, got {first_body}"
    );

    let pool = connect_sqlite_test_pool(&db_path).await;
    let (request_key_effect, request_failure_kind): (String, String) = sqlx::query_as(
        r#"
        SELECT key_effect_code, failure_kind
        FROM request_logs
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("fetch 429 request log");
    assert_eq!(request_failure_kind, "upstream_rate_limited_429");
    assert_eq!(request_key_effect, "mcp_session_init_backoff_set");

    let (cooldown_until, retry_after_secs): (i64, i64) = sqlx::query_as(
        r#"
        SELECT cooldown_until, retry_after_secs
        FROM api_key_transient_backoffs
        WHERE key_id = ? AND scope = 'mcp_session_init'
        LIMIT 1
        "#,
    )
    .bind(&hot_key_id)
    .fetch_one(&pool)
    .await
    .expect("fetch hot key cooldown row");
    assert!(cooldown_until > Utc::now().timestamp());
    assert_eq!(retry_after_secs, 30);

    let session_cool = initialize_mcp_session(&base_url, &token, "cool").await;

    let (initialize_api_key_id, initialize_key_effect, initialize_selection_effect): (
        String,
        String,
        String,
    ) = sqlx::query_as(
        r#"
        SELECT api_key_id, key_effect_code, selection_effect_code
        FROM request_logs
        WHERE request_kind_key = 'mcp:initialize'
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("fetch initialize log");
    assert_eq!(initialize_api_key_id, cool_key_id);
    assert_eq!(initialize_key_effect, "none");
    assert_eq!(
        initialize_selection_effect,
        "mcp_session_init_cooldown_avoided"
    );

    notify_initialized(&base_url, &token, &session_cool).await;
    let cool_row = fetch_mcp_session_record(&db_path, &session_cool.proxy_session_id).await;
    assert_eq!(cool_row.upstream_key_id, cool_key_id);

    let cool_tools = list_tools(&base_url, &token, &session_cool).await;
    assert_eq!(
        cool_tools["result"]["tools"].as_array().map(Vec::len),
        Some(1),
        "cool session should succeed on the alternate key"
    );

    let second_rate_limited = list_tools_raw(&base_url, &token, &session_hot).await;
    assert_eq!(
        second_rate_limited.status(),
        reqwest::StatusCode::TOO_MANY_REQUESTS
    );

    let recorded = upstream
        .calls
        .lock()
        .expect("mock calls lock poisoned")
        .clone();
    let hot_session_calls = recorded
        .iter()
        .filter(|call| {
            call.upstream_session_id_header.as_deref() == Some(hot_row.upstream_session_id.as_str())
                && call.method == "tools/list"
        })
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        hot_session_calls.len(),
        4,
        "existing hot session should keep using the same upstream session across retry-after retries"
    );
    assert!(
        hot_session_calls
            .iter()
            .all(|call| call.upstream_api_key == hot_key_secret),
        "existing hot session must stay pinned to the original upstream key"
    );
    assert!(
        recorded.iter().any(|call| {
            call.method == "initialize" && call.upstream_api_key == cool_key_secret
        }),
        "new initialize should switch to the cooled pool peer"
    );
}

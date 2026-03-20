use std::{
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{Json, Router, extract::Query, http::StatusCode, routing::any};
use nanoid::nanoid;
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
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

async fn connect_sqlite_test_pool(db_path: &str) -> sqlx::SqlitePool {
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

struct ProxyProcess {
    child: Child,
}

impl Drop for ProxyProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_proxy_process(db_path: &str, upstream: &str, port: u16) -> ProxyProcess {
    let child = Command::new(env!("CARGO_BIN_EXE_tavily-hikari"))
        .env("TAVILY_API_KEYS", "tvly-test-key")
        .env("TAVILY_UPSTREAM", upstream)
        .env("TAVILY_USAGE_BASE", upstream.trim_end_matches("/mcp"))
        .env("PROXY_BIND", "127.0.0.1")
        .env("PROXY_PORT", port.to_string())
        .env("PROXY_DB_PATH", db_path)
        .env("DEV_OPEN_ADMIN", "true")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn tavily-hikari");

    ProxyProcess { child }
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

async fn create_test_token(base_url: &str) -> Value {
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
}

fn token_id_from_secret(token: &str) -> &str {
    token
        .strip_prefix("th-")
        .and_then(|rest| rest.split_once('-').map(|(token_id, _)| token_id))
        .expect("token id embedded in secret")
}

async fn fetch_latest_token_log_credits(pool: &sqlx::SqlitePool, token_id: &str) -> Option<i64> {
    let row = sqlx::query(
        "SELECT business_credits FROM auth_token_logs WHERE token_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(token_id)
    .fetch_one(pool)
    .await
    .expect("fetch latest token log");

    row.try_get::<Option<i64>, _>("business_credits")
        .expect("read business_credits")
}

async fn fetch_latest_request_body(pool: &sqlx::SqlitePool, token_id: &str) -> Value {
    let row = sqlx::query(
        "SELECT request_body FROM request_logs WHERE auth_token_id = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(token_id)
    .fetch_one(pool)
    .await
    .expect("fetch latest request log");

    let request_body = row
        .try_get::<Vec<u8>, _>("request_body")
        .expect("read request_body");
    serde_json::from_slice::<Value>(&request_body).expect("decode request body json")
}

async fn fetch_token_monthly_used(pool: &sqlx::SqlitePool, token_id: &str) -> i64 {
    sqlx::query(
        "SELECT COALESCE((SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1), 0) AS month_count",
    )
        .bind(token_id)
        .fetch_one(pool)
        .await
        .expect("fetch token quota row")
        .try_get::<i64, _>("month_count")
        .expect("read month_count")
}

async fn spawn_mock_mcp_upstream_for_tool(
    expected_api_key: String,
    expected_tool_name: &'static str,
    include_usage: bool,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/mcp",
        any({
            let hits = hits.clone();
            move |Query(params): Query<std::collections::HashMap<String, String>>,
                  Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let received = params.get("tavilyApiKey").cloned();
                    assert_eq!(
                        received.as_deref(),
                        Some(expected_api_key.as_str()),
                        "missing or incorrect tavilyApiKey"
                    );
                    assert_eq!(
                        body.get("method").and_then(|value| value.as_str()),
                        Some("tools/call"),
                        "expected MCP tools/call",
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|params| params.get("name"))
                            .and_then(|value| value.as_str()),
                        Some(expected_tool_name),
                        "unexpected forwarded tool name",
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|params| params.get("arguments"))
                            .and_then(|arguments| arguments.get("include_usage"))
                            .and_then(|value| value.as_bool()),
                        Some(true),
                        "proxy should inject include_usage=true",
                    );

                    let mut structured_content = serde_json::Map::new();
                    structured_content.insert("status".into(), Value::Number(200.into()));
                    structured_content.insert("echo".into(), body.clone());
                    if include_usage {
                        let credits = if body
                            .get("params")
                            .and_then(|params| params.get("arguments"))
                            .and_then(|arguments| arguments.get("search_depth"))
                            .and_then(|value| value.as_str())
                            .is_some_and(|depth| depth.eq_ignore_ascii_case("advanced"))
                        {
                            2
                        } else {
                            1
                        };
                        structured_content.insert("usage".into(), json!({ "credits": credits }));
                    }

                    (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                            "result": {
                                "structuredContent": Value::Object(structured_content),
                            }
                        })),
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock upstream");
    let addr = listener.local_addr().expect("mock upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve mock upstream");
    });
    (addr, hits)
}

async fn spawn_mock_mcp_upstream_retrying_include_usage(
    expected_api_key: String,
    expected_tool_name: &'static str,
    fallback_credits: Option<i64>,
) -> (SocketAddr, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new().route(
        "/mcp",
        any({
            let hits = hits.clone();
            move |Query(params): Query<std::collections::HashMap<String, String>>,
                  Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let received = params.get("tavilyApiKey").cloned();
                    assert_eq!(
                        received.as_deref(),
                        Some(expected_api_key.as_str()),
                        "missing or incorrect tavilyApiKey"
                    );
                    assert_eq!(
                        body.get("method").and_then(|value| value.as_str()),
                        Some("tools/call"),
                        "expected MCP tools/call",
                    );
                    assert_eq!(
                        body.get("params")
                            .and_then(|params| params.get("name"))
                            .and_then(|value| value.as_str()),
                        Some(expected_tool_name),
                        "unexpected forwarded tool name",
                    );

                    let include_usage = body
                        .get("params")
                        .and_then(|params| params.get("arguments"))
                        .and_then(|arguments| arguments.get("include_usage"))
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false);

                    if include_usage {
                        return (
                            StatusCode::OK,
                            Json(json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                                "result": {
                                    "content": [{
                                        "type": "text",
                                        "text": format!(
                                            "Internal error: 1 validation error for call[{expected_tool_name}]\ninclude_usage\n  Unexpected keyword argument [type=unexpected_keyword_argument, input_value=True, input_type=bool]"
                                        )
                                    }],
                                    "isError": true
                                }
                            })),
                        );
                    }

                    let mut structured_content = serde_json::Map::new();
                    structured_content.insert("status".into(), Value::Number(200.into()));
                    structured_content.insert("echo".into(), body.clone());
                    if let Some(credits) = fallback_credits {
                        structured_content.insert("usage".into(), json!({ "credits": credits }));
                    }

                    (
                        StatusCode::OK,
                        Json(json!({
                            "jsonrpc": "2.0",
                            "id": body.get("id").cloned().unwrap_or_else(|| json!(1)),
                            "result": {
                                "structuredContent": Value::Object(structured_content),
                            }
                        })),
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind retry mock upstream");
    let addr = listener.local_addr().expect("retry mock upstream addr");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve retry mock upstream");
    });
    (addr, hits)
}

#[tokio::test]
async fn mcp_search_with_underscore_tool_injects_usage_and_bills() {
    let db_path = temp_db_path("mcp-search-underscore-usage");
    let db_str = db_path.to_string_lossy().to_string();
    let (upstream_addr, hits) =
        spawn_mock_mcp_upstream_for_tool("tvly-test-key".to_string(), "tavily_search", true).await;
    let port = reserve_local_port();
    let upstream = format!("http://{upstream_addr}/mcp");
    let _proxy = spawn_proxy_process(&db_str, &upstream, port);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let token_payload = create_test_token(&base_url).await;
    let token = token_payload["token"].as_str().expect("token secret");
    let token_id = token_id_from_secret(token);

    let response = Client::new()
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "search-usage",
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "smoke regression",
                    "search_depth": "advanced"
                }
            }
        }))
        .send()
        .await
        .expect("send search request")
        .error_for_status()
        .expect("search status")
        .json::<Value>()
        .await
        .expect("decode search response");

    assert_eq!(
        response["result"]["structuredContent"]["echo"]["params"]["arguments"]["include_usage"],
        Value::Bool(true)
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let credits = fetch_latest_token_log_credits(&pool, token_id).await;
    let request_body = fetch_latest_request_body(&pool, token_id).await;
    assert_eq!(credits, Some(2));
    assert_eq!(
        request_body["params"]["name"].as_str(),
        Some("tavily_search")
    );
    assert_eq!(
        request_body["params"]["arguments"]["include_usage"],
        Value::Bool(true)
    );
    assert_eq!(fetch_token_monthly_used(&pool, token_id).await, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mcp_search_with_underscore_tool_uses_missing_usage_fallback() {
    let db_path = temp_db_path("mcp-search-underscore-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let (upstream_addr, hits) =
        spawn_mock_mcp_upstream_for_tool("tvly-test-key".to_string(), "tavily_search", false).await;
    let port = reserve_local_port();
    let upstream = format!("http://{upstream_addr}/mcp");
    let _proxy = spawn_proxy_process(&db_str, &upstream, port);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let token_payload = create_test_token(&base_url).await;
    let token = token_payload["token"].as_str().expect("token secret");
    let token_id = token_id_from_secret(token);

    Client::new()
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "search-fallback",
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "smoke regression",
                    "search_depth": "advanced"
                }
            }
        }))
        .send()
        .await
        .expect("send search fallback request")
        .error_for_status()
        .expect("search fallback status");

    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let credits = fetch_latest_token_log_credits(&pool, token_id).await;
    let request_body = fetch_latest_request_body(&pool, token_id).await;
    assert_eq!(credits, Some(2));
    assert_eq!(
        request_body["params"]["arguments"]["include_usage"],
        Value::Bool(true)
    );
    assert_eq!(fetch_token_monthly_used(&pool, token_id).await, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mcp_extract_with_underscore_tool_injects_usage_but_skips_missing_usage_charge() {
    let db_path = temp_db_path("mcp-extract-underscore-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let (upstream_addr, hits) =
        spawn_mock_mcp_upstream_for_tool("tvly-test-key".to_string(), "tavily_extract", false)
            .await;
    let port = reserve_local_port();
    let upstream = format!("http://{upstream_addr}/mcp");
    let _proxy = spawn_proxy_process(&db_str, &upstream, port);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let token_payload = create_test_token(&base_url).await;
    let token = token_payload["token"].as_str().expect("token secret");
    let token_id = token_id_from_secret(token);

    Client::new()
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "extract-fallback",
            "method": "tools/call",
            "params": {
                "name": "tavily_extract",
                "arguments": {
                    "urls": ["https://example.com"]
                }
            }
        }))
        .send()
        .await
        .expect("send extract request")
        .error_for_status()
        .expect("extract status");

    assert_eq!(hits.load(Ordering::SeqCst), 1);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let credits = fetch_latest_token_log_credits(&pool, token_id).await;
    let request_body = fetch_latest_request_body(&pool, token_id).await;
    assert_eq!(credits, None);
    assert_eq!(
        request_body["params"]["name"].as_str(),
        Some("tavily_extract")
    );
    assert_eq!(
        request_body["params"]["arguments"]["include_usage"],
        Value::Bool(true)
    );
    assert_eq!(fetch_token_monthly_used(&pool, token_id).await, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mcp_search_retries_without_include_usage_when_upstream_rejects_it() {
    let db_path = temp_db_path("mcp-search-include-usage-retry");
    let db_str = db_path.to_string_lossy().to_string();
    let (upstream_addr, hits) = spawn_mock_mcp_upstream_retrying_include_usage(
        "tvly-test-key".to_string(),
        "tavily_search",
        None,
    )
    .await;
    let port = reserve_local_port();
    let upstream = format!("http://{upstream_addr}/mcp");
    let _proxy = spawn_proxy_process(&db_str, &upstream, port);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let token_payload = create_test_token(&base_url).await;
    let token = token_payload["token"].as_str().expect("token secret");
    let token_id = token_id_from_secret(token);

    let response = Client::new()
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "search-retry",
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "retry include usage",
                    "search_depth": "advanced"
                }
            }
        }))
        .send()
        .await
        .expect("send search retry request")
        .error_for_status()
        .expect("search retry status")
        .json::<Value>()
        .await
        .expect("decode search retry response");

    assert!(
        response.get("result").is_some(),
        "search retry should recover"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let credits = fetch_latest_token_log_credits(&pool, token_id).await;
    let request_body = fetch_latest_request_body(&pool, token_id).await;
    assert_eq!(credits, Some(2));
    assert_eq!(
        request_body["params"]["name"].as_str(),
        Some("tavily_search")
    );
    assert!(
        request_body["params"]["arguments"]
            .get("include_usage")
            .is_none(),
        "retry path should strip include_usage from the second upstream attempt"
    );
    assert_eq!(fetch_token_monthly_used(&pool, token_id).await, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mcp_extract_retries_without_include_usage_when_upstream_rejects_it() {
    let db_path = temp_db_path("mcp-extract-include-usage-retry");
    let db_str = db_path.to_string_lossy().to_string();
    let (upstream_addr, hits) = spawn_mock_mcp_upstream_retrying_include_usage(
        "tvly-test-key".to_string(),
        "tavily_extract",
        None,
    )
    .await;
    let port = reserve_local_port();
    let upstream = format!("http://{upstream_addr}/mcp");
    let _proxy = spawn_proxy_process(&db_str, &upstream, port);
    wait_for_health(port).await;

    let base_url = format!("http://127.0.0.1:{port}");
    let token_payload = create_test_token(&base_url).await;
    let token = token_payload["token"].as_str().expect("token secret");
    let token_id = token_id_from_secret(token);

    let response = Client::new()
        .post(format!("{base_url}/mcp"))
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "extract-retry",
            "method": "tools/call",
            "params": {
                "name": "tavily_extract",
                "arguments": {
                    "urls": ["https://example.com"]
                }
            }
        }))
        .send()
        .await
        .expect("send extract retry request")
        .error_for_status()
        .expect("extract retry status")
        .json::<Value>()
        .await
        .expect("decode extract retry response");

    assert!(
        response.get("result").is_some(),
        "extract retry should recover"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 2);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let credits = fetch_latest_token_log_credits(&pool, token_id).await;
    let request_body = fetch_latest_request_body(&pool, token_id).await;
    assert_eq!(credits, None);
    assert_eq!(
        request_body["params"]["name"].as_str(),
        Some("tavily_extract")
    );
    assert!(
        request_body["params"]["arguments"]
            .get("include_usage")
            .is_none(),
        "retry path should strip include_usage from the second upstream attempt"
    );
    assert_eq!(fetch_token_monthly_used(&pool, token_id).await, 0);

    let _ = std::fs::remove_file(db_path);
}

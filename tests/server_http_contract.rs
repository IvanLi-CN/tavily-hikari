use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Json, Path as AxumPath, Query};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::routing::{any, get, post};
use axum::{Router, body::Body, http::StatusCode};
use chrono::Utc;
use nanoid::nanoid;
use reqwest::Client;
use serde_json::Value;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

fn temp_db_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{}-{}.db", prefix, nanoid!(8)))
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
    }
}

async fn spawn_mock_upstream_at_path(
    expected_api_key: String,
    upstream_path: &str,
) -> (SocketAddr, oneshot::Receiver<(String, String)>) {
    let (tx, rx) = oneshot::channel::<(String, String)>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    let upstream_path = upstream_path.to_string();
    let app = Router::new()
        .route(
            upstream_path.as_str(),
            post({
                let tx = tx.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let tx = tx.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        let forwarded_auth = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        let forwarded_api_key = body
                            .get("api_key")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        if let Ok(mut guard) = tx.lock()
                            && let Some(ch) = guard.take()
                        {
                            let _ = ch.send((forwarded_auth.clone(), forwarded_api_key.clone()));
                        }

                        if forwarded_auth != format!("Bearer {expected_api_key}")
                            || forwarded_api_key != expected_api_key
                        {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Json(serde_json::json!({"error":"bad credentials"})),
                            );
                        }
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({"status":200,"results":[]})),
                        )
                    }
                }
            }),
        )
        .route("/mcp", any(|| async { (StatusCode::OK, Body::from("{}")) }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    (addr, rx)
}

async fn spawn_mock_upstream(
    expected_api_key: String,
    upstream_path: &'static str,
) -> (SocketAddr, oneshot::Receiver<(String, String)>) {
    spawn_mock_upstream_at_path(expected_api_key, upstream_path).await
}

async fn wait_for_health_ready(port: u16) -> bool {
    let client = Client::new();
    for _ in 0..80 {
        if let Ok(resp) = client
            .get(format!("http://127.0.0.1:{port}/health"))
            .timeout(Duration::from_millis(300))
            .send()
            .await
            && resp.status().is_success()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn spawn_backend_process_with_urls(
    port: u16,
    upstream: &str,
    usage_base: &str,
    db_path: PathBuf,
    dev_open_admin: bool,
) -> BackendGuard {
    let binary = env!("CARGO_BIN_EXE_tavily-hikari");
    let mut cmd = std::process::Command::new(binary);
    cmd.arg("--bind")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--db-path")
        .arg(&db_path)
        .arg("--keys")
        .arg("tvly-test-key")
        .arg("--upstream")
        .arg(upstream)
        .arg("--usage-base")
        .arg(usage_base);
    if dev_open_admin {
        cmd.arg("--dev-open-admin");
    }

    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn backend");
    BackendGuard { child, db_path }
}

async fn spawn_backend_ready(
    upstream_addr: SocketAddr,
    db_path: PathBuf,
    dev_open_admin: bool,
) -> (BackendGuard, u16) {
    let upstream = format!("http://{upstream_addr}/mcp");
    let usage_base = format!("http://{upstream_addr}");
    spawn_backend_ready_with_urls(&upstream, &usage_base, db_path, dev_open_admin).await
}

async fn spawn_backend_ready_with_urls(
    upstream: &str,
    usage_base: &str,
    db_path: PathBuf,
    dev_open_admin: bool,
) -> (BackendGuard, u16) {
    const MAX_RETRIES: usize = 5;
    for attempt in 1..=MAX_RETRIES {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let backend = spawn_backend_process_with_urls(
            port,
            upstream,
            usage_base,
            db_path.clone(),
            dev_open_admin,
        );
        if wait_for_health_ready(port).await {
            return (backend, port);
        }

        drop(backend);
        if attempt < MAX_RETRIES {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    panic!(
        "backend did not become healthy after {} retries",
        MAX_RETRIES
    );
}

async fn assert_upstream_rewrite_contract(
    api_path: &str,
    upstream_path: &'static str,
    request_body: Value,
) {
    assert_upstream_rewrite_contract_with_usage_base(api_path, upstream_path, request_body, None)
        .await;
}

async fn assert_upstream_rewrite_contract_with_usage_base(
    api_path: &str,
    upstream_path: &str,
    request_body: Value,
    usage_base_override: Option<String>,
) {
    const CONTRACT_TOKEN_ID: &str = "demo";
    const CONTRACT_TOKEN_SECRET: &str = "abcdefghijkl";

    let (upstream_addr, rx) =
        spawn_mock_upstream_at_path("tvly-test-key".to_string(), upstream_path).await;

    let db_path = temp_db_path("server-http-contract-endpoint");
    let usage_base = usage_base_override.unwrap_or_else(|| format!("http://{upstream_addr}"));
    let upstream = format!("http://{upstream_addr}/mcp");
    let (_backend, port) =
        spawn_backend_ready_with_urls(&upstream, &usage_base, db_path.clone(), true).await;
    insert_auth_token(&db_path, CONTRACT_TOKEN_ID, CONTRACT_TOKEN_SECRET).await;

    let mut request_body = request_body;
    request_body["api_key"] =
        Value::String(format!("th-{CONTRACT_TOKEN_ID}-{CONTRACT_TOKEN_SECRET}"));

    let resp = Client::new()
        .post(format!("http://127.0.0.1:{port}{api_path}"))
        .json(&request_body)
        .send()
        .await
        .expect("endpoint request");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let (forwarded_auth, forwarded_api_key) = tokio::time::timeout(Duration::from_secs(2), rx)
        .await
        .expect("mock upstream timeout")
        .expect("receive forwarded credentials");
    assert_eq!(forwarded_auth, "Bearer tvly-test-key");
    assert_eq!(forwarded_api_key, "tvly-test-key");
}

async fn insert_auth_token(db_path: &Path, id: &str, secret: &str) {
    let db_url = format!("sqlite://{}", db_path.display());
    let pool = SqlitePool::connect(&db_url).await.expect("connect sqlite");
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO auth_tokens
            (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
        VALUES
            (?, ?, 1, '', NULL, 0, ?, NULL, NULL)
        "#,
    )
    .bind(id)
    .bind(secret)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert auth token");
    pool.close().await;
}

fn assert_forwarded_bearer_auth(headers: &HeaderMap, expected_api_key: &str, endpoint: &str) {
    let forwarded_auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert_eq!(
        forwarded_auth,
        format!("Bearer {expected_api_key}"),
        "upstream Authorization for {endpoint} should use Tavily key"
    );
    assert!(
        !forwarded_auth.starts_with("Bearer th-"),
        "upstream Authorization for {endpoint} must not leak Hikari token"
    );
}

fn assert_probe_http_auth(
    headers: &HeaderMap,
    body: &Value,
    expected_api_key: &str,
    endpoint: &str,
) {
    assert_forwarded_bearer_auth(headers, expected_api_key, endpoint);

    let forwarded_api_key = body
        .get("api_key")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert_eq!(
        forwarded_api_key, expected_api_key,
        "upstream api_key for {endpoint} should use Tavily key"
    );
    assert!(
        !forwarded_api_key.starts_with("th-"),
        "upstream api_key for {endpoint} must not leak Hikari token"
    );
}

async fn spawn_mock_mcp_probe_upstream_at_path(
    expected_api_key: String,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    spawn_mock_mcp_probe_upstream_at_route(expected_api_key, "/mcp").await
}

async fn spawn_mock_mcp_probe_upstream_at_route(
    expected_api_key: String,
    route_path: &str,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let route_path = route_path.to_string();
    let app = Router::new().route(
        route_path.as_str(),
        any({
            let calls = calls.clone();
            move |headers: HeaderMap,
                  Query(params): Query<HashMap<String, String>>,
                  Json(body): Json<Value>| {
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                async move {
                    let forwarded_api_key = params.get("tavilyApiKey").cloned().unwrap_or_default();
                    assert_eq!(
                        forwarded_api_key, expected_api_key,
                        "upstream /mcp should receive the Tavily API key via query string"
                    );

                    let accept = headers
                        .get(axum::http::header::ACCEPT)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default();
                    assert!(
                        accept.contains("application/json"),
                        "browser probe Accept header should preserve application/json"
                    );
                    assert!(
                        accept.contains("text/event-stream"),
                        "browser probe Accept header should preserve text/event-stream"
                    );

                    let method = body
                        .get("method")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();
                    calls.lock().expect("mcp probe calls lock poisoned").push(method.clone());

                    match method.as_str() {
                        "ping" => (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!("probe-ping")),
                                "result": { "ok": true }
                            })),
                        )
                            .into_response(),
                        "tools/list" => {
                            let payload = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!("probe-tools-list")),
                                "result": { "tools": [{ "name": "tavily_search" }] }
                            });
                            (
                                StatusCode::OK,
                                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                                format!("event: message\ndata: {}\n\n", payload),
                            )
                                .into_response()
                        }
                        other => (
                            StatusCode::BAD_REQUEST,
                            Body::from(format!("unexpected MCP method: {other}")),
                        )
                            .into_response(),
                    }
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    (addr, calls)
}

async fn spawn_mock_mcp_probe_upstream(
    expected_api_key: String,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    spawn_mock_mcp_probe_upstream_at_path(expected_api_key).await
}

async fn spawn_mock_mcp_tools_contract_upstream(
    expected_api_key: String,
    advertised_tools: Vec<&'static str>,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route(
        "/mcp",
        any({
            let calls = calls.clone();
            move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                let advertised_tools = advertised_tools.clone();
                async move {
                    let forwarded_api_key = params.get("tavilyApiKey").cloned().unwrap_or_default();
                    assert_eq!(
                        forwarded_api_key, expected_api_key,
                        "upstream /mcp should receive the Tavily API key via query string"
                    );

                    let method = body
                        .get("method")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string();
                    calls.lock().expect("mcp tools contract calls lock poisoned").push(method.clone());

                    match method.as_str() {
                        "tools/list" => (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!("probe-tools-list")),
                                "result": {
                                    "tools": advertised_tools
                                        .iter()
                                        .map(|name| serde_json::json!({ "name": name }))
                                        .collect::<Vec<_>>()
                                }
                            })),
                        )
                            .into_response(),
                        "tools/call" => {
                            let tool_name = body
                                .get("params")
                                .and_then(|params| params.get("name"))
                                .and_then(|value| value.as_str())
                                .unwrap_or_default();
                            assert!(
                                advertised_tools.contains(&tool_name),
                                "unexpected advertised tool call: {tool_name}"
                            );

                            let include_usage = body
                                .get("params")
                                .and_then(|params| params.get("arguments"))
                                .and_then(|arguments| arguments.get("include_usage"))
                                .and_then(|value| value.as_bool());
                            assert_eq!(
                                include_usage,
                                None,
                                "MCP tool {tool_name} should not inject include_usage"
                            );

                            calls
                                .lock()
                                .expect("mcp tools contract calls lock poisoned")
                                .push(format!("tools/call:{tool_name}"));

                            (
                                StatusCode::OK,
                                axum::Json(serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!("tool-call")),
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 1 }
                                        }
                                    }
                                })),
                            )
                                .into_response()
                        }
                        other => (
                            StatusCode::BAD_REQUEST,
                            Body::from(format!("unexpected MCP method: {other}")),
                        )
                            .into_response(),
                    }
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    (addr, calls)
}

async fn spawn_mock_api_probe_upstream_with_prefix(
    expected_api_key: String,
    path_prefix: &str,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let usage_calls = Arc::new(AtomicUsize::new(0));
    let prefixed = |suffix: &str| {
        let prefix = path_prefix.trim_matches('/');
        let suffix = suffix.trim_matches('/');
        if prefix.is_empty() {
            format!("/{suffix}")
        } else {
            format!("/{prefix}/{suffix}")
        }
    };
    let search_path = prefixed("/search");
    let extract_path = prefixed("/extract");
    let crawl_path = prefixed("/crawl");
    let map_path = prefixed("/map");
    let usage_path = prefixed("/usage");
    let research_path = prefixed("/research");
    let research_result_path = prefixed("/research/:request_id");
    let mcp_path = prefixed("/mcp");
    let app = Router::new()
        .route(
            search_path.as_str(),
            post({
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_probe_http_auth(&headers, &body, &expected_api_key, "/search");
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push("search".to_string());
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": 1 }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            extract_path.as_str(),
            post({
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_probe_http_auth(&headers, &body, &expected_api_key, "/extract");
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push("extract".to_string());
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": 1 }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            crawl_path.as_str(),
            post({
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_probe_http_auth(&headers, &body, &expected_api_key, "/crawl");
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push("crawl".to_string());
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": 1 }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            map_path.as_str(),
            post({
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_probe_http_auth(&headers, &body, &expected_api_key, "/map");
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push("map".to_string());
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": 1 }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            usage_path.as_str(),
            get({
                let calls = calls.clone();
                let usage_calls = usage_calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap| {
                    let calls = calls.clone();
                    let usage_calls = usage_calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_forwarded_bearer_auth(&headers, &expected_api_key, "/usage");
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push("usage".to_string());
                        let call_index = usage_calls.fetch_add(1, Ordering::SeqCst);
                        let research_usage = if call_index == 0 { 10 } else { 11 };
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "key": { "research_usage": research_usage }
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            research_path.as_str(),
            post({
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_probe_http_auth(&headers, &body, &expected_api_key, "/research");
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push("research".to_string());
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "request_id": "probe-research-request",
                                "status": "pending"
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            research_result_path.as_str(),
            get({
                let calls = calls.clone();
                let expected_api_key = expected_api_key.clone();
                move |headers: HeaderMap, AxumPath(request_id): AxumPath<String>| {
                    let calls = calls.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_eq!(request_id, "probe-research-request");
                        assert_forwarded_bearer_auth(
                            &headers,
                            &expected_api_key,
                            "/research/:request_id",
                        );
                        calls
                            .lock()
                            .expect("api probe calls lock poisoned")
                            .push(format!("research-result:{request_id}"));
                        (
                            StatusCode::OK,
                            axum::Json(serde_json::json!({
                                "request_id": request_id,
                                "status": "completed"
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            mcp_path.as_str(),
            any(|| async { (StatusCode::OK, Body::from("{}")) }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    (addr, calls)
}

async fn spawn_mock_api_probe_upstream(
    expected_api_key: String,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    spawn_mock_api_probe_upstream_with_prefix(expected_api_key, "").await
}

#[tokio::test]
async fn health_endpoint_is_stable_after_modularization() {
    let (upstream_addr, _rx) = spawn_mock_upstream("tvly-test-key".to_string(), "/search").await;

    let db_path = temp_db_path("server-http-contract-health");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path, true).await;

    let resp = Client::new()
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .expect("health request");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn search_endpoint_rewrites_upstream_credentials() {
    assert_upstream_rewrite_contract(
        "/api/tavily/search",
        "/search",
        serde_json::json!({
            "query": "modularization contract",
            "api_key": "th-demo-token",
            "max_results": 3
        }),
    )
    .await;
}

#[tokio::test]
async fn search_endpoint_supports_prefixed_usage_base_paths() {
    const CONTRACT_TOKEN_ID: &str = "demo";
    const CONTRACT_TOKEN_SECRET: &str = "abcdefghijkl";
    let upstream_route = "/token/Tavily/https/api.tavily.com/search";
    let (upstream_addr, rx) =
        spawn_mock_upstream_at_path("tvly-test-key".to_string(), upstream_route).await;

    let db_path = temp_db_path("server-http-contract-prefixed-search");
    let upstream = format!("http://{upstream_addr}/mcp");
    let usage_base = format!("http://{upstream_addr}/token/Tavily/https/api.tavily.com/");
    let (_backend, port) =
        spawn_backend_ready_with_urls(&upstream, &usage_base, db_path.clone(), true).await;
    insert_auth_token(&db_path, CONTRACT_TOKEN_ID, CONTRACT_TOKEN_SECRET).await;

    let resp = Client::new()
        .post(format!("http://127.0.0.1:{port}/api/tavily/search"))
        .json(&serde_json::json!({
            "query": "prefixed search contract",
            "api_key": format!("th-{CONTRACT_TOKEN_ID}-{CONTRACT_TOKEN_SECRET}"),
            "max_results": 2
        }))
        .send()
        .await
        .expect("prefixed search request");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let (forwarded_auth, forwarded_api_key) = tokio::time::timeout(Duration::from_secs(2), rx)
        .await
        .expect("mock upstream timeout")
        .expect("receive forwarded credentials");
    assert_eq!(forwarded_auth, "Bearer tvly-test-key");
    assert_eq!(forwarded_api_key, "tvly-test-key");
}

#[tokio::test]
async fn extract_crawl_map_endpoints_rewrite_upstream_credentials() {
    let scenarios = vec![
        (
            "/api/tavily/extract",
            "/extract",
            serde_json::json!({
                "api_key": "th-demo-token",
                "urls": ["https://example.com/a"]
            }),
        ),
        (
            "/api/tavily/crawl",
            "/crawl",
            serde_json::json!({
                "api_key": "th-demo-token",
                "url": "https://example.com"
            }),
        ),
        (
            "/api/tavily/map",
            "/map",
            serde_json::json!({
                "api_key": "th-demo-token",
                "url": "https://example.com"
            }),
        ),
    ];

    for (api_path, upstream_path, payload) in scenarios {
        assert_upstream_rewrite_contract(api_path, upstream_path, payload).await;
    }
}

#[tokio::test]
async fn search_endpoint_requires_valid_token_when_dev_open_admin_disabled() {
    let (upstream_addr, _rx) = spawn_mock_upstream("tvly-test-key".to_string(), "/search").await;

    let db_path = temp_db_path("server-http-contract-auth");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path.clone(), false).await;

    let client = Client::new();
    let invalid = client
        .post(format!("http://127.0.0.1:{port}/api/tavily/search"))
        .json(&serde_json::json!({
            "query": "contract invalid token",
            "api_key": "th-bad1-invalid1234"
        }))
        .send()
        .await
        .expect("invalid token request");
    assert_eq!(invalid.status(), reqwest::StatusCode::UNAUTHORIZED);

    insert_auth_token(&db_path, "a1b2", "abcdefghijkl").await;

    let valid = client
        .post(format!("http://127.0.0.1:{port}/api/tavily/search"))
        .json(&serde_json::json!({
            "query": "contract valid token",
            "api_key": "th-a1b2-abcdefghijkl",
            "max_results": 1
        }))
        .send()
        .await
        .expect("valid token request");
    assert_eq!(valid.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn mcp_probe_requests_with_authorization_header_reach_upstream() {
    let (upstream_addr, calls) = spawn_mock_mcp_probe_upstream("tvly-test-key".to_string()).await;

    let db_path = temp_db_path("server-http-contract-mcp-probe");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path.clone(), false).await;
    insert_auth_token(&db_path, "zjvc", "abcdefghijkl").await;

    let client = Client::new();
    let token = "th-zjvc-abcdefghijkl";
    let mcp_url = format!("http://127.0.0.1:{port}/mcp");

    let ping = client
        .post(&mcp_url)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            axum::http::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "probe-ping",
            "method": "ping",
            "params": {}
        }))
        .send()
        .await
        .expect("mcp ping request");
    assert_eq!(ping.status(), reqwest::StatusCode::OK);

    let tools_list = client
        .post(&mcp_url)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            axum::http::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "probe-tools-list",
            "method": "tools/list",
            "params": {}
        }))
        .send()
        .await
        .expect("mcp tools/list request");
    assert_eq!(tools_list.status(), reqwest::StatusCode::OK);
    assert_eq!(
        tools_list
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream"),
    );
    let tools_list_body = tools_list.text().await.expect("read tools/list body");
    assert!(
        tools_list_body.contains("tavily_search"),
        "expected SSE probe response body to include tool discovery payload"
    );

    let upstream_calls = calls.lock().expect("mcp probe calls lock poisoned").clone();
    assert_eq!(
        upstream_calls,
        vec!["ping".to_string(), "tools/list".to_string()]
    );
}

#[tokio::test]
async fn mcp_probe_requests_preserve_prefixed_upstream_path() {
    let upstream_path = "/token/Tavily/https/mcp.tavily.com/mcp";
    let (upstream_addr, calls) =
        spawn_mock_mcp_probe_upstream_at_route("tvly-test-key".to_string(), upstream_path).await;

    let db_path = temp_db_path("server-http-contract-mcp-prefixed");
    let upstream = format!("http://{upstream_addr}{upstream_path}/");
    let usage_base = format!("http://{upstream_addr}");
    let (_backend, port) =
        spawn_backend_ready_with_urls(&upstream, &usage_base, db_path.clone(), false).await;
    insert_auth_token(&db_path, "zjvc", "abcdefghijkl").await;

    let client = Client::new();
    let token = "th-zjvc-abcdefghijkl";
    let mcp_url = format!("http://127.0.0.1:{port}/mcp");

    let ping = client
        .post(&mcp_url)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            axum::http::header::ACCEPT,
            "application/json, text/event-stream",
        )
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "probe-ping",
            "method": "ping",
            "params": {}
        }))
        .send()
        .await
        .expect("mcp prefixed ping request");
    assert_eq!(ping.status(), reqwest::StatusCode::OK);

    let upstream_calls = calls
        .lock()
        .expect("mcp prefixed calls lock poisoned")
        .clone();
    assert_eq!(upstream_calls, vec!["ping".to_string()]);
}

#[tokio::test]
async fn mcp_tools_list_advertised_tools_can_all_be_called_via_authorization_header() {
    let advertised_tools = vec![
        "tavily-search",
        "tavily-extract",
        "tavily-crawl",
        "tavily-map",
        "tavily-research",
    ];
    let (upstream_addr, calls) = spawn_mock_mcp_tools_contract_upstream(
        "tvly-test-key".to_string(),
        advertised_tools.clone(),
    )
    .await;

    let db_path = temp_db_path("server-http-contract-mcp-tools");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path.clone(), false).await;
    insert_auth_token(&db_path, "zjvc", "abcdefghijkl").await;

    let client = Client::new();
    let token = "th-zjvc-abcdefghijkl";
    let mcp_url = format!("http://127.0.0.1:{port}/mcp");

    let tools_list = client
        .post(&mcp_url)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "probe-tools-list",
            "method": "tools/list",
            "params": {}
        }))
        .send()
        .await
        .expect("mcp tools/list request");
    assert_eq!(tools_list.status(), reqwest::StatusCode::OK);

    let tools_list_body: Value = tools_list.json().await.expect("parse tools/list response");
    let tools = tools_list_body
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .expect("tools/list should return tools array");
    let discovered_tool_names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(discovered_tool_names, advertised_tools);

    for tool_name in &discovered_tool_names {
        let arguments = match *tool_name {
            "tavily-search" => serde_json::json!({
                "query": "health check",
                "search_depth": "basic"
            }),
            "tavily-extract" => serde_json::json!({
                "urls": ["https://example.com"]
            }),
            "tavily-crawl" => serde_json::json!({
                "url": "https://example.com",
                "max_depth": 1,
                "limit": 1
            }),
            "tavily-map" => serde_json::json!({
                "url": "https://example.com",
                "max_depth": 1,
                "limit": 1
            }),
            "tavily-research" => serde_json::json!({
                "input": "health check"
            }),
            other => panic!("unexpected tool name in test fixture: {other}"),
        };

        let response = client
            .post(&mcp_url)
            .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": format!("call-{tool_name}"),
                "method": "tools/call",
                "params": {
                    "name": tool_name,
                    "arguments": arguments
                }
            }))
            .send()
            .await
            .expect("mcp tools/call request");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "expected tools/call to succeed for advertised tool {tool_name}"
        );
    }

    let upstream_calls = calls
        .lock()
        .expect("mcp tools contract calls lock poisoned")
        .clone();
    assert!(
        upstream_calls.iter().any(|call| call == "tools/list"),
        "expected a tools/list discovery call"
    );
    for tool_name in &advertised_tools {
        let expected_call = format!("tools/call:{tool_name}");
        assert!(
            upstream_calls.iter().any(|call| call == &expected_call),
            "expected advertised tool {tool_name} to be exercised through /mcp tools/call"
        );
    }
}

#[tokio::test]
async fn mcp_tools_list_legacy_underscore_tools_still_apply_supported_tavily_metadata() {
    let advertised_tools = vec![
        "tavily_search",
        "tavily_extract",
        "tavily_crawl",
        "tavily_map",
        "tavily_research",
    ];
    let (upstream_addr, calls) = spawn_mock_mcp_tools_contract_upstream(
        "tvly-test-key".to_string(),
        advertised_tools.clone(),
    )
    .await;

    let db_path = temp_db_path("server-http-contract-mcp-tools-underscore");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path.clone(), false).await;
    insert_auth_token(&db_path, "zjvc", "abcdefghijkl").await;

    let client = Client::new();
    let token = "th-zjvc-abcdefghijkl";
    let mcp_url = format!("http://127.0.0.1:{port}/mcp");

    let tools_list = client
        .post(&mcp_url)
        .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": "probe-tools-list",
            "method": "tools/list",
            "params": {}
        }))
        .send()
        .await
        .expect("mcp tools/list request");
    assert_eq!(tools_list.status(), reqwest::StatusCode::OK);

    let tools_list_body: Value = tools_list.json().await.expect("parse tools/list response");
    let tools = tools_list_body
        .get("result")
        .and_then(|result| result.get("tools"))
        .and_then(Value::as_array)
        .expect("tools/list should return tools array");
    let discovered_tool_names = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(discovered_tool_names, advertised_tools);

    for tool_name in &discovered_tool_names {
        let arguments = match tool_name.to_ascii_lowercase().replace('_', "-").as_str() {
            "tavily-search" => serde_json::json!({
                "query": "health check",
                "search_depth": "basic"
            }),
            "tavily-extract" => serde_json::json!({
                "urls": ["https://example.com"]
            }),
            "tavily-crawl" => serde_json::json!({
                "url": "https://example.com",
                "max_depth": 1,
                "limit": 1
            }),
            "tavily-map" => serde_json::json!({
                "url": "https://example.com",
                "max_depth": 1,
                "limit": 1
            }),
            "tavily-research" => serde_json::json!({
                "input": "health check"
            }),
            other => panic!("unexpected tool name in test fixture: {other}"),
        };

        let response = client
            .post(&mcp_url)
            .header(axum::http::header::AUTHORIZATION, format!("Bearer {token}"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": format!("call-{tool_name}"),
                "method": "tools/call",
                "params": {
                    "name": tool_name,
                    "arguments": arguments
                }
            }))
            .send()
            .await
            .expect("mcp tools/call request");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "expected tools/call to succeed for advertised tool {tool_name}"
        );
    }

    let upstream_calls = calls
        .lock()
        .expect("mcp tools contract calls lock poisoned")
        .clone();
    assert!(
        upstream_calls.iter().any(|call| call == "tools/list"),
        "expected a tools/list discovery call"
    );
    for tool_name in &advertised_tools {
        let expected_call = format!("tools/call:{tool_name}");
        assert!(
            upstream_calls.iter().any(|call| call == &expected_call),
            "expected advertised tool {tool_name} to be exercised through /mcp tools/call"
        );
    }
}

#[tokio::test]
async fn api_probe_requests_with_authorization_header_reach_upstream() {
    let (upstream_addr, calls) = spawn_mock_api_probe_upstream("tvly-test-key".to_string()).await;

    let db_path = temp_db_path("server-http-contract-api-probe");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path.clone(), false).await;
    insert_auth_token(&db_path, "zjvc", "abcdefghijkl").await;

    let client = Client::new();
    let token = "th-zjvc-abcdefghijkl";
    let auth_header = format!("Bearer {token}");
    let base = format!("http://127.0.0.1:{port}");

    let search = client
        .post(format!("{base}/api/tavily/search"))
        .header(axum::http::header::AUTHORIZATION, &auth_header)
        .json(&serde_json::json!({
            "query": "health check",
            "max_results": 1,
            "search_depth": "basic",
            "include_answer": false,
            "include_raw_content": false,
            "include_images": false
        }))
        .send()
        .await
        .expect("search probe request");
    assert_eq!(search.status(), reqwest::StatusCode::OK);

    let extract = client
        .post(format!("{base}/api/tavily/extract"))
        .header(axum::http::header::AUTHORIZATION, &auth_header)
        .json(&serde_json::json!({
            "urls": ["https://example.com"],
            "include_images": false
        }))
        .send()
        .await
        .expect("extract probe request");
    assert_eq!(extract.status(), reqwest::StatusCode::OK);

    let crawl = client
        .post(format!("{base}/api/tavily/crawl"))
        .header(axum::http::header::AUTHORIZATION, &auth_header)
        .json(&serde_json::json!({
            "url": "https://example.com",
            "max_depth": 1,
            "limit": 1
        }))
        .send()
        .await
        .expect("crawl probe request");
    assert_eq!(crawl.status(), reqwest::StatusCode::OK);

    let map = client
        .post(format!("{base}/api/tavily/map"))
        .header(axum::http::header::AUTHORIZATION, &auth_header)
        .json(&serde_json::json!({
            "url": "https://example.com",
            "max_depth": 1,
            "limit": 1
        }))
        .send()
        .await
        .expect("map probe request");
    assert_eq!(map.status(), reqwest::StatusCode::OK);

    let research = client
        .post(format!("{base}/api/tavily/research"))
        .header(axum::http::header::AUTHORIZATION, &auth_header)
        .json(&serde_json::json!({
            "input": "health check",
            "model": "mini",
            "citation_format": "numbered"
        }))
        .send()
        .await
        .expect("research probe request");
    assert_eq!(research.status(), reqwest::StatusCode::OK);
    let research_body: Value = research.json().await.expect("parse research response");
    assert_eq!(
        research_body
            .get("request_id")
            .and_then(|value| value.as_str()),
        Some("probe-research-request"),
    );

    let research_result = client
        .get(format!("{base}/api/tavily/research/probe-research-request"))
        .header(axum::http::header::AUTHORIZATION, &auth_header)
        .send()
        .await
        .expect("research result probe request");
    assert_eq!(research_result.status(), reqwest::StatusCode::OK);
    let research_result_body: Value = research_result
        .json()
        .await
        .expect("parse research result response");
    assert_eq!(
        research_result_body
            .get("status")
            .and_then(|value| value.as_str()),
        Some("completed"),
    );

    let upstream_calls = calls.lock().expect("api probe calls lock poisoned").clone();
    assert_eq!(
        upstream_calls.len(),
        8,
        "expected probe flow to hit every upstream endpoint once, with two usage probes"
    );
    assert_eq!(
        upstream_calls
            .iter()
            .filter(|call| call.as_str() == "usage")
            .count(),
        2,
        "research probe should perform pre/post usage reads"
    );
    assert!(upstream_calls.iter().any(|call| call == "search"));
    assert!(upstream_calls.iter().any(|call| call == "extract"));
    assert!(upstream_calls.iter().any(|call| call == "crawl"));
    assert!(upstream_calls.iter().any(|call| call == "map"));
    assert!(upstream_calls.iter().any(|call| call == "research"));
    assert!(
        upstream_calls
            .iter()
            .any(|call| call == "research-result:probe-research-request")
    );
}

#[tokio::test]
async fn api_probe_requests_support_prefixed_usage_base_paths() {
    let prefix = "token/Tavily/https/api.tavily.com";
    let (upstream_addr, calls) =
        spawn_mock_api_probe_upstream_with_prefix("tvly-test-key".to_string(), prefix).await;

    let db_path = temp_db_path("server-http-contract-api-prefixed");
    let upstream = format!("http://{upstream_addr}/mcp");
    let usage_base = format!("http://{upstream_addr}/{prefix}/");
    let (_backend, port) =
        spawn_backend_ready_with_urls(&upstream, &usage_base, db_path.clone(), false).await;
    insert_auth_token(&db_path, "zjvc", "abcdefghijkl").await;

    let client = Client::new();
    let auth_header = "Bearer th-zjvc-abcdefghijkl";
    let base = format!("http://127.0.0.1:{port}");

    let search = client
        .post(format!("{base}/api/tavily/search"))
        .header(axum::http::header::AUTHORIZATION, auth_header)
        .json(&serde_json::json!({
            "query": "prefixed api probe",
            "max_results": 1
        }))
        .send()
        .await
        .expect("prefixed search request");
    assert_eq!(search.status(), reqwest::StatusCode::OK);

    let research = client
        .post(format!("{base}/api/tavily/research"))
        .header(axum::http::header::AUTHORIZATION, auth_header)
        .json(&serde_json::json!({
            "input": "prefixed api probe",
            "model": "mini"
        }))
        .send()
        .await
        .expect("prefixed research request");
    assert_eq!(research.status(), reqwest::StatusCode::OK);

    let research_result = client
        .get(format!("{base}/api/tavily/research/probe-research-request"))
        .header(axum::http::header::AUTHORIZATION, auth_header)
        .send()
        .await
        .expect("prefixed research result request");
    assert_eq!(research_result.status(), reqwest::StatusCode::OK);

    let usage = client
        .get(format!("{base}/api/tavily/usage"))
        .header(axum::http::header::AUTHORIZATION, auth_header)
        .send()
        .await
        .expect("prefixed usage request");
    assert_eq!(usage.status(), reqwest::StatusCode::OK);

    let upstream_calls = calls
        .lock()
        .expect("prefixed api probe calls lock poisoned")
        .clone();
    assert_eq!(
        upstream_calls,
        vec![
            "search".to_string(),
            "usage".to_string(),
            "research".to_string(),
            "usage".to_string(),
            "research-result:probe-research-request".to_string(),
        ],
    );
}

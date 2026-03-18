use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::Duration;

use axum::extract::Json;
use axum::http::HeaderMap;
use axum::routing::{any, post};
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

async fn spawn_mock_upstream(
    expected_api_key: String,
    upstream_path: &'static str,
) -> (SocketAddr, oneshot::Receiver<(String, String)>) {
    let (tx, rx) = oneshot::channel::<(String, String)>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    let app = Router::new()
        .route(
            upstream_path,
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

fn spawn_backend_process_with_mode(
    port: u16,
    upstream_addr: SocketAddr,
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
        .arg(format!("http://{upstream_addr}/mcp"))
        .arg("--usage-base")
        .arg(format!("http://{upstream_addr}"));
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
    const MAX_RETRIES: usize = 5;
    for attempt in 1..=MAX_RETRIES {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let backend =
            spawn_backend_process_with_mode(port, upstream_addr, db_path.clone(), dev_open_admin);
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
    const CONTRACT_TOKEN_ID: &str = "demo";
    const CONTRACT_TOKEN_SECRET: &str = "abcdefghijkl";

    let (upstream_addr, rx) = spawn_mock_upstream("tvly-test-key".to_string(), upstream_path).await;

    let db_path = temp_db_path("server-http-contract-endpoint");
    let (_backend, port) = spawn_backend_ready(upstream_addr, db_path.clone(), true).await;
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

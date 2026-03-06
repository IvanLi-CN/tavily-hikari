#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::{Json, Query};
    use axum::http::Method;
    use axum::routing::{any, get, post};
    use nanoid::nanoid;
    use reqwest::Client;
    use sqlx::Row;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tavily_hikari::DEFAULT_UPSTREAM;
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    fn temp_db_path(prefix: &str) -> PathBuf {
        let file = format!("{}-{}.db", prefix, nanoid!(8));
        std::env::temp_dir().join(file)
    }

    fn env_var_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = env_var_test_lock().lock().expect("env var test lock poisoned");
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            unsafe {
                if let Some(prev) = self.previous.as_deref() {
                    std::env::set_var(self.key, prev);
                } else {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    fn temp_static_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-static-{}", nanoid!(8)));
        std::fs::create_dir_all(&dir).expect("create temp static dir");
        std::fs::write(
            dir.join("index.html"),
            "<!doctype html><title>index</title>",
        )
        .expect("write index");
        std::fs::write(
            dir.join("console.html"),
            "<!doctype html><title>console</title>",
        )
        .expect("write console");
        std::fs::write(
            dir.join("admin.html"),
            "<!doctype html><title>admin</title>",
        )
        .expect("write admin");
        std::fs::write(
            dir.join("login.html"),
            "<!doctype html><title>login</title>",
        )
        .expect("write login");
        dir
    }

    async fn spawn_mock_upstream(expected_api_key: String) -> SocketAddr {
        let app = Router::new().route(
            "/mcp",
            any({
                move |Query(params): Query<HashMap<String, String>>| {
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        let received = params.get("tavilyApiKey").cloned();
                        if received.as_deref() != Some(expected_api_key.as_str()) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Body::from("missing or incorrect tavilyApiKey"),
                            );
                        }
                        (StatusCode::OK, Body::from("{\"ok\":true}"))
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
        addr
    }

    async fn spawn_mock_upstream_with_hits(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(_body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        let received = params.get("tavilyApiKey").cloned();
                        if received.as_deref() != Some(expected_api_key.as_str()) {
                            return (
                                StatusCode::UNAUTHORIZED,
                                Body::from("missing or incorrect tavilyApiKey"),
                            );
                        }
                        (StatusCode::OK, Body::from("{\"ok\":true}"))
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        let args = body
                            .get("params")
                            .and_then(|p| p.get("arguments"))
                            .unwrap_or(&Value::Null);
                        let depth = args
                            .get("search_depth")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let credits = if depth.eq_ignore_ascii_case("advanced") {
                            2
                        } else {
                            1
                        };

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": credits },
                                    }
                                }
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_empty_body(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        Response::builder()
                            .status(StatusCode::OK)
                            .body(Body::empty())
                            .expect("build response")
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_sse(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        let id = body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1));
                        let sse = format!(
                            "data: {{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"structuredContent\":{{\"status\":200,\"usage\":{{\"credits\":2}}}}}}}}\n\n"
                        );
                        let resp = Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(sse))
                            .expect("build response");
                        resp
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_batch(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert!(
                            !items.is_empty(),
                            "expected non-empty JSON-RPC batch body"
                        );

                        let mut responses: Vec<Value> = Vec::with_capacity(items.len());
                        for (idx, item) in items.iter().enumerate() {
                            let map = item
                                .as_object()
                                .expect("expected JSON-RPC object item in batch");
                            assert_eq!(
                                map.get("method").and_then(|v| v.as_str()),
                                Some("tools/call"),
                                "expected MCP tools/call in batch"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|v| v.as_str()),
                                Some("tavily-search"),
                                "expected tavily-search tool call"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("arguments"))
                                    .and_then(|a| a.get("include_usage"))
                                    .and_then(|v| v.as_bool()),
                                Some(true),
                                "proxy should inject include_usage=true"
                            );

                            let args = map
                                .get("params")
                                .and_then(|p| p.get("arguments"))
                                .unwrap_or(&Value::Null);
                            let depth = args
                                .get("search_depth")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let credits = if depth.eq_ignore_ascii_case("advanced") {
                                2
                            } else {
                                1
                            };

                            responses.push(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": map.get("id").cloned().unwrap_or_else(|| serde_json::json!(idx as i64 + 1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": credits },
                                    }
                                }
                            }));
                        }

                        (StatusCode::OK, Json(Value::Array(responses)))
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_batch_with_error(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert_eq!(items.len(), 2, "expected 2-item batch");

                        for item in items {
                            let map = item
                                .as_object()
                                .expect("expected JSON-RPC object item in batch");
                            assert_eq!(
                                map.get("method").and_then(|v| v.as_str()),
                                Some("tools/call"),
                                "expected MCP tools/call in batch"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|v| v.as_str()),
                                Some("tavily-search"),
                                "expected tavily-search tool call"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("arguments"))
                                    .and_then(|a| a.get("include_usage"))
                                    .and_then(|v| v.as_bool()),
                                Some(true),
                                "proxy should inject include_usage=true"
                            );
                        }

                        // 1st item succeeds with usage.credits, 2nd item is a JSON-RPC error.
                        let id1 = items[0]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(1));
                        let id2 = items[1]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(2));

                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": id1,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 1 },
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": id2,
                                    "error": { "code": -32000, "message": "boom" }
                                }
                            ])),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_batch_with_quota_exhausted(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert_eq!(items.len(), 2, "expected 2-item batch");

                        for item in items {
                            let map = item
                                .as_object()
                                .expect("expected JSON-RPC object item in batch");
                            assert_eq!(
                                map.get("method").and_then(|v| v.as_str()),
                                Some("tools/call"),
                                "expected MCP tools/call in batch"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|v| v.as_str()),
                                Some("tavily-search"),
                                "expected tavily-search tool call"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("arguments"))
                                    .and_then(|a| a.get("include_usage"))
                                    .and_then(|v| v.as_bool()),
                                Some(true),
                                "proxy should inject include_usage=true"
                            );
                        }

                        // 1st item succeeds with usage.credits, 2nd item returns quota exhausted.
                        let id1 = items[0]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(1));
                        let id2 = items[1]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(2));

                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": id1,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 1 },
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": id2,
                                    "result": {
                                        "structuredContent": {
                                            "status": 432
                                        }
                                    }
                                }
                            ])),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_batch_with_detail_error(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert_eq!(items.len(), 2, "expected 2-item batch");

                        for item in items {
                            let map = item
                                .as_object()
                                .expect("expected JSON-RPC object item in batch");
                            assert_eq!(
                                map.get("method").and_then(|v| v.as_str()),
                                Some("tools/call"),
                                "expected MCP tools/call in batch"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|v| v.as_str()),
                                Some("tavily-search"),
                                "expected tavily-search tool call"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("arguments"))
                                    .and_then(|a| a.get("include_usage"))
                                    .and_then(|v| v.as_bool()),
                                Some(true),
                                "proxy should inject include_usage=true"
                            );
                        }

                        // 1st item succeeds with usage.credits=2, 2nd item encodes an error via
                        // structuredContent.detail.status (no top-level structuredContent.status).
                        let id1 = items[0]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(1));
                        let id2 = items[1]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(2));

                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": id1,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 2 },
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": id2,
                                    "result": {
                                        "structuredContent": {
                                            "detail": { "status": 500 }
                                        }
                                    }
                                }
                            ])),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_batch_partial_usage(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert_eq!(items.len(), 2, "expected 2-item batch");

                        for item in items {
                            let map = item
                                .as_object()
                                .expect("expected JSON-RPC object item in batch");
                            assert_eq!(
                                map.get("method").and_then(|v| v.as_str()),
                                Some("tools/call"),
                                "expected MCP tools/call in batch"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("name"))
                                    .and_then(|v| v.as_str()),
                                Some("tavily-search"),
                                "expected tavily-search tool call"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("arguments"))
                                    .and_then(|a| a.get("include_usage"))
                                    .and_then(|v| v.as_bool()),
                                Some(true),
                                "proxy should inject include_usage=true"
                            );
                        }

                        // Both items succeed, but only the first includes usage.credits.
                        let id1 = items[0]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(1));
                        let id2 = items[1]
                            .get("id")
                            .cloned()
                            .unwrap_or_else(|| serde_json::json!(2));

                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": id1,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 1 },
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": id2,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200
                                        }
                                    }
                                }
                            ])),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_mixed_tools_list_and_search_usage(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert_eq!(items.len(), 2, "expected 2-item batch");

                        let a = items[0]
                            .as_object()
                            .expect("expected JSON-RPC object item in batch");
                        assert_eq!(
                            a.get("method").and_then(|v| v.as_str()),
                            Some("tools/list"),
                            "expected tools/list in mixed batch"
                        );

                        let b = items[1]
                            .as_object()
                            .expect("expected JSON-RPC object item in batch");
                        assert_eq!(
                            b.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected tools/call in mixed batch"
                        );
                        assert_eq!(
                            b.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            b.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true for tavily-search"
                        );

                        let id1 = a.get("id").cloned().unwrap_or_else(|| serde_json::json!(1));
                        let id2 = b.get("id").cloned().unwrap_or_else(|| serde_json::json!(2));

                        // Include usage.credits for both items to validate we only charge billable
                        // items (tools/list is non-billable by business quota).
                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": id1,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 50 }
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": id2,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": 2 }
                                        }
                                    }
                                }
                            ])),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_search_and_extract_partial_usage(
        expected_api_key: String,
        extract_credits: i64,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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

                        let items = body
                            .as_array()
                            .expect("expected JSON-RPC batch body (array)");
                        assert_eq!(items.len(), 2, "expected 2-item batch");

                        let mut search_id = None;
                        let mut extract_id = None;

                        for item in items {
                            let map = item
                                .as_object()
                                .expect("expected JSON-RPC object item in batch");
                            assert_eq!(
                                map.get("method").and_then(|v| v.as_str()),
                                Some("tools/call"),
                                "expected MCP tools/call in batch"
                            );
                            let name = map
                                .get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            assert!(
                                matches!(name, "tavily-search" | "tavily-extract"),
                                "unexpected tool name: {name}"
                            );
                            assert_eq!(
                                map.get("params")
                                    .and_then(|p| p.get("arguments"))
                                    .and_then(|a| a.get("include_usage"))
                                    .and_then(|v| v.as_bool()),
                                Some(true),
                                "proxy should inject include_usage=true"
                            );

                            if name == "tavily-search" {
                                search_id = Some(
                                    map.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                );
                            }
                            if name == "tavily-extract" {
                                extract_id = Some(
                                    map.get("id").cloned().unwrap_or_else(|| serde_json::json!(2)),
                                );
                            }
                        }

                        let search_id = search_id.expect("missing tavily-search id");
                        let extract_id = extract_id.expect("missing tavily-extract id");

                        // Search is missing usage.credits; extract includes usage.credits. The
                        // proxy should charge extract credits + expected search credits.
                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": search_id,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": extract_id,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200,
                                            "usage": { "credits": extract_credits }
                                        }
                                    }
                                }
                            ])),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_delayed(
        expected_api_key: String,
        arrived: Arc<Notify>,
        release: Arc<Notify>,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    let arrived = arrived.clone();
                    let release = release.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        let received = params.get("tavilyApiKey").cloned();
                        assert_eq!(
                            received.as_deref(),
                            Some(expected_api_key.as_str()),
                            "missing or incorrect tavilyApiKey"
                        );

                        assert_eq!(
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        let args = body
                            .get("params")
                            .and_then(|p| p.get("arguments"))
                            .unwrap_or(&Value::Null);
                        let depth = args
                            .get("search_depth")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let credits = if depth.eq_ignore_ascii_case("advanced") {
                            2
                        } else {
                            1
                        };

                        arrived.notify_one();
                        release.notified().await;

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": credits },
                                    }
                                }
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_error(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "error": {
                                    "code": -32000,
                                    "message": "mock jsonrpc error",
                                }
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_search_failed_status_string(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-search"),
                            "expected tavily-search tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        let args = body
                            .get("params")
                            .and_then(|p| p.get("arguments"))
                            .unwrap_or(&Value::Null);
                        let depth = args
                            .get("search_depth")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let credits = if depth.eq_ignore_ascii_case("advanced") {
                            2
                        } else {
                            1
                        };

                        // Simulate "HTTP 200 but structured failure" with a string `status`
                        // inside the JSON-RPC structuredContent envelope.
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": "failed",
                                        "usage": { "credits": credits },
                                        "message": "mock structured failure",
                                    }
                                }
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_unknown_tavily_tool(
        expected_api_key: String,
        tool_name: &'static str,
        credits: i64,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some(tool_name),
                            "expected {} tool call",
                            tool_name
                        );

                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true for tavily-* tool calls"
                        );

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": credits },
                                    }
                                }
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_non_search_tools(
        expected_api_key: String,
        extract_credits: i64,
        crawl_credits: i64,
        map_credits: i64,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );

                        let tool = body
                            .get("params")
                            .and_then(|p| p.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        assert!(
                            matches!(tool, "tavily-extract" | "tavily-crawl" | "tavily-map"),
                            "unexpected tool name: {tool}"
                        );

                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        let credits = match tool {
                            "tavily-extract" => extract_credits,
                            "tavily-crawl" => crawl_credits,
                            "tavily-map" => map_credits,
                            _ => 0,
                        };

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": credits },
                                    }
                                }
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_mock_mcp_upstream_for_tavily_extract_without_usage(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
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
                            body.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call"
                        );

                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some("tavily-extract"),
                            "expected tavily-extract tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            Some(true),
                            "proxy should inject include_usage=true"
                        );

                        // Intentionally omit `usage.credits` to validate that non-search tools
                        // skip billing when usage is missing (we do not guess unpredictable costs).
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "results": [],
                                    }
                                }
                            })),
                        )
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
        (addr, hits)
    }

    fn assert_upstream_json_auth(
        headers: &HeaderMap,
        body: &Value,
        expected_api_key: &str,
        endpoint: &str,
    ) {
        let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            api_key, expected_api_key,
            "upstream api_key for {endpoint} should use Tavily key from pool"
        );
        assert!(
            !api_key.starts_with("th-"),
            "upstream {endpoint} api_key must not be Hikari token"
        );

        if matches!(endpoint, "/search" | "/extract" | "/crawl" | "/map") {
            assert_eq!(
                body.get("include_usage").and_then(|v| v.as_bool()),
                Some(true),
                "upstream {endpoint} should be forced to include usage"
            );
        }

        assert_upstream_bearer_auth(headers, expected_api_key, endpoint);
    }

    fn assert_upstream_bearer_auth(headers: &HeaderMap, expected_api_key: &str, endpoint: &str) {
        let authorization = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let expected_auth = format!("Bearer {}", expected_api_key);
        assert_eq!(
            authorization, expected_auth,
            "upstream Authorization for {endpoint} should use Tavily key"
        );
        assert!(
            !authorization.starts_with("Bearer th-"),
            "upstream Authorization for {endpoint} must not use Hikari token"
        );
    }

    async fn spawn_http_search_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
        let app = Router::new().route(
            "/search",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
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
        addr
    }

    async fn spawn_http_search_mock_with_usage(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/search",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");

                        let search_depth = body
                            .get("search_depth")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let credits = if search_depth.eq_ignore_ascii_case("advanced") {
                            2
                        } else {
                            1
                        };

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": credits },
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_http_search_mock_with_usage_delayed(
        expected_api_key: String,
        arrived: Arc<Notify>,
        release: Arc<Notify>,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/search",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    let arrived = arrived.clone();
                    let release = release.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");

                        let search_depth = body
                            .get("search_depth")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let credits = if search_depth.eq_ignore_ascii_case("advanced") {
                            2
                        } else {
                            1
                        };

                        arrived.notify_one();
                        release.notified().await;

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                                "usage": { "credits": credits },
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_http_search_mock_without_usage(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/search",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");
                        // Intentionally omit `usage.credits` to exercise the handler-side
                        // fallback to expected cost (based on request search_depth).
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_http_search_mock_with_usage_and_failed_status(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/search",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/search");

                        let search_depth = body
                            .get("search_depth")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let credits = if search_depth.eq_ignore_ascii_case("advanced") {
                            2
                        } else {
                            1
                        };

                        // Simulate "HTTP 200 but structured failure" so AttemptAnalysis.status != "success".
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": "failed",
                                "results": [],
                                "usage": { "credits": credits },
                                "message": "mock structured failure",
                            })),
                        )
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
        (addr, hits)
    }

    async fn spawn_http_json_endpoints_mock_with_usage(
        expected_api_key: String,
        extract_credits: i64,
        crawl_credits: i64,
        map_credits: i64,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let expected_api_key_extract = expected_api_key.clone();
        let expected_api_key_crawl = expected_api_key.clone();
        let expected_api_key_map = expected_api_key;
        let app = Router::new()
            .route(
                "/extract",
                post({
                    let hits = hits.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let expected_api_key = expected_api_key_extract.clone();
                        let hits = hits.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_json_auth(&headers, &body, &expected_api_key, "/extract");
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "status": 200,
                                    "results": [],
                                    "usage": { "credits": extract_credits },
                                })),
                            )
                        }
                    }
                }),
            )
            .route(
                "/crawl",
                post({
                    let hits = hits.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let expected_api_key = expected_api_key_crawl.clone();
                        let hits = hits.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_json_auth(&headers, &body, &expected_api_key, "/crawl");
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "status": 200,
                                    "results": [],
                                    "usage": { "credits": crawl_credits },
                                })),
                            )
                        }
                    }
                }),
            )
            .route(
                "/map",
                post({
                    let hits = hits.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let expected_api_key = expected_api_key_map.clone();
                        let hits = hits.clone();
                        async move {
                            hits.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_json_auth(&headers, &body, &expected_api_key, "/map");
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "status": 200,
                                    "results": [],
                                    "usage": { "credits": map_credits },
                                })),
                            )
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
        (addr, hits)
    }

    async fn spawn_http_extract_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
        let app = Router::new().route(
            "/extract",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/extract");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
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
        addr
    }

    async fn spawn_http_crawl_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
        let app = Router::new().route(
            "/crawl",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/crawl");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
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
        addr
    }

    async fn spawn_http_map_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
        let app = Router::new().route(
            "/map",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/map");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": 200,
                                "results": [],
                            })),
                        )
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
        addr
    }

    async fn spawn_http_map_mock_returning_500(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/map",
            post({
                let hits = hits.clone();
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_upstream_json_auth(&headers, &body, &expected_api_key, "/map");
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Body::from("mock map upstream error"),
                        )
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
        (addr, hits)
    }

    async fn spawn_http_research_mock_with_usage_diff(
        expected_api_key: String,
        base_research_usage: i64,
        delta: i64,
    ) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let usage_calls = Arc::new(AtomicUsize::new(0));
        let research_calls = Arc::new(AtomicUsize::new(0));
        let expected_api_key_usage = expected_api_key.clone();
        let expected_api_key_research = expected_api_key;
        let app = Router::new()
            .route(
                "/usage",
                get({
                    let usage_calls = usage_calls.clone();
                    move |headers: HeaderMap| {
                        let expected_api_key = expected_api_key_usage.clone();
                        let usage_calls = usage_calls.clone();
                        async move {
                            let call_index = usage_calls.fetch_add(1, Ordering::SeqCst) + 1;
                            assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                            // First call: base, second call: base + delta.
                            let research_usage = if call_index <= 1 {
                                base_research_usage
                            } else {
                                base_research_usage + delta
                            };
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "key": { "research_usage": research_usage }
                                })),
                            )
                        }
                    }
                }),
            )
            .route(
                "/research",
                post({
                    let research_calls = research_calls.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let expected_api_key = expected_api_key_research.clone();
                        let research_calls = research_calls.clone();
                        async move {
                            research_calls.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "request_id": "mock-research-request",
                                    "status": "pending",
                                })),
                            )
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
        (addr, usage_calls, research_calls)
    }

    async fn spawn_http_research_mock_with_usage_diff_string_float(
        expected_api_key: String,
        base_research_usage: i64,
        delta: i64,
    ) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let usage_calls = Arc::new(AtomicUsize::new(0));
        let research_calls = Arc::new(AtomicUsize::new(0));
        let expected_api_key_usage = expected_api_key.clone();
        let expected_api_key_research = expected_api_key;
        let app = Router::new()
            .route(
                "/usage",
                get({
                    let usage_calls = usage_calls.clone();
                    move |headers: HeaderMap| {
                        let expected_api_key = expected_api_key_usage.clone();
                        let usage_calls = usage_calls.clone();
                        async move {
                            let call_index = usage_calls.fetch_add(1, Ordering::SeqCst) + 1;
                            assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                            let research_usage = if call_index <= 1 {
                                base_research_usage
                            } else {
                                base_research_usage + delta
                            };
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "key": { "research_usage": format!("{research_usage}.0") }
                                })),
                            )
                        }
                    }
                }),
            )
            .route(
                "/research",
                post({
                    let research_calls = research_calls.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let expected_api_key = expected_api_key_research.clone();
                        let research_calls = research_calls.clone();
                        async move {
                            research_calls.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "request_id": "mock-research-request",
                                    "status": "pending",
                                })),
                            )
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
        (addr, usage_calls, research_calls)
    }

    async fn spawn_http_research_mock_with_usage_probe_failure(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let usage_calls = Arc::new(AtomicUsize::new(0));
        let research_calls = Arc::new(AtomicUsize::new(0));
        let expected_api_key_usage = expected_api_key.clone();
        let expected_api_key_research = expected_api_key;
        let app = Router::new()
            .route(
                "/usage",
                get({
                    let usage_calls = usage_calls.clone();
                    move |headers: HeaderMap| {
                        let expected_api_key = expected_api_key_usage.clone();
                        let usage_calls = usage_calls.clone();
                        async move {
                            usage_calls.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_bearer_auth(&headers, &expected_api_key, "/usage");
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Body::from("mock usage probe failure"),
                            )
                        }
                    }
                }),
            )
            .route(
                "/research",
                post({
                    let research_calls = research_calls.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let expected_api_key = expected_api_key_research.clone();
                        let research_calls = research_calls.clone();
                        async move {
                            research_calls.fetch_add(1, Ordering::SeqCst);
                            assert_upstream_json_auth(&headers, &body, &expected_api_key, "/research");
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "request_id": "mock-research-request",
                                    "status": "pending",
                                })),
                            )
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
        (addr, usage_calls, research_calls)
    }

    async fn spawn_http_research_result_mock_asserting_bearer(
        expected_api_key: String,
        expected_request_id: String,
    ) -> SocketAddr {
        let app = Router::new().route(
            "/research/:request_id",
            get({
                move |headers: HeaderMap, Path(request_id): Path<String>| {
                    let expected_api_key = expected_api_key.clone();
                    let expected_request_id = expected_request_id.clone();
                    async move {
                        assert_eq!(
                            request_id, expected_request_id,
                            "upstream research result path should contain the request id"
                        );
                        assert_upstream_bearer_auth(&headers, &expected_api_key, "/research/:request_id");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": request_id,
                                "status": "pending",
                            })),
                        )
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
        addr
    }

    async fn spawn_http_research_mock_requiring_same_key_for_result() -> SocketAddr {
        let request_key_map: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let app = Router::new()
            .route(
                "/research",
                post({
                    let request_key_map = request_key_map.clone();
                    move |headers: HeaderMap, Json(body): Json<Value>| {
                        let request_key_map = request_key_map.clone();
                        async move {
                            let api_key = headers
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok())
                                .and_then(|v| v.strip_prefix("Bearer "))
                                .unwrap_or("")
                                .to_string();
                            assert!(
                                !api_key.is_empty(),
                                "upstream Authorization for /research should include bearer key"
                            );
                            let request_id = body
                                .get("input")
                                .and_then(|v| v.as_str())
                                .map(|v| format!("req-{v}"))
                                .unwrap_or_else(|| "req-same-key".to_string());
                            {
                                let mut guard = request_key_map
                                    .lock()
                                    .expect("request key map lock should not be poisoned");
                                guard.insert(request_id.clone(), api_key);
                            }
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "request_id": request_id,
                                    "status": "pending",
                                })),
                            )
                        }
                    }
                }),
            )
            .route(
                "/research/:request_id",
                get({
                    let request_key_map = request_key_map.clone();
                    move |headers: HeaderMap, Path(request_id): Path<String>| {
                        let request_key_map = request_key_map.clone();
                        async move {
                            let api_key = headers
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|v| v.to_str().ok())
                                .and_then(|v| v.strip_prefix("Bearer "))
                                .unwrap_or("")
                                .to_string();
                            let expected_api_key = {
                                let guard = request_key_map
                                    .lock()
                                    .expect("request key map lock should not be poisoned");
                                guard.get(&request_id).cloned()
                            };
                            match expected_api_key {
                                Some(expected) if expected == api_key => (
                                    StatusCode::OK,
                                    Json(serde_json::json!({
                                        "request_id": request_id,
                                        "status": "pending",
                                    })),
                                ),
                                Some(_) => (
                                    StatusCode::UNAUTHORIZED,
                                    Json(serde_json::json!({
                                        "detail": { "error": "Unauthorized: key mismatch." }
                                    })),
                                ),
                                None => (
                                    StatusCode::NOT_FOUND,
                                    Json(serde_json::json!({
                                        "detail": { "error": "Research task not found." }
                                    })),
                                ),
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
        addr
    }

    async fn spawn_proxy_server(proxy: TavilyProxy, usage_base: String) -> SocketAddr {
        spawn_proxy_server_with_dev(proxy, usage_base, false).await
    }

    async fn spawn_proxy_server_with_dev(
        proxy: TavilyProxy,
        usage_base: String,
        dev_open_admin: bool,
    ) -> SocketAddr {
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin,
            usage_base,
        });

        let app = Router::new()
            .route("/mcp", any(proxy_handler))
            .route("/mcp/*path", any(proxy_handler))
            .route("/api/tavily/search", post(tavily_http_search))
            .route("/api/tavily/extract", post(tavily_http_extract))
            .route("/api/tavily/crawl", post(tavily_http_crawl))
            .route("/api/tavily/map", post(tavily_http_map))
            .route("/api/tavily/research", post(tavily_http_research))
            .route(
                "/api/tavily/research/:request_id",
                get(tavily_http_research_result),
            )
            .route("/api/tavily/usage", get(tavily_http_usage))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        addr
    }

    async fn spawn_keys_admin_server(
        proxy: TavilyProxy,
        forward_auth: ForwardAuthConfig,
        dev_open_admin: bool,
    ) -> SocketAddr {
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth,
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin,
            usage_base: "http://127.0.0.1:58088".to_string(),
        });

        let app = Router::new()
            .route("/api/keys/batch", post(create_api_keys_batch))
            .route("/api/admin/login", post(post_admin_login))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        addr
    }

    async fn spawn_keys_admin_server_with_usage_base(
        proxy: TavilyProxy,
        forward_auth: ForwardAuthConfig,
        dev_open_admin: bool,
        usage_base: String,
    ) -> SocketAddr {
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth,
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin,
            usage_base,
        });

        let app = Router::new()
            .route("/api/keys/batch", post(create_api_keys_batch))
            .route("/api/keys/validate", post(post_validate_api_keys))
            .route("/api/admin/login", post(post_admin_login))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        addr
    }

    async fn spawn_usage_mock_server() -> SocketAddr {
        let app = Router::new().route(
            "/usage",
            get(|headers: HeaderMap| async move {
                let auth = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                match auth {
                    // ok: remaining > 0
                    "Bearer tvly-ok" => (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "key": { "limit": 1000, "usage": 10 },
                        })),
                    )
                        .into_response(),
                    // ok_exhausted: remaining == 0
                    "Bearer tvly-exhausted" => (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "key": { "limit": 1000, "usage": 1000 },
                        })),
                    )
                        .into_response(),
                    // unauthorized
                    "Bearer tvly-unauth" => {
                        (StatusCode::UNAUTHORIZED, Body::from("unauthorized")).into_response()
                    }
                    // forbidden
                    "Bearer tvly-forbidden" => {
                        (StatusCode::FORBIDDEN, Body::from("forbidden")).into_response()
                    }
                    // rate-limited transient client error
                    "Bearer tvly-rate-limited" => {
                        (StatusCode::TOO_MANY_REQUESTS, Body::from("rate limited")).into_response()
                    }
                    _ => (StatusCode::BAD_REQUEST, Body::from("unknown key")).into_response(),
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
        addr
    }

    fn hash_admin_password_for_test(password: &str) -> String {
        use argon2::password_hash::{PasswordHasher, SaltString};

        let salt = SaltString::generate(&mut rand::rngs::OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .expect("hash builtin admin password")
            .to_string()
    }

    async fn spawn_builtin_keys_admin_server(proxy: TavilyProxy, password: &str) -> SocketAddr {
        let password_hash = hash_admin_password_for_test(password);
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(true, None, Some(password_hash)),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
        });

        let app = Router::new()
            .route("/api/admin/login", post(post_admin_login))
            .route("/api/admin/logout", post(post_admin_logout))
            .route("/api/keys/batch", post(create_api_keys_batch))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        addr
    }

    fn linuxdo_oauth_options_for_test() -> LinuxDoOAuthOptions {
        LinuxDoOAuthOptions {
            enabled: true,
            client_id: Some("linuxdo-test-client-id".to_string()),
            client_secret: Some("linuxdo-test-client-secret".to_string()),
            authorize_url: "https://connect.linux.do/oauth2/authorize".to_string(),
            token_url: "https://connect.linux.do/oauth2/token".to_string(),
            userinfo_url: "https://connect.linux.do/api/user".to_string(),
            scope: "user".to_string(),
            redirect_url: Some("http://127.0.0.1/auth/linuxdo/callback".to_string()),
            session_max_age_secs: 3600,
            login_state_ttl_secs: 600,
        }
    }

    async fn spawn_user_oauth_server_with_options(
        proxy: TavilyProxy,
        linuxdo_oauth: LinuxDoOAuthOptions,
    ) -> SocketAddr {
        let static_dir = temp_static_dir("linuxdo-user-oauth");
        let state = Arc::new(AppState {
            proxy,
            static_dir: Some(static_dir),
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
        });

        let app = Router::new()
            .route("/", get(serve_index))
            .route("/console", get(serve_console_index))
            .route("/auth/linuxdo", get(get_linuxdo_auth).post(post_linuxdo_auth))
            .route("/auth/linuxdo/callback", get(get_linuxdo_callback))
            .route("/api/profile", get(get_profile))
            .route("/api/user/token", get(get_user_token))
            .route("/api/user/dashboard", get(get_user_dashboard))
            .route("/api/user/tokens", get(get_user_tokens))
            .route("/api/user/tokens/:id", get(get_user_token_detail))
            .route("/api/user/tokens/:id/secret", get(get_user_token_secret))
            .route("/api/user/tokens/:id/logs", get(get_user_token_logs))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        addr
    }

    async fn spawn_user_oauth_server(proxy: TavilyProxy) -> SocketAddr {
        spawn_user_oauth_server_with_options(proxy, linuxdo_oauth_options_for_test()).await
    }

    async fn spawn_admin_users_server(proxy: TavilyProxy, dev_open_admin: bool) -> SocketAddr {
        let static_dir = temp_static_dir("admin-users");
        let state = Arc::new(AppState {
            proxy,
            static_dir: Some(static_dir),
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin,
            usage_base: "http://127.0.0.1:58088".to_string(),
        });

        let app = Router::new()
            .route("/api/users", get(list_users))
            .route("/api/users/:id", get(get_user_detail))
            .route("/api/users/:id/quota", patch(update_user_quota))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        addr
    }


    async fn spawn_linuxdo_authorize_method_probe_server(
        method_probe: Arc<Mutex<Option<Method>>>,
    ) -> SocketAddr {
        let app = Router::new().route(
            "/oauth2/authorize",
            any({
                let method_probe = method_probe.clone();
                move |method: Method| {
                    let method_probe = method_probe.clone();
                    async move {
                        *method_probe.lock().expect("method probe lock poisoned") =
                            Some(method.clone());
                        if method == Method::GET {
                            StatusCode::OK
                        } else {
                            StatusCode::METHOD_NOT_ALLOWED
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
        addr
    }

    async fn spawn_linuxdo_oauth_mock_server(
        provider_user_id: &str,
        username: &str,
        display_name: &str,
    ) -> SocketAddr {
        let access_token = "mock-linuxdo-access-token".to_string();
        let profile = json!({
            "id": provider_user_id,
            "username": username,
            "name": display_name,
            "active": true,
            "trust_level": 3
        });

        let app = Router::new()
            .route(
                "/oauth2/token",
                post({
                    let access_token = access_token.clone();
                    move || {
                        let access_token = access_token.clone();
                        async move { (StatusCode::OK, Json(json!({ "access_token": access_token }))) }
                    }
                }),
            )
            .route(
                "/api/user",
                get({
                    let access_token = access_token.clone();
                    let profile = profile.clone();
                    move |headers: HeaderMap| {
                        let access_token = access_token.clone();
                        let profile = profile.clone();
                        async move {
                            let authorization = headers
                                .get(axum::http::header::AUTHORIZATION)
                                .and_then(|value| value.to_str().ok());
                            let expected = format!("Bearer {access_token}");
                            if authorization != Some(expected.as_str()) {
                                return (
                                    StatusCode::UNAUTHORIZED,
                                    Json(json!({ "error": "invalid_token" })),
                                );
                            }
                            (StatusCode::OK, Json(profile))
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
        addr
    }

    fn find_cookie_pair(
        headers: &reqwest::header::HeaderMap,
        cookie_name: &str,
    ) -> Option<String> {
        headers
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .map(str::trim)
            .find(|pair| pair.split_once('=').is_some_and(|(name, _)| name == cookie_name))
            .map(str::to_string)
    }

    #[tokio::test]
    async fn tavily_http_search_returns_401_without_token() {
        let db_path = temp_db_path("http-search-401-missing");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-any-limit-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy, usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "query": "test" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("error"),
            Some(&serde_json::Value::String("missing token".into()))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_dev_open_admin_does_not_fail_foreign_key() {
        let db_path = temp_db_path("http-search-dev-open-admin-fk");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-dev-open-admin-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server_with_dev(proxy, usage_base, true).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "query": "dev-open-admin fk" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_returns_403_for_non_admin() {
        let db_path = temp_db_path("keys-batch-403-non-admin");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "api_keys": ["k1"] }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_validate_reports_ok_exhausted_and_duplicates() {
        let db_path = temp_db_path("keys-validate-ok-exhausted");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );

        let usage_addr = spawn_usage_mock_server().await;
        let usage_base = format!("http://{}", usage_addr);
        let addr =
            spawn_keys_admin_server_with_usage_base(proxy, forward_auth, false, usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/validate", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "api_keys": ["tvly-ok", "tvly-exhausted", "tvly-unauth", "tvly-rate-limited", "tvly-ok"]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary");
        assert_eq!(summary.get("input_lines").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(summary.get("valid_lines").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(
            summary.get("unique_in_input").and_then(|v| v.as_u64()),
            Some(4)
        );
        assert_eq!(
            summary.get("duplicate_in_input").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(summary.get("ok").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("exhausted").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("invalid").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("error").and_then(|v| v.as_u64()), Some(1));

        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 5);
        assert_eq!(
            results[0].get("status").and_then(|v| v.as_str()),
            Some("ok")
        );
        assert_eq!(
            results[1].get("status").and_then(|v| v.as_str()),
            Some("ok_exhausted")
        );
        assert_eq!(
            results[2].get("status").and_then(|v| v.as_str()),
            Some("unauthorized")
        );
        assert_eq!(
            results[3].get("status").and_then(|v| v.as_str()),
            Some("error")
        );
        assert_eq!(
            results[4].get("status").and_then(|v| v.as_str()),
            Some("duplicate_in_input")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_can_mark_exhausted_by_secret() {
        let db_path = temp_db_path("keys-batch-mark-exhausted");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );

        let addr = spawn_keys_admin_server_with_usage_base(
            proxy.clone(),
            forward_auth,
            false,
            "http://127.0.0.1:58088".to_string(),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "api_keys": ["tvly-mark-exhausted"],
                "exhausted_api_keys": ["tvly-mark-exhausted"],
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].get("status").and_then(|v| v.as_str()),
            Some("created")
        );
        assert_eq!(
            results[0].get("marked_exhausted").and_then(|v| v.as_bool()),
            Some(true)
        );

        let metrics = proxy.list_api_key_metrics().await.expect("list keys");
        assert!(!metrics.is_empty(), "expected at least one key metric row");

        let mut found = None;
        for m in metrics {
            let secret = proxy
                .get_api_key_secret(&m.id)
                .await
                .expect("fetch secret")
                .unwrap_or_default();
            if secret == "tvly-mark-exhausted" {
                found = Some(m);
                break;
            }
        }
        let found = found.expect("find inserted key");
        assert_eq!(found.status, "exhausted");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_rejects_over_limit() {
        let db_path = temp_db_path("keys-batch-400-over-limit");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let api_keys: Vec<String> = (0..=API_KEYS_BATCH_LIMIT)
            .map(|i| format!("tvly-{i}"))
            .collect();

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({ "api_keys": api_keys }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("error"),
            Some(&serde_json::Value::String("too_many_items".into()))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_reports_statuses_and_is_partial_success() {
        let db_path = temp_db_path("keys-batch-mixed");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        // Pre-create: one active existing key, and one soft-deleted key.
        let _existing_id = proxy
            .add_or_undelete_key("tvly-existing")
            .await
            .expect("existing key created");
        let deleted_id = proxy
            .add_or_undelete_key("tvly-deleted")
            .await
            .expect("deleted key created");
        proxy
            .soft_delete_key_by_id(&deleted_id)
            .await
            .expect("key soft deleted");

        // Create a trigger that forces a deterministic failure for one key.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");
        sqlx::query(
            r#"
            CREATE TRIGGER fail_insert_api_key
            BEFORE INSERT ON api_keys
            WHEN NEW.api_key = 'tvly-fail'
            BEGIN
                SELECT RAISE(ABORT, 'boom');
            END;
            "#,
        )
        .execute(&pool)
        .await
        .expect("create trigger");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let input = vec![
            "  tvly-new  ".to_string(),
            "tvly-fail".to_string(),
            "tvly-new-2".to_string(),
            "tvly-existing".to_string(),
            "tvly-deleted".to_string(),
            "tvly-existing".to_string(),
            "tvly-new-2".to_string(),
            "".to_string(),
            "   ".to_string(),
        ];

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({ "api_keys": input, "group": "team-a" }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary exists");
        assert_eq!(summary.get("created").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(summary.get("undeleted").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(summary.get("existed").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            summary.get("duplicate_in_input").and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(summary.get("failed").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(
            summary.get("ignored_empty").and_then(|v| v.as_u64()),
            Some(2)
        );

        let results = body
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        assert_eq!(results.len(), 7, "empty items are ignored in results");

        let statuses: Vec<(&str, &str)> = results
            .iter()
            .map(|r| {
                (
                    r.get("api_key").and_then(|v| v.as_str()).unwrap_or(""),
                    r.get("status").and_then(|v| v.as_str()).unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            statuses,
            vec![
                ("tvly-new", "created"),
                ("tvly-fail", "failed"),
                ("tvly-new-2", "created"),
                ("tvly-existing", "existed"),
                ("tvly-deleted", "undeleted"),
                ("tvly-existing", "duplicate_in_input"),
                ("tvly-new-2", "duplicate_in_input"),
            ]
        );

        // id is present only when we hit the DB successfully.
        for (idx, expected_has_id) in [
            (0, true),
            (1, false),
            (2, true),
            (3, true),
            (4, true),
            (5, false),
            (6, false),
        ] {
            let has_id = results[idx].get("id").and_then(|v| v.as_str()).is_some();
            assert_eq!(has_id, expected_has_id, "result[{idx}] id presence");
        }

        // error is required for failed.
        let err = results[1]
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(
            err.contains("boom"),
            "failed error should include trigger message"
        );

        // DB side effects: soft-deleted key should be restored, failed key should not exist.
        let deleted_at: Option<i64> =
            sqlx::query_scalar("SELECT deleted_at FROM api_keys WHERE api_key = ?")
                .bind("tvly-deleted")
                .fetch_one(&pool)
                .await
                .expect("tvly-deleted exists");
        assert!(deleted_at.is_none(), "tvly-deleted should be undeleted");

        for key in ["tvly-new", "tvly-new-2", "tvly-existing", "tvly-deleted"] {
            let group_name: Option<String> =
                sqlx::query_scalar("SELECT group_name FROM api_keys WHERE api_key = ?")
                    .bind(key)
                    .fetch_one(&pool)
                    .await
                    .expect("key exists");
            assert_eq!(
                group_name.as_deref(),
                Some("team-a"),
                "{key} should have group_name=team-a"
            );
        }

        let fail_row: Option<String> =
            sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = ?")
                .bind("tvly-fail")
                .fetch_optional(&pool)
                .await
                .expect("query fail key");
        assert!(fail_row.is_none(), "tvly-fail should not be inserted");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_does_not_override_existing_group() {
        let db_path = temp_db_path("keys-batch-group-no-override");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        // Existing key already belongs to a group.
        proxy
            .add_or_undelete_key_in_group("tvly-existing", Some("old"))
            .await
            .expect("existing key created in old group");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({ "api_keys": ["tvly-existing"], "group": "new" }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary exists");
        assert_eq!(summary.get("existed").and_then(|v| v.as_u64()), Some(1));

        let group_name: Option<String> =
            sqlx::query_scalar("SELECT group_name FROM api_keys WHERE api_key = ?")
                .bind("tvly-existing")
                .fetch_one(&pool)
                .await
                .expect("tvly-existing exists");
        assert_eq!(
            group_name.as_deref(),
            Some("old"),
            "group_name should not be overridden for existing keys"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn builtin_admin_login_allows_admin_endpoints_and_logout_revokes() {
        let db_path = temp_db_path("builtin-admin-login");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let password = "pw-123";
        let addr = spawn_builtin_keys_admin_server(proxy, password).await;

        let client = Client::new();
        let keys_url = format!("http://{}/api/keys/batch", addr);

        let resp = client
            .post(&keys_url)
            .json(&serde_json::json!({ "api_keys": ["k1"] }))
            .send()
            .await
            .expect("request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let login_url = format!("http://{}/api/admin/login", addr);
        let resp = client
            .post(&login_url)
            .json(&serde_json::json!({ "password": "wrong" }))
            .send()
            .await
            .expect("login request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let resp = client
            .post(&login_url)
            .json(&serde_json::json!({ "password": password }))
            .send()
            .await
            .expect("login request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let set_cookie = resp
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .expect("set-cookie header")
            .to_str()
            .expect("set-cookie header string");
        let cookie = set_cookie
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string();

        let resp = client
            .post(&keys_url)
            .header(reqwest::header::COOKIE, cookie.clone())
            .json(&serde_json::json!({ "api_keys": ["k1"] }))
            .send()
            .await
            .expect("request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let logout_url = format!("http://{}/api/admin/logout", addr);
        let resp = client
            .post(&logout_url)
            .header(reqwest::header::COOKIE, cookie.clone())
            .send()
            .await
            .expect("logout request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

        let resp = client
            .post(&keys_url)
            .header(reqwest::header::COOKIE, cookie)
            .json(&serde_json::json!({ "api_keys": ["k2"] }))
            .send()
            .await
            .expect("request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn user_profile_and_user_token_reflect_linuxdo_session() {
        let db_path = temp_db_path("linuxdo-profile-token");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-user-1".to_string(),
                username: Some("linuxdo_alice".to_string()),
                name: Some("LinuxDO Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert oauth user");
        let bound_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_alice"))
            .await
            .expect("ensure token binding");
        let session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create user session");

        let addr = spawn_user_oauth_server(proxy).await;
        let client = Client::new();

        let profile_url = format!("http://{}/api/profile", addr);
        let anonymous_profile_resp = client
            .get(&profile_url)
            .send()
            .await
            .expect("anonymous profile request");
        assert_eq!(anonymous_profile_resp.status(), reqwest::StatusCode::OK);
        let anonymous_profile: serde_json::Value = anonymous_profile_resp
            .json()
            .await
            .expect("anonymous profile json");
        assert_eq!(
            anonymous_profile.get("userLoggedIn"),
            Some(&serde_json::Value::Bool(false))
        );

        let user_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", session.token);
        let logged_in_profile_resp = client
            .get(&profile_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("logged-in profile request");
        assert_eq!(logged_in_profile_resp.status(), reqwest::StatusCode::OK);
        let logged_in_profile: serde_json::Value = logged_in_profile_resp
            .json()
            .await
            .expect("logged-in profile json");
        assert_eq!(
            logged_in_profile.get("userLoggedIn"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            logged_in_profile.get("userProvider"),
            Some(&serde_json::Value::String("linuxdo".to_string()))
        );
        assert_eq!(
            logged_in_profile.get("userDisplayName"),
            Some(&serde_json::Value::String("LinuxDO Alice".to_string()))
        );

        let token_url = format!("http://{}/api/user/token", addr);
        let unauth_resp = client
            .get(&token_url)
            .send()
            .await
            .expect("user token anonymous request");
        assert_eq!(unauth_resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let token_resp = client
            .get(&token_url)
            .header(reqwest::header::COOKIE, user_cookie)
            .send()
            .await
            .expect("user token request");
        assert_eq!(token_resp.status(), reqwest::StatusCode::OK);
        let token_body: serde_json::Value = token_resp.json().await.expect("user token json");
        assert_eq!(
            token_body.get("token").and_then(|value| value.as_str()),
            Some(bound_token.token.as_str())
        );

        let user_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", session.token);
        let no_redirect = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");
        let root_url = format!("http://{}/", addr);
        let root_resp = no_redirect
            .get(&root_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("root request with user session");
        assert_eq!(root_resp.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            root_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/console")
        );

        let dashboard_url = format!("http://{}/api/user/dashboard", addr);
        let dashboard_resp = client
            .get(&dashboard_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user dashboard request");
        assert_eq!(dashboard_resp.status(), reqwest::StatusCode::OK);
        let dashboard_body: serde_json::Value =
            dashboard_resp.json().await.expect("user dashboard json");
        assert_eq!(
            dashboard_body
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64()),
            Some(effective_token_hourly_request_limit())
        );

        let tokens_url = format!("http://{}/api/user/tokens", addr);
        let tokens_resp = client
            .get(&tokens_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user tokens request");
        assert_eq!(tokens_resp.status(), reqwest::StatusCode::OK);
        let tokens_body: serde_json::Value = tokens_resp.json().await.expect("user tokens json");
        let items = tokens_body.as_array().expect("tokens response is array");
        assert_eq!(items.len(), 1);
        assert_eq!(
            items
                .first()
                .and_then(|item| item.get("tokenId"))
                .and_then(|value| value.as_str()),
            Some(bound_token.id.as_str())
        );

        let token_detail_url = format!("http://{}/api/user/tokens/{}", addr, bound_token.id);
        let token_detail_resp = client
            .get(&token_detail_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token detail request");
        assert_eq!(token_detail_resp.status(), reqwest::StatusCode::OK);

        let token_secret_url = format!("http://{}/api/user/tokens/{}/secret", addr, bound_token.id);
        let token_secret_resp = client
            .get(&token_secret_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token secret request");
        assert_eq!(token_secret_resp.status(), reqwest::StatusCode::OK);
        let token_secret_body: serde_json::Value = token_secret_resp
            .json()
            .await
            .expect("user token secret json");
        assert_eq!(
            token_secret_body
                .get("token")
                .and_then(|value| value.as_str()),
            Some(bound_token.token.as_str())
        );

        let token_logs_url = format!(
            "http://{}/api/user/tokens/{}/logs?limit=20",
            addr, bound_token.id
        );
        let token_logs_resp = client
            .get(&token_logs_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token logs request");
        assert_eq!(token_logs_resp.status(), reqwest::StatusCode::OK);

        let forbidden_detail_url = format!("http://{}/api/user/tokens/notmine", addr);
        let forbidden_detail_resp = client
            .get(&forbidden_detail_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("forbidden token detail request");
        assert_eq!(
            forbidden_detail_resp.status(),
            reqwest::StatusCode::NOT_FOUND
        );

        let unauth_dashboard = client
            .get(&dashboard_url)
            .send()
            .await
            .expect("unauth dashboard request");
        assert_eq!(unauth_dashboard.status(), reqwest::StatusCode::UNAUTHORIZED);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn post_linuxdo_auth_persists_preferred_token_id_in_oauth_state() {
        let db_path = temp_db_path("linuxdo-auth-post-preferred-token");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let preferred = proxy
            .create_access_token(Some("linuxdo:preferred"))
            .await
            .expect("create preferred token");

        let addr = spawn_user_oauth_server(proxy).await;
        let no_redirect = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_url = format!("http://{}/auth/linuxdo", addr);
        let response = no_redirect
            .post(&auth_url)
            .form(&[("token", preferred.token.clone())])
            .send()
            .await
            .expect("post linuxdo auth");

        assert_eq!(response.status(), reqwest::StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("location header");
        let location_url = reqwest::Url::parse(location).expect("parse redirect location");
        let state_value = location_url
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("state query param");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");

        let (bind_token_id,): (Option<String>,) =
            sqlx::query_as("SELECT bind_token_id FROM oauth_login_states WHERE state = ? LIMIT 1")
                .bind(state_value)
                .fetch_one(&pool)
                .await
                .expect("query oauth state");
        assert_eq!(
            bind_token_id.as_deref(),
            Some(preferred.id.as_str()),
            "preferred token id should be persisted in oauth state"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn post_linuxdo_auth_follow_redirect_uses_get_method() {
        let db_path = temp_db_path("linuxdo-auth-post-follow-redirect-method");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let preferred = proxy
            .create_access_token(Some("linuxdo:preferred"))
            .await
            .expect("create preferred token");

        let method_probe = Arc::new(Mutex::new(None));
        let oauth_upstream = spawn_linuxdo_authorize_method_probe_server(method_probe.clone()).await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let client = Client::new();
        let auth_url = format!("http://{}/auth/linuxdo", addr);
        let response = client
            .post(&auth_url)
            .form(&[("token", preferred.token.clone())])
            .send()
            .await
            .expect("post linuxdo auth");

        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "redirect follow should succeed when authorize endpoint receives GET"
        );
        assert_eq!(
            *method_probe.lock().expect("method probe lock poisoned"),
            Some(Method::GET),
            "authorize endpoint should be called with GET (303 See Other redirect)"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_rebinds_preferred_token_end_to_end() {
        let db_path = temp_db_path("linuxdo-callback-rebind-preferred-e2e");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-e2e-user".to_string(),
                username: Some("linuxdo_e2e".to_string()),
                name: Some("LinuxDO E2E".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let preferred = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_e2e"))
            .await
            .expect("create preferred binding");
        let mistaken = proxy
            .create_access_token(Some("linuxdo:mistaken"))
            .await
            .expect("create mistaken token");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");
        sqlx::query("UPDATE user_token_bindings SET token_id = ? WHERE user_id = ?")
            .bind(&mistaken.id)
            .bind(&user.user_id)
            .execute(&pool)
            .await
            .expect("simulate mistaken historical binding");

        let oauth_upstream = spawn_linuxdo_oauth_mock_server(
            "linuxdo-e2e-user",
            "linuxdo_e2e",
            "LinuxDO E2E",
        )
        .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let no_redirect = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_url = format!("http://{}/auth/linuxdo", addr);
        let auth_resp = no_redirect
            .post(&auth_url)
            .form(&[("token", preferred.token.clone())])
            .send()
            .await
            .expect("start linuxdo oauth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);

        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_url = format!("http://{}/auth/linuxdo/callback?code=e2e-code&state={state}", addr);
        let callback_resp = no_redirect
            .get(&callback_url)
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            callback_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/console")
        );

        let user_cookie = find_cookie_pair(callback_resp.headers(), USER_SESSION_COOKIE_NAME)
            .expect("user session cookie");
        let token_resp = Client::new()
            .get(format!("http://{}/api/user/token", addr))
            .header(reqwest::header::COOKIE, user_cookie)
            .send()
            .await
            .expect("get user token");
        assert_eq!(token_resp.status(), reqwest::StatusCode::OK);
        let token_body: serde_json::Value = token_resp.json().await.expect("token body");
        assert_eq!(
            token_body.get("token").and_then(|value| value.as_str()),
            Some(preferred.token.as_str())
        );

        let (bound_token_id,): (String,) =
            sqlx::query_as("SELECT token_id FROM user_token_bindings WHERE user_id = ? LIMIT 1")
                .bind(&user.user_id)
                .fetch_one(&pool)
                .await
                .expect("query rebound token");
        assert_eq!(bound_token_id, preferred.id);

        let mistaken_owner =
            sqlx::query_scalar::<_, Option<String>>("SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1")
                .bind(&mistaken.id)
                .fetch_optional(&pool)
                .await
                .expect("query mistaken owner")
                .flatten();
        assert!(
            mistaken_owner.is_none(),
            "mistaken token should remain unbound after self-heal rebind"
        );

        let (enabled, deleted_at): (i64, Option<i64>) =
            sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ? LIMIT 1")
                .bind(&mistaken.id)
                .fetch_one(&pool)
                .await
                .expect("query mistaken token state");
        assert_eq!(enabled, 1, "mistaken token should stay active");
        assert!(
            deleted_at.is_none(),
            "mistaken token should stay non-deleted"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn revoking_user_sessions_does_not_break_builtin_admin_session() {
        let db_path = temp_db_path("user-session-revoke-vs-admin-session");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-revoke-user".to_string(),
                username: Some("linuxdo_revoke".to_string()),
                name: Some("LinuxDO Revoke".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let _user_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_revoke"))
            .await
            .expect("ensure user token");
        let user_session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create user session");

        let user_addr = spawn_user_oauth_server(proxy.clone()).await;
        let admin_password = "pw-user-revoke-admin";
        let admin_addr = spawn_builtin_keys_admin_server(proxy.clone(), admin_password).await;
        let client = Client::new();

        let user_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", user_session.token);
        let before_user_resp = client
            .get(format!("http://{}/api/user/token", user_addr))
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token before revoke");
        assert_eq!(before_user_resp.status(), reqwest::StatusCode::OK);

        let login_resp = client
            .post(format!("http://{}/api/admin/login", admin_addr))
            .json(&serde_json::json!({ "password": admin_password }))
            .send()
            .await
            .expect("admin login");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
            .expect("admin session cookie");

        let admin_before_resp = client
            .post(format!("http://{}/api/keys/batch", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .json(&serde_json::json!({ "api_keys": ["k-user-revoke-admin"] }))
            .send()
            .await
            .expect("admin endpoint before revoke");
        assert_eq!(admin_before_resp.status(), reqwest::StatusCode::OK);

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");
        sqlx::query("UPDATE user_sessions SET revoked_at = ? WHERE revoked_at IS NULL")
            .bind(Utc::now().timestamp())
            .execute(&pool)
            .await
            .expect("revoke user sessions");

        let after_user_resp = client
            .get(format!("http://{}/api/user/token", user_addr))
            .header(reqwest::header::COOKIE, user_cookie)
            .send()
            .await
            .expect("user token after revoke");
        assert_eq!(after_user_resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let admin_after_resp = client
            .post(format!("http://{}/api/keys/batch", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .json(&serde_json::json!({ "api_keys": ["k-user-revoke-admin-2"] }))
            .send()
            .await
            .expect("admin endpoint after revoke");
        assert_eq!(admin_after_resp.status(), reqwest::StatusCode::OK);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_user_management_lists_details_and_updates_quota() {
        let db_path = temp_db_path("admin-users");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-alice".to_string(),
                username: Some("alice".to_string()),
                name: Some("Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");
        let bob = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-bob".to_string(),
                username: Some("bob".to_string()),
                name: Some("Bob".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert bob");
        let _charlie = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-charlie".to_string(),
                username: Some("charlie".to_string()),
                name: Some("Charlie".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(0),
                raw_payload_json: None,
            })
            .await
            .expect("upsert charlie");

        let alice_token = proxy
            .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
            .await
            .expect("bind alice token");
        let _bob_token = proxy
            .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob"))
            .await
            .expect("bind bob token");

        let _ = proxy
            .check_token_hourly_requests(&alice_token.id)
            .await
            .expect("seed hourly-any");
        let _ = proxy
            .check_token_quota(&alice_token.id)
            .await
            .expect("seed business quota");
        proxy
            .record_token_attempt(
                &alice_token.id,
                &Method::POST,
                "/mcp",
                None,
                Some(200),
                Some(0),
                true,
                "success",
                None,
            )
            .await
            .expect("record success");
        proxy
            .record_token_attempt(
                &alice_token.id,
                &Method::POST,
                "/mcp",
                None,
                Some(500),
                Some(-32001),
                true,
                "error",
                Some("upstream error"),
            )
            .await
            .expect("record error");

        let addr = spawn_admin_users_server(proxy, true).await;
        let client = Client::new();

        let list_url = format!("http://{}/api/users?page=1&per_page=20", addr);
        let list_resp = client
            .get(&list_url)
            .send()
            .await
            .expect("list users request");
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);
        let list_body: serde_json::Value = list_resp.json().await.expect("list users json");
        let items = list_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("items is array");
        let alice_item = items
            .iter()
            .find(|item| {
                item.get("userId")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == alice.user_id)
            })
            .expect("alice row exists");
        assert_eq!(
            alice_item
                .get("tokenCount")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert!(
            alice_item
                .get("hourlyAnyUsed")
                .and_then(|value| value.as_i64())
                .unwrap_or_default()
                >= 1
        );
        assert!(
            alice_item
                .get("quotaHourlyUsed")
                .and_then(|value| value.as_i64())
                .unwrap_or_default()
                >= 1
        );

        let detail_url = format!("http://{}/api/users/{}", addr, alice.user_id);
        let detail_resp = client
            .get(&detail_url)
            .send()
            .await
            .expect("user detail request");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("user detail json");
        let before_hourly_any_used = detail_body
            .get("hourlyAnyUsed")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let tokens = detail_body
            .get("tokens")
            .and_then(|value| value.as_array())
            .expect("tokens is array");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens
                .first()
                .and_then(|value| value.get("tokenId"))
                .and_then(|value| value.as_str()),
            Some(alice_token.id.as_str())
        );

        let patch_url = format!("http://{}/api/users/{}/quota", addr, alice.user_id);
        let patch_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyAnyLimit": 123,
                "hourlyLimit": 45,
                "dailyLimit": 678,
                "monthlyLimit": 910,
            }))
            .send()
            .await
            .expect("patch user quota request");
        assert_eq!(patch_resp.status(), reqwest::StatusCode::NO_CONTENT);

        let detail_after_resp = client
            .get(&detail_url)
            .send()
            .await
            .expect("user detail after patch request");
        assert_eq!(detail_after_resp.status(), reqwest::StatusCode::OK);
        let detail_after: serde_json::Value = detail_after_resp
            .json()
            .await
            .expect("user detail after patch json");
        assert_eq!(
            detail_after
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64()),
            Some(123)
        );
        assert_eq!(
            detail_after
                .get("quotaHourlyLimit")
                .and_then(|value| value.as_i64()),
            Some(45)
        );
        assert_eq!(
            detail_after
                .get("quotaDailyLimit")
                .and_then(|value| value.as_i64()),
            Some(678)
        );
        assert_eq!(
            detail_after
                .get("quotaMonthlyLimit")
                .and_then(|value| value.as_i64()),
            Some(910)
        );
        assert_eq!(
            detail_after
                .get("hourlyAnyUsed")
                .and_then(|value| value.as_i64()),
            Some(before_hourly_any_used)
        );

        let invalid_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyAnyLimit": 0,
                "hourlyLimit": 45,
                "dailyLimit": 678,
                "monthlyLimit": 910,
            }))
            .send()
            .await
            .expect("invalid patch request");
        assert_eq!(invalid_resp.status(), reqwest::StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_user_management_requires_admin() {
        let db_path = temp_db_path("admin-users-authz");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-authz-user".to_string(),
                username: Some("authz".to_string()),
                name: Some("Authz".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");

        let addr = spawn_admin_users_server(proxy, false).await;
        let client = Client::new();

        let list_url = format!("http://{}/api/users?page=1&per_page=20", addr);
        let list_resp = client
            .get(&list_url)
            .send()
            .await
            .expect("list users unauth request");
        assert_eq!(list_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let patch_url = format!("http://{}/api/users/{}/quota", addr, user.user_id);
        let patch_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyAnyLimit": 10,
                "hourlyLimit": 10,
                "dailyLimit": 100,
                "monthlyLimit": 1000,
            }))
            .send()
            .await
            .expect("patch users unauth request");
        assert_eq!(patch_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }


    #[tokio::test]
    async fn tavily_http_search_returns_401_for_invalid_token() {
        let db_path = temp_db_path("http-search-401-invalid");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let usage_base = "http://127.0.0.1:58088".to_string();
        let proxy_addr = spawn_proxy_server(proxy, usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .header("Authorization", "Bearer th-invalid-token")
            .json(&serde_json::json!({ "query": "test" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("error"),
            Some(&serde_json::Value::String(
                "invalid or disabled token".into()
            ))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_returns_429_when_quota_exhausted_and_logs_token_attempt() {
        let db_path = temp_db_path("http-search-429-quota");
        let db_str = db_path.to_string_lossy().to_string();

        // Keep env stable across proxy creation + quota warmup.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("quota-test"))
            .await
            .expect("create token");

        // Pre-saturate hourly quota so that the next check in handler will block.
        let hourly_limit = effective_token_hourly_limit();
        for _ in 0..hourly_limit {
            let verdict = proxy
                .check_token_quota(&token.id)
                .await
                .expect("quota check ok");
            assert!(
                verdict.allowed,
                "should be allowed within limit during warmup"
            );
        }

        let usage_base = "http://127.0.0.1:58088".to_string();
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test quota" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("error"),
            Some(&serde_json::Value::String("quota_exhausted".into()))
        );

        // Verify token logs contain a quota_exhausted entry with HTTP 429.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT http_status, mcp_status, result_status
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let http_status: Option<i64> = row.try_get("http_status").unwrap();
        let mcp_status: Option<i64> = row.try_get("mcp_status").unwrap();
        let result_status: String = row.try_get("result_status").unwrap();

        assert_eq!(http_status, Some(429));
        assert_eq!(mcp_status, None);
        assert_eq!(result_status, "quota_exhausted");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_charges_credits_and_blocks_basic_without_hitting_upstream() {
        let db_path = temp_db_path("http-search-credits-basic");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let expected_api_key = "tvly-http-search-credits-basic-key";
        let (upstream_addr, hits) =
            spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-credits-basic"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        let resp1 = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test-1", "search_depth": "basic" }))
            .send()
            .await
            .expect("request 1");
        assert_eq!(resp1.status(), reqwest::StatusCode::OK);
        let verdict1 = proxy.peek_token_quota(&token.id).await.expect("peek quota 1");
        assert_eq!(verdict1.hourly_used, 1);

        let resp2 = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test-2", "search_depth": "basic" }))
            .send()
            .await
            .expect("request 2");
        assert_eq!(resp2.status(), reqwest::StatusCode::OK);
        let verdict2 = proxy.peek_token_quota(&token.id).await.expect("peek quota 2");
        assert_eq!(verdict2.hourly_used, 2);

        // Third request should be blocked by predicted cost, without hitting upstream.
        let resp3 = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test-3", "search_depth": "basic" }))
            .send()
            .await
            .expect("request 3");
        assert_eq!(resp3.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let verdict3 = proxy.peek_token_quota(&token.id).await.expect("peek quota 3");
        assert_eq!(verdict3.hourly_used, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_charges_credits_and_blocks_advanced_without_hitting_upstream() {
        let db_path = temp_db_path("http-search-credits-advanced");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let expected_api_key = "tvly-http-search-credits-advanced-key";
        let (upstream_addr, hits) =
            spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-credits-advanced"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        let resp1 = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test-1", "search_depth": "advanced" }))
            .send()
            .await
            .expect("request 1");
        assert_eq!(resp1.status(), reqwest::StatusCode::OK);
        let verdict1 = proxy.peek_token_quota(&token.id).await.expect("peek quota 1");
        assert_eq!(verdict1.hourly_used, 2);

        // Second request should be blocked (2 + 2 > 2), without hitting upstream.
        let resp2 = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test-2", "search_depth": "advanced" }))
            .send()
            .await
            .expect("request 2");
        assert_eq!(resp2.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_charges_expected_credits_when_usage_missing() {
        let db_path = temp_db_path("http-search-credits-missing-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-search-missing-usage-key";
        let (upstream_addr, hits) =
            spawn_http_search_mock_without_usage(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-credits-missing-usage"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        // Missing usage.credits should fall back to expected cost: basic=1, advanced=2.
        let basic_resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "missing-usage-basic", "search_depth": "basic" }))
            .send()
            .await
            .expect("basic request");
        assert_eq!(basic_resp.status(), reqwest::StatusCode::OK);
        let verdict1 = proxy.peek_token_quota(&token.id).await.expect("peek quota 1");
        assert_eq!(verdict1.hourly_used, 1);

        let advanced_resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(
                &serde_json::json!({ "query": "missing-usage-advanced", "search_depth": "advanced" }),
            )
            .send()
            .await
            .expect("advanced request");
        assert_eq!(advanced_resp.status(), reqwest::StatusCode::OK);
        let verdict2 = proxy.peek_token_quota(&token.id).await.expect("peek quota 2");
        assert_eq!(verdict2.hourly_used, 3);

        assert_eq!(hits.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_does_not_charge_when_structured_status_failed() {
        let db_path = temp_db_path("http-search-failed-status-no-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-search-failed-status-key";
        let (upstream_addr, hits) =
            spawn_http_search_mock_with_usage_and_failed_status(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-failed-status"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "structured-failure", "search_depth": "basic" }))
            .send()
            .await
            .expect("request");

        // Upstream returns HTTP 200 but `status: failed` in the body.
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Structured failure should not charge credits quota.
        let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_returns_upstream_response_when_billing_write_fails_after_upstream_success(
    ) {
        let db_path = temp_db_path("http-search-billing-write-fails");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-search-billing-write-fails-key";
        let arrived = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (upstream_addr, hits) = spawn_http_search_mock_with_usage_delayed(
            expected_api_key.to_string(),
            arrived.clone(),
            release.clone(),
        )
        .await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-billing-write-fails"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        let handle = tokio::spawn({
            let client = client.clone();
            let url = url.clone();
            let token = token.token.clone();
            async move {
                client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&serde_json::json!({ "query": "billing-fail", "search_depth": "basic" }))
                    .send()
                    .await
                    .expect("request")
            }
        });

        // Wait until upstream is hit (after preflight checks), then break quota tables before
        // the proxy attempts to charge credits.
        arrived.notified().await;

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");
        sqlx::query("DROP TABLE token_usage_buckets")
            .execute(&pool)
            .await
            .expect("drop token_usage_buckets");

        release.notify_one();

        let resp = handle.await.expect("task join");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let row = sqlx::query(
            r#"
            SELECT result_status, error_message
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");
        let status: String = row.try_get("result_status").unwrap();
        let message: Option<String> = row.try_get("error_message").unwrap();
        assert_eq!(status, "success");
        assert!(
            message
                .unwrap_or_default()
                .contains("charge_token_quota failed"),
            "expected charge_token_quota failure to be logged"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_concurrent_requests_do_not_bypass_quota_due_to_billing_lock() {
        let db_path = temp_db_path("http-search-concurrent-billing-lock");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-http-search-concurrent-billing-lock-key";
        let arrived = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (upstream_addr, hits) = spawn_http_search_mock_with_usage_delayed(
            expected_api_key.to_string(),
            arrived.clone(),
            release.clone(),
        )
        .await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let token = proxy
            .create_access_token(Some("http-search-concurrent-billing-lock"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        // Fire the first request and block it in the upstream mock.
        let first = tokio::spawn({
            let client = client.clone();
            let url = url.clone();
            let token = token.token.clone();
            async move {
                client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&serde_json::json!({ "query": "concurrent-1", "search_depth": "basic" }))
                    .send()
                    .await
                    .expect("first request")
            }
        });

        // Wait until the upstream is hit (after quota preflight). The proxy should be holding the
        // billing lock for this token while the request is in-flight.
        arrived.notified().await;

        let second = tokio::spawn({
            let client = client.clone();
            let url = url.clone();
            let token = token.token.clone();
            async move {
                client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&serde_json::json!({ "query": "concurrent-2", "search_depth": "basic" }))
                    .send()
                    .await
                    .expect("second request")
            }
        });

        // Give the second request time to enter the handler; it must not reach upstream while
        // the first request is still holding the billing lock.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        release.notify_one();

        let resp1 = first.await.expect("join first");
        assert_eq!(resp1.status(), reqwest::StatusCode::OK);

        let resp2 = second.await.expect("join second");
        assert_eq!(resp2.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "second request should be blocked before upstream"
        );

        let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
        assert_eq!(verdict.hourly_used, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_hourly_any_limit_429_is_non_billable_and_excluded_from_rollup() {
        let db_path = temp_db_path("http-search-hourly-any-nonbillable");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_limit_guard = EnvVarGuard::set("TOKEN_HOURLY_REQUEST_LIMIT", "1");

        let expected_api_key = "tvly-http-search-hourly-any-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("hourly-any-e2e"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);

        // 1st request should pass and hit mock upstream.
        let first = client
            .post(url.clone())
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "query": "hourly-any smoke"
            }))
            .send()
            .await
            .expect("first request succeeds");
        assert!(
            first.status().is_success(),
            "first request should be allowed, got {}",
            first.status()
        );

        // 2nd request should be blocked by hourly-any limiter before upstream.
        let second = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "query": "hourly-any blocked"
            }))
            .send()
            .await
            .expect("second request succeeds");
        assert_eq!(
            second.status(),
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "expected hourly-any 429 on second request"
        );

        // Inspect latest auth_token_logs row for hourly-any 429.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT http_status, counts_business_quota
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let http_status: Option<i64> = row.try_get("http_status").unwrap();
        let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
        assert_eq!(
            http_status,
            Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
            "latest log should be hourly-any 429"
        );
        assert_eq!(
            counts_business_quota, 0,
            "hourly-any limiter blocks should be non-billable"
        );

        // Roll up and verify billable totals only include the first request.
        let _ = proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup token usage stats");
        let summary = proxy
            .token_summary_since(&access_token.id, 0, None)
            .await
            .expect("summary since");

        assert_eq!(
            summary.total_requests, 1,
            "billable totals should count only successful first request"
        );
        assert_eq!(summary.success_count, 1);
        assert_eq!(
            summary.quota_exhausted_count, 0,
            "hourly-any 429 should not be included in billable totals"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_rejects_negative_max_results() {
        let db_path = temp_db_path("http-search-max-results-negative");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-http-search-max-results-key".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-search-max-results"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy, "http://127.0.0.1:9".to_string()).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "query": "negative max_results should be rejected",
                "max_results": -1
            }))
            .send()
            .await
            .expect("request sent");
        assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);

        let payload: Value = resp.json().await.expect("json response");
        assert_eq!(
            payload.get("error"),
            Some(&serde_json::Value::String("invalid_request".into()))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_map_is_limited_by_hourly_any_request_limiter() {
        let db_path = temp_db_path("http-map-hourly-any");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_limit_guard = EnvVarGuard::set("TOKEN_HOURLY_REQUEST_LIMIT", "1");

        let expected_api_key = "tvly-http-map-hourly-any-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("hourly-any-map"))
            .await
            .expect("create token");

        let (upstream_addr, hits) =
            spawn_http_map_mock_returning_500(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/map", proxy_addr);

        let first = client
            .post(url.clone())
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com"
            }))
            .send()
            .await
            .expect("first request");
        assert_eq!(
            first.status(),
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "first request should hit upstream and return 500"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let second = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com/second"
            }))
            .send()
            .await
            .expect("second request");
        assert_eq!(
            second.status(),
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "second request should be blocked by hourly-any limiter"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-search-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-search"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "query": "hello world"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        // Verify request_logs entry has success status, structured status, and redacted bodies.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT request_body, response_body, result_status, tavily_status_code
            FROM request_logs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("request log row exists");

        let request_body: Vec<u8> = row.try_get("request_body").unwrap();
        let response_body: Vec<u8> = row.try_get("response_body").unwrap();
        let result_status: String = row.try_get("result_status").unwrap();
        let tavily_status_code: Option<i64> = row.try_get("tavily_status_code").unwrap();

        let req_text = String::from_utf8_lossy(&request_body);
        let resp_text = String::from_utf8_lossy(&response_body);

        assert_eq!(result_status, "success");
        assert_eq!(tavily_status_code, Some(200));
        assert!(
            !req_text.contains(expected_api_key)
                && !req_text.contains(&access_token.token)
                && !resp_text.contains(expected_api_key)
                && !resp_text.contains(&access_token.token),
            "request/response logs must not contain raw api_key secrets",
        );
        assert!(
            req_text.contains("***redacted***") || !req_text.contains("api_key"),
            "api_key fields in request logs should be redacted",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_rewrites_header_token_to_tavily_bearer() {
        let db_path = temp_db_path("http-search-header-token-rewrite");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-header-rewrite-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-search-header-token"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy, usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/search", proxy_addr);
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", access_token.token))
            .json(&serde_json::json!({
                "query": "header token path"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_usage_returns_daily_and_monthly_counts() {
        let db_path = temp_db_path("http-usage-view");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-usage-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-usage"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_search_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        // One successful /search call to generate request_logs + token_logs.
        let client = Client::new();
        let search_url = format!("http://{}/api/tavily/search", proxy_addr);
        let _ = client
            .post(search_url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "query": "usage metrics test"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        // Manually record one quota_exhausted attempt for this token so that monthly_quota_exhausted > 0.
        let method = Method::GET;
        proxy
            .record_token_attempt(
                &access_token.id,
                &method,
                "/api/tavily/search",
                None,
                Some(StatusCode::TOO_MANY_REQUESTS.as_u16() as i64),
                None,
                true,
                "quota_exhausted",
                Some("test quota exhaustion"),
            )
            .await
            .expect("record token attempt");

        // Roll up auth_token_logs into token_usage_stats for the usage summary.
        let _ = proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup token usage stats");

        // Query /api/tavily/usage.
        let usage_url = format!("http://{}/api/tavily/usage", proxy_addr);
        let resp = client
            .get(usage_url)
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to /api/tavily/usage succeeds");
        let status = resp.status();
        let text = resp.text().await.expect("read usage body");

        assert!(
            status.is_success(),
            "expected success from /api/tavily/usage, got status={} body={}",
            status,
            text
        );
        let body: serde_json::Value =
            serde_json::from_str(&text).expect("parse json body from /api/tavily/usage");

        assert_eq!(
            body.get("tokenId").and_then(|v| v.as_str()),
            Some(access_token.id.as_str())
        );
        let daily_success = body
            .get("dailySuccess")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let daily_error = body
            .get("dailyError")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let monthly_success = body
            .get("monthlySuccess")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let monthly_quota_exhausted = body
            .get("monthlyQuotaExhausted")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);

        assert!(
            daily_success >= 1,
            "daily_success should be at least 1, got {daily_success}"
        );
        assert_eq!(daily_error, 0, "no error requests expected in this test");
        assert!(
            monthly_success >= daily_success,
            "monthly_success should be >= daily_success"
        );
        assert!(
            monthly_quota_exhausted >= 1,
            "expected at least one quota_exhausted event, got {monthly_quota_exhausted}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_extract_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-extract-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-extract-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-extract"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_extract_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/extract", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com"]
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_crawl_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-crawl-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-crawl-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-crawl"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_crawl_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/crawl", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com/page"]
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_map_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-map-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-map-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-map"))
            .await
            .expect("create token");

        let upstream_addr =
            spawn_http_map_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/map", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_extract_crawl_map_charge_credits_from_upstream_usage() {
        let db_path = temp_db_path("http-json-credits-charge");
        let db_str = db_path.to_string_lossy().to_string();

        // Avoid cross-test env var interference (quota verdict clamps used to limit).
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-json-credits-charge-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-json-credits-charge"))
            .await
            .expect("create token");

        // extract=0 (no charge), crawl=5, map=3
        let (upstream_addr, hits) = spawn_http_json_endpoints_mock_with_usage(
            expected_api_key.to_string(),
            0,
            5,
            3,
        )
        .await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();

        let extract_url = format!("http://{}/api/tavily/extract", proxy_addr);
        let extract_resp = client
            .post(extract_url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com"]
            }))
            .send()
            .await
            .expect("extract request");
        assert_eq!(extract_resp.status(), reqwest::StatusCode::OK);
        let verdict_after_extract = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after extract");
        assert_eq!(verdict_after_extract.hourly_used, 0);

        let crawl_url = format!("http://{}/api/tavily/crawl", proxy_addr);
        let crawl_resp = client
            .post(crawl_url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com/page"]
            }))
            .send()
            .await
            .expect("crawl request");
        assert_eq!(crawl_resp.status(), reqwest::StatusCode::OK);
        let verdict_after_crawl = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after crawl");
        assert_eq!(verdict_after_crawl.hourly_used, 5);

        let map_url = format!("http://{}/api/tavily/map", proxy_addr);
        let map_resp = client
            .post(map_url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com"
            }))
            .send()
            .await
            .expect("map request");
        assert_eq!(map_resp.status(), reqwest::StatusCode::OK);
        let verdict_after_map = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after map");
        assert_eq!(verdict_after_map.hourly_used, 8);

        assert_eq!(hits.load(Ordering::SeqCst), 3);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_extract_does_not_charge_when_usage_missing() {
        let db_path = temp_db_path("http-extract-no-usage-no-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-extract-no-usage-key";
        let upstream_addr =
            spawn_http_extract_mock_asserting_api_key(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-extract-no-usage-no-charge"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/extract", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com"]
            }))
            .send()
            .await
            .expect("extract request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_crawl_allows_last_overage_then_blocks_next_request() {
        let db_path = temp_db_path("http-crawl-credits-overage");
        let db_str = db_path.to_string_lossy().to_string();

        // Set a small hourly credits limit to validate "allow last overage" behavior.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "3");

        let expected_api_key = "tvly-http-crawl-credits-overage-key";
        let (upstream_addr, hits) = spawn_http_json_endpoints_mock_with_usage(
            expected_api_key.to_string(),
            0,
            5,
            0,
        )
        .await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-crawl-credits-overage"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/crawl", proxy_addr);

        // First request is allowed because we're not exhausted yet, even though it'll put us over.
        let first = client
            .post(&url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com/page"]
            }))
            .send()
            .await
            .expect("first request");
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict1 = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota 1");
        assert!(!verdict1.allowed);
        assert_eq!(verdict1.hourly_used, verdict1.hourly_limit);

        // Second request should be blocked because we're already exhausted, and must not hit upstream.
        let second = client
            .post(&url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com/page-2"]
            }))
            .send()
            .await
            .expect("second request");
        assert_eq!(second.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict2 = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota 2");
        assert!(!verdict2.allowed);
        assert_eq!(verdict2.hourly_used, verdict2.hourly_limit);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-research-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

        // Avoid cross-test env var interference (research uses predicted min cost enforcement).
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research"))
            .await
            .expect("create token");

        let (upstream_addr, _usage_calls, _research_calls) =
            spawn_http_research_mock_with_usage_diff(expected_api_key.to_string(), 10, 0).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "health check",
                "model": "mini"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("request_id").and_then(|v| v.as_str()),
            Some("mock-research-request")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_credits_from_usage_diff() {
        let db_path = temp_db_path("http-research-usage-diff-charge");
        let db_str = db_path.to_string_lossy().to_string();

        // Avoid cross-test env var interference.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-usage-diff-key";
        let (upstream_addr, usage_calls, research_calls) =
            spawn_http_research_mock_with_usage_diff(expected_api_key.to_string(), 10, 7).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-research-usage-diff-charge"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "usage-diff",
                "model": "mini"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 7);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 2);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_credits_from_usage_diff_when_usage_is_string_float() {
        let db_path = temp_db_path("http-research-usage-diff-string-float");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-usage-diff-string-float-key";
        let (upstream_addr, usage_calls, research_calls) =
            spawn_http_research_mock_with_usage_diff_string_float(
                expected_api_key.to_string(),
                10,
                7,
            )
            .await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-research-usage-diff-string-float"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "usage-diff-string-float",
                "model": "mini"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 7);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 2);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_min_credits_when_usage_probe_fails() {
        let db_path = temp_db_path("http-research-usage-probe-fails");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-usage-probe-fails-key";
        let (upstream_addr, usage_calls, research_calls) =
            spawn_http_research_mock_with_usage_probe_failure(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-research-usage-probe-fails"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "usage-probe-fails",
                "model": "mini"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        // /usage probe is unavailable, so we fall back to charging the minimum model cost (mini=4).
        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 4);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 2);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        // The fallback should leave an audit trail on the token log entry.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT result_status, error_message
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");
        let result_status: String = row.try_get("result_status").unwrap();
        let error_message: Option<String> = row.try_get("error_message").unwrap();
        assert_eq!(result_status, "success");
        let error_message = error_message.unwrap_or_default();
        assert!(
            error_message.contains("usage diff unavailable"),
            "expected usage diff warning, got: {error_message:?}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_min_credits_when_usage_counter_rolls_back() {
        let db_path = temp_db_path("http-research-usage-rolls-back");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-usage-rolls-back-key";
        let (upstream_addr, usage_calls, research_calls) =
            spawn_http_research_mock_with_usage_diff(expected_api_key.to_string(), 10, -1).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-research-usage-rolls-back"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "usage-rolls-back",
                "model": "mini"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        // If usage counter rolls back (after < before), treat the probe as invalid and charge
        // the minimum model cost (mini=4) to avoid silently under-billing.
        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 4);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 2);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_blocks_when_min_cost_would_exceed_quota() {
        let db_path = temp_db_path("http-research-usage-diff-block");
        let db_str = db_path.to_string_lossy().to_string();

        // Research mini minimum is 4 credits.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "3");

        let expected_api_key = "tvly-http-research-usage-diff-block-key";
        let (upstream_addr, usage_calls, research_calls) =
            spawn_http_research_mock_with_usage_diff(expected_api_key.to_string(), 10, 7).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-research-usage-diff-block"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "usage-diff-block",
                "model": "mini"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 0);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_uses_upstream_bearer_and_request_id_path() {
        let db_path = temp_db_path("http-research-result");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-research-result-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-result"))
            .await
            .expect("create token");

        let request_id = "req-test-123";
        let upstream_addr = spawn_http_research_result_mock_asserting_bearer(
            expected_api_key.to_string(),
            request_id.to_string(),
        )
        .await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy_addr = spawn_proxy_server_with_dev(proxy.clone(), usage_base, true).await;

        let client = Client::new();
        let url = format!("http://{}/api/tavily/research/{}", proxy_addr, request_id);
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(resp.status().is_success());
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("pending"));
        assert_eq!(
            body.get("request_id").and_then(|v| v.as_str()),
            Some(request_id)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_encodes_request_id_path_segment_for_upstream() {
        let db_path = temp_db_path("http-research-result-encoded-path");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-research-result-encoded-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-result-encoded-path"))
            .await
            .expect("create token");

        let request_id = "req/segment";
        let upstream_addr = spawn_http_research_result_mock_asserting_bearer(
            expected_api_key.to_string(),
            request_id.to_string(),
        )
        .await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server_with_dev(proxy.clone(), usage_base, true).await;

        let client = Client::new();
        let encoded_request_id = urlencoding::encode(request_id);
        let url = format!(
            "http://{}/api/tavily/research/{}",
            proxy_addr, encoded_request_id
        );
        let resp = client
            .get(url)
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.expect("parse json body");
        assert_eq!(
            body.get("request_id").and_then(|v| v.as_str()),
            Some(request_id)
        );
        assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("pending"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_reuses_key_selected_by_research_create() {
        let db_path = temp_db_path("http-research-result-key-affinity");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-http-research-key-a".to_string(),
                "tvly-http-research-key-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-create"))
            .await
            .expect("create token");

        let upstream_addr = spawn_http_research_mock_requiring_same_key_for_result().await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server_with_dev(proxy.clone(), usage_base, true).await;

        // Ensure the selected key's last_used_at differs from untouched keys (second-level granularity).
        tokio::time::sleep(Duration::from_millis(1_100)).await;

        let client = Client::new();
        let create_resp = client
            .post(format!("http://{}/api/tavily/research", proxy_addr))
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "same-key-check",
                "model": "mini"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert!(create_resp.status().is_success());
        let create_body: Value = create_resp.json().await.expect("parse research create response");
        let request_id = create_body
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("research create should return request_id");

        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                proxy_addr, request_id
            ))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert_eq!(
            result_resp.status(),
            StatusCode::OK,
            "result query should reuse the same upstream key selected by create step"
        );
        let result_body: Value = result_resp.json().await.expect("parse research result response");
        assert_eq!(
            result_body.get("request_id").and_then(|v| v.as_str()),
            Some(request_id)
        );
        assert_eq!(
            result_body.get("status").and_then(|v| v.as_str()),
            Some("pending")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_does_not_consume_business_quota_when_exhausted() {
        let db_path = temp_db_path("http-research-result-does-not-charge");
        let db_str = db_path.to_string_lossy().to_string();

        // Research mini minimum is 4 credits. After create, quota is exhausted, but result retrieval
        // must still succeed and must not consume more credits.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "4");

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-http-research-result-no-charge-key".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-result-no-charge"))
            .await
            .expect("create token");

        let upstream_addr = spawn_http_research_mock_requiring_same_key_for_result().await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let create_resp = client
            .post(format!("http://{}/api/tavily/research", proxy_addr))
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "no-charge-result",
                "model": "mini"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert!(create_resp.status().is_success());
        let create_body: Value = create_resp.json().await.expect("parse research create response");
        let request_id = create_body
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("research create should return request_id");

        let quota_before = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota before result query");
        assert_eq!(quota_before.hourly_used, 4);

        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                proxy_addr, request_id
            ))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert_eq!(result_resp.status(), StatusCode::OK);

        let quota_after = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after result query");
        assert_eq!(
            quota_after.hourly_used, quota_before.hourly_used,
            "research result retrieval should not consume hourly business quota"
        );
        assert_eq!(
            quota_after.daily_used, quota_before.daily_used,
            "research result retrieval should not consume daily business quota"
        );
        assert_eq!(
            quota_after.monthly_used, quota_before.monthly_used,
            "research result retrieval should not consume monthly business quota"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_rejects_request_id_from_other_token() {
        let db_path = temp_db_path("http-research-result-owner-check");
        let db_str = db_path.to_string_lossy().to_string();

        // Avoid cross-test env var interference (research uses predicted min cost enforcement).
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-http-research-key-owner-a".to_string(),
                "tvly-http-research-key-owner-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let create_token = proxy
            .create_access_token(Some("http-research-owner-create"))
            .await
            .expect("create token");
        let other_token = proxy
            .create_access_token(Some("http-research-owner-other"))
            .await
            .expect("create token");

        let upstream_addr = spawn_http_research_mock_requiring_same_key_for_result().await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let create_resp = client
            .post(format!("http://{}/api/tavily/research", proxy_addr))
            .json(&serde_json::json!({
                "api_key": create_token.token,
                "input": "owner-check",
                "model": "mini"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert!(create_resp.status().is_success());
        let create_body: Value = create_resp.json().await.expect("parse research create response");
        let request_id = create_body
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("research create should return request_id");
        let quota_before = proxy
            .token_quota_snapshot(&other_token.id)
            .await
            .expect("read quota snapshot before owner-mismatch query")
            .expect("quota snapshot should exist before owner-mismatch query");

        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                proxy_addr, request_id
            ))
            .header("Authorization", format!("Bearer {}", other_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert_eq!(result_resp.status(), StatusCode::NOT_FOUND);
        let body: Value = result_resp.json().await.expect("parse research result response");
        assert_eq!(
            body.get("error").and_then(|v| v.as_str()),
            Some("research_request_not_found")
        );
        let quota_after = proxy
            .token_quota_snapshot(&other_token.id)
            .await
            .expect("read quota snapshot after owner-mismatch query")
            .expect("quota snapshot should exist after owner-mismatch query");
        assert_eq!(
            quota_after.hourly_used, quota_before.hourly_used,
            "owner-mismatch query should not consume hourly business quota"
        );
        assert_eq!(
            quota_after.daily_used, quota_before.daily_used,
            "owner-mismatch query should not consume daily business quota"
        );
        assert_eq!(
            quota_after.monthly_used, quota_before.monthly_used,
            "owner-mismatch query should not consume monthly business quota"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_returns_500_when_owner_lookup_fails() {
        let db_path = temp_db_path("http-research-result-owner-lookup-fails");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-http-research-owner-lookup-key".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-owner-lookup"))
            .await
            .expect("create token");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        sqlx::query("DROP TABLE research_requests")
            .execute(&pool)
            .await
            .expect("drop research_requests table");

        let usage_base = "http://127.0.0.1:58088".to_string();
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                proxy_addr, "req-owner-lookup-fail"
            ))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert_eq!(result_resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let row = sqlx::query(
            r#"
            SELECT http_status, counts_business_quota, result_status
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");
        let http_status: Option<i64> = row.try_get("http_status").unwrap();
        let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
        let result_status: String = row.try_get("result_status").unwrap();

        assert_eq!(http_status, Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64));
        assert_eq!(counts_business_quota, 0);
        assert_eq!(result_status, "error");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_proxy_error_is_non_billable() {
        let db_path = temp_db_path("http-research-result-no-keys-nonbillable");
        let db_str = db_path.to_string_lossy().to_string();

        // No keys in the pool => proxy_http_get_endpoint returns ProxyError::NoAvailableKeys.
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-result-no-keys"))
            .await
            .expect("create token");

        // Insert ownership record so the handler reaches proxy_http_get_endpoint.
        let request_id = "req-no-keys";
        let now = chrono::Utc::now().timestamp();

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        sqlx::query(
            r#"
            INSERT INTO research_requests (
                request_id, key_id, token_id,
                expires_at, created_at, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(request_id)
        .bind("fake-key")
        .bind(&access_token.id)
        .bind(now + 3600)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert research request affinity");

        let usage_base = "http://127.0.0.1:58088".to_string();
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                proxy_addr, request_id
            ))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(result_resp.status(), StatusCode::BAD_GATEWAY);

        // Ensure the error path logs as non-billable for business quota rollups.
        let row = sqlx::query(
            r#"
            SELECT counts_business_quota, result_status
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
        let result_status: String = row.try_get("result_status").unwrap();
        assert_eq!(counts_business_quota, 0);
        assert_eq!(result_status, "error");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_result_survives_proxy_restart_with_persisted_affinity() {
        let db_path = temp_db_path("http-research-result-restart-affinity");
        let db_str = db_path.to_string_lossy().to_string();
        let keys = vec![
            "tvly-http-research-key-restart-a".to_string(),
            "tvly-http-research-key-restart-b".to_string(),
        ];

        let proxy = TavilyProxy::with_endpoint(keys.clone(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-restart-owner"))
            .await
            .expect("create token");

        let upstream_addr = spawn_http_research_mock_requiring_same_key_for_result().await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server_with_dev(proxy.clone(), usage_base.clone(), true).await;

        let client = Client::new();
        let create_resp = client
            .post(format!("http://{}/api/tavily/research", proxy_addr))
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "restart-check",
                "model": "mini"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");
        assert!(create_resp.status().is_success());
        let create_body: Value = create_resp.json().await.expect("parse research create response");
        let request_id = create_body
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("research create should return request_id")
            .to_string();

        // Recreate proxy from the same SQLite path to simulate a restart.
        let restarted_proxy = TavilyProxy::with_endpoint(keys, DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("restarted proxy created");
        let restarted_addr = spawn_proxy_server_with_dev(restarted_proxy, usage_base, true).await;

        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                restarted_addr, request_id
            ))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .send()
            .await
            .expect("request to restarted proxy succeeds");
        assert_eq!(
            result_resp.status(),
            StatusCode::OK,
            "restarted proxy should load persisted research affinity"
        );
        let result_body: Value = result_resp.json().await.expect("parse research result response");
        assert_eq!(
            result_body.get("request_id").and_then(|v| v.as_str()),
            Some(request_id.as_str())
        );
        assert_eq!(
            result_body.get("status").and_then(|v| v.as_str()),
            Some("pending")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_accepts_token_from_query_param() {
        let db_path = temp_db_path("e2e-query-token");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-e2e-upstream-key";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("e2e-query-param"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let resp = client
            .post(url)
            .body("{}")
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(
            resp.status().is_success(),
            "expected success from /mcp using query param token, got {}",
            resp.status()
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_non_tool_calls_are_ignored_by_business_quota() {
        let db_path = temp_db_path("mcp-non-tool-ignored");
        let db_str = db_path.to_string_lossy().to_string();

        // Tighten business hourly quota to 1 so that the token is quickly exhausted
        // for TokenQuota, while the per-hour raw request limiter still uses default.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-non-tool-key";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-non-tool"))
            .await
            .expect("create access token");

        // Pre-exhaust business quota for this token.
        let hourly_limit = effective_token_hourly_limit();
        for _ in 0..=hourly_limit {
            let _ = proxy
                .check_token_quota(&access_token.id)
                .await
                .expect("quota check ok");
        }

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        // MCP 非工具调用：tools/list 应当被业务配额忽略，但仍经过“任意请求”限频。
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "method": "tools/list" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(
            resp.status().is_success(),
            "non-tool MCP call (tools/list) should not be blocked by business quota, got {}",
            resp.status()
        );

        // Verify that the most recent auth_token_logs entry is not billable.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT counts_business_quota
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
        assert_eq!(counts_business_quota, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_resources_subscribe_and_unsubscribe_are_ignored_by_business_quota() {
        let db_path = temp_db_path("mcp-resources-subscribe-ignored");
        let db_str = db_path.to_string_lossy().to_string();

        // Tighten business hourly quota to 1 so that the token is quickly exhausted.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-resources-subscribe-key";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-resources-subscribe"))
            .await
            .expect("create access token");

        // Pre-exhaust business quota for this token.
        let hourly_limit = effective_token_hourly_limit();
        for _ in 0..=hourly_limit {
            let _ = proxy
                .check_token_quota(&access_token.id)
                .await
                .expect("quota check ok");
        }

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let subscribe = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "resources/subscribe",
                "params": { "uri": "file:///tmp/demo.txt" }
            }))
            .send()
            .await
            .expect("subscribe request");
        assert!(
            subscribe.status().is_success(),
            "resources/subscribe should not be blocked by business quota, got {}",
            subscribe.status()
        );

        let unsubscribe = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "resources/unsubscribe",
                "params": { "uri": "file:///tmp/demo.txt" }
            }))
            .send()
            .await
            .expect("unsubscribe request");
        assert!(
            unsubscribe.status().is_success(),
            "resources/unsubscribe should not be blocked by business quota, got {}",
            unsubscribe.status()
        );

        // Verify that the most recent auth_token_logs entries are not billable.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let rows = sqlx::query(
            r#"
            SELECT counts_business_quota
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 2
            "#,
        )
        .bind(&access_token.id)
        .fetch_all(&pool)
        .await
        .expect("token log rows exist");
        assert_eq!(rows.len(), 2);
        for row in rows {
            let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
            assert_eq!(counts_business_quota, 0);
        }

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_non_tavily_tool_is_ignored_by_business_quota() {
        let db_path = temp_db_path("mcp-tools-call-non-tavily-ignored");
        let db_str = db_path.to_string_lossy().to_string();

        // Tighten business hourly quota to 1 so that the token is quickly exhausted.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-tools-call-non-tavily-key";
        let (upstream_addr, hits) =
            spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-non-tavily"))
            .await
            .expect("create access token");

        // Pre-exhaust business quota for this token.
        let hourly_limit = effective_token_hourly_limit();
        for _ in 0..=hourly_limit {
            let _ = proxy
                .check_token_quota(&access_token.id)
                .await
                .expect("quota check ok");
        }

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "non-tavily-tool",
                    "arguments": { "hello": "world" }
                }
            }))
            .send()
            .await
            .expect("non-tavily tools/call request");
        assert!(
            resp.status().is_success(),
            "non-tavily tools/call should not be blocked by business quota, got {}",
            resp.status()
        );

        // Still forwards to upstream.
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Verify that the most recent auth_token_logs entry is not billable.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT counts_business_quota
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
        assert_eq!(counts_business_quota, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_initialize_and_ping_are_ignored_by_business_quota() {
        let db_path = temp_db_path("mcp-initialize-ping-ignored");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-initialize-ping-key";
        let (upstream_addr, hits) = spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-initialize-ping"))
            .await
            .expect("create access token");

        // Pre-exhaust business quota for this token.
        let hourly_limit = effective_token_hourly_limit();
        for _ in 0..=hourly_limit {
            let _ = proxy
                .check_token_quota(&access_token.id)
                .await
                .expect("quota check ok");
        }

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let init = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "initialize",
                "params": { "capabilities": {} }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(init.status().is_success());

        let ping = client
            .post(&url)
            .json(&serde_json::json!({ "method": "ping" }))
            .send()
            .await
            .expect("ping request");
        assert!(ping.status().is_success());

        assert_eq!(hits.load(Ordering::SeqCst), 2);

        // Verify that the most recent auth_token_logs entry is not billable.
        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");

        let row = sqlx::query(
            r#"
            SELECT counts_business_quota
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let counts_business_quota: i64 = row.try_get("counts_business_quota").unwrap();
        assert_eq!(counts_business_quota, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_body_is_treated_as_billable_and_blocked_when_quota_exhausted() {
        let db_path = temp_db_path("mcp-batch-body-blocked");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-batch-body-key";
        let (upstream_addr, hits) = spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-batch-body"))
            .await
            .expect("create access token");

        // Exhaust business quota first.
        proxy
            .charge_token_quota(&access_token.id, 1)
            .await
            .expect("charge business quota");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // JSON-RPC batch / non-object top-level must not bypass business quota checks.
        let resp = client
            .post(url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": {
                        "name": "tavily-search",
                        "arguments": {
                            "query": "batch bypass",
                            "search_depth": "advanced"
                        }
                    }
                }
            ]))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "upstream must not be hit when blocked");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_tools_call_tavily_search_charges_total_credits_and_blocks_next_request() {
        let db_path = temp_db_path("mcp-batch-search-credits-total");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "3");

        let expected_api_key = "tvly-mcp-batch-search-credits-total-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_batch(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-credits-total"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // basic=1 + advanced=2 => expected_total=3; should pass and charge 3 credits.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": {
                        "name": "tavily-search",
                        "arguments": {
                            "query": "batch-1",
                            "search_depth": "basic"
                        }
                    }
                },
                {
                    "method": "tools/call",
                    "params": {
                        "name": "tavily-search",
                        "arguments": {
                            "query": "batch-2",
                            "search_depth": "advanced"
                        }
                    }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict1 = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota 1");
        assert_eq!(verdict1.hourly_used, 3);

        // Next request should be blocked (3 + 1 > 3) without hitting upstream.
        let blocked = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "blocked",
                        "search_depth": "basic"
                    }
                }
            }))
            .send()
            .await
            .expect("blocked request");
        assert_eq!(blocked.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_tools_call_tavily_search_charges_usage_credits_even_when_sibling_errors() {
        let db_path = temp_db_path("mcp-batch-search-charges-with-error");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-charges-with-error-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_batch_with_error(expected_api_key.to_string())
                .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-charges-with-error"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // Second item errors, but the successful item still consumes credits upstream; we must bill
        // based on usage.credits even if the overall attempt is marked as error.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "ok", "search_depth": "basic" } }
                },
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "boom", "search_depth": "basic" } }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_tools_call_tavily_search_charges_usage_credits_even_when_sibling_quota_exhausted(
    ) {
        let db_path = temp_db_path("mcp-batch-search-charges-with-quota-exhausted");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-charges-with-quota-exhausted-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_batch_with_quota_exhausted(
                expected_api_key.to_string(),
            )
            .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-charges-with-quota-exhausted"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // Second item is quota exhausted, but the successful item still consumes credits upstream;
        // we must bill based on usage.credits.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "ok", "search_depth": "basic" } }
                },
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "quota", "search_depth": "basic" } }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_tools_call_tavily_search_does_not_overcharge_when_error_is_in_detail_status() {
        let db_path = temp_db_path("mcp-batch-search-detail-status-no-overcharge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-detail-status-no-overcharge-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_batch_with_detail_error(
                expected_api_key.to_string(),
            )
            .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-detail-status-no-overcharge"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // Both items are advanced (expected_total=4), but one fails. We must not fall back to the
        // expected credits when the response indicates an error via structuredContent.detail.status.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "ok", "search_depth": "advanced" } }
                },
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "detail-error", "search_depth": "advanced" } }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_tools_call_tavily_search_charges_expected_total_when_usage_missing_for_some_items(
    ) {
        let db_path = temp_db_path("mcp-batch-search-partial-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-partial-usage-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_batch_partial_usage(expected_api_key.to_string())
                .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-partial-usage"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // basic=1 + advanced=2 => expected_total=3. Upstream response only includes usage for
        // one item; proxy should still bill at least the expected total.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "basic", "search_depth": "basic" } }
                },
                {
                    "method": "tools/call",
                    "params": { "name": "tavily-search", "arguments": { "query": "advanced", "search_depth": "advanced" } }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 3);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_mixed_tools_list_and_search_charges_only_billable_credits() {
        let db_path = temp_db_path("mcp-batch-mixed-tools-list-search-credits");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-mixed-tools-list-search-credits-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_mixed_tools_list_and_search_usage(expected_api_key.to_string())
                .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-mixed-tools-list-search-credits"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                { "method": "tools/list", "id": 1 },
                {
                    "method": "tools/call",
                    "id": 2,
                    "params": {
                        "name": "tavily-search",
                        "arguments": { "query": "mixed batch", "search_depth": "advanced" }
                    }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(
            verdict.hourly_used, 2,
            "non-billable tools/list usage should not be included in billed credits"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_search_and_extract_adds_expected_search_when_usage_missing() {
        let db_path = temp_db_path("mcp-batch-search-extract-missing-search-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-extract-missing-search-usage-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_search_and_extract_partial_usage(
            expected_api_key.to_string(),
            3,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-extract-missing-search-usage"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // Search advanced expected=2; extract usage=3. Search is missing usage.credits so we
        // should charge 3 + 2 = 5 credits.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "id": 1,
                    "params": {
                        "name": "tavily-search",
                        "arguments": { "query": "missing usage", "search_depth": "advanced" }
                    }
                },
                {
                    "method": "tools/call",
                    "id": 2,
                    "params": {
                        "name": "tavily-extract",
                        "arguments": { "url": "https://example.com" }
                    }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 5);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_rejects_duplicate_billable_ids_without_hitting_upstream() {
        let db_path = temp_db_path("mcp-batch-duplicate-ids");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-duplicate-ids-key";
        let (upstream_addr, hits) = spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-duplicate-ids"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "id": 1,
                    "params": { "name": "tavily-search", "arguments": { "query": "dup-1", "search_depth": "basic" } }
                },
                {
                    "method": "tools/call",
                    "id": 1,
                    "params": { "name": "tavily-search", "arguments": { "query": "dup-2", "search_depth": "advanced" } }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "upstream must not be hit");

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_charges_credits_and_blocks_without_hitting_upstream() {
        let db_path = temp_db_path("mcp-tools-call-search-credits");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let expected_api_key = "tvly-mcp-tools-call-search-credits-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-credits"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let first = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "mcp credits",
                        "search_depth": "advanced"
                    }
                }
            }))
            .send()
            .await
            .expect("first request");
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        let verdict1 = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota 1");
        assert_eq!(verdict1.hourly_used, 2);

        // Second request should be blocked (2 + 2 > 2), without hitting upstream.
        let second = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "mcp credits blocked",
                        "search_depth": "advanced"
                    }
                }
            }))
            .send()
            .await
            .expect("second request");
        assert_eq!(second.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_charges_credits_from_sse_response() {
        let db_path = temp_db_path("mcp-tools-call-search-sse-credits");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-search-sse-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_sse(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-sse"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "sse credits",
                        "search_depth": "advanced"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_charges_expected_credits_when_upstream_body_is_empty() {
        let db_path = temp_db_path("mcp-tools-call-search-empty-body");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-search-empty-body-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_empty_body(expected_api_key.to_string())
                .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-empty-body"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "empty body",
                        "search_depth": "advanced"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Even without a JSON response body, search is predictable and should still charge 2.
        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_returns_upstream_response_when_billing_write_fails_after_upstream_success(
    ) {
        let db_path = temp_db_path("mcp-tools-call-search-billing-write-fails");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-search-billing-write-fails-key";
        let arrived = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_search_delayed(
            expected_api_key.to_string(),
            arrived.clone(),
            release.clone(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-billing-write-fails"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let handle = tokio::spawn({
            let client = client.clone();
            let url = url.clone();
            async move {
                client
                    .post(&url)
                    .json(&serde_json::json!({
                        "method": "tools/call",
                        "params": {
                            "name": "tavily-search",
                            "arguments": {
                                "query": "mcp billing fail",
                                "search_depth": "basic"
                            }
                        }
                    }))
                    .send()
                    .await
                    .expect("request")
            }
        });

        arrived.notified().await;

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite");
        sqlx::query("DROP TABLE token_usage_buckets")
            .execute(&pool)
            .await
            .expect("drop token_usage_buckets");

        release.notify_one();

        let resp = handle.await.expect("task join");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let row = sqlx::query(
            r#"
            SELECT result_status, error_message
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row exists");
        let status: String = row.try_get("result_status").unwrap();
        let message: Option<String> = row.try_get("error_message").unwrap();
        assert_eq!(status, "success");
        assert!(
            message
                .unwrap_or_default()
                .contains("charge_token_quota failed"),
            "expected charge_token_quota failure to be logged"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_non_search_tools_charge_credits_from_usage() {
        let db_path = temp_db_path("mcp-tools-call-non-search-credits");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-non-search-credits-key";
        // extract=3, crawl=5, map=1
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_non_search_tools(
            expected_api_key.to_string(),
            3,
            5,
            1,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-non-search-credits"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let extract = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-extract",
                    "arguments": {
                        "urls": ["https://example.com"]
                    }
                }
            }))
            .send()
            .await
            .expect("extract request");
        assert_eq!(extract.status(), reqwest::StatusCode::OK);
        let after_extract = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after extract");
        assert_eq!(after_extract.hourly_used, 3);

        let crawl = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-crawl",
                    "arguments": {
                        "urls": ["https://example.com/page"]
                    }
                }
            }))
            .send()
            .await
            .expect("crawl request");
        assert_eq!(crawl.status(), reqwest::StatusCode::OK);
        let after_crawl = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after crawl");
        assert_eq!(after_crawl.hourly_used, 8);

        let map = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-map",
                    "arguments": {
                        "url": "https://example.com"
                    }
                }
            }))
            .send()
            .await
            .expect("map request");
        assert_eq!(map.status(), reqwest::StatusCode::OK);
        let after_map = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota after map");
        assert_eq!(after_map.hourly_used, 9);

        assert_eq!(hits.load(Ordering::SeqCst), 3);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_unknown_tavily_tool_is_forwarded_and_charges_credits() {
        let db_path = temp_db_path("mcp-tools-call-unknown-tool");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-unknown-tool-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_unknown_tavily_tool(
            expected_api_key.to_string(),
            "tavily-new-tool",
            5,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-unknown-tool"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-new-tool",
                    "arguments": {
                        "foo": "bar"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 5);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_crawl_allows_last_overage_then_blocks_next_request() {
        let db_path = temp_db_path("mcp-tools-call-crawl-credits-overage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "3");

        let expected_api_key = "tvly-mcp-tools-call-crawl-credits-overage-key";
        // crawl=5 > limit=3
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_non_search_tools(
            expected_api_key.to_string(),
            0,
            5,
            0,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-crawl-credits-overage"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let first = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-crawl",
                    "arguments": {
                        "urls": ["https://example.com/page"]
                    }
                }
            }))
            .send()
            .await
            .expect("first request");
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict1 = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota 1");
        assert!(!verdict1.allowed);
        assert_eq!(verdict1.hourly_used, verdict1.hourly_limit);

        // Second request should be blocked because we are already exhausted.
        let second = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-crawl",
                    "arguments": {
                        "urls": ["https://example.com/page-2"]
                    }
                }
            }))
            .send()
            .await
            .expect("second request");
        assert_eq!(second.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict2 = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota 2");
        assert!(!verdict2.allowed);
        assert_eq!(verdict2.hourly_used, verdict2.hourly_limit);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_extract_does_not_charge_when_usage_missing() {
        let db_path = temp_db_path("mcp-tools-call-extract-no-usage-no-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-extract-no-usage-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_extract_without_usage(expected_api_key.to_string())
                .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-extract-no-usage-no-charge"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-extract",
                    "arguments": {
                        "urls": ["https://example.com"]
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_failed_status_string_does_not_charge_credits() {
        let db_path = temp_db_path("mcp-tools-call-search-failed-status-string");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-search-failed-status-string-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_search_failed_status_string(
            expected_api_key.to_string(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-failed-status-string"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "mcp failed status string",
                        "search_depth": "advanced"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Structured failure should not charge credits quota.
        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_jsonrpc_error_does_not_charge_credits() {
        let db_path = temp_db_path("mcp-tools-call-search-jsonrpc-error");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "10");

        let expected_api_key = "tvly-mcp-tools-call-search-jsonrpc-error-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search_error(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-jsonrpc-error"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "mcp jsonrpc error",
                        "search_depth": "advanced"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: Value = resp.json().await.expect("parse json body");
        assert!(
            body.get("error").is_some(),
            "mock upstream should return jsonrpc error"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0, "JSON-RPC error must not charge credits");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_list_does_not_increment_billable_totals_after_rollup() {
        let db_path = temp_db_path("mcp-nonbillable-rollup");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-nonbillable-key";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-nonbillable-rollup"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "method": "tools/list" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(
            resp.status().is_success(),
            "expected success from /mcp tools/list, got {}",
            resp.status()
        );

        let _ = proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup token usage stats");

        let summary = proxy
            .token_summary_since(&access_token.id, 0, None)
            .await
            .expect("summary since");

        assert_eq!(
            summary.total_requests, 0,
            "non-billable MCP tools/list should not affect billable totals"
        );
        assert_eq!(summary.success_count, 0);
        assert_eq!(summary.quota_exhausted_count, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rejects_invalid_token_in_query_param() {
        let db_path = temp_db_path("e2e-query-token-invalid");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-e2e-upstream-key-invalid";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, "th-invalid-unknown"
        );
        let resp = client
            .post(url)
            .body("{}")
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(
            resp.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "expected 401 for invalid query param token"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn extract_token_from_query_none_or_empty() {
        let (q, t) = extract_token_from_query(None);
        assert_eq!(q, None);
        assert_eq!(t, None);

        let (q, t) = extract_token_from_query(Some(""));
        assert_eq!(q, None);
        assert_eq!(t, None);
    }

    #[test]
    fn extract_token_from_query_single_param_case_insensitive() {
        let (q, t) = extract_token_from_query(Some("TavilyApiKey=th-abc-xyz"));
        assert_eq!(q, None, "no other params → query should be None");
        assert_eq!(t.as_deref(), Some("th-abc-xyz"));
    }

    #[test]
    fn extract_token_from_query_strips_param_and_preserves_others() {
        let (q, t) = extract_token_from_query(Some("foo=1&tavilyApiKey=th-abc-xyz&bar=2"));
        assert_eq!(t.as_deref(), Some("th-abc-xyz"));
        // Order should be preserved for non-auth params.
        assert_eq!(q.as_deref(), Some("foo=1&bar=2"));
    }

    #[test]
    fn extract_token_from_query_uses_first_non_empty_token() {
        let (q, t) =
            extract_token_from_query(Some("tavilyApiKey=&tavilyApiKey=th-abc-xyz&foo=bar"));
        assert_eq!(t.as_deref(), Some("th-abc-xyz"));
        assert_eq!(q.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn extract_token_from_query_ignores_additional_token_params() {
        let (q, t) = extract_token_from_query(Some("tavilyApiKey=th-1&tavilyApiKey=th-2&foo=bar"));
        assert_eq!(t.as_deref(), Some("th-1"));
        assert_eq!(q.as_deref(), Some("foo=bar"));
    }
}

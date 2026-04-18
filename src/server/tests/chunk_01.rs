    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::extract::{Form, Json, Query, State};
    use axum::http::{HeaderMap, Method, Uri};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{any, get, patch, post};
    use bytes::Bytes;
    use nanoid::nanoid;
    use reqwest::Client;
    use sha2::{Digest, Sha256};
    use sqlx::Row;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::future::pending;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
    use std::time::Duration;
    use tavily_hikari::{
        DEFAULT_UPSTREAM, ForwardProxySettings, effective_auth_token_log_retention_days,
        effective_request_logs_retention_days, effective_token_hourly_limit,
        request_rate_limit, request_rate_limit_window_minutes,
    };
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    fn temp_db_path(prefix: &str) -> PathBuf {
        let file = format!("{}-{}.db", prefix, nanoid!(8));
        std::env::temp_dir().join(file)
    }

    fn sha256_hex(value: &str) -> String {
        let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
        let mut hex = String::with_capacity(digest.len() * 2);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(&mut hex, "{byte:02x}");
        }
        hex
    }

    async fn connect_sqlite_test_pool(db_str: &str) -> sqlx::SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(5)
            .connect_with(options)
            .await
            .expect("connect to sqlite")
    }

    async fn sqlite_column_exists(pool: &sqlx::SqlitePool, table: &str, column: &str) -> bool {
        let sql = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ? LIMIT 1");
        sqlx::query_scalar::<_, i64>(&sql)
            .bind(column)
            .fetch_optional(pool)
            .await
            .expect("probe sqlite column")
            .is_some()
    }

    async fn fetch_api_key_rows(pool: &sqlx::SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as("SELECT id, api_key FROM api_keys ORDER BY api_key ASC")
            .fetch_all(pool)
            .await
            .expect("fetch api key rows")
    }

    async fn fetch_user_last_login_at(pool: &sqlx::SqlitePool, user_id: &str) -> Option<i64> {
        sqlx::query_scalar("SELECT last_login_at FROM users WHERE id = ? LIMIT 1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("fetch user last_login_at")
    }

    async fn fetch_user_active(pool: &sqlx::SqlitePool, user_id: &str) -> i64 {
        sqlx::query_scalar("SELECT active FROM users WHERE id = ? LIMIT 1")
            .bind(user_id)
            .fetch_one(pool)
            .await
            .expect("fetch user active")
    }

    async fn install_refresh_token_write_failure_trigger(pool: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
            CREATE TRIGGER fail_oauth_refresh_token_persist
            BEFORE UPDATE OF refresh_token_ciphertext, refresh_token_nonce ON oauth_accounts
            BEGIN
                SELECT RAISE(FAIL, 'refresh token persistence failed');
            END
            "#,
        )
        .execute(pool)
        .await
        .expect("install refresh token failure trigger");
    }

    fn find_api_key_id(rows: &[(String, String)], api_key: &str) -> String {
        rows.iter()
            .find(|(_, secret)| secret == api_key)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| panic!("missing api key row for {api_key}"))
    }

    async fn read_sse_event_until(
        response: &mut reqwest::Response,
        predicate: impl Fn(&str) -> bool,
        chunk_context: &str,
    ) -> String {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut buffer = String::new();

        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let chunk = tokio::time::timeout(remaining, response.chunk())
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {chunk_context}"))
                .unwrap_or_else(|err| panic!("failed reading {chunk_context}: {err}"))
                .unwrap_or_else(|| panic!("missing chunk for {chunk_context}"));
            buffer.push_str(
                std::str::from_utf8(&chunk)
                    .unwrap_or_else(|_| panic!("invalid utf8 while reading {chunk_context}")),
            );

            while let Some((event_chunk, rest)) = buffer.split_once("\n\n") {
                let event_chunk = event_chunk.to_string();
                buffer = rest.to_string();
                if predicate(&event_chunk) {
                    return event_chunk;
                }
            }
        }

        panic!("timed out waiting for matching SSE event: {chunk_context}");
    }

    async fn fetch_key_quota_snapshot(
        pool: &sqlx::SqlitePool,
        key_id: &str,
    ) -> (Option<i64>, Option<i64>, Option<i64>) {
        sqlx::query_as(
            "SELECT quota_limit, quota_remaining, quota_synced_at FROM api_keys WHERE id = ?",
        )
        .bind(key_id)
        .fetch_one(pool)
        .await
        .expect("fetch key quota snapshot")
    }

    async fn fetch_key_quota_sample_count(pool: &sqlx::SqlitePool, key_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM api_key_quota_sync_samples WHERE key_id = ?")
            .bind(key_id)
            .fetch_one(pool)
            .await
            .expect("fetch quota sync sample count")
    }

    async fn create_request_log_reference_tables(pool: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE users (
                id TEXT PRIMARY KEY,
                display_name TEXT,
                username TEXT,
                avatar_template TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_login_at INTEGER
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("create users");

        sqlx::query(
            r#"
            CREATE TABLE auth_tokens (
                id TEXT PRIMARY KEY,
                secret TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                note TEXT,
                group_name TEXT,
                total_requests INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                last_used_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("create auth_tokens");

        sqlx::query(
            r#"
            CREATE TABLE auth_token_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                token_id TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                http_status INTEGER,
                mcp_status INTEGER,
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                result_status TEXT NOT NULL,
                error_message TEXT,
                failure_kind TEXT,
                key_effect_code TEXT NOT NULL DEFAULT 'none',
                key_effect_summary TEXT,
                binding_effect_code TEXT NOT NULL DEFAULT 'none',
                binding_effect_summary TEXT,
                selection_effect_code TEXT NOT NULL DEFAULT 'none',
                selection_effect_summary TEXT,
                counts_business_quota INTEGER NOT NULL DEFAULT 1,
                business_credits INTEGER,
                billing_subject TEXT,
                billing_state TEXT NOT NULL DEFAULT 'none',
                api_key_id TEXT,
                request_log_id INTEGER REFERENCES request_logs(id),
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("create auth_token_logs");

        sqlx::query(
            r#"
            CREATE TABLE api_key_maintenance_records (
                id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                source TEXT NOT NULL,
                operation_code TEXT NOT NULL,
                operation_summary TEXT NOT NULL,
                reason_code TEXT,
                reason_summary TEXT,
                reason_detail TEXT,
                request_log_id INTEGER,
                auth_token_log_id INTEGER,
                auth_token_id TEXT,
                actor_user_id TEXT,
                actor_display_name TEXT,
                status_before TEXT,
                status_after TEXT,
                quarantine_before INTEGER NOT NULL DEFAULT 0,
                quarantine_after INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (key_id) REFERENCES api_keys(id),
                FOREIGN KEY (auth_token_id) REFERENCES auth_tokens(id),
                FOREIGN KEY (request_log_id) REFERENCES request_logs(id),
                FOREIGN KEY (auth_token_log_id) REFERENCES auth_token_logs(id)
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("create api_key_maintenance_records");
    }

    async fn insert_request_log_reference_rows(
        pool: &sqlx::SqlitePool,
        key_id: &str,
        request_log_id: i64,
    ) {
        let auth_token_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                result_status,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/mcp', 'error', ?, ?)
            RETURNING id
            "#,
        )
        .bind("tok-ref")
        .bind(request_log_id)
        .bind(181_i64)
        .fetch_one(pool)
        .await
        .expect("insert auth_token_log reference");

        sqlx::query(
            r#"
            INSERT INTO api_key_maintenance_records (
                id,
                key_id,
                source,
                operation_code,
                operation_summary,
                request_log_id,
                auth_token_log_id,
                created_at
            ) VALUES (?, ?, 'system', 'mark_exhausted', 'Mark Exhausted', ?, ?, ?)
            "#,
        )
        .bind("maint-ref")
        .bind(key_id)
        .bind(request_log_id)
        .bind(auth_token_log_id)
        .bind(182_i64)
        .execute(pool)
        .await
        .expect("insert maintenance reference");
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
            let lock = env_var_test_lock()
                .lock()
                .expect("env var test lock poisoned");
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
        std::fs::write(
            dir.join("registration-paused.html"),
            "<!doctype html><title>registration-paused</title>",
        )
        .expect("write registration paused");
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
                            None,
                            "proxy should not inject include_usage for MCP tools"
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

    async fn spawn_mock_mcp_upstream_for_search_and_delete_405(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |method: Method,
                      Query(params): Query<HashMap<String, String>>,
                      body: Bytes| {
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

                        if method == Method::DELETE {
                            return Response::builder()
                                .status(StatusCode::METHOD_NOT_ALLOWED)
                                .header(CONTENT_TYPE, "application/json")
                                .body(Body::from(
                                    serde_json::json!({
                                        "error": "Method Not Allowed",
                                        "message": "Method Not Allowed: Session termination not supported"
                                    })
                                    .to_string(),
                                ))
                                .expect("build delete 405 response");
                        }

                        let body: Value =
                            serde_json::from_slice(&body).expect("valid MCP JSON body");
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

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": 1 },
                                    }
                                }
                            })),
                        )
                            .into_response()
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

    async fn spawn_mock_mcp_upstream_for_search_and_delete_500(
        expected_api_key: String,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |method: Method,
                      Query(params): Query<HashMap<String, String>>,
                      body: Bytes| {
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

                        if method == Method::DELETE {
                            return Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .header(CONTENT_TYPE, "application/json")
                                .body(Body::from(
                                    serde_json::json!({
                                        "error": "Internal Server Error",
                                        "message": "delete failed upstream"
                                    })
                                    .to_string(),
                                ))
                                .expect("build delete 500 response");
                        }

                        let body: Value =
                            serde_json::from_slice(&body).expect("valid MCP JSON body");
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

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": 1 },
                                    }
                                }
                            })),
                        )
                            .into_response()
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

    async fn spawn_mock_mcp_upstream_for_session_headers(
        allowed_api_keys: Vec<String>,
    ) -> (SocketAddr, Arc<Mutex<Vec<SessionHeaderCall>>>) {
        spawn_mock_mcp_upstream_for_session_headers_with_initialize_delay(
            allowed_api_keys,
            Duration::from_millis(0),
        )
        .await
    }

    async fn spawn_mock_mcp_upstream_for_session_headers_with_initialize_delay(
        allowed_api_keys: Vec<String>,
        initialize_delay: Duration,
    ) -> (SocketAddr, Arc<Mutex<Vec<SessionHeaderCall>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new().route(
            "/mcp",
            any({
                let calls = calls.clone();
                move |headers: HeaderMap,
                      Query(params): Query<HashMap<String, String>>,
                      Json(body): Json<Value>| {
                    let allowed_api_keys = allowed_api_keys.clone();
                    let calls = calls.clone();
                    let initialize_delay = initialize_delay;
                    async move {
                        let received = params.get("tavilyApiKey").cloned();
                        assert!(
                            received.as_ref().is_some_and(|key| allowed_api_keys.iter().any(|allowed| allowed == key)),
                            "missing or incorrect tavilyApiKey: {received:?}, allowed={allowed_api_keys:?}"
                        );

                        let method = body
                            .get("method")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let session_id = headers
                            .get("mcp-session-id")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        let protocol_version = headers
                            .get("mcp-protocol-version")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        let last_event_id = headers
                            .get("last-event-id")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);
                        let leaked_forwarded = headers.contains_key("x-forwarded-for")
                            || headers.contains_key("x-real-ip");
                        let user_agent = headers
                            .get("user-agent")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string);

                        calls.lock().expect("session header calls lock poisoned").push(SessionHeaderCall {
                            method: method.clone(),
                            session_id: session_id.clone(),
                            protocol_version: protocol_version.clone(),
                            last_event_id: last_event_id.clone(),
                            leaked_forwarded,
                            user_agent,
                            tavily_api_key: received.clone(),
                        });

                        match method.as_str() {
                            "initialize" => {
                                if !initialize_delay.is_zero() {
                                    tokio::time::sleep(initialize_delay).await;
                                }
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header(CONTENT_TYPE, "application/json")
                                    .header("mcp-session-id", "session-123")
                                    .body(Body::from(
                                        serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                            "result": {
                                                "protocolVersion": "2025-03-26",
                                                "serverInfo": { "name": "mock-mcp", "version": "1.0.0" },
                                                "capabilities": {}
                                            }
                                        })
                                        .to_string(),
                                    ))
                                    .expect("build initialize response")
                            }
                            "notifications/initialized" => {
                                assert_eq!(
                                    session_id.as_deref(),
                                    Some("session-123"),
                                    "notifications/initialized should preserve mcp-session-id"
                                );
                                assert_eq!(
                                    protocol_version.as_deref(),
                                    Some("2025-03-26"),
                                    "notifications/initialized should preserve mcp-protocol-version"
                                );
                                assert!(
                                    !leaked_forwarded,
                                    "proxy must continue dropping x-forwarded-for/x-real-ip"
                                );
                                Response::builder()
                                    .status(StatusCode::ACCEPTED)
                                    .body(Body::empty())
                                    .expect("build notifications/initialized response")
                            }
                            "tools/list" => {
                                assert_eq!(
                                    session_id.as_deref(),
                                    Some("session-123"),
                                    "tools/list should preserve mcp-session-id"
                                );
                                assert_eq!(
                                    protocol_version.as_deref(),
                                    Some("2025-03-26"),
                                    "tools/list should preserve mcp-protocol-version"
                                );
                                assert!(
                                    !leaked_forwarded,
                                    "proxy must continue dropping x-forwarded-for/x-real-ip"
                                );
                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header(CONTENT_TYPE, "application/json")
                                    .body(Body::from(
                                        serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                            "result": {
                                                "tools": [
                                                    { "name": "tavily_search", "description": "mock" }
                                                ]
                                            }
                                        })
                                        .to_string(),
                                    ))
                                    .expect("build tools/list response")
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

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SessionHeaderCall {
        method: String,
        session_id: Option<String>,
        protocol_version: Option<String>,
        last_event_id: Option<String>,
        leaked_forwarded: bool,
        user_agent: Option<String>,
        tavily_api_key: Option<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct RetryAfterSessionCall {
        method: String,
        upstream_session_id_header: Option<String>,
        tavily_api_key: String,
    }

    async fn spawn_mock_mcp_upstream_for_session_retry_after_once(
        expected_api_key: String,
        retry_after_secs: i64,
    ) -> (SocketAddr, Arc<Mutex<Vec<RetryAfterSessionCall>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let tools_list_attempts = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let app = Router::new().route(
            "/mcp",
            any({
                let calls = calls.clone();
                let tools_list_attempts = tools_list_attempts.clone();
                move |headers: HeaderMap,
                      Query(params): Query<HashMap<String, String>>,
                      Json(body): Json<Value>| {
                    let calls = calls.clone();
                    let tools_list_attempts = tools_list_attempts.clone();
                    let expected_api_key = expected_api_key.clone();
                    async move {
                        let received = params.get("tavilyApiKey").cloned();
                        assert_eq!(
                            received.as_deref(),
                            Some(expected_api_key.as_str()),
                            "missing or incorrect tavilyApiKey"
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

                        calls.lock().expect("retry-after calls lock poisoned").push(
                            RetryAfterSessionCall {
                                method: method.clone(),
                                upstream_session_id_header: upstream_session_id_header.clone(),
                                tavily_api_key: expected_api_key.clone(),
                            },
                        );

                        match method.as_str() {
                            "initialize" => Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "application/json")
                                .header("mcp-session-id", "upstream-session-123")
                                .body(Body::from(
                                    serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
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
                                .expect("build notifications/initialized response"),
                            "tools/list" => {
                                let session_key = upstream_session_id_header
                                    .clone()
                                    .unwrap_or_else(|| "missing-upstream-session".to_string());
                                let attempt = {
                                    let mut attempts = tools_list_attempts
                                        .lock()
                                        .expect("tools/list attempts lock poisoned");
                                    let entry = attempts.entry(session_key).or_insert(0);
                                    *entry += 1;
                                    *entry
                                };
                                if attempt == 1 {
                                    Response::builder()
                                        .status(StatusCode::TOO_MANY_REQUESTS)
                                        .header(CONTENT_TYPE, "application/json")
                                        .header("retry-after", retry_after_secs.to_string())
                                        .body(Body::from(
                                            serde_json::json!({
                                                "jsonrpc": "2.0",
                                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                                "error": {
                                                    "code": 429,
                                                    "message": "Your request has been blocked due to excessive requests."
                                                }
                                            })
                                            .to_string(),
                                        ))
                                        .expect("build retry-after response")
                                } else {
                                    Response::builder()
                                        .status(StatusCode::OK)
                                        .header(CONTENT_TYPE, "application/json")
                                        .body(Body::from(
                                            serde_json::json!({
                                                "jsonrpc": "2.0",
                                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                                "result": {
                                                    "tools": [
                                                        { "name": "tavily_search", "description": "mock tool" }
                                                    ]
                                                }
                                            })
                                            .to_string(),
                                        ))
                                        .expect("build tools/list success response")
                                }
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

    async fn spawn_mock_mcp_upstream_for_serialized_session_requests(
        expected_api_key: String,
        tools_list_delay: Duration,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let in_flight = in_flight.clone();
                let max_in_flight = max_in_flight.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let in_flight = in_flight.clone();
                    let max_in_flight = max_in_flight.clone();
                    async move {
                        let received = params.get("tavilyApiKey").cloned();
                        assert_eq!(
                            received.as_deref(),
                            Some(expected_api_key.as_str()),
                            "missing or incorrect tavilyApiKey"
                        );

                        let method = body
                            .get("method")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default();

                        match method {
                            "initialize" => Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "application/json")
                                .header("mcp-session-id", "upstream-session-serialize")
                                .body(Body::from(
                                    serde_json::json!({
                                        "jsonrpc": "2.0",
                                        "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
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
                                .expect("build notifications/initialized response"),
                            "tools/list" => {
                                let current = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                                loop {
                                    let observed = max_in_flight.load(Ordering::SeqCst);
                                    if current <= observed {
                                        break;
                                    }
                                    if max_in_flight
                                        .compare_exchange(
                                            observed,
                                            current,
                                            Ordering::SeqCst,
                                            Ordering::SeqCst,
                                        )
                                        .is_ok()
                                    {
                                        break;
                                    }
                                }
                                tokio::time::sleep(tools_list_delay).await;
                                in_flight.fetch_sub(1, Ordering::SeqCst);

                                Response::builder()
                                    .status(StatusCode::OK)
                                    .header(CONTENT_TYPE, "application/json")
                                    .body(Body::from(
                                        serde_json::json!({
                                            "jsonrpc": "2.0",
                                            "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                            "result": {
                                                "tools": [
                                                    { "name": "tavily_search", "description": "mock tool" }
                                                ]
                                            }
                                        })
                                        .to_string(),
                                    ))
                                    .expect("build tools/list response")
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
        (addr, max_in_flight)
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
                            None,
                            "proxy should not inject include_usage for MCP tools"
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
                            None,
                            "proxy should not inject include_usage for MCP tools"
                        );

                        let id = body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1));
                        let sse = format!(
                            "data: {{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"structuredContent\":{{\"status\":200,\"usage\":{{\"credits\":2}}}}}}}}\n\n"
                        );
                        Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(sse))
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
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
                            None,
                            "proxy should not inject include_usage for MCP tools"
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
                            );

                            if name == "tavily-search" {
                                search_id = Some(
                                    map.get("id")
                                        .cloned()
                                        .unwrap_or_else(|| serde_json::json!(1)),
                                );
                            }
                            if name == "tavily-extract" {
                                extract_id = Some(
                                    map.get("id")
                                        .cloned()
                                        .unwrap_or_else(|| serde_json::json!(2)),
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

    async fn spawn_mock_mcp_upstream_for_search_and_research_missing_usage(
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

                        let mut search_seen = false;
                        let mut research_seen = false;

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

                            match name {
                                "tavily-search" => {
                                    search_seen = true;
                                    assert_eq!(
                                        map.get("params")
                                            .and_then(|p| p.get("arguments"))
                                            .and_then(|a| a.get("include_usage"))
                                            .and_then(|v| v.as_bool()),
                                        None,
                                        "proxy should not inject include_usage for MCP tools"
                                    );
                                }
                                "tavily-research" => {
                                    research_seen = true;
                                }
                                _ => panic!("unexpected tool name: {name}"),
                            }
                        }

                        assert!(search_seen, "missing tavily-search request");
                        assert!(research_seen, "missing tavily-research request");

                        // Both items succeed but omit usage.credits; the proxy must fall back to
                        // the full reserved billable total for the id-less batch.
                        (
                            StatusCode::OK,
                            Json(serde_json::json!([
                                {
                                    "jsonrpc": "2.0",
                                    "id": 1,
                                    "result": {
                                        "structuredContent": {
                                            "status": 200
                                        }
                                    }
                                },
                                {
                                    "jsonrpc": "2.0",
                                    "id": 2,
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

    async fn spawn_mock_mcp_upstream_for_idless_tavily_tool_usage(
        expected_api_key: String,
        tool_name: String,
        usage_credits: Option<i64>,
    ) -> (SocketAddr, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    let tool_name = tool_name.clone();
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
                        assert_eq!(items.len(), 1, "expected 1-item batch");

                        let item = items[0]
                            .as_object()
                            .expect("expected JSON-RPC object item in batch");
                        assert_eq!(
                            item.get("method").and_then(|v| v.as_str()),
                            Some("tools/call"),
                            "expected MCP tools/call in batch"
                        );
                        assert_eq!(
                            item.get("params")
                                .and_then(|p| p.get("name"))
                                .and_then(|v| v.as_str()),
                            Some(tool_name.as_str()),
                            "unexpected tool name"
                        );
                        assert_eq!(
                            item.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            None,
                            "proxy should not inject include_usage for MCP tools"
                        );

                        let result = match usage_credits {
                            Some(credits) => serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": 1,
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                        "usage": { "credits": credits }
                                    }
                                }
                            }),
                            None => serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": 1,
                                "result": {
                                    "structuredContent": {
                                        "status": 200
                                    }
                                }
                            }),
                        };

                        (StatusCode::OK, Json(serde_json::json!([result])))
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

    async fn spawn_mock_mcp_upstream_for_search_missing_usage_with_extract_error(
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
                                None,
                                "proxy should not inject include_usage for MCP tools"
                            );

                            if name == "tavily-search" {
                                search_id = Some(
                                    map.get("id")
                                        .cloned()
                                        .unwrap_or_else(|| serde_json::json!(1)),
                                );
                            }
                            if name == "tavily-extract" {
                                extract_id = Some(
                                    map.get("id")
                                        .cloned()
                                        .unwrap_or_else(|| serde_json::json!(2)),
                                );
                            }
                        }

                        let search_id = search_id.expect("missing tavily-search id");
                        let extract_id = extract_id.expect("missing tavily-extract id");

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
                                    "error": { "code": -32000, "message": "extract boom" }
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
                            None,
                            "proxy should not inject include_usage for MCP tools"
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
                            None,
                            "proxy should not inject include_usage for MCP tools"
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


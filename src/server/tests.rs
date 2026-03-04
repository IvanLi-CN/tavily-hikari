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
    use tavily_hikari::DEFAULT_UPSTREAM;
    use tokio::net::TcpListener;

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

    async fn spawn_http_research_mock_asserting_api_key(expected_api_key: String) -> SocketAddr {
        let app = Router::new().route(
            "/research",
            post({
                move |headers: HeaderMap, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.clone();
                    async move {
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
        addr
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

    async fn spawn_user_oauth_server(proxy: TavilyProxy) -> SocketAddr {
        let static_dir = temp_static_dir("linuxdo-user-oauth");
        let state = Arc::new(AppState {
            proxy,
            static_dir: Some(static_dir),
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: linuxdo_oauth_options_for_test(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
        });

        let app = Router::new()
            .route("/", get(serve_index))
            .route("/console", get(serve_console_index))
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
    async fn tavily_http_map_skips_hourly_any_request_limiter() {
        let db_path = temp_db_path("http-map-hourly-any-bypass");
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

        let upstream_addr = spawn_http_map_mock_asserting_api_key(expected_api_key.to_string()).await;
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
        assert!(first.status().is_success(), "first request should pass");

        let second = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com/second"
            }))
            .send()
            .await
            .expect("second request");
        assert!(
            second.status().is_success(),
            "map should not be blocked by hourly-any limit, got {}",
            second.status()
        );

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
    async fn tavily_http_research_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-research-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

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

        let upstream_addr =
            spawn_http_research_mock_asserting_api_key(expected_api_key.to_string()).await;
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
    async fn tavily_http_research_result_rejects_request_id_from_other_token() {
        let db_path = temp_db_path("http-research-result-owner-check");
        let db_str = db_path.to_string_lossy().to_string();

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

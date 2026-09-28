use super::*;
    use super::core_support_and_parsing::*;
    use tavily_hikari::providers::ProviderId;

    /// Plan todo 11 bin coverage: table-driven resolver, dual-slot client auth
    /// matrix (universal Bearer + provider-native slots), strip-before-forward
    /// (M17), M2 x-project-id exclusion, M16 provider MCP subpath rejection,
    /// and the uniform gate seams. Provider-specific execution bodies
    /// (billing settle, translation) are asserted only up to the documented
    /// todo 12-17 boundary.

    #[derive(Clone, Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        query: Option<String>,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl CapturedRequest {
        fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }

        fn has_header(&self, name: &str) -> bool {
            self.header(name).is_some()
        }
    }

    async fn spawn_capturing_mock_upstream() -> (SocketAddr, Arc<Mutex<Vec<CapturedRequest>>>) {
        let captures: Arc<Mutex<Vec<CapturedRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let captures_for_app = captures.clone();
        let app = Router::new().fallback(move |req: Request<Body>| {
            let captures = captures_for_app.clone();
            async move {
                let (parts, body) = req.into_parts();
                let body_bytes = body::to_bytes(body, BODY_LIMIT).await.unwrap_or_default();
                let headers = parts
                    .headers
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_string(),
                            value.to_str().unwrap_or("").to_string(),
                        )
                    })
                    .collect();
                let payload = serde_json::json!({
                    "mock_ok": true,
                    "method": parts.method.as_str(),
                    "path": parts.uri.path(),
                    "query": parts.uri.query(),
                });
                captures.lock().expect("captures lock").push(CapturedRequest {
                    method: parts.method.as_str().to_string(),
                    path: parts.uri.path().to_string(),
                    query: parts.uri.query().map(str::to_string),
                    headers,
                    body: body_bytes.to_vec(),
                });
                Response::builder()
                    .status(StatusCode::OK)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(payload.to_string()))
                    .expect("mock response")
            }
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service()).await.unwrap();
        });
        (addr, captures)
    }

    struct ProviderFacadeHarness {
        addr: SocketAddr,
        rest_captures: Arc<Mutex<Vec<CapturedRequest>>>,
        mcp_captures: Arc<Mutex<Vec<CapturedRequest>>>,
        token: String,
        db_str: String,
        db_path: PathBuf,
    }

    impl Drop for ProviderFacadeHarness {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.db_path);
        }
    }

    async fn spawn_provider_facade_harness(prefix: &str) -> ProviderFacadeHarness {
        spawn_provider_facade_harness_with_options(prefix, false, None).await
    }

    async fn spawn_provider_facade_harness_with_dev(
        prefix: &str,
        dev_open_admin: bool,
    ) -> ProviderFacadeHarness {
        spawn_provider_facade_harness_with_options(prefix, dev_open_admin, None).await
    }

    async fn spawn_provider_facade_harness_with_options(
        prefix: &str,
        dev_open_admin: bool,
        request_rate_limit: Option<i64>,
    ) -> ProviderFacadeHarness {
        let db_path = temp_db_path(prefix);
        let db_str = db_path.to_string_lossy().to_string();
        let (rest_addr, rest_captures) = spawn_capturing_mock_upstream().await;
        let (mcp_addr, mcp_captures) = spawn_capturing_mock_upstream().await;
        let providers = tavily_hikari::ProviderBootstrap {
            serpapi_upstream: format!("http://{rest_addr}"),
            // Path-prefixed MCP upstream (amendment M1): the executor must
            // target /gw/mcp exactly, never the client-facing ingress path.
            serpapi_mcp_upstream: format!("http://{mcp_addr}/gw/mcp"),
            exa_upstream: format!("http://{rest_addr}"),
            brave_upstream: format!("http://{rest_addr}"),
            serpapi_keys: vec!["mock-serpapi-key".to_string()],
            exa_keys: vec!["mock-exa-key".to_string()],
            brave_keys: vec!["mock-brave-key".to_string()],
        };
        let proxy = TavilyProxy::with_providers_in_ha_mode(
            vec!["tvly-provider-facade-key".to_string()],
            &format!("http://{rest_addr}/mcp"),
            &db_str,
            tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
            providers,
            tavily_hikari::HaMode::Single,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some(prefix))
            .await
            .expect("create access token");
        if let Some(limit) = request_rate_limit {
            // The hourly any-request gate reads the requestRateLimit system
            // setting (hot-reload seam shared with /api/tavily).
            let mut settings = proxy.get_system_settings().await.expect("settings");
            settings.request_rate_limit = limit;
            proxy
                .set_system_settings(&settings)
                .await
                .expect("set request rate limit");
        }
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions::disabled(),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin,
            usage_base: format!("http://{rest_addr}"),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
            dashboard_overview_cache: new_dashboard_overview_cache(),
            remote_attempt_admission: new_remote_attempt_admission(),
        });
        // Same registration the production router merges in serve.rs, plus an
        // SPA-shaped HTML fallback so the M16 "never a 302" guarantee is
        // observable on a live router.
        let app = Router::new()
            .merge(provider_facade::provider_facade_routes())
            .fallback(|req: Request<Body>| async move {
                let accepts_html = req
                    .headers()
                    .get(axum::http::header::ACCEPT)
                    .and_then(|value| value.to_str().ok())
                    .map(|value| value.to_ascii_lowercase().contains("text/html"))
                    .unwrap_or(false);
                if accepts_html {
                    Redirect::temporary("/__404").into_response()
                } else {
                    (StatusCode::NOT_FOUND, Body::empty()).into_response()
                }
            })
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service()).await.unwrap();
        });
        ProviderFacadeHarness {
            addr,
            rest_captures,
            mcp_captures,
            token: access_token.token,
            db_str,
            db_path,
        }
    }

    #[test]
    fn resolver_table_covers_every_registered_facade_route() {
        // path, provider, native upstream path, canonical kind, upstream_operation
        let expected = [
            (
                "/api/serpapi/search",
                ProviderId::Serpapi,
                "/search",
                "api:serpapi/search",
                "serpapi_http_search",
                true,
                true,
            ),
            (
                "/api/exa/search",
                ProviderId::Exa,
                "/search",
                "api:exa/search",
                "exa_http_search",
                false,
                true,
            ),
            (
                "/api/exa/contents",
                ProviderId::Exa,
                "/contents",
                "api:exa/contents",
                "exa_http_contents",
                false,
                true,
            ),
            (
                "/api/exa/answer",
                ProviderId::Exa,
                "/answer",
                "api:exa/answer",
                "exa_http_answer",
                false,
                true,
            ),
            (
                "/api/brave/res/v1/web/search",
                ProviderId::Brave,
                "/res/v1/web/search",
                "api:brave/web",
                "brave_http_web_search",
                true,
                false,
            ),
            (
                "/api/brave/res/v1/news/search",
                ProviderId::Brave,
                "/res/v1/news/search",
                "api:brave/news",
                "brave_http_news_search",
                true,
                false,
            ),
            (
                "/api/brave/res/v1/llm/context",
                ProviderId::Brave,
                "/res/v1/llm/context",
                "api:brave/llm-context",
                "brave_http_llm_context",
                true,
                false,
            ),
        ];
        for (path, provider, upstream_path, kind_key, operation, allow_get, allow_post) in expected {
            let route = provider_facade::resolve_provider_rest_route(path)
                .unwrap_or_else(|| panic!("resolver must cover {path}"));
            assert_eq!(route.provider, provider, "{path}");
            assert_eq!(route.upstream_path, upstream_path, "{path}");
            assert_eq!(route.upstream_operation, operation, "{path}");
            // The route table's operation label must equal the lib-side
            // provider-scoped vocabulary (prevents drift between the two).
            assert_eq!(
                tavily_hikari::provider_rest_upstream_operation(provider, upstream_path),
                Some(operation),
                "{path}"
            );
            assert_eq!(route.allow_get, allow_get, "{path}");
            assert_eq!(route.allow_post, allow_post, "{path}");
            assert_eq!(
                classify_token_request_kind(route.kind_path, None).key,
                kind_key,
                "{path}"
            );
            assert_eq!(provider_facade::resolve_provider_for_path(path), Some(provider));
        }
        // MCP ingress entries agree with the lib-side ingress helper.
        for provider in [ProviderId::Serpapi, ProviderId::Exa, ProviderId::Brave] {
            let ingress = provider_facade::resolve_provider_mcp_ingress(provider);
            assert_eq!(provider_facade::resolve_provider_mcp_route(ingress), Some(provider));
            assert_eq!(provider_facade::resolve_provider_for_path(ingress), Some(provider));
            assert!(tavily_hikari::is_mcp_ingress_path(provider, ingress));
            assert_eq!(
                provider_facade::resolve_provider_for_path(&format!("{ingress}/messages")),
                Some(provider)
            );
        }
    }

    #[test]
    fn resolver_rejects_unknown_and_legacy_tavily_paths() {
        for path in [
            "/api/tavily/search",
            "/mcp",
            "/mcp/messages",
            "/api/serpapi/searchx",
            "/api/serpapi",
            "/serpapi/mcpx",
            "/serpapi",
            "/exa/mcp-messages",
            "/api/brave/res/v1/web/searchx",
            "/",
            "",
        ] {
            assert_eq!(
                provider_facade::resolve_provider_for_path(path),
                None,
                "{path} must stay out of the provider resolver"
            );
        }
    }

    #[tokio::test]
    async fn serpapi_search_accepts_bearer_and_query_slot_and_strips_client_slot() {
        let harness = spawn_provider_facade_harness("provider-facade-serpapi-auth").await;
        let client = Client::new();

        // Universal Bearer slot.
        let resp = client
            .get(format!(
                "http://{}/api/serpapi/search?q=rust&engine=google",
                harness.addr
            ))
            .header("authorization", format!("Bearer {}", harness.token))
            .send()
            .await
            .expect("bearer request");
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(body.get("mock_ok").and_then(Value::as_bool), Some(true));
        assert_eq!(body.get("path").and_then(Value::as_str), Some("/search"));

        // Provider-native query slot (SerpApi api_key) carrying the Hikari token.
        let resp = client
            .get(format!(
                "http://{}/api/serpapi/search?q=rust&engine=google&api_key={}",
                harness.addr, harness.token
            ))
            .send()
            .await
            .expect("slot request");
        assert_eq!(resp.status(), StatusCode::OK);

        let captures = harness.rest_captures.lock().expect("captures lock");
        assert_eq!(captures.len(), 2);
        for captured in captures.iter() {
            let query = captured.query.clone().expect("upstream query");
            // Pooled upstream key arrives in the native SerpApi slot ...
            assert!(query.contains("api_key=mock-serpapi-key"), "{query}");
            // ... and the client Hikari token is stripped before upstream (M17).
            assert!(!query.contains("th-"), "{query}");
            assert!(!query.contains(&harness.token), "{query}");
            assert!(!captured.has_header("authorization"));
            assert!(query.contains("q=rust") && query.contains("engine=google"));
        }
    }

    #[tokio::test]
    async fn exa_facades_accept_x_api_key_and_bearer_and_strip_client_header() {
        let harness = spawn_provider_facade_harness("provider-facade-exa-auth").await;
        let client = Client::new();

        let resp = client
            .post(format!("http://{}/api/exa/search", harness.addr))
            .header("x-api-key", harness.token.clone())
            .header("content-type", "application/json")
            .body(r#"{"query":"rust"}"#)
            .send()
            .await
            .expect("exa search request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .post(format!("http://{}/api/exa/contents", harness.addr))
            .header("authorization", format!("Bearer {}", harness.token))
            .header("content-type", "application/json")
            .body(r#"{"urls":["https://example.com"]}"#)
            .send()
            .await
            .expect("exa contents request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .post(format!("http://{}/api/exa/answer", harness.addr))
            .header("x-api-key", harness.token.clone())
            .header("content-type", "application/json")
            .body(r#"{"question":"why?"}"#)
            .send()
            .await
            .expect("exa answer request");
        assert_eq!(resp.status(), StatusCode::OK);

        let captures = harness.rest_captures.lock().expect("captures lock");
        assert_eq!(captures.len(), 3);
        let expected: [(&str, &str, &[u8]); 3] = [
            ("POST", "/search", br#"{"query":"rust"}"#),
            ("POST", "/contents", br#"{"urls":["https://example.com"]}"#),
            ("POST", "/answer", br#"{"question":"why?"}"#),
        ];
        for (captured, (method, path, body)) in captures.iter().zip(expected) {
            assert_eq!(captured.method, method);
            assert_eq!(captured.path, path);
            assert_eq!(captured.body, body, "client body forwarded verbatim");
            // Pooled key replaces the client slot in the SAME native header.
            assert_eq!(captured.header("x-api-key"), Some("mock-exa-key"));
            for (_, value) in captured.headers.iter() {
                assert!(!value.contains("th-"), "client token leaked upstream: {value}");
            }
            assert!(!captured.has_header("authorization"));
        }
    }

    #[tokio::test]
    async fn brave_web_search_accepts_subscription_token_and_bearer() {
        let harness = spawn_provider_facade_harness("provider-facade-brave-auth").await;
        let client = Client::new();

        let resp = client
            .get(format!(
                "http://{}/api/brave/res/v1/web/search?q=test&count=5",
                harness.addr
            ))
            .header("x-subscription-token", harness.token.clone())
            .send()
            .await
            .expect("brave token request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .get(format!(
                "http://{}/api/brave/res/v1/news/search?q=test",
                harness.addr
            ))
            .header("authorization", format!("Bearer {}", harness.token))
            .send()
            .await
            .expect("brave bearer request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .get(format!(
                "http://{}/api/brave/res/v1/llm/context?q=test",
                harness.addr
            ))
            .header("x-subscription-token", harness.token.clone())
            .send()
            .await
            .expect("brave llm context request");
        assert_eq!(resp.status(), StatusCode::OK);

        let captures = harness.rest_captures.lock().expect("captures lock");
        assert_eq!(captures.len(), 3);
        let expected_paths = [
            "/res/v1/web/search",
            "/res/v1/news/search",
            "/res/v1/llm/context",
        ];
        for (captured, path) in captures.iter().zip(expected_paths) {
            assert_eq!(captured.method, "GET");
            assert_eq!(captured.path, path);
            assert_eq!(
                captured.header("x-subscription-token"),
                Some("mock-brave-key")
            );
            for (_, value) in captured.headers.iter() {
                assert!(!value.contains("th-"), "client token leaked upstream: {value}");
            }
            assert!(!captured.has_header("authorization"));
        }
        assert!(captures[0]
            .query
            .clone()
            .expect("query")
            .contains("count=5"));
    }

    #[tokio::test]
    async fn client_x_project_id_never_reaches_provider_upstreams() {
        // Amendment M2: x-project-id is Tavily-only policy; a client-forged
        // value must be stripped for every provider REST facade.
        let harness = spawn_provider_facade_harness("provider-facade-m2-project-id").await;
        let client = Client::new();

        let resp = client
            .get(format!(
                "http://{}/api/serpapi/search?q=test",
                harness.addr
            ))
            .header("authorization", format!("Bearer {}", harness.token))
            .header("x-project-id", "forged-project")
            .header("user-agent", "client-marker/9.9")
            .send()
            .await
            .expect("serpapi request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .post(format!("http://{}/api/exa/search", harness.addr))
            .header("x-api-key", harness.token.clone())
            .header("x-project-id", "forged-project")
            .header("user-agent", "client-marker/9.9")
            .body("{}")
            .send()
            .await
            .expect("exa request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .get(format!(
                "http://{}/api/brave/res/v1/web/search?q=test",
                harness.addr
            ))
            .header("x-subscription-token", harness.token.clone())
            .header("x-project-id", "forged-project")
            .header("user-agent", "client-marker/9.9")
            .send()
            .await
            .expect("brave request");
        assert_eq!(resp.status(), StatusCode::OK);

        let captures = harness.rest_captures.lock().expect("captures lock");
        assert_eq!(captures.len(), 3);
        for captured in captures.iter() {
            assert!(
                !captured.has_header("x-project-id"),
                "x-project-id reached {}: {:?}",
                captured.path,
                captured.headers
            );
            // The client UA never relays (the egress HTTP client's own default
            // UA is the same policy as the Tavily executors; allowlist-level
            // stripping is pinned by the lib sanitize-dispatch test).
            assert_ne!(
                captured.header("user-agent"),
                Some("client-marker/9.9"),
                "client user-agent relayed to {}: {:?}",
                captured.path,
                captured.headers
            );
        }
    }

    #[tokio::test]
    async fn foreign_and_missing_tokens_get_hikari_typed_401_shapes() {
        let harness = spawn_provider_facade_harness("provider-facade-foreign-token").await;
        let client = Client::new();

        // An Exa-shaped provider key against the Brave facade: Hikari 401,
        // never a provider-shaped error body.
        let resp = client
            .get(format!(
                "http://{}/api/brave/res/v1/web/search?q=test",
                harness.addr
            ))
            .header("x-subscription-token", "exa-shaped-not-a-hikari-token")
            .send()
            .await
            .expect("foreign token request");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json; charset=utf-8")
        );
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("invalid or disabled token")
        );
        assert_eq!(body.as_object().expect("object").len(), 1);

        // SerpApi native slot with a foreign key.
        let resp = client
            .get(format!(
                "http://{}/api/serpapi/search?q=test&api_key=not-a-hikari-token",
                harness.addr
            ))
            .send()
            .await
            .expect("foreign slot request");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("invalid or disabled token")
        );

        // Missing token on every facade family keeps the parity shape.
        let resp = client
            .post(format!("http://{}/api/exa/search", harness.addr))
            .body("{}")
            .send()
            .await
            .expect("missing token request");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("missing token")
        );

        // Nothing reached the upstream mocks.
        assert!(harness.rest_captures.lock().expect("captures lock").is_empty());
    }

    #[tokio::test]
    async fn serpapi_mcp_route_returns_typed_boundary_without_upstream_calls() {
        let harness = spawn_provider_facade_harness("provider-facade-serpapi-mcp").await;
        let client = Client::new();
        let resp = client
            .post(format!("http://{}/serpapi/mcp", harness.addr))
            .header("authorization", format!("Bearer {}", harness.token))
            .header("content-type", "application/json")
            .body(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            )
            .send()
            .await
            .expect("serpapi mcp request");
        // Documented T11 boundary: todo 15 owns the hosted passthrough body.
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("provider_mcp_pending")
        );
        // Amendment M1/C1: no upstream call happens before todo 15 wires it.
        assert!(harness.mcp_captures.lock().expect("captures lock").is_empty());
        assert!(harness.rest_captures.lock().expect("captures lock").is_empty());

        // Dev-open-admin parity with /mcp: explicit token required.
        let dev_harness =
            spawn_provider_facade_harness_with_dev("provider-facade-serpapi-mcp-dev", true).await;
        let resp = client
            .post(format!("http://{}/serpapi/mcp", dev_harness.addr))
            .header("content-type", "application/json")
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .send()
            .await
            .expect("dev open admin request");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("explicit_token_required")
        );
    }

    #[tokio::test]
    async fn exa_mcp_accepts_native_header_and_exa_api_key_query_slot() {
        let harness = spawn_provider_facade_harness("provider-facade-exa-mcp").await;
        let client = Client::new();

        // MCP query slot exaApiKey (plan todo 11 auth matrix) authenticates.
        let resp = client
            .post(format!(
                "http://{}/exa/mcp?exaApiKey={}",
                harness.addr, harness.token
            ))
            .header("content-type", "application/json")
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .send()
            .await
            .expect("exa mcp query slot request");
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);

        // Native x-api-key header slot authenticates on the MCP surface too.
        let resp = client
            .post(format!("http://{}/exa/mcp", harness.addr))
            .header("x-api-key", harness.token.clone())
            .header("content-type", "application/json")
            .body(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .send()
            .await
            .expect("exa mcp header request");
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);

        // Foreign exaApiKey stays a Hikari 401.
        let resp = client
            .post(format!("http://{}/exa/mcp?exaApiKey=foreign", harness.addr))
            .header("content-type", "application/json")
            .body(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#)
            .send()
            .await
            .expect("exa mcp foreign request");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("invalid or disabled token")
        );
    }

    #[tokio::test]
    async fn provider_mcp_subpath_reject_is_provider_tagged_and_never_spa_redirect() {
        let harness = spawn_provider_facade_harness("provider-facade-m16-reject").await;
        let client = Client::new();

        // HTML-accepting client: the registered reject route must win over the
        // SPA 302 fallback (amendment M16).
        let resp = client
            .post(format!(
                "http://{}/serpapi/mcp/messages?api_key={}&q=test",
                harness.addr, harness.token
            ))
            .header("authorization", format!("Bearer {}", harness.token))
            .header("accept", "text/html,application/xhtml+xml")
            .header("content-type", "application/json")
            .body(r#"{"probe":"raw-body"}"#)
            .send()
            .await
            .expect("serpapi subpath request");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(
            resp.headers().get("location").is_none(),
            "must never redirect to the SPA 404 landing"
        );
        assert_eq!(
            resp.headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(resp.text().await.expect("body"), "Not Found");

        // Exa + Brave subpaths follow the same reject behavior.
        let resp = client
            .get(format!("http://{}/exa/mcp/whatever", harness.addr))
            .header("x-api-key", harness.token.clone())
            .send()
            .await
            .expect("exa subpath request");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = client
            .get(format!("http://{}/brave/mcp/whatever", harness.addr))
            .header("x-subscription-token", harness.token.clone())
            .send()
            .await
            .expect("brave subpath request");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Null-key raw mcp_path_404 logs are provider-tagged (M16) with the
        // client auth slot stripped from the persisted query (M17).
        let pool = connect_sqlite_test_pool(&harness.db_str).await;
        let rows = sqlx::query(
            "SELECT provider, failure_kind, result_status, api_key_id, path, query \
             FROM request_logs ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("request logs");
        assert_eq!(rows.len(), 3);
        let expected = [
            ("serpapi", "/serpapi/mcp/messages", Some("q=test")),
            ("exa", "/exa/mcp/whatever", None),
            ("brave", "/brave/mcp/whatever", None),
        ];
        for (row, (provider, path, query)) in rows.iter().zip(expected) {
            assert_eq!(row.get::<Option<String>, _>("provider").as_deref(), Some(provider));
            assert_eq!(
                row.get::<Option<String>, _>("failure_kind").as_deref(),
                Some("mcp_path_404")
            );
            assert_eq!(
                row.get::<Option<String>, _>("result_status").as_deref(),
                Some("error")
            );
            assert!(row.get::<Option<String>, _>("api_key_id").is_none());
            assert_eq!(row.get::<String, _>("path"), path);
            assert_eq!(row.get::<Option<String>, _>("query").as_deref(), query);
        }
        pool.close().await;
    }

    #[tokio::test]
    async fn persisted_reject_rows_contain_no_client_tokens() {
        // Amendment M17: neither th- tokens nor provider key values survive
        // into persisted query text.
        let harness = spawn_provider_facade_harness("provider-facade-m17-persist").await;
        let client = Client::new();
        let resp = client
            .post(format!(
                "http://{}/serpapi/mcp/messages?api_key={}&q=secretless",
                harness.addr, harness.token
            ))
            .header("content-type", "application/json")
            .body(r#"{"probe":"m17"}"#)
            .send()
            .await
            .expect("reject request");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let pool = connect_sqlite_test_pool(&harness.db_str).await;
        let row = sqlx::query(
            "SELECT path, query, CAST(request_body AS TEXT) AS body_text \
             FROM request_logs WHERE failure_kind = 'mcp_path_404'",
        )
        .fetch_one(&pool)
        .await
        .expect("reject row");
        let query = row.get::<Option<String>, _>("query").unwrap_or_default();
        assert_eq!(query, "q=secretless");
        assert!(!query.contains("th-"));
        assert!(!query.contains(&harness.token));
        let body_text = row.get::<Option<String>, _>("body_text").unwrap_or_default();
        assert!(body_text.contains(r#"{"probe":"m17"}"#), "raw body kept");
        assert!(!body_text.contains(&harness.token));

        let token_log_leaks = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM auth_token_logs WHERE query LIKE '%th-%'",
        )
        .fetch_one(&pool)
        .await
        .expect("token log scan");
        assert_eq!(token_log_leaks, 0);
        pool.close().await;
    }

    #[tokio::test]
    async fn provider_facade_hourly_limit_gate_rejects_at_parity() {
        // requestRateLimit=1: the shared in-memory any-request limiter (same
        // seam as /api/tavily) must reject the 2nd request with the Hikari
        // quota_exhausted / hourlyAny shape.
        let harness =
            spawn_provider_facade_harness_with_options("provider-facade-hourly-gate", false, Some(1))
                .await;
        let client = Client::new();

        let resp = client
            .get(format!(
                "http://{}/api/serpapi/search?q=one",
                harness.addr
            ))
            .header("authorization", format!("Bearer {}", harness.token))
            .send()
            .await
            .expect("first request");
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = client
            .get(format!(
                "http://{}/api/serpapi/search?q=two",
                harness.addr
            ))
            .header("authorization", format!("Bearer {}", harness.token))
            .send()
            .await
            .expect("second request");
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: Value = resp.json().await.expect("json body");
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("quota_exhausted")
        );
        assert!(body.get("hourlyAny").is_some(), "{body}");

        // Only the first request reached the upstream.
        let captures = harness.rest_captures.lock().expect("captures lock");
        assert_eq!(captures.len(), 1);
    }

    #[tokio::test]
    async fn mcp_batch_without_ids_search_and_research_charge_full_reserved_fallback_when_usage_missing()
     {
        let db_path = temp_db_path("mcp-batch-no-id-search-research-missing-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-no-id-search-research-missing-usage-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_search_and_research_missing_usage(
            expected_api_key.to_string(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-no-id-search-research-missing-usage"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        // Search advanced expected=2; research pro reserved minimum=15. Without ids or
        // usage.credits, the proxy must still bill the full reserved total of 17.
        let resp = client
            .post(&url)
            .json(&serde_json::json!([
                {
                    "method": "tools/call",
                    "params": {
                        "name": "tavily-search",
                        "arguments": { "query": "fallback search", "search_depth": "advanced" }
                    }
                },
                {
                    "method": "tools/call",
                    "params": {
                        "name": "tavily-research",
                        "arguments": { "query": "fallback research", "model": "pro" }
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
        assert_eq!(verdict.hourly_used, 17);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_without_ids_tavily_crawl_uses_reported_usage_without_reserved_floor() {
        let db_path = temp_db_path("mcp-batch-no-id-crawl-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-no-id-crawl-usage-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_idless_tavily_tool_usage(
            expected_api_key.to_string(),
            "tavily-crawl".to_string(),
            Some(5),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-no-id-crawl-usage"))
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
                    "params": {
                        "name": "tavily-crawl",
                        "arguments": { "url": "https://example.com/page" }
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
    async fn mcp_batch_without_ids_tavily_extract_does_not_charge_when_usage_missing() {
        let db_path = temp_db_path("mcp-batch-no-id-extract-missing-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-no-id-extract-missing-usage-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_idless_tavily_tool_usage(
            expected_api_key.to_string(),
            "tavily-extract".to_string(),
            None,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-no-id-extract-missing-usage"))
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
                    "params": {
                        "name": "tavily-extract",
                        "arguments": { "urls": ["https://example.com"] }
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
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_search_and_extract_blocks_when_reserved_credits_would_exceed_quota() {
        let db_path = temp_db_path("mcp-batch-search-extract-precheck");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let expected_api_key = "tvly-mcp-batch-search-extract-precheck-key";
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
            .create_access_token(Some("mcp-batch-search-extract-precheck"))
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
                    "params": {
                        "name": "tavily-search",
                        "arguments": { "query": "precheck", "search_depth": "basic" }
                    }
                },
                {
                    "method": "tools/call",
                    "id": 2,
                    "params": {
                        "name": "tavily-extract",
                        "arguments": {
                            "urls": [
                                "https://example.com/1",
                                "https://example.com/2",
                                "https://example.com/3",
                                "https://example.com/4",
                                "https://example.com/5",
                                "https://example.com/6"
                            ]
                        }
                    }
                }
            ]))
            .send()
            .await
            .expect("batch request");
        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_rejects_duplicate_billable_ids_without_hitting_upstream() {
        let db_path = temp_db_path("mcp-batch-duplicate-ids");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-duplicate-ids-key";
        let (upstream_addr, hits) =
            spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
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
        let second_body: Value = second.json().await.expect("second response json");
        assert_eq!(
            second_body.get("window").and_then(|v| v.as_str()),
            Some("hour")
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_unknown_tavily_tool_uses_billable_quota_guardrails() {
        let db_path = temp_db_path("mcp-unknown-tavily-tool-quota");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-unknown-tavily-tool-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_unknown_tavily_tool(
            expected_api_key.to_string(),
            "tavily-research",
            5,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-unknown-tavily-tool-quota"))
            .await
            .expect("create access token");
        proxy
            .charge_token_quota(&access_token.id, 1)
            .await
            .expect("seed quota usage");

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
                    "name": "tavily-research",
                    "arguments": {
                        "query": "future billable tool"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        let body: Value = resp.json().await.expect("response json");
        assert_eq!(body.get("window").and_then(|v| v.as_str()), Some("hour"));
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_legacy_underscore_research_uses_billable_quota_guardrails() {
        let db_path = temp_db_path("mcp-legacy-underscore-research-quota");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-legacy-underscore-research-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_unknown_tavily_tool(
            expected_api_key.to_string(),
            "tavily_research",
            5,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-legacy-underscore-research-quota"))
            .await
            .expect("create access token");
        proxy
            .charge_token_quota(&access_token.id, 1)
            .await
            .expect("seed quota usage");

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
                    "name": "tavily_research",
                    "arguments": {
                        "query": "legacy billable tool"
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        let body: Value = resp.json().await.expect("response json");
        assert_eq!(body.get("window").and_then(|v| v.as_str()), Some("hour"));
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_legacy_underscore_research_charges_min_credits_without_usage() {
        let db_path = temp_db_path("mcp-legacy-underscore-research-credits");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-legacy-underscore-research-credits-key";
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.to_string();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(
                            params.get("tavilyApiKey").map(String::as_str),
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
                            Some("tavily_research"),
                            "expected underscore tavily_research tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            None,
                            "proxy should not inject include_usage for tavily_research"
                        );

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                    }
                                }
                            })),
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-legacy-underscore-research-credits"))
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
                    "name": "tavily_research",
                    "arguments": {
                        "query": "legacy billable tool"
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
        assert_eq!(verdict.hourly_used, 4);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_research_charges_min_credits_without_usage() {
        let db_path = temp_db_path("mcp-tools-call-tavily-research-credits");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-tavily-research-credits-key";
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.to_string();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(
                            params.get("tavilyApiKey").map(String::as_str),
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
                            Some("tavily-research"),
                            "expected tavily-research tool call"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|a| a.get("include_usage"))
                                .and_then(|v| v.as_bool()),
                            None,
                            "proxy should not inject include_usage for tavily-research"
                        );

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200,
                                    }
                                }
                            })),
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-tavily-research-credits"))
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
                "id": "probe-tool-call:tavily-research",
                "params": {
                    "name": "tavily-research",
                    "arguments": {
                        "input": "health check"
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
        assert_eq!(verdict.hourly_used, 4);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_dev_open_admin_fallback_requires_explicit_token() {
        let db_path = temp_db_path("mcp-dev-open-admin-fallback-requires-token");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-dev-open-admin-fallback-key";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let proxy_addr = spawn_proxy_server_with_dev(proxy, upstream, true).await;
        let client = Client::new();
        let url = format!("http://{}/mcp", proxy_addr);

        let resp = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-dev-open-admin",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");

        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
        let body: Value = resp.json().await.expect("parse rejection body");
        assert_eq!(
            body.get("error").and_then(|value| value.as_str()),
            Some("explicit_token_required")
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "dev-open-admin MCP fallback should be rejected before hitting upstream"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_dev_open_admin_explicit_token_charges_bound_account() {
        let db_path = temp_db_path("mcp-dev-open-admin-explicit-token");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-dev-open-admin-explicit-token-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_tavily_search(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "mcp-dev-open-admin-explicit-token-user".to_string(),
                username: Some("mcpdevopenadmin".to_string()),
                name: Some("MCP Dev Open Admin".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let access_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:mcp-dev-open-admin-explicit"))
            .await
            .expect("bind token");

        let proxy_addr = spawn_proxy_server_with_dev(proxy.clone(), upstream.clone(), true).await;
        let client = Client::new();
        let url = format!("http://{}/mcp", proxy_addr);
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", access_token.token))
            .json(&serde_json::json!({
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "mcp dev-open-admin explicit token",
                        "search_depth": "basic"
                    }
                }
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let pool = connect_sqlite_test_pool(&db_str).await;
        let row = sqlx::query(
            r#"
            SELECT token_id, billing_subject, billing_state, business_credits
            FROM auth_token_logs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("token log row exists");

        let token_id: String = row.try_get("token_id").expect("token_id");
        let billing_subject: Option<String> = row.try_get("billing_subject").expect("subject");
        let billing_state: String = row.try_get("billing_state").expect("state");
        let business_credits: Option<i64> = row.try_get("business_credits").expect("credits");
        let expected_subject = format!("account:{}", user.user_id);
        assert_eq!(token_id, access_token.id);
        assert_eq!(billing_subject.as_deref(), Some(expected_subject.as_str()));
        assert_eq!(billing_state, "charged");
        assert_eq!(business_credits, Some(1));

        let account_month: (i64, i64) = sqlx::query_as(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
        )
        .bind(&user.user_id)
        .fetch_one(&pool)
        .await
        .expect("account monthly quota exists");
        assert_eq!(account_month.1, 1);

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
    async fn mcp_tools_call_tavily_search_preserves_non_object_arguments() {
        let db_path = temp_db_path("mcp-tools-call-search-preserve-non-object-arguments");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-tools-call-search-preserve-args-key";
        let hits = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            "/mcp",
            any({
                let hits = hits.clone();
                move |Query(params): Query<HashMap<String, String>>, Json(body): Json<Value>| {
                    let expected_api_key = expected_api_key.to_string();
                    let hits = hits.clone();
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(
                            params.get("tavilyApiKey").map(String::as_str),
                            Some(expected_api_key.as_str()),
                            "missing or incorrect tavilyApiKey"
                        );
                        assert_eq!(
                            body.get("params")
                                .and_then(|p| p.get("arguments"))
                                .and_then(|v| v.as_str()),
                            Some("raw-search-args"),
                            "proxy should preserve non-object arguments for tavily-search"
                        );

                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": body.get("id").cloned().unwrap_or_else(|| serde_json::json!(1)),
                                "result": {
                                    "structuredContent": {
                                        "status": 200
                                    }
                                }
                            })),
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let upstream = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-search-preserve-args"))
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
                    "arguments": "raw-search-args"
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
        assert_eq!(verdict.hourly_used, 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_tavily_search_returns_upstream_response_when_billing_write_fails_after_upstream_success()
     {
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
            SELECT result_status, error_message, business_credits, billing_state
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
        let business_credits: Option<i64> = row.try_get("business_credits").unwrap();
        let billing_state: String = row.try_get("billing_state").unwrap();
        assert_eq!(status, "success");
        assert_eq!(business_credits, Some(1));
        assert_eq!(billing_state, "pending");
        assert!(
            message
                .unwrap_or_default()
                .contains("charge_token_quota failed"),
            "expected charge_token_quota failure to be logged"
        );

        sqlx::query(
            r#"
            CREATE TABLE token_usage_buckets (
                token_id TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                granularity TEXT NOT NULL,
                count INTEGER NOT NULL,
                PRIMARY KEY (token_id, bucket_start, granularity),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("recreate token_usage_buckets");

        let _guard = proxy
            .lock_token_billing(&access_token.id)
            .await
            .expect("reconcile pending billing");
        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 1);

        let row = sqlx::query(
            r#"
            SELECT billing_state
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row after reconcile");
        let billing_state: String = row.try_get("billing_state").unwrap();
        assert_eq!(billing_state, "charged");

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
    async fn mcp_tools_call_unknown_tavily_tool_is_forwarded_and_billed_when_usage_is_present() {
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
    async fn mcp_tools_call_tavily_crawl_blocks_when_reserved_credits_would_exceed_quota() {
        let db_path = temp_db_path("mcp-tools-call-crawl-reserved-precheck");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let expected_api_key = "tvly-mcp-tools-call-crawl-reserved-precheck-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_non_search_tools(
            expected_api_key.to_string(),
            0,
            3,
            0,
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-crawl-reserved-precheck"))
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
                    "name": "tavily-crawl",
                    "arguments": {
                        "urls": ["https://example.com/page"],
                        "limit": 10
                    }
                }
            }))
            .send()
            .await
            .expect("request");
        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

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
        assert_eq!(
            verdict.hourly_used, 0,
            "JSON-RPC error must not charge credits"
        );

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
    async fn mcp_session_headers_are_forwarded_after_initialize() {
        let db_path = temp_db_path("mcp-session-header-forwarding");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-session-header-key";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-session-header-forwarding"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "1.2.3.4")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-1",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");

        assert!(
            initialize.status().is_success(),
            "initialize should succeed, got {}",
            initialize.status()
        );
        let session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();
        assert_ne!(session_id, "session-123");
        assert!(
            !session_id.is_empty(),
            "initialize should return an opaque proxy session id"
        );

        let tools_list = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", session_id.as_str())
            .header("last-event-id", "resume-42")
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "1.2.3.4")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-1",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("tools/list request");

        assert!(
            tools_list.status().is_success(),
            "tools/list should succeed after initialize, got {}",
            tools_list.status()
        );
        let tools_list_body: Value = tools_list.json().await.expect("parse tools/list response");
        assert_eq!(
            tools_list_body
                .get("result")
                .and_then(|value| value.get("tools"))
                .and_then(|value| value.as_array())
                .map(|tools| tools.len()),
            Some(1)
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(recorded.len(), 2, "expected initialize + tools/list");
        assert_eq!(recorded[0].method, "initialize");
        assert_eq!(recorded[0].protocol_version.as_deref(), Some("2025-03-26"));
        assert!(
            !recorded[0].leaked_forwarded,
            "initialize should not leak forwarded headers"
        );
        assert_eq!(
            recorded[0].user_agent.as_deref(),
            Some("tavily-hikari-mcp-proxy/1.0")
        );
        assert_eq!(recorded[1].method, "tools/list");
        assert_eq!(recorded[1].session_id.as_deref(), Some("session-123"));
        assert_eq!(recorded[1].protocol_version.as_deref(), Some("2025-03-26"));
        assert_eq!(recorded[1].last_event_id.as_deref(), Some("resume-42"));
        assert!(
            !recorded[1].leaked_forwarded,
            "tools/list should not leak forwarded headers"
        );
        assert_eq!(
            recorded[1].user_agent.as_deref(),
            Some("tavily-hikari-mcp-proxy/1.0")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_control_logs_record_gateway_diagnostics() {
        let db_path = temp_db_path("mcp-control-log-diagnostics");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-control-log-diagnostics";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-control-log-diagnostics"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "diag-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(initialize.status().is_success());
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("proxy session id")
            .to_string();

        let tools_list = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "diag-tools",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("tools/list request");
        assert!(tools_list.status().is_success());

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(recorded.len(), 2, "expected initialize + tools/list upstream hits");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_rows = sqlx::query(
            r#"
            SELECT gateway_mode, experiment_variant, proxy_session_id, upstream_operation
            FROM request_logs
            WHERE path = '/mcp'
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("fetch request diagnostics rows");
        assert_eq!(request_rows.len(), 2, "expected initialize + follow-up request logs");
        for row in &request_rows {
            assert_eq!(
                row.try_get::<String, _>("gateway_mode").unwrap(),
                tavily_hikari::MCP_GATEWAY_MODE_UPSTREAM
            );
            assert_eq!(
                row.try_get::<String, _>("experiment_variant").unwrap(),
                tavily_hikari::MCP_EXPERIMENT_VARIANT_CONTROL
            );
            assert_eq!(
                row.try_get::<String, _>("proxy_session_id").unwrap(),
                proxy_session_id
            );
            assert_eq!(
                row.try_get::<String, _>("upstream_operation").unwrap(),
                "mcp"
            );
        }

        let token_rows = sqlx::query(
            r#"
            SELECT gateway_mode, experiment_variant, proxy_session_id, upstream_operation
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id ASC
            "#,
        )
        .bind(&access_token.id)
        .fetch_all(&pool)
        .await
        .expect("fetch token diagnostics rows");
        assert_eq!(token_rows.len(), 2, "expected initialize + follow-up token logs");
        for row in &token_rows {
            assert_eq!(
                row.try_get::<String, _>("gateway_mode").unwrap(),
                tavily_hikari::MCP_GATEWAY_MODE_UPSTREAM
            );
            assert_eq!(
                row.try_get::<String, _>("experiment_variant").unwrap(),
                tavily_hikari::MCP_EXPERIMENT_VARIANT_CONTROL
            );
            assert_eq!(
                row.try_get::<String, _>("proxy_session_id").unwrap(),
                proxy_session_id
            );
            assert_eq!(
                row.try_get::<String, _>("upstream_operation").unwrap(),
                "mcp"
            );
        }

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_session_cannot_be_reused_by_another_token() {
        let db_path = temp_db_path("mcp-session-cross-token");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-cross-token-key";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let owner_token = proxy
            .create_access_token(Some("mcp-cross-token-owner"))
            .await
            .expect("create owner token");
        let other_token = proxy
            .create_access_token(Some("mcp-cross-token-other"))
            .await
            .expect("create other token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();

        let owner_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, owner_token.token
        );
        let initialize = client
            .post(&owner_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-owner",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(initialize.status().is_success());
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("proxy session id")
            .to_string();

        let other_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, other_token.token
        );
        let rejected = client
            .post(&other_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-cross",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("cross token request");

        assert_eq!(rejected.status(), reqwest::StatusCode::FORBIDDEN);
        let body: Value = rejected.json().await.expect("parse rejection body");
        assert_eq!(
            body.get("error").and_then(|value| value.as_str()),
            Some("session_forbidden")
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            1,
            "cross-token reuse should be rejected locally"
        );
        assert_eq!(recorded[0].method, "initialize");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_pinned_key_unavailable_revokes_session_and_requires_reconnect() {
        let db_path = temp_db_path("mcp-pinned-key-unavailable");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-pinned-key-unavailable";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-pinned-key-unavailable"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-pinned",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(initialize.status().is_success());
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("proxy session id")
            .to_string();

        let pool = connect_sqlite_test_pool(&db_str).await;
        let key_id: String = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("bound key id");

        proxy
            .disable_key_by_id(&key_id)
            .await
            .expect("disable bound key");

        let follow_up = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-pinned",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("follow-up request");

        assert_eq!(follow_up.status(), reqwest::StatusCode::NOT_FOUND);
        let body: Value = follow_up.json().await.expect("parse reconnect body");
        assert_eq!(
            body.get("error").and_then(|value| value.as_str()),
            Some("session_unavailable")
        );

        let revoke_reason: Option<String> = sqlx::query_scalar(
            r#"SELECT revoke_reason
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("revoke reason row");
        assert_eq!(revoke_reason.as_deref(), Some("pinned_key_unavailable"));

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            1,
            "follow-up with disabled pinned key should be rejected locally"
        );
        assert_eq!(recorded[0].method, "initialize");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_pinned_exhausted_key_keeps_existing_session_alive() {
        let db_path = temp_db_path("mcp-pinned-key-exhausted");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-pinned-key-exhausted";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-pinned-key-exhausted"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-pinned-exhausted",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(initialize.status().is_success());
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("proxy session id")
            .to_string();

        proxy
            .mark_key_quota_exhausted_by_secret(expected_api_key)
            .await
            .expect("mark pinned key exhausted");

        let follow_up = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &proxy_session_id)
            .header("last-event-id", "resume-42")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-pinned-exhausted",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("follow-up request");

        assert_eq!(follow_up.status(), reqwest::StatusCode::OK);
        let body: Value = follow_up.json().await.expect("parse follow-up body");
        assert_eq!(
            body.get("result")
                .and_then(|value| value.get("tools"))
                .and_then(|value| value.as_array())
                .map(|tools| tools.len()),
            Some(1)
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let revoke_reason: Option<String> = sqlx::query_scalar(
            r#"SELECT revoke_reason
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("revoke reason row");
        assert_eq!(revoke_reason, None);

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            2,
            "exhausted pinned key should keep the session usable"
        );
        assert_eq!(recorded[0].method, "initialize");
        assert_eq!(recorded[1].method, "tools/list");
        assert_eq!(recorded[1].session_id.as_deref(), Some("session-123"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebind_invalidates_previous_sessions() {
        let db_path = temp_db_path("mcp-session-rebind-invalidates");
        let db_str = db_path.to_string_lossy().to_string();

        let first_api_key = "tvly-mcp-rebind-a".to_string();
        let second_api_key = "tvly-mcp-rebind-b".to_string();
        let (upstream_addr, calls) = spawn_mock_mcp_upstream_for_session_headers(vec![
            first_api_key.clone(),
            second_api_key.clone(),
        ])
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![first_api_key.clone(), second_api_key.clone()],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-session-rebind"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let first_initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-1",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("first initialize");
        assert!(first_initialize.status().is_success());
        let old_proxy_session_id = first_initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("old proxy session id")
            .to_string();

        let pool = connect_sqlite_test_pool(&db_str).await;
        let old_key_id: String = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&old_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("old key id");

        proxy
            .disable_key_by_id(&old_key_id)
            .await
            .expect("disable old primary key");

        let second_initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-2",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("second initialize");
        assert!(second_initialize.status().is_success());
        let new_proxy_session_id = second_initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("new proxy session id")
            .to_string();
        assert_ne!(old_proxy_session_id, new_proxy_session_id);

        let new_key_id: String = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&new_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("new key id");
        assert_ne!(
            old_key_id, new_key_id,
            "new initialize should select a different upstream key"
        );

        let stale_follow_up = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &old_proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-stale",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("stale follow-up");

        assert_eq!(stale_follow_up.status(), reqwest::StatusCode::NOT_FOUND);
        let stale_body: Value = stale_follow_up.json().await.expect("parse stale body");
        assert_eq!(
            stale_body.get("error").and_then(|value| value.as_str()),
            Some("session_unavailable")
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            2,
            "stale session should be rejected locally after rebind"
        );
        assert_eq!(recorded[0].method, "initialize");
        assert_eq!(recorded[1].method, "initialize");
        assert_ne!(recorded[0].tavily_api_key, recorded[1].tavily_api_key);

        let _ = std::fs::remove_file(db_path);
    }


    #[tokio::test]
    async fn tavily_http_research_result_keeps_prefixed_usage_base_path() {
        let db_path = temp_db_path("http-research-result-prefixed-path");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-research-result-prefixed-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("http-research-result-prefixed-path"))
            .await
            .expect("create token");
        let pool = connect_sqlite_test_pool(&db_str).await;
        let api_key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("api key id");

        let request_id = "req/segment";
        sqlx::query(
            r#"
            INSERT INTO research_requests (request_id, key_id, token_id, expires_at, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(request_id)
        .bind(&api_key_id)
        .bind(&access_token.id)
        .bind(Utc::now().timestamp() + 3600)
        .bind(Utc::now().timestamp())
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("seed research request owner");
        let route_path = "/token/Tavily/https/api.tavily.com/research/:request_id";
        let upstream_addr = spawn_http_research_result_mock_asserting_bearer_at_path(
            expected_api_key.to_string(),
            request_id.to_string(),
            route_path,
        )
        .await;
        let usage_base = format!("http://{upstream_addr}/token/Tavily/https/api.tavily.com/");
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

        // Avoid cross-test env var interference (research create uses predicted min cost enforcement).
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

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
        let create_body: Value = create_resp
            .json()
            .await
            .expect("parse research create response");
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
        let result_body: Value = result_resp
            .json()
            .await
            .expect("parse research result response");
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
    async fn tavily_http_research_result_prefers_request_id_affinity_over_project_affinity() {
        let db_path = temp_db_path("http-research-result-request-id-priority");
        let db_str = db_path.to_string_lossy().to_string();
        let project_id = "research-priority-project";

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-http-research-priority-a".to_string(),
                "tvly-http-research-priority-b".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let pool = connect_sqlite_test_pool(&db_str).await;

        let access_token = proxy
            .create_access_token(Some("http-research-request-id-priority"))
            .await
            .expect("create token");

        let upstream_addr = spawn_http_research_mock_requiring_same_key_for_result().await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let create_resp = client
            .post(format!("http://{}/api/tavily/research", proxy_addr))
            .header("X-Project-ID", project_id)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "request-id-priority",
                "model": "mini"
            }))
            .send()
            .await
            .expect("research create succeeds");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let create_body: Value = create_resp
            .json()
            .await
            .expect("parse research create response");
        let request_id = create_body
            .get("request_id")
            .and_then(|v| v.as_str())
            .expect("request_id should exist")
            .to_string();

        let request_key_id: String =
            sqlx::query_scalar("SELECT key_id FROM research_requests WHERE request_id = ? LIMIT 1")
                .bind(&request_id)
                .fetch_one(&pool)
                .await
                .expect("load research request affinity");
        let other_key_id: String =
            sqlx::query_scalar("SELECT id FROM api_keys WHERE id != ? ORDER BY id ASC LIMIT 1")
                .bind(&request_key_id)
                .fetch_one(&pool)
                .await
                .expect("load alternate key id");

        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO http_project_api_key_affinity (
                owner_subject,
                project_id_hash,
                api_key_id,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(owner_subject, project_id_hash) DO UPDATE SET
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(format!("token:{}", access_token.id))
        .bind(sha256_hex(project_id))
        .bind(&other_key_id)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("overwrite project affinity to conflicting key");

        let result_resp = client
            .get(format!(
                "http://{}/api/tavily/research/{}",
                proxy_addr, request_id
            ))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .header("X-Project-ID", project_id)
            .send()
            .await
            .expect("research result succeeds");
        assert_eq!(
            result_resp.status(),
            StatusCode::OK,
            "request_id affinity should beat the conflicting project binding",
        );

        let result_body: Value = result_resp
            .json()
            .await
            .expect("parse research result response");
        assert_eq!(
            result_body.get("request_id").and_then(|v| v.as_str()),
            Some(request_id.as_str())
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
        let create_body: Value = create_resp
            .json()
            .await
            .expect("parse research create response");
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
        let create_body: Value = create_resp
            .json()
            .await
            .expect("parse research create response");
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
        let body: Value = result_resp
            .json()
            .await
            .expect("parse research result response");
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

        assert_eq!(
            http_status,
            Some(StatusCode::INTERNAL_SERVER_ERROR.as_u16() as i64)
        );
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

        // Avoid cross-test env var interference (research create uses predicted min cost enforcement).
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

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
        let create_body: Value = create_resp
            .json()
            .await
            .expect("parse research create response");
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
        let result_body: Value = result_resp
            .json()
            .await
            .expect("parse research result response");
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
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "query-token-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
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
    async fn mcp_subpath_rejects_authenticated_requests_locally_and_persists_null_key_logs() {
        let db_path = temp_db_path("mcp-subpath-local-reject");
        let db_str = db_path.to_string_lossy().to_string();

        let hits = Arc::new(AtomicUsize::new(0));
        let upstream_addr = spawn_counted_fake_forward_proxy(
            StatusCode::OK,
            Duration::from_millis(0),
            hits.clone(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-mcp-subpath-local-reject".to_string()],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-subpath-local-reject"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();
        let resp = client
            .post(format!(
                "http://{}/mcp/search?tavilyApiKey={}",
                proxy_addr, access_token.token
            ))
            .json(&serde_json::json!({
                "query": "why was this client pointed at /mcp/search"
            }))
            .send()
            .await
            .expect("subpath request");

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.text().await.expect("plain text body"), "Not Found");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "subpath reject must not hit upstream"
        );

        let latest_token_log = proxy
            .token_recent_logs(&access_token.id, 1, None)
            .await
            .expect("token recent logs")
            .into_iter()
            .next()
            .expect("token log exists");
        assert_eq!(latest_token_log.key_id, None);
        assert_eq!(latest_token_log.request_kind_key, "mcp:unsupported-path");
        assert_eq!(
            latest_token_log.request_kind_label,
            "MCP | unsupported path"
        );
        assert_eq!(
            latest_token_log.request_kind_detail.as_deref(),
            Some("/mcp/search")
        );
        assert_eq!(latest_token_log.result_status, "error");
        assert_eq!(
            latest_token_log.failure_kind.as_deref(),
            Some("mcp_path_404")
        );
        assert!(!latest_token_log.counts_business_quota);
        assert_eq!(latest_token_log.business_credits, None);

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_row = sqlx::query(
            r#"
            SELECT
                id,
                api_key_id,
                auth_token_id,
                status_code,
                tavily_status_code,
                result_status,
                request_kind_key,
                request_kind_label,
                failure_kind,
                key_effect_code,
                business_credits,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers
            FROM request_logs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("request log row");
        let request_log_id: i64 = request_row.try_get("id").expect("request log id");
        let request_api_key_id: Option<String> = request_row
            .try_get("api_key_id")
            .expect("request api key id");
        let request_auth_token_id: Option<String> = request_row
            .try_get("auth_token_id")
            .expect("request auth token id");
        let request_body: Vec<u8> = request_row.try_get("request_body").expect("request body");
        let response_body: Vec<u8> = request_row.try_get("response_body").expect("response body");
        assert_eq!(request_api_key_id, None);
        assert_eq!(
            request_auth_token_id.as_deref(),
            Some(access_token.id.as_str())
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("status_code")
                .unwrap(),
            Some(404)
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("tavily_status_code")
                .unwrap(),
            Some(404)
        );
        assert_eq!(
            request_row.try_get::<String, _>("result_status").unwrap(),
            "error"
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("request_kind_key")
                .unwrap(),
            "mcp:unsupported-path"
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("request_kind_label")
                .unwrap(),
            "MCP | unsupported path"
        );
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("failure_kind")
                .unwrap()
                .as_deref(),
            Some("mcp_path_404")
        );
        assert_eq!(
            request_row.try_get::<String, _>("key_effect_code").unwrap(),
            "none"
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("business_credits")
                .unwrap(),
            None
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&request_body)
                .expect("request body json")
                .get("query")
                .and_then(|value| value.as_str()),
            Some("why was this client pointed at /mcp/search")
        );
        assert_eq!(response_body, b"Not Found");
        assert_eq!(
            request_row
                .try_get::<String, _>("forwarded_headers")
                .unwrap(),
            "[]"
        );
        assert_eq!(
            request_row.try_get::<String, _>("dropped_headers").unwrap(),
            "[]"
        );

        let token_row = sqlx::query(
            r#"
            SELECT api_key_id, counts_business_quota, business_credits, request_log_id
            FROM auth_token_logs
            WHERE token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row");
        assert_eq!(
            token_row
                .try_get::<Option<String>, _>("api_key_id")
                .expect("token log api key id"),
            None
        );
        assert_eq!(
            token_row
                .try_get::<i64, _>("counts_business_quota")
                .expect("counts business quota"),
            0
        );
        assert_eq!(
            token_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("token log business credits"),
            None
        );
        assert_eq!(
            token_row
                .try_get::<Option<i64>, _>("request_log_id")
                .expect("linked request log id"),
            Some(request_log_id)
        );

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek token quota");
        assert_eq!(
            verdict.hourly_used, 0,
            "subpath rejects must not charge business quota"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_subpath_keeps_missing_and_invalid_token_401_responses() {
        let db_path = temp_db_path("mcp-subpath-auth-401");
        let db_str = db_path.to_string_lossy().to_string();

        let hits = Arc::new(AtomicUsize::new(0));
        let upstream_addr = spawn_counted_fake_forward_proxy(
            StatusCode::OK,
            Duration::from_millis(0),
            hits.clone(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-mcp-subpath-auth-401".to_string()],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        let proxy_addr = spawn_proxy_server(proxy, "https://api.tavily.com".to_string()).await;
        let client = Client::new();

        let missing_resp = client
            .post(format!("http://{}/mcp/search", proxy_addr))
            .json(&serde_json::json!({ "query": "missing token" }))
            .send()
            .await
            .expect("missing token request");
        assert_eq!(missing_resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            missing_resp
                .json::<serde_json::Value>()
                .await
                .expect("missing token json")
                .get("error")
                .and_then(|value| value.as_str()),
            Some("missing token")
        );

        let invalid_resp = client
            .post(format!("http://{}/mcp/search", proxy_addr))
            .header("Authorization", "Bearer th-invalid-token")
            .json(&serde_json::json!({ "query": "invalid token" }))
            .send()
            .await
            .expect("invalid token request");
        assert_eq!(invalid_resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            invalid_resp
                .json::<serde_json::Value>()
                .await
                .expect("invalid token json")
                .get("error")
                .and_then(|value| value.as_str()),
            Some("invalid or disabled token")
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "401 rejects must not hit upstream"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_logs")
            .fetch_one(&pool)
            .await
            .expect("request log count");
        let token_log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs")
            .fetch_one(&pool)
            .await
            .expect("token log count");
        assert_eq!(request_log_count, 0);
        assert_eq!(token_log_count, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_subpath_dev_open_admin_fallback_requires_explicit_token() {
        let db_path = temp_db_path("mcp-subpath-dev-open-admin-explicit-token");
        let db_str = db_path.to_string_lossy().to_string();

        let hits = Arc::new(AtomicUsize::new(0));
        let upstream_addr = spawn_counted_fake_forward_proxy(
            StatusCode::OK,
            Duration::from_millis(0),
            hits.clone(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-mcp-subpath-dev-open-admin".to_string()],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        let proxy_addr =
            spawn_proxy_server_with_dev(proxy, "https://api.tavily.com".to_string(), true).await;
        let client = Client::new();

        let resp = client
            .post(format!("http://{}/mcp/search", proxy_addr))
            .json(&serde_json::json!({ "query": "missing explicit token" }))
            .send()
            .await
            .expect("subpath request");

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.json::<serde_json::Value>()
                .await
                .expect("explicit token json")
                .get("error")
                .and_then(|value| value.as_str()),
            Some("explicit_token_required")
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "401 rejects must not hit upstream"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM request_logs")
            .fetch_one(&pool)
            .await
            .expect("request log count");
        let token_log_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs")
            .fetch_one(&pool)
            .await
            .expect("token log count");
        assert_eq!(request_log_count, 0);
        assert_eq!(token_log_count, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_root_sse_get_stays_405_while_subpath_sse_get_is_local_404() {
        let db_path = temp_db_path("mcp-root-sse-and-subpath");
        let db_str = db_path.to_string_lossy().to_string();

        let hits = Arc::new(AtomicUsize::new(0));
        let upstream_addr = spawn_counted_fake_forward_proxy(
            StatusCode::OK,
            Duration::from_millis(0),
            hits.clone(),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-mcp-root-sse-subpath".to_string()],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-root-sse-subpath"))
            .await
            .expect("create access token");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;
        let client = Client::new();

        let root_resp = client
            .get(format!(
                "http://{}/mcp?tavilyApiKey={}",
                proxy_addr, access_token.token
            ))
            .header("Accept", "text/event-stream")
            .send()
            .await
            .expect("root sse request");
        assert_eq!(root_resp.status(), StatusCode::METHOD_NOT_ALLOWED);

        let subpath_resp = client
            .get(format!(
                "http://{}/mcp/sse?tavilyApiKey={}",
                proxy_addr, access_token.token
            ))
            .header("Accept", "text/event-stream")
            .send()
            .await
            .expect("subpath sse request");
        assert_eq!(subpath_resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            subpath_resp.text().await.expect("subpath body"),
            "Not Found"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "SSE guard paths must stay local"
        );

        let latest_token_log = proxy
            .token_recent_logs(&access_token.id, 1, None)
            .await
            .expect("token recent logs")
            .into_iter()
            .next()
            .expect("token log exists");
        assert_eq!(latest_token_log.request_kind_key, "mcp:unsupported-path");
        assert_eq!(
            latest_token_log.request_kind_detail.as_deref(),
            Some("/mcp/sse")
        );
        assert_eq!(
            latest_token_log.failure_kind.as_deref(),
            Some("mcp_path_404")
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
    async fn mcp_session_delete_405_stays_non_billable_even_when_business_quota_is_exhausted() {
        let db_path = temp_db_path("mcp-session-delete-neutral");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-mcp-session-delete-neutral-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_search_and_delete_405(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-session-delete-neutral"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let search = client
            .post(&url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "search-before-delete",
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "exhaust quota before delete",
                        "search_depth": "basic"
                    }
                }
            }))
            .send()
            .await
            .expect("search request");
        assert_eq!(search.status(), StatusCode::OK);

        let delete = client.delete(&url).send().await.expect("delete request");
        assert_eq!(delete.status(), StatusCode::METHOD_NOT_ALLOWED);
        let delete_body: Value = delete.json().await.expect("delete body");
        assert_eq!(
            delete_body.get("message").and_then(|value| value.as_str()),
            Some("Method Not Allowed: Session termination not supported")
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "delete must still hit upstream"
        );

        let latest_token_log = proxy
            .token_recent_logs(&access_token.id, 1, None)
            .await
            .expect("token recent logs")
            .into_iter()
            .next()
            .expect("latest token log");
        assert_eq!(
            latest_token_log.request_kind_key,
            "mcp:session-delete-unsupported"
        );
        assert_eq!(
            latest_token_log.request_kind_label,
            "MCP | session delete unsupported"
        );
        assert_eq!(
            latest_token_log.failure_kind.as_deref(),
            Some("mcp_method_405")
        );
        assert_eq!(latest_token_log.http_status, Some(405));
        assert_eq!(latest_token_log.mcp_status, Some(405));
        assert_eq!(latest_token_log.result_status, "error");
        assert!(!latest_token_log.counts_business_quota);
        assert_eq!(latest_token_log.business_credits, None);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek token quota");
        assert_eq!(
            verdict.hourly_used, 1,
            "session delete 405 must not consume an extra business quota unit"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_row = sqlx::query(
            r#"
            SELECT
                status_code,
                tavily_status_code,
                request_kind_key,
                request_kind_label,
                business_credits,
                failure_kind
            FROM request_logs
            WHERE auth_token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("request log row");
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("status_code")
                .expect("status code"),
            Some(405)
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("tavily_status_code")
                .expect("tavily status code"),
            Some(405)
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("request_kind_key")
                .expect("request kind key"),
            "mcp:session-delete-unsupported"
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("request_kind_label")
                .expect("request kind label"),
            "MCP | session delete unsupported"
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("business credits"),
            None
        );
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("failure_kind")
                .expect("failure kind")
                .as_deref(),
            Some("mcp_method_405")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_session_delete_non_405_keeps_billable_error_semantics() {
        let db_path = temp_db_path("mcp-session-delete-500");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-session-delete-500-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_search_and_delete_500(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-session-delete-500"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let delete = client.delete(&url).send().await.expect("delete request");
        assert_eq!(delete.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let delete_body: Value = delete.json().await.expect("delete body");
        assert_eq!(
            delete_body.get("message").and_then(|value| value.as_str()),
            Some("delete failed upstream")
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let latest_token_log = proxy
            .token_recent_logs(&access_token.id, 1, None)
            .await
            .expect("token recent logs")
            .into_iter()
            .next()
            .expect("latest token log");
        assert_eq!(latest_token_log.http_status, Some(500));
        assert_eq!(latest_token_log.request_kind_key, "mcp:unknown-payload");
        assert!(latest_token_log.counts_business_quota);

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_row = sqlx::query(
            r#"
            SELECT request_kind_key, business_credits
            FROM request_logs
            WHERE auth_token_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(&access_token.id)
        .fetch_one(&pool)
        .await
        .expect("request log row");
        assert_eq!(
            request_row
                .try_get::<String, _>("request_kind_key")
                .expect("request kind key"),
            "mcp:unknown-payload"
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("business credits"),
            None
        );

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
        let (upstream_addr, hits) =
            spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
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
        let (upstream_addr, hits) =
            spawn_mock_upstream_with_hits(expected_api_key.to_string()).await;
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
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "upstream must not be hit when blocked"
        );

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
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_search_batch_with_error(
            expected_api_key.to_string(),
        )
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
    async fn mcp_batch_tools_call_tavily_search_charges_usage_credits_even_when_sibling_quota_exhausted()
     {
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
    async fn mcp_batch_tools_call_tavily_search_falls_back_per_id_when_billable_sibling_errors() {
        let db_path = temp_db_path("mcp-batch-search-missing-usage-with-billable-error");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-missing-usage-with-billable-error-key";
        let (upstream_addr, hits) =
            spawn_mock_mcp_upstream_for_search_missing_usage_with_extract_error(
                expected_api_key.to_string(),
            )
            .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-batch-search-missing-usage-with-billable-error"))
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
        assert_eq!(
            verdict.hourly_used, 2,
            "successful search should still charge expected credits when a billable sibling errors"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_batch_tools_call_tavily_search_does_not_overcharge_when_error_is_in_detail_status()
    {
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
    async fn mcp_batch_tools_call_tavily_search_charges_expected_total_when_usage_missing_for_some_items()
     {
        let db_path = temp_db_path("mcp-batch-search-partial-usage");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-mcp-batch-search-partial-usage-key";
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_tavily_search_batch_partial_usage(
            expected_api_key.to_string(),
        )
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
        let (upstream_addr, hits) = spawn_mock_mcp_upstream_for_mixed_tools_list_and_search_usage(
            expected_api_key.to_string(),
        )
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


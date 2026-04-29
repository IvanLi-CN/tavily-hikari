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
    async fn admin_registration_settings_require_admin_and_persist() {
        let db_path = temp_db_path("admin-registration-settings");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let unauth_addr = spawn_admin_users_server(proxy.clone(), false).await;
        let client = Client::new();
        let get_resp = client
            .get(format!("http://{}/api/admin/registration", unauth_addr))
            .send()
            .await
            .expect("get admin registration unauth request");
        assert_eq!(get_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let patch_resp = client
            .patch(format!("http://{}/api/admin/registration", unauth_addr))
            .json(&serde_json::json!({ "allowRegistration": false }))
            .send()
            .await
            .expect("patch admin registration unauth request");
        assert_eq!(patch_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let invalid_unauth_resp = client
            .patch(format!("http://{}/api/admin/registration", unauth_addr))
            .body("not-json")
            .send()
            .await
            .expect("patch admin registration invalid unauth request");
        assert_eq!(invalid_unauth_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let admin_addr = spawn_admin_users_server(proxy, true).await;
        let initial_resp = client
            .get(format!("http://{}/api/admin/registration", admin_addr))
            .send()
            .await
            .expect("get admin registration request");
        assert_eq!(initial_resp.status(), reqwest::StatusCode::OK);
        let initial_body: serde_json::Value = initial_resp.json().await.expect("initial json");
        assert_eq!(
            initial_body.get("allowRegistration"),
            Some(&serde_json::Value::Bool(true))
        );

        let updated_resp = client
            .patch(format!("http://{}/api/admin/registration", admin_addr))
            .json(&serde_json::json!({ "allowRegistration": false }))
            .send()
            .await
            .expect("patch admin registration request");
        assert_eq!(updated_resp.status(), reqwest::StatusCode::OK);
        let updated_body: serde_json::Value = updated_resp.json().await.expect("updated json");
        assert_eq!(
            updated_body.get("allowRegistration"),
            Some(&serde_json::Value::Bool(false))
        );

        let persisted_resp = client
            .get(format!("http://{}/api/admin/registration", admin_addr))
            .send()
            .await
            .expect("get persisted registration request");
        assert_eq!(persisted_resp.status(), reqwest::StatusCode::OK);
        let persisted_body: serde_json::Value =
            persisted_resp.json().await.expect("persisted json");
        assert_eq!(
            persisted_body.get("allowRegistration"),
            Some(&serde_json::Value::Bool(false))
        );

        let invalid_admin_resp = client
            .patch(format!("http://{}/api/admin/registration", admin_addr))
            .body("not-json")
            .send()
            .await
            .expect("patch admin registration invalid admin request");
        assert_eq!(
            invalid_admin_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

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
        let verdict1 = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota 1");
        assert_eq!(verdict1.hourly_used, 1);

        let resp2 = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "test-2", "search_depth": "basic" }))
            .send()
            .await
            .expect("request 2");
        assert_eq!(resp2.status(), reqwest::StatusCode::OK);
        let verdict2 = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota 2");
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
        let verdict3 = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota 3");
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
        let verdict1 = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota 1");
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
        let verdict1 = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota 1");
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
        let verdict2 = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota 2");
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
    async fn tavily_http_search_returns_upstream_response_when_billing_write_fails_after_upstream_success()
     {
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
            SELECT result_status, error_message, business_credits, billing_state
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
            .lock_token_billing(&token.id)
            .await
            .expect("reconcile pending billing");
        let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
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
        .bind(&token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row after reconcile");
        let billing_state: String = row.try_get("billing_state").unwrap();
        assert_eq!(billing_state, "charged");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_defers_billing_when_quota_subject_lock_is_lost_before_settle() {
        let db_path = temp_db_path("http-search-quota-subject-lock-lost");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-search-quota-subject-lock-lost-key";
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
            .create_access_token(Some("http-search-quota-subject-lock-lost"))
            .await
            .expect("create token");
        let billing_subject = format!("token:{}", token.id);

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
                    .json(&serde_json::json!({ "query": "lock-loss", "search_depth": "basic" }))
                    .send()
                    .await
                    .expect("request")
            }
        });

        arrived.notified().await;
        proxy.force_quota_subject_lock_loss_once_for_subject(&billing_subject);
        release.notify_one();

        let resp = handle.await.expect("task join");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

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
            SELECT result_status, error_message, business_credits, billing_state
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
        let business_credits: Option<i64> = row.try_get("business_credits").unwrap();
        let billing_state: String = row.try_get("billing_state").unwrap();
        assert_eq!(status, "success");
        assert_eq!(business_credits, Some(1));
        assert_eq!(billing_state, "pending");
        let message = message.unwrap_or_default();
        assert!(
            message.contains("charge_token_quota deferred")
                && message.contains("pending billing will retry"),
            "expected lock-loss defer message, got: {message}"
        );

        let _guard = proxy
            .lock_token_billing(&token.id)
            .await
            .expect("reconcile pending billing");
        let verdict = proxy
            .peek_token_quota(&token.id)
            .await
            .expect("peek quota after reconcile");
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
        .bind(&token.id)
        .fetch_one(&pool)
        .await
        .expect("token log row after reconcile");
        let billing_state: String = row.try_get("billing_state").unwrap();
        assert_eq!(billing_state, "charged");

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

        let expected_api_key = "tvly-http-search-hourly-any-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let configured_limit = 4;
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: configured_limit,
                mcp_session_affinity_key_count:
                    tavily_hikari::MCP_SESSION_AFFINITY_KEY_COUNT_DEFAULT,
                rebalance_mcp_enabled: tavily_hikari::REBALANCE_MCP_ENABLED_DEFAULT,
                rebalance_mcp_session_percent:
                    tavily_hikari::REBALANCE_MCP_SESSION_PERCENT_DEFAULT,
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("set request-rate limit");

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

        for _ in 1..configured_limit {
            let verdict = proxy
                .check_token_hourly_requests(&access_token.id)
                .await
                .expect("prefill request-rate window");
            assert!(verdict.allowed, "prefill raw limiter should stay allowed");
        }

        // Next request should be blocked by request-rate limiter before upstream.
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
            "expected request-rate 429 once the fixed 5m window is exhausted"
        );
        let retry_after = second
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .expect("429 response should include Retry-After");
        assert!(retry_after > 0);
        let second_body: Value = second.json().await.expect("429 json body");
        assert_eq!(
            second_body
                .get("requestRate")
                .and_then(|value| value.get("limit"))
                .and_then(|value| value.as_i64()),
            Some(configured_limit)
        );
        assert_eq!(
            second_body
                .get("requestRate")
                .and_then(|value| value.get("windowMinutes"))
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit_window_minutes())
        );
        assert_eq!(
            second_body
                .get("requestRate")
                .and_then(|value| value.get("scope"))
                .and_then(|value| value.as_str()),
            Some("token")
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

        let expected_api_key = "tvly-http-map-hourly-any-key";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let configured_limit = 3;
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: configured_limit,
                mcp_session_affinity_key_count:
                    tavily_hikari::MCP_SESSION_AFFINITY_KEY_COUNT_DEFAULT,
                rebalance_mcp_enabled: tavily_hikari::REBALANCE_MCP_ENABLED_DEFAULT,
                rebalance_mcp_session_percent:
                    tavily_hikari::REBALANCE_MCP_SESSION_PERCENT_DEFAULT,
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("set request-rate limit");

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

        for _ in 0..configured_limit {
            let verdict = proxy
                .check_token_hourly_requests(&access_token.id)
                .await
                .expect("prefill request-rate window");
            assert!(verdict.allowed, "prefill raw limiter should stay allowed");
        }

        let blocked = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com"
            }))
            .send()
            .await
            .expect("blocked request");
        assert_eq!(
            blocked.status(),
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            "map request should be blocked by the fixed request-rate limiter"
        );
        let retry_after = blocked
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
            .expect("429 response should include Retry-After");
        assert!(retry_after > 0);
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_search_hot_updates_request_rate_limit_without_clearing_existing_window() {
        let db_path = temp_db_path("http-search-hot-request-rate-limit");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-http-search-hot-limit";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("hourly-any-hot-update"))
            .await
            .expect("create token");

        for _ in 0..2 {
            let verdict = proxy
                .check_token_hourly_requests(&access_token.id)
                .await
                .expect("prefill request-rate window");
            assert!(verdict.allowed, "prefill raw limiter should stay allowed");
        }

        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: 2,
                mcp_session_affinity_key_count:
                    tavily_hikari::MCP_SESSION_AFFINITY_KEY_COUNT_DEFAULT,
                rebalance_mcp_enabled: tavily_hikari::REBALANCE_MCP_ENABLED_DEFAULT,
                rebalance_mcp_session_percent:
                    tavily_hikari::REBALANCE_MCP_SESSION_PERCENT_DEFAULT,
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("lower request-rate limit");

        let (upstream_addr, hits) =
            spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let blocked = Client::new()
            .post(format!("http://{}/api/tavily/search", proxy_addr))
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "query": "hot update blocked"
            }))
            .send()
            .await
            .expect("blocked request");

        assert_eq!(blocked.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "request should stop before upstream");
        let body: Value = blocked.json().await.expect("429 json body");
        assert_eq!(
            body.get("requestRate")
                .and_then(|value| value.get("limit"))
                .and_then(|value| value.as_i64()),
            Some(2)
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
    async fn tavily_http_search_forwards_raw_x_project_id_and_logs_project_affinity_effect() {
        let db_path = temp_db_path("http-search-project-header-forward");
        let db_str = db_path.to_string_lossy().to_string();

        let seen = Arc::new(Mutex::new(Vec::<(String, Option<String>)>::new()));
        let upstream_addr = spawn_http_search_mock_recording_upstream_identity(seen.clone()).await;
        let usage_base = format!("http://{}", upstream_addr);

        let expected_api_key = "tvly-http-search-project-header";
        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let pool = connect_sqlite_test_pool(&db_str).await;

        let access_token = proxy
            .create_access_token(Some("http-search-project-header"))
            .await
            .expect("create token");
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;

        let client = Client::new();
        let project_id = "project-header-forwarded";
        let resp = client
            .post(format!("http://{}/api/tavily/search", proxy_addr))
            .header("Authorization", format!("Bearer {}", access_token.token))
            .header("X-Project-ID", project_id)
            .json(&serde_json::json!({
                "query": "project header forward"
            }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        assert_eq!(body.get("status").and_then(|v| v.as_i64()), Some(200));

        let seen = {
            let seen = seen.lock().expect("seen lock should not be poisoned");
            seen.clone()
        };
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, expected_api_key);
        assert_eq!(seen[0].1.as_deref(), Some(project_id));

        let effect_row = sqlx::query(
            "SELECT key_effect_code, binding_effect_code, selection_effect_code FROM request_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("load project-affinity effect row");
        assert_eq!(
            effect_row.try_get::<String, _>("key_effect_code").unwrap(),
            "none"
        );
        assert_eq!(
            effect_row
                .try_get::<String, _>("binding_effect_code")
                .unwrap(),
            "http_project_affinity_bound"
        );
        assert_eq!(
            effect_row
                .try_get::<String, _>("selection_effect_code")
                .unwrap(),
            "none"
        );

        let token_effect_row = sqlx::query(
            "SELECT key_effect_code, binding_effect_code, selection_effect_code FROM auth_token_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("load project-affinity token effect row");
        assert_eq!(
            token_effect_row
                .try_get::<String, _>("key_effect_code")
                .unwrap(),
            "none"
        );
        assert_eq!(
            token_effect_row
                .try_get::<String, _>("binding_effect_code")
                .unwrap(),
            "http_project_affinity_bound"
        );
        assert_eq!(
            token_effect_row
                .try_get::<String, _>("selection_effect_code")
                .unwrap(),
            "none"
        );

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

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

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

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

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

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

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
        let (upstream_addr, hits) =
            spawn_http_json_endpoints_mock_with_usage(expected_api_key.to_string(), 0, 5, 3).await;
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
    async fn tavily_http_extract_blocks_when_reserved_credits_would_exceed_quota() {
        let db_path = temp_db_path("http-extract-reserved-precheck");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-http-extract-reserved-precheck-key";
        let (upstream_addr, hits) =
            spawn_http_json_endpoints_mock_with_usage(expected_api_key.to_string(), 2, 0, 0).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-extract-reserved-precheck"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/extract", proxy_addr);

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": [
                    "https://example.com/1",
                    "https://example.com/2",
                    "https://example.com/3",
                    "https://example.com/4",
                    "https://example.com/5",
                    "https://example.com/6"
                ],
                "extract_depth": "basic"
            }))
            .send()
            .await
            .expect("extract request");
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
    async fn tavily_http_crawl_blocks_when_reserved_credits_would_exceed_quota() {
        let db_path = temp_db_path("http-crawl-reserved-precheck");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

        let expected_api_key = "tvly-http-crawl-reserved-precheck-key";
        let (upstream_addr, hits) =
            spawn_http_json_endpoints_mock_with_usage(expected_api_key.to_string(), 0, 3, 0).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-crawl-reserved-precheck"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/crawl", proxy_addr);

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "urls": ["https://example.com/page"],
                "limit": 10
            }))
            .send()
            .await
            .expect("crawl request");
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
    async fn tavily_http_map_blocks_when_reserved_credits_would_exceed_quota() {
        let db_path = temp_db_path("http-map-reserved-precheck");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1");

        let expected_api_key = "tvly-http-map-reserved-precheck-key";
        let (upstream_addr, hits) =
            spawn_http_json_endpoints_mock_with_usage(expected_api_key.to_string(), 0, 0, 2).await;
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-map-reserved-precheck"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();
        let url = format!("http://{}/api/tavily/map", proxy_addr);

        let resp = client
            .post(&url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "url": "https://example.com",
                "limit": 11
            }))
            .send()
            .await
            .expect("map request");
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
    async fn tavily_http_research_replaces_body_api_key_with_tavily_key() {
        let db_path = temp_db_path("http-research-replace-key");
        let db_str = db_path.to_string_lossy().to_string();

        // Avoid cross-test env var interference (research uses model estimate enforcement).
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
    async fn tavily_http_research_charges_mini_estimate_without_usage_diff() {
        let db_path = temp_db_path("http-research-mini-estimate-charge");
        let db_str = db_path.to_string_lossy().to_string();

        // Avoid cross-test env var interference.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-mini-estimate-key";
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
            .create_access_token(Some("http-research-mini-estimate-charge"))
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
        assert_eq!(verdict.hourly_used, 40);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_pro_estimate_without_usage_diff() {
        let db_path = temp_db_path("http-research-pro-estimate-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-pro-estimate-key";
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
            .create_access_token(Some("http-research-pro-estimate-charge"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "pro-estimate",
                "model": "pro"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 100);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_auto_estimate_without_usage_probe() {
        let db_path = temp_db_path("http-research-auto-estimate-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-auto-estimate-key";
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
            .create_access_token(Some("http-research-auto-estimate-charge"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "auto-estimate",
                "model": "auto"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 50);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_charges_default_auto_estimate_when_model_missing() {
        let db_path = temp_db_path("http-research-default-auto-estimate");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-default-auto-key";
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
            .create_access_token(Some("http-research-default-auto-estimate"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "default-auto-estimate"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 50);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let latest_log = proxy
            .token_recent_logs(&access_token.id, 1, None)
            .await
            .expect("read token log")
            .into_iter()
            .next()
            .expect("token log exists");
        assert_eq!(latest_log.result_status, "success");
        assert_eq!(latest_log.error_message, None);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_does_not_charge_failed_upstream_response() {
        let db_path = temp_db_path("http-research-failed-upstream-no-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-failed-upstream-key";
        let usage_calls = Arc::new(AtomicUsize::new(0));
        let research_calls = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/usage",
                get({
                    let usage_calls = usage_calls.clone();
                    move || {
                        let usage_calls = usage_calls.clone();
                        async move {
                            usage_calls.fetch_add(1, Ordering::SeqCst);
                            (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(serde_json::json!({
                                    "error": "unexpected usage probe"
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
                    move || {
                        let research_calls = research_calls.clone();
                        async move {
                            research_calls.fetch_add(1, Ordering::SeqCst);
                            (
                                StatusCode::BAD_GATEWAY,
                                Json(serde_json::json!({
                                    "error": "mock upstream failure"
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
        let usage_base = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![expected_api_key.to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("http-research-failed-upstream-no-charge"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "failed-upstream",
                "model": "pro"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::BAD_GATEWAY);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_rejects_invalid_model_without_quota_or_upstream() {
        let db_path = temp_db_path("http-research-invalid-model-no-charge");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "39");

        let expected_api_key = "tvly-http-research-invalid-model-key";
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
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: 1,
                mcp_session_affinity_key_count:
                    tavily_hikari::MCP_SESSION_AFFINITY_KEY_COUNT_DEFAULT,
                rebalance_mcp_enabled: tavily_hikari::REBALANCE_MCP_ENABLED_DEFAULT,
                rebalance_mcp_session_percent:
                    tavily_hikari::REBALANCE_MCP_SESSION_PERCENT_DEFAULT,
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("set request-rate limit");
        let access_token = proxy
            .create_access_token(Some("http-research-invalid-model-no-charge"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "invalid-model",
                "model": "invalid-model"
            }))
            .send()
            .await
            .expect("research request");
        assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
        let body: serde_json::Value = resp.json().await.expect("invalid model json");
        assert_eq!(
            body.get("error").and_then(|v| v.as_str()),
            Some("invalid_request")
        );
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 0);

        let verdict = proxy
            .peek_token_quota(&access_token.id)
            .await
            .expect("peek quota");
        assert_eq!(verdict.hourly_used, 0);

        let blocked = client
            .post(format!("http://{}/api/tavily/research", proxy_addr))
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "valid-after-invalid",
                "model": "mini"
            }))
            .send()
            .await
            .expect("blocked follow-up request");
        assert_eq!(blocked.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(research_calls.load(Ordering::SeqCst), 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_does_not_probe_follow_up_usage() {
        let db_path = temp_db_path("http-research-follow-up-usage-probe-fails");
        let db_str = db_path.to_string_lossy().to_string();

        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

        let expected_api_key = "tvly-http-research-follow-up-usage-probe-fails-key";
        let (upstream_addr, usage_calls, research_calls) =
            spawn_http_research_mock_with_follow_up_usage_probe_failure(
                expected_api_key.to_string(),
                10,
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
            .create_access_token(Some("http-research-follow-up-usage-probe-fails"))
            .await
            .expect("create token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let client = Client::new();

        let url = format!("http://{}/api/tavily/research", proxy_addr);
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "api_key": access_token.token,
                "input": "follow-up-usage-probe-fails",
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
        assert_eq!(verdict.hourly_used, 40);
        assert_eq!(usage_calls.load(Ordering::SeqCst), 0);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);

        let latest_log = proxy
            .token_recent_logs(&access_token.id, 1, None)
            .await
            .expect("read token log")
            .into_iter()
            .next()
            .expect("token log exists");
        assert_eq!(latest_log.result_status, "success");
        assert_eq!(latest_log.error_message, None);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn tavily_http_research_blocks_when_estimate_would_exceed_quota() {
        let db_path = temp_db_path("http-research-estimate-block");
        let db_str = db_path.to_string_lossy().to_string();

        // Research mini estimate is 40 credits.
        let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "39");

        let expected_api_key = "tvly-http-research-estimate-block-key";
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
            .create_access_token(Some("http-research-estimate-block"))
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
        let pool = connect_sqlite_test_pool(&db_str).await;
        let api_key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("api key id");

        let request_id = "req-test-123";
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

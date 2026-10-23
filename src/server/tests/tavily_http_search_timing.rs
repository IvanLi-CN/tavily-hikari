use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn tavily_http_search_returns_429_when_quota_exhausted_and_logs_token_attempt() {
    let db_path = temp_db_path("http-search-429-quota");
    let db_str = db_path.to_string_lossy().to_string();

    // Keep env stable across proxy creation + quota warmup.
    let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "2");

    let expected_api_key = "tvly-http-search-429-quota-key";
    let (upstream_addr, _hits) =
        spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
    let usage_base = format!("http://{upstream_addr}");
    let metrics = Arc::new(
        tavily_hikari::observability::Metrics::new(None).expect("test metrics"),
    );
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        &usage_base,
        &db_str,
    )
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

    let proxy_addr =
        spawn_proxy_server_with_metrics(proxy.clone(), usage_base, metrics.clone()).await;

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
    let pool = connect_sqlite_test_pool(&db_str).await;

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
    assert_single_request_timing(
        &metrics.gather_test_metrics(),
        "http_search",
        "POST",
        "search",
        "rate_limited",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn tavily_http_search_serializes_bound_tokens_but_not_distinct_accounts() {
    let db_path = temp_db_path("http-search-bound-account-lock-scope");
    let db_str = db_path.to_string_lossy().to_string();

    let _hourly_business_guard = EnvVarGuard::set("TOKEN_HOURLY_LIMIT", "1000");

    let expected_api_key = "tvly-http-search-bound-account-lock-scope-key";
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
        &usage_base,
        &db_str,
    )
    .await
    .expect("proxy created");
    let user_a = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "test".to_string(),
            provider_user_id: "http-search-bound-account-a".to_string(),
            username: Some("bound-account-a".to_string()),
            name: Some("Bound Account A".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert account A");
    let user_b = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "test".to_string(),
            provider_user_id: "http-search-bound-account-b".to_string(),
            username: Some("bound-account-b".to_string()),
            name: Some("Bound Account B".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert account B");
    proxy
        .update_account_business_quota_limits(&user_a.user_id, 100, 1_000, 10_000)
        .await
        .expect("set account A quota");
    proxy
        .update_account_business_quota_limits(&user_b.user_id, 100, 1_000, 10_000)
        .await
        .expect("set account B quota");

    let first_a = proxy
        .ensure_user_token_binding(&user_a.user_id, Some("bound-account-a-first"))
        .await
        .expect("bind first account A token");
    let second_a_unbound = proxy
        .create_access_token(Some("bound-account-a-second"))
        .await
        .expect("create second account A token");
    let second_a = proxy
        .ensure_user_token_binding_with_preferred(
            &user_a.user_id,
            Some("bound-account-a-second"),
            Some(&second_a_unbound.id),
        )
        .await
        .expect("bind second account A token");
    assert_eq!(second_a.id, second_a_unbound.id);
    let token_b = proxy
        .ensure_user_token_binding(&user_b.user_id, Some("bound-account-b-token"))
        .await
        .expect("bind account B token");

    let pool = connect_sqlite_test_pool(&db_str).await;
    for (token_id, expected_user_id) in [
        (&first_a.id, &user_a.user_id),
        (&second_a.id, &user_a.user_id),
        (&token_b.id, &user_b.user_id),
    ] {
        let owner = sqlx::query_scalar::<_, String>(
            "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
        )
        .bind(token_id)
        .fetch_one(&pool)
        .await
        .expect("query bound token owner");
        assert_eq!(&owner, expected_user_id);
    }

    let first_metrics = Arc::new(
        tavily_hikari::observability::Metrics::new(None).expect("first request metrics"),
    );
    let second_metrics = Arc::new(
        tavily_hikari::observability::Metrics::new(None).expect("second request metrics"),
    );
    let other_metrics = Arc::new(
        tavily_hikari::observability::Metrics::new(None).expect("other account metrics"),
    );
    let first_addr =
        spawn_proxy_server_with_metrics(proxy.clone(), usage_base.clone(), first_metrics.clone())
            .await;
    let second_addr = spawn_proxy_server_with_metrics(
        proxy.clone(),
        usage_base.clone(),
        second_metrics.clone(),
    )
    .await;
    let other_addr =
        spawn_proxy_server_with_metrics(proxy.clone(), usage_base, other_metrics.clone()).await;
    let client = Client::new();

    let first = tokio::spawn({
        let client = client.clone();
        let url = format!("http://{first_addr}/api/tavily/search");
        let token = first_a.token.clone();
        async move {
            client
                .post(url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&serde_json::json!({ "query": "bound-account-first" }))
                .send()
                .await
                .expect("first bound-account request")
        }
    });
    arrived.notified().await;

    let second = tokio::spawn({
        let client = client.clone();
        let url = format!("http://{second_addr}/api/tavily/search");
        let token = second_a.token.clone();
        async move {
            client
                .post(url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&serde_json::json!({ "query": "bound-account-second" }))
                .send()
                .await
                .expect("second bound-account request")
        }
    });
    let other = tokio::spawn({
        let client = client.clone();
        let url = format!("http://{other_addr}/api/tavily/search");
        let token = token_b.token.clone();
        async move {
            client
                .post(url)
                .header("Authorization", format!("Bearer {token}"))
                .json(&serde_json::json!({ "query": "other-account" }))
                .send()
                .await
                .expect("other account request")
        }
    });

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if hits.load(Ordering::SeqCst) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("distinct account reaches upstream while same account waits");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        2,
        "only the first same-account request and distinct-account request should reach upstream"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;

    for _ in 0..3 {
        release.notify_one();
    }
    let first_response = first.await.expect("join first request");
    let second_response = second.await.expect("join second request");
    let other_response = other.await.expect("join other account request");
    assert_eq!(first_response.status(), reqwest::StatusCode::OK);
    assert_eq!(second_response.status(), reqwest::StatusCode::OK);
    assert_eq!(other_response.status(), reqwest::StatusCode::OK);
    for response in [first_response, second_response, other_response] {
        let body: serde_json::Value = response.json().await.expect("response json");
        assert_eq!(body.get("status"), Some(&serde_json::json!(200)));
    }
    assert_eq!(hits.load(Ordering::SeqCst), 3);

    let second_phase_sums = assert_single_request_timing(
        &second_metrics.gather_test_metrics(),
        "http_search",
        "POST",
        "search",
        "success",
    );
    assert!(
        second_phase_sums["billing_lock_wait"] > 0.05,
        "same-account token must record the controlled billing wait"
    );
    let other_phase_sums = assert_single_request_timing(
        &other_metrics.gather_test_metrics(),
        "http_search",
        "POST",
        "search",
        "success",
    );
    assert!(
        other_phase_sums["billing_lock_wait"] < 0.05,
        "distinct account should not inherit the same-account billing wait"
    );
    assert_single_request_timing(
        &first_metrics.gather_test_metrics(),
        "http_search",
        "POST",
        "search",
        "success",
    );

    assert_eq!(
        proxy
            .peek_token_quota(&first_a.id)
            .await
            .expect("peek account A first quota")
            .hourly_used,
        2
    );
    assert_eq!(
        proxy
            .peek_token_quota(&second_a.id)
            .await
            .expect("peek account A second quota")
            .hourly_used,
        2
    );
    assert_eq!(
        proxy
            .peek_token_quota(&token_b.id)
            .await
            .expect("peek account B quota")
            .hourly_used,
        1
    );

    let _ = std::fs::remove_file(db_path);
}

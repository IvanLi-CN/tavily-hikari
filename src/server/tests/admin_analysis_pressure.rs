use super::*;
use super::core_support_and_parsing::*;
use super::linuxdo_oauth_and_admin_keys::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn analysis_pressure_snapshot_requires_admin_auth_and_exposes_expected_shape() {
    let db_path = temp_db_path("admin-analysis-pressure-http-shape");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "analysis-pressure-alice".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice Chen".to_string()),
            avatar_template: Some("/avatar/alice/{size}/1.png".to_string()),
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("create alice");
    let bob = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "analysis-pressure-bob".to_string(),
            username: Some("bob".to_string()),
            name: Some("Bob Lin".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("create bob");

    let alice_token = proxy
        .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice-analysis-pressure"))
        .await
        .expect("bind alice token");
    let bob_token = proxy
        .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob-analysis-pressure"))
        .await
        .expect("bind bob token");

    let pool = connect_sqlite_test_pool(&db_str).await;
    let now_ts = Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id,
            auth_token_id,
            request_user_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            counts_business_quota,
            upstream_operation,
            visibility,
            created_at
        ) VALUES (?, NULL, ?, 'POST', '/api/tavily/search', 200, 200, ?, 1, ?, 'visible', ?)
        "#,
    )
    .bind(&alice_token.id)
    .bind(&alice.user_id)
    .bind("success")
    .bind("search")
    .bind(now_ts - 600)
    .execute(&pool)
    .await
    .expect("insert alice success");
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id,
            auth_token_id,
            request_user_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            counts_business_quota,
            upstream_operation,
            visibility,
            created_at
        ) VALUES (?, NULL, ?, 'POST', '/api/tavily/search', 500, 500, ?, 1, ?, 'visible', ?)
        "#,
    )
    .bind(&alice_token.id)
    .bind(&alice.user_id)
    .bind("error")
    .bind("search")
    .bind(now_ts - 300)
    .execute(&pool)
    .await
    .expect("insert alice failure");
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id,
            auth_token_id,
            request_user_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            counts_business_quota,
            upstream_operation,
            visibility,
            created_at
        ) VALUES (?, NULL, ?, 'POST', '/api/tavily/search', 200, 200, ?, 1, ?, 'visible', ?)
        "#,
    )
    .bind(&bob_token.id)
    .bind(&bob.user_id)
    .bind("success")
    .bind("search")
    .bind(now_ts - 120)
    .execute(&pool)
    .await
    .expect("insert bob success");
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id,
            auth_token_id,
            request_user_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            counts_business_quota,
            upstream_operation,
            visibility,
            created_at
        ) VALUES (?, NULL, ?, 'POST', '/api/tavily/search', 429, 429, 'quota_exhausted', 1, 'search', 'visible', ?)
        "#,
    )
    .bind(&bob_token.id)
    .bind(&bob.user_id)
    .bind(now_ts - 60)
    .execute(&pool)
    .await
    .expect("insert quota exhausted exclusion");
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id,
            auth_token_id,
            request_user_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            counts_business_quota,
            upstream_operation,
            visibility,
            created_at
        ) VALUES (?, NULL, ?, 'POST', '/api/tavily/search', 403, 403, 'error', 1, NULL, 'visible', ?)
        "#,
    )
    .bind(&bob_token.id)
    .bind(&bob.user_id)
    .bind(now_ts - 30)
    .execute(&pool)
    .await
    .expect("insert pre-upstream exclusion");
    drop(pool);
    drop(proxy);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("reopen proxy with startup backfill");

    let admin_password = "admin-analysis-pressure-password";
    let background_proxy = proxy.clone();
    let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
    assert!(
        background_proxy.spawn_server_pressure_buckets_rebuild_once(),
        "serving admin harness should schedule background pressure rebuild"
    );
    let verification_pool = connect_sqlite_test_pool(&db_str).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");

    let unauth_resp = client
        .get(format!("http://{}/api/analysis/pressure", admin_addr))
        .send()
        .await
        .expect("unauth pressure request");
    assert_eq!(unauth_resp.status(), reqwest::StatusCode::FORBIDDEN);

    let login_resp = client
        .post(format!("http://{}/api/admin/login", admin_addr))
        .json(&serde_json::json!({ "password": admin_password }))
        .send()
        .await
        .expect("admin login");
    assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
    let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("admin session cookie");

    let pressure_resp = client
        .get(format!("http://{}/api/analysis/pressure", admin_addr))
        .header(reqwest::header::COOKIE, admin_cookie.clone())
        .send()
        .await
        .expect("analysis pressure request");
    assert_eq!(pressure_resp.status(), reqwest::StatusCode::OK);
    let mut pressure_json: serde_json::Value = pressure_resp
        .json()
        .await
        .expect("analysis pressure json");

    assert!(
        pressure_json
            .get("generatedAt")
            .and_then(|value| value.as_i64())
            .is_some(),
        "expected generatedAt"
    );
    assert_eq!(
        pressure_json
            .pointer("/server24h/current")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(288)
    );
    assert_eq!(
        pressure_json
            .pointer("/server24h/previous")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(288)
    );
    assert_eq!(
        pressure_json
            .pointer("/server7d/points")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(168)
    );
    assert_eq!(
        pressure_json
            .pointer("/server7d/movingAverages")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        pressure_json
            .pointer("/server7d/movingAverages/0/points")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(168)
    );
    assert_eq!(
        pressure_json
            .pointer("/server7d/movingAverages/1/points")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(168)
    );
    assert_eq!(
        pressure_json
            .pointer("/currentUserDistribution/rows")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(2)
    );
    let rows = pressure_json
        .pointer("/currentUserDistribution/rows")
        .and_then(|value| value.as_array())
        .expect("pressure distribution rows");
    let row_for = |user_id: &str| {
        rows.iter()
            .find(|row| row.get("userId").and_then(|value| value.as_str()) == Some(user_id))
            .expect("expected pressure row for user")
    };
    assert_eq!(
        row_for(&alice.user_id)
            .get("pressure")
            .and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        row_for(&bob.user_id)
            .get("pressure")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        pressure_json
            .pointer("/currentUserDistribution/summary/activeUsers")
            .and_then(|value| value.as_i64()),
        Some(2)
    );
    assert_eq!(
        pressure_json
            .pointer("/currentUserDistribution/summary/zeroPressureUsers")
            .and_then(|value| value.as_i64()),
        Some(0)
    );
    assert_eq!(
        pressure_json
            .pointer("/currentUserDistribution/summary/p90")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        pressure_json
            .pointer("/currentUserDistribution/summary/currentPressure")
            .and_then(|value| value.as_i64()),
        Some(3)
    );
    assert_eq!(
        pressure_json
            .pointer("/currentUserDistribution/summary/vsYesterdayDelta")
            .and_then(|value| value.as_i64()),
        Some(3)
    );
    let rebuild_deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let bucket_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
        )
        .fetch_one(&verification_pool)
        .await
        .expect("count rebuilt server pressure buckets");
        if bucket_count >= 2 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < rebuild_deadline,
            "expected admin analysis pressure history buckets to recover after background rebuild, bucket_count={bucket_count}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let refreshed_resp = client
        .get(format!("http://{}/api/analysis/pressure", admin_addr))
        .header(reqwest::header::COOKIE, admin_cookie.clone())
        .send()
        .await
        .expect("analysis pressure refresh request");
    assert_eq!(refreshed_resp.status(), reqwest::StatusCode::OK);
    pressure_json = refreshed_resp
        .json()
        .await
        .expect("analysis pressure refresh json");
    assert_eq!(
        pressure_json
            .pointer("/server24h/currentPeak/pressure")
            .and_then(|value| value.as_i64()),
        Some(3)
    );

    let _ = std::fs::remove_file(db_path);
}

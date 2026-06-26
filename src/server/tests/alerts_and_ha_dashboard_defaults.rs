use super::*;
use super::core_support_and_parsing::*;
use super::linuxdo_oauth_and_admin_keys::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn alerts_endpoints_default_to_all_history_while_dashboard_recent_alerts_stays_24h() {
    let db_path = temp_db_path("alerts-dashboard-default-window");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-alerts-dashboard-default-window".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-alert-user".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice Wang".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("alerts-bound"))
        .await
        .expect("ensure token binding");

    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();
    let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, ?, 'POST', '/api/tavily/search', 'max_results=5', 429, 429, 'HTTP 429', 'error', '{"query":"quota"}', '{"status":429}', '[]', '[]', ?)
            RETURNING id
            "#,
        )
        .bind(&key_id)
        .bind(&token.id)
        .bind(now - 60)
        .fetch_one(&pool)
        .await
        .expect("insert request log");

    let upstream_429_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', 'max_results=5', 429, NULL, 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 'error', 'HTTP 429', 'upstream_rate_limited_429', 'none', 'none', 'none', 1, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(request_log_id)
        .bind(now - 60)
        .fetch_one(&pool)
        .await
        .expect("insert upstream 429 auth token log");

    let upstream_432_request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, ?, 'POST', '/api/tavily/search', NULL, 432, 432, 'usage limit', 'quota_exhausted', ?, ?, '[]', '[]', ?)
            RETURNING id
            "#,
        )
        .bind(&key_id)
        .bind(&token.id)
        .bind(r#"{"query":"usage"}"#)
        .bind(r#"{"detail":{"error":"This request exceeds your plan's set usage limit."}}"#)
        .bind(now - 45)
        .fetch_one(&pool)
        .await
        .expect("insert upstream 432 request log");

    sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', NULL, 432, NULL, 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 'quota_exhausted', ?, 'none', 'none', 'none', 1, ?, ?)
            "#,
        )
        .bind(&token.id)
        .bind("This request exceeds your plan's set usage limit.")
        .bind(upstream_432_request_log_id)
        .bind(now - 45)
        .execute(&pool)
        .await
        .expect("insert upstream 432 auth token log");

    sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                api_key_id,
                created_at
            ) VALUES (?, 'POST', '/mcp', NULL, 429, -1, 'mcp_search', 'MCP Search', 'POST /mcp', 'quota_exhausted', 'hourly any-request limit exceeded', 'none', 'none', 'none', 0, NULL, ?)
            "#,
        )
        .bind(&token.id)
        .bind(now - 120)
        .execute(&pool)
        .await
        .expect("insert request-rate-limited auth token log");

    sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                api_key_id,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', NULL, 429, NULL, 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 'quota_exhausted', 'quota exhausted', 'none', 'none', 'none', 1, ?, ?)
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(now - 180)
        .execute(&pool)
        .await
        .expect("insert user-quota auth token log");

    sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                api_key_id,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', NULL, 429, NULL, 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 'quota_exhausted', 'old quota exhausted', 'none', 'none', 'none', 1, ?, ?)
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(now - 30 * 3600)
        .execute(&pool)
        .await
        .expect("insert old auth token log outside default window");

    sqlx::query(
            r#"
            INSERT INTO api_key_maintenance_records (
                id,
                key_id,
                source,
                operation_code,
                operation_summary,
                reason_code,
                reason_summary,
                reason_detail,
                request_log_id,
                auth_token_log_id,
                auth_token_id,
                created_at
            ) VALUES (?, ?, 'system', 'quarantine', 'Quarantine key', 'account_deactivated', 'Upstream account deactivated', 'The upstream disabled this key.', ?, ?, ?, ?)
            "#,
        )
        .bind("maint-alert-1")
        .bind(&key_id)
        .bind(request_log_id)
        .bind(upstream_429_log_id)
        .bind(&token.id)
        .bind(now - 30)
        .execute(&pool)
        .await
        .expect("insert maintenance alert");

    let admin_password = "alerts-dashboard-default-window-password";
    let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");

    let login_resp = client
        .post(format!("http://{}/api/admin/login", admin_addr))
        .json(&serde_json::json!({ "password": admin_password }))
        .send()
        .await
        .expect("admin login");
    assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
    let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("admin session cookie");

    let catalog_resp = client
        .get(format!("http://{}/api/alerts/catalog", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("alert catalog request");
    assert_eq!(catalog_resp.status(), reqwest::StatusCode::OK);
    let catalog_body: serde_json::Value = catalog_resp.json().await.expect("alert catalog json");
    assert_eq!(
        catalog_body
            .get("requestKindOptions")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        catalog_body
            .get("users")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(1)
    );

    let events_resp = client
        .get(format!("http://{}/api/alerts/events", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("alert events request");
    assert_eq!(events_resp.status(), reqwest::StatusCode::OK);
    let events_body: serde_json::Value = events_resp.json().await.expect("alert events json");
    assert_eq!(
        events_body.get("total").and_then(|value| value.as_i64()),
        Some(6)
    );
    assert_eq!(
        events_body
            .pointer("/items/0/type")
            .and_then(|value| value.as_str()),
        Some("upstream_key_blocked")
    );
    assert_eq!(
        events_body
            .pointer("/items/1/type")
            .and_then(|value| value.as_str()),
        Some("upstream_usage_limit_432")
    );
    assert_eq!(
        events_body
            .pointer("/items/1/key/id")
            .and_then(|value| value.as_str()),
        Some(key_id.as_str())
    );
    assert_eq!(
        events_body
            .pointer("/items/1/request/id")
            .and_then(|value| value.as_i64()),
        Some(upstream_432_request_log_id)
    );
    assert_eq!(
        events_body
            .pointer("/items/2/type")
            .and_then(|value| value.as_str()),
        Some("upstream_rate_limited_429")
    );
    assert_eq!(
        events_body
            .pointer("/items/2/request/id")
            .and_then(|value| value.as_i64()),
        Some(request_log_id)
    );

    let upstream_429_request_kind = events_body
        .pointer("/items/2/requestKind/key")
        .and_then(|value| value.as_str())
        .expect("upstream 429 request kind key");

    let filtered_events_resp = client
        .get(format!(
            "http://{}/api/alerts/events?request_kind={}&type=upstream_rate_limited_429",
            admin_addr, upstream_429_request_kind
        ))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("filtered alert events request");
    assert_eq!(filtered_events_resp.status(), reqwest::StatusCode::OK);
    let filtered_events_body: serde_json::Value = filtered_events_resp
        .json()
        .await
        .expect("filtered alert events json");
    assert_eq!(
        filtered_events_body
            .get("total")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        filtered_events_body
            .pointer("/items/0/requestKind/key")
            .and_then(|value| value.as_str()),
        Some("api:search")
    );

    let filtered_groups_resp = client
        .get(format!(
            "http://{}/api/alerts/groups?request_kind={}&type=upstream_rate_limited_429",
            admin_addr, upstream_429_request_kind
        ))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("filtered alert groups request");
    assert_eq!(filtered_groups_resp.status(), reqwest::StatusCode::OK);
    let filtered_groups_body: serde_json::Value = filtered_groups_resp
        .json()
        .await
        .expect("filtered alert groups json");
    assert_eq!(
        filtered_groups_body
            .get("total")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        filtered_groups_body
            .pointer("/items/0/requestKind/key")
            .and_then(|value| value.as_str()),
        Some("api:search")
    );

    let groups_resp = client
        .get(format!("http://{}/api/alerts/groups", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("alert groups request");
    assert_eq!(groups_resp.status(), reqwest::StatusCode::OK);
    let groups_body: serde_json::Value = groups_resp.json().await.expect("alert groups json");
    assert_eq!(
        groups_body.get("total").and_then(|value| value.as_i64()),
        Some(5)
    );
    assert_eq!(
        groups_body
            .pointer("/items/0/type")
            .and_then(|value| value.as_str()),
        Some("upstream_key_blocked")
    );
    let semantic_rate_group = groups_body
        .get("items")
        .and_then(|value| value.as_array())
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("type").and_then(|value| value.as_str())
                    == Some("user_request_rate_limited")
            })
        })
        .expect("semantic request-rate mother group");
    assert_eq!(
        semantic_rate_group
            .get("groupingKind")
            .and_then(|value| value.as_str()),
        Some("mother")
    );
    assert_eq!(
        semantic_rate_group
            .get("childCount")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        semantic_rate_group
            .pointer("/children/0/groupingKind")
            .and_then(|value| value.as_str()),
        Some("child")
    );
    assert_eq!(
        semantic_rate_group
            .pointer("/children/0/childEvents/0/type")
            .and_then(|value| value.as_str()),
        Some("user_request_rate_limited")
    );

    let paged_groups_resp = client
        .get(format!(
            "http://{}/api/alerts/groups?page=2&per_page=1",
            admin_addr
        ))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("paged alert groups request");
    assert_eq!(paged_groups_resp.status(), reqwest::StatusCode::OK);
    let paged_groups_body: serde_json::Value =
        paged_groups_resp.json().await.expect("paged alert groups json");
    assert_eq!(
        paged_groups_body.get("total").and_then(|value| value.as_i64()),
        Some(5)
    );
    assert_eq!(
        paged_groups_body
            .pointer("/items/0/type")
            .and_then(|value| value.as_str()),
        Some("upstream_usage_limit_432")
    );
    assert_eq!(
        paged_groups_body
            .pointer("/items/0/groupingKind")
            .and_then(|value| value.as_str()),
        Some("compat")
    );
    assert_eq!(
        paged_groups_body
            .pointer("/items/0/children")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(0)
    );

    let semantic_page_resp = client
        .get(format!(
            "http://{}/api/alerts/groups?page=4&per_page=1",
            admin_addr
        ))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("semantic paged alert groups request");
    assert_eq!(semantic_page_resp.status(), reqwest::StatusCode::OK);
    let semantic_page_body: serde_json::Value = semantic_page_resp
        .json()
        .await
        .expect("semantic paged alert groups json");
    assert_eq!(
        semantic_page_body
            .pointer("/items/0/type")
            .and_then(|value| value.as_str()),
        Some("user_request_rate_limited")
    );
    assert_eq!(
        semantic_page_body
            .pointer("/items/0/groupingKind")
            .and_then(|value| value.as_str()),
        Some("mother")
    );
    assert_eq!(
        semantic_page_body
            .pointer("/items/0/childCount")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        semantic_page_body
            .pointer("/items/0/children/0/groupingKind")
            .and_then(|value| value.as_str()),
        Some("child")
    );
    assert_eq!(
        semantic_page_body
            .pointer("/items/0/children/0/childEvents/0/type")
            .and_then(|value| value.as_str()),
        Some("user_request_rate_limited")
    );

    let overview_resp = client
        .get(format!("http://{}/api/dashboard/overview", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("dashboard overview request");
    assert_eq!(overview_resp.status(), reqwest::StatusCode::OK);
    let overview_body: serde_json::Value =
        overview_resp.json().await.expect("dashboard overview json");
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/windowHours")
            .and_then(|value| value.as_i64()),
        Some(24)
    );
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/totalEvents")
            .and_then(|value| value.as_i64()),
        Some(5)
    );
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/groupedCount")
            .and_then(|value| value.as_i64()),
        Some(5)
    );
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/countsByType")
            .and_then(|value| value.as_array())
            .map(|values| values
                .iter()
                .filter_map(|item| item.get("count").and_then(|value| value.as_i64()))
                .sum::<i64>()),
        Some(5)
    );
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/topGroups/0/type")
            .and_then(|value| value.as_str()),
        Some("upstream_key_blocked")
    );

    let _ = std::fs::remove_file(db_path);
}

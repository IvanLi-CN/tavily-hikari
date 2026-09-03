use super::*;
use super::core_support_and_parsing::*;
use super::linuxdo_oauth_and_admin_keys::*;
use super::upstream_support_and_manual_jobs::*;
use tavily_hikari::SqliteAdmissionOutcome;

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

    for _ in 0..48 {
        proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("advance alert projection before admin reads");
        tokio::task::yield_now().await;
    }

    let projection_proxy = proxy.clone();
    let admin_password = "alerts-dashboard-default-window-password";
    let (admin_addr, dashboard_state) =
        spawn_builtin_keys_admin_server_with_state(proxy, admin_password).await;
    let canonical_catalog = projection_proxy
        .admin_alert_catalog()
        .await
        .expect("build canonical alert catalog fixture");
    super::super::record_admin_alerts_last_good(
        dashboard_state.as_ref(),
        "catalog".to_string(),
        super::super::AdminAlertsReadCacheValue::Catalog(canonical_catalog),
    )
    .await;
    let canonical_events = projection_proxy
        .admin_alert_events_page(None, None, None, None, None, None, &[], 1, 20)
        .await
        .expect("build canonical alert events fixture");
    super::super::record_admin_alerts_last_good(
        dashboard_state.as_ref(),
        super::super::default_admin_alert_cache_key("events"),
        super::super::AdminAlertsReadCacheValue::Events(canonical_events),
    )
    .await;
    let canonical_groups = projection_proxy
        .admin_alert_groups_page(None, None, None, None, None, None, &[], 1, 20)
        .await
        .expect("build canonical alert groups fixture");
    super::super::record_admin_alerts_last_good(
        dashboard_state.as_ref(),
        super::super::default_admin_alert_cache_key("groups"),
        super::super::AdminAlertsReadCacheValue::Groups(canonical_groups),
    )
    .await;
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

    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_recent_summaries SET computed_at = ? WHERE window_hours = 24",
    )
    .bind(Utc::now().timestamp().saturating_sub(61))
    .execute(&pool)
    .await
    .expect("expire projected alert summary before dashboard refresh");

    for _ in 0..48 {
        projection_proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("advance alert projection before dashboard read");
        let projected = projection_proxy
            .recent_alerts_summary(24)
            .await
            .expect("read projected summary");
        if !projected.stale && projected.total_events == 5 {
            break;
        }
    }
    let projected = projection_proxy
        .recent_alerts_summary(24)
        .await
        .expect("read projected summary");
    assert!(!projected.stale, "projected summary is still stale: {projected:?}");
    assert_eq!(projected.total_events, 5);
    *dashboard_state.dashboard_overview_cache.lock().await =
        DashboardOverviewCacheState::default();
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
            .pointer("/recentAlerts/groupedCountWindows/0/windowHours")
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/groupedCountWindows/1/windowHours")
            .and_then(|value| value.as_i64()),
        Some(24)
    );
    assert_eq!(
        overview_body
            .pointer("/recentAlerts/groupedCountWindows/2/windowHours")
            .and_then(|value| value.as_i64()),
        Some(168)
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

#[tokio::test]
async fn admin_alerts_pressure_uses_same_key_last_good_and_reports_cold_misses() {
    let db_path = temp_db_path("admin-alerts-last-good");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-last-good".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let password = "admin-alerts-last-good-password";
    let (admin_addr, state) = spawn_builtin_keys_admin_server_with_state(proxy, password).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");
    let login = client
        .post(format!("http://{admin_addr}/api/admin/login"))
        .json(&serde_json::json!({ "password": password }))
        .send()
        .await
        .expect("admin login");
    let cookie = find_cookie_pair(login.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("admin session cookie");

    let cold = client
        .get(format!("http://{admin_addr}/api/alerts/catalog"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("cold alerts catalog");
    assert_eq!(cold.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(cold.headers().get("retry-after").and_then(|v| v.to_str().ok()), Some("1"));

    let mut projection_ready = false;
    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before warming the admin cache");
        let summary = state
            .proxy
            .recent_alerts_summary(24)
            .await
            .expect("read empty projected alerts");
        projection_ready = state.proxy.admin_alert_catalog().await.is_ok();
        if projection_ready && !summary.stale {
            break;
        }
    }
    assert!(projection_ready, "empty projection must complete before warming admin cache");
    let catalog = state
        .proxy
        .admin_alert_catalog()
        .await
        .expect("build canonical catalog for the warm-cache fixture");
    super::super::record_admin_alerts_last_good(
        state.as_ref(),
        "catalog".to_string(),
        super::super::AdminAlertsReadCacheValue::Catalog(catalog),
    )
    .await;
    let warm = client
        .get(format!("http://{admin_addr}/api/alerts/catalog"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("warm alerts catalog");
    assert_eq!(warm.status(), reqwest::StatusCode::OK);
    let warm_body: serde_json::Value = warm.json().await.expect("warm alerts json");
    assert_eq!(warm_body.get("coverage"), None);
    assert_eq!(warm_body.get("staleReason"), None);

    // Alerts no longer depend on a maintenance-bulk permit. The native,
    // connection-local read deadline is the pressure boundary that selects
    // an exact-key last-good response.
    super::super::mark_dashboard_overview_alert_projection_dirty(state.as_ref()).await;
    state.proxy.force_next_admin_alert_read_deadline_for_test();
    let stale = client
        .get(format!("http://{admin_addr}/api/alerts/catalog"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("stale alerts catalog");
    assert_eq!(stale.status(), reqwest::StatusCode::OK);
    let stale_body: serde_json::Value = stale.json().await.expect("stale alerts json");
    assert_eq!(stale_body.get("coverage").and_then(|v| v.as_str()), Some("stale"));
    assert_eq!(
        stale_body.get("staleReason").and_then(|v| v.as_str()),
        Some("projection_refresh")
    );

    state.proxy.force_next_admin_alert_read_deadline_for_test();
    let cold = client
        .get(format!("http://{admin_addr}/api/alerts/events?page=2"))
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("cold alerts page");
    assert_eq!(cold.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(cold.headers().get("retry-after").and_then(|v| v.to_str().ok()), Some("1"));
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_alerts_canonical_read_waits_for_history_projection_coverage() {
    let db_path = temp_db_path("admin-alerts-history-coverage");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-history-coverage".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    // Drain the empty sidecar so the recent lane is healthy before isolating
    // the history lane. This mirrors the scheduler's normal catch-up path.
    for _ in 0..64 {
        proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("advance alert projection");
        if proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_history_state SET phase = 'catching_up'",
    )
    .execute(&pool)
    .await
    .expect("hold history projection in catch-up");

    let result = proxy.admin_alert_catalog().await;
    assert!(
        matches!(
            result,
            Err(tavily_hikari::ProxyError::Deferred { operation, ref reason })
                if operation == "admin_alerts_read" && reason == "history_projection_catching_up"
        ),
        "canonical admin reads must not publish while history is incomplete: {result:?}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_privacy_status_uses_a_dedicated_read_session_outside_bulk_admission() {
    let db_path = temp_db_path("admin-privacy-status-last-good");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-privacy-status-last-good".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let password = "admin-privacy-status-last-good-password";
    let (admin_addr, state) = spawn_builtin_keys_admin_server_with_state(proxy, password).await;
    prime_admin_privacy_status_for_test(state.clone()).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");
    let login = client
        .post(format!("http://{admin_addr}/api/admin/login"))
        .json(&serde_json::json!({ "password": password }))
        .send()
        .await
        .expect("admin login");
    let cookie = find_cookie_pair(login.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("admin session cookie");
    let privacy_status_url = format!("http://{admin_addr}/api/settings/system/privacy-status");

    let warm = client
        .get(&privacy_status_url)
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("warm privacy status");
    assert_eq!(warm.status(), reqwest::StatusCode::OK);
    let warm_body: serde_json::Value = warm.json().await.expect("warm privacy status json");
    assert!(warm_body.get("dailyReconciliationProgress").is_some());
    assert!(warm_body.get("retryBuckets").is_some());
    let refresh_count_before = admin_privacy_refresh_count_for_test(state.as_ref()).await;
    expire_admin_privacy_status_last_good_for_test(state.as_ref()).await;

    let held = match state.proxy.admit_dashboard_rollup_integrity() {
        SqliteAdmissionOutcome::Admitted(permit) => permit,
        SqliteAdmissionOutcome::Deferred { reason } => {
            panic!("test must hold the shared bulk permit, got {reason}")
        }
    };
    let stale = client
        .get(&privacy_status_url)
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("stale privacy status");
    assert_eq!(stale.status(), reqwest::StatusCode::OK);
    let stale_body: serde_json::Value = stale.json().await.expect("stale privacy status json");
    assert_eq!(stale_body.get("coverage").and_then(|v| v.as_str()), Some("stale"));
    assert!(
        stale_body
            .get("staleReason")
            .and_then(|value| value.as_str())
            .is_some()
    );
    assert_eq!(
        stale_body.get("staleReason").and_then(|value| value.as_str()),
        Some("refresh_in_flight"),
        "the controller must start the detached read while bulk work is held"
    );
    assert_eq!(
        admin_privacy_refresh_count_for_test(state.as_ref()).await,
        refresh_count_before + 1,
        "bulk admission must not defer the privacy-status singleflight refresh"
    );
    drop(held);
    wait_for_admin_privacy_status_refresh(state.as_ref()).await;
    let refreshed = client
        .get(&privacy_status_url)
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("refreshed privacy status");
    assert_eq!(refreshed.status(), reqwest::StatusCode::OK);
    let refreshed_body: serde_json::Value = refreshed
        .json()
        .await
        .expect("refreshed privacy status json");
    assert_eq!(
        refreshed_body.get("coverage").and_then(|value| value.as_str()),
        Some("ok")
    );

    let cold_db_path = temp_db_path("admin-privacy-status-cold-pressure");
    let cold_db_str = cold_db_path.to_string_lossy().to_string();
    let cold_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-privacy-status-cold-pressure".to_string()],
        DEFAULT_UPSTREAM,
        &cold_db_str,
    )
    .await
    .expect("cold proxy created");
    let cold_password = "admin-privacy-status-cold-pressure-password";
    let (cold_addr, cold_state) =
        spawn_builtin_keys_admin_server_with_state(cold_proxy, cold_password).await;
    let cold_login = client
        .post(format!("http://{cold_addr}/api/admin/login"))
        .json(&serde_json::json!({ "password": cold_password }))
        .send()
        .await
        .expect("cold admin login");
    let cold_cookie = find_cookie_pair(cold_login.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("cold admin session cookie");
    let cold_held = match cold_state.proxy.admit_dashboard_rollup_integrity() {
        SqliteAdmissionOutcome::Admitted(permit) => permit,
        SqliteAdmissionOutcome::Deferred { reason } => {
            panic!("test must hold the cold shared bulk permit, got {reason}")
        }
    };
    let cold_lock_pool = connect_sqlite_test_pool(&cold_db_str).await;
    let mut cold_writer = cold_lock_pool.acquire().await.expect("acquire cold writer");
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut *cold_writer)
        .await
        .expect("hold exclusive cold read lock");
    sqlx::query("CREATE TABLE admin_privacy_status_cold_lock (id INTEGER)")
        .execute(&mut *cold_writer)
        .await
        .expect("hold schema lock for cold privacy status");
    let cold_started = std::time::Instant::now();
    let cold_errors_before = cold_state.proxy.admin_privacy_read_errors_for_test();
    let cold = client
        .get(format!("http://{cold_addr}/api/settings/system/privacy-status"))
        .header(reqwest::header::COOKIE, &cold_cookie)
        .send()
        .await
        .expect("cold privacy status");
    assert_eq!(
        cold.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "a cold privacy read under SQLite pressure must return a bounded retry response"
    );
    assert_eq!(
        cold.headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );
    assert!(cold_started.elapsed() < std::time::Duration::from_millis(250));
    wait_for_admin_privacy_status_refresh(cold_state.as_ref()).await;
    assert_eq!(
        cold_state.proxy.admin_privacy_read_discards_for_test(),
        0,
        "a bounded SQLite BEGIN failure must restore its connection instead of discarding it"
    );
    assert!(
        cold_state.proxy.admin_privacy_read_errors_for_test() > cold_errors_before,
        "a failed cold privacy refresh must enter the runtime workload error metrics"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut *cold_writer)
        .await
        .expect("release exclusive cold read lock");
    drop(cold_held);
    cold_state
        .proxy
        .verify_admin_privacy_read_connection_clean_for_test()
        .await
        .expect("the failed cold refresh must leave the next SQLite transaction clean");
    wait_for_admin_privacy_status_last_good(cold_state.as_ref()).await;
    let ready = client
        .get(format!("http://{cold_addr}/api/settings/system/privacy-status"))
        .header(reqwest::header::COOKIE, &cold_cookie)
        .send()
        .await
        .expect("privacy status after autonomous cold refresh");
    assert_eq!(ready.status(), reqwest::StatusCode::OK);
    let ready_body: serde_json::Value = ready
        .json()
        .await
        .expect("cold refresh privacy status json");
    assert_eq!(ready_body.get("coverage").and_then(|v| v.as_str()), Some("ok"));

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(cold_db_path);
}

#[tokio::test]
async fn admin_privacy_status_serves_stale_last_good_without_cancelling_its_read_snapshot() {
    let db_path = temp_db_path("admin-privacy-status-singleflight-stale");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-privacy-status-singleflight-stale".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let password = "admin-privacy-status-singleflight-stale-password";
    let (admin_addr, state) = spawn_builtin_keys_admin_server_with_state(proxy, password).await;
    prime_admin_privacy_status_for_test(state.clone()).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");
    let login = client
        .post(format!("http://{admin_addr}/api/admin/login"))
        .json(&serde_json::json!({ "password": password }))
        .send()
        .await
        .expect("admin login");
    let cookie = find_cookie_pair(login.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("admin session cookie");
    let privacy_status_url = format!("http://{admin_addr}/api/settings/system/privacy-status");

    let warm = client
        .get(&privacy_status_url)
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("warm privacy status");
    assert_eq!(warm.status(), reqwest::StatusCode::OK);
    let refresh_count_before = admin_privacy_refresh_count_for_test(state.as_ref()).await;
    expire_admin_privacy_status_last_good_for_test(state.as_ref()).await;
    let pause = state
        .proxy
        .install_admin_privacy_read_pause_for_test()
        .await;
    let first_request = {
        let client = client.clone();
        let cookie = cookie.clone();
        let privacy_status_url = privacy_status_url.clone();
        tokio::spawn(async move {
            client
                .get(privacy_status_url)
                .header(reqwest::header::COOKIE, cookie)
                .send()
                .await
                .expect("stale privacy status")
        })
    };
    let stale = tokio::time::timeout(std::time::Duration::from_millis(225), first_request)
        .await
        .expect("last-good request must not wait for the refresh")
        .expect("stale request task joins");
    assert_eq!(stale.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = stale.json().await.expect("stale privacy status json");
    assert_eq!(body.get("coverage").and_then(|value| value.as_str()), Some("stale"));
    tokio::time::timeout(std::time::Duration::from_secs(2), pause.wait_until_arrived())
        .await
        .expect("the background refresh reached its controlled read pause");
    let second_started = std::time::Instant::now();
    let second = client
        .get(&privacy_status_url)
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("second stale privacy status");
    assert_eq!(second.status(), reqwest::StatusCode::OK);
    assert!(
        second_started.elapsed() < std::time::Duration::from_millis(225),
        "a refresh already in flight must not delay another stale reader"
    );
    assert_eq!(
        admin_privacy_refresh_count_for_test(state.as_ref()).await,
        refresh_count_before + 1,
        "concurrent request refreshes must be singleflight"
    );
    assert_eq!(
        state.proxy.admin_privacy_read_discards_for_test(),
        0,
        "the request budget must not discard an open read snapshot"
    );
    pause.release();
    wait_for_admin_privacy_status_refresh(state.as_ref()).await;
    let refresh_count_after_completion = admin_privacy_refresh_count_for_test(state.as_ref()).await;
    assert_eq!(
        start_admin_privacy_status_refresh(state.clone()).await,
        AdminPrivacyStatusRefreshStart::Fresh,
        "a completed refresh must publish fresh last-good before another claim can start"
    );
    assert_eq!(
        admin_privacy_refresh_count_for_test(state.as_ref()).await,
        refresh_count_after_completion,
        "a completion-boundary claim must not start a redundant privacy refresh"
    );

    expire_admin_privacy_status_last_good_for_test(state.as_ref()).await;
    let shutdown_pause = state
        .proxy
        .install_admin_privacy_read_pause_for_test()
        .await;
    let shutdown_stale = client
        .get(&privacy_status_url)
        .header(reqwest::header::COOKIE, &cookie)
        .send()
        .await
        .expect("stale privacy status before shutdown");
    assert_eq!(shutdown_stale.status(), reqwest::StatusCode::OK);
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        shutdown_pause.wait_until_arrived(),
    )
    .await
    .expect("the shutdown refresh reached its controlled read pause");
    let shutdown = {
        let state = state.clone();
        tokio::spawn(async move {
            shutdown_admin_privacy_status_refresh(state.as_ref()).await;
        })
    };
    tokio::task::yield_now().await;
    assert!(
        !shutdown.is_finished(),
        "shutdown must wait for the refresh close boundary"
    );
    assert_eq!(
        state.proxy.admin_privacy_read_discards_for_test(),
        0,
        "shutdown must not discard an open read snapshot"
    );
    shutdown_pause.release();
    shutdown.await.expect("shutdown task joins");
    state
        .proxy
        .verify_admin_privacy_read_connection_clean_for_test()
        .await
        .expect("the next SQLite transaction must be clean after refresh");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn alerts_and_dashboard_recent_alerts_include_api_key_exhausted_and_job_failed() {
    let db_path = temp_db_path("alerts-dashboard-api-key-exhausted-job-failed");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-alerts-dashboard-api-key-exhausted-job-failed".to_string()],
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

    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();

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
            actor_user_id,
            actor_display_name,
            status_before,
            status_after,
            quarantine_before,
            quarantine_after,
            created_at
        ) VALUES (
            ?, ?, 'system', 'auto_mark_exhausted', '自动标记为 exhausted', 'quota_exhausted', '上游额度耗尽', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?
        )
        "#,
    )
    .bind("maint-api-key-exhausted-1")
    .bind(&key_id)
    .bind(now - 30)
    .execute(&pool)
    .await
    .expect("insert api key exhausted alert");

    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (
            job_type, trigger_source, key_id, status, attempt, message, queued_at, started_at, finished_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("quota_sync")
    .bind("manual")
    .bind(&key_id)
    .bind("failed")
    .bind(2_i64)
    .bind("quota sync failed after upstream timeout")
    .bind(now - 25)
    .bind(now - 24)
    .bind(now - 20)
    .execute(&pool)
    .await
    .expect("insert failed scheduled job");

    for _ in 0..48 {
        proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("advance projected recent alerts before Dashboard read");
        tokio::task::yield_now().await;
        let summary = proxy
            .recent_alerts_summary(24)
            .await
            .expect("read projected recent alerts");
        if !summary.stale && summary.total_events == 2 {
            break;
        }
    }
    let projected_summary = proxy
        .recent_alerts_summary(24)
        .await
        .expect("read final projected recent alerts");
    assert_eq!(projected_summary.coverage, "ok");
    assert_eq!(projected_summary.total_events, 2);
    assert!(!projected_summary.stale);
    let mut direct_alerts = None;
    for _ in 0..128 {
        proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("advance full alert projection before admin read");
        if let Ok(events) = proxy
            .admin_alert_events_page(None, None, None, None, None, None, &[], 1, 20)
            .await
        {
            direct_alerts = Some(events);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(direct_alerts.is_some(), "direct admin alerts read did not become ready");
    let canonical_groups = proxy
        .admin_alert_groups_page(None, None, None, None, None, None, &[], 1, 20)
        .await
        .expect("canonical groups fixture");

    let admin_password = "alerts-dashboard-api-key-exhausted-job-failed-password";
    let (admin_addr, state) =
        spawn_builtin_keys_admin_server_with_state(proxy, admin_password).await;
    let canonical_events = direct_alerts
        .as_ref()
        .expect("canonical events fixture")
        .clone();
    super::super::record_admin_alerts_last_good(
        state.as_ref(),
        super::super::default_admin_alert_cache_key("events"),
        super::super::AdminAlertsReadCacheValue::Events(canonical_events),
    )
    .await;
    super::super::record_admin_alerts_last_good(
        state.as_ref(),
        super::super::default_admin_alert_cache_key("groups"),
        super::super::AdminAlertsReadCacheValue::Groups(canonical_groups),
    )
    .await;
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
    let events_resp = client
        .get(format!("http://{}/api/alerts/events", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("alert events request");
    assert_eq!(events_resp.status(), reqwest::StatusCode::OK);
    let events_body: serde_json::Value = events_resp.json().await.expect("alert events json");
    let event_types: Vec<String> = events_body
        .get("items")
        .and_then(|value| value.as_array())
        .expect("events items")
        .iter()
        .filter_map(|item| item.get("type").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect();
    assert!(event_types.contains(&"api_key_exhausted".to_string()));
    assert!(event_types.contains(&"job_failed".to_string()));
    assert_eq!(
        events_body
            .get("items")
            .and_then(|value| value.as_array())
            .and_then(|items| items.iter().find(|item| item.get("type").and_then(|value| value.as_str()) == Some("job_failed")))
            .and_then(|item| item.pointer("/job/id"))
            .and_then(|value| value.as_i64()),
        Some(1)
    );

    let groups_resp = client
        .get(format!("http://{}/api/alerts/groups", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("alert groups request");
    assert_eq!(groups_resp.status(), reqwest::StatusCode::OK);
    let groups_body: serde_json::Value = groups_resp.json().await.expect("alert groups json");
    let group_types: Vec<String> = groups_body
        .get("items")
        .and_then(|value| value.as_array())
        .expect("group items")
        .iter()
        .filter_map(|item| item.get("type").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect();
    assert!(group_types.contains(&"api_key_exhausted".to_string()));
    assert!(group_types.contains(&"job_failed".to_string()));

    let overview_resp = client
        .get(format!("http://{}/api/dashboard/overview", admin_addr))
        .header(reqwest::header::COOKIE, &admin_cookie)
        .send()
        .await
        .expect("dashboard overview request");
    assert_eq!(overview_resp.status(), reqwest::StatusCode::OK);
    let overview_body: serde_json::Value =
        overview_resp.json().await.expect("dashboard overview json");

    let counts_by_type = overview_body
        .pointer("/recentAlerts/countsByType")
        .and_then(|value| value.as_array())
        .expect("recent alerts counts by type");
    let recent_alert_types: Vec<String> = counts_by_type
        .iter()
        .filter_map(|item| item.get("type").and_then(|value| value.as_str()))
        .map(str::to_string)
        .collect();
    assert!(recent_alert_types.contains(&"api_key_exhausted".to_string()));
    assert!(recent_alert_types.contains(&"job_failed".to_string()));

    let _ = std::fs::remove_file(db_path);
}

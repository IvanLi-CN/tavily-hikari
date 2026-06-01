#[tokio::test]
async fn alerts_endpoints_and_dashboard_recent_alerts_share_default_window() {
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
        Some(5)
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

#[tokio::test]
async fn ha_snapshot_export_records_sync_watermark() {
    let db_path = temp_db_path("ha-snapshot-export");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-snapshot-export".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-a".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .get(format!("http://{addr}/api/admin/ha/snapshot"))
        .send()
        .await
        .expect("snapshot request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let manifest = response
        .headers()
        .get("x-ha-snapshot-manifest")
        .and_then(|value| value.to_str().ok())
        .expect("snapshot manifest header")
        .to_string();
    assert!(manifest.contains("node-a"));
    let bytes = response.bytes().await.expect("snapshot bytes");
    assert!(bytes.len() > 1024, "snapshot should contain sqlite bytes");

    let pool = connect_sqlite_test_pool(&db_str).await;
    let watermark: Option<String> =
        sqlx::query_scalar("SELECT detail FROM ha_sync_watermarks WHERE name = 'snapshot_export'")
            .fetch_optional(&pool)
            .await
            .expect("fetch sync watermark");
    assert!(
        watermark
            .as_deref()
            .is_some_and(|detail| detail.contains("node-a"))
    );
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_snapshot_import_restores_standby_business_tables() {
    let active_db = temp_db_path("ha-snapshot-active");
    let active_db_str = active_db.to_string_lossy().to_string();
    let active_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-active-snapshot-key".to_string()],
        DEFAULT_UPSTREAM,
        &active_db_str,
    )
    .await
    .expect("active proxy created");
    let active_ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-active".to_string(),
        database_path: Some(active_db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let active_addr = spawn_ha_admin_server(active_proxy, active_ha, true).await;
    let snapshot = Client::new()
        .get(format!("http://{active_addr}/api/admin/ha/snapshot"))
        .send()
        .await
        .expect("snapshot request")
        .bytes()
        .await
        .expect("snapshot bytes");

    let standby_db = temp_db_path("ha-snapshot-standby");
    let standby_db_str = standby_db.to_string_lossy().to_string();
    let standby_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-standby-old-key".to_string()],
        DEFAULT_UPSTREAM,
        &standby_db_str,
    )
    .await
    .expect("standby proxy created");
    let standby_ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-standby".to_string(),
        database_path: Some(standby_db_str.clone()),
        internal_token: Some("test-ha-internal-token".to_string()),
        ..tavily_hikari::HaConfig::default()
    });
    let standby_addr = spawn_ha_admin_server(standby_proxy, standby_ha, false).await;

    let import_response = Client::new()
        .put(format!(
            "http://{standby_addr}/api/admin/ha/snapshot?sourceNodeId=node-active"
        ))
        .header("x-ha-internal-token", "test-ha-internal-token")
        .body(snapshot)
        .send()
        .await
        .expect("snapshot import request");
    assert_eq!(import_response.status(), reqwest::StatusCode::OK);

    let pool = connect_sqlite_test_pool(&standby_db_str).await;
    let keys: Vec<String> = sqlx::query_scalar("SELECT api_key FROM api_keys ORDER BY api_key")
        .fetch_all(&pool)
        .await
        .expect("fetch restored keys");
    assert!(keys.contains(&"tvly-ha-active-snapshot-key".to_string()));
    assert!(!keys.contains(&"tvly-ha-standby-old-key".to_string()));
    let detail: Option<String> =
        sqlx::query_scalar("SELECT detail FROM ha_sync_watermarks WHERE name = 'snapshot_import'")
            .fetch_optional(&pool)
            .await
            .expect("fetch import watermark");
    assert!(
        detail
            .as_deref()
            .is_some_and(|value| value.contains("restoredTables"))
    );
    pool.close().await;
    let _ = std::fs::remove_file(active_db);
    let _ = std::fs::remove_file(standby_db);
}

#[tokio::test]
async fn ha_snapshot_import_accepts_large_sqlite_snapshot_after_checkpoint() {
    let active_db = temp_db_path("ha-snapshot-large-active");
    let active_db_str = active_db.to_string_lossy().to_string();
    let active_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-large-active-key".to_string()],
        DEFAULT_UPSTREAM,
        &active_db_str,
    )
    .await
    .expect("active proxy created");
    let active_pool = connect_sqlite_test_pool(&active_db_str).await;
    let large_body = vec![b'x'; 3 * 1024 * 1024];
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            method, path, result_status, request_body, visibility, created_at
        ) VALUES ('POST', '/ha/large-snapshot', 'success', ?, 'visible', ?)
        "#,
    )
    .bind(&large_body)
    .bind(Utc::now().timestamp())
    .execute(&active_pool)
    .await
    .expect("insert large WAL-backed request log");

    let active_ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-active-large".to_string(),
        database_path: Some(active_db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let active_addr = spawn_ha_admin_server(active_proxy, active_ha, true).await;
    let snapshot = Client::new()
        .get(format!("http://{active_addr}/api/admin/ha/snapshot"))
        .send()
        .await
        .expect("large snapshot request")
        .bytes()
        .await
        .expect("large snapshot bytes");
    assert!(
        snapshot.len() > 2 * 1024 * 1024,
        "snapshot should exceed axum's default body limit"
    );

    let standby_db = temp_db_path("ha-snapshot-large-standby");
    let standby_db_str = standby_db.to_string_lossy().to_string();
    let standby_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-large-standby-key".to_string()],
        DEFAULT_UPSTREAM,
        &standby_db_str,
    )
    .await
    .expect("standby proxy created");
    let standby_ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-standby-large".to_string(),
        database_path: Some(standby_db_str.clone()),
        internal_token: Some("test-ha-large-internal-token".to_string()),
        ..tavily_hikari::HaConfig::default()
    });
    let standby_addr = spawn_ha_admin_server(standby_proxy, standby_ha, false).await;
    let import_response = Client::new()
        .put(format!(
            "http://{standby_addr}/api/admin/ha/snapshot?sourceNodeId=node-active-large"
        ))
        .header("x-ha-internal-token", "test-ha-large-internal-token")
        .body(snapshot)
        .send()
        .await
        .expect("large snapshot import request");
    assert_eq!(import_response.status(), reqwest::StatusCode::OK);

    let standby_pool = connect_sqlite_test_pool(&standby_db_str).await;
    let restored_len: i64 = sqlx::query_scalar(
        "SELECT length(request_body) FROM request_logs WHERE path = '/ha/large-snapshot'",
    )
    .fetch_one(&standby_pool)
    .await
    .expect("fetch restored large request body length");
    assert_eq!(restored_len, large_body.len() as i64);

    active_pool.close().await;
    standby_pool.close().await;
    let _ = std::fs::remove_file(active_db);
    let _ = std::fs::remove_file(standby_db);
}

#[tokio::test]
async fn ha_startup_role_check_failure_does_not_recover_previous_active() {
    let edgeone_app = Router::new().fallback(post(|| async {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "Response": {
                    "Error": {
                        "Code": "InternalError",
                        "Message": "temporary EdgeOne failure"
                    },
                    "RequestId": "edgeone-startup-failure"
                }
            })),
        )
    }));
    let edgeone_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let edgeone_addr = edgeone_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(edgeone_listener, edgeone_app.into_make_service())
            .await
            .unwrap();
    });

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-previous-active".to_string(),
        node_public_host: Some("127.0.0.1".to_string()),
        node_public_port: Some(58102),
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: format!("http://{edgeone_addr}"),
        ..tavily_hikari::HaConfig::default()
    });

    let status =
        reconcile_ha_startup_role(&ha, Some(tavily_hikari::HaNodeRole::FullMaster)).await;
    assert_eq!(status.role, tavily_hikari::HaNodeRole::Standby);
    assert!(
        status
            .message
            .as_deref()
            .is_some_and(|message| message.contains("EdgeOne startup role check failed"))
    );
}

#[tokio::test]
async fn ha_promote_records_edgeone_request_response_audit() {
    let edgeone_app = Router::new().fallback(post(|| async {
        Json(serde_json::json!({
            "Response": {
                "RequestId": "edgeone-audit-test"
            }
        }))
    }));
    let edgeone_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let edgeone_addr = edgeone_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(edgeone_listener, edgeone_app.into_make_service())
            .await
            .unwrap();
    });

    let db_path = temp_db_path("ha-edgeone-audit");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-edgeone-audit".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-promote".to_string(),
        database_path: Some(db_str.clone()),
        node_public_host: Some("127.0.0.1".to_string()),
        node_public_port: Some(58101),
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: format!("http://{edgeone_addr}"),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .post(format!("http://{addr}/api/admin/ha/promote"))
        .json(&serde_json::json!({"force": true}))
        .send()
        .await
        .expect("promote response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let row: (String, String, String) = sqlx::query_as(
        r#"
        SELECT action, request_json, response_json
          FROM ha_edgeone_audit_logs
         WHERE action = 'ModifyAccelerationDomain'
         ORDER BY created_at DESC
         LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("fetch EdgeOne audit");
    assert_eq!(row.0, "ModifyAccelerationDomain");
    assert!(
        row.1.contains("\"Origin\":\"127.0.0.1\""),
        "request audit should contain split origin host: {}",
        row.1
    );
    assert!(
        row.1.contains("\"OriginProtocol\":\"HTTPS\""),
        "request audit should contain origin protocol: {}",
        row.1
    );
    assert!(
        row.1.contains("\"HttpsOriginPort\":58101"),
        "request audit should contain origin port: {}",
        row.1
    );
    assert!(row.2.contains("edgeone-audit-test"));
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_recovery_import_is_idempotent_and_keeps_importer_active() {
    let db_path = temp_db_path("ha-recovery-idempotent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-recovery-idempotent".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-new".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let client = Client::new();
    let payload = serde_json::json!({
        "batchId": "old-master-batch-1",
        "sourceNodeId": "node-old",
        "message": "usage/log/event recovery batch imported",
        "requestLogs": [{
            "authTokenId": "old-token",
            "method": "POST",
            "path": "/api/tavily/search",
            "statusCode": 200,
            "tavilyStatusCode": 200,
            "resultStatus": "success",
            "requestKindKey": "tavily_search",
            "requestKindLabel": "Tavily Search",
            "requestKindDetail": "POST /api/tavily/search",
            "businessCredits": 1,
            "requestBody": "{\"query\":\"old-master\"}",
            "responseBody": "{\"answer\":\"ok\"}",
            "forwardedHeaders": "[]",
            "droppedHeaders": "[]",
            "visibility": "visible",
            "createdAt": Utc::now().timestamp() - 60
        }],
        "authTokenLogs": [{
            "tokenId": "old-token",
            "method": "POST",
            "path": "/api/tavily/search",
            "httpStatus": 200,
            "mcpStatus": 200,
            "requestKindKey": "tavily_search",
            "requestKindLabel": "Tavily Search",
            "requestKindDetail": "POST /api/tavily/search",
            "resultStatus": "success",
            "countsBusinessQuota": 1,
            "businessCredits": 1,
            "billingState": "charged",
            "createdAt": Utc::now().timestamp() - 60
        }]
    });

    let first: Value = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&payload)
        .send()
        .await
        .expect("first recovery import")
        .json()
        .await
        .expect("first recovery response");
    assert_eq!(first["imported"], true);
    assert_eq!(first["eventCount"], 2);
    assert_eq!(first["status"]["role"], "full_master");

    let second: Value = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&payload)
        .send()
        .await
        .expect("second recovery import")
        .json()
        .await
        .expect("second recovery response");
    assert_eq!(second["imported"], false);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let row: (String, i64) = sqlx::query_as(
        "SELECT status, event_count FROM ha_recovery_batches WHERE id = 'old-master-batch-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch recovery batch");
    assert_eq!(row.0, "imported");
    assert_eq!(row.1, 2);
    let request_log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_logs WHERE auth_token_id = 'old-token'")
            .fetch_one(&pool)
            .await
            .expect("fetch imported request logs");
    assert_eq!(request_log_count, 1);
    let token_log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = 'old-token'")
            .fetch_one(&pool)
            .await
            .expect("fetch imported auth token logs");
    assert_eq!(token_log_count, 1);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_standby_blocks_external_tavily_and_mcp_business_routes() {
    let db_path = temp_db_path("ha-standby-business-gate");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-standby-business-gate".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-standby".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_proxy_server_with_dev_and_ha(
        proxy,
        "http://127.0.0.1:58088".to_string(),
        true,
        ha,
    )
    .await;
    let client = Client::new();

    let search = client
        .post(format!("http://{addr}/api/tavily/search"))
        .bearer_auth("th-missing-token-secret")
        .json(&serde_json::json!({"query": "ha"}))
        .send()
        .await
        .expect("search response");
    assert_eq!(search.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    let search_body: Value = search.json().await.expect("search body");
    assert_eq!(search_body["error"], "ha_role_not_serving");
    assert_eq!(search_body["role"], "standby");

    let mcp = client
        .post(format!("http://{addr}/mcp"))
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .send()
        .await
        .expect("mcp response");
    assert_eq!(mcp.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);

    let mcp_subpath = client
        .post(format!("http://{addr}/mcp/sse"))
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .send()
        .await
        .expect("mcp subpath response");
    assert_eq!(
        mcp_subpath.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );

    let usage = client
        .get(format!("http://{addr}/api/tavily/usage"))
        .send()
        .await
        .expect("usage response");
    assert_eq!(usage.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_provisional_allows_basic_tavily_and_mcp_entrypoints() {
    let db_path = temp_db_path("ha-provisional-business-gate");
    let db_str = db_path.to_string_lossy().to_string();
    let edgeone_app = Router::new().fallback(post(|| async {
        Json(serde_json::json!({
            "Response": {
                "RequestId": "test-edgeone-promote"
            }
        }))
    }));
    let edgeone_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let edgeone_addr = edgeone_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(edgeone_listener, edgeone_app.into_make_service())
            .await
            .unwrap();
    });
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-provisional-business-gate".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-provisional".to_string(),
        database_path: Some(db_str.clone()),
        node_public_host: Some("127.0.0.1".to_string()),
        node_public_port: Some(58100),
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: format!("http://{edgeone_addr}"),
        ..tavily_hikari::HaConfig::default()
    });
    ha.promote_self_to_provisional(true)
        .await
        .expect("promote through fake EdgeOne");
    let addr = spawn_proxy_server_with_dev_and_ha(
        proxy,
        "http://127.0.0.1:58088".to_string(),
        true,
        ha,
    )
    .await;
    let client = Client::new();

    let search = client
        .post(format!("http://{addr}/api/tavily/search"))
        .bearer_auth("th-missing-token-secret")
        .json(&serde_json::json!({"query": "ha"}))
        .send()
        .await
        .expect("search response");
    assert_eq!(search.status(), reqwest::StatusCode::UNAUTHORIZED);

    let mcp = client
        .post(format!("http://{addr}/mcp"))
        .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
        .send()
        .await
        .expect("mcp response");
    assert_eq!(mcp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let usage = client
        .get(format!("http://{addr}/api/tavily/usage"))
        .send()
        .await
        .expect("usage response");
    assert_eq!(usage.status(), reqwest::StatusCode::UNAUTHORIZED);

    let token_create = client
        .post(format!("http://{addr}/api/tokens"))
        .json(&serde_json::json!({"note": "blocked while provisional"}))
        .send()
        .await
        .expect("token create response");
    assert_eq!(
        token_create.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn compute_signatures_tracks_recent_alert_summary_changes() {
    let db_path = temp_db_path("summary-signatures-recent-alerts");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-signature-recent-alerts".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-signature-alert-user".to_string(),
            username: Some("sig_alert".to_string()),
            name: Some("Sig Alert".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("signature-alert-bound"))
        .await
        .expect("ensure token binding");

    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });

    let (before_sig, _) = compute_signatures(&state)
        .await
        .expect("compute signatures before alerts");
    let before_sig = before_sig.expect("summary signature before alerts");
    assert_eq!(before_sig.recent_alerts_total_events, 0);
    assert_eq!(before_sig.recent_alerts_grouped_count, 0);

    let now = Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
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
                created_at
            ) VALUES (?, 'POST', '/mcp', NULL, 429, -1, 'mcp_search', 'MCP Search', 'POST /mcp', 'quota_exhausted', 'hourly any-request limit exceeded', 'none', 'none', 'none', 0, ?)
            "#,
        )
        .bind(&token.id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert recent alert auth token log");

    let (after_sig, _) = compute_signatures(&state)
        .await
        .expect("compute signatures after alerts");
    let after_sig = after_sig.expect("summary signature after alerts");
    assert_eq!(after_sig.recent_alerts_total_events, 1);
    assert_eq!(after_sig.recent_alerts_grouped_count, 1);
    assert_eq!(
        after_sig.recent_alerts_counts,
        vec![
            (
                tavily_hikari::ALERT_TYPE_UPSTREAM_RATE_LIMITED_429.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_UPSTREAM_KEY_BLOCKED.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_USER_REQUEST_RATE_LIMITED.to_string(),
                1
            ),
            (
                tavily_hikari::ALERT_TYPE_USER_QUOTA_EXHAUSTED.to_string(),
                0
            ),
        ]
    );
    assert_ne!(before_sig, after_sig);

    let _ = std::fs::remove_file(db_path);
}

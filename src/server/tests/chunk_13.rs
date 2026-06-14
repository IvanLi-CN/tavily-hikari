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
async fn ha_snapshot_endpoint_is_gone() {
    let db_path = temp_db_path("ha-snapshot-gone");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-snapshot-gone".to_string()],
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
    assert_eq!(response.status(), reqwest::StatusCode::GONE);
    let body = response.text().await.expect("gone body");
    assert!(body.contains("ha_snapshot_removed"));

    let response = Client::new()
        .put(format!("http://{addr}/api/admin/ha/snapshot"))
        .body("deprecated snapshot body")
        .send()
        .await
        .expect("snapshot put request");
    assert_eq!(response.status(), reqwest::StatusCode::GONE);

    let response = Client::new()
        .put(format!("http://{addr}/api/admin/ha/snapshot"))
        .body(vec![b'x'; 128 * 1024])
        .send()
        .await
        .expect("oversized snapshot put request");
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_startup_clears_stale_outbox_suppression_marker() {
    let db_path = temp_db_path("ha-stale-outbox-suppression");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-suppression-marker".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("INSERT OR IGNORE INTO ha_outbox_suppression (id) VALUES ('local')")
        .execute(&pool)
        .await
        .expect("insert stale suppression marker");
    pool.close().await;

    let restarted = TavilyProxy::with_endpoint(
        vec!["tvly-ha-suppression-marker".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("restarted proxy created");
    drop(restarted);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let suppression_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox_suppression WHERE id = 'local'")
            .fetch_one(&pool)
            .await
            .expect("count suppression markers");
    assert_eq!(suppression_count, 0);

    sqlx::query(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('request_rate_limit_v1', '99')",
    )
    .execute(&pool)
    .await
    .expect("write whitelisted meta");
    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'meta'")
            .fetch_one(&pool)
            .await
            .expect("count emitted events");
    assert_eq!(event_count, 1);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_baseline_uses_zstd_and_excludes_call_records() {
    let active_db = temp_db_path("ha-baseline-active");
    let active_db_str = active_db.to_string_lossy().to_string();
    let active_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-baseline-key".to_string()],
        DEFAULT_UPSTREAM,
        &active_db_str,
    )
    .await
    .expect("active proxy created");
    let pool = connect_sqlite_test_pool(&active_db_str).await;
    let large_body = vec![b'x'; 3 * 1024 * 1024];
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            method, path, result_status, request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/ha/large-snapshot', 'success', ?, ?, 'visible', ?)
        "#,
    )
    .bind(&large_body)
    .bind(&large_body)
    .bind(Utc::now().timestamp())
    .execute(&pool)
    .await
    .expect("insert large request log");
    pool.close().await;
    let active_ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-active".to_string(),
        database_path: Some(active_db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let active_addr = spawn_ha_admin_server(active_proxy, active_ha, true).await;
    let response = Client::new()
        .get(format!("http://{active_addr}/api/admin/ha/baseline"))
        .send()
        .await
        .expect("baseline request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .and_then(|value| value.to_str().ok()),
        Some("zstd")
    );
    assert!(
        response.headers().get("x-ha-high-watermark").is_some(),
        "baseline should report high watermark"
    );
    let compressed = response.bytes().await.expect("baseline bytes");
    assert!(
        compressed.len() < 1024 * 1024,
        "baseline should stay small when request logs are large"
    );
    let decoded = zstd::stream::decode_all(compressed.as_ref()).expect("decode zstd baseline");
    let text = String::from_utf8(decoded).expect("baseline utf8");
    assert!(text.contains("\"kind\":\"baseline_start\""));
    assert!(text.contains("\"resource\":\"api_keys\""));
    assert!(text.contains("tvly-ha-baseline-key"));
    assert!(!text.contains("request_logs"));
    assert!(!text.contains("auth_token_logs"));
    assert!(!text.contains("/ha/large-snapshot"));
    assert!(!text.contains(&"x".repeat(1024)));

    let _ = std::fs::remove_file(active_db);
}

#[tokio::test]
async fn ha_events_endpoint_returns_zstd_ndjson() {
    let db_path = temp_db_path("ha-events");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'api_keys', 'key-1', 'upsert', '{"id":"key-1"}', ?, 'checksum')
        "#,
    )
    .bind(Utc::now().timestamp())
    .execute(&pool)
    .await
    .expect("insert outbox event");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'request_logs', 'log-1', 'upsert', '{"path":"/secret"}', ?, 'bad')
        "#,
    )
    .bind(Utc::now().timestamp())
    .execute(&pool)
    .await
    .expect("insert forbidden outbox event");
    pool.close().await;
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-events".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let response = Client::new()
        .get(format!("http://{addr}/api/admin/ha/events?after=0&limit=10"))
        .send()
        .await
        .expect("events request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .and_then(|value| value.to_str().ok()),
        Some("zstd")
    );
    let compressed = response.bytes().await.expect("events bytes");
    let decoded = zstd::stream::decode_all(compressed.as_ref()).expect("decode zstd events");
    let text = String::from_utf8(decoded).expect("events utf8");
    assert!(text.contains("\"kind\":\"event\""));
    assert!(text.contains("\"resource\":\"api_keys\""));
    assert!(!text.contains("request_logs"));
    assert!(!text.contains("/secret"));
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn ha_events_encoder_chunks_oversized_batches() {
    let events = (1..=4)
        .map(|seq| HaEventResponseItem {
            seq,
            value: serde_json::json!({
                "schemaVersion": 1,
                "kind": "event",
                "event": {
                    "seq": seq,
                    "kind": "state",
                    "resource": "meta",
                    "resourceId": format!("request_rate_limit_v1-{seq}"),
                    "op": "upsert",
                    "payload": {
                        "key": "request_rate_limit_v1",
                        "value": format!("{}-{}", seq, nanoid!(512))
                    },
                    "createdAt": seq,
                    "checksum": null
                }
            }),
        })
        .collect::<Vec<_>>();
    let single = encode_ha_events_limited(0, 4, &events[..1], usize::MAX)
        .expect("encode single event");
    let full = encode_ha_events_limited(0, 4, &events, usize::MAX).expect("encode full batch");
    assert!(full.compressed.len() > single.compressed.len());

    let chunked = encode_ha_events_limited(0, 4, &events, single.compressed.len())
        .expect("chunk oversized batch");
    assert_eq!(chunked.event_count, 1);
    assert_eq!(chunked.last_seq, 1);
}

#[tokio::test]
async fn ha_events_storage_allows_nonzero_cursor_when_outbox_is_empty() {
    let db_path = temp_db_path("ha-events-empty-retention");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-empty-retention-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("DELETE FROM ha_outbox")
        .execute(&pool)
        .await
        .expect("clear retained outbox");
    let current_seq = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT seq FROM sqlite_sequence WHERE name = 'ha_outbox'",
    )
    .fetch_optional(&pool)
    .await
    .expect("read retained outbox sequence")
    .flatten()
    .unwrap_or(0);
    pool.close().await;
    let events = proxy
        .list_ha_outbox_events_after(current_seq, 10)
        .await
        .expect("empty retained outbox should not force baseline");
    assert!(events.is_empty());

    let pool = connect_sqlite_test_pool(&db_str).await;
    let first_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'meta', 'request_rate_limit_v1', 'upsert', ?, 0, NULL)
        "#,
    )
    .bind(serde_json::json!({"key":"request_rate_limit_v1","value":"55"}).to_string())
    .execute(&pool)
    .await
    .expect("insert retained event")
    .last_insert_rowid();
    let second_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'meta', 'api_rebalance_enabled_v1', 'upsert', ?, 0, NULL)
        "#,
    )
    .bind(serde_json::json!({"key":"api_rebalance_enabled_v1","value":"true"}).to_string())
        .execute(&pool)
        .await
        .expect("insert retained event")
        .last_insert_rowid();
    pool.close().await;
    let err = proxy
        .list_ha_outbox_events_after(first_seq, 10)
        .await
        .expect_err("pruned cursor should require baseline");
    assert!(err.to_string().contains("retention window"));
    let events = proxy
        .list_ha_outbox_events_after(second_seq, 10)
        .await
        .expect("current cursor can poll empty pruned outbox");
    assert!(events.is_empty());
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_sync_transports_system_settings_meta_only() {
    let active_db = temp_db_path("ha-meta-active");
    let standby_db = temp_db_path("ha-meta-standby");
    let active_db_str = active_db.to_string_lossy().to_string();
    let standby_db_str = standby_db.to_string_lossy().to_string();
    let active = TavilyProxy::with_endpoint(
        vec!["tvly-ha-meta-active-key".to_string()],
        DEFAULT_UPSTREAM,
        &active_db_str,
    )
    .await
    .expect("active proxy created");
    let standby = TavilyProxy::with_endpoint(
        vec!["tvly-ha-meta-standby-key".to_string()],
        DEFAULT_UPSTREAM,
        &standby_db_str,
    )
    .await
    .expect("standby proxy created");

    let active_pool = connect_sqlite_test_pool(&active_db_str).await;
    sqlx::query(
        r#"
        INSERT INTO meta (key, value) VALUES
            ('request_rate_limit_v1', '42'),
            ('trusted_proxy_cidrs_v1', '["10.0.0.0/8"]'),
            ('ha_unsynced_local_marker', 'local-only')
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .execute(&active_pool)
    .await
    .expect("seed active meta");
    active_pool.close().await;

    let baseline = active
        .export_ha_baseline_ndjson("active-meta")
        .await
        .expect("export baseline");
    assert!(baseline.ndjson.contains("request_rate_limit_v1"));
    assert!(baseline.ndjson.contains("trusted_proxy_cidrs_v1"));
    assert!(!baseline.ndjson.contains("ha_unsynced_local_marker"));

    standby
        .apply_ha_baseline_ndjson(&baseline.ndjson)
        .await
        .expect("apply baseline");
    let standby_pool = connect_sqlite_test_pool(&standby_db_str).await;
    let request_rate_limit: Option<String> =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = 'request_rate_limit_v1'")
            .fetch_optional(&standby_pool)
            .await
            .expect("read synced meta");
    let local_only: Option<String> =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = 'ha_unsynced_local_marker'")
            .fetch_optional(&standby_pool)
            .await
            .expect("read unsynced meta");
    assert_eq!(request_rate_limit.as_deref(), Some("42"));
    assert!(local_only.is_none());
    standby_pool.close().await;

    let active_pool = connect_sqlite_test_pool(&active_db_str).await;
    sqlx::query("DELETE FROM ha_outbox")
        .execute(&active_pool)
        .await
        .expect("clear outbox");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value) VALUES
            ('request_rate_limit_v1', '55'),
            ('ha_unsynced_local_marker', 'still-local-only')
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .execute(&active_pool)
    .await
    .expect("update active meta");
    active_pool.close().await;

    let events = active
        .list_ha_outbox_events_after(0, 10)
        .await
        .expect("list meta events");
    assert_eq!(events.len(), 1, "only whitelisted meta should emit events");
    assert_eq!(events[0].resource, "meta");
    assert_eq!(events[0].payload["key"].as_str(), Some("request_rate_limit_v1"));
    let events_ndjson = [
        serde_json::json!({"schemaVersion":1,"kind":"events_start","after":0,"limit":10})
            .to_string(),
        serde_json::json!({"schemaVersion":1,"kind":"event","event":events[0]}).to_string(),
        serde_json::json!({"schemaVersion":1,"kind":"events_end","lastSeq":events[0].seq,"eventCount":1})
            .to_string(),
    ]
    .join("\n");
    standby
        .apply_ha_events_ndjson(&events_ndjson)
        .await
        .expect("apply meta event");
    let standby_pool = connect_sqlite_test_pool(&standby_db_str).await;
    let request_rate_limit: Option<String> =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = 'request_rate_limit_v1'")
            .fetch_optional(&standby_pool)
            .await
            .expect("read updated meta");
    let local_only: Option<String> =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = 'ha_unsynced_local_marker'")
            .fetch_optional(&standby_pool)
            .await
            .expect("read unsynced updated meta");
    assert_eq!(request_rate_limit.as_deref(), Some("55"));
    assert!(local_only.is_none());
    standby_pool.close().await;

    let _ = std::fs::remove_file(active_db);
    let _ = std::fs::remove_file(standby_db);
}

#[tokio::test]
async fn ha_standby_sync_does_not_repeat_zero_watermark_baseline() {
    let baseline_count = Arc::new(AtomicUsize::new(0));
    let events_count = Arc::new(AtomicUsize::new(0));
    let baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "baseline_start",
            "nodeId": "active-empty",
            "generatedAt": Utc::now().timestamp(),
            "highWatermark": 0,
            "encoding": "zstd-ndjson"
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "baseline_end",
            "nodeId": "active-empty",
            "highWatermark": 0,
            "rowCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let events_ndjson = [
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "events_start",
            "after": 0,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "events_end",
            "lastSeq": 0,
            "eventCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let baseline_body = zstd::stream::encode_all(baseline_ndjson.as_bytes(), 0)
        .expect("encode empty baseline");
    let events_body =
        zstd::stream::encode_all(events_ndjson.as_bytes(), 0).expect("encode empty events");
    let baseline_count_for_route = baseline_count.clone();
    let events_count_for_route = events_count.clone();
    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move || {
                let baseline_body = baseline_body.clone();
                let baseline_count = baseline_count_for_route.clone();
                async move {
                    baseline_count.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(baseline_body))
                        .expect("baseline response")
                }
            }),
        )
        .route(
            "/api/admin/ha/events",
            get(move || {
                let events_body = events_body.clone();
                let events_count = events_count_for_route.clone();
                async move {
                    events_count.fetch_add(1, Ordering::SeqCst);
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(events_body))
                        .expect("events response")
                }
            }),
        )
        .route("/api/admin/ha/events/ack", post(|| async { StatusCode::OK }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let source_addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let db_path = temp_db_path("ha-zero-watermark-standby");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-zero-watermark-standby-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("standby proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "standby-zero-watermark".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha,
        dev_open_admin: true,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
    });
    let source_url = format!("http://{source_addr}");
    let client = Client::new();

    run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect("first standby sync");
    run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect("second standby sync");

    assert_eq!(baseline_count.load(Ordering::SeqCst), 1);
    assert_eq!(events_count.load(Ordering::SeqCst), 2);
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_applied_seq")
            .await
            .expect("read applied seq"),
        Some(0)
    );
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_baseline_applied")
            .await
            .expect("read baseline marker"),
        Some(1)
    );
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_events_ack_records_peer_watermark() {
    let db_path = temp_db_path("ha-events-ack");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-ack-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-events-ack".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let response = Client::new()
        .post(format!("http://{addr}/api/admin/ha/events/ack"))
        .json(&serde_json::json!({"peerNodeId":"standby-a","ackedSeq":42}))
        .send()
        .await
        .expect("ack request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let acked: i64 =
        sqlx::query_scalar("SELECT acked_seq FROM ha_peer_watermarks WHERE peer_node_id = 'standby-a'")
            .fetch_one(&pool)
            .await
            .expect("fetch peer watermark");
    assert_eq!(acked, 42);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_event_apply_preserves_foreign_keys_and_composite_deletes() {
    let db_path = temp_db_path("ha-event-apply-keys");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-event-apply-seed".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO users (id, display_name, username, active, created_at, updated_at)
        VALUES ('user-ha-apply', 'HA Apply', 'ha_apply', 1, 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert user");
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at, last_used_at)
        VALUES ('key-ha-apply', 'tvly-ha-event-apply', 'active', 1, 0)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert key");
    sqlx::query(
        r#"
        INSERT INTO auth_tokens (id, secret, enabled, created_at)
        VALUES ('tok-ha-apply', 'secret-ha-apply', 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert token");
    sqlx::query(
        r#"
        INSERT INTO user_api_key_bindings (
            user_id, api_key_id, created_at, updated_at, last_success_at
        ) VALUES ('user-ha-apply', 'key-ha-apply', 1, 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert user key binding");
    sqlx::query(
        r#"
        INSERT INTO token_api_key_bindings (
            token_id, api_key_id, created_at, updated_at, last_success_at
        ) VALUES ('tok-ha-apply', 'key-ha-apply', 1, 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert token key binding");
    pool.close().await;

    let events = serde_json::json!([
        {"schemaVersion":1,"kind":"events_start","after":0,"limit":10},
        {
            "schemaVersion":1,
            "kind":"event",
            "event":{
                "seq":1,
                "kind":"state",
                "resource":"api_keys",
                "resourceId":"key-ha-apply",
                "op":"upsert",
                "payload":{
                    "id":"key-ha-apply",
                    "api_key":"tvly-ha-event-apply",
                    "status":"inactive",
                    "created_at":1,
                    "last_used_at":0
                },
                "createdAt":1,
                "checksum":null
            }
        },
        {
            "schemaVersion":1,
            "kind":"event",
            "event":{
                "seq":2,
                "kind":"state",
                "resource":"token_api_key_bindings",
                "resourceId":"not-the-standby-rowid",
                "op":"delete",
                "payload":{
                    "token_id":"tok-ha-apply",
                    "api_key_id":"key-ha-apply"
                },
                "createdAt":2,
                "checksum":null
            }
        },
        {
            "schemaVersion":1,
            "kind":"event",
            "event":{
                "seq":3,
                "kind":"state",
                "resource":"api_key_maintenance_records",
                "resourceId":"maint-ha-apply",
                "op":"upsert",
                "payload":{
                    "id":"maint-ha-apply",
                    "key_id":"key-ha-apply",
                    "source":"ha-test",
                    "operation_code":"disable",
                    "operation_summary":"disabled by test",
                    "request_log_id":999001,
                    "auth_token_log_id":999002,
                    "auth_token_id":"tok-ha-apply",
                    "created_at":3
                },
                "createdAt":3,
                "checksum":null
            }
        },
        {"schemaVersion":1,"kind":"events_end","lastSeq":3,"eventCount":3}
    ]);
    let ndjson = events
        .as_array()
        .expect("events array")
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let result = proxy
        .apply_ha_events_ndjson(&ndjson)
        .await
        .expect("apply ha events");
    assert_eq!(result.high_watermark, 3);
    assert_eq!(result.row_count, 3);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let status: String =
        sqlx::query_scalar("SELECT status FROM api_keys WHERE id = 'key-ha-apply'")
            .fetch_one(&pool)
            .await
            .expect("fetch key status");
    assert_eq!(status, "inactive");
    let user_binding_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_api_key_bindings WHERE api_key_id = 'key-ha-apply'",
    )
    .fetch_one(&pool)
    .await
    .expect("count user key bindings");
    assert_eq!(user_binding_count, 1);
    let token_binding_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM token_api_key_bindings WHERE token_id = 'tok-ha-apply' AND api_key_id = 'key-ha-apply'",
    )
    .fetch_one(&pool)
    .await
    .expect("count token key bindings");
    assert_eq!(token_binding_count, 0);
    let log_refs: (Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT request_log_id, auth_token_log_id FROM api_key_maintenance_records WHERE id = 'maint-ha-apply'",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch sanitized maintenance record");
    assert_eq!(log_refs, (None, None));
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
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
async fn ha_source_endpoint_persists_origin_group_settings() {
    let db_path = temp_db_path("ha-source-origin-group");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-source-origin-group".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-source-group".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .put(format!("http://{addr}/api/admin/ha/source"))
        .json(&serde_json::json!({
            "sourceKind": "origin_group",
            "originGroupId": "eo-group-api-test",
            "applyToEdgeone": false
        }))
        .send()
        .await
        .expect("source settings response");
    let status = response.status();
    let body = response.text().await.expect("source settings body text");
    assert!(
        status.is_success(),
        "source settings request should succeed, got {status}: {body}"
    );
    let response: Value = serde_json::from_str(&body).expect("source settings body");

    assert_eq!(response["haSourceOverride"]["sourceKind"], "origin_group");
    assert_eq!(response["haSourceOverride"]["originGroupId"], "eo-group-api-test");
    assert_eq!(response["haSourceEffective"]["target"], "eo-group-api-test");
    assert_eq!(response["edgeoneExpectedOrigin"], "eo-group-api-test");
    assert_eq!(response["edgeoneExpectedSourceKind"], "origin_group");

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

    let rejected = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&payload)
        .send()
        .await
        .expect("rejected recovery import");
    assert_eq!(rejected.status(), reqwest::StatusCode::BAD_REQUEST);
    let rejected_body = rejected.text().await.expect("rejected recovery body");
    assert!(
        rejected_body.contains("request_logs") && rejected_body.contains("auth_token_logs"),
        "legacy log recovery payload should be explicitly rejected: {rejected_body}"
    );

    let ledger_payload = serde_json::json!({
        "batchId": "old-master-batch-1",
        "sourceNodeId": "node-old",
        "message": "ledger recovery batch imported"
    });

    let first: Value = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&ledger_payload)
        .send()
        .await
        .expect("first ledger recovery import")
        .json()
        .await
        .expect("first ledger recovery response");
    assert_eq!(first["imported"], true);
    assert_eq!(first["eventCount"], 0);
    assert_eq!(first["status"]["role"], "full_master");

    let second: Value = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&ledger_payload)
        .send()
        .await
        .expect("second ledger recovery import")
        .json()
        .await
        .expect("second ledger recovery response");
    assert_eq!(second["imported"], false);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let row: (String, i64) = sqlx::query_as(
        "SELECT status, event_count FROM ha_recovery_batches WHERE id = 'old-master-batch-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch recovery batch");
    assert_eq!(row.0, "imported");
    assert_eq!(row.1, 0);
    let request_log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_logs WHERE auth_token_id = 'old-token'")
            .fetch_one(&pool)
            .await
            .expect("fetch rejected request logs");
    assert_eq!(request_log_count, 0);
    let token_log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = 'old-token'")
            .fetch_one(&pool)
            .await
            .expect("fetch rejected auth token logs");
    assert_eq!(token_log_count, 0);
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

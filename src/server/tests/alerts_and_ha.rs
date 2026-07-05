use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

const TEST_SECS_PER_DAY: i64 = 24 * 60 * 60;

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
    assert_eq!(event_count, 0);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_single_mode_does_not_emit_new_control_events() {
    let db_path = temp_db_path("ha-single-no-control-events");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_in_ha_mode(
        vec!["tvly-ha-single-mode-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        tavily_hikari::HaMode::Single,
    )
    .await
    .expect("proxy created");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES ('request_rate_limit_v1', '77')")
        .execute(&pool)
        .await
        .expect("write whitelisted meta");
    let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox")
        .fetch_one(&pool)
        .await
        .expect("count control outbox");
    assert_eq!(event_count, 0);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_switching_back_to_single_disables_billing_and_runtime_triggers() {
    let db_path = temp_db_path("ha-single-disables-non-control-triggers");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_in_ha_mode(
        vec!["tvly-ha-single-disable-triggers-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        tavily_hikari::HaMode::ActiveStandby,
    )
    .await
    .expect("proxy created");
    drop(proxy);

    let reopened = TavilyProxy::with_options_in_ha_mode(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        tavily_hikari::HaMode::Single,
    )
    .await
    .expect("proxy reopened in single mode");
    drop(reopened);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let control_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox")
        .fetch_one(&pool)
        .await
        .expect("count control outbox before single-mode writes");
    let billing_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_billing_outbox")
        .fetch_one(&pool)
        .await
        .expect("count billing outbox before single-mode writes");
    let runtime_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_runtime_outbox")
        .fetch_one(&pool)
        .await
        .expect("count runtime outbox before single-mode writes");
    let trigger_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'trg_ha_%'",
    )
    .fetch_one(&pool)
    .await
    .expect("count remaining ha triggers");
    sqlx::query(
        r#"
        INSERT INTO users (id, display_name, username, active, created_at, updated_at)
        VALUES ('user-ha-single-reopen', 'HA Single Reopen', 'ha_single_reopen', 1, 1, 1)
        ON CONFLICT(id) DO NOTHING
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed user for runtime row");
    sqlx::query(
        r#"
        INSERT INTO billing_ledger (
            auth_token_log_id, token_id, billing_subject, billing_state, business_credits,
            request_user_id, api_key_id, request_log_id, result_status, created_at, updated_at,
            settled_at, error_message
        ) VALUES (9101, 'tok-single-reopen', 'token:tok-single-reopen', 'charged', 2, NULL, NULL, NULL, 'success', 1, 1, 1, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert billing row after single-mode reopen");
    sqlx::query(
        r#"
        INSERT INTO account_quota_limits (
            user_id, business_calls_1h_limit, daily_credits_limit, monthly_credits_limit,
            monthly_broken_limit, monthly_blocked_key_limit_delta, inherits_defaults,
            created_at, updated_at
        ) VALUES ('user-ha-single-reopen', 2, 3, 4, 5, 0, 1, 1, 1)
        ON CONFLICT(user_id) DO UPDATE SET updated_at = excluded.updated_at
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert runtime row after single-mode reopen");

    let control_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox")
        .fetch_one(&pool)
        .await
        .expect("count control outbox");
    let billing_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_billing_outbox")
        .fetch_one(&pool)
        .await
        .expect("count billing outbox");
    let runtime_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_runtime_outbox")
        .fetch_one(&pool)
        .await
        .expect("count runtime outbox");

    assert_eq!(trigger_count, 0);
    assert_eq!(control_count, control_count_before);
    assert_eq!(billing_count, billing_count_before);
    assert_eq!(runtime_count, runtime_count_before);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_repair_clears_legacy_single_channel_triggers_from_upgraded_db() {
    let db_path = temp_db_path("ha-repair-legacy-single-channel-triggers");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_in_ha_mode(
        vec!["tvly-ha-repair-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        tavily_hikari::HaMode::ActiveStandby,
    )
    .await
    .expect("proxy created");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        CREATE TRIGGER trg_ha_outbox_scheduled_jobs_insert
        AFTER INSERT ON scheduled_jobs
        BEGIN
            INSERT INTO ha_outbox (
                kind, resource, resource_id, op, payload_json, created_at, checksum
            )
            VALUES (
                'state',
                'scheduled_jobs',
                CAST(NEW.id AS TEXT),
                'upsert',
                json_object('id', NEW.id, 'job_type', NEW.job_type),
                CAST(strftime('%s','now') AS INTEGER),
                NULL
            );
        END
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy scheduled_jobs trigger");
    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (
            job_type, trigger_source, status, attempt, queued_at, started_at, finished_at, message
        ) VALUES ('legacy_ha_trigger_test', 'manual', 'queued', 1, 1, NULL, NULL, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert scheduled job with legacy trigger");
    let legacy_count_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'scheduled_jobs'")
            .fetch_one(&pool)
            .await
            .expect("count legacy scheduled_jobs rows before repair");
    assert_eq!(legacy_count_before, 1);
    pool.close().await;

    let report =
        tavily_hikari::repair_ha_triggers_once(&db_str, tavily_hikari::HaMode::ActiveStandby)
            .await
            .expect("repair ha triggers");
    assert!(report.legacy_triggers_dropped >= 1);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let legacy_trigger_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name = 'trg_ha_outbox_scheduled_jobs_insert'",
    )
    .fetch_one(&pool)
    .await
    .expect("count legacy trigger after repair");
    assert_eq!(legacy_trigger_count, 0);
    sqlx::query(
        r#"
        INSERT INTO scheduled_jobs (
            job_type, trigger_source, status, attempt, queued_at, started_at, finished_at, message
        ) VALUES ('legacy_ha_trigger_test_after', 'manual', 'queued', 1, 2, NULL, NULL, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert scheduled job after repair");
    let legacy_count_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'scheduled_jobs'")
            .fetch_one(&pool)
            .await
            .expect("count legacy scheduled_jobs rows after repair");
    assert_eq!(legacy_count_after, 1);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_billing_and_runtime_channels_do_not_route_into_control_outbox() {
    let db_path = temp_db_path("ha-multichannel-outboxes");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_in_ha_mode(
        vec!["tvly-ha-multichannel-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        tavily_hikari::HaMode::ActiveStandby,
    )
    .await
    .expect("proxy created");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO users (id, display_name, username, active, created_at, updated_at)
        VALUES ('user-runtime', 'Runtime User', 'runtime_user', 1, 1, 1)
        ON CONFLICT(id) DO NOTHING
        "#,
    )
    .execute(&pool)
    .await
    .expect("seed runtime user");
    let control_count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox")
        .fetch_one(&pool)
        .await
        .expect("count initial control events");
    sqlx::query(
        r#"
        INSERT INTO billing_ledger (
            auth_token_log_id, token_id, billing_subject, billing_state, business_credits,
            request_user_id, api_key_id, request_log_id, result_status, created_at, updated_at,
            settled_at, error_message
        ) VALUES (9001, 'tok-billing', 'token:tok-billing', 'charged', 3, NULL, NULL, NULL, 'success', 1, 1, 1, NULL)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert billing row");
    sqlx::query(
        r#"
        INSERT INTO account_quota_limits (
            user_id, business_calls_1h_limit, daily_credits_limit, monthly_credits_limit,
            monthly_broken_limit, monthly_blocked_key_limit_delta, inherits_defaults,
            created_at, updated_at
        ) VALUES ('user-runtime', 2, 3, 4, 5, 0, 1, 1, 1)
        ON CONFLICT(user_id) DO UPDATE SET updated_at = excluded.updated_at
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert runtime row");

    let control_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox")
        .fetch_one(&pool)
        .await
        .expect("count control events");
    let billing_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_billing_outbox")
        .fetch_one(&pool)
        .await
        .expect("count billing events");
    let runtime_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_runtime_outbox")
        .fetch_one(&pool)
        .await
        .expect("count runtime events");

    assert_eq!(control_count, control_count_before);
    assert!(billing_count >= 1);
    assert!(runtime_count >= 1);
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
        .get(format!(
            "http://{active_addr}/api/admin/ha/baseline?channel=control"
        ))
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
async fn ha_baseline_returns_500_when_export_generation_fails() {
    let _env = EnvVarGuard::set("TAVILY_TEST_FAIL_HA_BASELINE_EXPORT", "control");
    let active_db = temp_db_path("ha-baseline-export-failure");
    let active_db_str = active_db.to_string_lossy().to_string();
    let active_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-baseline-export-failure".to_string()],
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

    let response = Client::new()
        .get(format!(
            "http://{active_addr}/api/admin/ha/baseline?channel=control"
        ))
        .send()
        .await
        .expect("baseline request");
    assert_eq!(response.status(), reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    let body = response.text().await.expect("baseline error body");
    assert!(
        body.contains("forced HA baseline export failure"),
        "unexpected baseline error body: {body}"
    );

    let _ = std::fs::remove_file(active_db);
}

#[tokio::test]
async fn ha_baseline_returns_413_when_compressed_stream_exceeds_cap() {
    let _cap = EnvVarGuard::set("TAVILY_TEST_HA_BASELINE_MAX_COMPRESSED_BYTES", "256");
    let active_db = temp_db_path("ha-baseline-export-too-large");
    let active_db_str = active_db.to_string_lossy().to_string();
    let active_proxy = TavilyProxy::with_endpoint(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &active_db_str,
    )
    .await
    .expect("active proxy created");
    let pool = connect_sqlite_test_pool(&active_db_str).await;
    let large_random = (0..4096)
        .map(|idx| format!("row-{idx:04}-{}", nanoid::nanoid!(64)))
        .collect::<Vec<_>>();
    for (idx, key) in large_random.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, created_at, status_changed_at
            ) VALUES (?, ?, 'active', ?, ?)
            "#,
        )
        .bind(format!("key-{idx}"))
        .bind(key)
        .bind(Utc::now().timestamp() + idx as i64)
        .bind(Utc::now().timestamp() + idx as i64)
        .execute(&pool)
        .await
        .expect("insert api key");
    }
    pool.close().await;
    let active_ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-active".to_string(),
        database_path: Some(active_db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let active_addr = spawn_ha_admin_server(active_proxy, active_ha, true).await;

    let response = Client::new()
        .get(format!(
            "http://{active_addr}/api/admin/ha/baseline?channel=control"
        ))
        .send()
        .await
        .expect("baseline request");
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let body = response.text().await.expect("baseline error body");
    assert!(
        body.contains("HA payload exceeds compressed limit"),
        "unexpected baseline error body: {body}"
    );

    let _ = std::fs::remove_file(active_db);
}

#[tokio::test]
async fn admin_ha_status_surfaces_peer_source_config_target() {
    let peer_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind peer listener");
    let peer_addr = peer_listener.local_addr().expect("peer addr");
    tokio::spawn(async move {
        let app = Router::new().route(
            "/api/internal/ha/status",
            get(|| async {
                Json(json!({
                    "mode": "active_standby",
                    "nodeId": "node-peer",
                    "nodePublicOrigin": "peer-public-origin:443",
                    "role": "standby",
                    "dualActiveEnabled": false,
                    "fullMasterNodeId": null,
                    "degraded": false,
                    "allowsBasicBusiness": false,
                    "allowsFullWrites": false,
                    "edgeoneDomain": "api.example.com",
                    "edgeoneOrigin": "edgeone-live-route:443",
                    "edgeoneExpectedOrigin": null,
                    "edgeoneCurrentTarget": "edgeone-live-route:443",
                    "edgeoneExpectedTarget": "peer-source-config:53844",
                    "edgeoneCurrentSourceKind": "direct",
                    "edgeoneExpectedSourceKind": "direct",
                    "edgeoneCurrentOriginGroupId": null,
                    "edgeoneExpectedOriginGroupId": null,
                    "haSourceDefaults": null,
                    "haSourceOverride": null,
                    "haSourceEffective": {
                        "sourceKind": "direct",
                        "directOriginScheme": "https",
                        "directOriginHost": "peer-source-config",
                        "directOriginPort": 53844,
                        "originGroupId": null,
                        "target": "peer-source-config:53844"
                    },
                    "edgeoneApiConfigured": true,
                    "lastEdgeoneCheckAt": 1_700_000_000,
                    "lastSyncAt": 1_700_000_001,
                    "syncLagSeconds": 1,
                    "recoveryStatus": null,
                    "message": "peer ready",
                    "peerNodes": [],
                    "plannedCutoverEligible": true
                }))
            }),
        );
        axum::serve(peer_listener, app.into_make_service())
            .await
            .expect("serve peer status");
    });

    let db_path = temp_db_path("ha-peer-source-config-target");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-peer-source-config-target".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-active".to_string(),
        database_path: Some(db_str.clone()),
        internal_token: Some("peer-token".to_string()),
        peer_nodes: vec![tavily_hikari::HaPeerNodeConfig {
            node_id: "node-peer".to_string(),
            admin_base_url: format!("http://{peer_addr}"),
            public_origin: "peer-public-origin:443".to_string(),
            role_hint: tavily_hikari::HaPeerRoleHint::StandbyCandidate,
        }],
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .get(format!("http://{addr}/api/admin/ha/status"))
        .send()
        .await
        .expect("ha status response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("ha status body");
    assert_eq!(body["peerNodes"][0]["publicOrigin"], "peer-public-origin:443");
    assert_eq!(body["peerNodes"][0]["sourceConfigTarget"], "peer-source-config:53844");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_events_endpoint_skips_legacy_non_control_rows_without_cursor_stall() {
    let db_path = temp_db_path("ha-events-legacy-control-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-legacy-cursor-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'request_logs', 'legacy-1', 'upsert', '{"path":"/legacy-1"}', ?, 'legacy-1')
        "#,
    )
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert first legacy row");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'request_logs', 'legacy-2', 'upsert', '{"path":"/legacy-2"}', ?, 'legacy-2')
        "#,
    )
    .bind(now + 1)
    .execute(&pool)
    .await
    .expect("insert second legacy row");
    let allowed_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'api_keys', 'key-2', 'upsert', '{"id":"key-2"}', ?, 'allowed')
        "#,
    )
    .bind(now + 2)
    .execute(&pool)
    .await
    .expect("insert allowed row")
    .last_insert_rowid();
    pool.close().await;

    let events = proxy
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Control, 0, 2)
        .await
        .expect("list control events should skip legacy rows");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, allowed_seq);
    assert_eq!(events[0].resource, "api_keys");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_endpoints_require_explicit_channel_query() {
    let db_path = temp_db_path("ha-channel-required");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-channel-required-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-channel-required".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let client = Client::new();

    let baseline = client
        .get(format!("http://{addr}/api/admin/ha/baseline"))
        .send()
        .await
        .expect("baseline request");
    assert_eq!(baseline.status(), reqwest::StatusCode::BAD_REQUEST);
    let baseline_body = baseline.text().await.expect("baseline body");
    assert!(
        baseline_body.contains("missing required HA channel"),
        "unexpected baseline error body: {baseline_body}"
    );

    let events = client
        .get(format!("http://{addr}/api/admin/ha/events?after=0&limit=10"))
        .send()
        .await
        .expect("events request");
    assert_eq!(events.status(), reqwest::StatusCode::BAD_REQUEST);
    let events_body = events.text().await.expect("events body");
    assert!(
        events_body.contains("missing required HA channel"),
        "unexpected events error body: {events_body}"
    );

    let _ = std::fs::remove_file(db_path);
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
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Control, current_seq, 10)
        .await
        .expect("empty retained outbox should not force baseline");
    assert!(events.is_empty());

    let pool = connect_sqlite_test_pool(&db_str).await;
    let recent_ts = Utc::now().timestamp();
    let first_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'meta', 'request_rate_limit_v1', 'upsert', ?, ?, NULL)
        "#,
    )
    .bind(serde_json::json!({"key":"request_rate_limit_v1","value":"55"}).to_string())
    .bind(recent_ts)
    .execute(&pool)
    .await
    .expect("insert retained event")
    .last_insert_rowid();
    let second_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'meta', 'api_rebalance_enabled_v1', 'upsert', ?, ?, NULL)
        "#,
    )
    .bind(serde_json::json!({"key":"api_rebalance_enabled_v1","value":"true"}).to_string())
    .bind(recent_ts + 1)
        .execute(&pool)
        .await
        .expect("insert retained event")
        .last_insert_rowid();
    pool.close().await;
    let events = proxy
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Control, first_seq, 10)
        .await
        .expect("existing rows after cursor should still be returned");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, second_seq);
    assert_eq!(events[0].resource, "meta");
    let events = proxy
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Control, second_seq, 10)
        .await
        .expect("current cursor can poll empty outbox after latest seq");
    assert!(events.is_empty());
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_events_read_path_hides_expired_rows_without_deleting_them() {
    let db_path = temp_db_path("ha-events-retention-read-filter");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-retention-filter-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let old_ts = Utc::now().timestamp() - (4 * TEST_SECS_PER_DAY);
    let recent_ts = Utc::now().timestamp();
    let old_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'meta', 'request_rate_limit_v1', 'upsert', ?, ?, NULL)
        "#,
    )
    .bind(serde_json::json!({"key":"request_rate_limit_v1","value":"55"}).to_string())
    .bind(old_ts)
    .execute(&pool)
    .await
    .expect("insert expired event")
    .last_insert_rowid();
    let recent_seq = sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'meta', 'api_rebalance_enabled_v1', 'upsert', ?, ?, NULL)
        "#,
    )
    .bind(serde_json::json!({"key":"api_rebalance_enabled_v1","value":"true"}).to_string())
    .bind(recent_ts)
    .execute(&pool)
    .await
    .expect("insert recent event")
    .last_insert_rowid();
    drop(pool);

    let events = proxy
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Control, 0, 10)
        .await
        .expect("list retained events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, recent_seq);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let stored_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE seq IN (?, ?)")
        .bind(old_seq)
        .bind(recent_seq)
        .fetch_one(&pool)
        .await
        .expect("count persisted rows");
    assert_eq!(stored_count, 2, "read path must not delete expired rows");
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
        .export_ha_baseline_ndjson(tavily_hikari::HaSyncChannel::Control, "active-meta")
        .await
        .expect("export baseline");
    assert!(baseline.ndjson.contains("request_rate_limit_v1"));
    assert!(baseline.ndjson.contains("trusted_proxy_cidrs_v1"));
    assert!(!baseline.ndjson.contains("ha_unsynced_local_marker"));

    standby
        .apply_ha_baseline_ndjson(tavily_hikari::HaSyncChannel::Control, &baseline.ndjson)
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
    let before_seq: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM ha_outbox")
        .fetch_one(&active_pool)
        .await
        .expect("read control watermark before update");
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
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Control, before_seq, 10)
        .await
        .expect("list meta events");
    assert_eq!(events.len(), 0, "meta changes are baseline-only in channel v2");
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
    assert_eq!(request_rate_limit.as_deref(), Some("42"));
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
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "control",
            "nodeId": "active-empty",
            "generatedAt": Utc::now().timestamp(),
            "highWatermark": 0,
            "encoding": "zstd-ndjson"
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "control",
            "nodeId": "active-empty",
            "highWatermark": 0,
            "rowCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let events_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "after": 0,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "control",
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
            admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha,
        dev_open_admin: true,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });
    let source_url = format!("http://{source_addr}");
    let client = Client::new();

    run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect("first standby sync");
    run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect("second standby sync");

    assert_eq!(baseline_count.load(Ordering::SeqCst), 3);
    assert_eq!(events_count.load(Ordering::SeqCst), 6);
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_applied_seq")
            .await
            .expect("read applied seq"),
        Some(0)
    );
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_baseline_applied")
            .await
            .expect("read baseline marker"),
        Some(1)
    );
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_standby_sync_recovers_after_invalid_baseline_stream() {
    let invalid_baseline_body =
        zstd::stream::encode_all(&b"{\"kind\":\"baseline_start\"}\nnot-json\n"[..], 0)
            .expect("encode invalid baseline");
    let valid_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "control",
            "nodeId": "active-retry",
            "highWatermark": 1
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "control",
            "resource": "users",
            "data": {
                "id": "user-ha-retry",
                "display_name": "Retry User",
                "username": "retry_user",
                "active": 1,
                "created_at": 1,
                "updated_at": 1
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "control",
            "nodeId": "active-retry",
            "highWatermark": 1,
            "rowCount": 1
        })
        .to_string(),
    ]
    .join("\n");
    let valid_baseline_body = zstd::stream::encode_all(valid_baseline_ndjson.as_bytes(), 0)
        .expect("encode valid baseline");
    let empty_events_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "after": 1,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "control",
            "lastSeq": 1,
            "eventCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let empty_events_body =
        zstd::stream::encode_all(empty_events_ndjson.as_bytes(), 0).expect("encode events");

    let baseline_requests = Arc::new(AtomicUsize::new(0));
    let baseline_requests_for_route = baseline_requests.clone();
    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move || {
                let invalid_baseline_body = invalid_baseline_body.clone();
                let valid_baseline_body = valid_baseline_body.clone();
                let baseline_requests = baseline_requests_for_route.clone();
                async move {
                    let attempt = baseline_requests.fetch_add(1, Ordering::SeqCst);
                    let body = if attempt == 0 {
                        invalid_baseline_body
                    } else {
                        valid_baseline_body
                    };
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(body))
                        .expect("baseline response")
                }
            }),
        )
        .route(
            "/api/admin/ha/events",
            get(move || {
                let empty_events_body = empty_events_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(empty_events_body))
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

    let db_path = temp_db_path("ha-invalid-baseline-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-invalid-baseline-recovery".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "standby-invalid-baseline".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha,
        dev_open_admin: true,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });
    let source_url = format!("http://{source_addr}");
    let client = Client::new();

    for channel in [
        tavily_hikari::HaSyncChannel::Billing,
        tavily_hikari::HaSyncChannel::Runtime,
    ] {
        proxy
            .persist_ha_sync_watermark(
                &format!("standby_{}_baseline_applied", channel.as_str()),
                Some(&source_url),
                Some("standby-invalid-baseline"),
                1,
                Some("seeded for control-only test"),
            )
            .await
            .expect("seed baseline marker");
    }

    let first_err = run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect_err("first sync should fail");
    let first_err_text = first_err.to_string();
    assert!(
        first_err_text.contains("invalid HA baseline NDJSON")
            || first_err_text.contains("unsupported HA baseline"),
        "unexpected first sync error: {first_err_text}"
    );

    run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect("second sync should recover");

    assert_eq!(baseline_requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_baseline_applied")
            .await
            .expect("read baseline marker"),
        Some(1)
    );
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_applied_seq")
            .await
            .expect("read applied seq"),
        Some(1)
    );

    let pool = connect_sqlite_test_pool(&db_str).await;
    let fk_enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .expect("foreign_keys pragma");
    assert_eq!(fk_enabled, 1);

    sqlx::query(
        r#"
        INSERT INTO user_tag_bindings (user_id, tag, created_at)
        VALUES ('missing-user', 'broken-tag', 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect_err("foreign key should still reject invalid binding");

    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = 'user-ha-retry'")
        .fetch_one(&pool)
        .await
        .expect("user inserted after recovery");
    assert_eq!(username, "retry_user");

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_standby_sync_recovers_after_invalid_events_stream() {
    let valid_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "control",
            "nodeId": "active-events-retry",
            "highWatermark": 1
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "control",
            "resource": "users",
            "data": {
                "id": "user-ha-events-retry",
                "display_name": "Events Retry User",
                "username": "events_retry_user",
                "active": 1,
                "created_at": 1,
                "updated_at": 1
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "control",
            "nodeId": "active-events-retry",
            "highWatermark": 1,
            "rowCount": 1
        })
        .to_string(),
    ]
    .join("\n");
    let valid_baseline_body = zstd::stream::encode_all(valid_baseline_ndjson.as_bytes(), 0)
        .expect("encode valid baseline");
    let invalid_events_body = zstd::stream::encode_all(
        &b"{\"kind\":\"events_start\"}\nnot-json\n"[..],
        0,
    )
    .expect("encode invalid events");
    let valid_events_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "after": 1,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "control",
            "event": {
                "seq": 2,
                "channel": "control",
                "kind": "state",
                "resource": "users",
                "resourceId": "user-ha-events-retry",
                "op": "upsert",
                "payload": {
                    "id": "user-ha-events-retry",
                    "display_name": "Events Retry User 2",
                    "username": "events_retry_user_2",
                    "active": 1,
                    "created_at": 1,
                    "updated_at": 2
                },
                "createdAt": 2,
                "checksum": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "control",
            "lastSeq": 2,
            "eventCount": 1
        })
        .to_string(),
    ]
    .join("\n");
    let valid_events_body =
        zstd::stream::encode_all(valid_events_ndjson.as_bytes(), 0).expect("encode valid events");
    let empty_events_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "after": 1,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "control",
            "lastSeq": 1,
            "eventCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let empty_events_body =
        zstd::stream::encode_all(empty_events_ndjson.as_bytes(), 0).expect("encode empty events");

    let events_requests = Arc::new(AtomicUsize::new(0));
    let events_requests_for_route = events_requests.clone();
    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move || {
                let valid_baseline_body = valid_baseline_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(valid_baseline_body))
                        .expect("baseline response")
                }
            }),
        )
        .route(
            "/api/admin/ha/events",
            get(move |Query(params): Query<std::collections::HashMap<String, String>>| {
                let invalid_events_body = invalid_events_body.clone();
                let valid_events_body = valid_events_body.clone();
                let empty_events_body = empty_events_body.clone();
                let events_requests = events_requests_for_route.clone();
                async move {
                    if params.get("channel").map(String::as_str) != Some("control") {
                        return Response::builder()
                            .header("content-encoding", "zstd")
                            .body(Body::from(empty_events_body))
                            .expect("events response");
                    }
                    let attempt = events_requests.fetch_add(1, Ordering::SeqCst);
                    let body = if attempt == 0 {
                        invalid_events_body
                    } else {
                        valid_events_body
                    };
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(body))
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

    let db_path = temp_db_path("ha-invalid-events-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-invalid-events-recovery".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "standby-invalid-events".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha,
        dev_open_admin: true,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });
    let source_url = format!("http://{source_addr}");
    let client = Client::new();

    for channel in [
        tavily_hikari::HaSyncChannel::Billing,
        tavily_hikari::HaSyncChannel::Runtime,
    ] {
        proxy
            .persist_ha_sync_watermark(
                &format!("standby_{}_baseline_applied", channel.as_str()),
                Some(&source_url),
                Some("standby-invalid-events"),
                1,
                Some("seeded for control-only test"),
            )
            .await
            .expect("seed baseline marker");
    }

    let first_err = run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect_err("first sync should fail");
    let first_err_text = first_err.to_string();
    assert!(
        first_err_text.contains("invalid HA events NDJSON")
            || first_err_text.contains("unsupported HA events"),
        "unexpected first sync error: {first_err_text}"
    );

    run_ha_standby_sync_once(&state, &client, &source_url, "test-token")
        .await
        .expect("second sync should recover");

    assert_eq!(events_requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_applied_seq")
            .await
            .expect("read applied seq"),
        Some(2)
    );

    let pool = connect_sqlite_test_pool(&db_str).await;
    let username: String =
        sqlx::query_scalar("SELECT username FROM users WHERE id = 'user-ha-events-retry'")
            .fetch_one(&pool)
            .await
            .expect("user inserted after event recovery");
    assert_eq!(username, "events_retry_user_2");

    sqlx::query(
        r#"
        INSERT INTO user_tag_bindings (user_id, tag, created_at)
        VALUES ('missing-user', 'broken-tag', 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect_err("foreign key should still reject invalid binding");

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
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
        .json(&serde_json::json!({"channel":"control","peerNodeId":"standby-a","ackedSeq":42}))
        .send()
        .await
        .expect("ack request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let acked: i64 =
        sqlx::query_scalar("SELECT acked_seq FROM ha_peer_watermarks WHERE peer_node_id = 'standby-a' AND channel = 'control'")
            .fetch_one(&pool)
            .await
            .expect("fetch peer watermark");
    assert_eq!(acked, 42);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_sync_watermark_reads_pending_overlay_before_flush() {
    let db_path = temp_db_path("ha-sync-watermark-pending-overlay");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-sync-watermark-pending-overlay".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    proxy
        .persist_ha_sync_watermark(
            "standby_control_applied_seq",
            Some("source-a"),
            Some("target-a"),
            42,
            Some("pending overlay"),
        )
        .await
        .expect("enqueue watermark");

    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_applied_seq")
            .await
            .expect("read pending watermark"),
        Some(42)
    );

    proxy
        .flush_ha_state_writes()
        .await
        .expect("flush pending watermark");

    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_control_applied_seq")
            .await
            .expect("read flushed watermark"),
        Some(42)
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_channel_outbox_stats_reports_count_age_and_ack_lag() {
    let db_path = temp_db_path("ha-outbox-stats");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-outbox-stats".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let now = Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('upsert', 'meta', 'request_rate_limit_v1', 'upsert', '{}', ?, 'checksum-a')
        "#,
    )
    .bind(now - 120)
    .execute(&pool)
    .await
    .expect("insert first outbox row");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('upsert', 'meta', 'global_ip_limit_v1', 'upsert', '{}', ?, 'checksum-b')
        "#,
    )
    .bind(now - 30)
    .execute(&pool)
    .await
    .expect("insert second outbox row");
    proxy
        .ack_ha_peer_watermark(tavily_hikari::HaSyncChannel::Control, "standby-a", 1)
        .await
        .expect("ack watermark");

    let stats = proxy
        .ha_channel_outbox_stats(tavily_hikari::HaSyncChannel::Control, Some("standby-a"))
        .await
        .expect("read outbox stats");
    assert_eq!(stats.row_count, 2);
    assert!(stats.oldest_age_secs >= 100);
    assert_eq!(stats.ack_lag, 1);

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
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
        {"schemaVersion":2,"kind":"events_start","channel":"control","after":0,"limit":10},
        {
            "schemaVersion":2,
            "kind":"event",
            "channel":"control",
            "event":{
                "seq":1,
                "channel":"control",
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
            "schemaVersion":2,
            "kind":"event",
            "channel":"control",
            "event":{
                "seq":2,
                "channel":"control",
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
            "schemaVersion":2,
            "kind":"event",
            "channel":"control",
            "event":{
                "seq":3,
                "channel":"control",
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
        {"schemaVersion":2,"kind":"events_end","channel":"control","lastSeq":3,"eventCount":3}
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
        .apply_ha_events_ndjson(tavily_hikari::HaSyncChannel::Control, &ndjson)
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
async fn ha_apply_ndjson_wrappers_abort_failed_sessions_before_retry() {
    let db_path = temp_db_path("ha-apply-wrapper-abort");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-apply-wrapper-abort".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let invalid_baseline = "{\"kind\":\"baseline_start\"}\nnot-json\n";
    let baseline_err = proxy
        .apply_ha_baseline_ndjson(tavily_hikari::HaSyncChannel::Control, invalid_baseline)
        .await
        .expect_err("invalid baseline should fail");
    assert!(
        baseline_err.to_string().contains("invalid HA baseline NDJSON"),
        "unexpected baseline error: {baseline_err}"
    );

    let valid_baseline = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "control",
            "nodeId": "retry-node",
            "generatedAt": 1,
            "highWatermark": 1,
            "encoding": "zstd-ndjson"
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "control",
            "resource": "meta",
            "op": "upsert",
            "data": {
                "key": "request_rate_limit_v1",
                "value": "55"
            }
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "control",
            "nodeId": "retry-node",
            "highWatermark": 1,
            "rowCount": 1
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let baseline_result = proxy
        .apply_ha_baseline_ndjson(tavily_hikari::HaSyncChannel::Control, &valid_baseline)
        .await
        .expect("valid baseline should recover after failure");
    assert_eq!(baseline_result.high_watermark, 1);

    let invalid_events = "{\"kind\":\"events_start\"}\nnot-json\n";
    let events_err = proxy
        .apply_ha_events_ndjson(tavily_hikari::HaSyncChannel::Control, invalid_events)
        .await
        .expect_err("invalid events should fail");
    assert!(
        events_err.to_string().contains("invalid HA events NDJSON"),
        "unexpected events error: {events_err}"
    );

    let truncated_events = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "afterSeq": 0
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "control",
            "event": {
                "channel": "control",
                "seq": 2,
                "kind": "state",
                "resource": "meta",
                "resourceId": "request_rate_limit_v1",
                "op": "upsert",
                "payload": {
                    "key": "request_rate_limit_v1",
                    "value": "66"
                },
                "createdAt": 2,
                "checksum": null
            }
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let truncated_err = proxy
        .apply_ha_events_ndjson(tavily_hikari::HaSyncChannel::Control, &truncated_events)
        .await
        .expect_err("truncated events should fail");
    assert!(
        truncated_err
            .to_string()
            .contains("HA events must include events_start and events_end"),
        "unexpected truncated events error: {truncated_err}"
    );

    let valid_events = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "afterSeq": 0
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "control",
            "event": {
                "channel": "control",
                "seq": 2,
                "kind": "state",
                "resource": "meta",
                "resourceId": "request_rate_limit_v1",
                "op": "upsert",
                "payload": {
                    "key": "request_rate_limit_v1",
                    "value": "77"
                },
                "createdAt": 2,
                "checksum": null
            }
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "control",
            "lastSeq": 2,
            "eventCount": 1
        }),
    ]
    .into_iter()
    .map(|value| value.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    let events_result = proxy
        .apply_ha_events_ndjson(tavily_hikari::HaSyncChannel::Control, &valid_events)
        .await
        .expect("valid events should recover after failure");
    assert_eq!(events_result.high_watermark, 2);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let request_rate_limit: Option<String> =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = 'request_rate_limit_v1'")
            .fetch_optional(&pool)
            .await
            .expect("read synced meta");
    assert_eq!(request_rate_limit.as_deref(), Some("77"));
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

    let db_path = temp_db_path("ha-startup-role-check-failure");
    let db_str = db_path.to_string_lossy().to_string();
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

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-alerts-ha-startup-role".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .unwrap();
    let status = reconcile_ha_startup_role(
        &proxy,
        &ha,
        Some(tavily_hikari::HaNodeRole::FullMaster),
    )
    .await;
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
        row.1.contains("\"OriginType\":\"IP_DOMAIN\""),
        "request audit should contain provider-compatible origin type: {}",
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
async fn ha_finalize_is_rejected_in_dual_active_mode() {
    let db_path = temp_db_path("ha-dual-active-finalize-rejected");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-finalize-rejected".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-dual-finalize".to_string(),
        database_path: Some(db_str.clone()),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("eo-group-dual-finalize".to_string()),
        core_dual_active: true,
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: "http://127.0.0.1:9".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .post(format!("http://{addr}/api/admin/ha/finalize"))
        .send()
        .await
        .expect("finalize response");
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    let body = response.text().await.expect("finalize body");
    assert!(
        body.contains("dual-active mode"),
        "dual-active finalize should be rejected: {body}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dual_active_promote_rejects_non_force_and_reachable_full_writer_peer() {
    let peer_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind peer listener");
    let peer_addr = peer_listener.local_addr().expect("peer addr");
    let now = Utc::now().timestamp();
    tokio::spawn(async move {
        let app = Router::new().route(
            "/api/internal/ha/status",
            get(move || async move {
                Json(json!({
                    "mode": "active_standby",
                    "nodeId": "node-b",
                    "nodePublicOrigin": "node-b-public:443",
                    "role": "full_master",
                    "dualActiveEnabled": true,
                    "fullMasterNodeId": "node-b",
                    "degraded": false,
                    "allowsBasicBusiness": true,
                    "allowsFullWrites": true,
                    "edgeoneDomain": "hikari.example.test",
                    "edgeoneOrigin": "og-core",
                    "edgeoneExpectedOrigin": null,
                    "edgeoneCurrentTarget": "og-core",
                    "edgeoneExpectedTarget": "og-core",
                    "edgeoneCurrentSourceKind": "origin_group",
                    "edgeoneExpectedSourceKind": "origin_group",
                    "edgeoneCurrentOriginGroupId": "og-core",
                    "edgeoneExpectedOriginGroupId": "og-core",
                    "haSourceDefaults": null,
                    "haSourceOverride": null,
                    "haSourceEffective": {
                        "sourceKind": "origin_group",
                        "directOriginScheme": null,
                        "directOriginHost": null,
                        "directOriginPort": null,
                        "originGroupId": "og-core",
                        "target": "og-core"
                    },
                    "edgeoneApiConfigured": true,
                    "lastEdgeoneCheckAt": now,
                    "lastSyncAt": now,
                    "syncLagSeconds": 0,
                    "recoveryStatus": null,
                    "message": null,
                    "peerNodes": [],
                    "plannedCutoverEligible": false
                }))
            }),
        );
        axum::serve(peer_listener, app.into_make_service())
            .await
            .expect("serve peer");
    });

    let db_path = temp_db_path("ha-dual-active-promote-split-brain");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-promote-split-brain".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        database_path: Some(db_str.clone()),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
        internal_token: Some("test-token".to_string()),
        peer_nodes: vec![tavily_hikari::HaPeerNodeConfig {
            node_id: "node-b".to_string(),
            admin_base_url: format!("http://{peer_addr}"),
            public_origin: "node-b-public:443".to_string(),
            role_hint: tavily_hikari::HaPeerRoleHint::StandbyCandidate,
        }],
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: "http://127.0.0.1:9".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    ha.apply_dual_active_leader(Some("node-b".to_string()))
        .await
        .expect("apply peer leader");
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let client = Client::new();

    let non_force = client
        .post(format!("http://{addr}/api/admin/ha/promote"))
        .json(&json!({}))
        .send()
        .await
        .expect("non-force promote response");
    assert_eq!(non_force.status(), reqwest::StatusCode::CONFLICT);
    let non_force_body = non_force.text().await.expect("non-force body");
    assert!(
        non_force_body.contains("requires force=true"),
        "unexpected non-force promote body: {non_force_body}"
    );

    let forced = client
        .post(format!("http://{addr}/api/admin/ha/promote"))
        .json(&json!({ "force": true }))
        .send()
        .await
        .expect("force promote response");
    assert_eq!(forced.status(), reqwest::StatusCode::CONFLICT);
    let forced_body = forced.text().await.expect("force body");
    assert!(
        forced_body.contains("still allows full writes"),
        "unexpected force promote body: {forced_body}"
    );

    let status_response = client
        .get(format!("http://{addr}/api/admin/ha/status"))
        .send()
        .await
        .expect("ha status response");
    assert_eq!(status_response.status(), reqwest::StatusCode::OK);
    let status: Value = status_response.json().await.expect("ha status body");
    assert_eq!(status["role"], "standby");
    assert_eq!(status["fullMasterNodeId"], "node-b");
    assert_eq!(status["allowsFullWrites"], false);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dual_active_planned_cutover_keeps_local_full_master_when_peer_update_fails() {
    let peer_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind peer listener");
    let peer_addr = peer_listener.local_addr().expect("peer addr");
    let now = Utc::now().timestamp();
    tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/api/internal/ha/status",
                get(move || async move {
                    Json(json!({
                        "mode": "active_standby",
                        "nodeId": "node-b",
                        "nodePublicOrigin": "node-b-public:443",
                        "role": "standby",
                        "dualActiveEnabled": true,
                        "fullMasterNodeId": "node-a",
                        "degraded": false,
                        "allowsBasicBusiness": true,
                        "allowsFullWrites": false,
                        "edgeoneDomain": "hikari.example.test",
                        "edgeoneOrigin": "og-core",
                        "edgeoneExpectedOrigin": null,
                        "edgeoneCurrentTarget": "og-core",
                        "edgeoneExpectedTarget": "og-core",
                        "edgeoneCurrentSourceKind": "origin_group",
                        "edgeoneExpectedSourceKind": "origin_group",
                        "edgeoneCurrentOriginGroupId": "og-core",
                        "edgeoneExpectedOriginGroupId": "og-core",
                        "haSourceDefaults": null,
                        "haSourceOverride": null,
                        "haSourceEffective": {
                            "sourceKind": "origin_group",
                            "directOriginScheme": null,
                            "directOriginHost": null,
                            "directOriginPort": null,
                            "originGroupId": "og-core",
                            "target": "og-core"
                        },
                        "edgeoneApiConfigured": true,
                        "lastEdgeoneCheckAt": now,
                        "lastSyncAt": now,
                        "syncLagSeconds": 0,
                        "recoveryStatus": null,
                        "message": "peer ready",
                        "peerNodes": [],
                        "plannedCutoverEligible": true
                    }))
                }),
            )
            .route(
                "/api/internal/ha/leader",
                post(|| async {
                    (
                        StatusCode::BAD_GATEWAY,
                        Body::from("simulated peer leader update failure"),
                    )
                }),
            );
        axum::serve(peer_listener, app.into_make_service())
            .await
            .expect("serve peer");
    });

    let db_path = temp_db_path("ha-dual-active-cutover-peer-failure");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-cutover-peer-failure".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_ha_full_master_node_id("node-a")
        .await
        .expect("seed local leader");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        database_path: Some(db_str.clone()),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
        internal_token: Some("peer-token".to_string()),
        peer_nodes: vec![tavily_hikari::HaPeerNodeConfig {
            node_id: "node-b".to_string(),
            admin_base_url: format!("http://{peer_addr}"),
            public_origin: "node-b-public:443".to_string(),
            role_hint: tavily_hikari::HaPeerRoleHint::StandbyCandidate,
        }],
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: "http://127.0.0.1:9".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    ha.apply_dual_active_leader(Some("node-a".to_string()))
        .await
        .expect("apply local leader");
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let client = Client::new();
    let response = client
        .post(format!("http://{addr}/api/admin/ha/planned-cutover"))
        .json(&json!({
            "targetNodeId": "node-b",
        }))
        .send()
        .await
        .expect("planned cutover response");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    let body = response.text().await.expect("planned cutover body");
    assert!(
        body.contains("dual-active peer leader update failed"),
        "unexpected planned cutover failure body: {body}"
    );

    let status_response = client
        .get(format!("http://{addr}/api/admin/ha/status"))
        .send()
        .await
        .expect("ha status response");
    assert_eq!(status_response.status(), reqwest::StatusCode::OK);
    let status: Value = status_response.json().await.expect("ha status body");
    assert_eq!(status["role"], "full_master");
    assert_eq!(status["fullMasterNodeId"], "node-a");
    assert_eq!(status["allowsFullWrites"], true);

    let leader_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-cutover-peer-failure".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("leader proxy created");
    assert_eq!(
        leader_proxy
            .get_ha_full_master_node_id()
            .await
            .expect("leader key lookup"),
        Some("node-a".to_string())
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dual_active_planned_cutover_fences_local_writer_before_peer_promotion() {
    let local_status_base = std::sync::Arc::new(tokio::sync::Mutex::new(None::<String>));
    let observed_local_full_writes = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let peer_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind peer listener");
    let peer_addr = peer_listener.local_addr().expect("peer addr");
    let now = Utc::now().timestamp();
    let local_status_base_for_peer = local_status_base.clone();
    let observed_local_full_writes_for_peer = observed_local_full_writes.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/api/internal/ha/status",
                get(move || async move {
                    Json(json!({
                        "mode": "active_standby",
                        "nodeId": "node-b",
                        "nodePublicOrigin": "node-b-public:443",
                        "role": "standby",
                        "dualActiveEnabled": true,
                        "fullMasterNodeId": "node-a",
                        "degraded": false,
                        "allowsBasicBusiness": true,
                        "allowsFullWrites": false,
                        "edgeoneDomain": "hikari.example.test",
                        "edgeoneOrigin": "og-core",
                        "edgeoneExpectedOrigin": null,
                        "edgeoneCurrentTarget": "og-core",
                        "edgeoneExpectedTarget": "og-core",
                        "edgeoneCurrentSourceKind": "origin_group",
                        "edgeoneExpectedSourceKind": "origin_group",
                        "edgeoneCurrentOriginGroupId": "og-core",
                        "edgeoneExpectedOriginGroupId": "og-core",
                        "haSourceDefaults": null,
                        "haSourceOverride": null,
                        "haSourceEffective": {
                            "sourceKind": "origin_group",
                            "directOriginScheme": null,
                            "directOriginHost": null,
                            "directOriginPort": null,
                            "originGroupId": "og-core",
                            "target": "og-core"
                        },
                        "edgeoneApiConfigured": true,
                        "lastEdgeoneCheckAt": now,
                        "lastSyncAt": now,
                        "syncLagSeconds": 0,
                        "recoveryStatus": null,
                        "message": "peer ready",
                        "peerNodes": [],
                        "plannedCutoverEligible": true
                    }))
                }),
            )
            .route(
                "/api/internal/ha/leader",
                post(move || {
                    let local_status_base_for_peer = local_status_base_for_peer.clone();
                    let observed_local_full_writes_for_peer =
                        observed_local_full_writes_for_peer.clone();
                    async move {
                        let local_status_base = loop {
                            let current = local_status_base_for_peer.lock().await.clone();
                            if let Some(base) = current {
                                break base;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        };
                        let status: Value = Client::new()
                            .get(format!("{local_status_base}/api/admin/ha/status"))
                            .send()
                            .await
                            .expect("fetch local ha status during peer promotion")
                            .json()
                            .await
                            .expect("decode local ha status during peer promotion");
                        observed_local_full_writes_for_peer.store(
                            status["allowsFullWrites"].as_bool().unwrap_or(true),
                            std::sync::atomic::Ordering::SeqCst,
                        );
                        Json(json!({
                            "mode": "active_standby",
                            "nodeId": "node-b",
                            "nodePublicOrigin": "node-b-public:443",
                            "role": "full_master",
                            "dualActiveEnabled": true,
                            "fullMasterNodeId": "node-b",
                            "degraded": false,
                            "allowsBasicBusiness": true,
                            "allowsFullWrites": true,
                            "edgeoneDomain": "hikari.example.test",
                            "edgeoneOrigin": "og-core",
                            "edgeoneExpectedOrigin": null,
                            "edgeoneCurrentTarget": "og-core",
                            "edgeoneExpectedTarget": "og-core",
                            "edgeoneCurrentSourceKind": "origin_group",
                            "edgeoneExpectedSourceKind": "origin_group",
                            "edgeoneCurrentOriginGroupId": "og-core",
                            "edgeoneExpectedOriginGroupId": "og-core",
                            "haSourceDefaults": null,
                            "haSourceOverride": null,
                            "haSourceEffective": {
                                "sourceKind": "origin_group",
                                "directOriginScheme": null,
                                "directOriginHost": null,
                                "directOriginPort": null,
                                "originGroupId": "og-core",
                                "target": "og-core"
                            },
                            "edgeoneApiConfigured": true,
                            "lastEdgeoneCheckAt": now,
                            "lastSyncAt": now,
                            "syncLagSeconds": 0,
                            "recoveryStatus": null,
                            "message": "peer promoted",
                            "peerNodes": [],
                            "plannedCutoverEligible": true
                        }))
                    }
                }),
            );
        axum::serve(peer_listener, app.into_make_service())
            .await
            .expect("serve peer");
    });

    let db_path = temp_db_path("ha-dual-active-cutover-local-fence");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-cutover-local-fence".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_ha_full_master_node_id("node-a")
        .await
        .expect("seed local leader");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        database_path: Some(db_str.clone()),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
        internal_token: Some("peer-token".to_string()),
        peer_nodes: vec![tavily_hikari::HaPeerNodeConfig {
            node_id: "node-b".to_string(),
            admin_base_url: format!("http://{peer_addr}"),
            public_origin: "node-b-public:443".to_string(),
            role_hint: tavily_hikari::HaPeerRoleHint::StandbyCandidate,
        }],
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: "http://127.0.0.1:9".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    ha.apply_dual_active_leader(Some("node-a".to_string()))
        .await
        .expect("apply local leader");
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    *local_status_base.lock().await = Some(format!("http://{addr}"));

    let client = Client::new();
    let response = client
        .post(format!("http://{addr}/api/admin/ha/planned-cutover"))
        .json(&json!({
            "targetNodeId": "node-b",
        }))
        .send()
        .await
        .expect("planned cutover response");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    assert!(
        !observed_local_full_writes.load(std::sync::atomic::Ordering::SeqCst),
        "local node should already be fenced when peer starts accepting full writes"
    );

    let status_response = client
        .get(format!("http://{addr}/api/admin/ha/status"))
        .send()
        .await
        .expect("ha status response");
    assert_eq!(status_response.status(), reqwest::StatusCode::OK);
    let status: Value = status_response.json().await.expect("ha status body");
    assert_eq!(status["role"], "standby");
    assert_eq!(status["fullMasterNodeId"], "node-b");
    assert_eq!(status["allowsFullWrites"], false);

    let leader_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-cutover-local-fence".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("leader proxy created");
    assert_eq!(
        leader_proxy
            .get_ha_full_master_node_id()
            .await
            .expect("leader key lookup"),
        Some("node-b".to_string())
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dual_active_planned_cutover_keeps_local_fenced_when_peer_response_is_ambiguous() {
    let peer_promoted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let peer_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind peer listener");
    let peer_addr = peer_listener.local_addr().expect("peer addr");
    let now = Utc::now().timestamp();
    let peer_promoted_for_status = peer_promoted.clone();
    let peer_promoted_for_leader = peer_promoted.clone();
    tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/api/internal/ha/status",
                get(move || {
                    let peer_promoted_for_status = peer_promoted_for_status.clone();
                    async move {
                        let promoted = peer_promoted_for_status
                            .load(std::sync::atomic::Ordering::SeqCst);
                        Json(json!({
                            "mode": "active_standby",
                            "nodeId": "node-b",
                            "nodePublicOrigin": "node-b-public:443",
                            "role": if promoted { "full_master" } else { "standby" },
                            "dualActiveEnabled": true,
                            "fullMasterNodeId": if promoted { "node-b" } else { "node-a" },
                            "degraded": false,
                            "allowsBasicBusiness": true,
                            "allowsFullWrites": promoted,
                            "edgeoneDomain": "hikari.example.test",
                            "edgeoneOrigin": "og-core",
                            "edgeoneExpectedOrigin": null,
                            "edgeoneCurrentTarget": "og-core",
                            "edgeoneExpectedTarget": "og-core",
                            "edgeoneCurrentSourceKind": "origin_group",
                            "edgeoneExpectedSourceKind": "origin_group",
                            "edgeoneCurrentOriginGroupId": "og-core",
                            "edgeoneExpectedOriginGroupId": "og-core",
                            "haSourceDefaults": null,
                            "haSourceOverride": null,
                            "haSourceEffective": {
                                "sourceKind": "origin_group",
                                "directOriginScheme": null,
                                "directOriginHost": null,
                                "directOriginPort": null,
                                "originGroupId": "og-core",
                                "target": "og-core"
                            },
                            "edgeoneApiConfigured": true,
                            "lastEdgeoneCheckAt": now,
                            "lastSyncAt": now,
                            "syncLagSeconds": 0,
                            "recoveryStatus": null,
                            "message": if promoted { "peer promoted" } else { "peer ready" },
                            "peerNodes": [],
                            "plannedCutoverEligible": true
                        }))
                    }
                }),
            )
            .route(
                "/api/internal/ha/leader",
                post(move || {
                    let peer_promoted_for_leader = peer_promoted_for_leader.clone();
                    async move {
                        peer_promoted_for_leader.store(true, std::sync::atomic::Ordering::SeqCst);
                        Response::builder()
                            .status(StatusCode::OK)
                            .body(Body::from("not-json"))
                            .expect("ambiguous peer leader response")
                    }
                }),
            );
        axum::serve(peer_listener, app.into_make_service())
            .await
            .expect("serve peer");
    });

    let db_path = temp_db_path("ha-dual-active-cutover-ambiguous-peer-response");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-cutover-ambiguous-peer-response".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_ha_full_master_node_id("node-a")
        .await
        .expect("seed local leader");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        database_path: Some(db_str.clone()),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
        internal_token: Some("peer-token".to_string()),
        peer_nodes: vec![tavily_hikari::HaPeerNodeConfig {
            node_id: "node-b".to_string(),
            admin_base_url: format!("http://{peer_addr}"),
            public_origin: "node-b-public:443".to_string(),
            role_hint: tavily_hikari::HaPeerRoleHint::StandbyCandidate,
        }],
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: "http://127.0.0.1:9".to_string(),
        ..tavily_hikari::HaConfig::default()
    });
    ha.apply_dual_active_leader(Some("node-a".to_string()))
        .await
        .expect("apply local leader");
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let client = Client::new();
    let response = client
        .post(format!("http://{addr}/api/admin/ha/planned-cutover"))
        .json(&json!({
            "targetNodeId": "node-b",
        }))
        .send()
        .await
        .expect("planned cutover response");
    assert_eq!(response.status(), reqwest::StatusCode::BAD_GATEWAY);
    let body = response.text().await.expect("planned cutover body");
    assert!(
        body.contains("local node remains fenced"),
        "unexpected planned cutover failure body: {body}"
    );

    let status_response = client
        .get(format!("http://{addr}/api/admin/ha/status"))
        .send()
        .await
        .expect("ha status response");
    assert_eq!(status_response.status(), reqwest::StatusCode::OK);
    let status: Value = status_response.json().await.expect("ha status body");
    assert_eq!(status["role"], "standby");
    assert_eq!(status["fullMasterNodeId"], "node-b");
    assert_eq!(status["allowsFullWrites"], false);

    let leader_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-cutover-ambiguous-peer-response".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("leader proxy created");
    assert_eq!(
        leader_proxy
            .get_ha_full_master_node_id()
            .await
            .expect("leader key lookup"),
        Some("node-b".to_string())
    );

    let _ = std::fs::remove_file(db_path);
}

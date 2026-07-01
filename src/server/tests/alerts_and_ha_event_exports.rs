use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

#[test]
fn dual_active_standby_can_export_billing_and_runtime_channels() {
    let status = tavily_hikari::HaStatusView {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-b".to_string(),
        node_public_origin: None,
        role: tavily_hikari::HaNodeRole::Standby,
        dual_active_enabled: true,
        full_master_node_id: None,
        degraded: false,
        allows_basic_business: false,
        allows_full_writes: false,
        edgeone_domain: None,
        edgeone_origin: Some("og-core".to_string()),
        edgeone_expected_origin: None,
        edgeone_current_target: Some("og-core".to_string()),
        edgeone_expected_target: Some("og-core".to_string()),
        edgeone_current_source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        edgeone_expected_source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        edgeone_current_origin_group_id: Some("og-core".to_string()),
        edgeone_expected_origin_group_id: Some("og-core".to_string()),
        ha_source_defaults: None,
        ha_source_override: None,
        ha_source_effective: None,
        edgeone_api_configured: true,
        last_edgeone_check_at: None,
        last_sync_at: None,
        sync_lag_seconds: None,
        recovery_status: None,
        message: Some("dual-active leader key is missing; serving stays fenced until seeded".to_string()),
        peer_nodes: Vec::new(),
        planned_cutover_eligible: false,
    };

    assert!(!ha_can_export_channel(
        &status,
        tavily_hikari::HaSyncChannel::Control
    ));
    assert!(ha_can_export_channel(
        &status,
        tavily_hikari::HaSyncChannel::Billing
    ));
    assert!(ha_can_export_channel(
        &status,
        tavily_hikari::HaSyncChannel::Runtime
    ));
}

#[test]
fn legacy_standby_cannot_export_billing_or_runtime_channels() {
    let status = tavily_hikari::HaStatusView {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-b".to_string(),
        node_public_origin: None,
        role: tavily_hikari::HaNodeRole::Standby,
        dual_active_enabled: false,
        full_master_node_id: None,
        degraded: false,
        allows_basic_business: false,
        allows_full_writes: false,
        edgeone_domain: None,
        edgeone_origin: Some("https://node-a.example".to_string()),
        edgeone_expected_origin: None,
        edgeone_current_target: Some("https://node-a.example".to_string()),
        edgeone_expected_target: Some("https://node-a.example".to_string()),
        edgeone_current_source_kind: Some(tavily_hikari::HaSourceKind::Direct),
        edgeone_expected_source_kind: Some(tavily_hikari::HaSourceKind::Direct),
        edgeone_current_origin_group_id: None,
        edgeone_expected_origin_group_id: None,
        ha_source_defaults: None,
        ha_source_override: None,
        ha_source_effective: None,
        edgeone_api_configured: true,
        last_edgeone_check_at: None,
        last_sync_at: None,
        sync_lag_seconds: None,
        recovery_status: None,
        message: Some("standby waiting for source sync".to_string()),
        peer_nodes: Vec::new(),
        planned_cutover_eligible: false,
    };

    assert!(!ha_can_export_channel(
        &status,
        tavily_hikari::HaSyncChannel::Billing
    ));
    assert!(!ha_can_export_channel(
        &status,
        tavily_hikari::HaSyncChannel::Runtime
    ));
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
        .get(format!(
            "http://{addr}/api/admin/ha/events?channel=control&after=0&limit=10"
        ))
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
    assert_eq!(
        response
            .headers()
            .get("x-ha-event-count")
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );
    assert_eq!(
        response
            .headers()
            .get("x-ha-last-seq")
            .and_then(|value| value.to_str().ok()),
        Some("1")
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

#[tokio::test]
async fn ha_events_endpoint_chunks_oversized_batches() {
    let db_path = temp_db_path("ha-events-chunked-batches");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-chunked-batches".to_string()],
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
        ) VALUES ('state', 'meta', ?, 'upsert', ?, ?, NULL)
        "#,
    )
    .bind("request_rate_limit_v1-1")
    .bind(
        serde_json::json!({
            "key": "request_rate_limit_v1",
            "value": format!("1-{}", nanoid!(512))
        })
        .to_string(),
    )
    .bind(now + 1)
    .execute(&pool)
    .await
    .expect("insert first outbox event");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-events-chunked".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let client = Client::new();

    let single = client
        .get(format!(
            "http://{addr}/api/admin/ha/events?channel=control&after=0&limit=4"
        ))
        .send()
        .await
        .expect("single events request");
    assert_eq!(single.status(), reqwest::StatusCode::OK);
    let single_compressed = single.bytes().await.expect("single events bytes");

    for seq in 2..=4 {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox (
                kind, resource, resource_id, op, payload_json, created_at, checksum
            ) VALUES ('state', 'meta', ?, 'upsert', ?, ?, NULL)
            "#,
        )
        .bind(format!("request_rate_limit_v1-{seq}"))
        .bind(
            serde_json::json!({
                "key": "request_rate_limit_v1",
                "value": format!("{}-{}", seq, nanoid!(512))
            })
            .to_string(),
        )
        .bind(now + i64::from(seq))
        .execute(&pool)
        .await
        .expect("insert outbox event");
    }
    pool.close().await;

    let _cap = EnvVarGuard::set(
        "TAVILY_TEST_HA_EVENTS_MAX_COMPRESSED_BYTES",
        &single_compressed.len().to_string(),
    );
    let response = client
        .get(format!(
            "http://{addr}/api/admin/ha/events?channel=control&after=0&limit=4"
        ))
        .send()
        .await
        .expect("chunked events request");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-ha-event-count")
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );
    assert_eq!(
        response
            .headers()
            .get("x-ha-last-seq")
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );
    let compressed = response.bytes().await.expect("chunked events bytes");
    assert!(
        compressed.len() <= single_compressed.len(),
        "chunked response should honor compressed cap"
    );
    let decoded = zstd::stream::decode_all(compressed.as_ref()).expect("decode chunked events");
    let text = String::from_utf8(decoded).expect("chunked events utf8");
    assert_eq!(text.matches("\"kind\":\"event\"").count(), 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_events_endpoint_returns_413_when_single_event_exceeds_cap() {
    let _cap = EnvVarGuard::set("TAVILY_TEST_HA_EVENTS_MAX_COMPRESSED_BYTES", "128");
    let db_path = temp_db_path("ha-events-single-over-cap");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-single-over-cap".to_string()],
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
        ) VALUES ('state', 'meta', 'request_rate_limit_v1', 'upsert', ?, ?, NULL)
        "#,
    )
    .bind(
        serde_json::json!({
            "key": "request_rate_limit_v1",
            "value": nanoid!(4096)
        })
        .to_string(),
    )
    .bind(Utc::now().timestamp())
    .execute(&pool)
    .await
    .expect("insert oversized outbox event");
    pool.close().await;
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-events-over-cap".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let response = Client::new()
        .get(format!(
            "http://{addr}/api/admin/ha/events?channel=control&after=0&limit=1"
        ))
        .send()
        .await
        .expect("oversized events request");
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let body = response.text().await.expect("oversized events body");
    assert!(
        body.contains("HA events payload exceeds compressed limit"),
        "unexpected events error body: {body}"
    );

    let _ = std::fs::remove_file(db_path);
}

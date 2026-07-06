use super::*;
use super::core_support_and_parsing::*;

#[tokio::test]
async fn dual_active_peer_sync_does_not_mark_success_when_no_peer_is_reached() {
    let db_path = temp_db_path("ha-dual-active-no-peer-reached");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-no-peer-reached".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let closed_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let unreachable_addr = closed_listener.local_addr().unwrap();
    drop(closed_listener);

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
        peer_nodes: vec![tavily_hikari::HaPeerNodeConfig {
            node_id: "node-b".to_string(),
            admin_base_url: format!("http://{unreachable_addr}"),
            public_origin: "node-b:8787".to_string(),
            role_hint: tavily_hikari::HaPeerRoleHint::StandbyCandidate,
        }],
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
    let client = Client::builder()
        .timeout(Duration::from_millis(200))
        .build()
        .expect("client");

    let err = run_ha_peer_sync_once(&state, &client, "test-token")
        .await
        .expect_err("peer sync should fail when no peer can be reached");
    assert!(
        err.to_string().contains("reached no peers"),
        "unexpected peer sync error: {err}"
    );
    let status = state.ha.status().await;
    assert_eq!(
        status.last_sync_at, None,
        "failed peer sync must not refresh last_sync_at"
    );
    assert_eq!(
        status.sync_lag_seconds, None,
        "failed peer sync must not produce a fresh lag"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_standby_sync_resets_runtime_baseline_after_foreign_key_gap() {
    let control_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "control",
            "nodeId": "active-fk-gap",
            "highWatermark": 0
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "control",
            "nodeId": "active-fk-gap",
            "highWatermark": 0,
            "rowCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let billing_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "billing",
            "nodeId": "active-fk-gap",
            "highWatermark": 0
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "billing",
            "nodeId": "active-fk-gap",
            "highWatermark": 0,
            "rowCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let runtime_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "runtime",
            "nodeId": "active-fk-gap",
            "highWatermark": 0
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "runtime",
            "resource": "mcp_sessions",
            "op": "upsert",
            "data": {
                "proxy_session_id": "sess-fk-gap",
                "upstream_session_id": "upstream-fk-gap",
                "upstream_key_id": null,
                "auth_token_id": null,
                "user_id": null,
                "protocol_version": "2025-03-26",
                "last_event_id": null,
                "gateway_mode": "upstream_mcp",
                "experiment_variant": "control",
                "ab_bucket": null,
                "routing_subject_hash": null,
                "fallback_reason": null,
                "rate_limited_until": null,
                "last_rate_limited_at": null,
                "last_rate_limit_reason": null,
                "created_at": 1,
                "updated_at": 1,
                "expires_at": 3600,
                "revoked_at": null,
                "revoke_reason": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "runtime",
            "nodeId": "active-fk-gap",
            "highWatermark": 0,
            "rowCount": 1
        })
        .to_string(),
    ]
    .join("\n");

    let empty_events_for = |channel: &str| {
        [
            serde_json::json!({
                "schemaVersion": 2,
                "kind": "events_start",
                "channel": channel,
                "after": 0,
                "limit": 1000
            })
            .to_string(),
            serde_json::json!({
                "schemaVersion": 2,
                "kind": "events_end",
                "channel": channel,
                "lastSeq": 0,
                "eventCount": 0
            })
            .to_string(),
        ]
        .join("\n")
    };

    let runtime_fk_gap_events = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "runtime",
            "after": 0,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "runtime",
            "event": {
                "seq": 1,
                "channel": "runtime",
                "kind": "state",
                "resource": "mcp_sessions",
                "resourceId": "sess-fk-gap",
                "op": "upsert",
                "payload": {
                    "proxy_session_id": "sess-fk-gap",
                    "upstream_session_id": "upstream-fk-gap",
                    "upstream_key_id": "missing-key",
                    "auth_token_id": null,
                    "user_id": null,
                    "protocol_version": "2025-03-26",
                    "last_event_id": null,
                    "gateway_mode": "upstream_mcp",
                    "experiment_variant": "control",
                    "ab_bucket": null,
                    "routing_subject_hash": null,
                    "fallback_reason": null,
                    "rate_limited_until": null,
                    "last_rate_limited_at": null,
                    "last_rate_limit_reason": null,
                    "created_at": 1,
                    "updated_at": 1,
                    "expires_at": 3600,
                    "revoked_at": null,
                    "revoke_reason": null
                },
                "createdAt": 1,
                "checksum": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "runtime",
            "lastSeq": 1,
            "eventCount": 1
        })
        .to_string(),
    ]
    .join("\n");

    let control_baseline_body = zstd::stream::encode_all(control_baseline_ndjson.as_bytes(), 0)
        .expect("encode control baseline");
    let billing_baseline_body = zstd::stream::encode_all(billing_baseline_ndjson.as_bytes(), 0)
        .expect("encode billing baseline");
    let runtime_baseline_body = zstd::stream::encode_all(runtime_baseline_ndjson.as_bytes(), 0)
        .expect("encode runtime baseline");
    let control_events_body = zstd::stream::encode_all(empty_events_for("control").as_bytes(), 0)
        .expect("encode control events");
    let billing_events_body = zstd::stream::encode_all(empty_events_for("billing").as_bytes(), 0)
        .expect("encode billing events");
    let runtime_events_body = zstd::stream::encode_all(runtime_fk_gap_events.as_bytes(), 0)
        .expect("encode runtime events");

    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move |Query(params): Query<std::collections::HashMap<String, String>>| {
                let control_baseline_body = control_baseline_body.clone();
                let billing_baseline_body = billing_baseline_body.clone();
                let runtime_baseline_body = runtime_baseline_body.clone();
                async move {
                    let body = match params.get("channel").map(String::as_str) {
                        Some("control") => control_baseline_body,
                        Some("billing") => billing_baseline_body,
                        Some("runtime") => runtime_baseline_body,
                        other => panic!("unexpected baseline channel: {other:?}"),
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
            get(move |Query(params): Query<std::collections::HashMap<String, String>>| {
                let control_events_body = control_events_body.clone();
                let billing_events_body = billing_events_body.clone();
                let runtime_events_body = runtime_events_body.clone();
                async move {
                    let body = match params.get("channel").map(String::as_str) {
                        Some("control") => control_events_body,
                        Some("billing") => billing_events_body,
                        Some("runtime") => runtime_events_body,
                        other => panic!("unexpected events channel: {other:?}"),
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

    let db_path = temp_db_path("ha-runtime-fk-gap");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-runtime-fk-gap".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "standby-runtime-fk-gap".to_string(),
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
        .expect("sync should recover by requiring runtime baseline reset");

    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_runtime_baseline_applied")
            .await
            .expect("read runtime baseline marker"),
        Some(0)
    );
    assert_eq!(
        proxy
            .get_ha_sync_watermark("standby_runtime_applied_seq")
            .await
            .expect("read runtime seq"),
        Some(0)
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn dual_active_peer_runtime_baseline_does_not_delete_local_rows() {
    let runtime_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "runtime",
            "nodeId": "peer-empty-runtime",
            "highWatermark": 0
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "runtime",
            "nodeId": "peer-empty-runtime",
            "highWatermark": 0,
            "rowCount": 0
        })
        .to_string(),
    ]
    .join("\n");
    let runtime_events_body = zstd::stream::encode_all(
        [
            serde_json::json!({
                "schemaVersion": 2,
                "kind": "events_start",
                "channel": "runtime",
                "after": 0,
                "limit": 1000
            })
            .to_string(),
            serde_json::json!({
                "schemaVersion": 2,
                "kind": "events_end",
                "channel": "runtime",
                "lastSeq": 0,
                "eventCount": 0
            })
            .to_string(),
        ]
        .join("\n")
        .as_bytes(),
        0,
    )
    .expect("encode runtime events");
    let runtime_baseline_body = zstd::stream::encode_all(runtime_baseline_ndjson.as_bytes(), 0)
        .expect("encode runtime baseline");

    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move |_query: Query<std::collections::HashMap<String, String>>| {
                let runtime_baseline_body = runtime_baseline_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(runtime_baseline_body))
                        .expect("runtime baseline response")
                }
            }),
        )
        .route(
            "/api/admin/ha/events",
            get(move |_query: Query<std::collections::HashMap<String, String>>| {
                let runtime_events_body = runtime_events_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(runtime_events_body))
                        .expect("runtime events response")
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

    let db_path = temp_db_path("ha-dual-active-runtime-baseline-upsert");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-runtime-baseline-upsert".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;

    sqlx::query(
        r#"
        INSERT INTO mcp_sessions (
            proxy_session_id,
            upstream_session_id,
            upstream_key_id,
            auth_token_id,
            user_id,
            protocol_version,
            last_event_id,
            gateway_mode,
            experiment_variant,
            ab_bucket,
            routing_subject_hash,
            fallback_reason,
            rate_limited_until,
            last_rate_limited_at,
            last_rate_limit_reason,
            created_at,
            updated_at,
            expires_at,
            revoked_at,
            revoke_reason
        ) VALUES (
            'sess-local-dual-active',
            'upstream-local-dual-active',
            NULL,
            NULL,
            NULL,
            '2025-03-26',
            NULL,
            'upstream_mcp',
            'control',
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            1,
            1,
            86400,
            NULL,
            NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert local runtime session");

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
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

    let client = Client::new();
    run_ha_sync_once_for_peer(
        &state,
        &client,
        &format!("http://{source_addr}"),
        "node-b",
        "test-token",
        &[tavily_hikari::HaSyncChannel::Runtime],
    )
    .await
    .expect("dual-active peer sync should preserve local runtime rows");

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mcp_sessions WHERE proxy_session_id = 'sess-local-dual-active'",
    )
    .fetch_one(&pool)
    .await
    .expect("count preserved dual-active runtime row");
    assert_eq!(row_count, 1);

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn dual_active_peer_billing_sync_namespaces_peer_log_ids() {
    let billing_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "billing",
            "nodeId": "peer-billing",
            "highWatermark": 1
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "billing",
            "resource": "billing_ledger",
            "op": "upsert",
            "data": {
                "auth_token_log_id": 1,
                "token_id": "tok-peer-billing",
                "billing_subject": "token:tok-peer-billing",
                "billing_state": "pending",
                "business_credits": 7,
                "request_user_id": null,
                "api_key_id": "key-peer-billing",
                "request_log_id": 88,
                "result_status": "success",
                "created_at": 10,
                "updated_at": 10,
                "settled_at": null,
                "error_message": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "billing",
            "nodeId": "peer-billing",
            "highWatermark": 1,
            "rowCount": 1
        })
        .to_string(),
    ]
    .join("\n");
    let billing_events_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "billing",
            "after": 1,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "billing",
            "event": {
                "channel": "billing",
                "seq": 2,
                "kind": "state",
                "resource": "billing_ledger",
                "resourceId": "1",
                "op": "upsert",
                "payload": {
                    "auth_token_log_id": 1,
                    "token_id": "tok-peer-billing",
                    "billing_subject": "token:tok-peer-billing",
                    "billing_state": "charged",
                    "business_credits": 8,
                    "request_user_id": null,
                    "api_key_id": "key-peer-billing",
                    "request_log_id": 89,
                    "result_status": "success",
                    "created_at": 10,
                    "updated_at": 20,
                    "settled_at": 20,
                    "error_message": null
                },
                "createdAt": 20,
                "checksum": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "billing",
            "lastSeq": 2,
            "eventCount": 1
        })
        .to_string(),
    ]
    .join("\n");
    let billing_baseline_body = zstd::stream::encode_all(billing_baseline_ndjson.as_bytes(), 0)
        .expect("encode billing baseline");
    let billing_events_body = zstd::stream::encode_all(billing_events_ndjson.as_bytes(), 0)
        .expect("encode billing events");

    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move |_query: Query<std::collections::HashMap<String, String>>| {
                let billing_baseline_body = billing_baseline_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(billing_baseline_body))
                        .expect("billing baseline response")
                }
            }),
        )
        .route(
            "/api/admin/ha/events",
            get(move |_query: Query<std::collections::HashMap<String, String>>| {
                let billing_events_body = billing_events_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(billing_events_body))
                        .expect("billing events response")
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

    let db_path = temp_db_path("ha-dual-active-billing-peer-id-namespace");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-billing-peer-id-namespace".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO billing_ledger (
            auth_token_log_id,
            token_id,
            billing_subject,
            billing_state,
            business_credits,
            request_user_id,
            api_key_id,
            request_log_id,
            result_status,
            created_at,
            updated_at,
            settled_at,
            error_message
        ) VALUES (
            1,
            'tok-local-billing',
            'token:tok-local-billing',
            'charged',
            3,
            NULL,
            'key-local-billing',
            NULL,
            'success',
            1,
            1,
            1,
            NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert local billing row");

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
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

    let client = Client::new();
    run_ha_sync_once_for_peer(
        &state,
        &client,
        &format!("http://{source_addr}"),
        "node-b",
        "test-token",
        &[tavily_hikari::HaSyncChannel::Billing],
    )
    .await
    .expect("dual-active peer billing sync should namespace peer ids");

    let local_token: String =
        sqlx::query_scalar("SELECT token_id FROM billing_ledger WHERE auth_token_log_id = 1")
            .fetch_one(&pool)
            .await
            .expect("read local billing row");
    assert_eq!(local_token, "tok-local-billing");
    let peer_row: (i64, String, String, i64, Option<i64>) = sqlx::query_as(
        r#"
        SELECT auth_token_log_id, token_id, billing_state, business_credits, request_log_id
        FROM billing_ledger
        WHERE token_id = 'tok-peer-billing'
        LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await
    .expect("read peer billing row");
    assert!(peer_row.0 < 0, "peer billing row should use negative local id");
    assert_eq!(peer_row.2, "charged");
    assert_eq!(peer_row.3, 8);
    assert_eq!(peer_row.4, None);

    let baseline = proxy
        .export_ha_baseline_ndjson(tavily_hikari::HaSyncChannel::Billing, "node-a")
        .await
        .expect("export local billing baseline");
    assert!(baseline.ndjson.contains("tok-local-billing"));
    assert!(
        !baseline.ndjson.contains("tok-peer-billing"),
        "peer-imported billing rows must not be exported back to peers"
    );

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn dual_active_peer_runtime_sync_merges_mutable_quota_counter_deltas() {
    let runtime_baseline_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "runtime",
            "nodeId": "peer-runtime-counters",
            "highWatermark": 1
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "runtime",
            "resource": "auth_token_quota",
            "op": "upsert",
            "data": {
                "token_id": "tok-runtime-preserve",
                "month_start": 100,
                "month_count": 99
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "resource",
            "channel": "runtime",
            "resource": "mcp_sessions",
            "op": "upsert",
            "data": {
                "proxy_session_id": "sess-peer-runtime-baseline",
                "upstream_session_id": "upstream-peer-runtime-baseline",
                "upstream_key_id": null,
                "auth_token_id": null,
                "user_id": null,
                "protocol_version": "2025-03-26",
                "last_event_id": null,
                "gateway_mode": "upstream_mcp",
                "experiment_variant": "control",
                "ab_bucket": null,
                "routing_subject_hash": null,
                "fallback_reason": null,
                "rate_limited_until": null,
                "last_rate_limited_at": null,
                "last_rate_limit_reason": null,
                "created_at": 1,
                "updated_at": 1,
                "expires_at": 3600,
                "revoked_at": null,
                "revoke_reason": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "runtime",
            "nodeId": "peer-runtime-counters",
            "highWatermark": 1,
            "rowCount": 2
        })
        .to_string(),
    ]
    .join("\n");
    let runtime_events_ndjson = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "runtime",
            "after": 1,
            "limit": 1000
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "runtime",
            "event": {
                "seq": 2,
                "resource": "auth_token_quota",
                "resourceId": "tok-runtime-preserve",
                "op": "upsert",
                "payload": {
                    "token_id": "tok-runtime-preserve",
                    "month_start": 100,
                    "month_count": 101
                },
                "createdAt": 2,
                "checksum": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "runtime",
            "event": {
                "seq": 3,
                "resource": "mcp_sessions",
                "resourceId": "sess-peer-runtime-event",
                "op": "upsert",
                "payload": {
                    "proxy_session_id": "sess-peer-runtime-event",
                    "upstream_session_id": "upstream-peer-runtime-event",
                    "upstream_key_id": null,
                    "auth_token_id": null,
                    "user_id": null,
                    "protocol_version": "2025-03-26",
                    "last_event_id": null,
                    "gateway_mode": "upstream_mcp",
                    "experiment_variant": "control",
                    "ab_bucket": null,
                    "routing_subject_hash": null,
                    "fallback_reason": null,
                    "rate_limited_until": null,
                    "last_rate_limited_at": null,
                    "last_rate_limit_reason": null,
                    "created_at": 2,
                    "updated_at": 2,
                    "expires_at": 3600,
                    "revoked_at": null,
                    "revoke_reason": null
                },
                "createdAt": 2,
                "checksum": null
            }
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "runtime",
            "lastSeq": 3,
            "eventCount": 2
        })
        .to_string(),
    ]
    .join("\n");
    let runtime_baseline_body = zstd::stream::encode_all(runtime_baseline_ndjson.as_bytes(), 0)
        .expect("encode runtime baseline");
    let runtime_events_body = zstd::stream::encode_all(runtime_events_ndjson.as_bytes(), 0)
        .expect("encode runtime events");

    let app = Router::new()
        .route(
            "/api/admin/ha/baseline",
            get(move |_query: Query<std::collections::HashMap<String, String>>| {
                let runtime_baseline_body = runtime_baseline_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(runtime_baseline_body))
                        .expect("runtime baseline response")
                }
            }),
        )
        .route(
            "/api/admin/ha/events",
            get(move |_query: Query<std::collections::HashMap<String, String>>| {
                let runtime_events_body = runtime_events_body.clone();
                async move {
                    Response::builder()
                        .header("content-encoding", "zstd")
                        .body(Body::from(runtime_events_body))
                        .expect("runtime events response")
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

    let db_path = temp_db_path("ha-dual-active-runtime-preserve-quota");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-runtime-preserve-quota".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;

    sqlx::query(
        r#"
        INSERT INTO auth_tokens (id, secret, enabled, created_at)
        VALUES ('tok-runtime-preserve', 'secret-runtime-preserve', 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert local auth token");
    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES ('tok-runtime-preserve', 100, 7)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert local auth token quota");

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-a".to_string(),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        core_dual_active: true,
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

    let client = Client::new();
    run_ha_sync_once_for_peer(
        &state,
        &client,
        &format!("http://{source_addr}"),
        "node-b",
        "test-token",
        &[tavily_hikari::HaSyncChannel::Runtime],
    )
    .await
    .expect("dual-active peer sync should merge mutable quota counters");

    let month_count: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ?")
            .bind("tok-runtime-preserve")
            .fetch_one(&pool)
            .await
            .expect("read local auth token quota");
    assert_eq!(month_count, 108);

    run_ha_sync_once_for_peer(
        &state,
        &client,
        &format!("http://{source_addr}"),
        "node-b",
        "test-token",
        &[tavily_hikari::HaSyncChannel::Runtime],
    )
    .await
    .expect("dual-active peer sync should not double-apply stable peer counters");
    let month_count_after_replay: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ?")
            .bind("tok-runtime-preserve")
            .fetch_one(&pool)
            .await
            .expect("read local auth token quota after replay");
    assert_eq!(month_count_after_replay, 108);

    let imported_sessions: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mcp_sessions WHERE proxy_session_id IN ('sess-peer-runtime-baseline', 'sess-peer-runtime-event')",
    )
    .fetch_one(&pool)
    .await
    .expect("count imported peer sessions");
    assert_eq!(imported_sessions, 2);

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn dual_active_runtime_counter_exports_only_local_contribution() {
    let db_path = temp_db_path("ha-dual-active-runtime-local-counter-export");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-runtime-local-counter-export".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    tavily_hikari::repair_ha_triggers_once(&db_str, tavily_hikari::HaMode::ActiveStandby)
        .await
        .expect("repair HA triggers");
    let pool = connect_sqlite_test_pool(&db_str).await;

    sqlx::query("INSERT OR IGNORE INTO ha_outbox_suppression (id) VALUES ('local')")
        .execute(&pool)
        .await
        .expect("enable HA suppression for fixture setup");
    sqlx::query(
        r#"
        INSERT INTO auth_tokens (id, secret, enabled, created_at)
        VALUES ('tok-local-counter-export', 'secret-local-counter-export', 1, 1)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert auth token");
    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES ('tok-local-counter-export', 100, 10)
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert imported-only aggregate counter");
    sqlx::query(
        r#"
        INSERT INTO ha_runtime_counter_imports (
            peer_node_id, resource, resource_id, counter_scope, counter_value, updated_at
        )
        VALUES (
            'node-b',
            'auth_token_quota',
            'tok-local-counter-export',
            '100',
            10,
            1
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("insert peer counter shadow");
    sqlx::query("DELETE FROM ha_outbox_suppression WHERE id = 'local'")
        .execute(&pool)
        .await
        .expect("disable HA suppression");
    sqlx::query("DELETE FROM ha_runtime_outbox")
        .execute(&pool)
        .await
        .expect("clear fixture outbox");

    sqlx::query(
        "UPDATE auth_token_quota SET month_count = 15 WHERE token_id = 'tok-local-counter-export'",
    )
    .execute(&pool)
    .await
    .expect("record local counter contribution");

    let baseline = proxy
        .export_ha_baseline_ndjson(tavily_hikari::HaSyncChannel::Runtime, "node-a")
        .await
        .expect("export runtime baseline");
    let mut baseline_count = None;
    for line in baseline.ndjson.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("parse baseline line");
        if value.get("kind").and_then(serde_json::Value::as_str) != Some("resource")
            || value.get("resource").and_then(serde_json::Value::as_str)
                != Some("auth_token_quota")
        {
            continue;
        }
        let data = value.get("data").expect("baseline resource data");
        if data.get("token_id").and_then(serde_json::Value::as_str)
            == Some("tok-local-counter-export")
        {
            baseline_count = data.get("month_count").and_then(serde_json::Value::as_i64);
        }
    }
    assert_eq!(
        baseline_count,
        Some(5),
        "runtime baseline must export only local counter contribution"
    );

    let events = proxy
        .list_ha_events_after(tavily_hikari::HaSyncChannel::Runtime, 0, 10)
        .await
        .expect("list runtime events");
    let counter_event = events
        .iter()
        .find(|event| {
            event.resource == "auth_token_quota"
                && event
                    .payload
                    .get("token_id")
                    .and_then(serde_json::Value::as_str)
                    == Some("tok-local-counter-export")
        })
        .expect("runtime counter event");
    assert_eq!(
        counter_event
            .payload
            .get("month_count")
            .and_then(serde_json::Value::as_i64),
        Some(5),
        "runtime events must store only local counter contribution"
    );

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

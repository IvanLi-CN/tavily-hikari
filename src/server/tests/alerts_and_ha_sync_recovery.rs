use super::*;
use super::core_support_and_parsing::*;

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

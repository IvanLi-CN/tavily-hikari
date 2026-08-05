use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

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
async fn dual_active_standby_with_persisted_epoch_keeps_basic_business_enabled() {
    let db_path = temp_db_path("ha-dual-active-standby-business-gate");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-standby-business-gate".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .advance_writable_authority_epoch(tavily_hikari::WritableAuthorityPhase::Standby)
        .await
        .expect("persist standby authority epoch");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-follower".to_string(),
        database_path: Some(db_str.clone()),
        core_dual_active: true,
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("og-core".to_string()),
        ..tavily_hikari::HaConfig::default()
    });
    ha.apply_dual_active_leader(Some("node-leader".to_string()))
        .await
        .expect("apply remote leader");
    ha.mark_sync_success().await;
    let authority = proxy
        .get_writable_authority_state()
        .await
        .expect("load standby authority");
    ha.restore_writable_authority(authority)
        .await
        .expect("restore standby authority");
    let status = ha.status().await;
    assert!(status.allows_basic_business, "unexpected HA status: {status:?}");
    let state = Arc::new(AppState {
        proxy,
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
    assert!(
        ensure_ha_allows_basic_business(&state, "/api/tavily/search")
            .await
            .is_ok()
    );

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

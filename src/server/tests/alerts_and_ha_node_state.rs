use super::*;
use super::core_support_and_parsing::*;

#[tokio::test]
async fn redundant_ha_node_state_flush_is_skipped_after_same_state_persists() {
    let db_path = temp_db_path("ha-node-state-dedup");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-node-state-dedup".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let source_settings = tavily_hikari::HaSourceSettingsView {
        source_kind: tavily_hikari::HaSourceKind::Direct,
        direct_origin_scheme: Some(tavily_hikari::OriginScheme::Http),
        direct_origin_host: Some("node-a.example.test".to_string()),
        direct_origin_port: Some(15443),
        origin_group_id: None,
        target: Some("node-a.example.test:15443".to_string()),
    };

    proxy
        .persist_ha_node_state(
            "node-a",
            tavily_hikari::HaNodeRole::Standby,
            Some("primary.example.test:443"),
            Some(&source_settings),
            None,
        )
        .await
        .expect("enqueue initial node state");
    proxy
        .flush_ha_state_writes()
        .await
        .expect("flush initial node state");

    let pool = connect_sqlite_test_pool(&db_str).await;
    let first_updated_at: i64 =
        sqlx::query_scalar("SELECT updated_at FROM ha_node_state WHERE id = 'local'")
            .fetch_one(&pool)
            .await
            .expect("read first updated_at");

    tokio::time::sleep(Duration::from_secs(1)).await;

    proxy
        .persist_ha_node_state(
            "node-a",
            tavily_hikari::HaNodeRole::Standby,
            Some("primary.example.test:443"),
            Some(&source_settings),
            None,
        )
        .await
        .expect("enqueue duplicate node state");
    proxy
        .flush_ha_state_writes()
        .await
        .expect("flush duplicate node state");

    let second_updated_at: i64 =
        sqlx::query_scalar("SELECT updated_at FROM ha_node_state WHERE id = 'local'")
            .fetch_one(&pool)
            .await
            .expect("read second updated_at");
    assert_eq!(
        second_updated_at, first_updated_at,
        "duplicate node state should not rewrite ha_node_state"
    );

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn dual_active_startup_seeds_leader_key_from_persisted_full_master_role() {
    let db_path = temp_db_path("ha-dual-active-seed-leader-key");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-dual-active-seed-leader-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .persist_ha_node_state(
            "node-dual-active",
            tavily_hikari::HaNodeRole::FullMaster,
            Some("edgeone.example.test:443"),
            None,
            Some("seeded full master"),
        )
        .await
        .expect("persist full master node state");
    proxy
        .flush_ha_state_writes()
        .await
        .expect("flush full master node state");

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-dual-active".to_string(),
        database_path: Some(db_str.clone()),
        source_kind: Some(tavily_hikari::HaSourceKind::OriginGroup),
        source_origin_group_id: Some("eo-group-dual-active".to_string()),
        core_dual_active: true,
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: "http://127.0.0.1:9".to_string(),
        ..tavily_hikari::HaConfig::default()
    });

    let status = super::reconcile_ha_startup_role(
        &proxy,
        &ha,
        Some(tavily_hikari::HaNodeRole::FullMaster),
    )
    .await;

    assert!(status.dual_active_enabled);
    assert_eq!(status.full_master_node_id.as_deref(), Some("node-dual-active"));
    assert_eq!(status.role, tavily_hikari::HaNodeRole::FullMaster);
    assert!(status.allows_basic_business);
    assert!(status.allows_full_writes);

    let persisted = proxy
        .get_ha_full_master_node_id()
        .await
        .expect("read leader key");
    assert_eq!(persisted.as_deref(), Some("node-dual-active"));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

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

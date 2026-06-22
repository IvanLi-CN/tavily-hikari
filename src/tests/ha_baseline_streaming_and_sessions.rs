use super::*;

#[tokio::test]
async fn baseline_apply_abort_restores_foreign_keys_on_reused_pool_connection() {
    let db_path = temp_db_path("ha-baseline-apply-abort-fk");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-baseline-apply-abort-fk".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let pinned_a = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("pin first connection");
    let pinned_b = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("pin second connection");

    let invalid_baseline = "{\"kind\":\"baseline_start\"}\nnot-json\n";
    let baseline_err = proxy
        .apply_ha_baseline_ndjson(HaSyncChannel::Control, invalid_baseline)
        .await
        .expect_err("invalid baseline should abort apply session");
    assert!(
        baseline_err
            .to_string()
            .contains("invalid HA baseline NDJSON"),
        "unexpected baseline error: {baseline_err}"
    );

    drop(pinned_a);
    drop(pinned_b);

    let mut reused = tokio::time::timeout(Duration::from_secs(2), proxy.key_store.pool.acquire())
        .await
        .expect("reacquire should not hang after abort")
        .expect("reacquire released connection");
    let fk_enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *reused)
        .await
        .expect("foreign_keys pragma after abort");
    assert_eq!(fk_enabled, 1);

    sqlx::query(
        r#"
        INSERT INTO user_tag_bindings (user_id, tag, created_at)
        VALUES ('missing-user', 'broken-tag', 1)
        "#,
    )
    .execute(&mut *reused)
    .await
    .expect_err("reused connection should still enforce foreign keys");

    drop(reused);
    let _ = std::fs::remove_file(db_path.clone());
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn write_ha_baseline_ndjson_closes_read_snapshot_before_reusing_connection() {
    let db_path = temp_db_path("ha-baseline-write-closes-read-snapshot");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-baseline-write-closes-read-snapshot".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let mut output = Vec::new();
    proxy
        .write_ha_baseline_ndjson(HaSyncChannel::Control, "writer-node", &mut output)
        .await
        .expect("write baseline ndjson");
    assert!(!output.is_empty(), "baseline writer should emit ndjson");

    let pinned_a = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("pin first connection");
    let pinned_b = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("pin second connection");
    let mut reused = tokio::time::timeout(Duration::from_secs(2), proxy.key_store.pool.acquire())
        .await
        .expect("read snapshot should be closed before reusing third connection")
        .expect("reacquire third connection");
    let fk_enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *reused)
        .await
        .expect("reused connection should stay usable");
    assert_eq!(fk_enabled, 1);

    drop(reused);
    drop(pinned_b);
    drop(pinned_a);
    let _ = std::fs::remove_file(db_path.clone());
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

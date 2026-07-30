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

#[tokio::test]
async fn ha_replicates_reconciliation_global_backoff_metadata() {
    let db_path = temp_db_path("ha-reconciliation-global-backoff-meta");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-reconciliation-global-backoff-meta".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .key_store
        .configure_ha_event_writes(HaMode::ActiveStandby)
        .await
        .expect("configure HA event writes");

    for (key, value) in [
        (META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1, 3),
        (META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1, 1),
        (
            META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
            1_752_000_120,
        ),
    ] {
        proxy
            .key_store
            .set_meta_i64(key, value)
            .await
            .expect("persist reconciliation backoff metadata");
    }

    let replicated_event_payloads: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT payload_json
        FROM ha_outbox
        WHERE resource = 'meta'
        ORDER BY seq ASC
        "#,
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read replicated reconciliation backoff events");
    assert_eq!(replicated_event_payloads.len(), 3);
    for key in [
        META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1,
        META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
    ] {
        assert!(
            replicated_event_payloads
                .iter()
                .any(|payload| payload.contains(key)),
            "incremental event should contain {key}"
        );
    }

    let mut output = Vec::new();
    proxy
        .write_ha_baseline_ndjson(HaSyncChannel::Control, "writer-node", &mut output)
        .await
        .expect("write control baseline");
    let baseline = String::from_utf8(output).expect("control baseline is utf8");
    for key in [
        META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1,
        META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
    ] {
        assert!(baseline.contains(key), "baseline should contain {key}");
    }

    let _ = std::fs::remove_file(db_path.clone());
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn runtime_baseline_upsert_preserves_existing_rows() {
    let db_path = temp_db_path("ha-runtime-baseline-upsert-preserves-existing");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-runtime-baseline-upsert".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
            'sess-local',
            'upstream-local',
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
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert local session");

    let empty_runtime_baseline = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_start",
            "channel": "runtime",
            "nodeId": "peer-empty",
            "highWatermark": 0
        })
        .to_string(),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "baseline_end",
            "channel": "runtime",
            "nodeId": "peer-empty",
            "highWatermark": 0,
            "rowCount": 0
        })
        .to_string(),
    ]
    .join("\n");

    let mut session = proxy
        .begin_ha_baseline_apply_with_mode(HaSyncChannel::Runtime, HaBaselineApplyMode::Upsert)
        .await
        .expect("begin upsert baseline apply");
    for line in empty_runtime_baseline.lines() {
        session.apply_line(line).await.expect("apply baseline line");
    }
    session.finish().await.expect("finish baseline apply");

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mcp_sessions WHERE proxy_session_id = 'sess-local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count preserved session");
    assert_eq!(
        row_count, 1,
        "upsert baseline should not delete local runtime rows"
    );

    let _ = std::fs::remove_file(db_path.clone());
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

use super::*;

#[tokio::test]
async fn runtime_baseline_rearms_current_source_identity_repair() {
    let source_path = temp_db_path("ha-current-source-identity-source");
    let source_str = source_path.to_string_lossy().to_string();
    let source = TavilyProxy::with_endpoint(
        vec!["tvly-ha-current-source-identity-source".to_string()],
        DEFAULT_UPSTREAM,
        &source_str,
    )
    .await
    .expect("create source proxy");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             settlement_mode, period_start, period_end, request_count,
             first_used_at, last_used_at, updated_at
           ) VALUES ('ha-identity-token', 'ha-identity-key', '2026-07-15/S1',
                     'identity-current', 'token:identity-current', 'shadow',
                     100, 400, 1, 100, 200, 300)"#,
    )
    .execute(&source.key_store.pool)
    .await
    .expect("insert current reconciliation source");
    sqlx::query(
        r#"UPDATE upstream_reconciliation_work
           SET project_id = 'identity-stale', billing_subject = 'token:identity-stale',
               period_start = 1, period_end = 2, scheduling_key_id = 'stale-key',
               work_generation = 3, completed_generation = 3, next_attempt_at = 999,
               last_outcome = 'settled'
           WHERE token_id = 'ha-identity-token' AND period_code = '2026-07-15/S1'"#,
    )
    .execute(&source.key_store.pool)
    .await
    .expect("seed stale durable work identity");
    let baseline = source
        .export_ha_baseline_ndjson(HaSyncChannel::Runtime, "identity-target")
        .await
        .expect("export runtime baseline with stale work");

    let target_path = temp_db_path("ha-current-source-identity-target");
    let target_str = target_path.to_string_lossy().to_string();
    let target = TavilyProxy::with_endpoint(
        vec!["tvly-ha-current-source-identity-target".to_string()],
        DEFAULT_UPSTREAM,
        &target_str,
    )
    .await
    .expect("create target proxy");
    let mut settings = target
        .get_system_settings()
        .await
        .expect("load target reconciliation settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    target
        .set_system_settings(&settings)
        .await
        .expect("enable target reconciliation representative");
    let mut session = target
        .begin_ha_baseline_apply(HaSyncChannel::Runtime)
        .await
        .expect("begin runtime baseline apply");
    for line in baseline
        .ndjson
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        session
            .apply_line(line)
            .await
            .expect("apply runtime baseline line");
    }
    session
        .finish()
        .await
        .expect("finish runtime baseline before enqueueing representative");
    target
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("enqueue the rearmed reconciliation representative");

    let pending: (i64, Option<String>, i64) = sqlx::query_as(
        "SELECT completed, last_defer_reason, identity_repair_generation \
         FROM upstream_reconciliation_projection_state \
         WHERE id = 'local'",
    )
    .fetch_one(&target.key_store.pool)
    .await
    .expect("read rearmed identity repair state");
    assert_eq!(
        pending,
        (0, Some("identity_repair_pending".to_string()), 2),
        "the Runtime baseline must fence snapshots from before the import"
    );
    let representative_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs \
         WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .fetch_one(&target.key_store.pool)
    .await
    .expect("read rearmed reconciliation representative");
    assert_eq!(representative_jobs, 1);
    assert_eq!(
        target
            .key_store
            .advance_upstream_reconciliation_work_projection()
            .await
            .expect("repair imported stale work"),
        ReconciliationProjectionSliceOutcome::Advanced {
            scanned_rows: 1,
            completed: false,
        }
    );
    let repaired: (String, String, i64, i64, String, i64, i64, Option<String>) = sqlx::query_as(
        "SELECT project_id, billing_subject, period_start, period_end, scheduling_key_id, \
                    work_generation, next_attempt_at, last_outcome \
             FROM upstream_reconciliation_work \
             WHERE token_id = 'ha-identity-token' AND period_code = '2026-07-15/S1'",
    )
    .fetch_one(&target.key_store.pool)
    .await
    .expect("read repaired imported work identity");
    assert_eq!(
        repaired,
        (
            "identity-current".to_string(),
            "token:identity-current".to_string(),
            100,
            400,
            "ha-identity-key".to_string(),
            4,
            0,
            None,
        )
    );

    drop(source);
    drop(target);
    for path in [&source_path, &target_path] {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }
}

#[tokio::test]
async fn runtime_event_key_removal_fences_reconciliation_source_identity() {
    let db_path = temp_db_path("ha-current-source-identity-delete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-current-source-identity-delete".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create target proxy");
    let token_id = "ha-identity-delete-token";
    let period_code = "2026-07-15/S1";
    let insert_usage = r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             settlement_mode, period_start, period_end, request_count,
             first_used_at, last_used_at, updated_at
           ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, 100, 200, 300)"#;
    for (key_id, project_id, billing_subject, period_start, period_end) in [
        (
            "ha-identity-delete-key-a",
            "identity-a",
            "token:identity-a",
            100_i64,
            400_i64,
        ),
        (
            "ha-identity-delete-key-b",
            "identity-b",
            "token:identity-b",
            200_i64,
            500_i64,
        ),
    ] {
        sqlx::query(insert_usage)
            .bind(token_id)
            .bind(key_id)
            .bind(period_code)
            .bind(project_id)
            .bind(billing_subject)
            .bind(period_start)
            .bind(period_end)
            .execute(&proxy.key_store.pool)
            .await
            .expect("seed runtime reconciliation source");
    }
    let initial_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read initial work generation");

    let events = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "runtime",
            "after": 0,
            "limit": 1,
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "runtime",
            "event": {
                "seq": 1,
                "resource": "upstream_reconciliation_usage",
                "resourceId": "ha-identity-delete-token:ha-identity-delete-key-a:2026-07-15/S1",
                "op": "delete",
                "payload": {
                    "token_id": token_id,
                    "key_id": "ha-identity-delete-key-a",
                    "period_code": period_code,
                },
            },
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "runtime",
            "lastSeq": 1,
            "eventCount": 1,
        }),
    ]
    .into_iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    proxy
        .apply_ha_events_ndjson(HaSyncChannel::Runtime, &events)
        .await
        .expect("apply runtime source Key removal");

    let rederived: (String, String, i64, i64) = sqlx::query_as(
        "SELECT scheduling_key_id, project_id, work_generation, next_attempt_at \
         FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read HA-fenced reconciliation work");
    assert_eq!(
        rederived,
        (
            "ha-identity-delete-key-b".to_string(),
            "identity-b".to_string(),
            initial_generation + 1,
            0,
        )
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_quota_truth_apply_invalidates_cached_account_resolution() {
    let db_path = temp_db_path("ha-quota-cache-invalidation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-quota-cache-invalidation".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let stale = build_account_quota_resolution(AccountQuotaLimits::zero_base(), Vec::new());
    let cache_generation = (
        proxy
            .key_store
            .account_quota_resolution_generation
            .load(std::sync::atomic::Ordering::Acquire),
        0,
    );
    proxy
        .key_store
        .cache_account_quota_resolution("ha-user", &stale, cache_generation)
        .await;

    let events = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "runtime",
            "after": 0,
            "limit": 1,
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "runtime",
            "event": {
                "seq": 1,
                "resource": "account_quota_limits",
                "resourceId": "missing-user",
                "op": "delete",
                "payload": null,
            },
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "runtime",
            "lastSeq": 1,
            "eventCount": 1,
        }),
    ]
    .into_iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    proxy
        .apply_ha_events_ndjson(HaSyncChannel::Runtime, &events)
        .await
        .expect("apply runtime quota event");

    assert!(
        proxy
            .key_store
            .cached_account_quota_resolution("ha-user")
            .await
            .is_none(),
        "HA quota truth must invalidate cached quota decisions"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn retired_passkey_ha_resources_are_discarded_without_blocking_control_sync() {
    let db_path = temp_db_path("ha-retired-passkey-resource");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-retired-passkey-resource".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let events = [
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_start",
            "channel": "control",
            "after": 0,
            "limit": 1,
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "event",
            "channel": "control",
            "event": {
                "seq": 7,
                "resource": "admin_passkey_credentials",
                "resourceId": "legacy-credential",
                "op": "upsert",
                "payload": { "credential_id": "legacy-credential" },
            },
        }),
        serde_json::json!({
            "schemaVersion": 2,
            "kind": "events_end",
            "channel": "control",
            "lastSeq": 7,
            "eventCount": 1,
        }),
    ]
    .into_iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");

    let result = proxy
        .apply_ha_events_ndjson(HaSyncChannel::Control, &events)
        .await
        .expect("retired passkey event should be accepted");
    assert_eq!(result.high_watermark, 7);
    assert_eq!(result.row_count, 0);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_passkey_credentials WHERE credential_id = 'legacy-credential'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count passkey rows");
    assert_eq!(count, 0);

    let baseline = proxy
        .export_ha_baseline_ndjson(HaSyncChannel::Control, "new-node")
        .await
        .expect("export control baseline");
    assert!(!baseline.ndjson.contains("admin_passkey_"));

    let legacy_baseline = [
        serde_json::json!({ "kind": "baseline_start", "highWatermark": 3 }),
        serde_json::json!({
            "kind": "resource",
            "resource": "admin_passkey_sessions",
            "data": { "token": "legacy-session" },
        }),
        serde_json::json!({ "kind": "baseline_end", "highWatermark": 3 }),
    ]
    .into_iter()
    .map(|line| line.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let baseline_result = proxy
        .apply_ha_baseline_ndjson(HaSyncChannel::Control, &legacy_baseline)
        .await
        .expect("retired passkey baseline should be accepted");
    assert_eq!(baseline_result.row_count, 0);
}

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
async fn ha_replicates_reconciliation_backoff_metadata() {
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
        (META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1, 3),
        (META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1, 1),
        (
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
            1_752_000_060,
        ),
        (
            META_KEY_UPSTREAM_RECONCILIATION_LOCAL_LAST_RECOVERED_AT_V1,
            1_752_000_000,
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
    assert_eq!(replicated_event_payloads.len(), 7);
    for key in [
        META_KEY_UPSTREAM_RECONCILIATION_PRESSURE_STREAK_V1,
        META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_LEVEL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_BACKOFF_UNTIL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_LAST_RECOVERED_AT_V1,
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
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_PRESSURE_STREAK_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_LEVEL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_BACKOFF_UNTIL_V1,
        META_KEY_UPSTREAM_RECONCILIATION_LOCAL_LAST_RECOVERED_AT_V1,
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

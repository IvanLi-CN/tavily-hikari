use super::*;

async fn explain_query_plan_details(pool: &sqlx::SqlitePool, sql: &str) -> Vec<String> {
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .expect("explain query plan")
        .into_iter()
        .map(|row| row.try_get::<String, _>("detail").expect("plan detail"))
        .collect()
}

#[test]
fn online_ha_gc_has_a_tight_slice_without_changing_cli_defaults() {
    let online = HaOutboxGcOptions::online();
    let cli = HaOutboxGcOptions::default();
    assert_eq!(online.batch_size, 250);
    assert_eq!(online.max_batches, 4);
    assert_eq!(online.max_runtime_secs, 1);
    assert_eq!(online.inter_batch_sleep_ms, 100);
    assert_eq!(cli.batch_size, 20_000);
    assert_eq!(cli.max_batches, 8);
    assert_eq!(cli.max_runtime_secs, 20);
}

#[tokio::test]
async fn online_ha_gc_persists_one_channel_rotation_between_slices() {
    let db_path = temp_db_path("ha-outbox-online-gc-round-robin");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-online-gc-round-robin".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let old_ts = Utc::now().timestamp() - (15 * SECS_PER_DAY);
    let pool = connect_sqlite_test_pool(&db_str).await;
    let mut tx = pool.begin().await.expect("begin seed transaction");
    for index in 0..1_000 {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES ('state', 'users', ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(format!("online-control-{index}"))
        .bind(old_ts)
        .execute(&mut *tx)
        .await
        .expect("insert old control event");
    }
    sqlx::query(
        r#"
        INSERT INTO ha_billing_outbox
            (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'billing_ledger', 'online-billing', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(old_ts)
    .execute(&mut *tx)
    .await
    .expect("insert old billing event");
    sqlx::query(
        r#"
        INSERT INTO ha_runtime_outbox
            (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'mcp_sessions', 'online-runtime', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(old_ts)
    .execute(&mut *tx)
    .await
    .expect("insert old runtime event");
    tx.commit().await.expect("commit seed transaction");
    pool.close().await;

    let report = proxy.gc_ha_outbox_online().await.expect("control gc");
    assert_eq!(report.batches, 4);
    assert_eq!(report.deleted_rows, 1_000);
    assert_eq!(report.channels.len(), 1);
    assert_eq!(report.channels[0].channel, HaSyncChannel::Control);
    assert_eq!(report.channels[0].batches, 4);
    assert_eq!(report.channels[0].deleted_rows, 1_000);
    assert!(!report.channels[0].has_more);
    assert!(report.has_more);

    let report = proxy.gc_ha_outbox_online().await.expect("billing gc");
    assert_eq!(report.channels.len(), 1);
    assert_eq!(report.channels[0].channel, HaSyncChannel::Billing);
    assert_eq!(report.channels[0].deleted_rows, 1);
    assert!(report.has_more);

    let report = proxy.gc_ha_outbox_online().await.expect("runtime gc");
    assert_eq!(report.channels.len(), 1);
    assert_eq!(report.channels[0].channel, HaSyncChannel::Runtime);
    assert_eq!(report.channels[0].deleted_rows, 1);
    assert!(!report.has_more);
    assert!(report.completed);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn online_ha_gc_scans_legacy_rows_by_persisted_sequence_cursor() {
    let db_path = temp_db_path("ha-outbox-online-gc-legacy-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-online-gc-legacy-cursor".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();
    for (resource, resource_id) in [
        ("scheduled_jobs", "legacy-job-1"),
        ("users", "valid-user"),
        ("scheduled_jobs", "legacy-job-2"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES ('state', ?, ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(resource)
        .bind(resource_id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert outbox row");
    }
    pool.close().await;

    let report = proxy.gc_ha_outbox_online().await.expect("online gc");
    let control = report.channels.first().expect("control report");
    assert_eq!(control.channel, HaSyncChannel::Control);
    assert_eq!(control.invalid_legacy_deleted_rows, 2);
    assert_eq!(control.retention_deleted_rows, 0);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let remaining_resources: Vec<String> =
        sqlx::query_scalar("SELECT resource FROM ha_outbox ORDER BY seq ASC")
            .fetch_all(&pool)
            .await
            .expect("read remaining resources");
    let cursor: i64 = sqlx::query_scalar(
        "SELECT last_legacy_control_seq FROM ha_outbox_gc_state WHERE id = 'local'",
    )
    .fetch_one(&pool)
    .await
    .expect("read legacy cursor");
    assert_eq!(remaining_resources, vec!["users"]);
    assert_eq!(cursor, 0);
    pool.close().await;

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn standalone_ha_outbox_gc_deletes_expired_rows_across_channels_in_bounded_batches() {
    let db_path = temp_db_path("ha-outbox-gc-bounded-control-only");
    let db_str = db_path.to_string_lossy().to_string();
    let old_control_ts = Utc::now().timestamp() - (4 * SECS_PER_DAY);
    let old_long_retention_ts = Utc::now().timestamp() - (15 * SECS_PER_DAY);
    let recent_ts = Utc::now().timestamp();

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE ha_outbox (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            resource TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            op TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            checksum TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create ha_outbox");
    sqlx::query(
        r#"
        CREATE TABLE ha_billing_outbox (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            resource TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            op TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            checksum TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create ha_billing_outbox");
    sqlx::query(
        r#"
        CREATE TABLE ha_runtime_outbox (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            resource TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            op TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            checksum TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create ha_runtime_outbox");
    sqlx::query(r#"CREATE INDEX idx_ha_outbox_created ON ha_outbox(created_at, seq)"#)
        .execute(&pool)
        .await
        .expect("create control outbox index");
    sqlx::query(
        r#"CREATE INDEX idx_ha_outbox_resource_created_seq ON ha_outbox(resource, created_at, seq)"#,
    )
    .execute(&pool)
    .await
    .expect("create control outbox resource/time index");
    sqlx::query(
        r#"CREATE INDEX idx_ha_billing_outbox_created ON ha_billing_outbox(created_at, seq)"#,
    )
    .execute(&pool)
    .await
    .expect("create billing outbox index");
    sqlx::query(
        r#"CREATE INDEX idx_ha_runtime_outbox_created ON ha_runtime_outbox(created_at, seq)"#,
    )
    .execute(&pool)
    .await
    .expect("create runtime outbox index");

    for seq in 0..3 {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox (
                kind, resource, resource_id, op, payload_json, created_at, checksum
            ) VALUES ('state', 'users', ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(format!("old-{seq}"))
        .bind(old_control_ts + seq)
        .execute(&pool)
        .await
        .expect("seed old control event");
    }
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'users', 'recent-1', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(recent_ts)
    .execute(&pool)
    .await
    .expect("seed recent control event");
    sqlx::query(
        r#"
        INSERT INTO ha_billing_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'billing_ledger', 'billing-1', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(old_long_retention_ts)
    .execute(&pool)
    .await
    .expect("seed billing outbox event");
    sqlx::query(
        r#"
        INSERT INTO ha_billing_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'billing_ledger', 'billing-recent', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(recent_ts)
    .execute(&pool)
    .await
    .expect("seed recent billing outbox event");
    sqlx::query(
        r#"
        INSERT INTO ha_runtime_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'mcp_sessions', 'runtime-1', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(old_long_retention_ts)
    .execute(&pool)
    .await
    .expect("seed runtime outbox event");
    sqlx::query(
        r#"
        INSERT INTO ha_runtime_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES ('state', 'mcp_sessions', 'runtime-recent', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(recent_ts)
    .execute(&pool)
    .await
    .expect("seed recent runtime outbox event");
    drop(pool);

    let report = run_ha_outbox_gc_once(
        &db_str,
        HaOutboxGcOptions {
            batch_size: 2,
            max_batches: 1,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        },
    )
    .await
    .expect("run standalone ha outbox gc");
    assert_eq!(report.deleted_rows, 4);
    assert_eq!(report.batches, 3);
    assert!(!report.completed);
    assert!(report.has_more);
    assert_eq!(report.channels.len(), 3);
    assert_eq!(report.channels[0].channel, HaSyncChannel::Control);
    assert_eq!(report.channels[0].deleted_rows, 2);
    assert!(report.channels[0].has_more);
    assert_eq!(report.channels[1].channel, HaSyncChannel::Billing);
    assert_eq!(report.channels[1].deleted_rows, 1);
    assert!(!report.channels[1].has_more);
    assert_eq!(report.channels[2].channel, HaSyncChannel::Runtime);
    assert_eq!(report.channels[2].deleted_rows, 1);
    assert!(!report.channels[2].has_more);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("reopen sqlite pool");
    let control_remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox")
        .fetch_one(&pool)
        .await
        .expect("count remaining control events");
    let old_control_remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE created_at < ?")
            .bind(recent_ts - SECS_PER_DAY)
            .fetch_one(&pool)
            .await
            .expect("count remaining old control events");
    let billing_remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_billing_outbox")
        .fetch_one(&pool)
        .await
        .expect("count remaining billing events");
    let runtime_remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ha_runtime_outbox")
        .fetch_one(&pool)
        .await
        .expect("count remaining runtime events");

    assert_eq!(control_remaining, 2);
    assert_eq!(old_control_remaining, 1);
    assert_eq!(billing_remaining, 1);
    assert_eq!(runtime_remaining, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_outbox_cursor_validation_query_prefers_resource_created_seq_index() {
    let db_path = temp_db_path("ha-outbox-cursor-query-plan");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE ha_outbox (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            resource TEXT NOT NULL,
            resource_id TEXT NOT NULL,
            op TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            checksum TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create ha_outbox");
    sqlx::query(
        r#"CREATE INDEX idx_ha_outbox_resource_created_seq ON ha_outbox(resource, created_at, seq)"#,
    )
    .execute(&pool)
    .await
    .expect("create resource/time index");
    sqlx::query(r#"CREATE INDEX idx_ha_outbox_created ON ha_outbox(created_at, seq)"#)
        .execute(&pool)
        .await
        .expect("create created/seq index");

    let plan = explain_query_plan_details(
        &pool,
        r#"
        EXPLAIN QUERY PLAN
        SELECT MIN(seq)
        FROM ha_outbox
        WHERE created_at >= 0
          AND resource IN ('meta', 'users', 'api_keys', 'api_key_quarantines', 'api_key_maintenance_records', 'system_settings', 'scheduled_jobs')
        "#,
    )
    .await;
    let joined = plan.join("\n");
    assert!(
        joined.contains("idx_ha_outbox_resource_created_seq"),
        "expected resource/time/seq index in query plan, got:\n{joined}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_events_cursor_ignores_legacy_sequence_after_valid_rows_are_gc() {
    let db_path = temp_db_path("ha-events-cursor-legacy-sequence");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-cursor-legacy-sequence-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let old_ts = Utc::now().timestamp() - (4 * SECS_PER_DAY);
    let now = Utc::now().timestamp();
    for (seq, resource, created_at) in [
        (1, "meta", now),
        (2, "meta", now),
        (3, "removed_resource", old_ts),
        (4, "meta", now),
    ] {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (seq, kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES (?, 'state', ?, ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(seq)
        .bind(resource)
        .bind(format!("legacy-sequence-{seq}"))
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert cursor sequence event");
    }
    pool.close().await;

    proxy
        .gc_ha_outbox_with_options(HaOutboxGcOptions {
            batch_size: 100,
            max_batches: 8,
            max_runtime_secs: 20,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("gc legacy event");
    let events = proxy
        .list_ha_events_after(HaSyncChannel::Control, 2, 10)
        .await
        .expect("legacy sqlite sequence must not force a baseline reset");
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![4]
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_events_cursor_accepts_legacy_bridge_before_first_valid_event() {
    let db_path = temp_db_path("ha-events-cursor-legacy-bridge");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-cursor-legacy-bridge-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();
    for (seq, resource) in [(1, "meta"), (2, "removed_resource"), (3, "meta")] {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (seq, kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES (?, 'state', ?, ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(seq)
        .bind(resource)
        .bind(format!("cursor-bridge-{seq}"))
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert cursor bridge event");
    }

    let events = proxy
        .list_ha_events_after(HaSyncChannel::Control, 1, 10)
        .await
        .expect("legacy bridge should keep cursor valid");
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![3]
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_events_cursor_returns_baseline_after_expired_valid_gap() {
    let db_path = temp_db_path("ha-events-cursor-expired-valid-gap");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-events-cursor-expired-valid-gap-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let old_ts = Utc::now().timestamp() - (4 * SECS_PER_DAY);
    let now = Utc::now().timestamp();
    for (seq, resource, created_at) in [
        (1, "meta", now),
        (2, "meta", old_ts),
        (3, "removed_resource", now),
        (4, "meta", now),
    ] {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (seq, kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES (?, 'state', ?, ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(seq)
        .bind(resource)
        .bind(format!("expired-valid-gap-{seq}"))
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert cursor gap event");
    }
    pool.close().await;

    proxy
        .gc_ha_outbox_with_options(HaOutboxGcOptions {
            batch_size: 100,
            max_batches: 8,
            max_runtime_secs: 20,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("gc expired valid and legacy events");
    let error = proxy
        .list_ha_events_after(HaSyncChannel::Control, 1, 10)
        .await
        .expect_err("cursor must require a baseline after valid retention deletion");
    assert!(
        error.to_string().contains("older than retention window"),
        "unexpected cursor error: {error}"
    );
    proxy
        .ack_ha_peer_watermark(HaSyncChannel::Control, "standby-expired-valid-gap", 1)
        .await
        .expect("ack peer watermark");
    let health = proxy
        .ha_peer_channel_health(HaSyncChannel::Control, "standby-expired-valid-gap")
        .await
        .expect("read expired channel health");
    assert_eq!(health.cursor_state, "expired_backlog");
    assert!(health.expired_backlog);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_peer_health_ignores_legacy_gap_after_gc() {
    let db_path = temp_db_path("ha-peer-health-legacy-gap");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-peer-health-legacy-gap".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();
    for (seq, resource) in [
        (1, "meta"),
        (2, "meta"),
        (3, "removed_resource"),
        (4, "meta"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (seq, kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES (?, 'state', ?, ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(seq)
        .bind(resource)
        .bind(format!("legacy-gap-{seq}"))
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert legacy-gap event");
    }
    pool.close().await;

    proxy
        .ack_ha_peer_watermark(HaSyncChannel::Control, "standby-legacy-gap", 2)
        .await
        .expect("ack watermark");
    proxy
        .gc_ha_outbox_with_options(HaOutboxGcOptions {
            batch_size: 10,
            max_batches: 8,
            max_runtime_secs: 20,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("gc legacy event");
    let health = proxy
        .ha_peer_channel_health(HaSyncChannel::Control, "standby-legacy-gap")
        .await
        .expect("read channel health");
    assert_eq!(health.cursor_state, "catching_up");
    assert!(!health.expired_backlog);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn standalone_ha_outbox_gc_deletes_invalid_legacy_rows_before_retention_rows() {
    let db_path = temp_db_path("ha-outbox-gc-invalid-legacy-first");
    let db_str = db_path.to_string_lossy().to_string();
    let old_control_ts = Utc::now().timestamp() - (4 * SECS_PER_DAY);
    let recent_ts = Utc::now().timestamp();

    let proxy = TavilyProxy::with_options_in_ha_mode(
        vec!["tvly-ha-invalid-gc-key".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        HaMode::ActiveStandby,
    )
    .await
    .expect("proxy created");
    drop(proxy);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (
            kind, resource, resource_id, op, payload_json, created_at, checksum
        ) VALUES
            ('state', 'scheduled_jobs', 'legacy-1', 'upsert', '{}', ?, NULL),
            ('state', 'scheduled_jobs', 'legacy-2', 'upsert', '{}', ?, NULL),
            ('state', 'users', 'old-user', 'upsert', '{}', ?, NULL),
            ('state', 'users', 'recent-user', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(recent_ts)
    .bind(recent_ts + 1)
    .bind(old_control_ts)
    .bind(recent_ts + 2)
    .execute(&pool)
    .await
    .expect("seed control outbox rows");
    drop(pool);

    let report = run_ha_outbox_gc_once(
        &db_str,
        HaOutboxGcOptions {
            batch_size: 10,
            max_batches: 2,
            max_runtime_secs: 30,
            inter_batch_sleep_ms: 0,
        },
    )
    .await
    .expect("run standalone ha outbox gc");

    let control = report
        .channels
        .iter()
        .find(|channel| channel.channel == HaSyncChannel::Control)
        .expect("control report");
    assert_eq!(control.invalid_legacy_deleted_rows, 2);
    assert_eq!(control.retention_deleted_rows, 1);
    assert_eq!(control.deleted_rows, 3);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("reopen sqlite pool");
    let legacy_remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'scheduled_jobs'")
            .fetch_one(&pool)
            .await
            .expect("count legacy rows");
    let old_allowed_remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ha_outbox WHERE resource = 'users' AND created_at < ?",
    )
    .bind(recent_ts - SECS_PER_DAY)
    .fetch_one(&pool)
    .await
    .expect("count old allowed rows");
    let recent_allowed_remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'users'")
            .fetch_one(&pool)
            .await
            .expect("count remaining allowed rows");
    assert_eq!(legacy_remaining, 0);
    assert_eq!(old_allowed_remaining, 0);
    assert_eq!(recent_allowed_remaining, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn sqlite_db_stats_reports_reclaimable_shape() {
    let db_path = temp_db_path("sqlite-db-stats-shape");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let stats = proxy.sqlite_db_stats().await.expect("db stats");
    assert!(stats.page_size > 0);
    assert!(stats.page_count > 0);
    assert!(stats.database_bytes > 0);
    assert!(stats.reclaimable_ratio >= 0.0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn db_compaction_once_skips_when_reclaimable_space_is_below_threshold() {
    let db_path = temp_db_path("db-compaction-once-skips-below-threshold");
    let db_str = db_path.to_string_lossy().to_string();
    let _proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let report = run_db_compaction_once(&db_str, false)
        .await
        .expect("db compaction report");
    assert!(report.skipped);
    assert!(!report.forced);
    assert!(report.reason.is_some());
    assert_eq!(report.before.database_bytes, report.after.database_bytes);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn db_compaction_once_force_runs_even_below_threshold() {
    let db_path = temp_db_path("db-compaction-once-force");
    let db_str = db_path.to_string_lossy().to_string();
    let _proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let report = run_db_compaction_once(&db_str, true)
        .await
        .expect("forced db compaction report");
    assert!(!report.skipped);
    assert!(report.forced);
    assert!(report.reason.is_none());
    assert!(report.after.database_bytes > 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

use super::*;

#[tokio::test]
async fn versioned_schema_migrations_are_idempotent_and_fail_closed_on_drift() {
    let db_path = temp_db_path("versioned-schema-migrations");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migrations".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("bootstrap database");
    drop(proxy);

    let reopened = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migrations".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("reopen migrated database");
    drop(reopened);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .expect("read migration ledger");
    assert_eq!(versions, vec![1, 2, 3]);
    sqlx::query("UPDATE schema_migrations SET checksum = 'drifted' WHERE version = 2")
        .execute(&pool)
        .await
        .expect("corrupt migration checksum");
    pool.close().await;

    let error = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migrations".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect_err("checksum drift must reject startup");
    assert!(error.to_string().contains("checksum mismatch"));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn versioned_schema_migrations_reject_missing_recorded_objects() {
    let db_path = temp_db_path("schema-migration-missing-object");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-object".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&pool)
        .await
        .expect("remove recorded migration object");
    pool.close().await;

    let error = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-object".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect_err("missing recorded migration object must reject startup");
    assert!(
        error
            .to_string()
            .contains("object validation failed at version 3")
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn versioned_schema_migrations_reject_unknown_future_versions() {
    let db_path = temp_db_path("schema-migration-future-version");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-future-version".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    sqlx::query(
        "INSERT INTO schema_migrations (version, name, checksum, applied_at) VALUES (99, 'future', 'sha256:future', 1)",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("record a future migration");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("older binaries must reject unknown migration versions");
    assert!(error.to_string().contains("unknown version 99"));

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn baseline_adoption_records_compatible_existing_schema_without_full_bootstrap() {
    let db_path = temp_db_path("schema-migration-compatible-adoption");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-compatible-adoption".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create compatible database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-ledger production database");

    assert!(
        !proxy
            .key_store
            .prepare_versioned_schema()
            .await
            .expect("adopt compatible database"),
        "compatible adoption must enter the warm path"
    );
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(&proxy.key_store.pool)
            .await
            .expect("read adopted ledger");
    assert_eq!(versions, vec![1, 2, 3]);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn baseline_adoption_rejects_runtime_schema_drift() {
    let db_path = temp_db_path("schema-migration-incomplete-baseline");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-incomplete-baseline".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create baseline database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-ledger production database");
    sqlx::query(
        "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, checksum TEXT NOT NULL, applied_at INTEGER NOT NULL)",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate interruption after ledger creation but before baseline record");
    sqlx::query("ALTER TABLE users DROP COLUMN debug_info_shared")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove a runtime-required historical column");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("runtime schema drift must reject adoption");
    assert!(
        error
            .to_string()
            .contains("missing main.users.debug_info_shared")
    );
    let recorded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("check interrupted ledger");
    assert_eq!(recorded, 0, "rejected drift must not record a baseline");

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn baseline_adoption_rejects_missing_source_schema_before_recording() {
    let db_path = temp_db_path("schema-migration-missing-source");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-source".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create baseline database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-ledger production database");
    sqlx::query("ALTER TABLE billing_ledger RENAME TO billing_ledger_missing")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove an irreplaceable source table");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("missing source data schema must reject adoption");
    assert!(
        error
            .to_string()
            .contains("missing source table main.billing_ledger")
    );
    let ledger_exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("check migration ledger absence");
    assert_eq!(
        ledger_exists, 0,
        "rejected source schemas must not be recorded"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_verification_is_read_only_and_rejects_runtime_column_drift() {
    let db_path = temp_db_path("schema-migration-warm-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-warm-read-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    let mut lock = lock_pool
        .acquire()
        .await
        .expect("acquire writer lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .expect("hold writer lock");
    let verified = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        proxy.key_store.prepare_versioned_schema(),
    )
    .await
    .expect("warm verification must not wait for the writer")
    .expect("warm verification succeeds");
    assert!(!verified);
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    sqlx::query("ALTER TABLE users DROP COLUMN debug_info_shared")
        .execute(&proxy.key_store.pool)
        .await
        .expect("corrupt a required runtime column");
    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("warm verification must reject runtime column drift");
    assert!(
        error
            .to_string()
            .contains("missing main.users.debug_info_shared")
    );
    proxy
        .key_store
        .initialize_schema()
        .await
        .expect("repair the user column before checking request log drift");
    sqlx::query("ALTER TABLE observability.request_logs DROP COLUMN forwarded_headers")
        .execute(&proxy.key_store.pool)
        .await
        .expect("corrupt a request-log write column");
    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("warm verification must reject request-log write column drift");
    assert!(
        error
            .to_string()
            .contains("missing observability.request_logs.forwarded_headers")
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

use super::*;

fn resolve_test_binary(file_name: &str) -> std::path::PathBuf {
    let current_exe = std::env::current_exe().expect("resolve current test executable");
    if let Some(debug_dir) = current_exe.parent().and_then(|deps| deps.parent()) {
        let direct = debug_dir.join(file_name);
        if direct.is_file() {
            return direct;
        }
    }
    if let Some(parent) = current_exe.parent() {
        let direct = parent.join(file_name);
        if direct.is_file() {
            return direct;
        }
        if let Ok(entries) = std::fs::read_dir(parent) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if name == file_name || name.starts_with(&format!("{file_name}-")) {
                    #[cfg(unix)]
                    let executable = {
                        use std::os::unix::fs::PermissionsExt;
                        path.metadata()
                            .map(|metadata| {
                                metadata.is_file() && (metadata.permissions().mode() & 0o111) != 0
                            })
                            .unwrap_or(false)
                    };
                    #[cfg(not(unix))]
                    let executable = path.is_file();
                    if executable {
                        return path;
                    }
                }
            }
        }
    }
    panic!("unable to resolve sibling test binary {file_name}");
}

#[tokio::test]
async fn large_legacy_single_db_request_logs_stay_in_core_database_for_startup() {
    let db_path = temp_db_path("observability-sidecar-large-legacy-compat");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create api_keys");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            auth_token_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            query TEXT,
            status_code INTEGER,
            error_message TEXT,
            result_status TEXT NOT NULL DEFAULT 'success',
            request_body BLOB,
            response_body BLOB,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-large-legacy-sidecar', 1, 1)",
    )
    .execute(&mut conn)
    .await
    .expect("insert api key");
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            error_message,
            result_status,
            request_body,
            response_body,
            visibility,
            created_at
        ) VALUES ('k1', NULL, 'POST', '/api/tavily/search', NULL, 200, NULL, 'success', NULL, NULL, 'visible', 1710000000)
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("insert legacy request log");
    drop(conn);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let attached_path = proxy
        .sqlite_observability_database_path()
        .expect("observability database attached");
    assert!(
        sqlite_paths_match(attached_path, &db_str),
        "large legacy databases should keep observability attached to the core file on startup"
    );

    let first_pool_connection = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("acquire first large-legacy pool connection");
    let second_pool_connection = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        proxy.key_store.pool.acquire(),
    )
    .await
    .expect("large legacy compatibility should keep the default sqlite pool capacity")
    .expect("acquire second large-legacy pool connection");

    let main_request_logs_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main request_logs")
    .is_some();
    assert!(
        main_request_logs_exists,
        "large legacy request_logs should stay in the core database during startup"
    );

    let observed_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count observable request logs");
    assert_eq!(observed_rows, 1);

    drop(second_pool_connection);
    drop(first_pool_connection);
    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let observability_path = layout
        .observability_database_path
        .expect("sidecar path should still be derivable");
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "legacy compatibility startup should not create a sidecar file"
    );
}

#[tokio::test]
async fn legacy_request_logs_backfill_uses_main_schema_when_sidecar_table_already_exists() {
    let db_path = temp_db_path("observability-sidecar-preseeded-request-logs");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create api_keys");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-preseeded-sidecar', 1, 1)",
    )
    .execute(&mut conn)
    .await
    .expect("insert api key");
    sqlx::query(
        r#"
        INSERT INTO request_logs (api_key_id, method, path, created_at)
        VALUES ('k1', 'POST', '/api/tavily/search', 1710000000)
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("insert legacy request log");
    drop(conn);

    let mut observability_conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(true),
    )
    .await
    .expect("open preseeded sidecar sqlite");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            auth_token_id TEXT,
            request_user_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            query TEXT,
            status_code INTEGER,
            tavily_status_code INTEGER,
            error_message TEXT,
            result_status TEXT NOT NULL DEFAULT 'unknown',
            request_kind_key TEXT,
            request_kind_label TEXT,
            request_kind_detail TEXT,
            counts_business_quota INTEGER,
            business_credits INTEGER,
            failure_kind TEXT,
            key_effect_code TEXT NOT NULL DEFAULT 'none',
            key_effect_summary TEXT,
            binding_effect_code TEXT NOT NULL DEFAULT 'none',
            binding_effect_summary TEXT,
            selection_effect_code TEXT NOT NULL DEFAULT 'none',
            selection_effect_summary TEXT,
            gateway_mode TEXT,
            experiment_variant TEXT,
            proxy_session_id TEXT,
            routing_subject_hash TEXT,
            upstream_operation TEXT,
            fallback_reason TEXT,
            request_body BLOB,
            response_body BLOB,
            request_body_bytes INTEGER,
            response_body_bytes INTEGER,
            request_body_sha256 TEXT,
            response_body_sha256 TEXT,
            body_retention_days INTEGER,
            body_retention_profile TEXT,
            body_cleaned_reason TEXT,
            body_cleaned_at INTEGER,
            forwarded_headers TEXT,
            dropped_headers TEXT,
            remote_addr TEXT,
            client_ip TEXT,
            client_ip_source TEXT,
            client_ip_trusted INTEGER NOT NULL DEFAULT 0,
            ip_headers TEXT,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&mut observability_conn)
    .await
    .expect("create preseeded sidecar request_logs");
    drop(observability_conn);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let migrated_row = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT api_key_id, path, proxy_session_id
        FROM observability.request_logs
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch migrated request log");
    assert_eq!(migrated_row.0, "k1");
    assert_eq!(migrated_row.1, "/api/tavily/search");
    assert_eq!(
        migrated_row.2, None,
        "backfill should use the legacy main schema instead of probing sidecar-only columns"
    );

    let main_request_logs_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main request_logs after preseeded migration")
    .is_some();
    assert!(
        !main_request_logs_exists,
        "legacy main request_logs should still be removed after migrating into a preseeded sidecar"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn small_legacy_sidecar_migration_rebuilds_partial_derived_tables() {
    let db_path = temp_db_path("observability-sidecar-partial-derived-auto");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open partial legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create api_keys");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            request_kind_key TEXT,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        r#"
        CREATE TABLE api_key_usage_buckets (
            api_key_id TEXT NOT NULL,
            bucket_start INTEGER NOT NULL,
            bucket_secs INTEGER NOT NULL,
            total_requests INTEGER NOT NULL,
            success_count INTEGER NOT NULL,
            error_count INTEGER NOT NULL,
            quota_exhausted_count INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (api_key_id, bucket_start, bucket_secs)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create only one legacy derived table");
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-partial-derived-auto', 1, 1)",
    )
    .execute(&mut conn)
    .await
    .expect("insert api key");
    sqlx::query(
        r#"
        INSERT INTO request_logs (api_key_id, method, path, result_status, request_kind_key, visibility, created_at)
        VALUES ('k1', 'POST', '/api/tavily/search', 'success', 'api:search', 'visible', 1710000000)
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("insert legacy request log");
    drop(conn);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy should migrate partial legacy derived tables");

    let main_request_logs_exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main request_logs");
    assert!(main_request_logs_exists.is_none());

    let main_api_key_usage_exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'api_key_usage_buckets' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main partial legacy derived table");
    assert!(main_api_key_usage_exists.is_none());

    let sidecar_api_key_usage_exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM observability.sqlite_master WHERE type = 'table' AND name = 'api_key_usage_buckets' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check sidecar api key usage table");
    assert_eq!(sidecar_api_key_usage_exists, Some(1));

    let api_key_usage_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM observability.api_key_usage_buckets")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count rebuilt api key usage rows");
    assert_eq!(api_key_usage_rows, 1);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_moves_large_legacy_request_logs_offline() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-large-legacy");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create api_keys");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            failure_kind TEXT,
            request_kind_key TEXT,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        r#"
        CREATE TABLE api_key_maintenance_records (
            id TEXT PRIMARY KEY,
            key_id TEXT NOT NULL,
            source TEXT NOT NULL,
            operation_code TEXT NOT NULL,
            operation_summary TEXT NOT NULL,
            reason_code TEXT,
            reason_summary TEXT,
            reason_detail TEXT,
            request_log_id INTEGER,
            auth_token_log_id INTEGER,
            auth_token_id TEXT,
            actor_user_id TEXT,
            actor_display_name TEXT,
            status_before TEXT,
            status_after TEXT,
            quarantine_before INTEGER NOT NULL DEFAULT 0,
            quarantine_after INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create maintenance records");
    sqlx::query(
        r#"
        CREATE TABLE api_key_transient_backoffs (
            key_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            cooldown_until INTEGER NOT NULL,
            retry_after_secs INTEGER NOT NULL,
            reason_code TEXT,
            source_request_log_id INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (key_id, scope)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create transient backoffs");
    sqlx::query(
        r#"
        CREATE TABLE billing_ledger (
            auth_token_log_id INTEGER PRIMARY KEY,
            token_id TEXT NOT NULL,
            billing_subject TEXT,
            billing_state TEXT NOT NULL DEFAULT 'none',
            business_credits INTEGER,
            request_user_id TEXT,
            api_key_id TEXT,
            request_log_id INTEGER,
            result_status TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            settled_at INTEGER,
            error_message TEXT
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create billing_ledger");
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-explicit-migrate', 1, 1)",
    )
    .execute(&pool)
    .await
    .expect("insert api key");
    for (id, created_at) in [(1_i64, 100_i64), (2, 200), (4, 400)] {
        sqlx::query(
            r#"
            INSERT INTO request_logs (id, api_key_id, method, path, result_status, failure_kind, request_kind_key, visibility, created_at)
            VALUES (?, 'k1', 'POST', '/api/tavily/search', 'success', NULL, 'api:search', 'visible', ?)
            "#,
        )
        .bind(id)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("seed request log");
    }
    sqlx::query(
        "INSERT INTO request_logs (id, api_key_id, method, path, result_status, failure_kind, request_kind_key, visibility, created_at) VALUES (3, 'k1', 'POST', '/api/tavily/search', 'error', 'upstream_rate_limited_429', 'api:search', 'visible', 300)",
    )
    .execute(&pool)
    .await
    .expect("seed 429 request log");
    sqlx::query(
        "INSERT INTO api_key_maintenance_records (id, key_id, source, operation_code, operation_summary, request_log_id, created_at) VALUES ('m1', 'k1', 'auto', 'noop', 'noop', 2, 1)",
    )
    .execute(&pool)
    .await
    .expect("seed maintenance");
    sqlx::query(
        "INSERT INTO api_key_transient_backoffs (key_id, scope, cooldown_until, retry_after_secs, source_request_log_id, created_at, updated_at) VALUES ('k1', 'scope', 1, 1, 4, 1, 1)",
    )
    .execute(&pool)
    .await
    .expect("seed backoff");
    sqlx::query(
        "INSERT INTO billing_ledger (auth_token_log_id, token_id, request_log_id, result_status, created_at) VALUES (1, 'token-a', 1, 'success', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed billing");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let dry_run = run_observability_sidecar_migrate(&db_str, 2, true)
        .await
        .expect("dry run succeeds");
    assert!(dry_run.large_legacy_fallback_active);
    assert!(dry_run.legacy_request_logs_exists);
    assert!(dry_run.offline_lock_acquired);
    assert!(dry_run.sqlite_write_probe_ok);
    assert!(!std::path::Path::new(&observability_path).exists());

    let report = run_observability_sidecar_migrate(&db_str, 2, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);
    assert!(report.startup_reopen_verified);
    assert_eq!(report.copied_request_logs, 4);
    assert_eq!(report.batches, 2);
    assert!(report.dropped_main_request_logs);
    assert!(report.rebuilt_api_key_usage_buckets);
    assert!(report.rebuilt_dashboard_request_rollup_buckets);
    assert!(report.rebuilt_request_log_catalog_rollups);
    assert!(report.marked_api_key_usage_buckets_meta_complete);
    assert!(report.marked_dashboard_request_rollup_buckets_meta_complete);
    assert!(report.marked_request_log_catalog_rollup_meta_complete);
    assert!(!report.startup_rebuild_required);
    assert!(report.derived_rebuild_elapsed_ms > 0);
    assert!(report.child_reference_checks_passed);
    assert!(std::path::Path::new(&observability_path).exists());

    let migrated_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(false),
    )
    .await
    .expect("open migrated observability sidecar pool");
    let api_key_usage_429_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COALESCE(SUM(valuable_failure_429_count), 0)
        FROM api_key_usage_buckets
        WHERE api_key_id = 'k1'
        "#,
    )
    .fetch_one(&migrated_pool)
    .await
    .expect("query api key usage 429 count");
    assert_eq!(api_key_usage_429_count, 1);
    drop(migrated_pool);

    let rerun = run_observability_sidecar_migrate(&db_str, 2, false)
        .await
        .expect("rerun stays idempotent");
    assert_eq!(rerun.copied_request_logs, 0);
    assert!(rerun.already_migrated);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy opens migrated db");
    let migrated_catalog_retention_days = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1",
    )
    .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read migrated catalog retention meta");
    assert_eq!(
        migrated_catalog_retention_days,
        effective_request_logs_retention_days(),
        "migration should preserve the current catalog retention window after resetting rebuild markers"
    );
    let explicit_cutover_done = sqlx::query_scalar::<_, i64>(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1",
    )
    .bind(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read explicit sidecar cutover marker");
    assert_eq!(explicit_cutover_done, 1);
    let attached_path = proxy
        .sqlite_observability_database_path()
        .expect("observability database attached");
    assert!(
        sqlite_paths_match(attached_path, &observability_path),
        "migrated startup should attach the sibling sidecar file"
    );
    let migrated_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count migrated rows");
    assert_eq!(migrated_rows, 4);
    let main_request_logs_exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main request_logs after migration");
    assert!(main_request_logs_exists.is_none());
    drop(proxy);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_rejects_live_service_lock_holder() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-live-lock-holder");
    let db_str = db_path.to_string_lossy().to_string();
    let _proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let helper_path = resolve_test_binary("observability_lock_holder");
    let mut holder = std::process::Command::new(helper_path)
        .args(["--db-path", &db_str])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn observability lock holder");
    let stdout = holder.stdout.take().expect("lock holder stdout");
    let mut reader = std::io::BufReader::new(stdout);
    let mut ready = String::new();
    std::io::BufRead::read_line(&mut reader, &mut ready).expect("read lock holder ready");
    assert_eq!(ready.trim(), "lock-held");

    let dry_run = run_observability_sidecar_migrate(&db_str, 2, true)
        .await
        .expect("dry run while service is live still reports state");
    assert!(!dry_run.offline_lock_acquired);

    let err = run_observability_sidecar_migrate(&db_str, 2, false)
        .await
        .expect_err("live service lock holder must block migration");
    let message = err.to_string();
    assert!(
        message.contains("exclusive observability service lock"),
        "unexpected live-service migration error: {message}"
    );

    drop(reader);
    drop(holder.stdin.take());
    let status = holder.wait().expect("wait for lock holder exit");
    assert!(status.success(), "lock holder exit status: {status}");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(crate::store::sqlite_lock_sidecar_path(&db_str));
}

#[tokio::test]
async fn observability_sidecar_migrate_closes_pool_before_same_process_reopen() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-reopen");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("same-process reopen succeeds immediately after migration");
    let attached_path = reopened
        .sqlite_observability_database_path()
        .expect("observability database attached");
    assert!(
        sqlite_paths_match(attached_path, &observability_path),
        "reopened proxy should attach the sibling sidecar file"
    );
    drop(reopened);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
    let _ = std::fs::remove_file(crate::store::sqlite_lock_sidecar_path(&db_str));
}

#[tokio::test]
async fn observability_sidecar_migrate_blocks_startup_before_opening_sqlite() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-startup-race");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let _offline_guard =
        crate::store::acquire_observability_offline_guard(&db_str).expect("hold offline guard");
    let startup_err = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect_err("startup should fail while offline guard is held");
    let message = startup_err.to_string();
    assert!(
        message.contains("shared observability service lock"),
        "unexpected startup error while offline guard is held: {message}"
    );
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "startup must not create the sidecar file before it acquires the shared lock"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
    let _ = std::fs::remove_file(crate::store::sqlite_lock_sidecar_path(&db_str));
}

#[tokio::test]
async fn observability_sidecar_migrate_resumes_copy_from_preseeded_sidecar_gaps() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-resume-gaps");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let store = KeyStore::new(&db_str).await.expect("keystore created");
    drop(store);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    for (id, path, created_at) in [
        (1_i64, "/api/tavily/search", 100_i64),
        (2, "/api/tavily/extract", 200),
        (4, "/mcp", 400),
    ] {
        sqlx::query(
            r#"
            INSERT INTO request_logs (id, method, path, result_status, visibility, created_at)
            VALUES (?, 'POST', ?, 'success', 'visible', ?)
            "#,
        )
        .bind(id)
        .bind(path)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("seed legacy request log");
    }
    drop(pool);

    let observability_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(false),
    )
    .await
    .expect("open sidecar pool");
    sqlx::query(
        r#"
        INSERT INTO request_logs (id, method, path, result_status, visibility, created_at)
        VALUES (2, 'POST', '/api/tavily/extract', 'success', 'visible', 200)
        "#,
    )
    .execute(&observability_pool)
    .await
    .expect("preseed migrated row");
    drop(observability_pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration resumes copy");
    assert!(report.completed);
    assert!(report.resumed_copy);
    assert_eq!(report.sidecar_request_log_rows_before, 1);
    assert_eq!(report.copied_request_logs, 2);
    assert_eq!(report.sidecar_request_log_rows_after, 3);
    assert_eq!(report.batches, 2);
    assert!(report.dropped_main_request_logs);
    assert!(report.rebuilt_api_key_usage_buckets);
    assert!(report.rebuilt_dashboard_request_rollup_buckets);
    assert!(report.rebuilt_request_log_catalog_rollups);
    assert!(!report.startup_rebuild_required);

    let rerun = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("rerun stays idempotent");
    assert_eq!(rerun.copied_request_logs, 0);
    assert!(rerun.already_migrated);
    assert!(rerun.completed);
    assert!(!rerun.startup_rebuild_required);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("reopen migrated db");
    let ids =
        sqlx::query_scalar::<_, i64>("SELECT id FROM observability.request_logs ORDER BY id ASC")
            .fetch_all(&reopened.key_store.pool)
            .await
            .expect("fetch migrated ids");
    assert_eq!(ids, vec![1, 2, 4]);
    drop(reopened);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_recovers_temporary_legacy_tables() {
    let db_path = temp_db_path("observability-sidecar-temp-legacy-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (id, method, path, created_at) VALUES (7, 'POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    sqlx::query(
        r#"CREATE TABLE "__observability_sidecar_legacy_request_logs" (id INTEGER PRIMARY KEY)"#,
    )
    .execute(&pool)
    .await
    .expect("create leftover temp request_logs table");
    sqlx::query(r#"INSERT INTO "__observability_sidecar_legacy_request_logs" (id) VALUES (7)"#)
        .execute(&pool)
        .await
        .expect("seed leftover temp request_logs table");
    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?, ?, ?)")
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
        .bind(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
        .execute(&pool)
        .await
        .expect("clear completion meta");
    drop(pool);

    let recovered = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration recovers temp table");
    assert!(recovered.completed);
    assert!(!recovered.already_migrated);
    assert_eq!(recovered.copied_request_logs, 0);
    assert!(recovered.dropped_main_request_logs);
    assert!(!recovered.startup_rebuild_required);

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool after recovery");
    let temp_exists: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '__observability_sidecar_legacy_request_logs'",
    )
    .fetch_optional(&pool)
    .await
    .expect("check temp table");
    assert_eq!(temp_exists, None);
    drop(pool);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_rebuilds_after_legacy_tables_dropped_before_meta() {
    let db_path = temp_db_path("observability-sidecar-dropped-before-meta");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            api_key_id TEXT,
            request_kind_key TEXT,
            request_body BLOB,
            business_credits INTEGER,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (id, method, path, api_key_id, request_kind_key, business_credits, created_at) VALUES (11, 'POST', '/api/tavily/search', 'key-a', 'api:search', 1, 120)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let meta_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?, ?, ?)")
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
        .bind(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
        .execute(&meta_pool)
        .await
        .expect("clear completion meta");
    drop(meta_pool);

    let recovered = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration rebuilds dropped-before-meta state");
    assert!(recovered.completed);
    assert!(!recovered.already_migrated);
    assert_eq!(recovered.copied_request_logs, 0);
    assert!(recovered.rebuilt_api_key_usage_buckets);
    assert!(recovered.rebuilt_dashboard_request_rollup_buckets);
    assert!(recovered.rebuilt_request_log_catalog_rollups);
    assert!(recovered.marked_api_key_usage_buckets_meta_complete);
    assert!(recovered.marked_dashboard_request_rollup_buckets_meta_complete);
    assert!(recovered.marked_request_log_catalog_rollup_meta_complete);
    assert!(!recovered.startup_rebuild_required);

    let restarted = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("startup accepts recovered cutover");
    drop(restarted);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_rebuilds_when_completion_meta_is_zero() {
    let db_path = temp_db_path("observability-sidecar-zero-meta");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            api_key_id TEXT,
            request_kind_key TEXT,
            request_body BLOB,
            business_credits INTEGER,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (id, method, path, api_key_id, request_kind_key, business_credits, created_at) VALUES (13, 'POST', '/api/tavily/search', 'key-a', 'api:search', 1, 120)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let meta_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    for key in [
        META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE,
        META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE,
        META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE,
        META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE,
    ] {
        sqlx::query("UPDATE meta SET value = '0' WHERE key = ?")
            .bind(key)
            .execute(&meta_pool)
            .await
            .expect("zero completion meta");
    }
    drop(meta_pool);

    let startup_err = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect_err("startup should reject zero completion meta");
    assert!(
        startup_err
            .to_string()
            .contains("observability sidecar derived tables are incomplete"),
        "unexpected startup error: {startup_err}"
    );

    let recovered = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration rebuilds zero-meta state");
    assert!(recovered.completed);
    assert!(!recovered.already_migrated);
    assert!(recovered.marked_api_key_usage_buckets_meta_complete);
    assert!(recovered.marked_dashboard_request_rollup_buckets_meta_complete);
    assert!(recovered.marked_request_log_catalog_rollup_meta_complete);
    assert!(!recovered.startup_rebuild_required);

    let restarted = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("startup accepts recovered zero-meta cutover");
    drop(restarted);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_rejects_startup_when_cutover_completed_but_meta_missing() {
    let db_path = temp_db_path("observability-sidecar-cutover-meta-missing");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let meta_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?, ?)")
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
        .execute(&meta_pool)
        .await
        .expect("clear completion meta");
    drop(meta_pool);

    let startup_err = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect_err("startup should reject missing completion meta");
    assert!(
        startup_err
            .to_string()
            .contains("observability sidecar derived tables are incomplete"),
        "unexpected startup error: {startup_err}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_reports_reopen_verification_before_completion() {
    let db_path = temp_db_path("observability-sidecar-reopen-verified");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);
    assert!(report.startup_reopen_verified);

    let restarted = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("startup accepts reopened cutover");
    drop(restarted);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_rejects_startup_when_explicit_marker_missing_but_cutover_visible() {
    let db_path = temp_db_path("observability-sidecar-cutover-marker-missing");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let meta_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?, ?, ?)")
        .bind(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
        .execute(&meta_pool)
        .await
        .expect("clear explicit marker and completion meta");
    drop(meta_pool);

    let startup_err = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect_err("startup should reject visible cutover without explicit marker");
    assert!(
        startup_err
            .to_string()
            .contains("observability sidecar derived tables are incomplete"),
        "unexpected startup error: {startup_err}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_rejects_explicit_cutover_before_small_self_heal() {
    let db_path = temp_db_path("observability-sidecar-cutover-self-heal-order");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let meta_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?, ?, ?)")
        .bind(META_KEY_OBSERVABILITY_SIDECAR_EXPLICIT_CUTOVER_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_V1_DONE)
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .bind(META_KEY_DASHBOARD_REQUEST_ROLLUP_BUCKETS_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_DONE)
        .bind(META_KEY_REQUEST_LOG_CATALOG_ROLLUP_V1_RETENTION_DAYS)
        .execute(&meta_pool)
        .await
        .expect("clear completion meta");
    drop(meta_pool);

    let startup_err = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect_err("startup should reject interrupted cutover before self-heal");
    assert!(
        startup_err
            .to_string()
            .contains("observability sidecar derived tables are incomplete"),
        "unexpected startup error: {startup_err}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_rejects_large_startup_schema_drift_rebuild() {
    let db_path = temp_db_path("observability-sidecar-cutover-schema-drift");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open sqlite pool");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed request log");
    drop(pool);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let report = run_observability_sidecar_migrate(&db_str, 1, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);

    let meta_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false),
    )
    .await
    .expect("open core pool");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .execute(&meta_pool)
        .await
        .expect("clear request-value completion meta");
    drop(meta_pool);

    let startup_err = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect_err("startup should reject large schema-drift rebuild");
    assert!(
        startup_err
            .to_string()
            .contains("observability sidecar derived tables are incomplete")
            || startup_err
                .to_string()
                .contains("observability sidecar derived tables require"),
        "unexpected startup error: {startup_err}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn observability_sidecar_migrate_rejects_missing_db_path_without_creating_files() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-missing-db");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");
    let lock_path = sqlite_lock_sidecar_path(&db_str);

    assert!(!db_path.exists(), "fixture should start with no core db");
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "fixture should start with no sidecar db"
    );

    let dry_run_err = run_observability_sidecar_migrate(&db_str, 2, true)
        .await
        .expect_err("dry-run must reject a missing core db path");
    assert!(
        dry_run_err.to_string().contains("missing core database"),
        "unexpected dry-run missing-db error: {dry_run_err}"
    );
    assert!(!db_path.exists(), "dry-run must not create a core db");
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "dry-run must not create a sidecar db"
    );
    assert!(
        !std::path::Path::new(&lock_path).exists(),
        "dry-run must not create an offline lock file"
    );

    let run_err = run_observability_sidecar_migrate(&db_str, 2, false)
        .await
        .expect_err("migration must reject a missing core db path");
    assert!(
        run_err.to_string().contains("missing core database"),
        "unexpected missing-db migration error: {run_err}"
    );
    assert!(!db_path.exists(), "migration must not create a core db");
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "migration must not create a sidecar db"
    );
    assert!(
        !std::path::Path::new(&lock_path).exists(),
        "migration must not create an offline lock file"
    );
}

#[tokio::test]
async fn observability_sidecar_migrate_dry_run_reports_startup_attach_for_small_legacy_db() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-small-legacy");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");
    let lock_path = crate::store::sqlite_lock_sidecar_path(&db_str);

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&mut conn)
    .await
    .expect("seed request log");
    drop(conn);

    let dry_run = run_observability_sidecar_migrate(&db_str, 2, true)
        .await
        .expect("dry run succeeds");
    assert!(!dry_run.large_legacy_fallback_active);
    assert_eq!(dry_run.attached_observability_path, observability_path);
    assert!(dry_run.legacy_request_logs_exists);
    assert!(!std::path::Path::new(&observability_path).exists());
    assert!(
        !std::path::Path::new(&lock_path).exists(),
        "dry-run must not create an offline lock file when probing an existing DB"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
    let _ = std::fs::remove_file(lock_path);
}

#[tokio::test]
async fn observability_sidecar_migrate_dry_run_tolerates_read_only_small_legacy_snapshot() {
    let db_path = temp_db_path("observability-sidecar-explicit-migrate-readonly-small-legacy");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");
    let lock_path = crate::store::sqlite_lock_sidecar_path(&db_str);

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO request_logs (method, path, created_at) VALUES ('POST', '/api/tavily/search', 1)",
    )
    .execute(&mut conn)
    .await
    .expect("seed request log");
    drop(conn);

    let metadata = std::fs::metadata(&db_path).expect("stat db");
    let mut perms = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o444);
    }
    std::fs::set_permissions(&db_path, perms).expect("make db read-only");

    let dry_run = run_observability_sidecar_migrate(&db_str, 2, true)
        .await
        .expect("dry run should still report against read-only snapshot");
    assert!(!dry_run.large_legacy_fallback_active);
    assert_eq!(dry_run.attached_observability_path, observability_path);
    assert!(!dry_run.sqlite_write_probe_ok);
    assert!(dry_run.legacy_request_logs_exists);
    assert!(!std::path::Path::new(&observability_path).exists());
    assert!(
        !std::path::Path::new(&lock_path).exists(),
        "dry-run must not create an offline lock file for read-only snapshots"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
    let _ = std::fs::remove_file(lock_path);
}

#[tokio::test]
async fn heal_orphan_auth_tokens_from_logs_creates_soft_deleted_token() {
    let db_path = temp_db_path("heal-orphan");
    let db_str = db_path.to_string_lossy().to_string();

    // Initialize schema.
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    // Insert an auth_token_logs entry for a token id that does not exist in auth_tokens.
    let orphan_token_id = "ZZZZ";
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1234567890)
        "#,
    )
    .bind(orphan_token_id)
    .execute(&store.pool)
    .await
    .expect("insert orphan log");

    // Clear healer meta key so that we can invoke the healer path again for this test.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_HEAL_ORPHAN_TOKENS_V1)
        .execute(&store.pool)
        .await
        .expect("delete meta gate");

    // Run healer directly.
    store
        .heal_orphan_auth_tokens_from_logs()
        .await
        .expect("heal orphan tokens");

    // Verify that a soft-deleted auth_tokens row was created for the orphan id.
    let (enabled, total_requests, deleted_at): (i64, i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, total_requests, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(orphan_token_id)
            .fetch_one(&store.pool)
            .await
            .expect("restored token row");

    assert_eq!(enabled, 0, "restored token should be disabled");
    assert_eq!(
        total_requests, 1,
        "restored token should count orphan log entries"
    );
    assert!(
        deleted_at.is_some(),
        "restored token should be marked soft-deleted"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn published_announcement_update_archives_previous_version() {
    let db_path = temp_db_path("announcement-published-update-archives");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    let draft = store
        .create_announcement(AnnouncementMutation {
            title: "Initial notice".to_string(),
            body: "Initial body".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_MODAL.to_string(),
        })
        .await
        .expect("create announcement");
    assert_eq!(draft.status, ANNOUNCEMENT_STATUS_DRAFT);
    assert!(
        store
            .list_user_active_announcements()
            .await
            .expect("list active before publish")
            .is_empty()
    );

    let published = store
        .publish_announcement(&draft.id)
        .await
        .expect("publish announcement")
        .expect("published announcement exists");
    assert_eq!(published.status, ANNOUNCEMENT_STATUS_PUBLISHED);
    assert!(published.published_at.is_some());

    let revised = store
        .update_announcement(
            &published.id,
            AnnouncementMutation {
                title: "Updated notice".to_string(),
                body: "Updated body".to_string(),
                display_kind: ANNOUNCEMENT_DISPLAY_MODAL.to_string(),
            },
        )
        .await
        .expect("update published announcement")
        .expect("updated announcement exists");
    assert_ne!(revised.id, published.id);
    assert_eq!(revised.status, ANNOUNCEMENT_STATUS_PUBLISHED);
    assert_eq!(revised.title, "Updated notice");

    let archived = store
        .get_announcement(&published.id)
        .await
        .expect("load previous announcement")
        .expect("previous announcement exists");
    assert_eq!(archived.status, ANNOUNCEMENT_STATUS_ARCHIVED);
    assert!(archived.archived_at.is_some());

    let active = store
        .list_user_active_announcements()
        .await
        .expect("list active announcements");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, revised.id);

    let history = store
        .list_user_announcement_history()
        .await
        .expect("list announcement history");
    assert!(history.iter().any(|item| item.id == revised.id));
    assert!(history.iter().any(|item| item.id == published.id));

    let draft_only = store
        .create_announcement(AnnouncementMutation {
            title: "Draft-only notice".to_string(),
            body: "Never published".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_TICKER.to_string(),
        })
        .await
        .expect("create draft-only announcement");
    let archived_draft = store
        .archive_announcement(&draft_only.id)
        .await
        .expect("archive draft-only announcement")
        .expect("archived draft exists");
    assert_eq!(archived_draft.status, ANNOUNCEMENT_STATUS_ARCHIVED);

    let history_after_draft_archive = store
        .list_user_announcement_history()
        .await
        .expect("list announcement history after draft archive");
    assert!(
        !history_after_draft_archive
            .iter()
            .any(|item| item.id == archived_draft.id)
    );

    let archived_revised = store
        .archive_announcement(&revised.id)
        .await
        .expect("archive revised announcement")
        .expect("archived revised announcement exists");
    assert_eq!(archived_revised.status, ANNOUNCEMENT_STATUS_ARCHIVED);

    let edited_archived = store
        .update_announcement(
            &archived_revised.id,
            AnnouncementMutation {
                title: "Edited archived notice".to_string(),
                body: "Edited archived body".to_string(),
                display_kind: ANNOUNCEMENT_DISPLAY_TICKER.to_string(),
            },
        )
        .await
        .expect("edit archived announcement")
        .expect("edited archived announcement creates draft");
    assert_ne!(edited_archived.id, archived_revised.id);
    assert_eq!(edited_archived.status, ANNOUNCEMENT_STATUS_DRAFT);

    let archived_revised_after_edit = store
        .get_announcement(&archived_revised.id)
        .await
        .expect("load archived revised after edit")
        .expect("archived revised still exists after edit");
    assert_eq!(
        archived_revised_after_edit.status,
        ANNOUNCEMENT_STATUS_ARCHIVED
    );
    assert_eq!(archived_revised_after_edit.title, archived_revised.title);

    let history_after_archived_edit = store
        .list_user_announcement_history()
        .await
        .expect("list announcement history after archived edit");
    assert!(
        !history_after_archived_edit
            .iter()
            .any(|item| item.id == edited_archived.id)
    );

    let republished = store
        .publish_announcement(&archived_revised.id)
        .await
        .expect("republish archived announcement")
        .expect("republished announcement exists");
    assert_ne!(republished.id, archived_revised.id);
    assert_eq!(republished.status, ANNOUNCEMENT_STATUS_PUBLISHED);
    assert_eq!(republished.title, archived_revised.title);

    let archived_revised_after_republish = store
        .get_announcement(&archived_revised.id)
        .await
        .expect("load archived revised after republish")
        .expect("archived revised still exists");
    assert_eq!(
        archived_revised_after_republish.status,
        ANNOUNCEMENT_STATUS_ARCHIVED
    );

    let active_after_republish = store
        .list_user_active_announcements()
        .await
        .expect("list active after republish");
    assert_eq!(active_after_republish.len(), 1);
    assert_eq!(active_after_republish[0].id, republished.id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn active_announcements_use_insert_order_for_same_second_ties() {
    let db_path = temp_db_path("announcement-active-same-second-order");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");
    let same_second = 1_764_300_000_i64;

    sqlx::query(
        r#"
        INSERT INTO announcements (
            id, title, body, display_kind, status,
            created_at, updated_at, published_at, archived_at
        ) VALUES
            ('zzzzzzzz', 'Older modal', 'Older body', 'modal', 'published', ?, ?, ?, NULL),
            ('22222222', 'Newer modal', 'Newer body', 'modal', 'published', ?, ?, ?, NULL)
        "#,
    )
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .execute(&store.pool)
    .await
    .expect("insert same-second announcements");

    let active = store
        .list_user_active_announcements()
        .await
        .expect("list active announcements");

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "22222222");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ticker_announcements_may_omit_body_but_modal_announcements_may_not() {
    let db_path = temp_db_path("announcement-ticker-empty-body");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    let ticker = store
        .create_announcement(AnnouncementMutation {
            title: "Ticker without details".to_string(),
            body: "   ".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_TICKER.to_string(),
        })
        .await
        .expect("create ticker without body");
    assert_eq!(ticker.body, "");

    let modal = store
        .create_announcement(AnnouncementMutation {
            title: "Modal without details".to_string(),
            body: "   ".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_MODAL.to_string(),
        })
        .await;
    assert!(
        modal.is_err(),
        "modal announcements still require body content"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_is_single_use() {
    let db_path = temp_db_path("oauth-state-single-use");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state("linuxdo", Some("/"), 120)
        .await
        .expect("create oauth state");
    let first = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state first");
    let second = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state second");

    assert_eq!(first, Some(Some("/".to_string())));
    assert_eq!(second, None, "oauth state must be single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_binding_hash_must_match() {
    let db_path = temp_db_path("oauth-state-binding-hash");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding("linuxdo", Some("/"), 120, Some("nonce-hash-a"))
        .await
        .expect("create oauth state");

    let wrong_hash = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-b"))
        .await
        .expect("consume oauth state with wrong hash");
    assert_eq!(wrong_hash, None, "wrong hash must not consume oauth state");

    let matched = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state with matching hash");
    assert_eq!(matched, Some(Some("/".to_string())));

    let reused = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state reused");
    assert_eq!(reused, None, "oauth state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_payload_carries_bind_token_id() {
    let db_path = temp_db_path("oauth-state-bind-token-id");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding_and_token(
            "linuxdo",
            Some("/console"),
            120,
            Some("nonce-hash-a"),
            Some("a1b2"),
        )
        .await
        .expect("create oauth state");

    let payload = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state")
        .expect("payload exists");

    assert_eq!(payload.redirect_to.as_deref(), Some("/console"));
    assert_eq!(payload.bind_token_id.as_deref(), Some("a1b2"));

    let consumed_again = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state second");
    assert!(consumed_again.is_none(), "state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_stats_coalescer_waits_for_window_before_background_flush() {
    let db_path = temp_db_path("request-stats-coalescer-windowed-flush");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-request-stats-coalescer-window".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now_ts = Utc::now().timestamp();

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), now_ts, OUTCOME_SUCCESS)
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    let stored_total: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT total_requests
        FROM api_key_usage_buckets
        WHERE api_key_id = ?
        ORDER BY bucket_start DESC
        LIMIT 1
        "#,
    )
    .bind(&key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query usage buckets before flush window");
    assert!(
        stored_total.is_none(),
        "background worker should not flush request stats before the coalescing window elapses"
    );

    tokio::time::sleep(RequestStatsCoalescer::FLUSH_INTERVAL + Duration::from_millis(150)).await;

    let stored_total: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT total_requests
        FROM api_key_usage_buckets
        WHERE api_key_id = ?
        ORDER BY bucket_start DESC
        LIMIT 1
        "#,
    )
    .bind(&key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query usage buckets after flush window");
    assert_eq!(stored_total, Some(1));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

use super::*;

/// Legacy sidecar `request_logs` shape (pre-provider, 48 columns) used to
/// prove the guarded ALTER upgrade path adds `provider` exactly once.
const LEGACY_SIDECAR_REQUEST_LOGS_DDL: &str = r#"
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
        "#;

fn cleanup_sidecar_fixture(db_path: &std::path::Path, observability_path: &str) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

async fn seed_legacy_main_db_with_request_log(db_path: &std::path::Path) {
    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(db_path)
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
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-sidecar-provider-seed', 1, 1)",
    )
    .execute(&mut conn)
    .await
    .expect("insert api key");
    sqlx::query(
        r#"
        INSERT INTO request_logs (api_key_id, method, path, created_at)
        VALUES ('k1', 'POST', '/api/tavily/search', ?)
        "#,
    )
    .bind(Utc::now().timestamp() - 60)
    .execute(&mut conn)
    .await
    .expect("insert legacy request log");
    drop(conn);
}

async fn provider_column_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('request_logs', 'observability') WHERE name = 'provider'",
    )
    .fetch_one(pool)
    .await
    .expect("count provider columns")
}

/// Given: a legacy main DB plus a preseeded sidecar whose `request_logs`
/// predates the provider dimension.
/// When: startup runs the sidecar ALTER upgrade path (twice).
/// Then: `provider` exists exactly once, legacy rows stay NULL (no backfill
/// rewrite), and readers coalesce them to `tavily`.
#[tokio::test]
async fn startup_upgrade_adds_sidecar_request_logs_provider_once() {
    let db_path = temp_db_path("observability-sidecar-provider-startup");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    seed_legacy_main_db_with_request_log(&db_path).await;

    let mut observability_conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(true),
    )
    .await
    .expect("open preseeded sidecar sqlite");
    sqlx::query(LEGACY_SIDECAR_REQUEST_LOGS_DDL)
        .execute(&mut observability_conn)
        .await
        .expect("create preseeded legacy sidecar request_logs");
    drop(observability_conn);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    assert_eq!(
        provider_column_count(&proxy.key_store.pool).await,
        1,
        "guarded sidecar ALTER must add provider exactly once"
    );
    let (raw_null_rows, coalesced_rows): (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN provider IS NULL THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN COALESCE(provider, 'tavily') = 'tavily' THEN 1 ELSE 0 END), 0)
        FROM observability.request_logs
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("aggregate legacy provider values");
    assert_eq!(
        raw_null_rows, coalesced_rows,
        "legacy rows must stay NULL (no DEFAULT-clause backfill) and coalesce to tavily"
    );
    assert!(raw_null_rows >= 1, "the seeded legacy row must survive");

    let page = proxy
        .key_store
        .fetch_request_logs_page(
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1,
            20,
            false,
            false,
            false,
        )
        .await
        .expect("fetch request logs page");
    assert!(
        !page.items.is_empty(),
        "migrated legacy rows stay readable through the log page reader"
    );
    for record in &page.items {
        assert_eq!(
            record.provider, "tavily",
            "readers coalesce legacy NULL provider rows to tavily"
        );
    }

    drop(proxy);
    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopens upgraded sidecar");
    assert_eq!(
        provider_column_count(&reopened.key_store.pool).await,
        1,
        "re-running the upgrade path must be a no-op"
    );
    let still_null: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.request_logs WHERE provider IS NULL",
    )
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("count NULL provider rows after reopen");
    assert!(
        still_null >= 1,
        "idempotent re-run must not rewrite legacy NULL providers"
    );

    drop(reopened);
    cleanup_sidecar_fixture(&db_path, &observability_path);
}

/// Given: a preseeded sidecar in the legacy shape that still carries the
/// ancient `api_key` column (which triggers the historical table rebuild).
/// When: startup runs the sidecar upgrade path.
/// Then: the rebuild completes, `api_key` is dropped, and the provider
/// column is (re)added AFTER the rebuild so it can never be lost.
#[tokio::test]
async fn legacy_sidecar_rebuild_gains_provider_after_upgrade() {
    let db_path = temp_db_path("observability-sidecar-provider-rebuild");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    seed_legacy_main_db_with_request_log(&db_path).await;

    let mut observability_conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(true),
    )
    .await
    .expect("open preseeded sidecar sqlite");
    sqlx::query(LEGACY_SIDECAR_REQUEST_LOGS_DDL)
        .execute(&mut observability_conn)
        .await
        .expect("create preseeded legacy sidecar request_logs");
    sqlx::query("ALTER TABLE request_logs ADD COLUMN api_key TEXT")
        .execute(&mut observability_conn)
        .await
        .expect("add ancient api_key column");
    sqlx::query(
        r#"
        INSERT INTO request_logs (id, api_key_id, method, path, created_at)
        VALUES (500, 'k1', 'POST', '/api/tavily/search', ?)
        "#,
    )
    .bind(Utc::now().timestamp() - 30)
    .execute(&mut observability_conn)
    .await
    .expect("seed preexisting sidecar row");
    drop(observability_conn);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy upgrades legacy sidecar shape with rebuild");

    assert_eq!(
        provider_column_count(&proxy.key_store.pool).await,
        1,
        "provider must exist exactly once after the rebuild path"
    );
    let api_key_column_gone: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM pragma_table_info('request_logs', 'observability') WHERE name = 'api_key' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("probe legacy api_key column");
    assert!(
        api_key_column_gone.is_none(),
        "the rebuild must still drop the ancient api_key column"
    );
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count rows after rebuild");
    assert_eq!(rows, 2, "preexisting and copied legacy rows both survive");
    let null_providers: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.request_logs WHERE provider IS NULL",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count NULL providers after rebuild");
    assert_eq!(
        null_providers, 2,
        "legacy rows stay NULL through the rebuild"
    );

    drop(proxy);
    cleanup_sidecar_fixture(&db_path, &observability_path);
}

/// Given: a large legacy single-file DB (offline-migrate territory).
/// When: `run_observability_sidecar_migrate` runs dry-run, then apply, then
/// apply again, then dry-run again.
/// Then: the sidecar gains `request_logs.provider` exactly once, copied
/// legacy rows stay NULL (readers coalesce to tavily), and every rerun is
/// an idempotent no-op.
#[tokio::test]
async fn offline_sidecar_migrate_adds_provider_idempotently() {
    let db_path = temp_db_path("observability-sidecar-provider-offline");
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
            request_user_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            result_status TEXT NOT NULL DEFAULT 'success',
            failure_kind TEXT,
            counts_business_quota INTEGER,
            upstream_operation TEXT,
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
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-offline-provider', 1, 1)",
    )
    .execute(&pool)
    .await
    .expect("insert api key");
    let now_ts = Utc::now().timestamp();
    for (id, created_at) in [(1_i64, now_ts - 400), (2, now_ts - 300), (4, now_ts - 100)] {
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                id, api_key_id, request_user_id, method, path, result_status,
                failure_kind, counts_business_quota, upstream_operation,
                request_kind_key, visibility, created_at
            )
            VALUES (?, 'k1', 'user-k1', 'POST', '/api/tavily/search', 'success', NULL, 1, 'search', 'api:search', 'visible', ?)
            "#,
        )
        .bind(id)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("seed request log");
    }
    sqlx::query(
        "INSERT INTO request_logs (id, api_key_id, request_user_id, method, path, result_status, failure_kind, counts_business_quota, upstream_operation, request_kind_key, visibility, created_at) VALUES (3, 'k1', 'user-k1', 'POST', '/api/tavily/search', 'error', 'upstream_rate_limited_429', 1, 'search', 'api:search', 'visible', ?)",
    )
    .bind(now_ts - 200)
    .execute(&pool)
    .await
    .expect("seed 429 request log");
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
    assert!(!dry_run.already_migrated);
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "dry run must not create or alter the sidecar"
    );

    let report = run_observability_sidecar_migrate(&db_str, 2, false)
        .await
        .expect("offline migration succeeds");
    assert!(report.completed);
    assert!(report.startup_reopen_verified);
    assert_eq!(report.copied_request_logs, 4);
    assert!(report.dropped_main_request_logs);

    let sidecar_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(false),
    )
    .await
    .expect("open migrated sidecar");
    let provider_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('request_logs') WHERE name = 'provider'",
    )
    .fetch_one(&sidecar_pool)
    .await
    .expect("count provider columns in migrated sidecar");
    assert_eq!(provider_columns, 1, "offline migrate adds provider once");
    let (null_rows, coalesced_rows): (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN provider IS NULL THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN COALESCE(provider, 'tavily') = 'tavily' THEN 1 ELSE 0 END), 0)
        FROM request_logs
        "#,
    )
    .fetch_one(&sidecar_pool)
    .await
    .expect("aggregate migrated provider values");
    assert_eq!(null_rows, 4, "copied legacy rows keep NULL provider");
    assert_eq!(coalesced_rows, 4, "every legacy row coalesces to tavily");
    drop(sidecar_pool);

    let rerun = run_observability_sidecar_migrate(&db_str, 2, false)
        .await
        .expect("rerun stays idempotent");
    assert!(rerun.already_migrated);
    assert_eq!(rerun.copied_request_logs, 0);

    let post_dry_run = run_observability_sidecar_migrate(&db_str, 2, true)
        .await
        .expect("post-apply dry run succeeds");
    assert!(post_dry_run.already_migrated);
    assert!(!post_dry_run.startup_rebuild_required);

    let verify_pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(false),
    )
    .await
    .expect("reopen sidecar after reruns");
    let provider_columns_after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('request_logs') WHERE name = 'provider'",
    )
    .fetch_one(&verify_pool)
    .await
    .expect("recount provider columns after reruns");
    assert_eq!(provider_columns_after, 1, "reruns never duplicate provider");
    let null_rows_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_logs WHERE provider IS NULL")
            .fetch_one(&verify_pool)
            .await
            .expect("recount NULL provider rows after reruns");
    assert_eq!(
        null_rows_after, 4,
        "idempotent reruns must not rewrite legacy rows"
    );
    drop(verify_pool);

    cleanup_sidecar_fixture(&db_path, &observability_path);
}

fn attempt_log_fixture<'a>(
    key_id: Option<&'a str>,
    request_body: &'a [u8],
    response_body: &'a [u8],
    provider: Option<&'a str>,
) -> AttemptLog<'a> {
    AttemptLog {
        key_id,
        auth_token_id: None,
        method: &Method::POST,
        path: "/mcp",
        query: None,
        status: Some(StatusCode::OK),
        tavily_status_code: Some(200),
        error: None,
        request_body,
        response_body,
        outcome: OUTCOME_SUCCESS,
        failure_kind: None,
        key_effect_code: KEY_EFFECT_NONE,
        key_effect_summary: None,
        binding_effect_code: KEY_EFFECT_NONE,
        binding_effect_summary: None,
        selection_effect_code: KEY_EFFECT_NONE,
        selection_effect_summary: None,
        gateway_mode: None,
        experiment_variant: None,
        provider,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
        forwarded_headers: &[],
        dropped_headers: &[],
        client_ip: None,
    }
}

/// Given: a fresh sidecar bootstrap (canonical DDL already carries provider).
/// When: log writers persist an explicit provider, omit it, or write via the
/// rebalance audit path.
/// Then: explicit providers persist verbatim, absent provider persists
/// 'tavily', an absent-vs-explicit-tavily pair is byte-identical across every
/// other column, rebalance audits persist 'tavily', and the page reader
/// coalesces a raw NULL legacy row to 'tavily'.
#[tokio::test]
async fn log_writers_persist_provider_with_tavily_default() {
    let db_path = temp_db_path("observability-sidecar-provider-writers");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    assert_eq!(
        provider_column_count(&proxy.key_store.pool).await,
        1,
        "fresh sidecar bootstrap DDL must carry provider exactly once"
    );

    let request_body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let response_body = br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"search"}]}}"#;

    let absent_id = proxy
        .key_store
        .log_attempt(attempt_log_fixture(None, request_body, response_body, None))
        .await
        .expect("log attempt without provider");
    let explicit_tavily_id = proxy
        .key_store
        .log_attempt(attempt_log_fixture(
            None,
            request_body,
            response_body,
            Some("tavily"),
        ))
        .await
        .expect("log attempt with explicit tavily");
    let serpapi_id = proxy
        .key_store
        .log_attempt(attempt_log_fixture(
            None,
            request_body,
            response_body,
            Some("serpapi"),
        ))
        .await
        .expect("log attempt with serpapi");

    let providers: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, provider FROM observability.request_logs WHERE id IN (?, ?, ?) ORDER BY id",
    )
    .bind(absent_id)
    .bind(explicit_tavily_id)
    .bind(serpapi_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch persisted providers");
    let by_id = |wanted: i64| -> Option<String> {
        providers
            .iter()
            .find(|(id, _)| *id == wanted)
            .and_then(|(_, provider)| provider.clone())
    };
    assert_eq!(
        by_id(absent_id).as_deref(),
        Some("tavily"),
        "absent provider persists the tavily default"
    );
    assert_eq!(by_id(explicit_tavily_id).as_deref(), Some("tavily"));
    assert_eq!(by_id(serpapi_id).as_deref(), Some("serpapi"));

    // Byte-identical Tavily behavior: every column except the volatile
    // identity/time columns and provider itself must match between the
    // absent-provider and explicit-tavily rows.
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('request_logs', 'observability')")
            .fetch_all(&proxy.key_store.pool)
            .await
            .expect("list request_logs columns");
    let fingerprint_sql = format!(
        "SELECT {} FROM observability.request_logs WHERE id = ?",
        columns
            .iter()
            .filter(|column| {
                !matches!(
                    column.as_str(),
                    "id" | "provider" | "created_at" | "body_cleaned_at"
                )
            })
            .map(|column| format!("quote({column})"))
            .collect::<Vec<_>>()
            .join(" || ',' || ")
    );
    let pool = &proxy.key_store.pool;
    let absent_fingerprint: String = sqlx::query_scalar(&fingerprint_sql)
        .bind(absent_id)
        .fetch_one(pool)
        .await
        .expect("fingerprint absent-provider row");
    let explicit_fingerprint: String = sqlx::query_scalar(&fingerprint_sql)
        .bind(explicit_tavily_id)
        .fetch_one(pool)
        .await
        .expect("fingerprint explicit-tavily row");
    assert_eq!(
        absent_fingerprint, explicit_fingerprint,
        "absent provider must behave byte-identically to explicit tavily"
    );

    // Rebalance audit rows are Tavily-exclusive by construction.
    let receipts = proxy
        .key_store
        .log_rebalance_audit_entries(vec![RebalanceAuditEntry {
            auth_token_id: None,
            method: Method::POST,
            path: "/mcp".to_string(),
            request_body: request_body.to_vec(),
            response_status: StatusCode::OK,
            tavily_status_code: Some(200),
            response_body: response_body.to_vec(),
            result_status: OUTCOME_SUCCESS.to_string(),
            failure_kind: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            fallback_reason: None,
        }])
        .await
        .expect("log rebalance audit entry");
    assert_eq!(receipts.len(), 1);
    let rebalance_provider: Option<String> = sqlx::query_scalar(
        "SELECT provider FROM observability.request_logs WHERE gateway_mode = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(MCP_GATEWAY_MODE_REBALANCE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch rebalance audit provider");
    assert_eq!(
        rebalance_provider.as_deref(),
        Some("tavily"),
        "rebalance audit rows persist the tavily provider"
    );

    // A raw NULL legacy row (pre-upgrade artifact) reads back coalesced.
    let now_ts = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (method, path, result_status, visibility, created_at)
        VALUES ('POST', '/api/tavily/search', 'success', 'visible', ?)
        "#,
    )
    .bind(now_ts)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert raw NULL-provider legacy row");
    let legacy_id: i64 = sqlx::query_scalar(
        "SELECT id FROM observability.request_logs WHERE provider IS NULL ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch legacy row id");

    let page = proxy
        .key_store
        .fetch_request_logs_page(
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            1,
            20,
            false,
            false,
            false,
        )
        .await
        .expect("fetch request logs page");
    let legacy_record = page
        .items
        .iter()
        .find(|record| record.id == legacy_id)
        .expect("legacy row visible in page reader");
    assert_eq!(
        legacy_record.provider, "tavily",
        "page reader coalesces legacy NULL provider to tavily"
    );
    let serpapi_record = page
        .items
        .iter()
        .find(|record| record.id == serpapi_id)
        .expect("serpapi row visible in page reader");
    assert_eq!(serpapi_record.provider, "serpapi");

    drop(proxy);
    cleanup_sidecar_fixture(&db_path, &observability_path);
}

fn extract_request_logs_ddl_column_lists(source: &str) -> Vec<Vec<String>> {
    let marker = "CREATE TABLE IF NOT EXISTS observability.request_logs (";
    let mut blocks = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find(marker) {
        let after = &rest[start + marker.len()..];
        let end = after.find("\n            )").expect("closing paren");
        let columns = after[..end]
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| {
                line.trim_end_matches(',')
                    .split_whitespace()
                    .next()
                    .expect("column name")
                    .to_string()
            })
            .collect::<Vec<_>>();
        blocks.push(columns);
        rest = &after[end..];
    }
    blocks
}

/// QA failure scenario: a mismatched sidecar DDL copy is caught — all three
/// production request_logs DDL copies must carry the provider column in the
/// identical position.
#[test]
fn request_logs_ddl_copies_stay_consistent_with_provider() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let bootstrap_source =
        std::fs::read_to_string(format!("{manifest_dir}/src/store/key_store_bootstrap.rs"))
            .expect("read key_store_bootstrap.rs");
    let sidecar_source = std::fs::read_to_string(format!(
        "{manifest_dir}/src/store/key_store_observability_sidecar.rs"
    ))
    .expect("read key_store_observability_sidecar.rs");

    let mut blocks = extract_request_logs_ddl_column_lists(&bootstrap_source);
    blocks.extend(extract_request_logs_ddl_column_lists(&sidecar_source));
    assert_eq!(
        blocks.len(),
        3,
        "expected the canonical bootstrap, standalone GC, and sidecar DDL copies"
    );
    for columns in &blocks {
        assert!(
            columns.iter().any(|column| column == "provider"),
            "every request_logs DDL copy must contain the provider column"
        );
        assert_eq!(
            columns, &blocks[0],
            "all request_logs DDL copies must stay textually consistent"
        );
    }
}

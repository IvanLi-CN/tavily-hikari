use crate::store::*;
use crate::tavily_proxy::*;
use crate::*;

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{any, post},
};
use nanoid::nanoid;
use sqlx::Connection;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;

pub(super) fn env_lock() -> Arc<tokio::sync::Mutex<()>> {
    static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

pub(super) fn dead_request_kind_migration_owner_pid() -> u32 {
    for candidate in [999_999_u32, 888_888, 777_777, 666_666] {
        if !request_kind_canonical_migration_owner_pid_is_live(candidate) {
            return candidate;
        }
    }
    panic!("unable to find a dead pid candidate for request-kind migration tests");
}

pub(super) async fn spawn_api_key_geo_mock_server() -> SocketAddr {
    let app = Router::new().route(
        "/geo",
        post(|Json(ips): Json<Vec<String>>| async move {
            let entries = ips
                .into_iter()
                .map(|ip| match ip.as_str() {
                    "18.183.246.69" => serde_json::json!({
                        "ip": ip,
                        "country": "JP",
                        "city": "Tokyo",
                        "subdivision": "13"
                    }),
                    "1.1.1.1" => serde_json::json!({
                        "ip": ip,
                        "country": "HK",
                        "city": null,
                        "subdivision": null
                    }),
                    "1.0.0.1" => serde_json::json!({
                        "ip": ip,
                        "country": "HK",
                        "city": null,
                        "subdivision": null
                    }),
                    "8.8.8.8" => serde_json::json!({
                        "ip": ip,
                        "country": "US",
                        "city": null,
                        "subdivision": null
                    }),
                    _ => serde_json::json!({
                        "ip": ip,
                        "country": null,
                        "city": null,
                        "subdivision": null
                    }),
                })
                .collect::<Vec<_>>();
            Json(entries)
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

pub(super) async fn spawn_fake_forward_proxy_with_body(body: String) -> SocketAddr {
    let app = Router::new().fallback(any(move || {
        let body = body.clone();
        async move { (StatusCode::OK, body) }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

pub(super) fn temp_db_path(prefix: &str) -> PathBuf {
    static CLEANUP_ONCE: OnceLock<()> = OnceLock::new();
    CLEANUP_ONCE.get_or_init(cleanup_stale_test_db_dirs);
    let dir = std::env::temp_dir().join(format!(
        "tavily-hikari-libtestdb-{}-{}",
        std::process::id(),
        nanoid!(8)
    ));
    std::fs::create_dir_all(&dir).expect("create lib test temp dir");
    dir.join(format!("{prefix}.db"))
}

fn cleanup_stale_test_db_dirs() {
    let tmp_dir = std::env::temp_dir();
    let Ok(entries) = std::fs::read_dir(&tmp_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(pid_part) = name
            .strip_prefix("tavily-hikari-libtestdb-")
            .and_then(|suffix| suffix.split('-').next())
        else {
            continue;
        };
        let Ok(pid) = pid_part.parse::<u32>() else {
            continue;
        };
        let is_live = unsafe { libc::kill(pid as i32, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        if pid == std::process::id() || is_live {
            continue;
        }
        let _ = std::fs::remove_dir_all(path);
    }
}

pub(super) fn sqlite_test_layout(database_path: &str) -> SqliteDatabaseLayout {
    SqliteDatabaseLayout::from_database_path(database_path)
}

pub(super) async fn connect_sqlite_test_pool(db_str: &str) -> sqlx::SqlitePool {
    let layout = sqlite_test_layout(db_str);
    open_sqlite_pool_with_observability(
        &layout.core_database_path,
        layout.observability_database_path.as_deref(),
        true,
        false,
    )
    .await
    .expect("open sqlite test pool")
}

pub(super) async fn connect_sqlite_test_connection(
    db_str: &str,
    create_if_missing: bool,
    read_only: bool,
    busy_timeout: Duration,
) -> sqlx::SqliteConnection {
    let layout = sqlite_test_layout(db_str);
    let mut options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&layout.core_database_path)
        .create_if_missing(create_if_missing)
        .read_only(read_only)
        .busy_timeout(busy_timeout);
    if !read_only {
        options = options.journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    }
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("connect sqlite test connection");
    if let Some(observability_database_path) = layout.observability_database_path {
        let attach_sql = format!(
            "ATTACH DATABASE '{}' AS observability",
            observability_database_path.replace('\'', "''")
        );
        sqlx::query(&attach_sql)
            .execute(&mut conn)
            .await
            .expect("attach observability sidecar");
    }
    conn
}

pub(super) async fn insert_summary_window_bucket(
    proxy: &TavilyProxy,
    key_id: &str,
    bucket_start: i64,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO api_key_usage_buckets (
            api_key_id,
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            valuable_success_count,
            valuable_failure_count,
            other_success_count,
            other_failure_count,
            unknown_count,
            updated_at
        ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?, ?, 0, 0, 0, ?)
        "#,
    )
    .bind(key_id)
    .bind(bucket_start)
    .bind(total_requests)
    .bind(success_count)
    .bind(error_count)
    .bind(quota_exhausted_count)
    .bind(success_count)
    .bind(error_count + quota_exhausted_count)
    .bind(bucket_start + 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert summary window bucket");
}

pub(super) async fn insert_dashboard_summary_rollup_day_bucket(
    proxy: &TavilyProxy,
    bucket_start: i64,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            valuable_success_count,
            valuable_failure_count,
            valuable_failure_429_count,
            other_success_count,
            other_failure_count,
            unknown_count,
            mcp_non_billable,
            mcp_billable,
            api_non_billable,
            api_billable,
            local_estimated_credits,
            updated_at
        ) VALUES (?, 86400, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, 0, ?, 0, ?)
        "#,
    )
    .bind(bucket_start)
    .bind(total_requests)
    .bind(success_count)
    .bind(error_count)
    .bind(quota_exhausted_count)
    .bind(success_count)
    .bind(error_count + quota_exhausted_count)
    .bind(total_requests)
    .bind(bucket_start + 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert dashboard summary rollup day bucket");
}

pub(super) async fn insert_summary_window_logs(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    outcome: &str,
    count: usize,
) {
    insert_summary_window_logs_with_visibility(
        proxy,
        key_id,
        created_at,
        outcome,
        count,
        REQUEST_LOG_VISIBILITY_VISIBLE,
    )
    .await;
}

pub(super) async fn insert_summary_window_logs_with_visibility(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    outcome: &str,
    count: usize,
    visibility: &str,
) {
    for offset in 0..count {
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
            ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, ?, NULL, NULL, '[]', '[]', ?, ?)
            "#,
        )
        .bind(key_id)
        .bind(outcome)
        .bind(visibility)
        .bind(created_at + offset as i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert summary window log");
    }

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(
            Some(created_at),
            Some(created_at + count as i64),
        )
        .await
        .expect("rebuild dashboard rollup after summary log seed");
}

#[derive(Clone)]
pub(super) struct DashboardHourlyLogSeed<'a> {
    pub(super) created_at: i64,
    pub(super) path: &'a str,
    pub(super) request_kind_key: &'a str,
    pub(super) request_kind_label: &'a str,
    pub(super) result_status: &'a str,
    pub(super) failure_kind: Option<&'a str>,
    pub(super) request_body: Option<&'a [u8]>,
    pub(super) visibility: &'a str,
}

pub(super) async fn insert_dashboard_hourly_log(
    proxy: &TavilyProxy,
    key_id: &str,
    seed: DashboardHourlyLogSeed<'_>,
) {
    let status_code = match seed.result_status {
        OUTCOME_SUCCESS => 200,
        OUTCOME_QUOTA_EXHAUSTED => 429,
        _ => 500,
    };
    let tavily_status_code = if seed.failure_kind == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        || seed.result_status == OUTCOME_QUOTA_EXHAUSTED
    {
        Some(429)
    } else if seed.result_status == OUTCOME_SUCCESS {
        Some(200)
    } else {
        Some(500)
    };

    sqlx::query(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            failure_kind,
            created_at
        ) VALUES (?, NULL, 'POST', ?, NULL, ?, ?, NULL, ?, ?, ?, ?, NULL, '[]', '[]', ?, ?, ?)
        "#,
    )
    .bind(key_id)
    .bind(seed.path)
    .bind(status_code)
    .bind(tavily_status_code)
    .bind(seed.result_status)
    .bind(seed.request_kind_key)
    .bind(seed.request_kind_label)
    .bind(seed.request_body)
    .bind(seed.visibility)
    .bind(seed.failure_kind)
    .bind(seed.created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert dashboard hourly log");

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(
            Some(seed.created_at),
            Some(seed.created_at + 1),
        )
        .await
        .expect("rebuild dashboard rollup after hourly log seed");
}

pub(super) async fn insert_summary_window_charged_logs(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    credits: i64,
    count: usize,
) {
    for offset in 0..count {
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                business_credits,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
            ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, ?, ?, NULL, NULL, '[]', '[]', ?, ?)
            "#,
        )
        .bind(key_id)
        .bind(OUTCOME_SUCCESS)
        .bind(credits)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(created_at + offset as i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert summary window charged log");
    }

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(
            Some(created_at),
            Some(created_at + count as i64),
        )
        .await
        .expect("rebuild dashboard rollup after charged log seed");
}

pub(super) async fn insert_summary_window_maintenance_record(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    source: &str,
    operation_code: &str,
    reason_code: Option<&str>,
) {
    let reason_summary = reason_code.map(|code| format!("{code} summary"));
    sqlx::query(
        r#"
        INSERT INTO api_key_maintenance_records (
            id,
            key_id,
            source,
            operation_code,
            operation_summary,
            reason_code,
            reason_summary,
            reason_detail,
            request_log_id,
            auth_token_log_id,
            auth_token_id,
            actor_user_id,
            actor_display_name,
            status_before,
            status_after,
            quarantine_before,
            quarantine_after,
            created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?)
        "#,
    )
    .bind(format!("summary-window-maint-{}", nanoid!(8)))
    .bind(key_id)
    .bind(source)
    .bind(operation_code)
    .bind(format!("{operation_code} summary"))
    .bind(reason_code)
    .bind(reason_summary)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert summary window maintenance record");
}

pub(super) async fn insert_auth_token_metric_log(
    proxy: &TavilyProxy,
    token_id: &str,
    created_at: i64,
    result_status: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            result_status,
            error_message,
            created_at,
            counts_business_quota
        ) VALUES (?, 'POST', '/api/tavily/search', NULL, 200, 200, ?, NULL, ?, 1)
        "#,
    )
    .bind(token_id)
    .bind(result_status)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert auth token metric log");
}

pub(super) async fn hold_sqlite_write_lock_for_test(
    pool: &SqlitePool,
) -> tokio::task::JoinHandle<()> {
    hold_sqlite_write_lock_for_test_for(pool, Duration::from_millis(120)).await
}

pub(super) async fn begin_held_sqlite_write_lock_for_test(
    pool: &SqlitePool,
) -> sqlx::pool::PoolConnection<sqlx::Sqlite> {
    begin_immediate_sqlite_connection(pool)
        .await
        .expect("begin immediate transaction")
}

pub(super) async fn hold_sqlite_write_lock_for_test_for(
    pool: &SqlitePool,
    hold_for: Duration,
) -> tokio::task::JoinHandle<()> {
    hold_sqlite_write_lock_for_test_for_with_release(pool, hold_for, None).await
}

pub(super) async fn hold_sqlite_write_lock_for_test_for_with_release(
    pool: &SqlitePool,
    hold_for: Duration,
    release: Option<crate::ManualBackendTime>,
) -> tokio::task::JoinHandle<()> {
    let mut immediate_conn = begin_immediate_sqlite_connection(pool)
        .await
        .expect("begin immediate transaction");
    tokio::spawn(async move {
        if let Some(release) = release {
            release.advance(hold_for).await;
        } else {
            tokio::time::sleep(hold_for).await;
        }
        sqlx::query("ROLLBACK")
            .execute(&mut *immediate_conn)
            .await
            .expect("rollback immediate transaction");
    })
}

pub(super) async fn ensure_quota_subject_lock_schema_for_test(pool: &SqlitePool) {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS quota_subject_locks (
            subject TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await
    .expect("create quota_subject_locks table");
    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_quota_subject_locks_expires_at
           ON quota_subject_locks(expires_at)"#,
    )
    .execute(pool)
    .await
    .expect("create quota_subject_locks expires index");
}

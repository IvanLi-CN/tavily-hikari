use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tavily_hikari::{DEFAULT_UPSTREAM, TavilyProxy};

fn temp_db_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{}-{}-{}.db",
        prefix,
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

async fn connect_sqlite_test_pool(db_path: &str) -> sqlx::SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open sqlite pool")
}

fn run_backfill(db_path: &str, batch_size: i64) {
    let output = Command::new(env!("CARGO_BIN_EXE_request_kind_canonical_backfill"))
        .args([
            "--db-path",
            db_path,
            "--batch-size",
            &batch_size.to_string(),
        ])
        .output()
        .expect("run request_kind_canonical_backfill");
    assert!(
        output.status.success(),
        "backfill binary failed: stdout={}; stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

async fn sqlite_column_exists(pool: &sqlx::SqlitePool, table: &str, column: &str) -> bool {
    sqlx::query_scalar::<_, i64>(&format!(
        "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ? LIMIT 1"
    ))
    .bind(column)
    .fetch_optional(pool)
    .await
    .expect("probe sqlite column")
    .is_some()
}

#[tokio::test]
async fn request_kind_backfill_is_idempotent_without_legacy_snapshots() {
    let db_path = temp_db_path("request-kind-backfill-idempotent");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-request-kind-backfill".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("request-kind-backfill"))
        .await
        .expect("token created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("fetch key id");

    let request_log_id: i64 = sqlx::query_scalar(
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
            request_kind_detail,
            business_credits,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/search', NULL, 404, 404, 'Not Found', 'error',
            'mcp:raw:/mcp/search', 'MCP | /mcp/search', NULL,
            NULL, 'mcp_path_404', 'none', NULL, X'7B226B223A317D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(&key_id)
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .fetch_one(&pool)
    .await
    .expect("insert request log");

    let token_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            api_key_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
            result_status,
            error_message,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            created_at,
            counts_business_quota,
            billing_state
        ) VALUES (
            ?, ?, 'POST', '/mcp', NULL, 200, 200,
            'mcp:tool:acme-lookup', 'MCP | acme_lookup', NULL,
            'success', NULL, NULL, 'none', NULL, ?, 0, 'none'
        )
        RETURNING id
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .bind(Utc::now().timestamp() + 1)
    .fetch_one(&pool)
    .await
    .expect("insert token log");

    assert!(
        !sqlite_column_exists(&pool, "request_logs", "legacy_request_kind_key").await,
        "current schema should not expose request_logs legacy snapshot columns"
    );
    assert!(
        !sqlite_column_exists(&pool, "auth_token_logs", "legacy_request_kind_key").await,
        "current schema should not expose auth_token_logs legacy snapshot columns"
    );

    run_backfill(&db_str, 1);

    let request_row = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM request_logs
        WHERE id = ?
        "#,
    )
    .bind(request_log_id)
    .fetch_one(&pool)
    .await
    .expect("request row after backfill");
    assert_eq!(
        request_row
            .try_get::<String, _>("request_kind_key")
            .unwrap(),
        "mcp:unsupported-path"
    );
    assert_eq!(
        request_row
            .try_get::<String, _>("request_kind_label")
            .unwrap(),
        "MCP | unsupported path"
    );
    assert_eq!(
        request_row
            .try_get::<Option<String>, _>("request_kind_detail")
            .unwrap()
            .as_deref(),
        Some("/mcp/search")
    );

    let token_row = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM auth_token_logs
        WHERE id = ?
        "#,
    )
    .bind(token_log_id)
    .fetch_one(&pool)
    .await
    .expect("token row after backfill");
    assert_eq!(
        token_row.try_get::<String, _>("request_kind_key").unwrap(),
        "mcp:third-party-tool"
    );
    assert_eq!(
        token_row
            .try_get::<String, _>("request_kind_label")
            .unwrap(),
        "MCP | third-party tool"
    );
    assert_eq!(
        token_row
            .try_get::<Option<String>, _>("request_kind_detail")
            .unwrap()
            .as_deref(),
        Some("acme-lookup")
    );

    let request_snapshot = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM request_logs
        WHERE id = ?
        "#,
    )
    .bind(request_log_id)
    .fetch_one(&pool)
    .await
    .expect("request snapshot");
    let token_snapshot = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM auth_token_logs
        WHERE id = ?
        "#,
    )
    .bind(token_log_id)
    .fetch_one(&pool)
    .await
    .expect("token snapshot");

    run_backfill(&db_str, 1);

    let request_row_again = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM request_logs
        WHERE id = ?
        "#,
    )
    .bind(request_log_id)
    .fetch_one(&pool)
    .await
    .expect("request row after second backfill");
    let token_row_again = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM auth_token_logs
        WHERE id = ?
        "#,
    )
    .bind(token_log_id)
    .fetch_one(&pool)
    .await
    .expect("token row after second backfill");

    for column in [
        "request_kind_key",
        "request_kind_label",
        "request_kind_detail",
    ] {
        assert_eq!(
            request_snapshot
                .try_get::<Option<String>, _>(column)
                .expect("snapshot request column"),
            request_row_again
                .try_get::<Option<String>, _>(column)
                .expect("second request column"),
            "request_logs {column} should stay stable across reruns",
        );
        assert_eq!(
            token_snapshot
                .try_get::<Option<String>, _>(column)
                .expect("snapshot token column"),
            token_row_again
                .try_get::<Option<String>, _>(column)
                .expect("second token column"),
            "auth_token_logs {column} should stay stable across reruns",
        );
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_backfill_resumes_from_meta_cursor() {
    let db_path = temp_db_path("request-kind-backfill-resume");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-request-kind-backfill-resume".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("request-kind-backfill-resume"))
        .await
        .expect("token created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("fetch key id");

    let first_request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id, auth_token_id, method, path, status_code, tavily_status_code,
            result_status, request_kind_key, request_kind_label, failure_kind,
            key_effect_code, request_body, response_body, forwarded_headers, dropped_headers, visibility, created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/search', 404, 404, 'error',
            'mcp:raw:/mcp/search', 'MCP | /mcp/search', 'mcp_path_404',
            'none', X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(&key_id)
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .fetch_one(&pool)
    .await
    .expect("insert first request log");
    let second_request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id, auth_token_id, method, path, status_code, tavily_status_code,
            result_status, request_kind_key, request_kind_label, failure_kind,
            key_effect_code, request_body, response_body, forwarded_headers, dropped_headers, visibility, created_at
        ) VALUES (
            ?, ?, 'GET', '/mcp/sse', 404, 404, 'error',
            'mcp:raw:/mcp/sse', 'MCP | /mcp/sse', 'mcp_path_404',
            'none', X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(&key_id)
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 1)
    .fetch_one(&pool)
    .await
    .expect("insert second request log");

    let first_token_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO auth_token_logs (
            token_id, api_key_id, method, path, http_status, mcp_status,
            request_kind_key, request_kind_label, result_status, key_effect_code, created_at, counts_business_quota, billing_state
        ) VALUES (
            ?, ?, 'POST', '/mcp', 200, 200,
            'mcp:tool:acme-first', 'MCP | acme-first', 'success', 'none', ?, 0, 'none'
        )
        RETURNING id
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .bind(Utc::now().timestamp() + 2)
    .fetch_one(&pool)
    .await
    .expect("insert first token log");
    let second_token_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO auth_token_logs (
            token_id, api_key_id, method, path, http_status, mcp_status,
            request_kind_key, request_kind_label, result_status, key_effect_code, created_at, counts_business_quota, billing_state
        ) VALUES (
            ?, ?, 'POST', '/mcp', 200, 200,
            'mcp:tool:acme-second', 'MCP | acme-second', 'success', 'none', ?, 0, 'none'
        )
        RETURNING id
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .bind(Utc::now().timestamp() + 3)
    .fetch_one(&pool)
    .await
    .expect("insert second token log");

    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES ('request_kind_canonical_backfill_request_logs_v1', ?),
               ('request_kind_canonical_backfill_auth_token_logs_v1', ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(first_request_log_id.to_string())
    .bind(first_token_log_id.to_string())
    .execute(&pool)
    .await
    .expect("seed backfill cursors");

    run_backfill(&db_str, 1);

    let first_request_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM request_logs WHERE id = ?")
            .bind(first_request_log_id)
            .fetch_one(&pool)
            .await
            .expect("first request kind");
    let second_request_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM request_logs WHERE id = ?")
            .bind(second_request_log_id)
            .fetch_one(&pool)
            .await
            .expect("second request kind");
    assert_eq!(first_request_kind, "mcp:raw:/mcp/search");
    assert_eq!(second_request_kind, "mcp:unsupported-path");

    let first_token_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM auth_token_logs WHERE id = ?")
            .bind(first_token_log_id)
            .fetch_one(&pool)
            .await
            .expect("first token kind");
    let second_token_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM auth_token_logs WHERE id = ?")
            .bind(second_token_log_id)
            .fetch_one(&pool)
            .await
            .expect("second token kind");
    assert_eq!(first_token_kind, "mcp:tool:acme-first");
    assert_eq!(second_token_kind, "mcp:third-party-tool");

    let _ = std::fs::remove_file(db_path);
}

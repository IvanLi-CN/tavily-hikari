pub(super) async fn wait_for_rebalance_audit_count(
    pool: &sqlx::SqlitePool,
    expected_count: usize,
) -> Vec<sqlx::sqlite::SqliteRow> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let rows = sqlx::query(
                r#"
                SELECT status_code, failure_kind, fallback_reason, request_body
                FROM observability.request_logs
                WHERE path = '/mcp'
                ORDER BY id ASC
                "#,
            )
            .fetch_all(pool)
            .await
            .expect("fetch rebalance audit records");
            if rows.len() == expected_count {
                return rows;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("rebalance audits should flush promptly")
}

pub(super) async fn wait_for_rebalance_audit_with_fallback(
    pool: &sqlx::SqlitePool,
    fallback_reason: &str,
) -> sqlx::sqlite::SqliteRow {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let row = sqlx::query(
                r#"
                SELECT status_code, failure_kind, fallback_reason
                FROM observability.request_logs
                WHERE path = '/mcp'
                  AND fallback_reason = ?
                ORDER BY id DESC
                LIMIT 1
                "#,
            )
            .bind(fallback_reason)
            .fetch_optional(pool)
            .await
            .expect("fetch rebalance audit record");
            if let Some(row) = row {
                return row;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("rebalance audit should flush promptly")
}

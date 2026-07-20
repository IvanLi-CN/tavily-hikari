use super::*;
use super::core_support_and_parsing::temp_db_path;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn admin_system_status_reports_reconciliation_diagnostic_timestamps() {
    let db_path = temp_db_path("admin-system-status-reconciliation-diagnostics");
    let db_str = db_path.to_string_lossy().to_string();
    let upstream_addr = spawn_forward_proxy_probe_upstream().await;
    let upstream = format!("http://{upstream_addr}/mcp");
    let usage_base = format!("http://{upstream_addr}");
    let proxy = TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
        .await
        .expect("create proxy");
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_str)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .expect("open diagnostics meta pool");

    for (key, value) in [
        ("upstream_reconciliation_last_run_at_v1", 1_783_958_250_i64),
        (
            "upstream_reconciliation_last_shadow_adjustment_at_v1",
            1_783_958_100_i64,
        ),
        (
            "upstream_reconciliation_last_enqueue_error_at_v1",
            1_783_957_900_i64,
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value.to_string())
        .execute(&pool)
        .await
        .expect("seed reconciliation diagnostic marker");
    }

    let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;
    let status_response = Client::new()
        .get(format!("http://{addr}/api/settings/system/status"))
        .send()
        .await
        .expect("get system status");
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_body = status_response
        .json::<serde_json::Value>()
        .await
        .expect("decode system status");
    assert_eq!(
        status_body["lastReconciliationRunAt"].as_i64(),
        Some(1_783_958_250)
    );
    assert_eq!(
        status_body["lastShadowAdjustmentAt"].as_i64(),
        Some(1_783_958_100)
    );
    assert_eq!(
        status_body["lastReconciliationEnqueueErrorAt"].as_i64(),
        Some(1_783_957_900)
    );

    let _ = std::fs::remove_file(db_path);
}

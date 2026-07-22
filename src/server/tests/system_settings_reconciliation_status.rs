use super::*;
use super::core_support_and_parsing::temp_db_path;
use super::upstream_support_and_manual_jobs::*;
use tavily_hikari::business_period_for_timestamp;

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
    let current_period = business_period_for_timestamp(proxy.backend_time().now_ts());
    for (token_id, key_id, project_id, billing_subject) in [
        ("token-hot-a", "key-hot", "project-hot-a", "account:user-a"),
        ("token-hot-b", "key-hot", "project-hot-b", "account:user-b"),
        ("token-cold-a", "key-cold", "project-cold-a", "account:user-c"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                period_start, period_end, request_count, first_used_at, last_used_at, updated_at,
                settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(token_id)
        .bind(key_id)
        .bind(&current_period.code)
        .bind(project_id)
        .bind(billing_subject)
        .bind(current_period.starts_at)
        .bind(current_period.ends_at)
        .bind(current_period.starts_at + 60)
        .bind(current_period.starts_at + 120)
        .bind(current_period.starts_at + 120)
        .execute(&pool)
        .await
        .expect("seed current period reconciliation usage");
    }
    for (settlement_key, token_id, period_code, project_id, billing_subject, reason) in [
        (
            "v1:token-hot-a:current",
            "token-hot-a",
            current_period.code.as_str(),
            "project-hot-a",
            "account:user-a",
            "upstream429",
        ),
        (
            "v1:token-older-429:2026-07-15/S1",
            "token-older-429",
            "2026-07-15/S1",
            "project-older-429",
            "account:user-older-429",
            "usage http error 429 Too Many Requests",
        ),
        (
            "v1:token-local:2026-07-15/S1",
            "token-local",
            "2026-07-15/S1",
            "project-local",
            "account:user-local",
            "local_usage_rate_limit",
        ),
        (
            "v1:token-other:2026-07-15/S1",
            "token-other",
            "2026-07-15/S1",
            "project-other",
            "account:user-other",
            "usage http error 503",
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_settlements (
                settlement_key, token_id, period_code, project_id, billing_subject,
                period_start, period_end, status, degraded_reason, next_attempt_at,
                attempt_count, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 'rate_limited', ?, ?, 1, ?, ?)
            "#,
        )
        .bind(settlement_key)
        .bind(token_id)
        .bind(period_code)
        .bind(project_id)
        .bind(billing_subject)
        .bind(current_period.starts_at - 7_200)
        .bind(current_period.starts_at - 3_600)
        .bind(reason)
        .bind(current_period.ends_at + 60)
        .bind(current_period.starts_at)
        .bind(current_period.starts_at)
        .execute(&pool)
        .await
        .expect("seed rate limited settlement");
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
    assert_eq!(status_body["retryBuckets"]["upstream429"].as_i64(), Some(2));
    assert_eq!(
        status_body["retryBuckets"]["localUsageRateLimit"].as_i64(),
        Some(1)
    );
    assert_eq!(status_body["retryBuckets"]["other"].as_i64(), Some(1));
    assert_eq!(
        status_body["currentPeriodBoundUsersByKey"][0]["keyIdHint"].as_str(),
        Some("key-hot")
    );
    assert_eq!(
        status_body["currentPeriodBoundUsersByKey"][0]["count"].as_i64(),
        Some(2)
    );
    assert_eq!(
        status_body["currentPeriodPendingProjectIdsByKey"][0]["keyIdHint"].as_str(),
        Some("key-hot")
    );
    assert_eq!(
        status_body["currentPeriodPendingProjectIdsByKey"][0]["count"].as_i64(),
        Some(2)
    );

    let _ = std::fs::remove_file(db_path);
}

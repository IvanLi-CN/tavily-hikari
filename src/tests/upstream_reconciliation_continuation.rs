use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_work_projection_uses_period_index() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-query-plan".to_string()],
        "http://127.0.0.1:9",
        &db_string,
    )
    .await
    .expect("create proxy");

    let plan_rows = sqlx::query_as::<_, (i64, i64, i64, String)>(
        r#"EXPLAIN QUERY PLAN
           SELECT token_id, period_code
           FROM upstream_reconciliation_work
           WHERE period_end >= ? AND period_end < ?
           ORDER BY period_end, scheduling_key_id, token_id, period_code
           LIMIT 20"#,
    )
    .bind(0_i64)
    .bind(i64::MAX)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("explain reconciliation projection query");
    let plan = plan_rows
        .into_iter()
        .map(|(_, _, _, detail)| detail)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        plan.contains("idx_upstream_reconciliation_work_period"),
        "reconciliation projection must use its bounded period index: {plan}"
    );
    assert!(
        !plan.contains("SCAN upstream_reconciliation_usage"),
        "candidate selection must not aggregate the raw usage table: {plan}"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_continuation_waits_for_pending_research_poll() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-continuation"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("enable reconciliation shadow gate");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-research-continuation")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-research-continuation', ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("research-continuation-token")
    .bind(&key_id)
    .bind("token:research-continuation-token")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending reconciliation work");
    let usage_tail: i64 = sqlx::query_scalar(
        "SELECT MAX(rowid) FROM upstream_reconciliation_usage WHERE token_id = 'research-continuation-token'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read projected usage tail");
    // This case is about pending research only. An unread source row is deliberately immediate
    // durable projection work, even if an already-projected period has a later research poll.
    sqlx::query(
        "INSERT INTO meta (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind("upstream_reconciliation_work_cursor_v1")
    .bind(usage_tail.to_string())
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark source projection caught up");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, next_poll_at, updated_at
        ) VALUES (?, ?, ?, '2026-07-15/S1', ?, NULL, ?, ?)
        "#,
    )
    .bind("research-continuation-request")
    .bind("research-continuation-token")
    .bind(&key_id)
    .bind(now - 900)
    .bind(now + 120)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert delayed pending research");

    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_research_drain_available_at()
            .await
            .expect("read delayed research continuation"),
        Some(now + 120)
    );
    proxy
        .ensure_upstream_reconciliation_research_drain_job()
        .await
        .expect("enqueue delayed research continuation");
    let scheduled: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), MAX(available_at) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation_research_drain' AND status = 'queued'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read delayed representative");
    assert_eq!(scheduled, (1, now + 120));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_disabled_gate_preserves_work_without_representative_churn() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-disabled-gate"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (token_id, key_id, period_code, project_id, \
         billing_subject, period_start, period_end, request_count, first_used_at, last_used_at, \
         updated_at, settlement_mode) VALUES ('disabled-gate-token', 'disabled-gate-key', \
         '2026-07-15/S1', 'disabled-gate-project', 'token:disabled-gate-token', ?, ?, 1, ?, ?, ?, \
         'shadow')",
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed eligible Research usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, next_poll_at, updated_at
        ) VALUES ('disabled-gate-research', 'disabled-gate-token', 'disabled-gate-key',
                  '2026-07-15/S1', ?, NULL, ?, ?)
        "#,
    )
    .bind(now - 60)
    .bind(now)
    .bind(now - 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert due research");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1,
            0,
        )
        .await
        .expect("mark durable projection pending");

    let pending_research: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_research WHERE terminal_at IS NULL",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read raw pending Research diagnostics");
    assert_eq!(pending_research, 1);
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_continuation_at()
            .await
            .expect("read raw main continuation"),
        None,
        "Research does not wake the main representative"
    );
    assert_eq!(
        proxy
            .upstream_reconciliation_representative_available_at()
            .await
            .expect("read runnable continuation while disabled"),
        None,
        "a disabled gate must not requeue a no-op worker"
    );
    proxy
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("suppress disabled representative");
    proxy
        .ensure_upstream_reconciliation_research_drain_job()
        .await
        .expect("suppress disabled Research drain");
    let disabled_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type IN ('upstream_reconciliation', 'upstream_reconciliation_research_drain') AND status IN ('queued', 'running')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count disabled representatives");
    assert_eq!(disabled_jobs, 0);

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("load disabled settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("enable reconciliation shadow gate");
    proxy
        .ensure_upstream_reconciliation_research_drain_job()
        .await
        .expect("enqueue enabled Research drain");
    let enabled_job: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), MIN(available_at) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation_research_drain' AND status = 'queued'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read resumed representative");
    assert_eq!(enabled_job, (1, now));

    let _ = std::fs::remove_file(db_path);
}

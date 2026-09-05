use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_observation_reports_due_window_without_queue_count() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-queue"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind("token-queued")
    .bind("key-queued")
    .bind("2026-07-15/S1")
    .bind("project-queued")
    .bind("token:token-queued")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert queued usage");

    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind("token-research")
    .bind("key-research")
    .bind("2026-07-15/S1")
    .bind("project-research")
    .bind("token:token-research")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert research usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind("research-1")
    .bind("token-research")
    .bind("key-research")
    .bind("2026-07-15/S1")
    .bind(now - 950)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending research");

    let observation = proxy
        .key_store
        .upstream_reconciliation_observation()
        .await
        .expect("read bounded reconciliation observation");
    assert_eq!(observation.coverage, "unknown");
    assert!(observation.queue_estimate.is_none());
    assert!(observation.has_eligible);
    assert!(
        observation
            .oldest_candidate_age_secs
            .is_some_and(|age| age >= 900)
    );

    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_settlements (
            settlement_key, token_id, period_code, project_id, billing_subject,
            period_start, period_end, status, next_attempt_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 'rate_limited', ?, ?, ?)
        "#,
    )
    .bind("v1:token-queued:2026-07-15/S1")
    .bind("token-queued")
    .bind("2026-07-15/S1")
    .bind("project-queued")
    .bind("token:token-queued")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now + 300)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("delay queued settlement");
    let delayed_observation = proxy
        .key_store
        .upstream_reconciliation_observation()
        .await
        .expect("read delayed reconciliation observation");
    assert!(!delayed_observation.has_eligible);
    assert!(delayed_observation.oldest_candidate_age_secs.is_none());

    for suffix in ["second", "third"] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                period_start, period_end, request_count, first_used_at, last_used_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
            "#,
        )
        .bind(format!("token-{suffix}"))
        .bind(format!("key-{suffix}"))
        .bind("2026-07-15/S1")
        .bind(format!("project-{suffix}"))
        .bind(format!("token:token-{suffix}"))
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert additional queued usage");
    }
    proxy
        .key_store
        .mark_upstream_reconciliation_run_completed_at(now)
        .await
        .expect("mark reconciliation observation ready");
    let observed = proxy
        .key_store
        .upstream_reconciliation_observation()
        .await
        .expect("read observed bounded queue estimate");
    assert_eq!(observed.coverage, "bounded");
    assert_eq!(observed.queue_estimate, Some(2));

    let _ = std::fs::remove_file(db_path);
}

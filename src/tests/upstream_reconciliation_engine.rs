use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_transport_observation_survives_a_following_non_transport_run() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 19, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-transport-state"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    proxy
        .key_store
        .record_upstream_reconciliation_engine_observation(
            crate::store::ReconciliationRunObservationWrite {
                claimed_job: None,
                mode: "compare",
                hydrate_ms: 1,
                first_remote_ms: Some(2),
                remote_ms: 3,
                finalization_ms: 1,
                research_ms: 0,
                settled: 0,
                no_adjustment: 0,
                observed: 0,
                upstream_429: 0,
                transport_failure: 1,
                semantic_failure: 0,
                local_pressure: 0,
                last_transport_kind: Some("timeout"),
                last_retryable_outcome: Some("transport_failure"),
                continuation_reason: Some("transport_failure"),
                next_retry_at: Some(now + 30),
            },
        )
        .await
        .expect("record transport observation");

    let first = proxy
        .key_store
        .upstream_reconciliation_run_observation()
        .await
        .expect("read transport observation");
    assert_eq!(first.last_transport_kind.as_deref(), Some("timeout"));
    assert_eq!(first.last_transport_kind_at, Some(now));
    assert_eq!(
        first.last_retryable_outcome.as_deref(),
        Some("transport_failure")
    );

    proxy
        .key_store
        .record_upstream_reconciliation_engine_observation(
            crate::store::ReconciliationRunObservationWrite {
                claimed_job: None,
                mode: "compare",
                hydrate_ms: 1,
                first_remote_ms: None,
                remote_ms: 0,
                finalization_ms: 1,
                research_ms: 0,
                settled: 0,
                no_adjustment: 1,
                observed: 0,
                upstream_429: 0,
                transport_failure: 0,
                semantic_failure: 0,
                local_pressure: 0,
                last_transport_kind: None,
                last_retryable_outcome: None,
                continuation_reason: Some("no_adjustment"),
                next_retry_at: None,
            },
        )
        .await
        .expect("record terminal observation");

    let recovered = proxy
        .key_store
        .upstream_reconciliation_run_observation()
        .await
        .expect("read recovered observation");
    assert_eq!(recovered.last_transport_kind.as_deref(), Some("timeout"));
    assert_eq!(recovered.last_transport_kind_at, Some(now));
    assert_eq!(recovered.last_retryable_outcome, None);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}
use axum::{Json, Router, routing::get};
use tokio::net::TcpListener;

#[test]
fn projection_defer_does_not_block_existing_main_candidates() {
    assert!(!ReconciliationEngine::projection_defer_exhausts_preparation(20));
    assert!(ReconciliationEngine::projection_defer_exhausts_preparation(
        0
    ));
}

#[tokio::test]
async fn reconciliation_one_shot_waits_for_a_transient_bulk_admission() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_752_500_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-one-shot-admission"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    let admission = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            match proxy.admit_upstream_reconciliation_projection() {
                SqliteAdmissionOutcome::Admitted(admission) => return admission,
                SqliteAdmissionOutcome::Deferred { .. } => {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        }
    })
    .await
    .expect("obtain temporary reconciliation admission");

    let release_admission = async {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        drop(admission);
    };
    let (settled, ()) = tokio::join!(
        proxy.run_upstream_reconciliation_once("http://127.0.0.1:9"),
        release_admission,
    );
    assert_eq!(
        settled.expect("one-shot reconciliation completes after admission clears"),
        0
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_failure_states_are_independent() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-independent-failures"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let candidate = UpstreamReconciliationCandidate {
        token_id: "independent-failure-token".to_string(),
        period_code: "2026-07-15/S1".to_string(),
        project_id: "independent-failure-project".to_string(),
        billing_subject: "token:independent-failure-token".to_string(),
        settlement_mode: "shadow".to_string(),
        period_start: now - 4_000,
        period_end: now - 900,
        pending_research: 0,
        degraded: false,
    };
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_work (
             token_id, period_code, project_id, billing_subject, settlement_mode,
             period_start, period_end, scheduling_key_id, updated_at
           ) VALUES (?, ?, ?, ?, ?, ?, ?, 'independent-failure-key', ?)"#,
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .bind(&candidate.project_id)
    .bind(&candidate.billing_subject)
    .bind(&candidate.settlement_mode)
    .bind(candidate.period_start)
    .bind(candidate.period_end)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert durable work");

    proxy
        .key_store
        .mark_reconciliation_retry(
            &candidate,
            "waiting",
            now,
            Some("transport"),
            RECONCILIATION_OUTCOME_TRANSPORT_FAILURE,
            None,
        )
        .await
        .expect("record transport failure");
    proxy
        .key_store
        .mark_reconciliation_retry(
            &candidate,
            "waiting",
            now,
            Some("semantic"),
            RECONCILIATION_OUTCOME_SEMANTIC_FAILURE,
            None,
        )
        .await
        .expect("record semantic failure");
    let state: (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT transport_failure_streak, transport_retry_at,
                  semantic_failure_streak, semantic_retry_at, next_attempt_at
           FROM upstream_reconciliation_work
           WHERE token_id = ? AND period_code = ?"#,
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read independent retry state");
    assert_eq!(state, (1, now + 30, 1, now + 300, now + 300));

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_is_cancellation_safe() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_752_500_000);
    let proxy = Arc::new(
        TavilyProxy::with_options_and_time(
            vec!["tvly-reconciliation-projection-cancel"],
            "http://127.0.0.1:9",
            &db_string,
            TavilyProxyOptions::from_database_path(&db_string),
            backend_time,
        )
        .await
        .expect("create proxy"),
    );
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('cancel-token', 'cancel-key', '2026-07-15/S1', 'cancel-project',
                     'token:cancel-token', 1, 2, 1, 1, 2, 2, 'shadow')"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending projection source");
    let lock_pool = connect_sqlite_test_pool(&db_string).await;
    let mut writer = lock_pool.acquire().await.expect("acquire writer");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .expect("hold writer lock");
    let cancelled_proxy = Arc::clone(&proxy);
    let task = tokio::spawn(async move {
        cancelled_proxy
            .key_store
            .advance_upstream_reconciliation_work_projection()
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    task.abort();
    assert!(
        task.await
            .expect_err("projection task is cancelled")
            .is_cancelled()
    );
    sqlx::query("ROLLBACK")
        .execute(&mut *writer)
        .await
        .expect("release writer lock");
    drop(writer);
    lock_pool.close().await;

    let cursor: (String, String, String) = sqlx::query_as(
        "SELECT cursor_token_id, cursor_key_id, cursor_period_code FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged cursor");
    assert_eq!(cursor, (String::new(), String::new(), String::new()));
    let tx = proxy
        .key_store
        .sqlite_runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("next transaction begins on a clean connection");
    tx.rollback().await.expect("rollback clean transaction");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn claimed_engine_run_reaches_a_safe_boundary_after_the_caller_is_cancelled() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = Arc::new(
        TavilyProxy::with_endpoint(
            vec!["tvly-reconciliation-projection-owner"],
            "http://127.0.0.1:9",
            &db_string,
        )
        .await
        .expect("create proxy"),
    );
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('owner-token', 'owner-key', '2026-07-15/S1', 'owner-project',
                     'token:owner-token', 1, 2, 1, 1, 2, 2, 'shadow')"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending projection source");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative becomes running");
    let lock_pool = connect_sqlite_test_pool(&db_string).await;
    let mut writer = lock_pool.acquire().await.expect("acquire writer");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .expect("hold writer lock");

    let cancelled_proxy = Arc::clone(&proxy);
    let remote_io_slot = Arc::new(tokio::sync::Semaphore::new(1));
    let remote_io_permit = remote_io_slot
        .clone()
        .try_acquire_owned()
        .expect("acquire remote I/O permit");
    let task = tokio::spawn(async move {
        cancelled_proxy
            .run_upstream_reconciliation_once_claimed_outcome_with_remote_io_permit(
                "http://127.0.0.1:9",
                claim.id,
                claim.claim_generation,
                Some(remote_io_permit),
            )
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    task.abort();
    assert!(task.await.expect_err("caller is cancelled").is_cancelled());
    assert!(
        !proxy
            .key_store
            .sqlite_runtime
            .shutdown_maintenance_bulk(std::time::Duration::from_millis(50))
            .await,
        "shutdown must still observe the detached claimed run"
    );
    assert!(
        remote_io_slot.clone().try_acquire_owned().is_err(),
        "the detached claimed run must retain the remote I/O permit"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut *writer)
        .await
        .expect("release writer lock");
    drop(writer);
    lock_pool.close().await;

    assert!(
        proxy
            .key_store
            .sqlite_runtime
            .shutdown_maintenance_bulk(std::time::Duration::from_secs(2))
            .await,
        "detached claimed run reaches a safe shutdown boundary"
    );
    assert!(
        remote_io_slot.try_acquire_owned().is_ok(),
        "the remote I/O permit is released only after the run reaches a safe boundary"
    );
    assert_eq!(
        proxy
            .key_store
            .sqlite_runtime
            .discarded_connections_for_test(SqliteOperation::ReconciliationProjection),
        0,
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_rolls_back_sql_errors_without_discarding_connection() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-projection-error"],
        "http://127.0.0.1:9",
        &db_string,
    )
    .await
    .expect("create proxy");
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('error-token', 'error-key', '2026-07-15/S1', 'error-project',
                     'token:error-token', 1, 2, 1, 1, 2, 2, 'shadow')"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending projection source");
    sqlx::query(
        r#"CREATE TRIGGER fail_projection_work_insert
           BEFORE INSERT ON upstream_reconciliation_work
           BEGIN
             SELECT RAISE(ABORT, 'injected projection write failure');
           END"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("inject projection write failure");

    let err = proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect_err("projection write fails");
    assert!(
        err.to_string()
            .contains("injected projection write failure")
    );
    assert_eq!(
        proxy
            .key_store
            .sqlite_runtime
            .discarded_connections_for_test(SqliteOperation::ReconciliationProjection),
        0,
        "ordinary SQL errors must rollback instead of discarding the connection"
    );
    let tx = proxy
        .key_store
        .sqlite_runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("next transaction begins on the rolled-back connection");
    tx.rollback().await.expect("rollback clean transaction");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_returns_read_errors_without_discarding_connection() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-projection-read-error"],
        "http://127.0.0.1:9",
        &db_string,
    )
    .await
    .expect("create proxy");
    sqlx::query("DROP TABLE upstream_reconciliation_usage")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove projection source");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");

    proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect_err("projection source read fails");
    assert_eq!(
        proxy
            .key_store
            .sqlite_runtime
            .discarded_connections_for_test(SqliteOperation::ReconciliationProjection),
        0,
        "ordinary read errors must return the operation connection to the pool"
    );
    let tx = proxy
        .key_store
        .sqlite_runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("next transaction begins after the read error");
    tx.rollback().await.expect("rollback clean transaction");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_rejects_stale_claim() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, clock) = BackendTime::manual_from_ts(1_752_500_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-stale"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative becomes running");
    clock.set_now_ts(1_752_500_061);
    assert_eq!(proxy.recover_stale_scheduled_jobs().await.unwrap(), 1);

    assert_eq!(
        proxy
            .key_store
            .advance_upstream_reconciliation_work_projection_claimed(
                claim.id,
                claim.claim_generation,
            )
            .await
            .expect("reject stale projection claim"),
        ReconciliationProjectionSliceOutcome::StaleClaim
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn unclaimed_projection_preserves_typed_defer() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-projection-defer"],
        "http://127.0.0.1:9",
        &db_string,
    )
    .await
    .expect("create proxy");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");
    let lock_pool = connect_sqlite_test_pool(&db_string).await;
    let mut writer = lock_pool.acquire().await.expect("acquire writer");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .expect("hold writer lock");

    assert!(matches!(
        proxy
            .key_store
            .advance_upstream_reconciliation_work_projection()
            .await
            .expect("projection returns typed pressure"),
        ReconciliationProjectionSliceOutcome::Deferred {
            reason: "sqlite_pressure"
        }
    ));

    sqlx::query("ROLLBACK")
        .execute(&mut *writer)
        .await
        .expect("release writer lock");
    drop(writer);
    lock_pool.close().await;
    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn projection_pressure_defer_persists_state_when_the_write_window_recovers() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-persisted-defer"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark projection incomplete");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('pressure-token', 'pressure-key', '2026-07-15/S1', 'pressure-project',
                     'token:pressure-token', 1, 2, 1, 1, 2, 2, 'shadow')"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert projection source");
    sqlx::query(
        r#"CREATE TRIGGER defer_projection_work_insert
           BEFORE INSERT ON upstream_reconciliation_work
           BEGIN
             SELECT RAISE(ABORT, 'database is locked');
           END"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("inject transient projection write failure");

    assert_eq!(
        proxy
            .key_store
            .advance_upstream_reconciliation_work_projection()
            .await
            .expect("projection returns a typed defer"),
        ReconciliationProjectionSliceOutcome::Deferred {
            reason: "sqlite_pressure"
        }
    );
    let state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read persisted deferred projection state");
    assert_eq!(state, (now + 30, Some("sqlite_pressure".to_string())));
    let observation = proxy
        .key_store
        .upstream_reconciliation_run_observation()
        .await
        .expect("read projection observation");
    assert_eq!(observation.projection_state, "deferred");
    assert_eq!(
        observation.continuation_reason.as_deref(),
        Some("sqlite_pressure")
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn settlement_sqlite_pressure_returns_a_typed_defer_without_completing_work() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-settlement-pressure"],
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
    settings.rebalance_mcp_enabled = true;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("enable compare reconciliation");
    let token = proxy
        .create_access_token(Some("reconciliation-settlement-pressure"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-settlement-pressure")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES (?, ?, '2026-07-15/S1', 'settlement-pressure-project', ?,
                     ?, ?, 1, ?, ?, ?, 'shadow')"#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .bind(format!("token:{}", token.id))
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert due reconciliation work");
    sqlx::query(
        r#"CREATE TRIGGER defer_reconciliation_settlement
           BEFORE INSERT ON upstream_reconciliation_settlements
           BEGIN
             SELECT RAISE(ABORT, 'database is locked');
           END"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("inject transient settlement failure");
    let app = Router::new().route(
        "/usage",
        get(|| async { Json(serde_json::json!({ "key": { "usage": 5 } })) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind usage upstream");
    let address = listener.local_addr().expect("read usage upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve usage upstream");
    });
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");

    assert_eq!(
        proxy
            .run_upstream_reconciliation_once_claimed_outcome(
                &format!("http://{address}"),
                claim.id,
                claim.claim_generation,
            )
            .await
            .expect("SQLite pressure is a typed run outcome"),
        ClaimedReconciliationRunOutcome::Deferred {
            reason: "local_pressure"
        }
    );
    let generations: (i64, i64) = sqlx::query_as(
        "SELECT work_generation, completed_generation FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = '2026-07-15/S1'",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unfinished reconciliation work");
    assert!(
        generations.0 > generations.1,
        "a pressure defer must not terminally complete the observed work"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_without_an_eligible_key_records_semantic_retry() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-missing-key"],
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
        .expect("enable compare reconciliation");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_work (
             token_id, period_code, project_id, billing_subject, settlement_mode,
             period_start, period_end, scheduling_key_id, updated_at
           ) VALUES ('missing-key-token', '2026-07-15/S1', 'missing-key-project',
                     'token:missing-key-token', 'shadow', ?, ?, 'deleted-key', ?)"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert work without an eligible key");

    proxy
        .run_upstream_reconciliation_once("http://127.0.0.1:9")
        .await
        .expect("run reconciliation without a key");
    let retry: (String, i64, i64, i64) = sqlx::query_as(
        r#"SELECT last_outcome, semantic_failure_streak, semantic_retry_at,
                  completed_generation
           FROM upstream_reconciliation_work
           WHERE token_id = 'missing-key-token' AND period_code = '2026-07-15/S1'"#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read semantic retry state");
    assert_eq!(retry.0, RECONCILIATION_OUTCOME_SEMANTIC_FAILURE);
    assert_eq!(retry.1, 1);
    assert_eq!(retry.2, now + 300);
    assert_eq!(retry.3, 0, "retryable work must remain incomplete");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_does_not_partially_fetch_a_candidate_over_remote_limit() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-multi-key"],
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
        .expect("enable compare reconciliation");
    for index in 0..3 {
        let key_id = proxy
            .add_or_undelete_key(&format!("tvly-reconciliation-multi-key-{index}"))
            .await
            .expect("create upstream key");
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 period_start, period_end, request_count, first_used_at,
                 last_used_at, updated_at, settlement_mode
               ) VALUES ('multi-key-token', ?, '2026-07-15/S1', 'multi-key-project',
                         'token:multi-key-token', ?, ?, 1, ?, ?, ?, 'shadow')"#,
        )
        .bind(key_id)
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert multi-key usage");
    }

    proxy
        .run_upstream_reconciliation_once("http://127.0.0.1:9")
        .await
        .expect("classify candidate before remote fetch");
    let state: (String, i64, i64) = sqlx::query_as(
        r#"SELECT last_outcome, semantic_failure_streak, completed_generation
           FROM upstream_reconciliation_work
           WHERE token_id = 'multi-key-token' AND period_code = '2026-07-15/S1'"#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read multi-key retry state");
    assert_eq!(state.0, RECONCILIATION_OUTCOME_SEMANTIC_FAILURE);
    assert_eq!(state.1, 1);
    assert_eq!(state.2, 0, "partial fetch must not complete work");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

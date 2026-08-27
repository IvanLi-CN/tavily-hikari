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

#[tokio::test]
async fn post_process_defer_finalization_is_atomic_and_never_marks_the_claim_error() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 1, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-atomic-defer"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    let retry_at = now + 30;

    let continuation = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            "local_pressure",
            retry_at,
        )
        .await
        .expect("atomically finalize deferred claim");
    let current: (String, String) =
        sqlx::query_as("SELECT status, message FROM scheduled_jobs WHERE id = ?")
            .bind(claim.id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read completed claim");
    assert_eq!(current.0, "success");
    assert_ne!(current.0, "error");
    assert!(current.1.contains("defer_reason=local_pressure"));
    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status IN ('queued', 'running')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count active representatives");
    assert_eq!(active_count, 1);
    let continuation_available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(continuation.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read continuation schedule");
    assert_eq!(continuation_available_at, retry_at);
    let observation = proxy
        .key_store
        .upstream_reconciliation_run_observation()
        .await
        .expect("read deferred observation");
    assert_eq!(
        observation.last_retryable_outcome.as_deref(),
        Some("local_pressure")
    );
    assert_eq!(observation.next_retry_at, Some(retry_at));
    let local_backoff: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM meta WHERE key IN ('upstream_reconciliation_local_pressure_streak_v1', 'upstream_reconciliation_local_backoff_level_v1') ORDER BY key",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read atomically updated local backoff");
    assert_eq!(
        local_backoff,
        vec![
            (
                "upstream_reconciliation_local_backoff_level_v1".to_string(),
                "0".to_string(),
            ),
            (
                "upstream_reconciliation_local_pressure_streak_v1".to_string(),
                "1".to_string(),
            ),
        ]
    );
    assert!(matches!(
        proxy
            .finalize_deferred_upstream_reconciliation_claim(
                claim.id,
                claim.claim_generation,
                "local_pressure",
                retry_at,
            )
            .await,
        Err(ProxyError::StaleClaim { .. })
    ));
    let stale_backoff: String = sqlx::query_scalar(
        "SELECT value FROM meta WHERE key = 'upstream_reconciliation_local_pressure_streak_v1'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read stale-finalization backoff state");
    assert_eq!(stale_backoff, "1");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

async fn record_research_progress_window_observation(
    proxy: &TavilyProxy,
) -> Result<(), ProxyError> {
    proxy
        .key_store
        .record_upstream_reconciliation_engine_observation(
            crate::store::ReconciliationRunObservationWrite {
                claimed_job: None,
                mode: "compare",
                hydrate_ms: 0,
                first_remote_ms: None,
                remote_ms: 0,
                finalization_ms: 0,
                research_ms: 0,
                settled: 0,
                no_adjustment: 0,
                observed: 0,
                upstream_429: 0,
                transport_failure: 0,
                semantic_failure: 0,
                local_pressure: 0,
                last_transport_kind: None,
                last_retryable_outcome: None,
                continuation_reason: Some("observed"),
                next_retry_at: None,
            },
        )
        .await
}

#[tokio::test]
async fn research_progress_window_requires_terminal_progress_without_pending_growth() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-window"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let period_code = "2026-08-21/S1";
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind("research-window-token")
    .bind("research-window-key")
    .bind(period_code)
    .bind("research-window-project")
    .bind("token:research-window-token")
    .bind(now - 60)
    .bind(now + 3_600)
    .bind(now - 60)
    .bind(now - 60)
    .bind(now - 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed current-period research usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind("research-window-request")
    .bind("research-window-token")
    .bind("research-window-key")
    .bind(period_code)
    .bind(now - 60)
    .bind(now - 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pending research");

    record_research_progress_window_observation(&proxy)
        .await
        .expect("start research observation window");
    clock.set_now_ts(now + 600);
    record_research_progress_window_observation(&proxy)
        .await
        .expect("complete stalled research observation window");
    let stalled = proxy
        .upstream_privacy_status()
        .await
        .expect("read stalled research observation");
    assert!(stalled.reconciliation_research_progress_window.complete);
    assert!(
        !stalled
            .reconciliation_research_progress_window
            .terminal_rate_positive
    );
    assert!(
        stalled
            .reconciliation_research_progress_window
            .pending_non_growing
    );

    clock.set_now_ts(now + 601);
    proxy
        .mark_upstream_reconciliation_research_terminal("research-window-request")
        .await
        .expect("mark research terminal before the fixed window boundary");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind("research-window-late-request")
    .bind("research-window-token")
    .bind("research-window-key")
    .bind(period_code)
    .bind(now + 1_201)
    .bind(now + 1_201)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pending research after the fixed window boundary");
    clock.set_now_ts(now + 1_201);
    record_research_progress_window_observation(&proxy)
        .await
        .expect("complete the fixed window from a late observation");
    let advancing = proxy
        .upstream_privacy_status()
        .await
        .expect("read advancing research observation");
    assert!(advancing.reconciliation_research_progress_window.complete);
    assert_eq!(
        advancing
            .reconciliation_research_progress_window
            .window_seconds,
        600
    );
    assert_eq!(
        advancing
            .reconciliation_research_progress_window
            .window_ended_at,
        Some(now + 1_200)
    );
    assert_eq!(
        advancing
            .reconciliation_research_progress_window
            .pending_delta,
        -1,
        "research created after the boundary must not be counted in the completed window"
    );
    assert!(
        advancing
            .reconciliation_research_progress_window
            .terminal_rate_positive
    );
    assert!(
        advancing
            .reconciliation_research_progress_window
            .pending_non_growing
    );
    assert_eq!(
        advancing
            .reconciliation_research_progress_window
            .window_seconds,
        600
    );

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
async fn reconciliation_projection_read_deadline_interrupts_without_discarding_connection() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-projection-read-deadline"],
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
    for index in 0..128 {
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 period_start, period_end, request_count, first_used_at,
                 last_used_at, updated_at, settlement_mode
               ) VALUES (?, ?, ?, ?, ?, 1, 2, 1, 1, 2, 2, 'shadow')"#,
        )
        .bind(format!("deadline-token-{index}"))
        .bind(format!("deadline-key-{index}"))
        .bind(format!("2026-07-15/S{index}"))
        .bind(format!("deadline-project-{index}"))
        .bind(format!("token:deadline-token-{index}"))
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert pending projection source");
    }

    proxy
        .key_store
        .sqlite_runtime
        .force_next_cooperative_query_deadline_for_test();
    let outcome = proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect("source deadline is a typed defer");
    assert!(matches!(
        outcome,
        ReconciliationProjectionSliceOutcome::Deferred {
            reason: "projection_read_budget"
        }
    ));
    let cursor: (String, String, String) = sqlx::query_as(
        "SELECT cursor_token_id, cursor_key_id, cursor_period_code FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged cursor");
    assert_eq!(cursor, (String::new(), String::new(), String::new()));
    assert_eq!(
        proxy
            .key_store
            .sqlite_runtime
            .discarded_connections_for_test(SqliteOperation::ReconciliationProjection),
        0,
        "a native SQLite interrupt must leave a clean connection for the pool"
    );
    let tx = proxy
        .key_store
        .sqlite_runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("next transaction begins after the interrupted source read");
    tx.rollback().await.expect("rollback clean transaction");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_candidate_read_deadline_defers_without_remote_or_billing_changes() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 27, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-candidate-read-deadline"],
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
    for index in 0..128 {
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_work (
                 token_id, period_code, project_id, billing_subject, settlement_mode,
                 period_start, period_end, scheduling_key_id, updated_at
               ) VALUES (?, ?, ?, ?, 'shadow', ?, ?, ?, ?)"#,
        )
        .bind(format!("read-deadline-token-{index}"))
        .bind(format!("2026-08-27/S{index}"))
        .bind(format!("read-deadline-project-{index}"))
        .bind(format!("token:read-deadline-token-{index}"))
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(format!("read-deadline-key-{index}"))
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert eligible reconciliation work");
    }
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");

    // Candidate selection uses the recent and backlog lanes, then key hydration.
    // Interrupt the following BillingHydrate preflight before any remote request.
    proxy
        .key_store
        .sqlite_runtime
        .force_cooperative_query_deadline_after_reads_for_test(3);
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            "http://127.0.0.1:9",
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("candidate deadline becomes a typed defer");
    assert!(matches!(
        outcome,
        ClaimedReconciliationRunOutcome::Deferred {
            reason: "projection_read_budget",
            retry_at,
        } if retry_at == now + 30
    ));
    let generations: (i64, i64) = sqlx::query_as(
        "SELECT work_generation, completed_generation FROM upstream_reconciliation_work WHERE token_id = 'read-deadline-token-0'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged work generation");
    assert!(
        generations.0 > generations.1,
        "a source-read deadline must not complete reconciliation work"
    );
    let settlements: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_settlements WHERE settlement_key = 'v1:read-deadline-token-0:2026-08-27/S0'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count settlements");
    assert_eq!(
        settlements, 0,
        "a source-read deadline starts no settlement"
    );
    assert!(
        proxy
            .key_store
            .sqlite_runtime
            .operation_telemetry(SqliteOperation::ReconciliationProjection)
            .cooperative_read_deadlines
            >= 1,
        "the native SQLite deadline is recorded on the reconciliation operation"
    );

    let persisted = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            "projection_read_budget",
            now + 30,
        )
        .await
        .expect("persist the claim-fenced continuation");
    assert!(
        persisted.created || persisted.promoted,
        "the current claim owns the deferred continuation"
    );
    let representatives: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), MIN(available_at) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read queued representative");
    assert_eq!(representatives, (1, now + 30));

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_research_read_deadline_defers_the_claimed_run() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 27, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-read-deadline"],
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
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("keep the source row out of main reconciliation work");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind("research-deadline-token")
    .bind("research-deadline-key")
    .bind("2026-08-27/R0")
    .bind("research-deadline-project")
    .bind("token:research-deadline-token")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed research source usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind("research-deadline-request")
    .bind("research-deadline-token")
    .bind("research-deadline-key")
    .bind("2026-08-27/R0")
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed due research candidate");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");

    // The empty recent/backlog lanes use the first two source sessions. The
    // due research query is the next source read and must defer, not turn the
    // claimed representative into a scheduler error.
    proxy
        .key_store
        .sqlite_runtime
        .force_cooperative_query_deadline_after_reads_for_test(2);
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            "http://127.0.0.1:9",
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("research source deadline becomes a typed defer");
    assert!(matches!(
        outcome,
        ClaimedReconciliationRunOutcome::Deferred {
            reason: "projection_read_budget",
            retry_at,
        } if retry_at == now + 30
    ));
    let research: (Option<i64>, i64) = sqlx::query_as(
        "SELECT terminal_at, poll_attempt_count FROM upstream_reconciliation_research WHERE request_id = 'research-deadline-request'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged research candidate");
    assert_eq!(
        research,
        (None, 0),
        "deadline must not mutate research work"
    );
    let persisted = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            "projection_read_budget",
            now + 30,
        )
        .await
        .expect("persist the claim-fenced continuation");
    assert!(
        persisted.created || persisted.promoted,
        "the research deadline retains one representative"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_deadline_stops_a_run_before_remote_attempts() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 27, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-read-deadline"],
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
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_work (
             token_id, period_code, project_id, billing_subject, settlement_mode,
             period_start, period_end, scheduling_key_id, updated_at
           ) VALUES ('projection-deadline-token', '2026-08-27/P0', 'projection-project',
             'token:projection-deadline-token', 'shadow', ?, ?, 'projection-key', ?)"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert an eligible candidate");
    for index in 0..128 {
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 period_start, period_end, request_count, first_used_at,
                 last_used_at, updated_at, settlement_mode
               ) VALUES (?, ?, ?, ?, ?, 1, 2, 1, 1, 2, 2, 'shadow')"#,
        )
        .bind(format!("projection-source-token-{index}"))
        .bind(format!("projection-source-key-{index}"))
        .bind(format!("2026-08-27/P{index}"))
        .bind(format!("projection-source-project-{index}"))
        .bind(format!("token:projection-source-token-{index}"))
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert historical projection source");
    }
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");

    // Candidate selection performs the recent and backlog source reads first; interrupt the
    // following historical projection read while an eligible candidate is already present.
    proxy
        .key_store
        .sqlite_runtime
        .force_cooperative_query_deadline_after_reads_for_test(2);
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            "http://127.0.0.1:9",
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("projection deadline becomes a typed defer");
    assert!(matches!(
        outcome,
        ClaimedReconciliationRunOutcome::Deferred {
            reason: "projection_read_budget",
            retry_at,
        } if retry_at == now + 30
    ));
    let work: (i64, i64) = sqlx::query_as(
        "SELECT work_generation, completed_generation FROM upstream_reconciliation_work WHERE token_id = 'projection-deadline-token'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged candidate work");
    assert!(work.0 > work.1, "the candidate remains unfinished");
    let settlements: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_settlements WHERE settlement_key = 'v1:projection-deadline-token:2026-08-27/P0'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count settlements");
    assert_eq!(settlements, 0, "deadline must start no remote request");

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
    let task = tokio::spawn(async move {
        cancelled_proxy
            .run_upstream_reconciliation_once_claimed_outcome_with_remote_attempt_admission(
                "http://127.0.0.1:9",
                claim.id,
                claim.claim_generation,
                None,
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

    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            &format!("http://{address}"),
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("SQLite pressure is a typed run outcome");
    assert!(matches!(
        outcome,
        ClaimedReconciliationRunOutcome::Deferred {
            reason: "local_pressure",
            retry_at,
        } if retry_at >= proxy.backend_time().now_ts().saturating_add(30)
    ));
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

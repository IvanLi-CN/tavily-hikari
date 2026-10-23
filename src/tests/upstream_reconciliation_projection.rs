use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;
use axum::{Json, Router, routing::get};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::net::TcpListener;

#[tokio::test]
async fn reconciliation_candidates_prioritize_partial_observations() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-partial-priority"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-partial-priority")
        .await
        .expect("create shared upstream key");
    let period_start = now - 1_200;
    let period_end = now - 900;
    for (token_id, period_code) in [
        ("token-partial-observation", "2026-07-15/S1-partial"),
        ("token-fresh-candidate", "2026-07-15/S1-fresh"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(token_id)
        .bind(&key_id)
        .bind(period_code)
        .bind(format!("project-{token_id}"))
        .bind(format!("account:{token_id}"))
        .bind(period_start)
        .bind(period_end)
        .bind(period_start)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert candidate usage");
    }
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_key_observations (
             token_id, period_code, work_generation, key_id, upstream_usage, observed_at
           ) VALUES (?, ?, 1, ?, 5, ?)"#,
    )
    .bind("token-partial-observation")
    .bind("2026-07-15/S1-partial")
    .bind(&key_id)
    .bind(now - 10)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert partial key observation");

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(2)
        .await
        .expect("load candidate batch");
    assert_eq!(batch.candidates.len(), 2);
    assert_eq!(
        batch.candidates[0].token_id, "token-partial-observation",
        "a candidate with a durable partial observation must resume before a fresh candidate"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_micro_slices_resume() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-slices"],
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
        .expect("disable live projection for backfill fixture");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1,
            0,
        )
        .await
        .expect("mark historical projection pending");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("reset projection controller state");
    sqlx::query(
        r#"WITH RECURSIVE rows(n) AS (
             VALUES(1) UNION ALL SELECT n + 1 FROM rows WHERE n < 60
           )
           INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           )
           SELECT printf('slice-token-%03d', n), printf('slice-key-%03d', n),
                  '2026-07-15/S1', printf('slice-project-%03d', n),
                  printf('token:slice-%03d', n), ?, ?, 1, ?, ?, ?, 'shadow'
           FROM rows"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert projection fixture");

    proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect("advance one projection slice");
    let projected: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upstream_reconciliation_work")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count projected work");
    assert_eq!(
        projected, 25,
        "the first durable micro-slice starts at 25 rows"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_research_sweep_stays_bounded_without_main_work() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-budget"],
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
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save reconciliation settings");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-research-budget")
        .await
        .expect("create reconciliation key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?)
        "#,
    )
    .bind("research-budget-token")
    .bind(&key_id)
    .bind("2026-07-15/R1")
    .bind("project-research-budget")
    .bind("token:research-budget-token")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .bind("shadow")
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
    .bind("research-budget-slow")
    .bind("research-budget-token")
    .bind(&key_id)
    .bind("2026-07-15/R1")
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending research");
    sqlx::query(
        r#"
        UPDATE upstream_reconciliation_work
        SET completed_generation = work_generation
        WHERE token_id = 'research-budget-token' AND period_code = '2026-07-15/R1'
        "#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("complete main work before research-only run");

    let research_started = Arc::new(AtomicBool::new(false));
    let research_started_for_route = Arc::clone(&research_started);
    let research_request_started_at = Arc::new(std::sync::Mutex::new(None));
    let research_request_started_at_for_route = Arc::clone(&research_request_started_at);
    let app = Router::new().route(
        "/research/research-budget-slow",
        get(move || {
            let research_started = Arc::clone(&research_started_for_route);
            let research_request_started_at = Arc::clone(&research_request_started_at_for_route);
            async move {
                research_started.store(true, Ordering::SeqCst);
                *research_request_started_at
                    .lock()
                    .expect("record research request start") = Some(std::time::Instant::now());
                tokio::time::sleep(Duration::from_secs(10)).await;
                Json(serde_json::json!({ "status": "completed" }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind research upstream");
    let addr = listener
        .local_addr()
        .expect("read research upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve research upstream");
    });

    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation_research_drain", "auto", None, 1)
        .await
        .expect("enqueue claimed research representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim research representative")
        .expect("research representative is claimed");
    // Keep this bounded-sweep regression focused on the remote timeout rather
    // than the first lazy maintenance-pool admission under hosted-runner load.
    proxy
        .prewarm_upstream_reconciliation_projection_capacity()
        .await
        .expect("prewarm reconciliation projection capacity");
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        proxy.run_upstream_reconciliation_research_drain_claimed(
            &format!("http://{addr}"),
            claim.id,
            claim.claim_generation,
            Arc::new(RemoteAttemptAdmissionController::default()),
        ),
    )
    .await
    .expect("run claimed research-only reconciliation within its bounded budget")
    .expect("return a typed claimed outcome");
    assert!(matches!(
        outcome,
        ClaimedResearchDrainOutcome::Persisted {
            polled: 1,
            terminal: 0,
            pending: 0,
            retries: 1,
            unavailable: 0,
            credentials_cooling: 0,
        }
    ));
    assert!(research_started.load(Ordering::SeqCst));
    assert!(
        research_request_started_at
            .lock()
            .expect("read research request start")
            .expect("research request started")
            .elapsed()
            < Duration::from_secs(3),
        "a slow research probe must finish within its two-second sweep after it starts"
    );
    let (_, attempted, _, _, _, budget_exhausted) = proxy
        .key_store
        .upstream_reconciliation_last_run_stats()
        .await
        .expect("read reconciliation observation");
    assert_eq!(attempted, 0);
    assert!(
        !budget_exhausted,
        "research's independent budget must not report primary local pressure"
    );
    let research_retry: (Option<String>, Option<String>, i64, i64) = sqlx::query_as(
        "SELECT last_poll_outcome, last_poll_error_kind, next_poll_at, poll_attempt_count \
         FROM upstream_reconciliation_research WHERE request_id = 'research-budget-slow'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read the durable outcome for the timed-out Research probe");
    assert_eq!(
        research_retry,
        (
            Some("retry".to_string()),
            Some("timeout".to_string()),
            now + 60,
            1,
        ),
        "a bounded Research timeout must persist a retry before the scan cursor is accepted"
    );
    let cursor: (i64, String, String) = sqlx::query_as(
        "SELECT cursor_next_poll_at, cursor_key_id, cursor_request_id \
         FROM upstream_reconciliation_research_scan_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unaccepted scan cursor after a bounded Research timeout");
    assert_eq!(cursor.2, "research-budget-slow");

    let (work_generation, completed_generation): (i64, i64) = sqlx::query_as(
        "SELECT work_generation, completed_generation FROM upstream_reconciliation_work \
         WHERE token_id = 'research-budget-token' AND period_code = '2026-07-15/R1'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read preserved main terminal work");
    assert_eq!(completed_generation, work_generation);
    assert_eq!(
        proxy
            .key_store
            .sqlite_runtime
            .discarded_connections_for_test(SqliteOperation::ReconciliationProjection),
        0
    );
    let transaction = proxy
        .key_store
        .sqlite_runtime
        .begin_immediate(SqliteOperation::ReconciliationProjection)
        .await
        .expect("begin transaction after bounded research read");
    transaction
        .rollback()
        .await
        .expect("rollback reusable projection connection");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_research_selector_rotates_with_a_claim_fenced_cursor() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-selector"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    for index in 0..3 {
        let token_id = format!("selector-token-{index}");
        let key_id = format!("selector-key-{index}");
        let period_code = format!("2026-07-15/R{index}");
        let request_id = format!("selector-request-{index}");
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 period_start, period_end, request_count, first_used_at, last_used_at,
                 updated_at, settlement_mode
               ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')"#,
        )
        .bind(&token_id)
        .bind(&key_id)
        .bind(&period_code)
        .bind(format!("selector-project-{index}"))
        .bind(format!("token:{token_id}"))
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed selector usage");
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_research (
                 request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
               ) VALUES (?, ?, ?, ?, ?, NULL, ?)"#,
        )
        .bind(request_id)
        .bind(token_id)
        .bind(key_id)
        .bind(period_code)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed selector research");
    }

    let query_plan: Vec<String> = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT request_id FROM upstream_reconciliation_research
          WHERE terminal_at IS NULL AND next_poll_at <= 9999999999
          ORDER BY next_poll_at, key_id, request_id LIMIT 80",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("explain research selector")
    .into_iter()
    .map(|row| row.try_get("detail").expect("read selector plan detail"))
    .collect();
    assert!(
        query_plan
            .iter()
            .any(|detail| detail.contains("idx_upstream_reconciliation_research_due_scan")),
        "research selection must seek through the covering due index"
    );

    let first = proxy
        .key_store
        .next_upstream_reconciliation_research_candidates(2)
        .await
        .expect("read first research page");
    assert_eq!(first.candidates.len(), 2);
    let first_ids = first
        .candidates
        .iter()
        .map(|candidate| candidate.request_id.clone())
        .collect::<std::collections::HashSet<_>>();
    proxy
        .key_store
        .accept_upstream_reconciliation_research_cursor(
            first.next_cursor.as_ref(),
            first.wrapped,
            None,
        )
        .await
        .expect("accept first cursor page");

    let second = proxy
        .key_store
        .next_upstream_reconciliation_research_candidates(2)
        .await
        .expect("read second research page");
    assert_eq!(second.candidates.len(), 1);
    assert!(
        second
            .candidates
            .iter()
            .all(|candidate| !first_ids.contains(&candidate.request_id)),
        "accepted cursor must not return a duplicate candidate"
    );
    proxy
        .key_store
        .accept_upstream_reconciliation_research_cursor(
            second.next_cursor.as_ref(),
            second.wrapped,
            None,
        )
        .await
        .expect("accept second cursor page");

    let wrapped = proxy
        .key_store
        .next_upstream_reconciliation_research_candidates(2)
        .await
        .expect("wrap the research cursor once");
    assert!(wrapped.wrapped);
    assert_eq!(wrapped.candidates.len(), 2);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_due_research_uses_reserved_budget_after_a_slow_main_attempt() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-reserve"],
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
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save compare-only settings");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-research-reserve")
        .await
        .expect("create reconciliation key");

    for (token_id, period_code, project_id, billing_subject) in [
        (
            "slow-main-token",
            "2026-07-15/S1",
            "project-slow-main",
            "token:slow-main-token",
        ),
        (
            "reserved-research-token",
            "2026-07-15/R1",
            "project-reserved-research",
            "token:reserved-research-token",
        ),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(token_id)
        .bind(&key_id)
        .bind(period_code)
        .bind(project_id)
        .bind(billing_subject)
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed reconciliation usage");
    }
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind("reserved-research-request")
    .bind("reserved-research-token")
    .bind(&key_id)
    .bind("2026-07-15/R1")
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed due research");
    sqlx::query(
        r#"
        UPDATE upstream_reconciliation_work
        SET completed_generation = work_generation
        WHERE token_id = 'reserved-research-token' AND period_code = '2026-07-15/R1'
        "#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("keep pending research out of main settlement work");

    let usage_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let usage_hits_for_route = Arc::clone(&usage_hits);
    let research_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let research_hits_for_route = Arc::clone(&research_hits);
    let app = Router::new()
        .route(
            "/usage",
            get(move || {
                let usage_hits = Arc::clone(&usage_hits_for_route);
                async move {
                    usage_hits.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(8_500)).await;
                    Json(serde_json::json!({ "key": { "usage": 0 } }))
                }
            }),
        )
        .route(
            "/research/reserved-research-request",
            get(move || {
                let research_hits = Arc::clone(&research_hits_for_route);
                async move {
                    research_hits.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({ "status": "completed" }))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reconciliation upstream");
    let addr = listener
        .local_addr()
        .expect("read reconciliation upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reconciliation upstream");
    });

    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("run reconciliation with a slow main attempt"),
        0
    );
    assert!(
        (1..=2).contains(&usage_hits.load(Ordering::SeqCst)),
        "one eligible main candidate may use the existing bounded HTTP retry path"
    );
    assert_eq!(
        research_hits.load(Ordering::SeqCst),
        1,
        "a due research item must receive its reserved request window after the durable main retry"
    );
    let main_work: (i64, i64, String) = sqlx::query_as(
        r#"
        SELECT work_generation, completed_generation, last_outcome
        FROM upstream_reconciliation_work
        WHERE token_id = 'slow-main-token' AND period_code = '2026-07-15/S1'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read retryable main work");
    assert!(main_work.1 < main_work.0);
    assert_eq!(main_work.2, "transport_failure");
    let research_terminal_at: Option<i64> = sqlx::query_scalar(
        "SELECT terminal_at FROM upstream_reconciliation_research WHERE request_id = 'reserved-research-request'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read research terminal state");
    assert_eq!(research_terminal_at, Some(now));
    let (_, attempted_candidate_count, _, _, _, _) = proxy
        .key_store
        .upstream_reconciliation_last_run_stats()
        .await
        .expect("read reconciliation run stats");
    assert_eq!(
        attempted_candidate_count, 1,
        "the due research reservation must not turn one main work item into another candidate"
    );
    let actual_adjustments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM billing_reconciliation_adjustments")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count actual billing adjustments");
    assert_eq!(actual_adjustments, 0);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_unknown_research_eligibility_preserves_two_main_remote_attempts() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-main-capacity-one",
            "tvly-reconciliation-main-capacity-two",
        ],
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
    settings.upstream_precise_reconciliation_enabled = false;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save compare-only settings");
    let first_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-main-capacity-one")
        .await
        .expect("create first reconciliation key");
    let second_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-main-capacity-two")
        .await
        .expect("create second reconciliation key");
    for key_id in [&first_key_id, &second_key_id] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind("two-main-attempts-token")
        .bind(key_id)
        .bind("2026-07-15/S1")
        .bind("project-two-main-attempts")
        .bind("token:two-main-attempts-token")
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed two-key reconciliation usage");
    }
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("current-period-research-token")
    .bind(&first_key_id)
    .bind("2026-07-15/S2")
    .bind("project-current-period-research")
    .bind("token:current-period-research-token")
    .bind(now - 900)
    .bind(now + 3_600)
    .bind(now - 900)
    .bind(now)
    .bind(now)
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
    .bind("current-period-research-request")
    .bind("current-period-research-token")
    .bind(&first_key_id)
    .bind("2026-07-15/S2")
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed current-period research");
    sqlx::query(
        r#"
        UPDATE upstream_reconciliation_work
        SET completed_generation = work_generation
        WHERE token_id = 'current-period-research-token' AND period_code = '2026-07-15/S2'
        "#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("keep current-period research out of main settlement work");
    assert!(
        !proxy
            .key_store
            .has_due_upstream_reconciliation_research()
            .await
            .expect("read Research eligibility"),
        "current-period Research must not reserve main settlement capacity"
    );

    let usage_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let usage_hits_for_route = Arc::clone(&usage_hits);
    let app = Router::new().route(
        "/usage",
        get(move || {
            let usage_hits = Arc::clone(&usage_hits_for_route);
            async move {
                usage_hits.fetch_add(1, Ordering::SeqCst);
                Json(serde_json::json!({ "key": { "usage": 0 } }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind reconciliation upstream");
    let addr = listener
        .local_addr()
        .expect("read reconciliation upstream address");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reconciliation upstream");
    });

    proxy.fail_next_reconciliation_research_read_for_test();
    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("run reconciliation with an unavailable Research eligibility probe"),
        1
    );
    assert_eq!(
        usage_hits.load(Ordering::SeqCst),
        2,
        "an unavailable Research eligibility probe must not defer main settlement"
    );
    let (_, attempted_candidate_count, _, _, _, _) = proxy
        .key_store
        .upstream_reconciliation_last_run_stats()
        .await
        .expect("read reconciliation run stats");
    assert_eq!(attempted_candidate_count, 1);
    let research_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_research WHERE terminal_at IS NULL",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count current-period research");
    assert_eq!(
        research_count, 1,
        "current-period Research must remain unpolled"
    );
    let actual_adjustments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM billing_reconciliation_adjustments")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count actual billing adjustments");
    assert_eq!(actual_adjustments, 0);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_preserves_global_min_across_backfill_pages() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-pages"],
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
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection for backfill fixture");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1,
            0,
        )
        .await
        .expect("mark existing usage projection pending");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("reset projection controller state");
    sqlx::query(
        r#"WITH RECURSIVE rows(n) AS (
             VALUES(1) UNION ALL SELECT n + 1 FROM rows WHERE n < 501
           )
           INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           )
           SELECT 'projection-token', printf('key-%03d', n), '2026-07-15/S1',
                  CASE WHEN n = 501 THEN 'project-a' ELSE 'project-z' END,
                  CASE WHEN n = 501 THEN 'account:a' ELSE 'account:z' END,
                  ?, ?, 1, ?, ?, ?,
                  CASE WHEN n = 501 THEN 'actual' ELSE 'shadow' END
           FROM rows"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert paged projection fixture");
    let projection_plan: Vec<String> = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT token_id, key_id, period_code FROM upstream_reconciliation_usage WHERE (token_id, key_id, period_code) > ('', '', '') ORDER BY token_id, key_id, period_code LIMIT 25",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("explain projection continuation cursor")
    .into_iter()
    .map(|row| row.try_get("detail").expect("read plan detail"))
    .collect();
    assert!(
        projection_plan.iter().any(|detail| detail
            .contains("USING COVERING INDEX sqlite_autoindex_upstream_reconciliation_usage_1")),
        "the projection cursor must seek through the stable usage primary key"
    );

    assert!(
        proxy
            .key_store
            .next_upstream_reconciliation_candidates(1)
            .await
            .expect("select candidates without advancing legacy projection")
            .candidates
            .is_empty(),
        "candidate selection must not write or scan the legacy projection before main settlement"
    );
    for _ in 0..24 {
        proxy
            .key_store
            .advance_upstream_reconciliation_work_projection()
            .await
            .expect("advance projection micro-slice");
    }
    let projected: (String, String, String) = sqlx::query_as(
        "SELECT project_id, billing_subject, settlement_mode FROM upstream_reconciliation_work WHERE token_id = 'projection-token' AND period_code = '2026-07-15/S1'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read merged projection");
    assert_eq!(
        projected,
        (
            "project-a".to_string(),
            "account:a".to_string(),
            "actual".to_string()
        )
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_projection_continues_after_a_settled_backfill_page() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-continuation"],
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
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection for backfill fixture");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1,
            0,
        )
        .await
        .expect("mark existing usage projection pending");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("reset projection controller state");
    sqlx::query(
        r#"WITH RECURSIVE rows(n) AS (
             VALUES(1) UNION ALL SELECT n + 1 FROM rows WHERE n < 501
           )
           INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           )
           SELECT printf('projection-page-%03d', n), printf('key-%03d', n),
                  '2026-07-15/S1', printf('project-%03d', n), printf('account:%03d', n),
                  ?, ?, 1, ?, ?, ?, 'shadow'
           FROM rows"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert paged projection fixture");

    proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect("project first page");
    sqlx::query("UPDATE upstream_reconciliation_work SET completed_generation = work_generation")
        .execute(&proxy.key_store.pool)
        .await
        .expect("settle first projection page");

    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_continuation_at()
            .await
            .expect("continue incomplete source projection"),
        Some(now + 1),
        "low-pressure source projection must remain durable work after its current page drains"
    );
    proxy
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("enqueue next projection page");
    let continuation: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), MAX(available_at) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read next-page representative");
    assert_eq!(continuation, (1, now + 1));

    proxy
        .key_store
        .advance_upstream_reconciliation_work_projection()
        .await
        .expect("project second page");
    let next_page = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .expect("select second projected page");
    assert_eq!(next_page.candidates.len(), 1);
    assert_eq!(next_page.candidates[0].token_id, "projection-page-026");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn projection_no_progress_gate_uses_durable_ladder_and_maximum() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 1, 30);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-gate"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let existing_gate = now + 90;
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0, next_retry_at = ?, last_defer_reason = NULL WHERE id = 'local'",
    )
    .bind(existing_gate)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed incomplete projection gate");

    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    let first = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            "bulk_busy",
            now + 5,
        )
        .await
        .expect("persist first projection defer");
    let first_available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(first.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read first continuation");
    assert_eq!(first_available_at, existing_gate);
    assert!(
        proxy
            .scheduled_job_mark_running(first.job_id)
            .await
            .expect("check early continuation")
            .is_none()
    );
    let first_state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read first projection gate");
    assert_eq!(first_state, (existing_gate, Some("bulk_busy".to_string())));

    clock.set_now_ts(existing_gate);
    let second_claim = proxy
        .scheduled_job_mark_running(first.job_id)
        .await
        .expect("claim first continuation")
        .expect("continuation is due");
    let second = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            second_claim.id,
            second_claim.claim_generation,
            "pool_pressure",
            existing_gate + 5,
        )
        .await
        .expect("persist second projection defer");
    let second_available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(second.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read second continuation");
    assert_eq!(second_available_at, existing_gate + 60);
    let second_state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read second projection gate");
    assert_eq!(
        second_state,
        (existing_gate + 60, Some("pool_pressure".to_string()))
    );

    clock.set_now_ts(existing_gate + 60);
    let third_claim = proxy
        .scheduled_job_mark_running(second.job_id)
        .await
        .expect("claim second continuation")
        .expect("second continuation is due");
    let third = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            third_claim.id,
            third_claim.claim_generation,
            "foreground_pressure",
            existing_gate + 65,
        )
        .await
        .expect("persist third projection defer");
    let third_available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(third.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read third continuation");
    assert_eq!(third_available_at, existing_gate + 180);
    let third_state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read third projection gate");
    assert_eq!(
        third_state,
        (existing_gate + 180, Some("foreground_pressure".to_string()))
    );

    clock.set_now_ts(existing_gate + 180);
    let fourth_claim = proxy
        .scheduled_job_mark_running(third.job_id)
        .await
        .expect("claim third continuation")
        .expect("third continuation is due");
    let fourth = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            fourth_claim.id,
            fourth_claim.claim_generation,
            "recent_contention",
            existing_gate + 125,
        )
        .await
        .expect("persist fourth projection defer");
    let fourth_available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(fourth.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read fourth continuation");
    assert_eq!(fourth_available_at, existing_gate + 480);
    let fourth_state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read fourth projection gate");
    assert_eq!(
        fourth_state,
        (existing_gate + 480, Some("recent_contention".to_string()))
    );

    assert!(matches!(
        proxy
            .finalize_deferred_upstream_reconciliation_claim(
                claim.id,
                claim.claim_generation,
                "local_pressure",
                existing_gate + 60,
            )
            .await,
        Err(ProxyError::StaleClaim { .. })
    ));
    let stale_state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read stale projection gate");
    assert_eq!(stale_state, fourth_state);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn projection_backoff_preserves_existing_durable_gate() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 2, 30);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-backoff-gate"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let existing_gate = now + 300;
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0, next_retry_at = ?, last_defer_reason = 'bulk_busy' WHERE id = 'local'",
    )
    .bind(existing_gate)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed projection backoff");

    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    let continuation = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            "projection_backoff",
            now + 5,
        )
        .await
        .expect("persist projection backoff");
    let continuation_available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(continuation.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read continuation schedule");
    assert_eq!(
        continuation_available_at, existing_gate,
        "projection short-circuit must not override its durable retry gate"
    );
    let local_pressure_streak: String = sqlx::query_scalar(
        "SELECT COALESCE((SELECT value FROM meta WHERE key = 'upstream_reconciliation_local_pressure_streak_v1'), '0')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read local pressure streak");
    assert_eq!(
        local_pressure_streak, "0",
        "projection short-circuit must not create a new local pressure streak"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn projection_progress_does_not_reapply_defer_gate() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 1, 45);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-progress-reset"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0, next_retry_at = 0, last_defer_reason = NULL WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("reset projection gate");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    let continuation = proxy
        .finalize_deferred_upstream_reconciliation_claim_with_progress(
            claim.id,
            claim.claim_generation,
            "local_pressure",
            now + 5,
            true,
        )
        .await
        .expect("finalize after accepted projection progress");
    let state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read projection state");
    assert_eq!(state, (0, None));
    assert_eq!(continuation.status, "queued");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn projection_cursor_conflict_uses_durable_gate_reason() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 2, 15);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-cursor-conflict"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0, next_retry_at = 0, last_defer_reason = NULL WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("reset projection gate");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    let continuation = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            "cursor_conflict",
            now + 5,
        )
        .await
        .expect("persist cursor-conflict defer");
    let state: (i64, Option<String>) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read cursor-conflict state");
    assert_eq!(state, (now + 30, Some("cursor_conflict".to_string())));
    assert_eq!(continuation.status, "queued");

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn projection_defer_skips_research_candidates_in_the_same_turn() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 8, 21, 2, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-projection-research-isolation"],
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
        .expect("disable live projection trigger");
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
           ) VALUES ('projection-isolation-source', 'projection-isolation-key', '2026-08-21/S0',
                     'projection-isolation-project', 'token:projection-isolation-source', ?, ?,
                     1, ?, ?, ?, 'shadow')"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed historical projection source");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_work (
             token_id, period_code, project_id, billing_subject, settlement_mode,
             period_start, period_end, scheduling_key_id, updated_at
           ) VALUES ('projection-isolation-main', '2026-08-21/S1', 'main-project',
                     'token:projection-isolation-main', 'shadow', ?, ?,
                     'missing-key', ?)"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed existing main candidate");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('projection-isolation-research', 'research-key', '2026-08-21/R0',
                     'research-project', 'token:projection-isolation-research', ?, ?,
                     1, ?, ?, ?, 'shadow')"#,
    )
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed research source");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_research (
             request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
           ) VALUES ('projection-isolation-request', 'projection-isolation-research',
                     'research-key', '2026-08-21/R0', ?, NULL, ?)"#,
    )
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed due research candidate");
    sqlx::query(
        r#"CREATE TRIGGER fail_projection_isolation_work_insert
           BEFORE INSERT ON upstream_reconciliation_work
           BEGIN
             SELECT RAISE(ABORT, 'database is locked');
           END"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("inject projection defer");
    proxy.fail_next_reconciliation_research_read_for_test();
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            "http://127.0.0.1:9",
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("projection defer does not fail the main turn");
    assert!(
        matches!(outcome, ClaimedReconciliationRunOutcome::Completed { .. }),
        "projection no-progress must skip the same-turn Research sweep: {outcome:?}"
    );
    assert!(
        proxy
            .key_store
            .next_upstream_reconciliation_research_candidates(80)
            .await
            .is_err(),
        "the injected Research read must remain unused when projection makes no progress"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

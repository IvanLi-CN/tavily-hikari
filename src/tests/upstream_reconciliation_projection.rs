use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;
use axum::{Json, Router, routing::get};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::net::TcpListener;

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

    // This direct helper has a short foreground-safe admission wait. Under a
    // loaded shard, retry only its documented transient admission result; the
    // test's subject is the bounded Research request, not pool arbitration.
    let settled = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match proxy
                .run_upstream_reconciliation_once(&format!("http://{addr}"))
                .await
            {
                Ok(settled) => break settled,
                Err(ProxyError::Other(message))
                    if message.starts_with(
                        "upstream reconciliation local preparation remained deferred",
                    ) =>
                {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(err) => panic!("run reconciliation with research-only work: {err}"),
            }
        }
    })
    .await
    .expect("admit research-only reconciliation within a bounded retry window");
    assert_eq!(settled, 0);
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

    proxy.fail_next_reconciliation_research_read_for_test();
    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("defer a transient research eligibility read"),
        0
    );
    let (streak, level, _) = proxy
        .key_store
        .upstream_reconciliation_local_backoff_state()
        .await
        .expect("read local pressure state after a preparation defer");
    assert_eq!(
        (streak, level),
        (0, 0),
        "an unclaimed preparation defer must not fabricate a durable local-pressure backoff"
    );

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

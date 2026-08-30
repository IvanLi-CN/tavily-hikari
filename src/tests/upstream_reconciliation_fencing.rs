use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_rejects_reclaimed_claim_before_research_writes() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-stale-research"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    for request_id in ["stale-research-terminal", "stale-research-poll"] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_research (
                request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
            ) VALUES (?, 'stale-research-token', 'stale-research-key', '2026-07-15/S1', ?, NULL, ?)
            "#,
        )
        .bind(request_id)
        .bind(now)
        .bind(now)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert pending research");
    }
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative job");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative job")
        .expect("representative job becomes running");
    clock.set_now_ts(now + 61);
    assert_eq!(
        proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("recover stale representative job"),
        1
    );

    assert!(matches!(
        proxy
            .key_store
            .mark_upstream_reconciliation_research_terminal_claimed(
                "stale-research-terminal",
                claim.id,
                claim.claim_generation,
            )
            .await,
        Err(ProxyError::StaleClaim { .. })
    ));
    assert!(matches!(
        proxy
            .key_store
            .arm_api_key_transient_backoff_claimed(
                ApiKeyTransientBackoffArm {
                    key_id: "stale-research-key",
                    scope: "period_reconciliation",
                    cooldown_until: now + 300,
                    retry_after_secs: 300,
                    reason_code: Some(RECONCILIATION_RETRY_REASON_UPSTREAM_429),
                    source_request_log_id: None,
                    now,
                },
                claim.id,
                claim.claim_generation,
            )
            .await,
        Err(ProxyError::StaleClaim { .. })
    ));
    assert!(matches!(
        proxy
            .key_store
            .record_upstream_reconciliation_research_poll_claimed(
                "stale-research-poll",
                now + 120,
                "pending",
                None,
                claim.id,
                claim.claim_generation,
            )
            .await,
        Err(ProxyError::StaleClaim { .. })
    ));
    assert!(matches!(
        proxy
            .key_store
            .mark_upstream_reconciliation_research_sweep_at_claimed(
                now + 61,
                claim.id,
                claim.claim_generation,
            )
            .await,
        Err(ProxyError::StaleClaim { .. })
    ));

    let research_rows: Vec<(String, Option<i64>, i64)> = sqlx::query_as(
        r#"
        SELECT request_id, terminal_at, poll_attempt_count
        FROM upstream_reconciliation_research
        ORDER BY request_id
        "#,
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read unchanged research rows");
    assert_eq!(
        research_rows,
        vec![
            ("stale-research-poll".to_string(), None, 0),
            ("stale-research-terminal".to_string(), None, 0),
        ]
    );
    assert_eq!(
        proxy
            .key_store
            .get_meta_i64("upstream_reconciliation_last_research_sweep_at_v1")
            .await
            .expect("read unchanged research sweep marker"),
        None
    );
    let transient_backoff_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_transient_backoffs WHERE key_id = 'stale-research-key' AND scope = 'period_reconciliation'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged key backoff");
    assert_eq!(transient_backoff_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_research_drain_stale_claim_does_not_commit_cursor_or_result() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 16, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-stale-drain"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    sqlx::query(
        "INSERT INTO upstream_reconciliation_research \
         (request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at) \
         VALUES ('stale-drain-request', 'stale-drain-token', 'stale-drain-key', \
                 '2026-07-16/S1', ?, NULL, ?)",
    )
    .bind(now - 100)
    .bind(now - 100)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed drain row");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation_research_drain", "auto", None, now)
        .await
        .expect("enqueue drain");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim drain")
        .expect("drain claim exists");
    clock.set_now_ts(now + 61);
    assert_eq!(proxy.recover_stale_scheduled_jobs().await.unwrap(), 1);

    let cursor = crate::store::UpstreamReconciliationResearchCursor {
        next_poll_at: -1,
        key_id: String::new(),
        request_id: String::new(),
    };
    let accepted = proxy
        .key_store
        .commit_upstream_reconciliation_research_drain(
            crate::store::UpstreamReconciliationResearchDrainCommit {
                request_id: "stale-drain-request",
                expected_cursor: &cursor,
                accepted_cursor: &cursor,
                wrapped: false,
                poll: crate::store::UpstreamReconciliationResearchDrainPoll::Terminal,
                key_backoff: None,
                job_id: claim.id,
                claim_generation: claim.claim_generation,
            },
        )
        .await;
    assert!(matches!(accepted, Err(ProxyError::StaleClaim { .. })));
    let row: (Option<i64>, i64) = sqlx::query_as(
        "SELECT terminal_at, poll_attempt_count FROM upstream_reconciliation_research \
         WHERE request_id = 'stale-drain-request'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged stale drain row");
    assert_eq!(row, (None, 0));
    let persisted_cursor: (i64, String, String) = sqlx::query_as(
        "SELECT cursor_next_poll_at, cursor_key_id, cursor_request_id \
         FROM upstream_reconciliation_research_scan_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unchanged cursor");
    assert_eq!(persisted_cursor, (-1, String::new(), String::new()));
    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_ignores_429_text_in_non_429_responses() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-real-status"],
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
    let token = proxy
        .create_access_token(Some("reconciliation-real-status"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-real-status")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-real-status', ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
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
    .expect("insert main reconciliation usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S2', 'project-real-status-research', ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
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
    .expect("insert research reconciliation usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES ('research-real-status', ?, ?, '2026-07-15/S2', ?, NULL, ?)
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending research");

    let app = Router::new()
        .route(
            "/usage",
            get(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "429 Too Many Requests" })),
                )
            }),
        )
        .route(
            "/research/research-real-status",
            get(|| async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "429 Too Many Requests" })),
                )
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve misleading 429 upstream");
    });

    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("run reconciliation with non-429 failures"),
        0
    );
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("read global backoff state"),
        (0, 0, 0)
    );
    let transient_backoff_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_transient_backoffs WHERE key_id = ? AND scope = 'period_reconciliation'",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read key backoff state");
    assert_eq!(transient_backoff_count, 0);
    let main_status: String = sqlx::query_scalar(
        "SELECT status FROM upstream_reconciliation_settlements WHERE token_id = ? AND period_code = '2026-07-15/S1'",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read semantic failure settlement");
    assert_eq!(main_status, "waiting");
    let research_poll: (String, Option<String>) = sqlx::query_as(
        "SELECT last_poll_outcome, last_poll_error_kind FROM upstream_reconciliation_research WHERE request_id = 'research-real-status'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read semantic failure research poll");
    assert_eq!(
        research_poll,
        ("retry".to_string(), Some("other".to_string()))
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn claimed_backoff_recovery_leaves_representative_for_scheduler_continuation() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-continuation-fence"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim reconciliation representative")
        .expect("representative job becomes running");

    proxy
        .key_store
        .update_upstream_reconciliation_local_backoff_claimed(
            false,
            now,
            queued.job_id,
            claim.claim_generation,
        )
        .await
        .expect("clear claimed backoff without finishing job");
    assert_eq!(
        proxy
            .scheduled_job_by_id(queued.job_id)
            .await
            .expect("read representative job")
            .expect("representative job exists")
            .status,
        "running"
    );

    let continuation = proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            queued.job_id,
            claim.claim_generation,
            "upstream_reconciliation",
            None,
            1,
            Some("settled=0 continuation_at=next"),
            now + 60,
        )
        .await
        .expect("scheduler finishes claimed representative and queues continuation");
    assert!(continuation.created);
    assert_eq!(
        proxy
            .scheduled_job_by_id(queued.job_id)
            .await
            .expect("read completed representative job")
            .expect("completed representative job exists")
            .status,
        "success"
    );
    assert_eq!(
        proxy
            .scheduled_job_by_id(continuation.job_id)
            .await
            .expect("read continuation job")
            .expect("continuation job exists")
            .status,
        "queued"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn startup_resume_schedules_pending_research_with_default_poll_time() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-restart"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time.clone(),
    )
    .await
    .expect("create first proxy");
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
    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (token_id, key_id, period_code, project_id, \
         billing_subject, period_start, period_end, request_count, first_used_at, last_used_at, \
         updated_at, settlement_mode) VALUES ('default-poll-token', 'default-poll-key', \
         '2026-07-15/S1', 'default-poll-project', 'token:default-poll-token', ?, ?, 1, ?, ?, ?, \
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
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES ('default-poll-research', 'default-poll-token', 'default-poll-key',
                  '2026-07-15/S1', ?, NULL, ?)
        "#,
    )
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("persist historical pending research with default poll time");
    drop(proxy);

    let restarted = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-research-restart"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("restart proxy");
    restarted
        .ensure_upstream_reconciliation_research_drain_job()
        .await
        .expect("resume historical pending research");
    let representative: (i64, i64) = sqlx::query_as(
        r#"
        SELECT COUNT(*), MIN(available_at)
        FROM scheduled_jobs
        WHERE job_type = 'upstream_reconciliation_research_drain'
          AND status IN ('queued', 'running')
        "#,
    )
    .fetch_one(&restarted.key_store.pool)
    .await
    .expect("read resumed representative job");
    assert_eq!(representative, (1, now));

    let _ = std::fs::remove_file(db_path);
}

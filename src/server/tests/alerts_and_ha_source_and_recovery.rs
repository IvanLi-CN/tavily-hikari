use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;
use super::*;
use futures_util::FutureExt;

#[tokio::test]
async fn reconciliation_low_pressure_recovery_runs_shadow_fixture_despite_prior_local_backoff() {
    let db_path = temp_db_path("reconciliation-low-pressure-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-low-pressure-recovery".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = tavily_hikari::UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("enable compare reconciliation");
    let token = proxy
        .create_access_token(Some("reconciliation-low-pressure-recovery"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-low-pressure-recovery")
        .await
        .expect("create upstream key");
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES (?, ?, 'recovery/S1', 'recovery-project', ?,
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
    .execute(&pool)
    .await
    .expect("insert shadow fixture");

    let upstream = Router::new().route(
        "/usage",
        get(|| async { Json(serde_json::json!({ "key": { "usage": 0 } })) }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind usage upstream");
    let address = listener.local_addr().expect("read usage upstream address");
    tokio::spawn(async move {
        axum::serve(listener, upstream.into_make_service())
            .await
            .expect("serve usage upstream");
    });

    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: format!("http://{address}"),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });
    sqlx::query(
        r#"INSERT INTO meta (key, value) VALUES
             ('upstream_reconciliation_local_pressure_streak_v1', '3'),
             ('upstream_reconciliation_local_backoff_level_v1', '1'),
             ('upstream_reconciliation_local_backoff_until_v1', ?)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
    )
    .bind((now + 30).to_string())
    .execute(&pool)
    .await
    .expect("seed sustained local pressure backoff");
    let queued = state
        .proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = state
        .proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim reconciliation representative")
        .expect("representative becomes running");

    assert_eq!(
        state.proxy.foreground_activity_rps(),
        0,
        "recovery worker starts after foreground traffic has drained"
    );
    assert!(
        run_manual_claimed_job(
            state.clone(),
            "upstream_reconciliation".to_string(),
            None,
            ClaimedScheduledJob {
                job_id: claim.id,
                claim_generation: claim.claim_generation,
                _job_execution_gate: None,
            },
            None,
            false,
        )
        .await
    );
    let completed: i64 = sqlx::query_scalar(
        "SELECT completed_generation >= work_generation FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = 'recovery/S1'",
    )
    .bind(&token.id)
    .fetch_one(&pool)
    .await
    .expect("read shadow fixture completion");
    assert_eq!(
        completed, 1,
        "a low-pressure recovery worker must not turn an eligible shadow terminal into an empty backoff completion"
    );

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_foreground_defer_releases_scheduler_reconciliation_turn() {
    let db_path = temp_db_path("reconciliation-foreground-dispatch-release");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-foreground-dispatch-release".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-foreground-dispatch-release-password",
    )
    .await;
    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }
    assert!(
        state.proxy.foreground_activity_rps() > tavily_hikari::HA_OUTBOX_GC_LOW_PRESSURE_RPS,
        "fixture establishes foreground pressure before reconciliation starts"
    );

    let queued = state
        .proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = state
        .proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim reconciliation representative")
        .expect("representative becomes running");
    let controller = remote_attempt_admission_for_state(state.as_ref());
    let turn = controller
        .reserve_aged_reconciliation_turn()
        .expect("scheduler reserves the aged reconciliation turn");

    assert!(
        run_manual_claimed_job(
            state.clone(),
            "upstream_reconciliation".to_string(),
            None,
            ClaimedScheduledJob {
                job_id: claim.id,
                claim_generation: claim.claim_generation,
                _job_execution_gate: None,
            },
            Some(turn),
            false,
        )
        .await,
        "typed foreground defer persists a representative"
    );
    assert!(
        !controller.reconciliation_turn_required(),
        "a defer before HTTP releases the fairness turn"
    );
    drop(
        controller
            .reserve_aged_reconciliation_turn()
            .expect("a deferred reconciliation turn permits later automatic work"),
    );

    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' ORDER BY id",
    )
    .fetch_all(&connect_sqlite_test_pool(&db_str).await)
    .await
    .expect("read deferred reconciliation lifecycle");
    assert_eq!(statuses, vec!["success", "queued"]);

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn remote_attempt_controller_fairly_serves_aged_research_after_main() {
    let db_path = temp_db_path("reconciliation-aged-research-turn");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-aged-research-turn".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-aged-research-turn-password",
    )
    .await;
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = state.proxy.backend_time().now_ts();
    let main = state
        .proxy
        .scheduled_job_enqueue_at("upstream_reconciliation", "auto", None, 1, now - 121)
        .await
        .expect("enqueue aged main reconciliation");
    let research = state
        .proxy
        .scheduled_job_enqueue_at(
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
            "auto",
            None,
            1,
            now - 121,
        )
        .await
        .expect("enqueue aged Research drain");
    for job_id in [main.job_id, research.job_id] {
        sqlx::query("UPDATE scheduled_jobs SET queued_at = ?, available_at = ? WHERE id = ?")
            .bind(now - 121)
            .bind(now - 121)
            .bind(job_id)
            .execute(&pool)
            .await
            .expect("age automatic representative");
    }

    let (main_job, main_turn) = dequeue_next_scheduled_job(state.as_ref())
        .await
        .expect("select the oldest aged automatic job")
        .expect("aged main is runnable");
    assert_eq!(main_job.id, main.job_id, "main wins an exact-age tie");
    assert_eq!(
        main_turn.expect("main reserves the turn").kind(),
        ReconciliationTurnKind::Main
    );

    let (research_job, research_turn) = dequeue_next_scheduled_job(state.as_ref())
        .await
        .expect("select the remaining aged automatic job")
        .expect("aged Research is runnable");
    assert_eq!(research_job.id, research.job_id);
    assert_eq!(
        research_turn.expect("Research reserves the next turn").kind(),
        ReconciliationTurnKind::ResearchDrain
    );

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn remote_attempt_controller_uses_research_wait_anchor_for_aged_fairness() {
    let db_path = temp_db_path("reconciliation-aged-research-anchor-order");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-aged-research-anchor-order".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-aged-research-anchor-order-password",
    )
    .await;
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = state.proxy.backend_time().now_ts();
    let main = state
        .proxy
        .scheduled_job_enqueue_at("upstream_reconciliation", "auto", None, 1, now - 121)
        .await
        .expect("enqueue aged main reconciliation");
    let research = state
        .proxy
        .scheduled_job_enqueue_at(
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
            "auto",
            None,
            1,
            now,
        )
        .await
        .expect("enqueue aged Research drain");
    sqlx::query("UPDATE scheduled_jobs SET queued_at = ?, available_at = ? WHERE id = ?")
        .bind(now - 121)
        .bind(now - 121)
        .bind(main.job_id)
        .execute(&pool)
        .await
        .expect("age main reconciliation");
    sqlx::query("UPDATE scheduled_jobs SET queued_at = ?, available_at = ? WHERE id = ?")
        .bind(now - 180)
        .bind(now)
        .bind(research.job_id)
        .execute(&pool)
        .await
        .expect("preserve an older Research fairness anchor after a defer");

    let (selected, turn) = dequeue_next_scheduled_job(state.as_ref())
        .await
        .expect("select the oldest aged automatic job")
        .expect("aged Research is runnable");
    assert_eq!(
        selected.id, research.job_id,
        "an older Research wait anchor wins even when its latest defer made available_at newer"
    );
    assert_eq!(
        turn.expect("Research reserves the turn").kind(),
        ReconciliationTurnKind::ResearchDrain
    );

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_retained_turn_runs_before_ordinary_automatic_remote_work() {
    let db_path = temp_db_path("reconciliation-retained-research-turn");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-retained-research-turn".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-retained-research-turn-password",
    )
    .await;
    let now = state.proxy.backend_time().now_ts();
    let research = state
        .proxy
        .scheduled_job_enqueue_at(
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
            "auto",
            None,
            1,
            now - 121,
        )
        .await
        .expect("enqueue aged Research drain");
    state
        .proxy
        .scheduled_job_enqueue_at("forward_proxy_geo_refresh", "auto", None, 1, now - 121)
        .await
        .expect("enqueue ordinary automatic remote work");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("UPDATE scheduled_jobs SET queued_at = ?, available_at = ? WHERE id = ?")
        .bind(now - 121)
        .bind(now - 121)
        .bind(research.job_id)
        .execute(&pool)
        .await
        .expect("age Research representative");

    let controller = remote_attempt_admission_for_state(state.as_ref());
    let retained = controller
        .reserve_aged_research_drain_turn()
        .expect("Research reserves its aged request turn");
    retained.retain_for_continuation();
    drop(retained);

    let (selected, turn) = dequeue_next_scheduled_job(state.as_ref())
        .await
        .expect("select the retained aged turn")
        .expect("retained Research is dispatchable");
    assert_eq!(selected.id, research.job_id);
    assert_eq!(
        turn.expect("retained Research reclaims its turn").kind(),
        ReconciliationTurnKind::ResearchDrain
    );

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn aged_research_bypasses_foreground_heuristic_once() {
    let db_path = temp_db_path("reconciliation-aged-research-foreground");
    let db_str = db_path.to_string_lossy().to_string();
    let hits = Arc::new(AtomicUsize::new(0));
    let route_hits = Arc::clone(&hits);
    let upstream = Router::new().route(
        "/research/aged-research-request",
        get(move || {
            let route_hits = Arc::clone(&route_hits);
            async move {
                route_hits.fetch_add(1, Ordering::SeqCst);
                Json(serde_json::json!({ "status": "completed" }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Research upstream");
    let upstream_addr = listener.local_addr().expect("read Research upstream address");
    tokio::spawn(async move {
        axum::serve(listener, upstream.into_make_service())
            .await
            .expect("serve Research upstream");
    });
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-aged-research-foreground".to_string()],
        &format!("http://{upstream_addr}"),
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: format!("http://{upstream_addr}"),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = state.proxy.backend_time().now_ts();
    let key_id = state
        .proxy
        .add_or_undelete_key("tvly-aged-research-foreground")
        .await
        .expect("create Research key");
    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (token_id, key_id, period_code, project_id, \
         billing_subject, period_start, period_end, request_count, first_used_at, last_used_at, \
         updated_at, settlement_mode) VALUES ('aged-research-token', ?, 'aged/R1', \
         'aged-research-project', 'token:aged-research-token', ?, ?, 1, ?, ?, ?, 'shadow')",
    )
    .bind(&key_id)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&pool)
    .await
    .expect("seed closed-period Research usage");
    sqlx::query(
        "INSERT INTO upstream_reconciliation_research (request_id, token_id, key_id, period_code, \
         created_at, terminal_at, updated_at) VALUES ('aged-research-request', \
         'aged-research-token', ?, 'aged/R1', ?, NULL, ?)",
    )
    .bind(&key_id)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&pool)
    .await
    .expect("seed due Research poll");

    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }
    assert!(
        state.proxy.foreground_activity_rps() > tavily_hikari::HA_OUTBOX_GC_LOW_PRESSURE_RPS,
        "fixture establishes continuous foreground-rate pressure"
    );
    let queued = state
        .proxy
        .scheduled_job_enqueue(RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE, "auto", None, 1)
        .await
        .expect("enqueue Research drain");
    let claim = state
        .proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim Research drain")
        .expect("Research drain becomes running");
    let controller = remote_attempt_admission_for_state(state.as_ref());
    let turn = controller
        .reserve_aged_research_drain_turn()
        .expect("aged Research receives a turn");

    assert!(
        run_manual_claimed_job(
            state.clone(),
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE.to_string(),
            None,
            ClaimedScheduledJob {
                job_id: claim.id,
                claim_generation: claim.claim_generation,
                _job_execution_gate: None,
            },
            Some(turn),
            false,
        )
        .await,
        "an aged Research turn runs one bounded poll under foreground pressure"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let terminal: Option<i64> = sqlx::query_scalar(
        "SELECT terminal_at FROM upstream_reconciliation_research WHERE request_id = 'aged-research-request'",
    )
    .fetch_one(&pool)
    .await
    .expect("read accepted Research result");
    assert!(terminal.is_some(), "accepted poll commits the terminal outcome");

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_foreground_defer_keeps_its_aged_turn_anchor() {
    let db_path = temp_db_path("research-aged-turn-anchor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-research-aged-turn-anchor".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let now = proxy.backend_time().now_ts();
    let queued = proxy
        .scheduled_job_enqueue(RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE, "auto", None, 1)
        .await
        .expect("enqueue Research drain");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("UPDATE scheduled_jobs SET queued_at = ? WHERE id = ?")
        .bind(now.saturating_sub(RECONCILIATION_REMOTE_TURN_WAIT_SECS + 1))
        .bind(queued.job_id)
        .execute(&pool)
        .await
        .expect("seed a continuously waiting Research representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim Research drain")
        .expect("Research drain becomes running");

    proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            claim.id,
            claim.claim_generation,
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
            None,
            1,
            Some("deferred=foreground_pressure"),
            now,
        )
        .await
        .expect("persist foreground defer");

    let aged = proxy
        .fetch_aged_queued_scheduled_job_by_type(
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE,
            RECONCILIATION_REMOTE_TURN_WAIT_SECS,
        )
        .await
        .expect("query aged Research representative")
        .expect("foreground defer preserves its fairness anchor");
    assert!(
        aged.queued_at <= now.saturating_sub(RECONCILIATION_REMOTE_TURN_WAIT_SECS),
        "the continuation must retain the wait that makes its single RPS exception live"
    );

    drop(pool);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn non_aged_research_defers_for_foreground_pressure() {
    let db_path = temp_db_path("reconciliation-research-foreground-defer");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-research-foreground-defer".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-research-foreground-defer-password",
    )
    .await;
    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }
    let now = state.proxy.backend_time().now_ts();
    let queued = state
        .proxy
        .scheduled_job_enqueue(RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE, "auto", None, now)
        .await
        .expect("enqueue Research drain");
    let claim = state
        .proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim Research drain")
        .expect("Research drain becomes running");

    assert!(
        run_manual_claimed_job(
            state.clone(),
            RECONCILIATION_RESEARCH_DRAIN_JOB_TYPE.to_string(),
            None,
            ClaimedScheduledJob {
                job_id: claim.id,
                claim_generation: claim.claim_generation,
                _job_execution_gate: None,
            },
            None,
            false,
        )
        .await,
        "a non-aged Research representative receives a durable defer"
    );
    let finished_message: String = sqlx::query_scalar(
        "SELECT COALESCE(message, '') FROM scheduled_jobs WHERE id = ?",
    )
    .bind(claim.id)
    .fetch_one(&connect_sqlite_test_pool(&db_str).await)
    .await
    .expect("read deferred Research job");
    let continuation_at: i64 = sqlx::query_scalar(
        "SELECT available_at FROM scheduled_jobs \
         WHERE job_type = 'upstream_reconciliation_research_drain' AND status = 'queued'",
    )
    .fetch_one(&connect_sqlite_test_pool(&db_str).await)
    .await
    .expect("read Research continuation");
    assert_eq!(finished_message, "deferred=foreground_pressure");
    assert_eq!(continuation_at, now + 30);

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn aged_reconciliation_turn_allows_other_remote_local_preparation() {
    let db_path = temp_db_path("reconciliation-turn-does-not-serialize-preparation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-turn-does-not-serialize-preparation".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-turn-does-not-serialize-preparation-password",
    )
    .await;
    let now = state.proxy.backend_time().now_ts();
    let pool = connect_sqlite_test_pool(&db_str).await;
    let reconciliation = state
        .proxy
        .scheduled_job_enqueue_at("upstream_reconciliation", "auto", None, 1, now - 121)
        .await
        .expect("enqueue aged reconciliation representative");
    sqlx::query("UPDATE scheduled_jobs SET queued_at = ?, available_at = ? WHERE id = ?")
        .bind(now - 121)
        .bind(now - 121)
        .bind(reconciliation.job_id)
        .execute(&pool)
        .await
        .expect("age reconciliation representative");
    let geo_refresh = state
        .proxy
        .scheduled_job_enqueue("forward_proxy_geo_refresh", "auto", None, 1)
        .await
        .expect("enqueue a second automatic remote job");

    let (reconciliation_job, turn) = dequeue_next_scheduled_job(state.as_ref())
        .await
        .expect("claim the aged reconciliation representative")
        .expect("aged reconciliation is scheduled");
    assert_eq!(reconciliation_job.id, reconciliation.job_id);
    let turn = turn.expect("aged reconciliation owns the fairness turn");

    let (other_remote_job, other_turn) = dequeue_next_scheduled_job(state.as_ref())
        .await
        .expect("schedule another remote job while reconciliation prepares locally")
        .expect("the second remote job is not serialized behind local projection");
    assert_eq!(other_remote_job.id, geo_refresh.job_id);
    assert!(
        other_turn.is_none(),
        "only the aged reconciliation representative owns the fairness turn"
    );
    let controller = remote_attempt_admission_for_state(state.as_ref());
    assert!(
        controller.acquire_attempt().now_or_never().is_none(),
        "an automatic remote job may prepare locally but waits for the aged reconciliation lease"
    );
    let request_lease = turn
        .acquire_attempt()
        .await
        .expect("the matching reconciliation turn acquires the next outbound HTTP lease");
    drop(request_lease);

    drop(turn);
    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_scheduler_preserves_unclassified_terminal_errors() {
    let db_path = temp_db_path("reconciliation-unclassified-run-failure");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-unclassified-run-failure".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-unclassified-run-failure-password",
    )
    .await;
    let pool = connect_sqlite_test_pool(&db_str).await;
    let queued = state
        .proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = state
        .proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim reconciliation representative")
        .expect("representative becomes running");

    sqlx::query("DROP TABLE upstream_reconciliation_control_state")
        .execute(&pool)
        .await
        .expect("inject an unclassified reconciliation read failure");

    let succeeded = run_manual_claimed_job(
        state.clone(),
        "upstream_reconciliation".to_string(),
        None,
        ClaimedScheduledJob {
            job_id: claim.id,
            claim_generation: claim.claim_generation,
            _job_execution_gate: None,
        },
        None,
        false,
    )
    .await;
    assert!(!succeeded, "unclassified run failures must remain observable");
    let statuses: Vec<(String,)> = sqlx::query_as(
        "SELECT status FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("read deferred reconciliation jobs");
    assert_eq!(statuses.first().map(|row| row.0.as_str()), Some("error"));
    assert!(statuses.iter().all(|row| row.0 != "queued"));

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_scheduler_persists_typed_defer_without_a_terminal_error() {
    let db_path = temp_db_path("reconciliation-scheduler-typed-defer");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-scheduler-typed-defer".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create reconciliation proxy");
    let (_addr, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "reconciliation-scheduler-typed-defer-password",
    )
    .await;
    let pool = connect_sqlite_test_pool(&db_str).await;
    let queued = state
        .proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = state
        .proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim reconciliation representative")
        .expect("representative becomes running");
    let retry_at = state.proxy.backend_time().now_ts().saturating_add(30);

    let succeeded = persist_claimed_reconciliation_run(
        state.clone(),
        claim.id,
        claim.claim_generation,
        Ok(ClaimedReconciliationRunOutcome::Deferred {
            reason: "local_pressure",
            retry_at,
        }),
    )
    .await;
    assert!(succeeded, "typed deadline defer must finalize the claim");

    let statuses: Vec<(String,)> = sqlx::query_as(
        "SELECT status FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .expect("read deferred reconciliation jobs");
    assert_eq!(statuses.first().map(|row| row.0.as_str()), Some("success"));
    assert_eq!(statuses.get(1).map(|row| row.0.as_str()), Some("queued"));
    assert!(statuses.iter().all(|row| row.0 != "error"));

    let observation: (Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT last_retryable_outcome, next_retry_at FROM upstream_reconciliation_run_observation WHERE id = 'local'",
    )
    .fetch_one(&pool)
    .await
    .expect("read deferred observation");
    assert_eq!(observation.0.as_deref(), Some("local_pressure"));
    assert_eq!(observation.1, Some(retry_at));
    let continuation_available_at: i64 = sqlx::query_scalar(
        "SELECT available_at FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .fetch_one(&pool)
    .await
    .expect("read delayed representative");
    assert_eq!(continuation_available_at, retry_at);

    drop(state);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_gc_real_worker_wakes_an_eligible_channel_before_a_legacy_defer() {
    let db_path = temp_db_path("ha-gc-worker-fair-wake");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();
    let mut tx = pool.begin().await.expect("begin outbox seed transaction");
    for index in 0..250 {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES ('state', 'users', ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(format!("legacy-scan-{index}"))
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("seed retained control event");
    }
    sqlx::query(
        r#"
        INSERT INTO ha_outbox
            (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'scheduled_jobs', 'legacy-after-page', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(now)
    .execute(&mut *tx)
    .await
    .expect("seed deferred legacy control event");
    sqlx::query(
        r#"
        INSERT INTO ha_billing_outbox
            (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('state', 'billing_ledger', 'billing-ready', 'upsert', '{}', ?, NULL)
        "#,
    )
    .bind(now - 15 * 24 * 60 * 60)
    .execute(&mut *tx)
    .await
    .expect("seed eligible billing event");
    tx.commit().await.expect("commit outbox seed transaction");
    sqlx::query(
        "UPDATE ha_outbox_gc_state SET next_channel = 'control', pending_channel_mask = 7 WHERE id = 'local'",
    )
    .execute(&pool)
    .await
    .expect("select control first");

    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });

    let initial = state
        .proxy
        .scheduled_job_enqueue("ha_outbox_gc", "test", None, 1)
        .await
        .expect("enqueue control worker job");
    // Claim admission has its own bounded concurrency regression coverage. This
    // fixture isolates controller wake persistence and must not depend on an
    // unrelated 100ms test-pool control budget.
    let initial_started_at = Utc::now().timestamp();
    assert_eq!(
        sqlx::query(
            "UPDATE scheduled_jobs SET status = 'running', started_at = ?, claim_generation = claim_generation + 1 WHERE id = ? AND status = 'queued'",
        )
        .bind(initial_started_at)
        .bind(initial.job_id)
        .execute(&pool)
        .await
        .expect("claim control worker job in the isolated fixture")
        .rows_affected(),
        1
    );
    let initial_claim_generation: i64 = sqlx::query_scalar(
        "SELECT claim_generation FROM scheduled_jobs WHERE id = ?",
    )
    .bind(initial.job_id)
    .fetch_one(&pool)
    .await
    .expect("read control worker claim generation");
    assert_eq!(state.proxy.foreground_activity_rps(), 0, "test starts without foreground pressure");
    assert!(
        run_ha_outbox_gc_claimed_job(
            state.clone(),
            ClaimedScheduledJob {
                job_id: initial.job_id,
                claim_generation: initial_claim_generation,
                _job_execution_gate: None,
            },
        )
        .await
    );

    let continuation: (i64, i64, i64) = sqlx::query_as(
        "SELECT id, queued_at, available_at FROM scheduled_jobs WHERE job_type = 'ha_outbox_gc' AND status = 'queued'",
    )
    .fetch_one(&pool)
    .await
    .expect("read controller continuation");
    assert_eq!(
        continuation.2.saturating_sub(continuation.1),
        1,
        "the scheduler must persist the controller's one-second fair wake, not the control channel's 300-second legacy defer"
    );
    let (control_delay, control_cursor): (i64, i64) = sqlx::query_as(
        "SELECT next_retry_at - last_attempt_at, legacy_cursor_seq FROM ha_outbox_gc_channel_state WHERE channel = 'control'",
    )
    .fetch_one(&pool)
    .await
    .expect("read durable control defer");
    assert_eq!(control_delay, 300);
    assert_eq!(control_cursor, 250);

    // Contract split: the library's manual-clock regression proves the
    // controller calculates this one-second fair wake. This binary test proves
    // the scheduler persists that exact typed wake and the next real worker
    // advances billing when the durable job becomes due.
    sqlx::query("UPDATE scheduled_jobs SET available_at = ? WHERE id = ?")
        .bind(Utc::now().timestamp())
        .bind(continuation.0)
        .execute(&pool)
        .await
        .expect("advance continuation to its fair wake");
    let billing_started_at = Utc::now().timestamp();
    assert_eq!(
        sqlx::query(
            "UPDATE scheduled_jobs SET status = 'running', started_at = ?, claim_generation = claim_generation + 1 WHERE id = ? AND status = 'queued' AND available_at <= ?",
        )
        .bind(billing_started_at)
        .bind(continuation.0)
        .bind(billing_started_at)
        .execute(&pool)
        .await
        .expect("claim billing worker job in the isolated fixture")
        .rows_affected(),
        1
    );
    let billing_claim_generation: i64 = sqlx::query_scalar(
        "SELECT claim_generation FROM scheduled_jobs WHERE id = ?",
    )
    .bind(continuation.0)
    .fetch_one(&pool)
    .await
    .expect("read billing worker claim generation");
    assert!(
        run_ha_outbox_gc_claimed_job(
            state.clone(),
            ClaimedScheduledJob {
                job_id: continuation.0,
                claim_generation: billing_claim_generation,
                _job_execution_gate: None,
            },
        )
        .await
    );
    let billing_remaining: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM ha_billing_outbox WHERE resource_id = 'billing-ready' LIMIT 1)",
    )
    .fetch_one(&pool)
    .await
    .expect("read billing progress");
    assert!(!billing_remaining, "billing must advance on the fair wake");


    drop(state);
    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_productive_continuation_lock_defers_to_stale_reaper() {
    let db_path = temp_db_path("ha-gc-writer-lock-continuation-retry");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let expired_created_at = Utc::now().timestamp() - 15 * 24 * 60 * 60;
    for index in 0..1_250 {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES ('state', 'users', ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(format!("locked-continuation-{index}"))
        .bind(expired_created_at)
        .execute(&pool)
        .await
        .expect("seed expired HA GC debt");
    }
    let report = proxy
        .gc_ha_outbox_online()
        .await
        .expect("run a productive GC slice before continuation handoff");
    assert_eq!(report.deleted_rows, 1_000);
    let continuation_delay_secs = report
        .continuation_delay_secs
        .expect("the productive GC slice must require a continuation");
    assert!(report.has_more);
    let initial = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "test", None, 1)
        .await
        .expect("enqueue HA GC job");
    let initial_claim = proxy
        .scheduled_job_mark_running(initial.job_id)
        .await
        .expect("claim HA GC job")
        .expect("HA GC job is due");
    let lock_options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(0));
    let mut lock_conn = sqlx::SqliteConnection::connect_with(&lock_options)
        .await
        .expect("connect writer lock holder");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock_conn)
        .await
        .expect("hold SQLite writer lock");

    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });
    assert!(
        tokio::time::timeout(
            Duration::from_millis(500),
            finish_ha_gc_with_continuation(
                &state,
                initial_claim.id,
                initial_claim.claim_generation,
                "controller_wake_delay_secs=1 productive_slice".to_string(),
                continuation_delay_secs,
            )
        )
        .await
        .expect("GC worker must yield when continuation persistence is busy")
    );
    let running_claim: (String, i64) = sqlx::query_as(
        "SELECT status, claim_generation FROM scheduled_jobs WHERE id = ?",
    )
    .bind(initial_claim.id)
    .fetch_one(&pool)
    .await
    .expect("read unresolved HA GC claim");
    assert_eq!(running_claim, ("running".to_string(), initial_claim.claim_generation));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ha_outbox")
            .fetch_one(&pool)
            .await
            .expect("read remaining HA GC debt"),
        250,
        "the productive slice must leave a continuation-worthy tail"
    );

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release SQLite writer lock");
    lock_conn.close().await.expect("close writer lock holder");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM scheduled_jobs WHERE id = ?")
            .bind(initial_claim.id)
            .fetch_one(&pool)
            .await
            .expect("continuation persistence must not retry in the background"),
        "running"
    );

    let recovery_now = Utc::now().timestamp();
    sqlx::query("UPDATE scheduled_jobs SET started_at = ? WHERE id = ?")
        .bind(recovery_now - 120)
        .bind(initial_claim.id)
        .execute(&pool)
        .await
        .expect("age unresolved HA GC claim for stale reaper");
    assert_eq!(
        state
            .proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("recover stale HA GC claim"),
        1,
        "the stale reaper is the sole recovery path after persistence conflict"
    );
    assert_eq!(
        state
            .proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("a recovered HA GC claim cannot be recovered twice"),
        0
    );
    let recovered: (String, i64, i64, Option<String>) = sqlx::query_as(
        "SELECT status, claim_generation, available_at, message FROM scheduled_jobs WHERE id = ?",
    )
    .bind(initial_claim.id)
    .fetch_one(&pool)
    .await
    .expect("read stale-reaper continuation");
    assert_eq!(recovered.0, "queued");
    assert_eq!(recovered.1, initial_claim.claim_generation + 1);
    assert!(
        (recovery_now + 30..=recovery_now + 31).contains(&recovered.2),
        "stale recovery must preserve the 30-second continuation delay"
    );
    assert_eq!(recovered.3.as_deref(), Some("deferred=stale_recovery"));

    drop(state);
    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn request_logs_gc_handoff_preserves_error_and_defers_to_stale_reaper() {
    let db_path = temp_db_path("request-logs-gc-continuation-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let queued = proxy
        .scheduled_job_enqueue("request_logs_gc", "test", None, 1)
        .await
        .expect("enqueue request-log GC");
    let claimed = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim request-log GC")
        .expect("request-log GC is due");
    let lock_options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(0));
    let mut lock_conn = sqlx::SqliteConnection::connect_with(&lock_options)
        .await
        .expect("connect writer lock holder");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock_conn)
        .await
        .expect("hold SQLite writer lock");
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });
    assert!(
        !tokio::time::timeout(
            Duration::from_millis(500),
            finish_request_logs_gc_with_continuation(
                &state,
                claimed.id,
                claimed.claim_generation,
                "success",
                "incomplete=true".to_string(),
            )
        )
        .await
        .expect("request-log GC handoff must yield under writer contention")
    );
    let running: String = sqlx::query_scalar("SELECT status FROM scheduled_jobs WHERE id = ?")
        .bind(claimed.id)
        .fetch_one(&pool)
        .await
        .expect("read unresolved request-log claim");
    assert_eq!(running, "running");

    sqlx::query("ROLLBACK")
        .execute(&mut lock_conn)
        .await
        .expect("release SQLite writer lock");
    lock_conn.close().await.expect("close writer lock holder");
    let recovery_now = Utc::now().timestamp();
    sqlx::query("UPDATE scheduled_jobs SET started_at = ? WHERE id = ?")
        .bind(recovery_now - 120)
        .bind(claimed.id)
        .execute(&pool)
        .await
        .expect("age unresolved request-log continuation");
    assert_eq!(
        state
            .proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("recover request-log GC continuation"),
        1
    );
    let recovered: (String, i64, String) = sqlx::query_as(
        "SELECT status, available_at, message FROM scheduled_jobs WHERE id = ?",
    )
    .bind(claimed.id)
    .fetch_one(&pool)
    .await
    .expect("read recovered request-log continuation");
    assert_eq!(recovered.0, "queued");
    assert!(recovered.1 >= recovery_now + 299);
    assert_eq!(recovered.2, "deferred=stale_request_logs_gc_recovery");

    sqlx::query("UPDATE scheduled_jobs SET status = 'success' WHERE id = ?")
        .bind(claimed.id)
        .execute(&pool)
        .await
        .expect("retire the recovered continuation before the error handoff case");

    let failed = state
        .proxy
        .scheduled_job_enqueue("request_logs_gc", "test", None, 1)
        .await
        .expect("enqueue failing request-log GC");
    let failed_claim = state
        .proxy
        .scheduled_job_mark_running(failed.job_id)
        .await
        .expect("claim failing request-log GC")
        .expect("failing request-log GC is due");
    assert!(
        finish_request_logs_gc_with_continuation(
            &state,
            failed_claim.id,
            failed_claim.claim_generation,
            "error",
            "error=permanent_failure".to_string(),
        )
        .await,
        "error must finish the failed job and retain its continuation"
    );
    let failed_status: String = sqlx::query_scalar("SELECT status FROM scheduled_jobs WHERE id = ?")
        .bind(failed_claim.id)
        .fetch_one(&pool)
        .await
        .expect("read failed request-log GC status");
    assert_eq!(failed_status, "error", "permanent GC errors remain observable");

    drop(state);
    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_channel_outbox_stats_reports_indexed_span_age_and_ack_lag() {
    let db_path = temp_db_path("ha-outbox-stats");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-outbox-stats".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let now = Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('upsert', 'meta', 'request_rate_limit_v1', 'upsert', '{}', ?, 'checksum-a')
        "#,
    )
    .bind(now - 4 * 24 * 60 * 60)
    .execute(&pool)
    .await
    .expect("insert first outbox row");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('upsert', 'meta', 'global_ip_limit_v1', 'upsert', '{}', ?, 'checksum-b')
        "#,
    )
    .bind(now - 30)
    .execute(&pool)
    .await
    .expect("insert second outbox row");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('upsert', 'meta', 'request_rate_limit_v1', 'upsert', '{}', ?, 'checksum-c')
        "#,
    )
    .bind(now - 20)
    .execute(&pool)
    .await
    .expect("insert third outbox row");
    proxy
        .ack_ha_peer_watermark(tavily_hikari::HaSyncChannel::Control, "standby-a", 1)
        .await
        .expect("ack watermark");

    let stats = proxy
        .ha_channel_outbox_stats(tavily_hikari::HaSyncChannel::Control, Some("standby-a"))
        .await
        .expect("read outbox stats");
    assert_eq!(stats.sequence_span_estimate, 3);
    assert_eq!(stats.high_watermark, 3);
    assert!(stats.oldest_age_secs >= 100);
    assert_eq!(stats.ack_lag, Some(2));
    let span_plan: Vec<String> = sqlx::query(
        "EXPLAIN QUERY PLAN SELECT MIN(seq) FROM ha_outbox WHERE resource IN ('meta')",
    )
    .fetch_all(&pool)
    .await
    .expect("explain indexed span query")
    .into_iter()
    .map(|row| row.try_get("detail").expect("read query plan detail"))
    .collect();
    assert!(
        span_plan
            .iter()
            .any(|detail| detail.contains("idx_ha_outbox_resource")),
        "indexed sequence-span diagnostics must not scan the whole outbox: {span_plan:?}"
    );
    let no_peer_stats = proxy
        .ha_channel_outbox_stats(tavily_hikari::HaSyncChannel::Control, None)
        .await
        .expect("read peer-less outbox stats");
    assert_eq!(no_peer_stats.ack_lag, None);
    let health = proxy
        .ha_peer_channel_health(tavily_hikari::HaSyncChannel::Control, "standby-a")
        .await
        .expect("read channel health");
    assert_eq!(health.acked_seq, Some(1));
    assert_eq!(health.high_watermark, 3);
    assert_eq!(health.ack_lag, Some(2));
    assert_eq!(health.cursor_state, "catching_up");
    assert_eq!(health.retention_secs, 72 * 60 * 60);
    assert!(!health.expired_backlog);
    assert_eq!(health.gc_state, "unknown");

    sqlx::query("DELETE FROM ha_outbox WHERE seq = 2")
        .execute(&pool)
        .await
        .expect("delete middle event for gap probe");
    let stats_after_gap = proxy
        .ha_channel_outbox_stats(tavily_hikari::HaSyncChannel::Control, Some("standby-a"))
        .await
        .expect("read span estimate after gap");
    assert_eq!(
        stats_after_gap.sequence_span_estimate, 3,
        "HA diagnostics must expose the indexed sequence span, not run an exact row count"
    );
    proxy
        .ack_ha_peer_watermark(tavily_hikari::HaSyncChannel::Control, "standby-gap", 1)
        .await
        .expect("ack gap probe watermark");
    let gap_health = proxy
        .ha_peer_channel_health(tavily_hikari::HaSyncChannel::Control, "standby-gap")
        .await
        .expect("read gap channel health");
    assert_eq!(gap_health.cursor_state, "expired_backlog");

    sqlx::query(
        r#"
        INSERT INTO ha_outbox (seq, kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES (2, 'legacy', 'removed_resource', 'legacy-bridge', 'delete', '{}', ?, 'checksum-legacy-bridge')
        "#,
    )
    .bind(now - 15)
    .execute(&pool)
    .await
    .expect("insert legacy row that bridges the sequence gap");
    proxy
        .ack_ha_peer_watermark(
            tavily_hikari::HaSyncChannel::Control,
            "standby-legacy-bridge",
            1,
        )
        .await
        .expect("ack legacy bridge watermark");
    let legacy_bridge_health = proxy
        .ha_peer_channel_health(
            tavily_hikari::HaSyncChannel::Control,
            "standby-legacy-bridge",
        )
        .await
        .expect("read legacy bridge channel health");
    assert_eq!(legacy_bridge_health.cursor_state, "catching_up");

    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('upsert', 'meta', 'request_rate_limit_v1', 'upsert', '{}', ?, 'checksum-d')
        "#,
    )
    .bind(now - 10)
    .execute(&pool)
    .await
    .expect("insert fourth outbox row");
    proxy
        .ack_ha_peer_watermark(tavily_hikari::HaSyncChannel::Control, "standby-valid", 4)
        .await
        .expect("ack latest valid watermark");
    sqlx::query(
        r#"
        INSERT INTO ha_outbox (kind, resource, resource_id, op, payload_json, created_at, checksum)
        VALUES ('legacy', 'removed_resource', 'legacy-1', 'delete', '{}', ?, 'checksum-legacy')
        "#,
    )
    .bind(now - 5)
    .execute(&pool)
    .await
    .expect("insert invalid legacy outbox row");
    proxy
        .gc_ha_outbox_with_options(tavily_hikari::HaOutboxGcOptions {
            batch_size: 10,
            max_batches: 8,
            max_runtime_secs: 20,
            inter_batch_sleep_ms: 0,
        })
        .await
        .expect("gc invalid legacy outbox row");
    let valid_health = proxy
        .ha_peer_channel_health(tavily_hikari::HaSyncChannel::Control, "standby-valid")
        .await
        .expect("read valid channel health after legacy gc");
    assert_eq!(valid_health.high_watermark, 4);
    assert_eq!(valid_health.cursor_state, "healthy");

    proxy
        .ack_ha_peer_watermark(tavily_hikari::HaSyncChannel::Control, "standby-zero", 0)
        .await
        .expect("ack zero watermark");
    let zero_cursor_health = proxy
        .ha_peer_channel_health(tavily_hikari::HaSyncChannel::Control, "standby-zero")
        .await
        .expect("read zero cursor health");
    assert_eq!(zero_cursor_health.cursor_state, "baseline_required");

    sqlx::query("DELETE FROM ha_outbox")
        .execute(&pool)
        .await
        .expect("delete retained control events");
    let expired_health = proxy
        .ha_peer_channel_health(tavily_hikari::HaSyncChannel::Control, "standby-a")
        .await
        .expect("read expired channel health");
    assert_eq!(expired_health.high_watermark, 4);
    assert_eq!(expired_health.cursor_state, "expired_backlog");
    assert!(expired_health.expired_backlog);

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_channel_outbox_stats_span_uses_minimum_valid_sequence() {
    let db_path = temp_db_path("ha-outbox-span-minimum-sequence");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-outbox-span-minimum-sequence".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = Utc::now().timestamp();

    for (seq, created_at) in [(1, now - 10), (2, now - 5), (3, now - 100)] {
        sqlx::query(
            r#"
            INSERT INTO ha_outbox
                (seq, kind, resource, resource_id, op, payload_json, created_at, checksum)
            VALUES (?, 'state', 'meta', ?, 'upsert', '{}', ?, NULL)
            "#,
        )
        .bind(seq)
        .bind(format!("out-of-order-created-at-{seq}"))
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert valid control event");
    }

    let stats = proxy
        .ha_channel_outbox_stats(tavily_hikari::HaSyncChannel::Control, None)
        .await
        .expect("read indexed span estimate");
    assert_eq!(stats.high_watermark, 3);
    assert_eq!(
        stats.sequence_span_estimate, 3,
        "the sequence span must use the minimum valid sequence, not the oldest timestamp row"
    );
    assert!(stats.oldest_age_secs >= 100);

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_source_endpoint_persists_origin_group_settings() {
    let db_path = temp_db_path("ha-source-origin-group");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-source-origin-group".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-source-group".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .put(format!("http://{addr}/api/admin/ha/source"))
        .json(&serde_json::json!({
            "sourceKind": "origin_group",
            "originGroupId": "eo-group-api-test",
            "applyToEdgeone": false
        }))
        .send()
        .await
        .expect("source settings response");
    let status = response.status();
    let body = response.text().await.expect("source settings body text");
    assert!(
        status.is_success(),
        "source settings request should succeed, got {status}: {body}"
    );
    let response: Value = serde_json::from_str(&body).expect("source settings body");

    assert_eq!(response["haSourceOverride"]["sourceKind"], "origin_group");
    assert_eq!(response["haSourceOverride"]["originGroupId"], "eo-group-api-test");
    assert_eq!(response["haSourceEffective"]["target"], "eo-group-api-test");
    assert_eq!(response["edgeoneExpectedOrigin"], "eo-group-api-test");
    assert_eq!(response["edgeoneExpectedSourceKind"], "origin_group");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_source_endpoint_accepts_lowercase_direct_origin_scheme() {
    let db_path = temp_db_path("ha-source-direct-scheme");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-source-direct-scheme".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-source-direct".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;

    let response = Client::new()
        .put(format!("http://{addr}/api/admin/ha/source"))
        .json(&serde_json::json!({
            "sourceKind": "direct",
            "directOriginScheme": "https",
            "directOriginHost": "gz.ivanli.cc",
            "directOriginPort": 1443,
            "applyToEdgeone": false
        }))
        .send()
        .await
        .expect("source settings response");
    let status = response.status();
    let body = response.text().await.expect("source settings body text");
    assert!(
        status.is_success(),
        "direct source settings request should succeed, got {status}: {body}"
    );
    let response: Value = serde_json::from_str(&body).expect("source settings body");

    assert_eq!(response["haSourceOverride"]["sourceKind"], "direct");
    assert_eq!(response["haSourceOverride"]["directOriginScheme"], "https");
    assert_eq!(response["haSourceOverride"]["directOriginHost"], "gz.ivanli.cc");
    assert_eq!(response["haSourceOverride"]["directOriginPort"], 1443);
    assert_eq!(response["haSourceEffective"]["directOriginScheme"], "https");
    assert_eq!(response["edgeoneExpectedSourceKind"], "direct");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ha_recovery_import_is_idempotent_and_keeps_importer_active() {
    let db_path = temp_db_path("ha-recovery-idempotent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-recovery-idempotent".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        node_id: "node-new".to_string(),
        database_path: Some(db_str.clone()),
        ..tavily_hikari::HaConfig::default()
    });
    let addr = spawn_ha_admin_server(proxy, ha, true).await;
    let client = Client::new();
    let payload = serde_json::json!({
        "batchId": "old-master-batch-1",
        "sourceNodeId": "node-old",
        "message": "usage/log/event recovery batch imported",
        "requestLogs": [{
            "authTokenId": "old-token",
            "method": "POST",
            "path": "/api/tavily/search",
            "statusCode": 200,
            "tavilyStatusCode": 200,
            "resultStatus": "success",
            "requestKindKey": "tavily_search",
            "requestKindLabel": "Tavily Search",
            "requestKindDetail": "POST /api/tavily/search",
            "businessCredits": 1,
            "requestBody": "{\"query\":\"old-master\"}",
            "responseBody": "{\"answer\":\"ok\"}",
            "forwardedHeaders": "[]",
            "droppedHeaders": "[]",
            "visibility": "visible",
            "createdAt": Utc::now().timestamp() - 60
        }],
        "authTokenLogs": [{
            "tokenId": "old-token",
            "method": "POST",
            "path": "/api/tavily/search",
            "httpStatus": 200,
            "mcpStatus": 200,
            "requestKindKey": "tavily_search",
            "requestKindLabel": "Tavily Search",
            "requestKindDetail": "POST /api/tavily/search",
            "resultStatus": "success",
            "countsBusinessQuota": 1,
            "businessCredits": 1,
            "billingState": "charged",
            "createdAt": Utc::now().timestamp() - 60
        }]
    });

    let rejected = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&payload)
        .send()
        .await
        .expect("rejected recovery import");
    assert_eq!(rejected.status(), reqwest::StatusCode::BAD_REQUEST);
    let rejected_body = rejected.text().await.expect("rejected recovery body");
    assert!(
        rejected_body.contains("request_logs") && rejected_body.contains("auth_token_logs"),
        "legacy log recovery payload should be explicitly rejected: {rejected_body}"
    );

    let ledger_payload = serde_json::json!({
        "batchId": "old-master-batch-1",
        "sourceNodeId": "node-old",
        "message": "ledger recovery batch imported"
    });

    let first: Value = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&ledger_payload)
        .send()
        .await
        .expect("first ledger recovery import")
        .json()
        .await
        .expect("first ledger recovery response");
    assert_eq!(first["imported"], true);
    assert_eq!(first["eventCount"], 0);
    assert_eq!(first["status"]["role"], "full_master");

    let second: Value = client
        .post(format!("http://{addr}/api/admin/ha/recovery/import"))
        .json(&ledger_payload)
        .send()
        .await
        .expect("second ledger recovery import")
        .json()
        .await
        .expect("second ledger recovery response");
    assert_eq!(second["imported"], false);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let row: (String, i64) = sqlx::query_as(
        "SELECT status, event_count FROM ha_recovery_batches WHERE id = 'old-master-batch-1'",
    )
    .fetch_one(&pool)
    .await
    .expect("fetch recovery batch");
    assert_eq!(row.0, "imported");
    assert_eq!(row.1, 0);
    let request_log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM request_logs WHERE auth_token_id = 'old-token'")
            .fetch_one(&pool)
            .await
            .expect("fetch rejected request logs");
    assert_eq!(request_log_count, 0);
    let token_log_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM auth_token_logs WHERE token_id = 'old-token'")
            .fetch_one(&pool)
            .await
            .expect("fetch rejected auth token logs");
    assert_eq!(token_log_count, 0);
    pool.close().await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn compute_signatures_tracks_recent_alert_summary_changes() {
    let db_path = temp_db_path("summary-signatures-recent-alerts");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-signature-recent-alerts".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-signature-alert-user".to_string(),
            username: Some("sig_alert".to_string()),
            name: Some("Sig Alert".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert oauth user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("signature-alert-bound"))
        .await
        .expect("ensure token binding");

    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });

    let (before_sig, _) = compute_signatures(&state)
        .await
        .expect("compute signatures before alerts");
    let before_sig = before_sig.expect("summary signature before alerts");
    assert_eq!(before_sig.freshness.recent_alerts_total_events, 0);
    assert_eq!(before_sig.freshness.recent_alerts_grouped_count, 0);

    let now = Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                counts_business_quota,
                created_at
            ) VALUES (?, 'POST', '/mcp', NULL, 429, -1, 'mcp_search', 'MCP Search', 'POST /mcp', 'quota_exhausted', 'hourly any-request limit exceeded', 'none', 'none', 'none', 0, ?)
            "#,
        )
        .bind(&token.id)
        .bind(now)
        .execute(&pool)
    .await
    .expect("insert recent alert auth token log");

    for _ in 0..6 {
        state
            .proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("advance alert projection after alert write");
    }

    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_recent_summaries SET computed_at = ? WHERE window_hours = 24",
    )
    .bind(Utc::now().timestamp() - 61)
    .execute(&pool)
    .await
    .expect("expire materialized alert summary refresh window");
    state
        .proxy
        .advance_dashboard_alert_projection_scheduler_step()
        .await
        .expect("refresh alert projection after its rate-limit window");

    expire_dashboard_overview_freshness_probe(&state).await;
    let _ = compute_signatures(&state)
        .await
        .expect("serve last-good signatures while alerts refresh");
    let _ = wait_for_dashboard_overview_refresh(&state).await;
    let (after_sig, _) = compute_signatures(&state)
        .await
        .expect("compute signatures after alerts");
    let after_sig = after_sig.expect("summary signature after alerts");
    assert_eq!(after_sig.freshness.recent_alerts_total_events, 1);
    assert_eq!(after_sig.freshness.recent_alerts_grouped_count, 1);
    assert_eq!(
        after_sig.freshness.recent_alerts_counts,
        vec![
            (
                tavily_hikari::ALERT_TYPE_UPSTREAM_RATE_LIMITED_429.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_UPSTREAM_KEY_BLOCKED.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_USER_REQUEST_RATE_LIMITED.to_string(),
                1
            ),
            (
                tavily_hikari::ALERT_TYPE_USER_QUOTA_EXHAUSTED.to_string(),
                0
            ),
            (
                tavily_hikari::ALERT_TYPE_API_KEY_EXHAUSTED.to_string(),
                0
            ),
            (tavily_hikari::ALERT_TYPE_JOB_FAILED.to_string(), 0),
        ]
    );
    assert_ne!(before_sig, after_sig);

    let _ = std::fs::remove_file(db_path);
}

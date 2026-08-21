use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;
use super::*;

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

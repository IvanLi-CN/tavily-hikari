use super::*;
use chrono::{Local, LocalResult, TimeZone};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

pub(super) fn local_ts(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
    match Local.with_ymd_and_hms(year, month, day, hour, minute, 0) {
        LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => value.timestamp(),
        LocalResult::None => panic!("local time is unavailable"),
    }
}

pub(super) fn reconciliation_test_db_path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tavily-hikari-reconciliation-{}-{}",
        std::process::id(),
        nanoid!(8)
    ));
    std::fs::create_dir_all(&dir).expect("create reconciliation temp dir");
    dir.join("test.db")
}

#[tokio::test]
async fn reconciliation_waits_for_a_complete_eligible_period() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, clock) = BackendTime::manual_from_ts(1_752_500_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-gate"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = false;
    settings.api_rebalance_percent = 0;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save ineligible settings");
    let (eligible, epoch, _) = proxy
        .key_store
        .refresh_upstream_reconciliation_epoch()
        .await
        .expect("refresh ineligible epoch");
    assert!(!eligible);
    assert_eq!(epoch, 0);

    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save eligible settings");
    let (eligible, epoch, _) = proxy
        .key_store
        .refresh_upstream_reconciliation_epoch()
        .await
        .expect("arm next epoch");
    assert!(!eligible);
    assert!(epoch > clock.now_ts());

    clock.set_now_ts(epoch + 1);
    let (eligible, persisted_epoch, _) = proxy
        .key_store
        .refresh_upstream_reconciliation_epoch()
        .await
        .expect("activate complete epoch");
    assert!(eligible);
    assert_eq!(persisted_epoch, epoch);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_status_reports_observation_and_unknown_estimates() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_752_500_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-observation"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    let value = serde_json::to_value(
        proxy
            .upstream_privacy_status()
            .await
            .expect("read privacy status"),
    )
    .expect("serialize privacy status");
    assert!(
        value.get("reconciliationObservation").is_some(),
        "admin status must identify when queue data was observed"
    );
    assert!(
        value.get("reconciliationLocalBackoff").is_some(),
        "admin status must expose local backoff independently from remote 429 state"
    );
    for field in [
        "partialKeyObservations",
        "multiKeyPending",
        "remoteAttemptBudgetDefers",
        "resumedRuns",
        "terminalRuns",
    ] {
        assert!(
            value["reconciliationRunObservation"].get(field).is_some(),
            "admin status must expose bounded reconciliation progress field {field}"
        );
    }

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn signed_reconciliation_adjustment_is_idempotent_and_restores_quota() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_752_500_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-adjustment"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let token = proxy
        .create_access_token(Some("reconciliation-adjustment"))
        .await
        .expect("create token");
    proxy
        .charge_token_quota(&token.id, 10)
        .await
        .expect("charge local estimate");
    let before = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("quota before adjustment");
    let now = proxy.backend_time().now_ts();
    let candidate = UpstreamReconciliationCandidate {
        token_id: token.id.clone(),
        period_code: "2026-07-14/S2".to_string(),
        project_id: "anonymous-project".to_string(),
        billing_subject: format!("token:{}", token.id),
        settlement_mode: "actual".to_string(),
        period_start: now - 3600,
        period_end: now + 60,
        pending_research: 0,
        degraded: false,
    };
    assert!(
        proxy
            .key_store
            .settle_upstream_reconciliation(&candidate, 7, 10, None)
            .await
            .expect("first settlement")
    );
    assert!(
        !proxy
            .key_store
            .settle_upstream_reconciliation(&candidate, 7, 10, None)
            .await
            .expect("duplicate settlement")
    );
    let after = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("quota after adjustment");
    assert_eq!(before.daily_used - after.daily_used, 3);
    assert_eq!(before.monthly_used - after.monthly_used, 3);
    let adjustments = proxy
        .key_store
        .recent_reconciliation_adjustments(10)
        .await
        .expect("read adjustments");
    assert_eq!(adjustments.len(), 1);
    assert_eq!(adjustments[0].delta_credits, -3);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn shadow_usage_records_even_when_active_upstream_mcp_sessions_block_precise_cutover() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-shadow-compare"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let token = proxy
        .create_access_token(Some("reconciliation-shadow-compare"))
        .await
        .expect("create token");
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    settings.upstream_precise_reconciliation_enabled = true;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save shadow compare settings");

    sqlx::query(
        r#"
        INSERT INTO mcp_sessions (
            proxy_session_id,
            upstream_session_id,
            upstream_key_id,
            auth_token_id,
            user_id,
            protocol_version,
            last_event_id,
            gateway_mode,
            experiment_variant,
            ab_bucket,
            routing_subject_hash,
            fallback_reason,
            rate_limited_until,
            last_rate_limited_at,
            last_rate_limit_reason,
            created_at,
            updated_at,
            expires_at,
            revoked_at,
            revoke_reason
        ) VALUES (?, ?, NULL, ?, NULL, '2025-03-26', NULL, ?, 'control', NULL, NULL, NULL, NULL, NULL, NULL, ?, ?, ?, NULL, NULL)
        "#,
    )
    .bind("sess-shadow-blocker")
    .bind("upstream-shadow-blocker")
    .bind(&token.id)
    .bind(MCP_GATEWAY_MODE_UPSTREAM)
    .bind(now - 300)
    .bind(now - 60)
    .bind(now + 3_600)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert active upstream session");

    let (eligible, epoch, active_sessions) = proxy
        .key_store
        .refresh_upstream_reconciliation_epoch()
        .await
        .expect("refresh reconciliation epoch");
    assert!(!eligible);
    assert_eq!(epoch, 0);
    assert_eq!(active_sessions, 1);

    let period = proxy
        .key_store
        .record_upstream_reconciliation_usage(
            &token.id,
            "key-shadow-compare",
            &format!("token:{}", token.id),
            None,
        )
        .await
        .expect("record shadow usage")
        .expect("shadow period");
    let row = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT period_code, settlement_mode, project_id
        FROM upstream_reconciliation_usage
        WHERE token_id = ? AND key_id = ?
        LIMIT 1
        "#,
    )
    .bind(&token.id)
    .bind("key-shadow-compare")
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch shadow usage row");
    assert_eq!(row.0, period.code);
    assert_eq!(row.1, "shadow");
    assert!(!row.2.is_empty(), "project_id should still be derived");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn shadow_reconciliation_keeps_zero_delta_usage_and_updates_runtime_markers() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-shadow-zero-delta"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let candidate = UpstreamReconciliationCandidate {
        token_id: "tok-shadow-zero".to_string(),
        period_code: "2026-07-15/S2".to_string(),
        project_id: "anonymous-project".to_string(),
        billing_subject: "account:user-shadow-zero".to_string(),
        settlement_mode: "shadow".to_string(),
        period_start: now - 3_600,
        period_end: now,
        pending_research: 0,
        degraded: false,
    };
    assert!(
        proxy
            .key_store
            .settle_upstream_reconciliation_shadow(&candidate, 7, 7, None)
            .await
            .expect("shadow zero-delta settlement")
    );

    let usage = proxy
        .shadow_daily_reconciled_usage_for_accounts(&["user-shadow-zero".to_string()])
        .await
        .expect("read zero-delta shadow usage");
    assert_eq!(usage.get("user-shadow-zero"), Some(&0));

    let (_, last_shadow_adjustment_at, _, _, _) = proxy
        .key_store
        .upstream_reconciliation_runtime_markers()
        .await
        .expect("read runtime markers");
    assert_eq!(last_shadow_adjustment_at, Some(now));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_upstream_reconciliation_once_updates_runtime_markers() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-runtime-markers"],
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

    let token = proxy
        .create_access_token(Some("reconciliation-runtime-markers"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-runtime-markers")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .bind("2026-07-15/S1")
    .bind("project-shadow-runtime")
    .bind(format!("token:{}", token.id))
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .bind("shadow")
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert due reconciliation usage");
    for candidate_index in 1..20 {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start,
                period_end, request_count, first_used_at, last_used_at, updated_at,
                settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(format!("2026-07-{candidate_index:02}/S2"))
        .bind(format!("project-shadow-runtime-{candidate_index}"))
        .bind(format!("token:{}", token.id))
        .bind(now - 100_000 - i64::from(candidate_index) * 10)
        .bind(now - 10_000 - i64::from(candidate_index) * 10)
        .bind(now - 20_000 - i64::from(candidate_index) * 10)
        .bind(now - 10_000 - i64::from(candidate_index) * 10)
        .bind(now - 10_000 - i64::from(candidate_index) * 10)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert additional main reconciliation candidate");
    }
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?)
        "#,
    )
    .bind("research-token")
    .bind(&key_id)
    .bind("2026-07-15/R1")
    .bind("project-research-runtime")
    .bind("token:research-token")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .bind("shadow")
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert research usage row");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?)
        "#,
    )
    .bind("research-runtime-marker")
    .bind("research-token")
    .bind(&key_id)
    .bind("2026-07-15/R1")
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending research");
    for research_index in 1..80 {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_research (
                request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, NULL, ?)
            "#,
        )
        .bind(format!("research-runtime-marker-{research_index}"))
        .bind("research-token")
        .bind(&key_id)
        .bind("2026-07-15/R1")
        .bind(now - 800 + i64::from(research_index))
        .bind(now - 800 + i64::from(research_index))
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert additional pending research");
    }
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("disable live projection for historical source fixture");
    sqlx::query(
        "UPDATE upstream_reconciliation_projection_state SET completed = 0 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark historical projection incomplete");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('historical-projection-token', ?, '2026-07-15/H1',
                     'historical-projection-project',
                     'token:historical-projection-token', ?, ?, 1, ?, ?, ?, 'shadow')"#,
    )
    .bind(&key_id)
    .bind(now - 8_000)
    .bind(now - 7_000)
    .bind(now - 8_000)
    .bind(now - 7_000)
    .bind(now - 7_000)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert historical projection source");

    let usage_started = Arc::new(AtomicUsize::new(usize::MAX));
    let usage_started_for_route = Arc::clone(&usage_started);
    let usage_attempts = Arc::new(AtomicUsize::new(0));
    let usage_attempts_for_route = Arc::clone(&usage_attempts);
    let research_started = Arc::new(AtomicUsize::new(usize::MAX));
    let research_started_for_route = Arc::clone(&research_started);
    let run_started = std::time::Instant::now();
    let app = Router::new()
        .route(
            "/usage",
            get(move || {
                let usage_started = Arc::clone(&usage_started_for_route);
                let usage_attempts = Arc::clone(&usage_attempts_for_route);
                async move {
                    let started_ms =
                        run_started.elapsed().as_millis().min(usize::MAX as u128) as usize;
                    let _ = usage_started.compare_exchange(
                        usize::MAX,
                        started_ms,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    );
                    usage_attempts.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({
                        "key": { "usage": 5 }
                    }))
                }
            }),
        )
        .route(
            "/research/research-runtime-marker",
            get(move || {
                let research_started = Arc::clone(&research_started_for_route);
                async move {
                    research_started.store(
                        run_started.elapsed().as_millis().min(usize::MAX as u128) as usize,
                        Ordering::SeqCst,
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    Json(serde_json::json!({ "status": "completed" }))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reconciliation usage upstream");
    });

    let _run_result = proxy
        .run_upstream_reconciliation_once(&format!("http://{addr}"))
        .await
        .expect("run reconciliation once");
    assert!(
        usage_attempts.load(Ordering::SeqCst) > 0,
        "the main reconciliation lane must issue at least one remote request"
    );
    assert!(
        usage_started.load(Ordering::SeqCst) < 2_000,
        "the first main settlement attempt must start before research consumes the budget"
    );
    assert!(
        research_started.load(Ordering::SeqCst) > usage_started.load(Ordering::SeqCst),
        "research must not start before the first main settlement request"
    );
    let (
        last_run_at,
        last_shadow_adjustment_at,
        _,
        last_research_sweep_at,
        last_research_terminal_at,
    ) = proxy
        .key_store
        .upstream_reconciliation_runtime_markers()
        .await
        .expect("read runtime markers");
    assert_eq!(last_run_at, Some(now));
    assert_eq!(last_shadow_adjustment_at, Some(now));
    assert_eq!(last_research_sweep_at, Some(now));
    assert_eq!(last_research_terminal_at, Some(now));
    let work_outcome: String = sqlx::query_scalar(
        "SELECT last_outcome FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = '2026-07-15/S1'",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read reconciliation work outcome");
    assert_eq!(work_outcome, "observed");
    let actual_adjustments: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM billing_reconciliation_adjustments")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count actual reconciliation adjustments");
    assert_eq!(
        actual_adjustments, 0,
        "compare observations must not mutate billing truth"
    );
    let observation = proxy
        .key_store
        .upstream_reconciliation_run_observation()
        .await
        .expect("read reconciliation engine observation");
    assert_eq!(observation.mode, "compare");
    assert_eq!(observation.settled, 0);
    assert!(observation.observed <= 2);
    let terminal_at: Option<i64> = sqlx::query_scalar(
        "SELECT terminal_at FROM upstream_reconciliation_research WHERE request_id = 'research-runtime-marker'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read terminal research");
    assert_eq!(terminal_at, Some(now));
    let projection_state: (i64, i64) = sqlx::query_as(
        "SELECT scanned_rows, transaction_p95_ms FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read projection progress after primary reconciliation");
    assert!(
        projection_state.0 > 0,
        "main work must not starve projection"
    );
    assert!(
        projection_state.1 > 0 && projection_state.1 < 100,
        "projection transaction p95 must be observed below 100ms"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_no_adjustment_completes_only_the_current_usage_generation() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-no-adjustment"],
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
        .create_access_token(Some("reconciliation-no-adjustment"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-no-adjustment")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-no-adjustment', ?, ?, ?, 1, ?, ?, ?, 'shadow')
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
    .expect("insert due reconciliation usage");

    let usage_hits = Arc::new(AtomicUsize::new(0));
    let app_hits = Arc::clone(&usage_hits);
    let app = Router::new().route(
        "/usage",
        get(move || {
            let app_hits = Arc::clone(&app_hits);
            async move {
                app_hits.fetch_add(1, Ordering::SeqCst);
                Json(serde_json::json!({ "key": { "usage": 0 } }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve no-adjustment upstream");
    });

    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("settle no-adjustment generation"),
        1
    );
    let first_generation: (i64, i64, String) = sqlx::query_as(
        "SELECT work_generation, completed_generation, last_outcome FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read completed no-adjustment generation");
    assert_eq!(first_generation, (1, 1, "no_adjustment".to_string()));
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_continuation_at()
            .await
            .expect("read continuation after completion"),
        None
    );
    assert_eq!(
        proxy
            .key_store
            .get_meta_i64("upstream_reconciliation_work_cursor_v1")
            .await
            .expect("read legacy projection cursor after settlement"),
        None,
        "a projected candidate must be settled without running the legacy source projection"
    );
    let queued_continuations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count queued continuations");
    assert_eq!(
        queued_continuations, 0,
        "a completed no-adjustment generation must not create a minute retry loop"
    );
    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("skip completed no-adjustment generation"),
        0
    );
    assert_eq!(usage_hits.load(Ordering::SeqCst), 1);

    sqlx::query(
        r#"
        UPDATE upstream_reconciliation_usage
        SET request_count = request_count + 1, updated_at = updated_at + 1
        WHERE token_id = ? AND key_id = ? AND period_code = '2026-07-15/S1'
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("record new usage in the same settlement window");
    assert_eq!(
        proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("settle new no-adjustment generation"),
        1
    );
    let second_generation: (i64, i64, String) = sqlx::query_as(
        "SELECT work_generation, completed_generation, last_outcome FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read completed replacement generation");
    assert_eq!(second_generation, (2, 2, "no_adjustment".to_string()));
    assert_eq!(usage_hits.load(Ordering::SeqCst), 2);
    let adjustment_delta: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(delta_credits), 0) FROM billing_reconciliation_shadow_adjustments WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("sum durable shadow adjustment impact");
    assert_eq!(
        adjustment_delta, 0,
        "zero deltas must not alter billing truth even when shadow audit state is retained"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_rejects_usage_generation_changed_during_remote_fetch() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-generation-fence"],
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
        .create_access_token(Some("reconciliation-generation-fence"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-generation-fence")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-generation-fence', ?, ?, ?, 1, ?, ?, ?, 'shadow')
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
    .expect("insert due reconciliation usage");
    let fetch_started = Arc::new(Notify::new());
    let release_first_fetch = Arc::new(Notify::new());
    let fetch_count = Arc::new(AtomicUsize::new(0));
    let route_started = Arc::clone(&fetch_started);
    let route_release = Arc::clone(&release_first_fetch);
    let route_count = Arc::clone(&fetch_count);
    let app = Router::new().route(
        "/usage",
        get(move || {
            let route_started = Arc::clone(&route_started);
            let route_release = Arc::clone(&route_release);
            let route_count = Arc::clone(&route_count);
            async move {
                if route_count.fetch_add(1, Ordering::SeqCst) == 0 {
                    route_started.notify_one();
                    route_release.notified().await;
                }
                Json(serde_json::json!({ "key": { "usage": 0 } }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve generation-fence upstream");
    });

    let first_run_proxy = proxy.clone();
    let first_run = tokio::spawn(async move {
        first_run_proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), fetch_started.notified())
        .await
        .expect("first upstream fetch starts");
    sqlx::query(
        r#"
        UPDATE upstream_reconciliation_usage
        SET request_count = request_count + 1, updated_at = updated_at + 1
        WHERE token_id = ? AND key_id = ? AND period_code = '2026-07-15/S1'
        "#,
    )
    .bind(&token.id)
    .bind(&key_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("record usage while the remote result is in flight");
    release_first_fetch.notify_one();

    assert_eq!(
        first_run
            .await
            .expect("join first reconciliation")
            .expect("finish stale reconciliation"),
        0,
        "the stale remote result must not settle a newer work generation"
    );
    let stale_generation: (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT work_generation, completed_generation, last_outcome FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read fenced generation");
    assert_eq!(stale_generation, (2, 0, None));
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_continuation_at()
            .await
            .expect("read continuation for newer generation"),
        Some(now),
    );

    // The direct helper retries admission while the scheduler persists a continuation.
    let settled = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let settled = proxy
                .run_upstream_reconciliation_once(&format!("http://{addr}"))
                .await
                .expect("retry current generation");
            if settled > 0 {
                break settled;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("settle current generation after foreground-safe admission");
    assert_eq!(settled, 1);
    let current_generation: (i64, i64, String) = sqlx::query_as(
        "SELECT work_generation, completed_generation, last_outcome FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read completed current generation");
    assert_eq!(current_generation, (2, 2, "no_adjustment".to_string()));
    assert_eq!(fetch_count.load(Ordering::SeqCst), 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_startup_resume_keeps_one_pending_representative_job() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-startup-resume"],
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
    let token = proxy
        .create_access_token(Some("reconciliation-startup-resume"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-startup-resume")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-startup-resume', ?, ?, ?, 1, ?, ?, ?, 'shadow')
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
    .expect("seed durable pending work without a live request wake");

    proxy
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("resume pending reconciliation work on startup");
    proxy
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("coalesce repeated startup resume");
    let representative_jobs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status IN ('queued', 'running')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count startup representative jobs");
    assert_eq!(representative_jobs, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_rejects_reclaimed_scheduled_job_claim() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-stale-claim"],
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
    let token = proxy
        .create_access_token(Some("reconciliation-stale-claim"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-stale-claim")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-stale-claim', ?, ?, ?, 1, ?, ?, ?, 'shadow')
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
    .expect("seed durable reconciliation work");
    proxy
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("enqueue representative job");
    let job = proxy
        .fetch_queued_scheduled_jobs(1)
        .await
        .expect("load representative job")
        .into_iter()
        .next()
        .expect("representative job is queued");
    let claimed = proxy
        .scheduled_job_mark_running(job.id)
        .await
        .expect("claim representative job")
        .expect("representative job became running");
    clock.set_now_ts(now + 61);
    assert_eq!(
        proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("recover stale representative job"),
        1
    );

    // This regression owns the stale-claim fence, not lazy maintenance-pool
    // growth. Prewarm so a concurrent test run cannot turn it into an
    // admission timing assertion before the fenced claim is inspected.
    proxy
        .prewarm_upstream_reconciliation_projection_capacity()
        .await
        .expect("prewarm reconciliation projection capacity");
    assert!(matches!(
        proxy
            .run_upstream_reconciliation_once_claimed_outcome(
                "http://127.0.0.1:9",
                claimed.id,
                claimed.claim_generation,
            )
            .await
            .expect("reject stale claimed worker"),
        ClaimedReconciliationRunOutcome::StaleClaim
    ));
    let work: (i64, i64) = sqlx::query_as(
        "SELECT work_generation, completed_generation FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read pending work after stale claim rejection");
    assert_eq!(work, (1, 0));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_rejects_reclaimed_claim_after_remote_fetch() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-reclaimed-during-fetch"],
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
        .create_access_token(Some("reconciliation-reclaimed-during-fetch"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-reclaimed-during-fetch")
        .await
        .expect("create upstream key");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, '2026-07-15/S1', 'project-reclaimed-during-fetch', ?, ?, ?, 1, ?, ?, ?, 'shadow')
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
    .expect("insert reconciliation usage");
    proxy
        .ensure_upstream_reconciliation_representative_job()
        .await
        .expect("enqueue representative job");
    let job = proxy
        .fetch_queued_scheduled_jobs(1)
        .await
        .expect("load representative job")
        .into_iter()
        .next()
        .expect("representative job is queued");
    let claimed = proxy
        .scheduled_job_mark_running(job.id)
        .await
        .expect("claim representative job")
        .expect("representative job became running");

    let fetch_started = Arc::new(Notify::new());
    let release_fetch = Arc::new(Notify::new());
    let route_started = Arc::clone(&fetch_started);
    let route_release = Arc::clone(&release_fetch);
    let app = Router::new().route(
        "/usage",
        get(move || {
            let route_started = Arc::clone(&route_started);
            let route_release = Arc::clone(&route_release);
            async move {
                route_started.notify_one();
                route_release.notified().await;
                Json(serde_json::json!({ "key": { "usage": 0 } }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reclaim-during-fetch upstream");
    });

    // This test exercises claim fencing after a request has started. Prewarm
    // the bounded local projection capacity so unrelated lazy-pool admission
    // does not turn it into a foreground-admission timing test.
    proxy
        .prewarm_upstream_reconciliation_projection_capacity()
        .await
        .expect("prewarm reconciliation projection capacity");
    let running_proxy = proxy.clone();
    let running = tokio::spawn(async move {
        running_proxy
            .run_upstream_reconciliation_once_claimed_outcome(
                &format!("http://{addr}"),
                claimed.id,
                claimed.claim_generation,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), fetch_started.notified())
        .await
        .expect("upstream usage fetch starts");
    clock.set_now_ts(now + 61);
    assert_eq!(
        proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("reclaim stale representative job"),
        1
    );
    release_fetch.notify_one();

    assert!(matches!(
        running
            .await
            .expect("join stale claimed worker")
            .expect("stale claimed worker exits cleanly"),
        ClaimedReconciliationRunOutcome::StaleClaim
    ));
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_local_backoff_state()
            .await
            .expect("read local backoff after stale worker"),
        (0, 0, 0)
    );
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("read global backoff after stale worker"),
        (0, 0, 0)
    );
    let work: (i64, i64) = sqlx::query_as(
        "SELECT work_generation, completed_generation FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read pending work after stale worker");
    assert_eq!(work, (1, 0));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_request_cap_never_settles_partially_observed_candidate() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-cap-a",
            "tvly-reconciliation-cap-b",
            "tvly-reconciliation-cap-c",
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
        .expect("save reconciliation settings");
    let token = proxy
        .create_access_token(Some("reconciliation-cap"))
        .await
        .expect("create token");
    let mut key_ids = Vec::new();
    for suffix in ["a", "b", "c"] {
        key_ids.push(
            proxy
                .add_or_undelete_key(&format!("tvly-reconciliation-cap-{suffix}"))
                .await
                .expect("create upstream key"),
        );
    }
    for key_id in &key_ids {
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 period_start, period_end, request_count, first_used_at,
                 last_used_at, updated_at, settlement_mode
               ) VALUES (?, ?, '2026-07-15/S1', ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')"#,
        )
        .bind(&token.id)
        .bind(key_id)
        .bind("project-reconciliation-cap")
        .bind(format!("token:{}", token.id))
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert multi-key candidate usage");
    }

    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES ('reconciliation-cap-research', ?, '2026-07-15/S2',
             'project-reconciliation-cap-research', 'token:reconciliation-cap-research',
             ?, ?, 1, ?, ?, ?, 'shadow')"#,
    )
    .bind(&key_ids[0])
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending research usage");
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_research (
             request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
           ) VALUES ('reconciliation-cap-research', 'reconciliation-cap-research', ?,
             '2026-07-15/S2', ?, NULL, ?)"#,
    )
    .bind(&key_ids[0])
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert pending research");

    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let route_hits = Arc::clone(&upstream_hits);
    let research_hits = Arc::new(AtomicUsize::new(0));
    let research_route_hits = Arc::clone(&research_hits);
    let app = Router::new()
        .route(
            "/usage",
            get(move || {
                let route_hits = Arc::clone(&route_hits);
                async move {
                    route_hits.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({ "key": { "usage": 0 } }))
                }
            }),
        )
        .route(
            "/research/reconciliation-cap-research",
            get(move || {
                let research_route_hits = Arc::clone(&research_route_hits);
                async move {
                    research_route_hits.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({ "status": "completed" }))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve capped reconciliation upstream");
    });

    let settled = proxy
        .run_upstream_reconciliation_once(&format!("http://{addr}"))
        .await
        .expect("run capped reconciliation");
    assert_eq!(
        upstream_hits.load(Ordering::SeqCst),
        2,
        "the first run observes at most two missing keys"
    );
    assert_eq!(
        research_hits.load(Ordering::SeqCst),
        1,
        "research may run only after the main request budget is durably consumed"
    );
    assert_eq!(settled, 0);
    let settlement_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_settlements WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read partial settlement count");
    assert_eq!(settlement_count, 1, "the candidate keeps a typed retry");
    let retry_outcome: String = sqlx::query_scalar(
        "SELECT last_outcome FROM upstream_reconciliation_work WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read capped candidate outcome");
    assert_eq!(retry_outcome, RECONCILIATION_OUTCOME_REMOTE_ATTEMPT_BUDGET);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_upstream_reconciliation_once_applies_key_scoped_backoff_for_429() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-key-backoff-hot",
            "tvly-reconciliation-key-backoff-cool",
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

    let hot_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-key-backoff-hot")
        .await
        .expect("create hot upstream key");
    let cool_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-key-backoff-cool")
        .await
        .expect("create cool upstream key");
    for (token_id, key_id, project_id, billing_subject) in [
        (
            "token-hot-a",
            hot_key_id.as_str(),
            "project-hot-a",
            "account:user-hot-a",
        ),
        (
            "token-hot-b",
            hot_key_id.as_str(),
            "project-hot-b",
            "account:user-hot-b",
        ),
        (
            "token-cool",
            cool_key_id.as_str(),
            "project-cool",
            "account:user-cool",
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
        .bind(key_id)
        .bind("2026-07-15/S1")
        .bind(project_id)
        .bind(billing_subject)
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert due reconciliation usage");
    }

    let hot_hits = Arc::new(AtomicUsize::new(0));
    let cool_hits = Arc::new(AtomicUsize::new(0));
    let app_hot_hits = Arc::clone(&hot_hits);
    let app_cool_hits = Arc::clone(&cool_hits);
    let app = Router::new().route(
        "/usage",
        get(move |headers: HeaderMap| {
            let hot_hits = Arc::clone(&app_hot_hits);
            let cool_hits = Arc::clone(&app_cool_hits);
            async move {
                let authorization = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if authorization.contains("tvly-reconciliation-key-backoff-hot") {
                    hot_hits.fetch_add(1, Ordering::SeqCst);
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(serde_json::json!({ "error": "rate limited" })),
                    )
                        .into_response();
                }
                if authorization.contains("tvly-reconciliation-key-backoff-cool") {
                    cool_hits.fetch_add(1, Ordering::SeqCst);
                    return Json(serde_json::json!({
                        "key": { "usage": 4 }
                    }))
                    .into_response();
                }
                StatusCode::UNAUTHORIZED.into_response()
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reconciliation usage upstream");
    });

    let settled = proxy
        .run_upstream_reconciliation_once(&format!("http://{addr}"))
        .await
        .expect("run reconciliation once");
    assert_eq!(settled, 1);
    assert_eq!(hot_hits.load(Ordering::SeqCst), 1);
    assert_eq!(cool_hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("mixed successful settlement does not escalate global 429 backoff"),
        (0, 0, 0)
    );

    let hot_rate_limited: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM upstream_reconciliation_settlements
        WHERE token_id IN ('token-hot-a', 'token-hot-b')
          AND status = 'rate_limited'
          AND degraded_reason = 'upstream429'
          AND next_attempt_at >= ?
        "#,
    )
    .bind(now + 300)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count hot key backoff settlements");
    assert_eq!(hot_rate_limited, 1);

    clock.set_now_ts(now + 301);
    let settled = proxy
        .run_upstream_reconciliation_once(&format!("http://{addr}"))
        .await
        .expect("run reconciliation after cooldown expires");
    assert_eq!(settled, 0);
    assert_eq!(hot_hits.load(Ordering::SeqCst), 2);
    let hot_retry_after_secs: i64 = sqlx::query_scalar(
        "SELECT retry_after_secs FROM api_key_transient_backoffs WHERE key_id = ? AND scope = 'period_reconciliation'",
    )
    .bind(&hot_key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read escalated hot key cooldown");
    assert_eq!(hot_retry_after_secs, 600);

    let cool_status: String = sqlx::query_scalar(
        r#"
        SELECT status
        FROM upstream_reconciliation_settlements
        WHERE token_id = 'token-cool'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read cool settlement");
    assert_eq!(cool_status, "shadow_settled");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_global_backoff_counts_only_attempted_candidates() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-shared-429"],
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
        .add_or_undelete_key("tvly-reconciliation-shared-429")
        .await
        .expect("create upstream key");
    let upstream_hits = Arc::new(AtomicUsize::new(0));
    let app_hits = Arc::clone(&upstream_hits);
    let app = Router::new().route(
        "/usage",
        get(move || {
            let app_hits = Arc::clone(&app_hits);
            async move {
                app_hits.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({ "error": "rate limited" })),
                )
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reconciliation usage upstream");
    });

    for (candidate, timestamp) in [now, now + 301, now + 902].into_iter().enumerate() {
        // Expose one candidate per elapsed key cooldown. The one-shot API is a
        // compatibility helper, so it must not be asked to spin through other
        // durable candidates that correctly remain key-cooldown deferred.
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(format!("shared-429-token-{candidate}"))
        .bind(&key_id)
        .bind("2026-07-15/S1")
        .bind(format!("shared-429-project-{candidate}"))
        .bind(format!("account:shared-429-{candidate}"))
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert reconciliation candidate");
        clock.set_now_ts(timestamp);
        let settled = proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("run shared-key reconciliation");
        assert_eq!(settled, 0);
        assert_eq!(upstream_hits.load(Ordering::SeqCst), candidate + 1);
    }

    assert_eq!(upstream_hits.load(Ordering::SeqCst), 3);
    let (pressure_streak, backoff_level, backoff_until) = proxy
        .key_store
        .upstream_reconciliation_global_backoff_state()
        .await
        .expect("read legacy global backoff state");
    assert_eq!(
        (pressure_streak, backoff_level, backoff_until),
        (0, 0, 0),
        "legacy global 429 metadata must not be advanced by reconciliation"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_key_429_does_not_stop_other_key_research() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-global-gate-hot",
            "tvly-reconciliation-global-gate-research",
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
        .expect("save compare settings");
    let hot_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-global-gate-hot")
        .await
        .expect("create hot key");
    let research_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-global-gate-research")
        .await
        .expect("create research key");

    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES ('global-gate-main', ?, '2026-07-15/S1', 'global-gate-main-project',
                  'account:global-gate-main', ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind(&hot_key_id)
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert main candidate");
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&proxy.key_store.pool)
        .await
        .expect("keep research fixture out of main work");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES ('global-gate-research', ?, '2026-07-15/R1', 'global-gate-research-project',
                  'account:global-gate-research', ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind(&research_key_id)
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
        ) VALUES ('global-gate-research-request', 'global-gate-research', ?, '2026-07-15/R1', ?, NULL, ?)
        "#,
    )
    .bind(&research_key_id)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert due research");
    sqlx::query(
        r#"
        INSERT INTO api_key_transient_backoffs (
            key_id, scope, cooldown_until, retry_after_secs, reason_code,
            source_request_log_id, created_at, updated_at
        ) VALUES (?, 'period_reconciliation', ?, 300, 'upstream429', NULL, ?, ?)
        "#,
    )
    .bind(&hot_key_id)
    .bind(now + 600)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("cool the main candidate key");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value) VALUES
            ('upstream_reconciliation_pressure_streak_v1', '3'),
            ('upstream_reconciliation_backoff_level_v1', '1'),
            ('upstream_reconciliation_backoff_until_v1', ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(now + 600)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy global gate");

    let usage_hits = Arc::new(AtomicUsize::new(0));
    let research_hits = Arc::new(AtomicUsize::new(0));
    let app_usage_hits = Arc::clone(&usage_hits);
    let app_research_hits = Arc::clone(&research_hits);
    let app = Router::new()
        .route(
            "/usage",
            get(move || {
                let app_usage_hits = Arc::clone(&app_usage_hits);
                async move {
                    app_usage_hits.fetch_add(1, Ordering::SeqCst);
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }),
        )
        .route(
            "/research/global-gate-research-request",
            get(move || {
                let app_research_hits = Arc::clone(&app_research_hits);
                async move {
                    app_research_hits.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({ "status": "completed" }))
                }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve cross-key reconciliation upstream");
    });

    let observed = proxy
        .run_upstream_reconciliation_once(&format!("http://{addr}"))
        .await
        .expect("run reconciliation with a legacy global gate");
    assert_eq!(
        observed, 0,
        "Research terminal observations do not settle a main candidate"
    );
    assert_eq!(usage_hits.load(Ordering::SeqCst), 0);
    assert_eq!(research_hits.load(Ordering::SeqCst), 1);
    let terminal_at: Option<i64> = sqlx::query_scalar(
        "SELECT terminal_at FROM upstream_reconciliation_research WHERE request_id = 'global-gate-research-request'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read research terminal state");
    assert!(terminal_at.is_some());
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("read unchanged legacy global gate"),
        (3, 1, now + 600)
    );
    let privacy_status = proxy
        .upstream_privacy_status()
        .await
        .expect("read legacy global diagnostics without using them as a gate");
    assert_eq!(privacy_status.reconciliation_pressure_streak, 3);
    assert_eq!(privacy_status.reconciliation_backoff_level, 1);
    assert_eq!(privacy_status.reconciliation_backoff_until, Some(now + 600));

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_all_key_cooldowns_defer_to_earliest_retry() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-cooldown-earliest-a",
            "tvly-reconciliation-cooldown-earliest-b",
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
        .expect("save compare settings");
    let key_ids = [
        proxy
            .add_or_undelete_key("tvly-reconciliation-cooldown-earliest-a")
            .await
            .expect("create first key"),
        proxy
            .add_or_undelete_key("tvly-reconciliation-cooldown-earliest-b")
            .await
            .expect("create second key"),
    ];
    for (index, key_id) in key_ids.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                period_start, period_end, request_count, first_used_at,
                last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(format!("all-cooldown-token-{index}"))
        .bind(key_id)
        .bind(format!("2026-07-15/C{index}"))
        .bind(format!("all-cooldown-project-{index}"))
        .bind(format!("account:all-cooldown-{index}"))
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert cooldown candidate");
    }
    for (key_id, cooldown_until) in key_ids.iter().zip([now + 600, now + 300]) {
        sqlx::query(
            r#"
            INSERT INTO api_key_transient_backoffs (
                key_id, scope, cooldown_until, retry_after_secs, reason_code,
                source_request_log_id, created_at, updated_at
            ) VALUES (?, 'period_reconciliation', ?, 300, 'upstream429', NULL, ?, ?)
            "#,
        )
        .bind(key_id)
        .bind(cooldown_until)
        .bind(now)
        .bind(now)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed key cooldown");
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
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            "http://127.0.0.1:9",
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("all key cooldowns produce a typed defer");
    assert!(matches!(
        outcome,
        ClaimedReconciliationRunOutcome::Deferred {
            reason: RECONCILIATION_RETRY_REASON_KEY_COOLDOWN,
            retry_at,
        } if retry_at == now + 300
    ));
    let work_counts: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(completed_generation), 0) FROM upstream_reconciliation_work WHERE token_id LIKE 'all-cooldown-token-%'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unfinished cooldown work");
    assert_eq!(work_counts, (2, 0));
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_continuation_at()
            .await
            .expect("read earliest key cooldown wake"),
        Some(now + 300)
    );

    let continuation = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            claim.id,
            claim.claim_generation,
            RECONCILIATION_RETRY_REASON_KEY_COOLDOWN,
            now + 300,
        )
        .await
        .expect("persist earliest cooldown continuation");
    assert_eq!(continuation.status, "queued");
    let representatives: (i64, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), MIN(available_at) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status IN ('queued', 'running')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read unique cooldown representative");
    assert_eq!(representatives, (1, Some(now + 300)));
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_local_backoff_state()
            .await
            .expect("read unchanged local pressure state"),
        (0, 0, 0)
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_fresh_429_mixed_key_allows_healthy_sibling() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-mixed-cooldown-a",
            "tvly-reconciliation-mixed-cooldown-b",
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
        .expect("save compare settings");
    let cooling_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-mixed-cooldown-a")
        .await
        .expect("create cooling key");
    let healthy_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-mixed-cooldown-b")
        .await
        .expect("create healthy key");
    for key_id in [&cooling_key_id, &healthy_key_id] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                period_start, period_end, request_count, first_used_at,
                last_used_at, updated_at, settlement_mode
            ) VALUES ('mixed-cooldown-token', ?, '2026-07-15/M1',
                       'mixed-cooldown-project', 'account:mixed-cooldown',
                       ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(key_id)
        .bind(now - 4_000)
        .bind(now - 900)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert mixed-key candidate usage");
    }
    let healthy_hits = Arc::new(AtomicUsize::new(0));
    let cooling_hits = Arc::new(AtomicUsize::new(0));
    let app_healthy_hits = Arc::clone(&healthy_hits);
    let app_cooling_hits = Arc::clone(&cooling_hits);
    let app = Router::new().route(
        "/usage",
        get(move |headers: HeaderMap| {
            let app_healthy_hits = Arc::clone(&app_healthy_hits);
            let app_cooling_hits = Arc::clone(&app_cooling_hits);
            async move {
                let authorization = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if authorization.contains("tvly-reconciliation-mixed-cooldown-b") {
                    app_healthy_hits.fetch_add(1, Ordering::SeqCst);
                    return Json(serde_json::json!({ "key": { "usage": 4 } })).into_response();
                }
                if authorization.contains("tvly-reconciliation-mixed-cooldown-a") {
                    app_cooling_hits.fetch_add(1, Ordering::SeqCst);
                }
                StatusCode::TOO_MANY_REQUESTS.into_response()
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve mixed-key reconciliation upstream");
    });

    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue mixed-key representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim mixed-key representative")
        .expect("mixed-key representative is claimable");
    let outcome = proxy
        .run_upstream_reconciliation_once_claimed_outcome(
            &format!("http://{addr}"),
            claim.id,
            claim.claim_generation,
        )
        .await
        .expect("defer mixed-key candidate at cooling key");
    assert!(matches!(
        outcome,
        ClaimedReconciliationRunOutcome::Deferred {
            reason: RECONCILIATION_RETRY_REASON_KEY_COOLDOWN,
            retry_at,
        } if retry_at == now + 300
    ));
    assert_eq!(healthy_hits.load(Ordering::SeqCst), 1);
    assert_eq!(cooling_hits.load(Ordering::SeqCst), 1);
    let work: (i64, i64, String) = sqlx::query_as(
        "SELECT work_generation, completed_generation, last_outcome FROM upstream_reconciliation_work WHERE token_id = 'mixed-cooldown-token'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read mixed-key work state");
    assert!(work.0 > 0, "mixed-key work has a durable generation");
    assert_eq!(work.1, 0, "cooling key keeps the candidate incomplete");
    assert_eq!(work.2, RECONCILIATION_OUTCOME_KEY_COOLDOWN.to_string());
    let observed_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_key_observations WHERE token_id = 'mixed-cooldown-token' AND key_id = ?",
    )
    .bind(&healthy_key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read healthy key observation");
    assert_eq!(observed_count, 1);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn upstream_429_backoff_escalates_persists_across_restart_and_resets() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-429-states"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    for (streak, level, delay_secs) in [
        (1_i64, 0_i64, 0_i64),
        (2, 0, 0),
        (3, 1, 120),
        (4, 2, 300),
        (5, 3, 600),
        (6, 4, 1_800),
    ] {
        let attempted_at = now + streak * 10_000;
        let state = proxy
            .key_store
            .update_upstream_reconciliation_global_backoff(true, attempted_at, None)
            .await
            .expect("persist upstream 429 backoff");
        assert_eq!(state, (streak, level, attempted_at + delay_secs));
    }
    let retry_after_until = now + 100_000;
    assert_eq!(
        proxy
            .key_store
            .update_upstream_reconciliation_global_backoff(
                true,
                now + 50_000,
                Some(retry_after_until)
            )
            .await
            .expect("honor upstream retry-after"),
        (7, 4, retry_after_until)
    );

    drop(proxy);
    let (restart_time, _) = BackendTime::manual_from_ts(now + 50_001);
    let restarted = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-429-states"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        restart_time,
    )
    .await
    .expect("restart proxy");
    assert_eq!(
        restarted
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("read persisted 429 backoff"),
        (7, 4, retry_after_until)
    );
    assert_eq!(
        restarted
            .key_store
            .update_upstream_reconciliation_global_backoff(false, now + 50_002, None)
            .await
            .expect("reset after successful reconciliation"),
        (0, 0, 0)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn local_reconciliation_pressure_is_separate_from_upstream_backoff() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-local-pressure"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    for offset in 0..3 {
        proxy
            .key_store
            .update_upstream_reconciliation_local_backoff(true, now + offset)
            .await
            .expect("record local pressure");
    }

    let (local_streak, local_level, local_until) = proxy
        .key_store
        .upstream_reconciliation_local_backoff_state()
        .await
        .expect("read local pressure state");
    assert_eq!(local_streak, 3);
    assert_eq!(local_level, 1);
    assert_eq!(local_until, now + 30 + 2);
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("read global backoff state"),
        (0, 0, 0)
    );
    let queued: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), MAX(available_at) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status = 'queued' AND trigger_source = 'auto'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read atomic delayed representative");
    assert_eq!(queued, (1, local_until));

    drop(proxy);
    let (restart_time, _) = BackendTime::manual_from_ts(now + 3);
    let restarted = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-local-pressure"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        restart_time,
    )
    .await
    .expect("restart proxy");
    assert_eq!(
        restarted
            .key_store
            .upstream_reconciliation_local_backoff_state()
            .await
            .expect("read persisted local pressure state"),
        (local_streak, local_level, local_until)
    );
    restarted
        .key_store
        .update_upstream_reconciliation_local_backoff(false, now + 3)
        .await
        .expect("recover local pressure");
    let queued_after_recovery: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status = 'queued' AND trigger_source = 'auto'",
    )
    .fetch_one(&restarted.key_store.pool)
    .await
    .expect("read recovered representative state");
    assert_eq!(queued_after_recovery, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn claimed_reconciliation_backoff_requeues_the_same_generation_fenced_job() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-claimed-backoff"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    for offset in 0..2 {
        proxy
            .key_store
            .update_upstream_reconciliation_local_backoff(true, now + offset)
            .await
            .expect("seed local pressure streak");
    }
    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue reconciliation representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim reconciliation representative")
        .expect("running claim");

    let (_, level, until) = proxy
        .key_store
        .update_upstream_reconciliation_local_backoff_claimed(
            true,
            now + 2,
            queued.job_id,
            claim.claim_generation,
        )
        .await
        .expect("atomically persist backoff and requeue claim");
    assert_eq!(level, 1);
    let job = proxy
        .scheduled_job_by_id(queued.job_id)
        .await
        .expect("read representative")
        .expect("representative exists");
    assert_eq!(job.status, "queued");
    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status IN ('queued', 'running')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count active representatives");
    assert_eq!(active_count, 1);
    let available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(queued.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read representative availability");
    assert_eq!(available_at, until);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_upstream_reconciliation_once_prioritizes_recent_windows_over_old_backlog() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-recent-priority-hot",
            "tvly-reconciliation-recent-priority-cool",
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

    let hot_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-priority-hot")
        .await
        .expect("create hot upstream key");
    let cool_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-priority-cool")
        .await
        .expect("create cool upstream key");
    for index in 0..20 {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(format!("token-backlog-{index:02}"))
        .bind(&hot_key_id)
        .bind("2026-07-13/S2")
        .bind(format!("project-backlog-{index:02}"))
        .bind(format!("account:user-backlog-{index:02}"))
        .bind(local_ts(2026, 7, 13, 11, 0))
        .bind(local_ts(2026, 7, 13, 22, 0))
        .bind(local_ts(2026, 7, 13, 11, 15))
        .bind(local_ts(2026, 7, 13, 21, 45))
        .bind(local_ts(2026, 7, 13, 21, 45))
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert old backlog usage");
    }
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("token-recent")
    .bind(&cool_key_id)
    .bind("2026-07-15/S1")
    .bind("project-recent")
    .bind("account:user-recent")
    .bind(now - 4_000)
    .bind(now - 900)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert recent usage");

    let hot_hits = Arc::new(AtomicUsize::new(0));
    let cool_hits = Arc::new(AtomicUsize::new(0));
    let app_hot_hits = Arc::clone(&hot_hits);
    let app_cool_hits = Arc::clone(&cool_hits);
    let app = Router::new().route(
        "/usage",
        get(move |headers: HeaderMap| {
            let hot_hits = Arc::clone(&app_hot_hits);
            let cool_hits = Arc::clone(&app_cool_hits);
            async move {
                let authorization = headers
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if authorization.contains("tvly-reconciliation-recent-priority-hot") {
                    hot_hits.fetch_add(1, Ordering::SeqCst);
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "300")],
                        Json(serde_json::json!({ "error": "rate limited" })),
                    )
                        .into_response();
                }
                if authorization.contains("tvly-reconciliation-recent-priority-cool") {
                    cool_hits.fetch_add(1, Ordering::SeqCst);
                    return Json(serde_json::json!({
                        "key": { "usage": 4 }
                    }))
                    .into_response();
                }
                StatusCode::UNAUTHORIZED.into_response()
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve reconciliation usage upstream");
    });

    let settled = proxy
        .run_upstream_reconciliation_once(&format!("http://{addr}"))
        .await
        .expect("run reconciliation once");
    assert_eq!(settled, 1);
    assert_eq!(hot_hits.load(Ordering::SeqCst), 1);
    assert_eq!(cool_hits.load(Ordering::SeqCst), 1);

    let recent_status: String = sqlx::query_scalar(
        r#"
        SELECT status
        FROM upstream_reconciliation_settlements
        WHERE token_id = 'token-recent'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read recent settlement");
    assert_eq!(recent_status, "shadow_settled");

    let backlog_rate_limited: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM upstream_reconciliation_settlements
        WHERE token_id LIKE 'token-backlog-%'
          AND status = 'rate_limited'
          AND degraded_reason = 'upstream429'
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count backlog settlements");
    assert_eq!(backlog_rate_limited, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn next_upstream_reconciliation_candidates_keep_recent_refill_ahead_of_backlog() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-recent-order-hot",
            "tvly-reconciliation-recent-order-cool",
        ],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let hot_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-order-hot")
        .await
        .expect("create hot upstream key");
    let cool_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-order-cool")
        .await
        .expect("create cool upstream key");

    for index in 0..15 {
        let period_start = now.saturating_sub(((index + 2) as i64) * 900);
        let period_end = period_start + 300;
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(format!("token-recent-order-{index:02}"))
        .bind(&cool_key_id)
        .bind(format!("2026-07-15/S1-{index:02}"))
        .bind(format!("project-recent-order-{index:02}"))
        .bind(format!("account:user-recent-order-{index:02}"))
        .bind(period_start)
        .bind(period_end)
        .bind(period_start)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert recent usage");
    }
    for index in 0..2 {
        let period_start = local_ts(2026, 7, 13, 11 + index, 0);
        let period_end = period_start + 300;
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(format!("token-backlog-order-{index:02}"))
        .bind(&hot_key_id)
        .bind(format!("2026-07-13/S2-{index:02}"))
        .bind(format!("project-backlog-order-{index:02}"))
        .bind(format!("account:user-backlog-order-{index:02}"))
        .bind(period_start)
        .bind(period_end)
        .bind(period_start)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert backlog usage");
    }

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(20)
        .await
        .expect("load candidate batch");
    assert_eq!(batch.recent_lane_budget, 12);
    assert_eq!(batch.backlog_lane_budget, 8);
    assert_eq!(batch.recent_candidate_count, 15);
    assert_eq!(batch.backlog_candidate_count, 2);
    assert_eq!(batch.candidates.len(), 17);
    assert!(
        batch
            .candidates
            .iter()
            .take(batch.recent_candidate_count as usize)
            .all(|candidate| candidate.token_id.starts_with("token-recent-order-"))
    );
    assert!(
        batch
            .candidates
            .iter()
            .skip(batch.recent_candidate_count as usize)
            .all(|candidate| candidate.token_id.starts_with("token-backlog-order-"))
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn next_upstream_reconciliation_candidates_treat_midnight_closing_s3_as_recent() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-recent-s3-boundary"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-s3-boundary")
        .await
        .expect("create upstream key");

    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("token-recent-s3-boundary")
    .bind(&key_id)
    .bind("2026-07-13/S3")
    .bind("project-recent-s3-boundary")
    .bind("account:user-recent-s3-boundary")
    .bind(local_ts(2026, 7, 13, 22, 0))
    .bind(local_ts(2026, 7, 14, 0, 0))
    .bind(local_ts(2026, 7, 13, 22, 5))
    .bind(local_ts(2026, 7, 14, 0, 0))
    .bind(local_ts(2026, 7, 14, 0, 0))
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert midnight-closing s3 usage");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("token-backlog-older")
    .bind(&key_id)
    .bind("2026-07-12/S2")
    .bind("project-backlog-older")
    .bind("account:user-backlog-older")
    .bind(local_ts(2026, 7, 12, 11, 0))
    .bind(local_ts(2026, 7, 12, 22, 0))
    .bind(local_ts(2026, 7, 12, 11, 5))
    .bind(local_ts(2026, 7, 12, 22, 0))
    .bind(local_ts(2026, 7, 12, 22, 0))
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert older backlog usage");

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(20)
        .await
        .expect("load candidate batch");
    assert_eq!(batch.recent_candidate_count, 1);
    assert_eq!(batch.backlog_candidate_count, 1);
    assert_eq!(batch.candidates[0].token_id, "token-recent-s3-boundary");
    assert_eq!(batch.candidates[1].token_id, "token-backlog-older");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn next_upstream_reconciliation_candidates_skip_pending_recent_rows_before_limiting() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-recent-pending-queue"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-pending-queue")
        .await
        .expect("create upstream key");

    for index in 0..128 {
        let period_end = now.saturating_sub(((index + 1) as i64) * 600);
        let period_start = period_end.saturating_sub(300);
        let period_code = format!("2026-07-15/S2-pending-{index:02}");
        let token_id = format!("token-recent-pending-{index:02}");
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(&token_id)
        .bind(&key_id)
        .bind(&period_code)
        .bind(format!("project-recent-pending-{index:02}"))
        .bind(format!("account:user-recent-pending-{index:02}"))
        .bind(period_start)
        .bind(period_end)
        .bind(period_start)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert pending recent usage");
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_research (
                request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, NULL, ?)
            "#,
        )
        .bind(format!("research-pending-{index:02}"))
        .bind(&token_id)
        .bind(&key_id)
        .bind(&period_code)
        .bind(period_end.saturating_sub(60))
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert pending research");
    }

    let eligible_period_start = local_ts(2026, 7, 14, 8, 0);
    let eligible_period_end = eligible_period_start + 300;
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("token-recent-eligible")
    .bind(&key_id)
    .bind("2026-07-14/S1-eligible")
    .bind("project-recent-eligible")
    .bind("account:user-recent-eligible")
    .bind(eligible_period_start)
    .bind(eligible_period_end)
    .bind(eligible_period_start)
    .bind(eligible_period_end)
    .bind(eligible_period_end)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert eligible recent usage");

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(12)
        .await
        .expect("load candidate batch");
    assert_eq!(batch.recent_lane_budget, 12);
    assert_eq!(batch.backlog_lane_budget, 0);
    assert_eq!(batch.recent_candidate_count, 1);
    assert_eq!(batch.backlog_candidate_count, 0);
    assert_eq!(batch.candidates.len(), 1);
    assert_eq!(batch.candidates[0].token_id, "token-recent-eligible");
    assert_eq!(batch.candidates[0].pending_research, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn next_upstream_reconciliation_candidates_interleave_recent_keys_before_limiting() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec![
            "tvly-reconciliation-recent-interleave-hot",
            "tvly-reconciliation-recent-interleave-cool",
        ],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let hot_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-interleave-hot")
        .await
        .expect("create hot key");
    let cool_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-recent-interleave-cool")
        .await
        .expect("create cool key");

    // The bounded candidate page is 96 rows for this lane. More hot-key rows
    // than that must not hide the older cool-key candidate before per-key
    // ranking has a chance to interleave it.
    for index in 0..100 {
        let period_end = now.saturating_sub(((index + 1) as i64) * 600);
        let period_start = period_end.saturating_sub(300);
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind(format!("token-recent-interleave-hot-{index:02}"))
        .bind(&hot_key_id)
        .bind(format!("2026-07-15/S2-hot-{index:02}"))
        .bind(format!("project-recent-interleave-hot-{index:02}"))
        .bind(format!("account:user-recent-interleave-hot-{index:02}"))
        .bind(period_start)
        .bind(period_end)
        .bind(period_start)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert hot recent usage");
    }
    let cool_period_start = local_ts(2026, 7, 14, 8, 0);
    let cool_period_end = cool_period_start + 300;
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("token-recent-interleave-cool")
    .bind(&cool_key_id)
    .bind("2026-07-14/S2-cool")
    .bind("project-recent-interleave-cool")
    .bind("account:user-recent-interleave-cool")
    .bind(cool_period_start)
    .bind(cool_period_end)
    .bind(cool_period_start)
    .bind(cool_period_end)
    .bind(cool_period_end)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert cool recent usage");

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(20)
        .await
        .expect("load candidate batch");
    assert_eq!(batch.recent_candidate_count, 20);
    assert_eq!(batch.backlog_candidate_count, 0);
    assert_eq!(batch.candidates.len(), 20);
    assert_eq!(
        batch
            .candidates
            .iter()
            .filter(|candidate| candidate.token_id == "token-recent-interleave-cool")
            .count(),
        1
    );
    assert_eq!(
        batch
            .candidates
            .iter()
            .filter(|candidate| candidate
                .token_id
                .starts_with("token-recent-interleave-hot-"))
            .count(),
        19
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn next_upstream_reconciliation_candidates_page_logical_windows_before_limiting() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-window-page"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let hot_period_start = now.saturating_sub(900);
    let hot_period_end = hot_period_start + 300;
    for index in 0..161 {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
            "#,
        )
        .bind("token-many-keys-one-window")
        .bind(format!("key-many-keys-{index:03}"))
        .bind("2026-07-15/S2-many-keys")
        .bind("project-many-keys")
        .bind("account:user-many-keys")
        .bind(hot_period_start)
        .bind(hot_period_end)
        .bind(hot_period_start)
        .bind(hot_period_end)
        .bind(hot_period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert multi-key logical window");
    }
    let cool_period_start = now.saturating_sub(1_500);
    let cool_period_end = cool_period_start + 300;
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind("token-visible-after-window-page")
    .bind("key-visible-after-window-page")
    .bind("2026-07-15/S2-visible")
    .bind("project-visible-after-window-page")
    .bind("account:user-visible-after-window-page")
    .bind(cool_period_start)
    .bind(cool_period_end)
    .bind(cool_period_start)
    .bind(cool_period_end)
    .bind(cool_period_end)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert visible logical window");

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(2)
        .await
        .expect("load candidate batch");
    assert_eq!(batch.candidates.len(), 2);
    assert!(
        batch
            .candidates
            .iter()
            .any(|candidate| candidate.token_id == "token-many-keys-one-window")
    );
    assert!(
        batch
            .candidates
            .iter()
            .any(|candidate| candidate.token_id == "token-visible-after-window-page")
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn s3_next_day_settlement_does_not_restore_current_hour_quota() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let start_ts = local_ts(2026, 7, 14, 23, 55);
    let (backend_time, clock) = BackendTime::manual_from_ts(start_ts);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-s3"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let token = proxy
        .create_access_token(Some("reconciliation-s3"))
        .await
        .expect("create token");
    proxy
        .charge_token_quota(&token.id, 10)
        .await
        .expect("charge prior-day estimate");

    clock.set_now_ts(local_ts(2026, 7, 15, 0, 12));
    let before = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("quota before s3 settlement");
    assert_eq!(before.hourly_used, 10);
    assert_eq!(before.daily_used, 0);
    assert_eq!(before.monthly_used, 10);

    let candidate = UpstreamReconciliationCandidate {
        token_id: token.id.clone(),
        period_code: "2026-07-14/S3".to_string(),
        project_id: "anonymous-project".to_string(),
        billing_subject: format!("token:{}", token.id),
        settlement_mode: "actual".to_string(),
        period_start: local_ts(2026, 7, 14, 22, 0),
        period_end: local_ts(2026, 7, 15, 0, 0),
        pending_research: 0,
        degraded: false,
    };
    assert!(
        proxy
            .key_store
            .settle_upstream_reconciliation(&candidate, 7, 10, None)
            .await
            .expect("s3 settlement")
    );

    let after = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("quota after s3 settlement");
    assert_eq!(after.hourly_used, before.hourly_used);
    assert_eq!(after.daily_used, before.daily_used);
    assert_eq!(after.monthly_used, 7);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn daily_reconciliation_progress_includes_actual_mode_windows() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-progress"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    for (token_id, key_id, billing_subject, status) in [
        (
            "token-settled",
            "key-one",
            "account:user-settled",
            Some("settled"),
        ),
        (
            "token-degraded",
            "key-one",
            "account:user-degraded",
            Some("degraded"),
        ),
        ("token-pending", "key-two", "account:user-pending", None),
        (
            "token-shadow",
            "key-one",
            "account:user-shadow",
            Some("shadow_settled"),
        ),
    ] {
        let period_code = format!("2026-07-15/{token_id}");
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
                request_count, first_used_at, last_used_at, updated_at, settlement_mode
            ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'actual')
            "#,
        )
        .bind(token_id)
        .bind(key_id)
        .bind(&period_code)
        .bind(format!("project-{token_id}"))
        .bind(billing_subject)
        .bind(now - 3_600)
        .bind(now - 900)
        .bind(now - 3_600)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert actual reconciliation usage");

        if let Some(status) = status {
            sqlx::query(
                r#"
                INSERT INTO upstream_reconciliation_settlements (
                    settlement_key, token_id, period_code, project_id, billing_subject,
                    period_start, period_end, status, attempt_count, created_at, updated_at, settled_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)
                "#,
            )
            .bind(format!("v1:{token_id}:{period_code}"))
            .bind(token_id)
            .bind(&period_code)
            .bind(format!("project-{token_id}"))
            .bind(billing_subject)
            .bind(now - 3_600)
            .bind(now - 900)
            .bind(status)
            .bind(now - 900)
            .bind(now - 900)
            .bind(now - 900)
            .execute(&proxy.key_store.pool)
            .await
            .expect("insert actual reconciliation settlement");
        }
    }

    for (request_id, token_id, key_id, terminal_at) in [
        (
            "research-terminal",
            "token-settled",
            "key-one",
            Some(now - 800),
        ),
        ("research-pending", "token-pending", "key-two", None),
    ] {
        sqlx::query(
            r#"
            INSERT INTO upstream_reconciliation_research (
                request_id, token_id, key_id, period_code, created_at, terminal_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(request_id)
        .bind(token_id)
        .bind(key_id)
        .bind(format!("2026-07-15/{token_id}"))
        .bind(now - 900)
        .bind(terminal_at)
        .bind(now - 800)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert actual reconciliation research");
    }

    let (progress, by_key) = proxy
        .key_store
        .daily_reconciliation_progress()
        .await
        .expect("read daily reconciliation progress");
    assert_eq!(progress.observed_accounts, 4);
    assert_eq!(progress.accounts_with_settled_period, 1);
    assert_eq!(progress.fully_terminal_accounts, 3);
    assert_eq!(progress.observed_periods, 4);
    assert_eq!(progress.settled_periods, 1);
    assert_eq!(progress.degraded_periods, 1);
    assert_eq!(progress.pending_periods, 1);
    assert_eq!(progress.research_total, 2);
    assert_eq!(progress.research_terminal, 1);
    assert_eq!(progress.research_pending, 1);
    assert!(by_key.iter().any(|key| {
        key.key_id_hint == "key-one"
            && key.terminal_research == 1
            && key.pending_research == 0
            && key.pending_project_ids == 0
    }));
    assert!(by_key.iter().any(|key| {
        key.key_id_hint == "key-two"
            && key.terminal_research == 0
            && key.pending_research == 1
            && key.pending_project_ids == 1
    }));

    let _ = std::fs::remove_file(db_path);
}

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

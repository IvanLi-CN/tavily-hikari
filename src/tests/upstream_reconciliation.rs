use super::*;
use chrono::{Local, LocalResult, TimeZone};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn local_ts(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
    match Local.with_ymd_and_hms(year, month, day, hour, minute, 0) {
        LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => value.timestamp(),
        LocalResult::None => panic!("local time is unavailable"),
    }
}

fn reconciliation_test_db_path() -> PathBuf {
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
        reservation_id: None,
        scheduling_key_id: String::new(),
        fair_rank: 0,
        hydration_cursor_key_id: None,
        upstream_usage_total: 0,
        hydration_complete: false,
    };
    assert!(
        proxy
            .key_store
            .settle_upstream_reconciliation(&candidate, 7, 10)
            .await
            .expect("first settlement")
    );
    assert!(
        !proxy
            .key_store
            .settle_upstream_reconciliation(&candidate, 7, 10)
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
    let job: (String, String) = sqlx::query_as(
        "SELECT status, trigger_source
         FROM scheduled_jobs
         WHERE job_type = 'upstream_reconciliation'
         ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("record usage should enqueue representative job");
    assert_eq!(job, ("queued".to_string(), "request".to_string()));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn shadow_compare_activity_persists_cutover_epoch_when_meta_is_missing() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-shadow-compare-epoch"],
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
    settings.upstream_precise_reconciliation_enabled = true;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("save precise settings");

    assert!(
        proxy
            .key_store
            .get_meta_i64("upstream_reconciliation_ready_after_v1")
            .await
            .expect("load reconciliation epoch before compare check")
            .unwrap_or(0)
            <= 0
    );

    assert!(
        proxy
            .key_store
            .upstream_reconciliation_shadow_compare_active_with_settings(&settings)
            .await
            .expect("compute compare-active state before epoch expires")
    );
    let persisted_epoch = proxy
        .key_store
        .get_meta_i64("upstream_reconciliation_ready_after_v1")
        .await
        .expect("load persisted reconciliation epoch after compare check")
        .expect("persisted reconciliation epoch");
    assert_eq!(persisted_epoch, business_period_for_timestamp(now).ends_at);

    clock.set_now_ts(persisted_epoch + 1);
    assert!(
        !proxy
            .key_store
            .upstream_reconciliation_shadow_compare_active_with_settings(&settings)
            .await
            .expect("compute compare-active state after epoch expires")
    );
    assert_eq!(
        proxy
            .key_store
            .get_meta_i64("upstream_reconciliation_ready_after_v1")
            .await
            .expect("load reconciliation epoch after expiry"),
        Some(persisted_epoch)
    );

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
        reservation_id: None,
        scheduling_key_id: String::new(),
        fair_rank: 0,
        hydration_cursor_key_id: None,
        upstream_usage_total: 0,
        hydration_complete: false,
    };
    assert!(
        proxy
            .key_store
            .settle_upstream_reconciliation_shadow(&candidate, 7, 7)
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

    let usage_started = Arc::new(AtomicUsize::new(usize::MAX));
    let usage_started_for_route = Arc::clone(&usage_started);
    let run_started = std::time::Instant::now();
    let app = Router::new()
        .route(
            "/usage",
            get(move || {
                let usage_started = Arc::clone(&usage_started_for_route);
                async move {
                    usage_started.store(
                        run_started.elapsed().as_millis().min(usize::MAX as u128) as usize,
                        Ordering::SeqCst,
                    );
                    Json(serde_json::json!({
                        "key": { "usage": 0 }
                    }))
                }
            }),
        )
        .route(
            "/research/research-runtime-marker",
            get(|| async {
                tokio::time::sleep(std::time::Duration::from_millis(2_100)).await;
                Json(serde_json::json!({ "status": "completed" }))
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
    assert!(
        usage_started.load(Ordering::SeqCst) < 2_000,
        "the first main settlement attempt must start before research consumes the budget"
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
    let terminal_at: Option<i64> = sqlx::query_scalar(
        "SELECT terminal_at FROM upstream_reconciliation_research WHERE request_id = 'research-runtime-marker'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read terminal research");
    assert_eq!(terminal_at, Some(now));

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
    for candidate in 0..3 {
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
    }

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

    for timestamp in [now, now + 301, now + 902] {
        clock.set_now_ts(timestamp);
        let settled = proxy
            .run_upstream_reconciliation_once(&format!("http://{addr}"))
            .await
            .expect("run shared-key reconciliation");
        assert_eq!(settled, 0);
    }

    assert_eq!(upstream_hits.load(Ordering::SeqCst), 3);
    let (pressure_streak, backoff_level, backoff_until) = proxy
        .key_store
        .upstream_reconciliation_global_backoff_state()
        .await
        .expect("read global backoff state");
    assert_eq!(pressure_streak, 3);
    assert_eq!(backoff_level, 1);
    assert_eq!(backoff_until, now + 902 + 120);

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
    assert_eq!(local_until, now + 60 + 2);
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_global_backoff_state()
            .await
            .expect("read global backoff state"),
        (0, 0, 0)
    );

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
async fn reconciliation_work_projection_persists_cursor_and_recovers_expired_reservation() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_700_000_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-work-projection"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    let token_id = "projection-token";
    let key_id = "projection-key";
    let period_code = "2023-11";
    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            settlement_mode, period_start, period_end, request_count,
            first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(token_id)
    .bind(key_id)
    .bind(period_code)
    .bind("projection-project")
    .bind("token:projection-token")
    .bind("actual")
    .bind(1_699_900_000_i64)
    .bind(1_699_999_000_i64)
    .bind(1_i64)
    .bind(1_699_999_000_i64)
    .bind(1_699_999_000_i64)
    .bind(1_699_999_000_i64)
    .execute(&proxy.key_store.pool)
    .await
    .unwrap();

    let first = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap();
    assert_eq!(first.candidates.len(), 1);
    let first_candidate = first.candidates[0].clone();
    let first_reservation_id = first_candidate
        .reservation_id
        .as_deref()
        .expect("candidate should carry a durable reservation");
    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_continuation_at()
            .await
            .unwrap(),
        Some(1_700_000_030)
    );

    let (status, stored_reservation): (String, Option<String>) = sqlx::query_as(
        "SELECT status, reservation_id
         FROM upstream_reconciliation_work
         WHERE work_key = ?",
    )
    .bind(format!("v1:{token_id}:{period_code}"))
    .fetch_one(&proxy.key_store.pool)
    .await
    .unwrap();
    assert_eq!(status, "reserved");
    assert_eq!(stored_reservation.as_deref(), Some(first_reservation_id));

    let cursor_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_cursors
         WHERE lane = 'recent'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .unwrap();
    assert_eq!(cursor_count, 1);

    sqlx::query(
        "UPDATE upstream_reconciliation_work
         SET reservation_expires_at = ?, updated_at = ?
         WHERE work_key = ?",
    )
    .bind(1_700_000_030_i64)
    .bind(1_700_000_000_i64)
    .bind(format!("v1:{token_id}:{period_code}"))
    .execute(&proxy.key_store.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO scheduled_jobs (
            job_type, trigger_source, key_id, status, attempt, queued_at,
            available_at, claim_generation, started_at, finished_at
        ) VALUES ('upstream_reconciliation', 'scheduler', NULL, 'running', 1, ?, ?, 0, ?, NULL)",
    )
    .bind(1_699_999_800_i64)
    .bind(1_699_999_800_i64)
    .bind(1_699_999_800_i64)
    .execute(&proxy.key_store.pool)
    .await
    .unwrap();
    drop(proxy);

    let (backend_time, _) = BackendTime::manual_from_ts(1_700_000_000);
    let restarted = TavilyProxy::with_options_and_time(
        vec!["tvly-work-projection"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();
    restarted.abandon_active_scheduled_jobs().await.unwrap();
    let recovered = restarted
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap();

    assert_eq!(recovered.candidates.len(), 1);
    assert_eq!(recovered.candidates[0].token_id, token_id);
    assert_ne!(
        recovered.candidates[0].reservation_id.as_deref(),
        Some(first_reservation_id)
    );
    let recovered_job_status: String = sqlx::query_scalar(
        "SELECT status FROM scheduled_jobs
         WHERE job_type = 'upstream_reconciliation'
         ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&restarted.key_store.pool)
    .await
    .unwrap();
    assert_eq!(recovered_job_status, "queued");

    let mut unfenced_candidate = recovered.candidates[0].clone();
    unfenced_candidate.reservation_id = None;
    assert!(
        !restarted
            .key_store
            .settle_upstream_reconciliation(&unfenced_candidate, 7, 0)
            .await
            .unwrap()
    );
    let recovered_status: String =
        sqlx::query_scalar("SELECT status FROM upstream_reconciliation_work WHERE work_key = ?")
            .bind(format!("v1:{token_id}:{period_code}"))
            .fetch_one(&restarted.key_store.pool)
            .await
            .unwrap();
    assert_eq!(recovered_status, "reserved");

    assert!(
        !restarted
            .key_store
            .settle_upstream_reconciliation(&first_candidate, 7, 0)
            .await
            .unwrap()
    );
    let stale_adjustment_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM billing_reconciliation_adjustments
         WHERE settlement_key = ?",
    )
    .bind(format!("v1:{token_id}:{period_code}"))
    .fetch_one(&restarted.key_store.pool)
    .await
    .unwrap();
    assert_eq!(stale_adjustment_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn aborted_reconciliation_recovery_finishes_job_and_requeues_continuation() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_700_000_000_i64;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-aborted-reconciliation"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            settlement_mode, period_start, period_end, request_count,
            first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, ?, ?, ?)",
    )
    .bind("aborted-reconciliation-token")
    .bind("aborted-reconciliation-key")
    .bind("2023-11-aborted")
    .bind("aborted-reconciliation-project")
    .bind("account:aborted-reconciliation")
    .bind(now - 3_600)
    .bind(now - 900)
    .bind(now - 3_600)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .unwrap();

    let candidate = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .next()
        .unwrap();
    assert!(candidate.reservation_id.is_some());
    let job_id = proxy
        .scheduled_job_claim("upstream_reconciliation", "scheduler", None, 1)
        .await
        .unwrap()
        .unwrap();

    assert!(
        proxy
            .recover_upstream_reconciliation_after_aborted_run(
                job_id,
                1,
                "success",
                "settled=unknown budget_exhausted=true".to_string(),
                "test_aborted_recovery_failed",
            )
            .await
    );

    let current_job: (String, Option<String>) =
        sqlx::query_as("SELECT status, message FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .unwrap();
    assert_eq!(current_job.0, "success");
    assert_eq!(
        current_job.1.as_deref(),
        Some("settled=unknown budget_exhausted=true")
    );

    let work: (String, Option<String>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT status, reservation_id, reservation_expires_at, next_attempt_at
         FROM upstream_reconciliation_work
         WHERE work_key = 'v1:aborted-reconciliation-token:2023-11-aborted'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .unwrap();
    assert_eq!(work.0, "retry");
    assert_eq!(work.1, None);
    assert_eq!(work.2, None);
    assert_eq!(work.3, Some(now + 60));

    let queued: (i64, Option<i64>) = sqlx::query_as(
        "SELECT COUNT(*), MIN(available_at)
         FROM scheduled_jobs
         WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .unwrap();
    assert_eq!(queued, (1, Some(now + 60)));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_cursor_skips_deferred_work_until_retry_is_due() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-work-cursor"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    for (index, period_end) in [now - 900, now - 1_500].into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                settlement_mode, period_start, period_end, request_count,
                first_used_at, last_used_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
        )
        .bind(format!("cursor-token-{index}"))
        .bind("cursor-key")
        .bind(format!("2026-07-15/S2-cursor-{index}"))
        .bind("cursor-project")
        .bind("account:cursor-user")
        .bind("shadow")
        .bind(period_end - 300)
        .bind(period_end)
        .bind(period_end - 300)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .unwrap();
    }

    let first = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .next()
        .unwrap();
    proxy
        .key_store
        .mark_reconciliation_retry(&first, "waiting", now + 60, None)
        .await
        .unwrap();

    let second = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .next()
        .unwrap();
    assert_ne!(second.token_id, first.token_id);
    assert_eq!(second.token_id, "cursor-token-1");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_candidate_query_plan_uses_work_projection_and_hydrates_selected_windows() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-work-query-plan"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            settlement_mode, period_start, period_end, request_count,
            first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind("query-plan-token")
    .bind("query-plan-key-2")
    .bind("2026-07-15/S2-query-plan")
    .bind("query-plan-project")
    .bind("account:query-plan-user")
    .bind("shadow")
    .bind(now - 1_200)
    .bind(now - 900)
    .bind(now - 1_200)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            settlement_mode, period_start, period_end, request_count,
            first_used_at, last_used_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)",
    )
    .bind("query-plan-token")
    .bind("query-plan-key")
    .bind("2026-07-15/S2-query-plan")
    .bind("query-plan-project")
    .bind("account:query-plan-user")
    .bind("shadow")
    .bind(now - 1_200)
    .bind(now - 900)
    .bind(now - 1_200)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .unwrap();

    let plan_rows = sqlx::query_as::<_, (i64, i64, i64, String)>(
        "EXPLAIN QUERY PLAN
         SELECT work_key
         FROM upstream_reconciliation_work
         WHERE status IN ('ready', 'retry') AND next_attempt_at <= 1700000000
         ORDER BY period_end DESC, token_id ASC, period_code ASC
         LIMIT 16",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .unwrap();
    let plan = plan_rows
        .into_iter()
        .map(|(_, _, _, detail)| detail)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plan.contains("upstream_reconciliation_work"), "{plan}");
    assert!(!plan.contains("upstream_reconciliation_usage"), "{plan}");

    let batch = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap();
    assert_eq!(batch.candidates[0].token_id, "query-plan-token");
    let hydrated = proxy
        .key_store
        .reconciliation_key_ids_batch(&[(
            "query-plan-token".to_string(),
            "2026-07-15/S2-query-plan".to_string(),
        )])
        .await
        .unwrap();
    assert_eq!(
        hydrated.get(&(
            "query-plan-token".to_string(),
            "2026-07-15/S2-query-plan".to_string(),
        )),
        Some(&vec![
            "query-plan-key".to_string(),
            "query-plan-key-2".to_string(),
        ])
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_hydration_page_is_bounded_and_preserves_accumulated_usage() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-hydration-page"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    for index in 0..33 {
        let key_id = format!("hydration-key-{index:02}");
        sqlx::query(
            "INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                settlement_mode, period_start, period_end, request_count,
                first_used_at, last_used_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, ?, ?, ?)",
        )
        .bind("hydration-token")
        .bind(key_id)
        .bind("2026-07-15/S2-hydration")
        .bind("hydration-project")
        .bind("account:hydration")
        .bind(now - 1_200)
        .bind(now - 900)
        .bind(now - 1_200)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .unwrap();
    }

    let first = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .next()
        .unwrap();
    let first_page = proxy
        .key_store
        .reconciliation_key_pages(std::slice::from_ref(&first))
        .await
        .unwrap()
        .remove(&(first.token_id.clone(), first.period_code.clone()))
        .unwrap();
    assert_eq!(first_page.key_ids.len(), 32);
    assert!(first_page.has_more);
    assert!(
        proxy
            .key_store
            .advance_upstream_reconciliation_hydration(
                &first,
                first_page.key_ids.last().map(String::as_str),
                32,
                true,
            )
            .await
            .unwrap()
    );

    let second = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await
        .unwrap()
        .candidates
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(second.upstream_usage_total, 32);
    let second_page = proxy
        .key_store
        .reconciliation_key_pages(std::slice::from_ref(&second))
        .await
        .unwrap()
        .remove(&(second.token_id.clone(), second.period_code.clone()))
        .unwrap();
    assert_eq!(second_page.key_ids.len(), 1);
    assert!(!second_page.has_more);
    assert!(
        proxy
            .key_store
            .advance_upstream_reconciliation_hydration(
                &second,
                second_page.key_ids.last().map(String::as_str),
                1,
                false,
            )
            .await
            .unwrap()
    );

    let state: (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT upstream_usage_total, hydration_complete, hydration_cursor_key_id
         FROM upstream_reconciliation_work
         WHERE work_key = 'v1:hydration-token:2026-07-15/S2-hydration'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .unwrap();
    assert_eq!(state.0, 33);
    assert_eq!(state.1, 1);
    assert_eq!(state.2.as_deref(), Some("hydration-key-32"));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_legacy_backfill_advances_persisted_pages() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-backfill"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    for index in 0..257 {
        let period_end = now - 10_000 + index;
        sqlx::query(
            "INSERT INTO upstream_reconciliation_usage (
                token_id, key_id, period_code, project_id, billing_subject,
                settlement_mode, period_start, period_end, request_count,
                first_used_at, last_used_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, ?, ?, ?)",
        )
        .bind(format!("backfill-token-{index:03}"))
        .bind("backfill-key")
        .bind(format!("2026-07-15/S2-backfill-{index:03}"))
        .bind("backfill-project")
        .bind("account:backfill")
        .bind(period_end - 300)
        .bind(period_end)
        .bind(period_end - 300)
        .bind(period_end)
        .bind(period_end)
        .execute(&proxy.key_store.pool)
        .await
        .unwrap();
    }
    sqlx::query("DELETE FROM upstream_reconciliation_work")
        .execute(&proxy.key_store.pool)
        .await
        .unwrap();

    assert_eq!(
        proxy
            .key_store
            .backfill_upstream_reconciliation_work_page()
            .await
            .unwrap(),
        256
    );
    assert_eq!(
        proxy
            .key_store
            .backfill_upstream_reconciliation_work_page()
            .await
            .unwrap(),
        1
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM upstream_reconciliation_work")
        .fetch_one(&proxy.key_store.pool)
        .await
        .unwrap();
    assert_eq!(count, 257);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_candidate_claim_defers_within_sqlite_budget_when_writer_is_busy() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(1_700_000_000);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-work-budget"],
        "http://127.0.0.1:9",
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .unwrap();

    let mut lock = proxy.key_store.pool.acquire().await.unwrap();
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let started_at = std::time::Instant::now();
    let result = proxy
        .key_store
        .next_upstream_reconciliation_candidates(1)
        .await;
    assert!(result.is_err());
    assert!(started_at.elapsed() < std::time::Duration::from_millis(750));
    sqlx::query("ROLLBACK").execute(&mut *lock).await.unwrap();

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
        reservation_id: None,
        scheduling_key_id: String::new(),
        fair_rank: 0,
        hydration_cursor_key_id: None,
        upstream_usage_total: 0,
        hydration_complete: false,
    };
    assert!(
        proxy
            .key_store
            .settle_upstream_reconciliation(&candidate, 7, 10)
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
    assert_eq!(progress.observed_accounts, 3);
    assert_eq!(progress.accounts_with_settled_period, 1);
    assert_eq!(progress.fully_terminal_accounts, 2);
    assert_eq!(progress.observed_periods, 3);
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

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
            .settle_upstream_reconciliation_shadow(&candidate, 7, 7)
            .await
            .expect("shadow zero-delta settlement")
    );

    let usage = proxy
        .shadow_daily_reconciled_usage_for_accounts(&["user-shadow-zero".to_string()])
        .await
        .expect("read zero-delta shadow usage");
    assert_eq!(usage.get("user-shadow-zero"), Some(&0));

    let (_, last_shadow_adjustment_at, _) = proxy
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

    let app = Router::new().route(
        "/usage",
        get(|| async {
            Json(serde_json::json!({
                "key": { "usage": 0 }
            }))
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
    let (last_run_at, last_shadow_adjustment_at, _) = proxy
        .key_store
        .upstream_reconciliation_runtime_markers()
        .await
        .expect("read runtime markers");
    assert_eq!(last_run_at, Some(now));
    assert_eq!(last_shadow_adjustment_at, Some(now));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn run_upstream_reconciliation_once_applies_key_scoped_backoff_for_429() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
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
                        [("retry-after", "300")],
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
    assert_eq!(hot_rate_limited, 2);

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
async fn queue_counts_include_due_unsettled_usage_windows() {
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

    let (pending_research, queued, degraded) = proxy
        .key_store
        .upstream_reconciliation_queue_counts()
        .await
        .expect("read queue counts");
    assert_eq!(pending_research, 1);
    assert_eq!(queued, 1);
    assert_eq!(degraded, 0);

    let _ = std::fs::remove_file(db_path);
}

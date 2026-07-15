use super::*;
use chrono::{Local, LocalResult, TimeZone};

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

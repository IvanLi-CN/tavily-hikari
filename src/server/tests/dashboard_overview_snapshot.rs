use super::*;
use super::core_support_and_parsing::*;
use super::linuxdo_oauth_and_admin_keys::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn compute_signatures_reuses_dashboard_boundary_contract() {
    let db_path = temp_db_path("summary-signatures-dashboard-boundaries");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-signature-boundaries".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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

    let snapshot = load_dashboard_overview_snapshot(&state)
        .await
        .expect("overview snapshot");
    let (sig, latest_id) = compute_signatures(&state)
        .await
        .expect("compute signatures");
    let sig = sig.expect("summary signature");

    assert_eq!(
        sig.freshness.summary_window_starts,
        snapshot.freshness.summary_window_starts,
        "SSE freshness probe should reuse the same cheap local day/month boundary contract as the cached overview snapshot",
    );
    assert_eq!(
        sig.freshness.latest_request_log_id,
        snapshot.freshness.latest_request_log_id,
        "SSE freshness probe should stay aligned with the retention-filtered request-log visibility contract",
    );
    assert_eq!(
        sig.freshness.recent_request_logs,
        snapshot.freshness.recent_request_logs,
        "SSE freshness probe should track the same displayed recent-log signature as the shared overview snapshot",
    );
    assert_eq!(
        sig.freshness.trend_request_logs,
        snapshot.freshness.trend_request_logs,
        "SSE freshness probe should also track the full trend source window used by the shared overview snapshot",
    );
    assert_eq!(
        sig.freshness.pending_dashboard_rollup_signature,
        snapshot.freshness.pending_dashboard_rollup_signature,
        "SSE freshness should inherit the same pending dashboard rollup signature that the rebuilt snapshot stores",
    );
    assert_eq!(latest_id, snapshot.freshness.latest_request_log_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_is_reused_within_the_same_freshness_wave() {
    let db_path = temp_db_path("dashboard-overview-shared-snapshot");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-shared-snapshot".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");

    reset_dashboard_overview_build_count(&state).await;

    let (_snapshot_event, emitted_sig) = build_snapshot_event(&state)
        .await
        .expect("snapshot event");

    assert_eq!(
        dashboard_overview_build_count(&state).await,
        0,
        "SSE snapshot should reuse the shared overview cache instead of rebuilding within the same refresh wave",
    );
    assert!(
        !first.payload.month_series.current.is_empty(),
        "snapshot should still expose the expected month series payload",
    );
    assert_eq!(
        emitted_sig.freshness,
        first.freshness,
        "SSE should remember the emitted snapshot freshness instead of the pre-rebuild probe state",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_recovers_from_stale_loading_flag() {
    let db_path = temp_db_path("dashboard-overview-stale-loading-flag");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-stale-loading-flag".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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

    {
        let cache_handle = dashboard_overview_cache_for_state(state.as_ref());
        let mut cache = cache_handle.lock().await;
        cache.loading = true;
        cache.loading_started_at =
            Some(tokio::time::Instant::now() - DASHBOARD_OVERVIEW_LOADING_STALE_AFTER - Duration::from_secs(1));
    }

    let snapshot = tokio::time::timeout(
        Duration::from_secs(5),
        load_dashboard_overview_snapshot(&state),
    )
    .await
    .expect("stale loading flag should not block forever")
    .expect("overview snapshot should rebuild after stale loading flag");

    assert!(
        !snapshot.payload.month_series.current.is_empty(),
        "recovered overview should still include normal dashboard data",
    );
    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "stale loading recovery should take over and rebuild the shared snapshot",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_load_guard_does_not_clear_newer_loader_generation() {
    let cache_handle = new_dashboard_overview_cache();
    {
        let mut cache = cache_handle.lock().await;
        cache.loading = true;
        cache.loading_generation = 1;
        cache.loading_started_at = Some(tokio::time::Instant::now());
    }

    let guard = DashboardOverviewLoadGuard::new(cache_handle.clone(), 1);

    {
        let mut cache = cache_handle.lock().await;
        cache.loading = true;
        cache.loading_generation = 2;
        cache.loading_started_at = Some(tokio::time::Instant::now());
    }

    drop(guard);
    tokio::time::sleep(Duration::from_millis(25)).await;

    let cache = cache_handle.lock().await;
    assert!(cache.loading, "old loader guard must not clear a newer loader");
    assert_eq!(
        cache.loading_generation, 2,
        "old loader guard must preserve the newer loader generation",
    );
    assert!(
        cache.loading_started_at.is_some(),
        "old loader guard must not erase newer loader start time",
    );
}

#[tokio::test]
async fn dashboard_overview_freshness_advances_on_five_minute_window_anchor() {
    let first_anchor = dashboard_hourly_window_anchor(1_774_070_520);
    let second_anchor = dashboard_hourly_window_anchor(1_774_070_700);

    assert_eq!(first_anchor, 1_774_070_400);
    assert_eq!(second_anchor, 1_774_070_700);
    assert_ne!(
        second_anchor, first_anchor,
        "dashboard overview cache must advance with the 5 minute realtime window, not wait for the next hour",
    );
    assert_eq!(
        dashboard_hourly_window_anchor(1_774_073_999),
        1_774_073_700,
        "freshness anchor should stay on five minute boundaries within the same hour",
    );
}

#[tokio::test]
async fn dashboard_snapshot_event_uses_rebuilt_freshness_after_pending_rollups() {
    let db_path = temp_db_path("dashboard-overview-emitted-freshness");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-emitted-freshness".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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

    let initial = load_dashboard_overview_snapshot(&state)
        .await
        .expect("initial overview snapshot");
    reset_dashboard_overview_build_count(&state).await;

    let created_at = Utc::now().timestamp();
    state
        .proxy
        .debug_enqueue_dashboard_credit_rollups(created_at, 7)
        .await;

    let (probe_sig, _) = compute_signatures(&state)
        .await
        .expect("compute signatures after pending rollup");
    let probe_sig = probe_sig.expect("summary signature after pending rollup");
    assert_ne!(
        probe_sig.freshness.pending_dashboard_rollup_signature,
        initial.freshness.pending_dashboard_rollup_signature,
        "cheap freshness should notice pending rollup work before the shared snapshot is rebuilt",
    );

    let (_event, emitted_sig) = build_snapshot_event(&state)
        .await
        .expect("snapshot event after pending rollup");

    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "pending rollup drift should trigger a shared snapshot rebuild",
    );
    assert_ne!(
        emitted_sig.freshness.pending_dashboard_rollup_signature,
        probe_sig.freshness.pending_dashboard_rollup_signature,
        "emitted snapshot freshness should reflect the rebuilt post-flush state, not the stale pre-rebuild probe",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_snapshot_event_emits_latest_log_cursor_from_rebuilt_snapshot() {
    let db_path = temp_db_path("dashboard-overview-emitted-log-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-emitted-log-cursor".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;

    let (probe_sig, stale_latest_id) = compute_signatures(&state)
        .await
        .expect("compute signatures before new log");
    let probe_sig = probe_sig.expect("summary signature before new log");
    assert_eq!(
        stale_latest_id, None,
        "fresh probe should start without a visible request-log cursor",
    );

    let new_created_at = Utc::now().timestamp();
    let new_log_id = sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_body, response_body, forwarded_headers,
            dropped_headers, created_at
        ) VALUES (NULL, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
        "#,
    )
    .bind(new_created_at)
    .execute(&pool)
    .await
    .expect("insert new visible request log")
    .last_insert_rowid();

    let (_event, emitted_sig) = build_snapshot_event(&state)
        .await
        .expect("snapshot event after new log");

    assert_eq!(
        emitted_sig.freshness.latest_request_log_id,
        Some(new_log_id),
        "emitted snapshot freshness should carry the rebuilt latest visible request-log id",
    );
    assert_ne!(
        stale_latest_id,
        emitted_sig.freshness.latest_request_log_id,
        "probe cursor should be allowed to go stale while the snapshot rebuild catches up",
    );

    let (next_sig, next_latest_id) = compute_signatures(&state)
        .await
        .expect("compute signatures after snapshot emit");
    let next_sig = next_sig.expect("summary signature after snapshot emit");
    assert_eq!(
        next_sig,
        emitted_sig,
        "the next SSE probe should match the freshness that was just emitted",
    );
    assert_eq!(
        next_latest_id,
        emitted_sig.freshness.latest_request_log_id,
        "SSE cursor should advance to the emitted snapshot freshness to avoid duplicate snapshots on the next poll",
    );
    assert_ne!(
        probe_sig.freshness.latest_request_log_id,
        emitted_sig.freshness.latest_request_log_id,
        "the regression only shows up when a request log lands after the probe but before snapshot emission",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_ignores_retained_out_request_logs_in_freshness_probe() {
    let db_path = temp_db_path("dashboard-overview-retained-log-freshness");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-retained-log-freshness".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    assert!(
        first.freshness.latest_request_log_id.is_none(),
        "fresh snapshot should start without recent request logs",
    );

    let retention_days = effective_request_logs_retention_days();
    let retained_out_created_at = Utc::now()
        .checked_sub_signed(ChronoDuration::days(retention_days + 1))
        .expect("retained-out timestamp")
        .timestamp();
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_body, response_body, forwarded_headers,
            dropped_headers, created_at
        ) VALUES (NULL, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
        "#,
    )
    .bind(retained_out_created_at)
    .execute(&pool)
    .await
    .expect("insert retained-out request log");

    reset_dashboard_overview_build_count(&state).await;

    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");

    assert_eq!(
        dashboard_overview_build_count(&state).await,
        0,
        "retained-out request logs should not make the shared snapshot freshness diverge and force rebuilds",
    );
    assert_eq!(
        second.freshness.latest_request_log_id, None,
        "freshness probe should stay aligned with retention-filtered payload semantics",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_freshness_notices_same_second_rollup_updates() {
    let db_path = temp_db_path("dashboard-overview-rollup-same-second-update");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-rollup-same-second-update".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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

    let initial = load_dashboard_overview_snapshot(&state)
        .await
        .expect("initial overview snapshot");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let summary_windows = state.proxy.summary_windows().await.expect("summary windows");
    let bucket_start = summary_windows.today_start;
    let updated_at = Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            valuable_success_count,
            valuable_failure_count,
            valuable_failure_429_count,
            other_success_count,
            other_failure_count,
            unknown_count,
            mcp_non_billable,
            mcp_billable,
            api_non_billable,
            api_billable,
            local_estimated_credits,
            updated_at
        ) VALUES (?, 86400, 2, 2, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, ?)
        ON CONFLICT(bucket_start, bucket_secs) DO UPDATE SET
            total_requests = excluded.total_requests,
            success_count = excluded.success_count,
            error_count = excluded.error_count,
            quota_exhausted_count = excluded.quota_exhausted_count,
            valuable_success_count = excluded.valuable_success_count,
            valuable_failure_count = excluded.valuable_failure_count,
            valuable_failure_429_count = excluded.valuable_failure_429_count,
            other_success_count = excluded.other_success_count,
            other_failure_count = excluded.other_failure_count,
            unknown_count = excluded.unknown_count,
            mcp_non_billable = excluded.mcp_non_billable,
            mcp_billable = excluded.mcp_billable,
            api_non_billable = excluded.api_non_billable,
            api_billable = excluded.api_billable,
            local_estimated_credits = excluded.local_estimated_credits,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(bucket_start)
    .bind(updated_at)
    .execute(&pool)
    .await
    .expect("upsert initial rollup row");

    let after_insert = load_dashboard_overview_snapshot(&state)
        .await
        .expect("overview snapshot after insert");
    assert_ne!(
        after_insert.freshness.dashboard_rollup_signature,
        initial.freshness.dashboard_rollup_signature,
        "adding a rollup bucket should invalidate the cached overview freshness signature",
    );

    sqlx::query(
        r#"
        UPDATE dashboard_request_rollup_buckets
        SET total_requests = 2,
            success_count = 2,
            error_count = 0,
            valuable_success_count = 0,
            valuable_failure_count = 0,
            other_success_count = 2,
            api_billable = 0,
            api_non_billable = 2,
            updated_at = ?
        WHERE bucket_start = ?
          AND bucket_secs = 86400
        "#,
    )
    .bind(updated_at)
    .bind(bucket_start)
    .execute(&pool)
    .await
    .expect("update same bucket within same second");

    let after_update = load_dashboard_overview_snapshot(&state)
        .await
        .expect("overview snapshot after update");
    assert_ne!(
        after_update.freshness.dashboard_rollup_signature,
        after_insert.freshness.dashboard_rollup_signature,
        "same-second rollup classification changes should still invalidate the cached overview freshness signature even when coarse totals stay the same",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_freshness_tracks_time_driven_stale_key_transitions() {
    let db_path = temp_db_path("dashboard-overview-stale-key-transition");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-stale-key-transition".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let now_ts = Utc::now().timestamp();
    sqlx::query("UPDATE api_keys SET last_used_at = ?, quota_synced_at = ? WHERE id = ?")
        .bind(now_ts - 10 * 60)
        .bind(now_ts - 15 * 60 + 1)
        .bind(&key_id)
        .execute(&pool)
        .await
        .expect("seed near-stale key");

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    assert_eq!(
        first.payload.summary_windows.today.quota_charge.stale_key_count,
        0,
        "fresh key should not be counted stale before the 15 minute threshold",
    );

    tokio::time::sleep(Duration::from_secs(2)).await;
    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");
    assert_eq!(
        second.payload.summary_windows.today.quota_charge.stale_key_count,
        1,
        "crossing the stale threshold should update the quota stale-key count without requiring a new sample row",
    );
    assert_ne!(
        second.freshness.dashboard_stale_key_count,
        first.freshness.dashboard_stale_key_count,
        "cheap freshness should include time-driven stale-key transitions",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_rebuilds_when_displayed_log_signature_changes() {
    let db_path = temp_db_path("dashboard-overview-log-order-freshness");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-log-order-freshness".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;

    let newest_created_at = Utc::now().timestamp();
    let newest_id = sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_body, response_body, forwarded_headers,
            dropped_headers, created_at
        ) VALUES (NULL, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
        "#,
    )
    .bind(newest_created_at)
    .execute(&pool)
    .await
    .expect("insert newest request log")
    .last_insert_rowid();
    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    assert_eq!(
        first.payload.recent_logs.first().map(|log| log.id),
        Some(newest_id),
        "dashboard payload should keep the most recent log first by created_at/id ordering",
    );
    assert_eq!(
        first.freshness.latest_request_log_id,
        Some(newest_id),
        "snapshot freshness should track the same most recent visible log as the retention-filtered payload",
    );
    assert_eq!(
        first.freshness.recent_request_logs,
        vec![(newest_id, newest_created_at)],
        "freshness should include the displayed recent-log signature",
    );

    reset_dashboard_overview_build_count(&state).await;

    let older_id = sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_body, response_body, forwarded_headers,
            dropped_headers, created_at
        ) VALUES (NULL, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
        "#,
    )
    .bind(newest_created_at - 3600)
    .execute(&pool)
    .await
    .expect("insert older request log")
    .last_insert_rowid();
    assert!(
        older_id > newest_id,
        "second insert should get a higher id while remaining older by created_at",
    );

    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");

    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "displayed recent-log changes should rebuild the shared snapshot even when the newest log id is unchanged",
    );
    assert_eq!(second.freshness.latest_request_log_id, Some(newest_id));
    assert_eq!(
        second.freshness.recent_request_logs,
        vec![(newest_id, newest_created_at), (older_id, newest_created_at - 3600)],
        "freshness should stay aligned with the displayed recent-log ordering",
    );
    assert_eq!(
        second
            .payload
            .recent_logs
            .iter()
            .map(|log| log.id)
            .collect::<Vec<_>>(),
        vec![newest_id, older_id],
        "rebuilding should refresh the displayed recent-log rows after an older retained log is inserted",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_rebuilds_when_trend_only_log_window_changes() {
    let db_path = temp_db_path("dashboard-overview-trend-log-freshness");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-trend-log-freshness".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;
    let base_created_at = Utc::now().timestamp();

    for offset in 0..DASHBOARD_TREND_SOURCE_LIMIT {
        sqlx::query(
            r#"
            INSERT INTO observability.request_logs (
                api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
                error_message, result_status, request_body, response_body, forwarded_headers,
                dropped_headers, created_at
            ) VALUES (NULL, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
            "#,
        )
        .bind(base_created_at - offset as i64)
        .execute(&pool)
        .await
        .expect("seed trend request log");
    }

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    let first_recent_ids = first
        .payload
        .recent_logs
        .iter()
        .map(|log| log.id)
        .collect::<Vec<_>>();
    let first_trend_error = first.payload.trend.error.clone();
    let first_trend_signature = first.freshness.trend_request_logs.clone();

    reset_dashboard_overview_build_count(&state).await;

    let extra_created_at = base_created_at - DASHBOARD_RECENT_LOGS_LIMIT as i64 - 1;
    let extra_id = sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
            error_message, result_status, request_body, response_body, forwarded_headers,
            dropped_headers, created_at
        ) VALUES (NULL, NULL, 'POST', '/search', NULL, 500, 500, NULL, 'error', NULL, NULL, '', '', ?)
        "#,
    )
    .bind(extra_created_at)
    .execute(&pool)
    .await
    .expect("insert trend-only request log")
    .last_insert_rowid();

    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");

    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "trend-only freshness changes should rebuild the shared snapshot",
    );
    assert_eq!(
        second
            .payload
            .recent_logs
            .iter()
            .map(|log| log.id)
            .collect::<Vec<_>>(),
        first_recent_ids,
        "logs outside the displayed top-five window should not disturb the displayed recent-log list",
    );
    assert_ne!(
        second.freshness.trend_request_logs,
        first_trend_signature,
        "the cached freshness should include the full trend source window, not only displayed logs",
    );
    assert_ne!(
        second.payload.trend.error,
        first_trend_error,
        "trend data should refresh when a retained error log enters the trend window outside the displayed top-five list",
    );
    assert!(
        second
            .freshness
            .trend_request_logs
            .iter()
            .any(|(id, created_at)| *id == extra_id && *created_at == extra_created_at),
        "trend freshness signature should capture the new retained trend-only log",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_does_not_reuse_recent_cache_after_freshness_changes() {
    let db_path = temp_db_path("dashboard-overview-shared-snapshot-freshness-change");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-shared-snapshot-freshness-change".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    let _ = first;

    reset_dashboard_overview_build_count(&state).await;

    sqlx::query(
        "UPDATE api_keys SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ?",
    )
    .bind(2_000_i64)
    .bind(1_234_i64)
    .bind(Utc::now().timestamp())
    .execute(&pool)
    .await
    .expect("update quota totals");

    let refreshed = load_dashboard_overview_snapshot(&state)
        .await
        .expect("refreshed overview snapshot");

    assert_eq!(
        refreshed.payload.site_status.remaining_quota,
        1_234,
        "freshness changes should bypass the recently loaded cache entry"
    );
    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "overview snapshot should rebuild after freshness changes even inside the grace window"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_rebuilds_when_previous_month_lifecycle_changes() {
    let db_path = temp_db_path("dashboard-overview-previous-month-lifecycle");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-previous-month-lifecycle".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;
    let summary_windows = proxy.summary_windows().await.expect("summary windows");

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    let first_new_quarantines = first
        .payload
        .month_series
        .comparison
        .first()
        .and_then(|point| point.new_quarantines);

    reset_dashboard_overview_build_count(&state).await;

    sqlx::query(
        r#"
        INSERT INTO api_key_quarantines (
            id,
            key_id,
            source,
            reason_code,
            reason_summary,
            reason_detail,
            created_at
        ) VALUES (?, ?, 'system', 'test_previous_month_refresh', 'test previous month refresh', 'test previous month refresh detail', ?)
        "#,
    )
    .bind("quarantine-previous-month-refresh")
    .bind(
        proxy
            .list_api_key_metrics()
            .await
            .expect("key metrics")
            .into_iter()
            .next()
            .expect("seeded key")
            .id,
    )
    .bind(summary_windows.previous_month_start + 120)
    .execute(&pool)
    .await
    .expect("insert previous month quarantine");

    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");

    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "previous-month lifecycle changes should rebuild the shared snapshot",
    );
    assert_ne!(
        second
            .payload
            .month_series
            .comparison
            .first()
            .and_then(|point| point.new_quarantines),
        first_new_quarantines,
        "comparison month series should refresh after previous-month lifecycle changes",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_rebuilds_when_month_quota_samples_backfill() {
    let db_path = temp_db_path("dashboard-overview-month-quota-backfill");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-month-quota-backfill".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;
    let summary_windows = proxy.summary_windows().await.expect("summary windows");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    let first_latest_sync = first
        .payload
        .summary_windows
        .month
        .quota_charge
        .latest_sync_at;
    let month_quota_sample_start = summary_windows
        .month_start
        .max(start_of_month_dt(Utc::now()).timestamp());

    reset_dashboard_overview_build_count(&state).await;

    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (
            key_id,
            quota_limit,
            quota_remaining,
            captured_at,
            source
        ) VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&key_id)
    .bind(2_000_i64)
    .bind(1_111_i64)
    .bind(month_quota_sample_start + 90)
    .bind("ha_backfill")
    .execute(&pool)
    .await
    .expect("insert month quota sample backfill");

    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");

    assert!(
        dashboard_overview_build_count(&state).await >= 1,
        "month quota sample backfills should rebuild the shared snapshot",
    );
    assert_ne!(
        second.payload.summary_windows.month.quota_charge.latest_sync_at,
        first_latest_sync,
        "month quota charge window should refresh after a retained backfill sample arrives",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_overview_snapshot_ignores_quota_baseline_backfills_for_cheap_freshness() {
    let db_path = temp_db_path("dashboard-overview-quota-baseline-backfill");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-overview-quota-baseline-backfill".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

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
    let pool = connect_sqlite_test_pool(&db_str).await;
    let summary_windows = proxy.summary_windows().await.expect("summary windows");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let quota_sample_window_start = summary_windows
        .yesterday_start
        .min(start_of_month_dt(Utc::now()).timestamp());
    let window_sample_at = quota_sample_window_start + 120;

    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (
            key_id,
            quota_limit,
            quota_remaining,
            captured_at,
            source
        ) VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&key_id)
    .bind(2_000_i64)
    .bind(1_000_i64)
    .bind(window_sample_at)
    .bind("window_sample")
    .execute(&pool)
    .await
    .expect("insert window sample");

    let first = load_dashboard_overview_snapshot(&state)
        .await
        .expect("first overview snapshot");
    let first_upstream_actual = first
        .payload
        .summary_windows
        .month
        .quota_charge
        .upstream_actual_credits;
    let first_latest_sync = first
        .payload
        .summary_windows
        .month
        .quota_charge
        .latest_sync_at;
    let first_quota_signature = first.freshness.dashboard_quota_charge_token;

    reset_dashboard_overview_build_count(&state).await;

    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (
            key_id,
            quota_limit,
            quota_remaining,
            captured_at,
            source
        ) VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(&key_id)
    .bind(2_000_i64)
    .bind(1_400_i64)
    .bind(quota_sample_window_start - 60)
    .bind("baseline_backfill")
    .execute(&pool)
    .await
    .expect("insert baseline backfill sample");

    let second = load_dashboard_overview_snapshot(&state)
        .await
        .expect("second overview snapshot");

    assert_eq!(
        dashboard_overview_build_count(&state).await,
        0,
        "baseline quota sample backfills should not invalidate the cheap freshness contract",
    );
    assert_eq!(
        second.freshness.dashboard_quota_charge_token,
        first_quota_signature,
        "cheap quota token should stay stable when only pre-window baseline rows change",
    );
    assert_eq!(
        second.payload.summary_windows.month.quota_charge.latest_sync_at,
        first_latest_sync,
        "baseline backfills should not need a newer in-window sample timestamp to keep cache hit stable",
    );
    assert_eq!(
        second.payload.summary_windows.month.quota_charge.upstream_actual_credits,
        first_upstream_actual,
        "core overview should serve the cached quota slice until the cheap token changes",
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_dashboard_sse_snapshot_refreshes_when_quota_totals_change() {
    let db_path = temp_db_path("admin-dashboard-snapshot-quota-change");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-dashboard-quota".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;

    let admin_password = "admin-dashboard-quota-password";
    let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("build client");

    let login_resp = client
        .post(format!("http://{}/api/admin/login", admin_addr))
        .json(&serde_json::json!({ "password": admin_password }))
        .send()
        .await
        .expect("admin login");
    assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
    let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
        .expect("admin session cookie");

    let mut events_resp = client
        .get(format!("http://{}/api/events", admin_addr))
        .header(reqwest::header::COOKIE, admin_cookie)
        .send()
        .await
        .expect("admin events request");
    assert_eq!(events_resp.status(), reqwest::StatusCode::OK);

    let initial_snapshot = read_sse_event_until(
        &mut events_resp,
        |chunk| chunk.contains("event: snapshot"),
        "initial admin snapshot event",
    )
    .await;
    let initial_data = initial_snapshot
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("initial snapshot data");
    let initial_json: serde_json::Value =
        serde_json::from_str(initial_data).expect("initial snapshot payload json");
    assert_eq!(
        initial_json
            .pointer("/siteStatus/remainingQuota")
            .and_then(|value| value.as_i64()),
        Some(0)
    );

    let options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open db pool");

    sqlx::query(
        "UPDATE api_keys SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ? WHERE id = ?",
    )
    .bind(2_000_i64)
    .bind(1_234_i64)
    .bind(Utc::now().timestamp())
    .bind(&key_id)
    .execute(&pool)
    .await
    .expect("update quota totals");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    let mut buffer = String::new();
    let mut refreshed_snapshot: Option<serde_json::Value> = None;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let chunk = tokio::time::timeout(remaining, events_resp.chunk())
            .await
            .expect("await refreshed event chunk in time")
            .expect("read refreshed event chunk")
            .expect("refreshed event chunk exists");
        buffer.push_str(std::str::from_utf8(&chunk).expect("refreshed event chunk utf8"));
        while let Some((event_chunk, rest)) = buffer.split_once("\n\n") {
            let event_chunk = event_chunk.to_string();
            buffer = rest.to_string();
            if !event_chunk.contains("event: snapshot") {
                continue;
            }
            let Some(data) = event_chunk
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
            else {
                continue;
            };
            let payload: serde_json::Value =
                serde_json::from_str(data).expect("refreshed snapshot payload json");
            if payload
                .pointer("/siteStatus/remainingQuota")
                .and_then(|value| value.as_i64())
                == Some(1_234)
            {
                refreshed_snapshot = Some(payload);
                break;
            }
        }
        if refreshed_snapshot.is_some() {
            break;
        }
    }

    let refreshed_snapshot = refreshed_snapshot.expect("quota snapshot refresh");
    assert_eq!(
        refreshed_snapshot
            .pointer("/siteStatus/remainingQuota")
            .and_then(|value| value.as_i64()),
        Some(1_234)
    );
    assert_eq!(
        refreshed_snapshot
            .pointer("/siteStatus/totalQuotaLimit")
            .and_then(|value| value.as_i64()),
        Some(2_000)
    );

    drop(events_resp);
    let _ = std::fs::remove_file(db_path);
}

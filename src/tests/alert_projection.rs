use super::*;

async fn insert_projected_rate_limit_alert(proxy: &TavilyProxy, token_id: &str, created_at: i64) {
    sqlx::query(
        "INSERT OR IGNORE INTO users (id, display_name, username, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("projection-user")
    .bind("Projection User")
    .bind("projection-user")
    .bind(created_at)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert projection user");
    sqlx::query("INSERT INTO auth_tokens (id, secret, created_at) VALUES (?, ?, ?)")
        .bind(token_id)
        .bind(format!("secret-{token_id}"))
        .bind(created_at)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert projection token");
    sqlx::query(
        "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind("projection-user")
    .bind(token_id)
    .bind(created_at)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("bind projection token");
    sqlx::query(
        r#"INSERT INTO auth_token_logs (
             token_id, method, path, request_kind_key, request_kind_label,
             request_kind_detail, result_status, error_message, key_effect_code,
             binding_effect_code, selection_effect_code, counts_business_quota, created_at
           ) VALUES (?, 'POST', '/mcp', 'mcp_call', 'MCP call', 'MCP call',
                     'quota_exhausted', 'user request rate limit exceeded on rolling 5m window (limit 25, used 25)',
                     'none', 'none', 'none', 0, ?)"#,
    )
    .bind(token_id)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert projection alert");
}

async fn advance_alert_projection_until(proxy: &TavilyProxy, projected_events: i64) {
    let mut outcomes = Vec::new();
    for _ in 0..48 {
        let dashboard_dirty = proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("advance alert projection slice");
        outcomes.push(format!("dashboard_dirty={dashboard_dirty}"));
        if !dashboard_dirty {
            // Production retries a deferred slice on a later scheduler wake.
            // Keep this helper from spinning against a lazy pool in tests.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let status = proxy
            .dashboard_alert_projection_status()
            .await
            .expect("read projection coverage");
        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events",
        )
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count projected alert events");
        if event_count == projected_events && status.coverage == "ok" {
            return;
        }
    }
    let status = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read final projection coverage");
    let event_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM observability.dashboard_alert_projection_events")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count final projected alert events");
    panic!(
        "alert projection did not converge: expected {projected_events} events, got {event_count} ({}) after {outcomes:?}",
        status.coverage,
    );
}

async fn advance_alert_projection_slice_until_admitted(
    proxy: &TavilyProxy,
) -> AlertProjectionSliceOutcome {
    for _ in 0..48 {
        let outcome = proxy
            .key_store
            .advance_alert_projection_slice()
            .await
            .expect("advance alert projection slice");
        if !matches!(outcome, AlertProjectionSliceOutcome::Deferred { .. }) {
            return outcome;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("alert projection remained admission-deferred");
}

async fn refresh_alert_projection_observation_until_admitted(proxy: &TavilyProxy) {
    for _ in 0..48 {
        if proxy
            .refresh_dashboard_alert_projection_observation()
            .await
            .expect("refresh idle projection observation")
        {
            return;
        }
        // The maintenance admission policy can defer immediately after the
        // foreground state reads in this test.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("idle projection observation remained admission-deferred");
}

async fn advance_alert_projection_until_full_coverage(proxy: &TavilyProxy) {
    let mut outcomes = Vec::new();
    for _ in 0..48 {
        let outcome = advance_alert_projection_slice_until_admitted(proxy).await;
        outcomes.push(format!("{outcome:?}"));
        let status = proxy
            .dashboard_alert_projection_status()
            .await
            .expect("read projection coverage");
        if status.coverage == "ok" {
            return;
        }
    }
    let status = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read final projection coverage");
    panic!(
        "alert projection history did not converge: {} after {outcomes:?}",
        status.coverage
    );
}

async fn refresh_projected_recent_alerts_until_fresh(proxy: &TavilyProxy) -> RecentAlertsSummary {
    let mut last_summary = None;
    for _ in 0..48 {
        proxy
            .key_store
            .refresh_dashboard_alert_projection_summary()
            .await
            .expect("refresh materialized recent-alert summary");
        let summary = proxy
            .recent_alerts_summary(24)
            .await
            .expect("read materialized recent-alert summary");
        if !summary.stale {
            return summary;
        }
        last_summary = Some(summary);
        // Bulk admission may legitimately defer one materialization attempt.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "materialized recent-alert summary remained stale after bounded retries: {last_summary:?}"
    );
}

#[tokio::test]
async fn alert_projection_cursor_replays_tail_without_duplicates() {
    let db_path = temp_db_path("alert-projection-tail");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_500_000;
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-tail".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "projection-token-a", now).await;
    advance_alert_projection_until(&proxy, 1).await;
    clock.set_now_ts(now + 60);
    proxy
        .refresh_dashboard_alert_projection_observation()
        .await
        .expect("refresh initial projection observation before materializing the summary");
    let initial = refresh_projected_recent_alerts_until_fresh(&proxy).await;
    assert_eq!(initial.total_events, 1);
    assert_eq!(initial.coverage, "ok");

    drop(proxy);
    let (resumed_time, resumed_clock) = BackendTime::manual_from_ts(now + 61);
    let resumed = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-tail".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        resumed_time,
    )
    .await
    .expect("reopen proxy with projected alert cursor");
    insert_projected_rate_limit_alert(&resumed, "projection-token-b", now + 61).await;
    advance_alert_projection_until(&resumed, 2).await;
    resumed_clock.set_now_ts(now + 121);
    resumed
        .refresh_dashboard_alert_projection_observation()
        .await
        .expect("refresh projection observation before materializing the summary");
    let replayed = refresh_projected_recent_alerts_until_fresh(&resumed).await;
    assert_eq!(replayed.total_events, 2);
    assert!(
        !replayed.stale,
        "replayed summary must be fresh: {replayed:?}"
    );

    drop(resumed);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_materializes_empty_dashboard_summary_without_read_side_aggregation() {
    let db_path = temp_db_path("alert-projection-materialized-empty-summary");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_500_025;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-materialized-empty-summary".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    advance_alert_projection_until_full_coverage(&proxy).await;
    let materialized = refresh_projected_recent_alerts_until_fresh(&proxy).await;
    assert_eq!(materialized.total_events, 0);
    let cached_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_recent_summaries WHERE window_hours = 24",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read empty materialized summary row");
    assert_eq!(cached_rows, 1);

    // A missing raw source must not cause Dashboard reads to rebuild the CTE.
    sqlx::query("DROP TABLE auth_token_logs")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove unused raw alert source");
    let summary = proxy
        .dashboard_recent_alerts_summary(24)
        .await
        .expect("read empty materialized dashboard summary");
    assert_eq!(summary.total_events, 0);
    assert_eq!(summary.coverage, "ok");

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_summary_refresh_is_rate_limited_across_generation_changes() {
    let db_path = temp_db_path("alert-projection-summary-generation-throttle");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_500_040;
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-summary-generation-throttle".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "projection-throttle-first", now).await;
    advance_alert_projection_until(&proxy, 1).await;
    let first_computed_at: i64 = sqlx::query_scalar(
        "SELECT computed_at FROM observability.dashboard_alert_projection_recent_summaries WHERE window_hours = 24",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read initial summary timestamp");

    insert_projected_rate_limit_alert(&proxy, "projection-throttle-second", now + 1).await;
    advance_alert_projection_until(&proxy, 2).await;
    assert!(
        !proxy
            .key_store
            .refresh_dashboard_alert_projection_summary()
            .await
            .expect("do not refresh again inside the fixed window")
    );
    let computed_at: i64 = sqlx::query_scalar(
        "SELECT computed_at FROM observability.dashboard_alert_projection_recent_summaries WHERE window_hours = 24",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read throttled summary timestamp");
    assert_eq!(computed_at, first_computed_at);
    let stale = proxy
        .recent_alerts_summary(24)
        .await
        .expect("read stale materialized summary");
    assert!(stale.stale);
    assert_eq!(stale.coverage, "stale");
    assert_eq!(stale.error.as_deref(), Some("summary_refresh_pending"));

    clock.set_now_ts(now + 60);
    let refreshed = refresh_projected_recent_alerts_until_fresh(&proxy).await;
    assert_eq!(refreshed.total_events, 2);
    assert!(!refreshed.stale);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_materializes_dashboard_summary_before_dashboard_reads() {
    let db_path = temp_db_path("alert-projection-materialized-summary");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_500_050;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-materialized-summary".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "projection-summary-token", now).await;
    for _ in 0..6 {
        let outcome = proxy
            .key_store
            .advance_alert_projection_slice()
            .await
            .expect("advance exact-boundary projection slice");
        if matches!(outcome, AlertProjectionSliceOutcome::Deferred { .. }) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    let materialized = refresh_projected_recent_alerts_until_fresh(&proxy).await;
    assert_eq!(materialized.total_events, 1);
    let cached_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_recent_summaries WHERE window_hours = 24",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read materialized summary row");
    assert_eq!(cached_rows, 1);

    // Dashboard reads must not rebuild the alert CTE. The source table is not
    // needed once the tail is idle and the summary has been materialized.
    sqlx::query("DROP TABLE auth_token_logs")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove raw alert source after projection is idle");
    let summary = proxy
        .dashboard_recent_alerts_summary(24)
        .await
        .expect("read materialized dashboard summary");
    assert_eq!(summary.total_events, 1);
    assert_eq!(summary.coverage, "ok");

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_serves_admin_events_and_groups_after_full_coverage() {
    let db_path = temp_db_path("alert-projection-admin-read-model");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_550_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-admin-read-model".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "projection-admin-token", now).await;
    advance_alert_projection_until(&proxy, 1).await;
    advance_alert_projection_until_full_coverage(&proxy).await;
    let status = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read complete projection status");
    assert_eq!(status.coverage, "ok");

    // A complete projection is the administrator read source. Removing the
    // original alert source makes this regression fail if events or groups
    // accidentally rebuild their CTE from auth_token_logs.
    sqlx::query("DROP TABLE auth_token_logs")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove raw alert source after projection completed");

    let events = proxy
        .alert_events_page(None, None, None, None, None, None, &[], 1, 20)
        .await
        .expect("read projected administrator events");
    assert_eq!(events.total, 1);
    assert_eq!(events.items.len(), 1);
    assert_eq!(events.items[0].source.kind, ALERT_SOURCE_AUTH_TOKEN_LOG);

    let catalog = proxy
        .alert_catalog()
        .await
        .expect("read projected administrator alert catalog");
    assert_eq!(catalog.types.iter().map(|item| item.count).sum::<i64>(), 1);
    assert_eq!(catalog.tokens.len(), 1);

    let groups = proxy
        .alert_groups_page(None, None, None, None, None, None, &[], 1, 20)
        .await
        .expect("read projected administrator groups");
    assert_eq!(groups.total, 1);
    assert_eq!(groups.items.len(), 1);
    assert_eq!(
        groups.items[0].latest_event.source.kind,
        ALERT_SOURCE_AUTH_TOKEN_LOG
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_alerts_canonical_groups_model_is_generation_fenced() {
    let db_path = temp_db_path("alert-canonical-groups-generation-fence");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_555_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-canonical-groups-generation-fence".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "canonical-groups-token", now).await;
    advance_alert_projection_until(&proxy, 1).await;
    advance_alert_projection_until_full_coverage(&proxy).await;

    let expected = proxy
        .key_store
        .fetch_admin_alert_groups_page_for_operation(
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            1,
            20,
            SqliteOperation::AdminAlertsRead,
        )
        .await
        .expect("read existing default Groups semantics");
    let groups = proxy
        .key_store
        .fetch_admin_alert_groups_page_for_operation(
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            1,
            20,
            SqliteOperation::AdminAlertsCacheWarm,
        )
        .await
        .expect("build canonical groups model from complete projection");
    assert_eq!(groups.total, 1);
    assert_eq!(groups.items.len(), 1);
    assert_eq!(
        groups, expected,
        "the canonical model preserves Groups semantics"
    );
    let state: (i64, i64, i64) = sqlx::query_as(
        "SELECT active_generation, source_recent_generation, source_history_generation \
         FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read canonical groups state");
    assert!(state.0 > 0, "the complete Groups model must be published");
    assert_eq!(
        (state.1, state.2),
        proxy
            .admin_alerts_canonical_warm_projection_fence()
            .await
            .expect("read source fence"),
        "a published Groups generation must record the exact projection fence"
    );

    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_history_state \
         SET generation = generation + 1",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("advance source generation");
    let rebuilt = proxy
        .key_store
        .fetch_admin_alert_groups_page_for_operation(
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            1,
            20,
            SqliteOperation::AdminAlertsCacheWarm,
        )
        .await
        .expect("rebuild canonical groups model after source generation change");
    assert_eq!(rebuilt.total, 1);
    let rebuilt_state: (i64, i64, i64) = sqlx::query_as(
        "SELECT active_generation, source_recent_generation, source_history_generation \
         FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rebuilt canonical groups state");
    assert!(rebuilt_state.0 > state.0);
    assert_eq!(
        (rebuilt_state.1, rebuilt_state.2),
        proxy
            .admin_alerts_canonical_warm_projection_fence()
            .await
            .expect("read rebuilt source fence")
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn admin_alerts_canonical_groups_model_reclaims_retired_generations_before_rebuild() {
    let db_path = temp_db_path("alert-canonical-groups-generation-reclaim");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_555_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-canonical-groups-generation-reclaim".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "canonical-groups-reclaim-token", now).await;
    advance_alert_projection_until(&proxy, 1).await;
    advance_alert_projection_until_full_coverage(&proxy).await;
    let active = proxy
        .key_store
        .fetch_admin_alert_groups_page_for_operation(
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            1,
            20,
            SqliteOperation::AdminAlertsCacheWarm,
        )
        .await
        .expect("publish an active canonical groups generation");
    assert_eq!(active.total, 1);
    let active_generation: i64 = sqlx::query_scalar(
        "SELECT active_generation FROM observability.admin_alert_canonical_groups_state \
         WHERE singleton = 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read active generation");

    // Model rows written before a source-fence rejection are indistinguishable from these
    // retired rows. More than two cleanup batches proves that a later warm tick keeps
    // reclaiming instead of publishing another generation over the stale staging data.
    for position in 1..=51_i64 {
        sqlx::query(
            r#"INSERT INTO observability.admin_alert_canonical_groups (
                   build_generation, position, last_seen, total_count,
                   alert_type, group_id, payload_json
               ) VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(active_generation + 1)
        .bind(position)
        .bind(now)
        .bind(1_i64)
        .bind("retired")
        .bind(format!("retired-{position}"))
        .bind("{}")
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed a retired staged generation");
    }

    for remaining_after_batch in [26_i64, 1_i64] {
        let error = proxy
            .key_store
            .fetch_admin_alert_groups_page_for_operation(
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                1,
                20,
                SqliteOperation::AdminAlertsCacheWarm,
            )
            .await
            .expect_err("retired rows must be reclaimed before another canonical read");
        assert!(matches!(
            error,
            ProxyError::Deferred { ref reason, .. } if reason == "groups_generation_reclaim_pending"
        ));
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.admin_alert_canonical_groups \
             WHERE build_generation != ?",
        )
        .bind(active_generation)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count remaining retired rows");
        assert_eq!(remaining, remaining_after_batch);
    }

    let reclaimed = proxy
        .key_store
        .fetch_admin_alert_groups_page_for_operation(
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            1,
            20,
            SqliteOperation::AdminAlertsCacheWarm,
        )
        .await
        .expect("serve the active model after retired rows are drained");
    assert_eq!(reclaimed, active);
    let retired_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.admin_alert_canonical_groups \
         WHERE build_generation != ?",
    )
    .bind(active_generation)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("confirm retired generations are gone");
    assert_eq!(retired_rows, 0);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_keeps_dashboard_tail_complete_while_history_catches_up() {
    let db_path = temp_db_path("alert-projection-independent-history");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_560_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-independent-history".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    let history_at = now - 60 * 24 * 60 * 60;
    for index in 0..26 {
        insert_projected_rate_limit_alert(
            &proxy,
            &format!("projection-history-{index}"),
            history_at,
        )
        .await;
    }
    insert_projected_rate_limit_alert(&proxy, "projection-recent", now).await;

    for _ in 0..12 {
        proxy
            .key_store
            .advance_alert_projection_slice()
            .await
            .expect("advance independent projection lane");
        let status = proxy
            .dashboard_alert_projection_status()
            .await
            .expect("read projection status");
        if status.recent_coverage == "ok" && status.coverage == "projecting" {
            // Dashboard materialization is a separately admitted bulk step.
            // A one-shot refresh may legitimately defer while the source tail
            // is already complete, so prove the worker retry converges instead
            // of treating that safe defer as a missing alert.
            let recent = refresh_projected_recent_alerts_until_fresh(&proxy).await;
            assert_eq!(recent.total_events, 1);
            assert!(!recent.stale);
            break;
        }
    }

    let status = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read final independent projection status");
    assert_eq!(status.recent_coverage, "ok");
    assert_eq!(status.coverage, "projecting");

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_does_not_offer_partial_tail_as_dashboard_data() {
    let db_path = temp_db_path("alert-projection-dashboard-partial");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_565_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-dashboard-partial".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(&proxy, "projection-unprojected", now).await;
    let raw_projection = proxy
        .recent_alerts_summary(24)
        .await
        .expect("inspect incomplete projection");
    assert!(raw_projection.stale);
    assert_eq!(raw_projection.coverage, "projecting");
    let (cold_dashboard_summary, _) = proxy
        .dashboard_recent_alerts_summary_for_cold_start_with_token(24)
        .await
        .expect("conservative cold Dashboard summary");
    assert!(cold_dashboard_summary.stale);
    assert_eq!(cold_dashboard_summary.total_events, 0);
    assert_eq!(cold_dashboard_summary.grouped_count, 0);
    assert!(
        proxy.dashboard_recent_alerts_summary(24).await.is_err(),
        "Dashboard must retain last-good data instead of accepting partial alerts"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_recent_summary_does_not_silently_truncate_events() {
    let db_path = temp_db_path("alert-projection-summary-exact-count");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_570_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-summary-exact-count".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    let payload = serde_json::json!({
        "source_kind": ALERT_SOURCE_AUTH_TOKEN_LOG,
        "source_id": "placeholder",
        "row_sort_id": "placeholder",
        "alert_type": ALERT_TYPE_UPSTREAM_RATE_LIMITED_429,
        "occurred_at": now,
        "token_id": "projection-summary-token",
        "key_id": null,
        "request_log_id": null,
        "method": null,
        "path": null,
        "query": null,
        "request_kind_key": null,
        "request_kind_label": null,
        "request_kind_detail": null,
        "result_status": null,
        "failure_kind": null,
        "error_message": null,
        "counts_business_quota": null,
        "user_id": null,
        "user_display_name": null,
        "user_username": null,
        "reason_code": null,
        "reason_summary": null,
        "reason_detail": null,
        "job_id": null,
        "job_type": null,
        "job_trigger_source": null,
        "job_status": null,
        "job_attempt": null,
        "job_message": null,
        "job_queued_at": null,
        "job_started_at": null,
        "job_finished_at": null,
    });
    let mut tx = proxy
        .key_store
        .pool
        .begin()
        .await
        .expect("begin sidecar seed");
    for index in 0..10_001_i64 {
        let source_id = format!("summary-{index}");
        let row_sort_id = format!("atl:{index:020}");
        let mut row = payload.clone();
        row["source_id"] = serde_json::Value::String(source_id.clone());
        row["row_sort_id"] = serde_json::Value::String(row_sort_id.clone());
        sqlx::query(
            r#"INSERT INTO observability.dashboard_alert_projection_events
                    (source_kind, source_id, occurred_at, row_sort_id, payload_json, projected_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(ALERT_SOURCE_AUTH_TOKEN_LOG)
        .bind(source_id)
        .bind(now)
        .bind(row_sort_id)
        .bind(row.to_string())
        .bind(now)
        .execute(&mut *tx)
        .await
        .expect("seed exact projected alert count");
    }
    tx.commit().await.expect("commit sidecar seed");
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_state \
         SET phase = 'idle', observed_at = ?, stale_reason = NULL",
    )
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark Dashboard tail complete");
    proxy
        .key_store
        .refresh_dashboard_alert_projection_summary()
        .await
        .expect("materialize exact derived summary");

    let summary = proxy
        .dashboard_recent_alerts_summary(24)
        .await
        .expect("read exact derived summary");
    assert_eq!(summary.total_events, 10_001);
    assert_eq!(
        summary
            .counts_by_type
            .iter()
            .find(|count| count.alert_type == ALERT_TYPE_UPSTREAM_RATE_LIMITED_429)
            .map(|count| count.count),
        Some(10_001)
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_admin_history_migration_preserves_sidecar_and_restarts_cursors() {
    let db_path = temp_db_path("alert-projection-admin-history-migration");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_575_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-admin-history-migration".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    let recent_window_start = now.saturating_sub(30 * 24 * 60 * 60);
    insert_projected_rate_limit_alert(&proxy, "projection-history-token", now).await;
    insert_projected_rate_limit_alert(
        &proxy,
        "projection-history-boundary-token",
        recent_window_start.saturating_sub(1),
    )
    .await;
    advance_alert_projection_until(&proxy, 1).await;
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_state \
         SET cursor_occurred_at = ?, cursor_row_sort_id = 'legacy-window', \
             phase = 'idle', observed_at = ?",
    )
    .bind(recent_window_start)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate a completed recent-window projection");
    sqlx::query("DELETE FROM schema_migrations WHERE version IN (14, 15)")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-admin-history ledger");
    sqlx::query("DROP TABLE observability.dashboard_alert_projection_history_state")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-admin-history sidecar");

    proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect("apply the cursor-only administrator history migration");

    let (preserved_tail_sources, reset_history_sources, retained_events): (i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM observability.dashboard_alert_projection_state \
             WHERE cursor_occurred_at = ? AND cursor_row_sort_id = 'legacy-window' AND phase = 'idle'), \
           (SELECT COUNT(*) FROM observability.dashboard_alert_projection_history_state \
             WHERE cursor_occurred_at = 0 AND cursor_row_sort_id = '' AND phase = 'catching_up'), \
           (SELECT COUNT(*) FROM observability.dashboard_alert_projection_events)",
    )
    .bind(recent_window_start)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("verify cursor-only administrator history migration");
    assert_eq!(
        preserved_tail_sources, 3,
        "Dashboard tail must keep its complete cursor"
    );
    assert_eq!(
        reset_history_sources, 3,
        "admin history starts from an independent cursor"
    );
    assert_eq!(retained_events, 1, "migration must not rewrite the sidecar");

    advance_alert_projection_until_full_coverage(&proxy).await;
    let historical_boundary_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events \
         WHERE source_kind = 'auth_token_log'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read projected history boundary events");
    assert_eq!(
        historical_boundary_events, 2,
        "history must include an alert from the second immediately below the tail boundary"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_history_fence_repair_replays_old_v14_gap() {
    let db_path = temp_db_path("alert-projection-history-fence-repair");
    let db_string = db_path.to_string_lossy().to_string();
    let now: i64 = 1_752_575_025;
    let tail_boundary = now.saturating_sub(30 * 24 * 60 * 60);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-history-fence-repair".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(
        &proxy,
        "projection-history-fence-repair-token",
        tail_boundary.saturating_sub(1),
    )
    .await;
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_state \
         SET cursor_occurred_at = ?, cursor_row_sort_id = 'legacy-tail', \
             phase = 'idle', observed_at = ?",
    )
    .bind(tail_boundary)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate an existing completed tail");
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_history_state \
         SET cursor_occurred_at = 0, cursor_row_sort_id = '', \
             fence_occurred_at = ?, fence_row_sort_id = '', phase = 'catching_up'",
    )
    .bind(tail_boundary.saturating_sub(1))
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate the old v14 one-second-short history fence");
    let v14_checksum: String =
        sqlx::query_scalar("SELECT checksum FROM schema_migrations WHERE version = 14")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read recorded v14 checksum");
    assert_eq!(v14_checksum, "sha256:ff6f5901c3a603feac18afbbb04a1cdf");
    sqlx::query("DELETE FROM schema_migrations WHERE version = 15")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate upgrade from the old v14 ledger");

    proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect("repair persisted v14 history fence");
    let repaired_sources: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_history_state \
         WHERE cursor_occurred_at = 0 AND cursor_row_sort_id = '' \
           AND fence_occurred_at = ? AND fence_row_sort_id = '' \
           AND phase = 'catching_up'",
    )
    .bind(tail_boundary)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired history state");
    assert_eq!(
        repaired_sources, 3,
        "repair must reset derived history only"
    );
    let v15_recorded: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 15 \
         AND checksum = 'sha256:8ef5bf8e2b29acd27096657ad0d3d97e')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("verify recorded fence repair migration");
    assert_eq!(v15_recorded, 1);

    advance_alert_projection_until_full_coverage(&proxy).await;
    let boundary_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events \
         WHERE source_kind = 'auth_token_log'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired boundary event");
    assert_eq!(
        boundary_events, 1,
        "repair must replay the omitted boundary second"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_exact_tail_boundary_belongs_to_recent_lane() {
    let db_path = temp_db_path("alert-projection-exact-tail-boundary");
    let db_string = db_path.to_string_lossy().to_string();
    let now: i64 = 1_752_575_075;
    let tail_boundary = now.saturating_sub(30 * 24 * 60 * 60);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-exact-tail-boundary".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    insert_projected_rate_limit_alert(
        &proxy,
        "projection-exact-tail-boundary-token",
        tail_boundary,
    )
    .await;
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_state \
         SET cursor_occurred_at = ?, cursor_row_sort_id = '', phase = 'idle', observed_at = ?",
    )
    .bind(tail_boundary)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate an old recent tail boundary");
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_history_state \
         SET cursor_occurred_at = 0, cursor_row_sort_id = '', \
             fence_occurred_at = ?, fence_row_sort_id = '', phase = 'catching_up'",
    )
    .bind(tail_boundary)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate the old exact-second history fence");
    sqlx::query("DELETE FROM schema_migrations WHERE version = 17")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate upgrade before exact-boundary repair");

    proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect("repair exact-second boundary ownership");
    let repaired_fences: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_history_state \
         WHERE cursor_occurred_at = 0 AND cursor_row_sort_id = '' \
           AND fence_occurred_at = ? AND fence_row_sort_id = '' \
           AND phase = 'catching_up'",
    )
    .bind(tail_boundary.saturating_sub(1))
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired history fence");
    assert_eq!(repaired_fences, 3);

    for _ in 0..6 {
        let outcome = proxy
            .key_store
            .advance_alert_projection_slice()
            .await
            .expect("advance exact-boundary projection slice");
        if matches!(outcome, AlertProjectionSliceOutcome::Deferred { .. }) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
    let boundary_events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events \
         WHERE source_kind = 'auth_token_log'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read exact-boundary recent-tail event");
    assert_eq!(boundary_events, 1);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_preempts_backlog_for_a_new_idle_source() {
    let db_path = temp_db_path("alert-projection-tail-source-fairness");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_575_050;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-tail-source-fairness".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    advance_alert_projection_until_full_coverage(&proxy).await;
    for index in 0..26 {
        insert_projected_rate_limit_alert(
            &proxy,
            &format!("projection-fairness-token-{index}"),
            now,
        )
        .await;
    }
    let first = advance_alert_projection_slice_until_admitted(&proxy).await;
    assert!(matches!(
        first,
        AlertProjectionSliceOutcome::Advanced { rows: 25, .. }
    ));
    sqlx::query(
        r#"INSERT INTO scheduled_jobs
                (job_type, trigger_source, status, attempt, message, queued_at, started_at, finished_at)
           VALUES ('projection_fairness', 'scheduler', 'failed', 1, 'fresh failure', ?, ?, ?)"#,
    )
    .bind(now + 1)
    .bind(now + 1)
    .bind(now + 1)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert a fresh failed job alert");

    let second = advance_alert_projection_slice_until_admitted(&proxy).await;
    assert!(matches!(
        second,
        AlertProjectionSliceOutcome::Advanced { rows: 1, .. }
    ));
    let (scheduled_rows, auth_phase): (i64, String) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM observability.dashboard_alert_projection_events \
             WHERE source_kind = 'scheduled_job'), \
           (SELECT phase FROM observability.dashboard_alert_projection_state \
             WHERE source_kind = 'auth_token_log')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("verify idle-source preemption");
    assert_eq!(
        scheduled_rows, 1,
        "fresh scheduled-job alert must be projected"
    );
    assert_eq!(
        auth_phase, "catching_up",
        "the original backlog remains resumable"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_idle_probe_does_not_persist_empty_cursors() {
    let db_path = temp_db_path("alert-projection-idle-no-write");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_575_100;
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-idle-no-write".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    advance_alert_projection_until_full_coverage(&proxy).await;
    let before: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT SUM(generation) FROM observability.dashboard_alert_projection_state \
         UNION ALL \
         SELECT SUM(generation) FROM observability.dashboard_alert_projection_history_state",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read cursor generations before idle probe")
    .into_iter()
    .sum();
    let outcome = advance_alert_projection_slice_until_admitted(&proxy).await;
    assert_eq!(outcome, AlertProjectionSliceOutcome::Idle);
    let after: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT SUM(generation) FROM observability.dashboard_alert_projection_state \
         UNION ALL \
         SELECT SUM(generation) FROM observability.dashboard_alert_projection_history_state",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read cursor generations after idle probe")
    .into_iter()
    .sum();
    assert_eq!(
        after, before,
        "an empty probe must not write projection state"
    );

    clock.advance_wall(std::time::Duration::from_secs(91));
    let stale = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read expired tail coverage");
    assert_eq!(stale.recent_coverage, "stale");
    refresh_alert_projection_observation_until_admitted(&proxy).await;
    let recovered = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read refreshed tail coverage");
    assert_eq!(recovered.recent_coverage, "ok");
    let heartbeat_generation: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT SUM(generation) FROM observability.dashboard_alert_projection_state \
         UNION ALL \
         SELECT SUM(generation) FROM observability.dashboard_alert_projection_history_state",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read cursor generations after idle heartbeat")
    .into_iter()
    .sum();
    assert_eq!(
        heartbeat_generation, before,
        "the observation heartbeat must not advance a projection cursor"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_marks_expired_observation_as_stale() {
    let db_path = temp_db_path("alert-projection-stale-observation");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_500_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-stale".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");

    for _ in 0..3 {
        proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("observe each projection source");
    }
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_state \
         SET phase = 'idle', observed_at = ?, stale_reason = NULL",
    )
    .bind(now - 91)
    .execute(&proxy.key_store.pool)
    .await
    .expect("expire projection observations");

    let status = proxy
        .dashboard_alert_projection_status()
        .await
        .expect("read alert projection coverage");
    assert_eq!(status.coverage, "stale");
    assert_eq!(status.stale_reason.as_deref(), Some("observation_expired"));

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn alert_projection_writer_contention_is_a_typed_defer() {
    let db_path = temp_db_path("alert-projection-writer-contention");
    let db_string = db_path.to_string_lossy().to_string();
    let now = 1_752_600_000;
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-contention".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    insert_projected_rate_limit_alert(&proxy, "projection-token-contention", now).await;

    let mut holder =
        connect_sqlite_test_connection(&db_string, false, false, std::time::Duration::ZERO).await;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut holder)
        .await
        .expect("hold writer lock");

    let started = std::time::Instant::now();
    let outcome = proxy
        .key_store
        .advance_alert_projection_slice()
        .await
        .expect("writer contention is an expected projection defer");
    assert!(
        matches!(
            outcome,
            AlertProjectionSliceOutcome::Deferred {
                reason: SqliteAdmissionDeferReason::RecentContention
            }
        ),
        "expected a typed contention defer, got {outcome:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_millis(250),
        "projection must yield instead of waiting behind the writer lock"
    );

    sqlx::query("ROLLBACK")
        .execute(&mut holder)
        .await
        .expect("release writer lock");
    drop(holder);
    // Restart recovery uses a fresh runtime rather than relying on a
    // contention cooldown. The deferred slice did not advance its cursor,
    // so the durable source fence can safely replay it.
    drop(proxy);
    let (resumed_time, _) = BackendTime::manual_from_ts(now + 1);
    let resumed = TavilyProxy::with_options_and_time(
        vec!["tvly-alert-projection-contention".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        resumed_time,
    )
    .await
    .expect("reopen after writer contention");
    advance_alert_projection_until(&resumed, 1).await;

    drop(resumed);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

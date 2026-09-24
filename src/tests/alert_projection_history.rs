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

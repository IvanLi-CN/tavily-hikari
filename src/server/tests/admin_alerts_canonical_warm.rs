use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn admin_alerts_warm_refreshes_stale_idle_projection_under_foreground_pressure() {
    let db_path = temp_db_path("admin-alerts-liveness-stale-observation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-stale-observation".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let (_, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "admin-alerts-liveness-stale-observation-password",
    )
    .await;

    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before warming the admin cache");
        if state.proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        "UPDATE observability.dashboard_alert_projection_state \
            SET observed_at = 0, stale_reason = NULL",
    )
    .execute(&pool)
    .await
    .expect("make the idle projection observation stale");

    super::super::rearm_admin_alerts_prewarm_for_test(state.as_ref()).await;
    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }
    assert!(
        state.proxy.foreground_activity_rps() > 5,
        "fixture establishes sustained foreground pressure"
    );

    let projection_state = state.clone();
    let projection = tokio::spawn(async move {
        loop {
            let _ = projection_state
                .proxy
                .advance_dashboard_alert_projection_scheduler_step_with_alerts()
                .await;
            let _ = projection_state
                .proxy
                .refresh_dashboard_alert_projection_observation()
                .await;
            tokio::task::yield_now().await;
        }
    });
    let foreground_state = state.clone();
    let foreground = tokio::spawn(async move {
        loop {
            for _ in 0..6 {
                foreground_state.proxy.record_foreground_activity();
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    super::super::prewarm_admin_alerts(state.clone()).await;

    let published = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let cache_handle = super::super::dashboard_overview_cache_for_state(state.as_ref());
            let cache = cache_handle.lock().await;
            let keys = [
                "catalog".to_string(),
                super::super::default_admin_alert_cache_key("events"),
                super::super::default_admin_alert_cache_key("groups"),
            ];
            let published = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .any(|entry| entry.canonical && entry.key == *key)
            });
                let same_generation = keys.iter().all(|key| {
                    cache
                        .admin_alerts
                        .entries
                        .iter()
                        .find(|entry| entry.canonical && entry.key == *key)
                        .is_some_and(|entry| entry.generation == cache.alert_projection_generation)
                });
                if published && same_generation {
                    break true;
                }
            drop(cache);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;

    projection.abort();
    let _ = projection.await;
    foreground.abort();
    let _ = foreground.await;
    super::super::shutdown_admin_alerts_workers(state.as_ref()).await;
    let _ = std::fs::remove_file(db_path);

    assert!(
        published.is_ok(),
        "stale idle projection observations must not starve canonical warm"
    );
}

#[tokio::test]
async fn admin_alerts_warm_liveness_publishes_all_keys_under_foreground_pressure_and_projection_churn() {
    let db_path = temp_db_path("admin-alerts-liveness-slice-pressure");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-slice-pressure".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let (_, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "admin-alerts-liveness-slice-pressure-password",
    )
    .await;

    let mut projection_ready = false;
    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before warming the admin cache");
        if state.proxy.admin_alert_catalog().await.is_ok() {
            projection_ready = true;
            break;
        }
    }
    assert!(projection_ready, "empty projection must complete before warming admin cache");

    let pool = connect_sqlite_test_pool(&db_str).await;
    let occurred_at = Utc::now().timestamp().saturating_sub(60);
    for index in 0..501_i64 {
        let source_id = format!("alert-liveness-{index:04}");
        let row_sort_id = format!("alert-liveness-sort-{index:04}");
        let payload = serde_json::json!({
            "source_kind": "auth_token_log",
            "source_id": source_id,
            "row_sort_id": row_sort_id,
            "alert_type": "upstream_rate_limited_429",
            "occurred_at": occurred_at - index,
            "token_id": "token-liveness",
            "key_id": "key-liveness",
            "request_log_id": null,
            "method": "POST",
            "path": "/mcp",
            "query": null,
            "request_kind_key": "tavily_search",
            "request_kind_label": "Tavily Search",
            "request_kind_detail": "POST /mcp",
            "result_status": "error",
            "failure_kind": "upstream_rate_limited_429",
            "error_message": "HTTP 429",
            "counts_business_quota": true,
            "user_id": "user-liveness",
            "user_display_name": "Liveness User",
            "user_username": "liveness",
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
            "job_finished_at": null
        });
        sqlx::query(
            r#"INSERT INTO observability.dashboard_alert_projection_events
                   (source_kind, source_id, occurred_at, row_sort_id, payload_json, projected_at)
               VALUES ('auth_token_log', ?, ?, ?, ?, ?)"#,
        )
        .bind(&source_id)
        .bind(occurred_at - index)
        .bind(&row_sort_id)
        .bind(payload.to_string())
        .bind(occurred_at - index)
        .execute(&pool)
        .await
        .expect("seed multi-slice projected alert event");
    }

    super::super::rearm_admin_alerts_prewarm_for_test(state.as_ref()).await;
    {
        let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
        let mut cache = cache.lock().await;
        cache.admin_alerts_prewarm_last_progress_at = Some(
            tokio::time::Instant::now()
                .checked_sub(super::super::ADMIN_ALERTS_PREWARM_LIVENESS_AFTER)
                .expect("test clock must support the Alerts liveness anchor"),
        );
    }
    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }
    assert!(
        state.proxy.foreground_activity_rps() > 5,
        "fixture establishes sustained foreground pressure"
    );

    let churn_state = state.clone();
    let churn_pool = pool.clone();
    let projection_churn = tokio::spawn(async move {
        for index in 0..400_i64 {
            sqlx::query(
                r#"INSERT INTO auth_token_logs (
                       token_id, method, path, result_status, error_message, failure_kind,
                       key_effect_code, binding_effect_code, selection_effect_code,
                       counts_business_quota, created_at
                   ) VALUES (?, 'POST', '/mcp', 'quota_exhausted', 'HTTP 429',
                             'upstream_rate_limited_429', 'none', 'none', 'none', 0, ?)"#,
            )
            .bind(format!("token-alert-liveness-churn-{index:04}"))
            .bind(churn_state.proxy.backend_time().now_ts().saturating_add(index))
            .execute(&churn_pool)
            .await
            .expect("seed projection churn alert");
            let _ = churn_state
                .proxy
                .advance_dashboard_alert_projection_scheduler_step_with_alerts()
                .await;
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
    });

    super::super::prewarm_admin_alerts(state.clone()).await;
    let published = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let cache_handle = super::super::dashboard_overview_cache_for_state(state.as_ref());
            let cache = cache_handle.lock().await;
            let keys = [
                "catalog".to_string(),
                super::super::default_admin_alert_cache_key("events"),
                super::super::default_admin_alert_cache_key("groups"),
            ];
            let published = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .any(|entry| entry.canonical && entry.key == *key)
            });
            let same_generation = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .find(|entry| entry.canonical && entry.key == *key)
                    .is_some_and(|entry| entry.generation == cache.alert_projection_generation)
            });
            if published && same_generation {
                break;
            }
            drop(cache);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;

    projection_churn.abort();
    let _ = projection_churn.await;
    super::super::shutdown_admin_alerts_workers(state.as_ref()).await;
    let _ = std::fs::remove_file(db_path);
    published.expect("an aged canonical warm must publish all three keys in one logical stage");
}

#[tokio::test]
async fn admin_alerts_warm_liveness_does_not_sleep_between_groups_slices() {
    let db_path = temp_db_path("admin-alerts-liveness-many-groups");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-many-groups".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let (_, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "admin-alerts-liveness-many-groups-password",
    )
    .await;

    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before warming the admin cache");
        if state.proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    let pool = connect_sqlite_test_pool(&db_str).await;
    let occurred_at = Utc::now().timestamp().saturating_sub(60);
    for index in 0..80_i64 {
        let source_id = format!("alert-many-groups-{index:04}");
        let token_id = format!("token-many-groups-{index:04}");
        let row_sort_id = format!("alert-many-groups-sort-{index:04}");
        let payload = serde_json::json!({
            "source_kind": "auth_token_log",
            "source_id": source_id,
            "row_sort_id": row_sort_id,
            "alert_type": "upstream_rate_limited_429",
            "occurred_at": occurred_at - index,
            "token_id": token_id,
            "key_id": format!("key-many-groups-{index:04}"),
            "request_log_id": null,
            "method": "POST",
            "path": "/mcp",
            "query": null,
            "request_kind_key": "tavily_search",
            "request_kind_label": "Tavily Search",
            "request_kind_detail": "POST /mcp",
            "result_status": "error",
            "failure_kind": "upstream_rate_limited_429",
            "error_message": "HTTP 429",
            "counts_business_quota": true,
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
            "job_finished_at": null
        });
        sqlx::query(
            r#"INSERT INTO observability.dashboard_alert_projection_events
                   (source_kind, source_id, occurred_at, row_sort_id, payload_json, projected_at)
               VALUES ('auth_token_log', ?, ?, ?, ?, ?)"#,
        )
        .bind(&source_id)
        .bind(occurred_at - index)
        .bind(&row_sort_id)
        .bind(payload.to_string())
        .bind(occurred_at - index)
        .execute(&pool)
        .await
        .expect("seed many canonical Groups partitions");
    }

    super::super::rearm_admin_alerts_prewarm_for_test(state.as_ref()).await;
    {
        let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
        let mut cache = cache.lock().await;
        cache.admin_alerts.entries.clear();
        cache.admin_alerts_prewarm_last_progress_at = Some(
            tokio::time::Instant::now()
                .checked_sub(super::super::ADMIN_ALERTS_PREWARM_LIVENESS_AFTER)
                .expect("test clock must support the Alerts liveness anchor"),
        );
    }
    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }

    super::super::prewarm_admin_alerts(state.clone()).await;
    let published = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
            let cache = cache.lock().await;
            let keys = [
                "catalog".to_string(),
                super::super::default_admin_alert_cache_key("events"),
                super::super::default_admin_alert_cache_key("groups"),
            ];
            let complete = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .any(|entry| entry.canonical && entry.key == *key)
            });
            let same_generation = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .find(|entry| entry.canonical && entry.key == *key)
                    .is_some_and(|entry| entry.generation == cache.alert_projection_generation)
            });
            if complete && same_generation {
                break;
            }
            drop(cache);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;

    let build_state: (i64, String, i64, i64) = sqlx::query_as(
        "SELECT build_generation, build_phase, build_cursor_source_rowid, active_generation \
           FROM observability.admin_alert_canonical_groups_state \
          WHERE singleton = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("read canonical Groups state after warm attempt");
    super::super::shutdown_admin_alerts_workers(state.as_ref()).await;
    drop(pool);
    let _ = std::fs::remove_file(&db_path);

    assert!(
        published.is_ok(),
        "canonical warm must publish many Groups partitions without a fixed inter-slice sleep; state={build_state:?}"
    );
}

#[tokio::test]
async fn admin_alerts_warm_liveness_fences_projection_until_groups_publish() {
    let db_path = temp_db_path("admin-alerts-liveness-clearing-build");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-clearing-build".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let (_, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "admin-alerts-liveness-clearing-build-password",
    )
    .await;

    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before warming the admin cache");
        if state.proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    let pool = connect_sqlite_test_pool(&db_str).await;
    let (revision, recent_generation, history_generation, source_rowid_upper_bound): (i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT
                (SELECT revision FROM observability.dashboard_alert_projection_revision_state WHERE singleton = 1),
                (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state),
                (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_history_state),
                (SELECT COALESCE(MAX(rowid), 0) FROM observability.dashboard_alert_projection_events)",
        )
        .fetch_one(&pool)
        .await
        .expect("read the current canonical warm fence");
    for index in 0..625_i64 {
        sqlx::query(
            r#"INSERT INTO observability.admin_alert_canonical_group_events
                   (build_generation, source_kind, source_id, occurred_at, row_sort_id,
                    partition_key, payload_json)
               VALUES (1, 'auth_token_log', ?, ?, ?, 'key:stale', '{}')"#,
        )
        .bind(format!("stale-clearing-{index:04}"))
        .bind(index)
        .bind(format!("stale-clearing:{index:04}"))
        .execute(&pool)
        .await
        .expect("seed bounded clearing debt");
    }
    sqlx::query(
        r#"UPDATE observability.admin_alert_canonical_groups_state
              SET build_generation = 1,
                  build_projection_revision = ?,
                  build_source_recent_generation = ?,
                  build_source_history_generation = ?,
                  build_source_rowid_upper_bound = ?,
                  build_cursor_source_rowid = 9223372036854775807,
                  build_phase = 'clearing'
            WHERE singleton = 1"#,
    )
    .bind(revision)
    .bind(recent_generation)
    .bind(history_generation)
    .bind(source_rowid_upper_bound)
    .execute(&pool)
    .await
    .expect("seed same-fence clearing build state");

    super::super::rearm_admin_alerts_prewarm_for_test(state.as_ref()).await;
    {
        let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
        let mut cache = cache.lock().await;
        cache.admin_alerts_prewarm_last_progress_at = Some(
            tokio::time::Instant::now()
                .checked_sub(super::super::ADMIN_ALERTS_PREWARM_LIVENESS_AFTER)
                .expect("test clock must support the Alerts liveness anchor"),
        );
    }
    for _ in 0..6 {
        state.proxy.record_foreground_activity();
    }
    assert!(state.proxy.foreground_activity_rps() > 5);

    let groups_defer_pause =
        super::super::install_admin_alerts_warm_after_groups_defer_pause_for_test(state.as_ref())
            .await;
    super::super::prewarm_admin_alerts(state.clone()).await;
    let handoff_reached = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        groups_defer_pause.wait_until_arrived(),
    )
    .await;
    if handoff_reached.is_err() {
        let cache_handle = super::super::dashboard_overview_cache_for_state(state.as_ref());
        let cache = cache_handle.lock().await;
        let phase: String = sqlx::query_scalar(
            "SELECT build_phase FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("read canonical Groups build phase after handoff timeout");
        panic!(
            "warm did not reach Groups build defer: in_flight={}, defers={}, groups_build_in_flight={}, liveness={}, phase={phase}",
            cache.admin_alerts_prewarm_in_flight,
            cache.admin_alerts_prewarm_defers,
            cache.admin_alerts_groups_build_in_flight,
            state.proxy.admin_alerts_cache_warm_liveness_admission_active(),
        );
    }
    sqlx::query(
        r#"INSERT INTO auth_token_logs (
               token_id, method, path, result_status, error_message, failure_kind,
               key_effect_code, binding_effect_code, selection_effect_code,
               counts_business_quota, created_at
           ) VALUES (?, 'POST', '/mcp', 'quota_exhausted', 'HTTP 429',
                     'upstream_rate_limited_429', 'none', 'none', 'none', 0, ?)"#,
    )
    .bind("token-alert-liveness-groups-defer-handoff")
    .bind(state.proxy.backend_time().now_ts())
    .execute(&pool)
    .await
    .expect("seed a new alert while Groups yielded its liveness stage");
    let projection_step = state
        .proxy
        .advance_dashboard_alert_projection_scheduler_step_with_alerts()
        .await
        .expect("advance projection after Groups yielded its liveness stage");
    let churn_state = state.clone();
    let churn_pool = pool.clone();
    let projection_churn = tokio::spawn(async move {
        for index in 0..400_i64 {
            sqlx::query(
                r#"INSERT INTO auth_token_logs (
                       token_id, method, path, result_status, error_message, failure_kind,
                       key_effect_code, binding_effect_code, selection_effect_code,
                       counts_business_quota, created_at
                   ) VALUES (?, 'POST', '/mcp', 'quota_exhausted', 'HTTP 429',
                             'upstream_rate_limited_429', 'none', 'none', 'none', 0, ?)"#,
            )
            .bind(format!("token-alert-liveness-groups-churn-{index:04}"))
            .bind(churn_state.proxy.backend_time().now_ts().saturating_add(index))
            .execute(&churn_pool)
            .await
            .expect("seed a churn alert");
            churn_state
                .proxy
                .advance_dashboard_alert_projection_scheduler_step_with_alerts()
                .await
                .expect("advance projection during Groups churn");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    });
    groups_defer_pause.release();
    assert!(
        !projection_step.canonical_alerts_dirty,
        "Groups build must retain its liveness fence until the canonical publish"
    );
    let published = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
            let cache = cache.lock().await;
            let keys = [
                "catalog".to_string(),
                super::super::default_admin_alert_cache_key("events"),
                super::super::default_admin_alert_cache_key("groups"),
            ];
            let published = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .any(|entry| entry.canonical && entry.key == *key)
            });
            let same_generation = keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .find(|entry| entry.canonical && entry.key == *key)
                    .is_some_and(|entry| entry.generation == cache.alert_projection_generation)
            });
            if published && same_generation {
                break;
            }
            drop(cache);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    if published.is_err() {
        projection_churn.abort();
        let _ = projection_churn.await;
        let build_state: (i64, String, i64, i64, String, bool, i64, i64, i64, i64, i64) =
            sqlx::query_as(
                "SELECT build_generation, build_phase, build_cursor_occurred_at,
                        build_cursor_source_rowid, build_partition_key,
                        build_partition_source_complete,
                        build_partition_fragment_next_position,
                        build_partition_finalize_fragment_position,
                        build_next_position, payload_read_generation,
                        payload_read_position
                   FROM observability.admin_alert_canonical_groups_state
                  WHERE singleton = 1",
            )
            .fetch_one(&pool)
            .await
            .expect("read canonical Groups state after publish timeout");
        panic!(
            "canonical Alerts publish timed out: build_state={build_state:?}, projection_fence={:?}",
            state.proxy.admin_alerts_canonical_warm_projection_fence().await
        );
    }
    projection_churn
        .await
        .expect("projection churn task must complete without panicking");
    super::super::shutdown_admin_alerts_workers(state.as_ref()).await;
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_alerts_warm_publishes_all_keys_after_groups_write_lock_recovery() {
    let db_path = temp_db_path("admin-alerts-liveness-write-lock-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-write-lock-recovery".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let (_, state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "admin-alerts-liveness-write-lock-recovery-password",
    )
    .await;

    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before warming the admin cache");
        if state.proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    let pool = connect_sqlite_test_pool(&db_str).await;
    let occurred_at = Utc::now().timestamp().saturating_sub(60);
    for index in 0..126_i64 {
        sqlx::query(
            r#"INSERT INTO auth_token_logs (
                   token_id, method, path, result_status, error_message, failure_kind,
                   key_effect_code, binding_effect_code, selection_effect_code,
                   counts_business_quota, created_at
               ) VALUES (?, 'POST', '/mcp', 'quota_exhausted', 'HTTP 429',
                         'upstream_rate_limited_429', 'none', 'none', 'none', 0, ?)"#,
        )
        .bind("token-alert-liveness-write-lock")
        .bind(occurred_at - index)
        .execute(&pool)
        .await
        .expect("seed source alert");
    }
    for _ in 0..64 {
        state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the source projection before warming Alerts");
        if state.proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    super::super::rearm_admin_alerts_prewarm_for_test(state.as_ref()).await;
    {
        let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
        let mut cache = cache.lock().await;
        cache.admin_alerts_prewarm_last_progress_at = Some(
            tokio::time::Instant::now()
                .checked_sub(super::super::ADMIN_ALERTS_PREWARM_LIVENESS_AFTER)
                .expect("test clock must support the Alerts liveness anchor"),
        );
    }

    let mut writer = pool.acquire().await.expect("acquire lock writer");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *writer)
        .await
        .expect("hold the observability writer lock");

    super::super::prewarm_admin_alerts(state.clone()).await;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if super::super::dashboard_overview_cache_for_state(state.as_ref())
                .lock()
                .await
                .admin_alerts_prewarm_defers
                > 0
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the bounded Groups write must defer on the held SQLite lock");
    assert!(
        state.proxy.admin_alerts_cache_warm_liveness_admission_active(),
        "a transient Groups write defer must retain the aged canonical warm liveness turn"
    );
    assert!(
        !super::super::dashboard_overview_cache_for_state(state.as_ref())
            .lock()
            .await
            .admin_alerts_groups_reclaimer_in_flight,
        "an active canonical Groups build must not start a competing reclaimer"
    );
    sqlx::query("ROLLBACK")
        .execute(&mut *writer)
        .await
        .expect("release the observability writer lock");

    let foreground = state.clone();
    let foreground_task = tokio::spawn(async move {
        loop {
            for _ in 0..6 {
                foreground.proxy.record_foreground_activity();
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });

    let published = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let cache = super::super::dashboard_overview_cache_for_state(state.as_ref());
            let cache = cache.lock().await;
            let keys = [
                "catalog".to_string(),
                super::super::default_admin_alert_cache_key("events"),
                super::super::default_admin_alert_cache_key("groups"),
            ];
            if keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .any(|entry| entry.canonical && entry.key == *key)
            }) {
                break true;
            }
            drop(cache);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;

    foreground_task.abort();
    let _ = foreground_task.await;
    super::super::shutdown_admin_alerts_workers(state.as_ref()).await;
    drop(writer);
    let _ = std::fs::remove_file(db_path);

    assert!(
        published.is_ok(),
        "a recovered canonical warm must publish catalog, Events 1/20, and Groups 1/20"
    );
}

#[tokio::test]
async fn admin_alerts_warm_restart_retains_liveness_for_a_stale_groups_build() {
    let db_path = temp_db_path("admin-alerts-liveness-restart-stale-groups-build");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-restart-stale-groups-build".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("initial proxy created");
    let (_, initial_state) = spawn_builtin_keys_admin_server_with_state(
        proxy,
        "admin-alerts-liveness-restart-stale-groups-build-password",
    )
    .await;

    for _ in 0..64 {
        initial_state
            .proxy
            .advance_dashboard_alert_projection_scheduler_step()
            .await
            .expect("complete the empty alert projection before seeding the durable build");
        if initial_state.proxy.admin_alert_catalog().await.is_ok() {
            break;
        }
    }

    let pool = connect_sqlite_test_pool(&db_str).await;
    let (revision, recent_generation, history_generation, source_rowid_upper_bound):
        (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT
                (SELECT revision FROM observability.dashboard_alert_projection_revision_state WHERE singleton = 1),
                (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state),
                (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_history_state),
                (SELECT COALESCE(MAX(rowid), 0) FROM observability.dashboard_alert_projection_events)",
        )
        .fetch_one(&pool)
        .await
        .expect("read the durable Groups build fence");
    sqlx::query(
        r#"INSERT INTO observability.admin_alert_canonical_group_events
               (build_generation, source_kind, source_id, occurred_at, row_sort_id,
                partition_key, payload_json)
           VALUES (1, 'auth_token_log', 'restart-stale-row', 1, 'restart-stale-row', 'key:stale', '{}')"#,
    )
    .execute(&pool)
    .await
    .expect("seed an inactive durable Groups build row");
    sqlx::query(
        r#"UPDATE observability.admin_alert_canonical_groups_state
              SET build_generation = 1,
                  build_projection_revision = ?,
                  build_source_recent_generation = ?,
                  build_source_history_generation = ?,
                  build_source_rowid_upper_bound = ?,
                  build_cursor_source_rowid = 9223372036854775807,
                  build_phase = 'clearing'
            WHERE singleton = 1"#,
    )
    .bind(revision)
    .bind(recent_generation)
    .bind(history_generation)
    .bind(source_rowid_upper_bound)
    .execute(&pool)
    .await
    .expect("persist the durable Groups build fence");

    sqlx::query(
        r#"INSERT INTO auth_token_logs (
               token_id, method, path, result_status, error_message, failure_kind,
               key_effect_code, binding_effect_code, selection_effect_code,
               counts_business_quota, created_at
           ) VALUES ('restart-stale-fence', 'POST', '/mcp', 'quota_exhausted', 'HTTP 429',
                     'upstream_rate_limited_429', 'none', 'none', 'none', 0, ?)"#,
    )
    .bind(initial_state.proxy.backend_time().now_ts())
    .execute(&pool)
    .await
    .expect("seed the source-fence advance");
    initial_state
        .proxy
        .advance_dashboard_alert_projection_scheduler_step_with_alerts()
        .await
        .expect("advance the source projection after the durable build was staged");
    let (advanced_recent_generation, advanced_history_generation): (i64, i64) = sqlx::query_as(
        "SELECT
            (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state),
            (SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_history_state)",
    )
    .fetch_one(&pool)
    .await
    .expect("read the advanced source fence");
    assert_ne!(
        (advanced_recent_generation, advanced_history_generation),
        (recent_generation, history_generation),
        "the restart fixture must leave the durable Groups build stale"
    );

    super::super::shutdown_admin_alerts_workers(initial_state.as_ref()).await;
    drop(initial_state);

    let recovered_proxy = TavilyProxy::with_endpoint(
        vec!["tvly-admin-alerts-liveness-restart-stale-groups-build".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("recovered proxy created");
    let (_, recovered_state) = spawn_builtin_keys_admin_server_with_state(
        recovered_proxy,
        "admin-alerts-liveness-restart-stale-groups-build-password",
    )
    .await;
    super::super::rearm_admin_alerts_prewarm_for_test(recovered_state.as_ref()).await;
    let stale_fence_pause = super::super::install_admin_alerts_warm_after_groups_source_fence_changed_pause_for_test(
        recovered_state.as_ref(),
    )
    .await;
    super::super::prewarm_admin_alerts(recovered_state.clone()).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        stale_fence_pause.wait_until_arrived(),
    )
    .await
    .expect("recovered warm must discard the stale durable Groups build");
    assert!(
        recovered_state
            .proxy
            .admin_alerts_cache_warm_liveness_admission_active(),
        "stale durable Groups recovery must retain the canonical warm liveness fence"
    );
    stale_fence_pause.release();

    let published = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let cache = super::super::dashboard_overview_cache_for_state(recovered_state.as_ref());
            let cache = cache.lock().await;
            let keys = [
                "catalog".to_string(),
                super::super::default_admin_alert_cache_key("events"),
                super::super::default_admin_alert_cache_key("groups"),
            ];
            if keys.iter().all(|key| {
                cache
                    .admin_alerts
                    .entries
                    .iter()
                    .any(|entry| entry.canonical && entry.key == *key)
            }) {
                break true;
            }
            drop(cache);
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;

    super::super::shutdown_admin_alerts_workers(recovered_state.as_ref()).await;
    drop(pool);
    let _ = std::fs::remove_file(&db_path);
    assert!(
        published.is_ok(),
        "recovered canonical warm must publish catalog, Events 1/20, and Groups 1/20"
    );
}

use super::*;

#[tokio::test]
async fn dashboard_month_series_uses_full_natural_month_axis_and_previous_month_comparison() {
    let db_path = temp_db_path("dashboard-month-series-natural-axis");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-month-series".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;

    let evaluation_time = Utc
        .with_ymd_and_hms(2026, 4, 7, 12, 10, 0)
        .single()
        .expect("valid utc evaluation time");
    let local_evaluation = evaluation_time.with_timezone(&Local);
    let summary_windows = proxy
        .summary_windows_at(local_evaluation)
        .await
        .expect("summary windows");

    let current_month_start = summary_windows.month_start;
    let previous_month_start = summary_windows.previous_month_start;
    let previous_month_end = summary_windows.previous_month_end;
    let current_month_day_start = next_local_day_start_utc_ts(current_month_start);
    let previous_month_day_start = next_local_day_start_utc_ts(previous_month_start);
    let current_day_start = local_day_bucket_start_utc_ts(evaluation_time.timestamp());

    insert_dashboard_summary_rollup_day_bucket(&proxy, current_month_start, 12, 9, 2, 1).await;
    insert_dashboard_summary_rollup_day_bucket(&proxy, current_month_day_start, 18, 14, 3, 1).await;

    insert_dashboard_summary_rollup_day_bucket(&proxy, previous_month_start, 10, 7, 2, 1).await;
    insert_dashboard_summary_rollup_day_bucket(&proxy, previous_month_day_start, 8, 6, 1, 1).await;

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at) VALUES
            ('month-series-current-day-1', 'tvly-month-series-current-day-1', 'active', ?),
            ('month-series-current-day-2', 'tvly-month-series-current-day-2', 'active', ?),
            ('month-series-current-day-3', 'tvly-month-series-current-day-3', 'active', ?),
            ('month-series-previous-day-1', 'tvly-month-series-previous-day-1', 'active', ?),
            ('month-series-previous-day-2', 'tvly-month-series-previous-day-2', 'active', ?)
        "#,
    )
    .bind(current_month_start + 30)
    .bind(current_month_day_start + 30)
    .bind(current_day_start + 30)
    .bind(previous_month_start + 30)
    .bind(previous_month_day_start + 30)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert lifecycle test api keys");

    sqlx::query(
        r#"
        INSERT INTO api_key_quarantines (
            id,
            key_id,
            source,
            reason_code,
            reason_summary,
            reason_detail,
            created_at,
            cleared_at
        ) VALUES
            ('month-series-quarantine-current-1', 'month-series-current-day-1', 'system', 'quota_exhausted', 'quota exhausted', 'current month quarantine', ?, NULL),
            ('month-series-quarantine-current-2', 'month-series-current-day-2', 'system', 'quota_exhausted', 'quota exhausted', 'current day quarantine', ?, NULL),
            ('month-series-quarantine-previous-1', 'month-series-previous-day-1', 'system', 'quota_exhausted', 'quota exhausted', 'previous month quarantine', ?, NULL)
        "#,
    )
    .bind(current_month_start + 45)
    .bind(current_day_start + 45)
    .bind(previous_month_start + 45)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert lifecycle quarantines");

    sqlx::query(
        r#"
        INSERT INTO api_key_maintenance_records (
            id,
            key_id,
            source,
            operation_code,
            operation_summary,
            reason_code,
            reason_summary,
            reason_detail,
            request_log_id,
            auth_token_log_id,
            auth_token_id,
            actor_user_id,
            actor_display_name,
            status_before,
            status_after,
            quarantine_before,
            quarantine_after,
            created_at
        ) VALUES
            ('month-series-maint-current-1', 'month-series-current-day-1', ?, ?, 'auto mark exhausted', ?, 'quota exhausted', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
            ('month-series-maint-current-1-repeat', 'month-series-current-day-1', ?, ?, 'auto mark exhausted', ?, 'quota exhausted', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
            ('month-series-maint-current-2', 'month-series-current-day-2', ?, ?, 'auto mark exhausted', ?, 'quota exhausted', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
            ('month-series-maint-previous-1', 'month-series-previous-day-1', ?, ?, 'auto mark exhausted', ?, 'quota exhausted', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
            ('month-series-maint-previous-2', 'month-series-previous-day-2', ?, ?, 'auto mark exhausted', ?, 'quota exhausted', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?)
        "#,
    )
    .bind(MAINTENANCE_SOURCE_SYSTEM)
    .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
    .bind(OUTCOME_QUOTA_EXHAUSTED)
    .bind(current_month_start + 60)
    .bind(MAINTENANCE_SOURCE_SYSTEM)
    .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
    .bind(OUTCOME_QUOTA_EXHAUSTED)
    .bind(current_month_day_start + 60)
    .bind(MAINTENANCE_SOURCE_SYSTEM)
    .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
    .bind(OUTCOME_QUOTA_EXHAUSTED)
    .bind(current_day_start + 60)
    .bind(MAINTENANCE_SOURCE_SYSTEM)
    .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
    .bind(OUTCOME_QUOTA_EXHAUSTED)
    .bind(previous_month_start + 60)
    .bind(MAINTENANCE_SOURCE_SYSTEM)
    .bind(MAINTENANCE_OP_AUTO_MARK_EXHAUSTED)
    .bind(OUTCOME_QUOTA_EXHAUSTED)
    .bind(previous_month_day_start + 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert lifecycle maintenance records");

    insert_dashboard_hourly_log(
        &proxy,
        &key_id,
        DashboardHourlyLogSeed {
            created_at: current_day_start + 60,
            path: "/api/tavily/search",
            request_kind_key: "api:search",
            request_kind_label: "API | search",
            result_status: OUTCOME_SUCCESS,
            failure_kind: None,
            request_body: None,
            visibility: REQUEST_LOG_VISIBILITY_VISIBLE,
        },
    )
    .await;
    insert_dashboard_hourly_log(
        &proxy,
        &key_id,
        DashboardHourlyLogSeed {
            created_at: current_day_start + 180,
            path: "/mcp",
            request_kind_key: "mcp:search",
            request_kind_label: "MCP | search",
            result_status: OUTCOME_ERROR,
            failure_kind: None,
            request_body: None,
            visibility: REQUEST_LOG_VISIBILITY_VISIBLE,
        },
    )
    .await;

    let month_series = proxy
        .key_store
        .fetch_dashboard_month_series(&summary_windows)
        .await
        .expect("dashboard month series");

    let current_month_days = KeyStore::collect_bucket_ranges(
        summary_windows.month_start,
        summary_windows.month_period_end,
        next_local_day_start_utc_ts,
    )
    .len();
    let previous_month_days = KeyStore::collect_bucket_ranges(
        previous_month_start,
        previous_month_end,
        next_local_day_start_utc_ts,
    )
    .len();
    assert_eq!(month_series.current.len(), current_month_days);
    assert_eq!(month_series.comparison.len(), previous_month_days);

    assert_eq!(month_series.current[0].total, Some(12));
    assert_eq!(month_series.current[1].total, Some(30));
    let current_day_index = month_series
        .current
        .iter()
        .position(|point| point.bucket_start == current_day_start)
        .expect("current day point");
    assert_eq!(month_series.current[current_day_index].total, Some(32));
    assert_eq!(month_series.current[0].upstream_exhausted, Some(1));
    assert_eq!(month_series.current[1].upstream_exhausted, Some(1));
    assert_eq!(
        month_series.current[current_day_index].upstream_exhausted,
        Some(2)
    );
    assert_eq!(month_series.current[0].new_keys, Some(1));
    assert_eq!(month_series.current[1].new_keys, Some(2));
    assert_eq!(month_series.current[current_day_index].new_keys, Some(3));
    assert_eq!(month_series.current[0].new_quarantines, Some(1));
    assert_eq!(month_series.current[1].new_quarantines, Some(1));
    assert_eq!(
        month_series.current[current_day_index].new_quarantines,
        Some(2)
    );
    assert!(
        month_series.current[(current_day_index + 1).min(month_series.current.len() - 1)]
            .total
            .is_none()
    );

    assert_eq!(month_series.comparison[0].total, Some(10));
    assert_eq!(month_series.comparison[1].total, Some(18));
    assert_eq!(month_series.comparison[0].upstream_exhausted, Some(1));
    assert_eq!(month_series.comparison[1].upstream_exhausted, Some(2));
    assert_eq!(month_series.comparison[0].new_keys, Some(1));
    assert_eq!(month_series.comparison[1].new_keys, Some(2));
    assert_eq!(month_series.comparison[0].new_quarantines, Some(1));
    assert_eq!(month_series.comparison[1].new_quarantines, Some(1));
    assert!(
        month_series
            .comparison
            .iter()
            .take(2)
            .all(|point| point.display_bucket_start.is_some())
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn dashboard_month_series_bucket_iteration_uses_successor_boundaries() {
    let ranges = KeyStore::collect_bucket_ranges(
        1_000,
        1_000 + 23 * 3_600 + 24 * 3_600 + 25 * 3_600,
        |start| match start {
            1_000 => 1_000 + 23 * 3_600,
            value if value == 1_000 + 23 * 3_600 => value + 24 * 3_600,
            value => value + 25 * 3_600,
        },
    );

    assert_eq!(
        ranges,
        vec![
            (1_000, 1_000 + 23 * 3_600),
            (1_000 + 23 * 3_600, 1_000 + 23 * 3_600 + 24 * 3_600,),
            (
                1_000 + 23 * 3_600 + 24 * 3_600,
                1_000 + 23 * 3_600 + 24 * 3_600 + 25 * 3_600,
            ),
        ]
    );
}

#[test]
fn dashboard_month_series_bucket_population_hides_only_future_or_non_current_truncation() {
    assert!(KeyStore::should_populate_dashboard_month_series_bucket(
        100, 200, 200, 300
    ));
    assert!(!KeyStore::should_populate_dashboard_month_series_bucket(
        200, 300, 200, 100
    ));
    assert!(KeyStore::should_populate_dashboard_month_series_bucket(
        100, 250, 200, 100
    ));
    assert!(!KeyStore::should_populate_dashboard_month_series_bucket(
        100, 250, 200, 50
    ));
}

#[tokio::test]
async fn dashboard_rollup_bucket_metrics_can_use_non_86400_second_day_bounds() {
    let db_path = temp_db_path("dashboard-rollup-variable-day-bounds");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-rollup-variable-day-bounds".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let mut tx = proxy.key_store.pool.begin().await.expect("begin tx");

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
        ) VALUES
            (1000, 86400, 10, 8, 2, 0, 8, 2, 0, 0, 0, 0, 0, 0, 0, 10, 0, 1060),
            (83800, 86400, 99, 80, 19, 0, 80, 19, 0, 0, 0, 0, 0, 0, 0, 99, 0, 83860)
        "#,
    )
    .execute(&mut *tx)
    .await
    .expect("insert rollup buckets");

    let exact = KeyStore::fetch_dashboard_rollup_bucket_metrics_in_range_tx(
        &mut tx,
        SECS_PER_DAY,
        1000,
        83800,
    )
    .await
    .expect("fetch variable-width bucket");
    let widened = KeyStore::fetch_dashboard_rollup_bucket_metrics_in_range_tx(
        &mut tx,
        SECS_PER_DAY,
        1000,
        1000 + SECS_PER_DAY,
    )
    .await
    .expect("fetch fixed-width bucket");

    assert_eq!(exact.total_requests, 10);
    assert_eq!(widened.total_requests, 109);

    tx.rollback().await.expect("rollback tx");
    let _ = std::fs::remove_file(db_path);
}

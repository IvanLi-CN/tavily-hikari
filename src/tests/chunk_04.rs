#[tokio::test]
async fn summary_windows_include_quota_charge_estimates_and_sample_diffs() {
    let db_path = temp_db_path("summary-windows-quota-charge");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-quota-charge".to_string()],
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

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);
    let yesterday_start = previous_local_day_start_utc_ts(now);
    let yesterday_same_time = previous_local_same_time_utc_ts(now);
    let local_month_start = start_of_local_month_utc_ts(now);
    let utc_month_start = start_of_month(now.with_timezone(&Utc)).timestamp();
    let now_ts = now.with_timezone(&Utc).timestamp();
    let today_quota_sample_start = today_start.max(utc_month_start);

    sqlx::query("UPDATE api_keys SET last_used_at = ?, quota_synced_at = ? WHERE id = ?")
        .bind(now_ts - 30 * 60)
        .bind(now_ts - 2 * 60 * 60)
        .bind(&key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("mark key stale for summary");

    insert_summary_window_charged_logs(&proxy, &key_id, today_start + 60, 7, 1).await;
    insert_summary_window_charged_logs(&proxy, &key_id, today_start + 120, 3, 1).await;
    insert_summary_window_charged_logs(&proxy, &key_id, yesterday_start + 60, 5, 1).await;

    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (
            key_id,
            quota_limit,
            quota_remaining,
            captured_at,
            source
        ) VALUES
            (?, 1000, 1000, ?, 'quota_sync/test'),
            (?, 1000, 980, ?, 'quota_sync/test'),
            (?, 1000, 970, ?, 'quota_sync/test'),
            (?, 1000, 975, ?, 'quota_sync/test'),
            (?, 1000, 960, ?, 'quota_sync/test')
        "#,
    )
    .bind(&key_id)
    .bind(yesterday_start - 60)
    .bind(&key_id)
    .bind(yesterday_start + 60)
    .bind(&key_id)
    .bind(today_quota_sample_start + 60)
    .bind(&key_id)
    .bind(today_quota_sample_start + 120)
    .bind(&key_id)
    .bind(today_quota_sample_start + 180)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert quota sync samples");

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");

    let expected_month_local = if utc_month_start <= yesterday_start {
        15
    } else {
        10
    };
    let expected_month_upstream = if utc_month_start <= yesterday_start {
        45
    } else {
        25
    };
    let expected_month_total = if local_month_start <= yesterday_start {
        3
    } else {
        2
    };

    assert_eq!(summary.today.quota_charge.local_estimated_credits, 10);
    assert_eq!(summary.today.quota_charge.upstream_actual_credits, 25);
    assert_eq!(summary.today.quota_charge.sampled_key_count, 1);
    assert_eq!(summary.today.quota_charge.stale_key_count, 1);
    assert_eq!(
        summary.today.quota_charge.latest_sync_at,
        Some(today_quota_sample_start + 180)
    );

    assert_eq!(summary.yesterday.quota_charge.local_estimated_credits, 5);
    assert_eq!(summary.yesterday.quota_charge.upstream_actual_credits, 20);
    assert_eq!(summary.yesterday.quota_charge.sampled_key_count, 1);

    assert_eq!(
        summary.month.quota_charge.local_estimated_credits,
        expected_month_local
    );
    assert_eq!(
        summary.month.quota_charge.upstream_actual_credits,
        expected_month_upstream
    );
    assert_eq!(summary.month.quota_charge.sampled_key_count, 1);
    assert_eq!(summary.month.quota_charge.stale_key_count, 1);
    assert_eq!(summary.month.total_requests, expected_month_total);
    assert_eq!(summary.month.success_count, expected_month_total);

    // The same-time window should end before the sample inserted at the current day's midday.
    assert!(
        summary
            .yesterday
            .quota_charge
            .latest_sync_at
            .unwrap_or_default()
            <= yesterday_same_time + 180
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_windows_month_bucket_fallback_skips_unaligned_first_local_day_bucket() {
    let db_path = temp_db_path("summary-windows-month-bucket-fallback");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-bucket-fallback".to_string()],
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

    let now = Local::now();
    let month_start = (Utc::now() - chrono::Duration::days(10))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid synthetic month start")
        .and_utc()
        .timestamp();
    let bucket_start = local_day_bucket_start_utc_ts(month_start);
    if bucket_start == month_start {
        let _ = std::fs::remove_file(db_path);
        return;
    }
    insert_summary_window_bucket(&proxy, &key_id, bucket_start, 12, 9, 2, 1).await;

    let summary = proxy
        .key_store
        .fetch_summary_windows(
            start_of_local_day_utc_ts(now),
            now.with_timezone(&Utc).timestamp().saturating_add(1),
            previous_local_day_start_utc_ts(now),
            previous_local_same_time_utc_ts(now).saturating_add(1),
            month_start,
            month_start,
        )
        .await
        .expect("summary windows");

    assert_eq!(summary.month.total_requests, 0);
    assert_eq!(summary.month.success_count, 0);
    assert_eq!(summary.month.error_count, 0);
    assert_eq!(summary.month.quota_exhausted_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_windows_month_reads_dashboard_rollup_day_buckets_for_historical_days() {
    let db_path = temp_db_path("summary-windows-month-bucket-partial-gap-fallback");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-bucket-fallback".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let month_start = (Utc::now() - chrono::Duration::days(10))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid synthetic month start")
        .and_utc()
        .timestamp();
    let first_bucket_start = local_day_bucket_start_utc_ts(month_start);
    let first_full_bucket_start = if first_bucket_start == month_start {
        month_start
    } else {
        next_local_day_start_utc_ts(first_bucket_start)
    };
    let partial_bucket_start = next_local_day_start_utc_ts(first_full_bucket_start);
    let now = Local::now();

    insert_dashboard_summary_rollup_day_bucket(&proxy, first_full_bucket_start, 10, 7, 2, 1).await;
    insert_dashboard_summary_rollup_day_bucket(&proxy, partial_bucket_start, 8, 6, 1, 1).await;

    let summary = proxy
        .key_store
        .fetch_summary_windows(
            start_of_local_day_utc_ts(now),
            now.with_timezone(&Utc).timestamp().saturating_add(1),
            previous_local_day_start_utc_ts(now),
            previous_local_same_time_utc_ts(now).saturating_add(1),
            month_start,
            month_start,
        )
        .await
        .expect("summary windows");

    assert_eq!(summary.month.total_requests, 18);
    assert_eq!(summary.month.success_count, 13);
    assert_eq!(summary.month.error_count, 3);
    assert_eq!(summary.month.quota_exhausted_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_month_falls_back_to_usage_buckets_for_partial_gap_before_logs_resume()
 {
    let db_path = temp_db_path("public-success-breakdown-bucket-fallback");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-bucket-fallback".to_string()],
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

    let month_start = (Utc::now() - chrono::Duration::days(10))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("valid synthetic month start")
        .and_utc()
        .timestamp();
    let first_bucket_start = local_day_bucket_start_utc_ts(month_start);
    let first_full_bucket_start = if first_bucket_start == month_start {
        month_start
    } else {
        next_local_day_start_utc_ts(first_bucket_start)
    };
    let partial_bucket_start = next_local_day_start_utc_ts(first_full_bucket_start);
    let partial_bucket_end = next_local_day_start_utc_ts(partial_bucket_start);
    let partial_gap_end = partial_bucket_start + ((partial_bucket_end - partial_bucket_start) / 2);
    let today_window = server_local_day_window_utc(Local::now());

    insert_summary_window_bucket(&proxy, &key_id, first_full_bucket_start, 10, 7, 2, 1).await;
    insert_summary_window_bucket(&proxy, &key_id, partial_bucket_start, 8, 6, 1, 1).await;
    insert_summary_window_logs(&proxy, &key_id, partial_gap_end, OUTCOME_SUCCESS, 3).await;
    insert_summary_window_logs(&proxy, &key_id, partial_gap_end + 300, OUTCOME_ERROR, 1).await;

    let summary = proxy
        .key_store
        .fetch_success_breakdown(month_start, today_window.start, today_window.end)
        .await
        .expect("success breakdown");
    let public_summary = proxy
        .success_breakdown(Some(today_window))
        .await
        .expect("public success breakdown");
    let current_month_start = start_of_month(Utc::now()).timestamp();
    let expected_public_monthly_success = if month_start >= current_month_start {
        13
    } else {
        0
    };

    assert_eq!(summary.monthly_success, 13);
    assert_eq!(summary.daily_success, 0);
    assert_eq!(public_summary.monthly_success, expected_public_monthly_success);
    assert_eq!(public_summary.daily_success, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_uses_dashboard_rollups_without_scanning_request_logs() {
    let db_path = temp_db_path("public-success-breakdown-dashboard-rollups");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-dashboard-rollups".to_string()],
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

    let now = Utc::now().timestamp();
    let window = TimeRangeUtc {
        start: now.saturating_sub(120),
        end: now.saturating_add(120),
    };

    insert_summary_window_logs(&proxy, &key_id, now.saturating_sub(60), OUTCOME_SUCCESS, 2).await;
    insert_summary_window_logs(&proxy, &key_id, now.saturating_sub(300), OUTCOME_SUCCESS, 3).await;
    insert_summary_window_logs(&proxy, &key_id, now, OUTCOME_ERROR, 1).await;

    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("public rollup success breakdown");

    assert_eq!(public.monthly_success, 5);
    assert_eq!(public.daily_success, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn public_success_breakdown_does_not_double_count_retained_partial_minute() {
    let db_path = temp_db_path("public-success-breakdown-partial-minute");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-public-success-partial-minute".to_string()],
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

    let minute_start = Utc::now()
        .timestamp()
        .saturating_sub(300)
        .div_euclid(60)
        * 60;
    let retained_floor = minute_start + 17;
    let window = TimeRangeUtc {
        start: minute_start.saturating_sub(60),
        end: minute_start.saturating_add(3_600),
    };

    insert_summary_window_bucket(
        &proxy,
        &key_id,
        local_day_bucket_start_utc_ts(minute_start),
        4,
        4,
        0,
        0,
    )
    .await;
    insert_summary_window_logs(&proxy, &key_id, minute_start + 5, OUTCOME_SUCCESS, 1).await;
    insert_summary_window_logs(&proxy, &key_id, retained_floor, OUTCOME_SUCCESS, 2).await;
    insert_summary_window_logs(&proxy, &key_id, minute_start + 63, OUTCOME_SUCCESS, 1).await;
    sqlx::query("DELETE FROM request_logs WHERE created_at = ?")
        .bind(minute_start + 5)
        .execute(&proxy.key_store.pool)
        .await
        .expect("prune pre-floor request log");

    let public = proxy
        .success_breakdown(Some(window))
        .await
        .expect("public rollup success breakdown");
    let current_month_start = start_of_month(Utc::now()).timestamp();
    let expected_monthly_success = if local_day_bucket_start_utc_ts(minute_start) >= current_month_start {
        4
    } else {
        3
    };

    assert_eq!(public.monthly_success, expected_monthly_success);
    assert_eq!(public.daily_success, 4);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_windows_return_zero_for_empty_yesterday_bucket() {
    let db_path = temp_db_path("summary-windows-empty-yesterday");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-b".to_string()],
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

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);
    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_SUCCESS, 4).await;
    insert_summary_window_logs(&proxy, &key_id, today_start + 3600, OUTCOME_ERROR, 1).await;
    insert_summary_window_bucket(&proxy, &key_id, today_start, 5, 4, 1, 0).await;

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");
    assert_eq!(
        summary.yesterday,
        SummaryWindowMetrics {
            quota_charge: SummaryQuotaCharge {
                stale_key_count: 1,
                ..SummaryQuotaCharge::default()
            },
            ..SummaryWindowMetrics::default()
        }
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_hourly_request_window_returns_49_hours_including_current_hour_with_zero_fill() {
    let db_path = temp_db_path("dashboard-hourly-request-window");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-hourly-window".to_string()],
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
    let current_hour_start = start_of_local_hour_utc_ts(evaluation_time.with_timezone(&Local));
    let visible_bucket_start = current_hour_start;
    let previous_day_same_hour_start = visible_bucket_start - 24 * 3600;

    insert_dashboard_hourly_log(
        &proxy,
        &key_id,
        DashboardHourlyLogSeed {
            created_at: visible_bucket_start + 60,
            path: "/mcp",
            request_kind_key: "mcp:tools/list",
            request_kind_label: "MCP | tools/list",
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
            created_at: visible_bucket_start + 120,
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
            created_at: visible_bucket_start + 180,
            path: "/mcp",
            request_kind_key: "mcp:notifications/initialized",
            request_kind_label: "MCP | notifications/initialized",
            result_status: OUTCOME_ERROR,
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
            created_at: visible_bucket_start + 240,
            path: "/mcp",
            request_kind_key: "mcp:search",
            request_kind_label: "MCP | search",
            result_status: OUTCOME_ERROR,
            failure_kind: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            request_body: None,
            visibility: REQUEST_LOG_VISIBILITY_VISIBLE,
        },
    )
    .await;
    insert_dashboard_hourly_log(
        &proxy,
        &key_id,
        DashboardHourlyLogSeed {
            created_at: visible_bucket_start + 300,
            path: "/api/tavily/extract",
            request_kind_key: "api:extract",
            request_kind_label: "API | extract",
            result_status: OUTCOME_ERROR,
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
            created_at: visible_bucket_start + 360,
            path: "/api/tavily/usage",
            request_kind_key: "api:usage",
            request_kind_label: "API | usage",
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
            created_at: visible_bucket_start + 420,
            path: "/api/tavily/unknown",
            request_kind_key: "api:unknown-path",
            request_kind_label: "API | unknown path",
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
            created_at: previous_day_same_hour_start + 60,
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
            created_at: current_hour_start + 30,
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

    let window = proxy
        .dashboard_hourly_request_window_at(evaluation_time)
        .await
        .expect("hourly request window");

    assert_eq!(window.bucket_seconds, 3600);
    assert_eq!(window.visible_buckets, 25);
    assert_eq!(window.retained_buckets, 49);
    assert_eq!(window.buckets.len(), 49);
    assert_eq!(
        window.buckets.first().map(|bucket| bucket.bucket_start),
        Some(current_hour_start - 48 * 3600)
    );
    assert_eq!(
        window.buckets.last().map(|bucket| bucket.bucket_start),
        Some(current_hour_start)
    );

    let zero_bucket = window
        .buckets
        .iter()
        .find(|bucket| bucket.bucket_start == visible_bucket_start - 3600)
        .expect("zero-filled bucket");
    assert_eq!(zero_bucket.primary_success, 0);
    assert_eq!(zero_bucket.api_billable, 0);

    let latest_bucket = window
        .buckets
        .iter()
        .find(|bucket| bucket.bucket_start == visible_bucket_start)
        .expect("latest in-progress bucket");
    assert_eq!(latest_bucket.secondary_success, 2);
    assert_eq!(latest_bucket.primary_success, 2);
    assert_eq!(latest_bucket.secondary_failure, 1);
    assert_eq!(latest_bucket.primary_failure_429, 1);
    assert_eq!(latest_bucket.primary_failure_other, 1);
    assert_eq!(latest_bucket.unknown, 1);
    assert_eq!(latest_bucket.mcp_non_billable, 2);
    assert_eq!(latest_bucket.mcp_billable, 1);
    assert_eq!(latest_bucket.api_non_billable, 2);
    assert_eq!(latest_bucket.api_billable, 3);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_hourly_request_window_classifies_non_billable_mcp_batch_from_body() {
    let db_path = temp_db_path("dashboard-hourly-batch-classification");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-hourly-batch".to_string()],
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
        .with_ymd_and_hms(2026, 4, 7, 12, 2, 0)
        .single()
        .expect("valid utc evaluation time");
    let current_hour_start = start_of_local_hour_utc_ts(evaluation_time.with_timezone(&Local));
    let visible_bucket_start = current_hour_start;

    insert_dashboard_hourly_log(
        &proxy,
        &key_id,
        DashboardHourlyLogSeed {
            created_at: visible_bucket_start + 60,
            path: "/mcp",
            request_kind_key: "mcp:batch",
            request_kind_label: "MCP | batch",
            result_status: OUTCOME_SUCCESS,
            failure_kind: None,
            request_body: Some(br#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"}]"#),
            visibility: REQUEST_LOG_VISIBILITY_VISIBLE,
        },
    )
    .await;

    let window = proxy
        .dashboard_hourly_request_window_at(evaluation_time)
        .await
        .expect("hourly request window");

    let latest_bucket = window
        .buckets
        .iter()
        .find(|bucket| bucket.bucket_start == visible_bucket_start)
        .expect("latest in-progress bucket");
    assert_eq!(latest_bucket.secondary_success, 1);
    assert_eq!(latest_bucket.mcp_non_billable, 1);
    assert_eq!(latest_bucket.mcp_billable, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn dashboard_rollup_bounded_rebuild_is_idempotent() {
    let db_path = temp_db_path("dashboard-rollup-bounded-rebuild");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-dashboard-rollup-bounded".to_string()],
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

    let created_at = Utc
        .with_ymd_and_hms(2026, 4, 7, 12, 34, 56)
        .single()
        .expect("valid timestamp")
        .timestamp();

    sqlx::query(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, 'success', 'api:search', 'API | search', NULL, NULL, '[]', '[]', ?, ?)
        "#,
    )
    .bind(&key_id)
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert request log");

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(Some(created_at), Some(created_at + 1))
        .await
        .expect("first bounded rebuild");
    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(Some(created_at), Some(created_at + 1))
        .await
        .expect("second bounded rebuild");

    let minute_bucket = sqlx::query(
        r#"
        SELECT total_requests, success_count, api_billable
        FROM dashboard_request_rollup_buckets
        WHERE bucket_secs = 60 AND bucket_start = ?
        "#,
    )
    .bind(created_at.div_euclid(60) * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("minute bucket");
    assert_eq!(
        minute_bucket
            .try_get::<i64, _>("total_requests")
            .expect("minute total"),
        1
    );
    assert_eq!(
        minute_bucket
            .try_get::<i64, _>("success_count")
            .expect("minute success"),
        1
    );
    assert_eq!(
        minute_bucket
            .try_get::<i64, _>("api_billable")
            .expect("minute api billable"),
        1
    );

    let day_bucket = sqlx::query(
        r#"
        SELECT total_requests, success_count
        FROM dashboard_request_rollup_buckets
        WHERE bucket_secs = 86400 AND bucket_start = ?
        "#,
    )
    .bind(local_day_bucket_start_utc_ts(created_at))
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("day bucket");
    assert_eq!(
        day_bucket
            .try_get::<i64, _>("total_requests")
            .expect("day total"),
        1
    );
    assert_eq!(
        day_bucket
            .try_get::<i64, _>("success_count")
            .expect("day success"),
        1
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn suppressed_retry_shadow_logs_are_hidden_from_recent_logs_and_summary_windows() {
    let db_path = temp_db_path("summary-windows-suppressed-retry-shadow");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-shadow".to_string()],
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

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);

    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_SUCCESS, 1).await;
    insert_summary_window_logs_with_visibility(
        &proxy,
        &key_id,
        today_start + 120,
        OUTCOME_ERROR,
        1,
        REQUEST_LOG_VISIBILITY_SUPPRESSED_RETRY_SHADOW,
    )
    .await;
    proxy
        .rebuild_api_key_usage_buckets()
        .await
        .expect("rebuild api key usage buckets");

    let recent_logs = proxy
        .recent_request_logs(10)
        .await
        .expect("recent request logs");
    assert_eq!(recent_logs.len(), 1);
    assert_eq!(recent_logs[0].result_status, OUTCOME_SUCCESS);

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");
    assert_eq!(summary.today.total_requests, 1);
    assert_eq!(summary.today.success_count, 1);
    assert_eq!(summary.today.error_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn startup_preserves_existing_usage_buckets_when_request_value_columns_are_added() {
    let db_path = temp_db_path("usage-bucket-request-value-upgrade");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-upgrade".to_string()],
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

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);

    insert_summary_window_bucket(&proxy, &key_id, today_start, 10, 8, 1, 1).await;
    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_SUCCESS, 1).await;

    drop(proxy);

    let pool = open_sqlite_pool(&db_str, false, false)
        .await
        .expect("open sqlite pool");
    let mut conn = pool.acquire().await.expect("acquire sqlite connection");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .execute(&mut *conn)
        .await
        .expect("clear request-value bucket migration marker");
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *conn)
        .await
        .expect("disable foreign keys for legacy bucket rewrite");
    sqlx::query(
        r#"
        CREATE TABLE api_key_usage_buckets_legacy (
            api_key_id TEXT NOT NULL,
            bucket_start INTEGER NOT NULL,
            bucket_secs INTEGER NOT NULL,
            total_requests INTEGER NOT NULL,
            success_count INTEGER NOT NULL,
            error_count INTEGER NOT NULL,
            quota_exhausted_count INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (api_key_id, bucket_start, bucket_secs),
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut *conn)
    .await
    .expect("create legacy usage bucket table");
    sqlx::query(
        r#"
        INSERT INTO api_key_usage_buckets_legacy (
            api_key_id,
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            updated_at
        )
        SELECT
            api_key_id,
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            updated_at
        FROM api_key_usage_buckets
        "#,
    )
    .execute(&mut *conn)
    .await
    .expect("copy usage buckets into legacy schema");
    sqlx::query("DROP TABLE api_key_usage_buckets")
        .execute(&mut *conn)
        .await
        .expect("drop current usage bucket table");
    sqlx::query("ALTER TABLE api_key_usage_buckets_legacy RENAME TO api_key_usage_buckets")
        .execute(&mut *conn)
        .await
        .expect("rename legacy usage bucket table");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *conn)
        .await
        .expect("re-enable foreign keys after legacy bucket rewrite");
    drop(conn);
    drop(pool);

    let reopened = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-upgrade".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy reopened");

    let row = sqlx::query(
        r#"
        SELECT
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            valuable_success_count,
            valuable_failure_count,
            other_success_count,
            other_failure_count,
            unknown_count
        FROM api_key_usage_buckets
        WHERE api_key_id = ? AND bucket_start = ?
        "#,
    )
    .bind(&key_id)
    .bind(today_start)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("bucket row after startup migration");

    assert_eq!(row.try_get::<i64, _>("total_requests").unwrap(), 10);
    assert_eq!(row.try_get::<i64, _>("success_count").unwrap(), 8);
    assert_eq!(row.try_get::<i64, _>("error_count").unwrap(), 1);
    assert_eq!(row.try_get::<i64, _>("quota_exhausted_count").unwrap(), 1);
    assert_eq!(row.try_get::<i64, _>("valuable_success_count").unwrap(), 1);
    assert_eq!(row.try_get::<i64, _>("valuable_failure_count").unwrap(), 0);
    assert_eq!(row.try_get::<i64, _>("other_success_count").unwrap(), 0);
    assert_eq!(row.try_get::<i64, _>("other_failure_count").unwrap(), 0);
    assert_eq!(row.try_get::<i64, _>("unknown_count").unwrap(), 0);

    let request_value_marker: Option<i64> =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ?")
            .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
            .fetch_optional(&reopened.key_store.pool)
            .await
            .expect("load request-value bucket migration marker");
    assert_eq!(request_value_marker, Some(1));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn startup_request_value_backfill_preserves_existing_breakdown_for_pruned_buckets() {
    let db_path = temp_db_path("usage-bucket-request-value-preserve");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-preserve".to_string()],
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

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);
    let older_bucket_start = today_start - 2 * 86_400;

    insert_summary_window_bucket(&proxy, &key_id, older_bucket_start, 10, 8, 1, 1).await;
    insert_summary_window_logs(&proxy, &key_id, older_bucket_start + 60, OUTCOME_SUCCESS, 1).await;

    insert_summary_window_bucket(&proxy, &key_id, today_start, 6, 5, 1, 0).await;
    sqlx::query(
        r#"
        UPDATE api_key_usage_buckets
        SET valuable_success_count = 0,
            valuable_failure_count = 0,
            other_success_count = 0,
            other_failure_count = 0,
            unknown_count = 0
        WHERE api_key_id = ? AND bucket_start = ?
        "#,
    )
    .bind(&key_id)
    .bind(today_start)
    .execute(&proxy.key_store.pool)
    .await
    .expect("zero request-value counts for zero-row backfill case");
    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_ERROR, 1).await;

    drop(proxy);

    let pool = open_sqlite_pool(&db_str, false, false)
        .await
        .expect("open sqlite pool");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_API_KEY_USAGE_BUCKETS_REQUEST_VALUE_V2_DONE)
        .execute(&pool)
        .await
        .expect("clear request-value bucket migration marker");
    drop(pool);

    let reopened = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-preserve".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy reopened");

    let older_row = sqlx::query(
        r#"
        SELECT
            valuable_success_count,
            valuable_failure_count,
            other_success_count,
            other_failure_count,
            unknown_count
        FROM api_key_usage_buckets
        WHERE api_key_id = ? AND bucket_start = ?
        "#,
    )
    .bind(&key_id)
    .bind(older_bucket_start)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("older bucket row after request-value backfill");
    assert_eq!(
        older_row
            .try_get::<i64, _>("valuable_success_count")
            .unwrap(),
        8
    );
    assert_eq!(
        older_row
            .try_get::<i64, _>("valuable_failure_count")
            .unwrap(),
        2
    );
    assert_eq!(
        older_row.try_get::<i64, _>("other_success_count").unwrap(),
        0
    );
    assert_eq!(
        older_row.try_get::<i64, _>("other_failure_count").unwrap(),
        0
    );
    assert_eq!(older_row.try_get::<i64, _>("unknown_count").unwrap(), 0);

    let today_row = sqlx::query(
        r#"
        SELECT
            valuable_success_count,
            valuable_failure_count,
            other_success_count,
            other_failure_count,
            unknown_count
        FROM api_key_usage_buckets
        WHERE api_key_id = ? AND bucket_start = ?
        "#,
    )
    .bind(&key_id)
    .bind(today_start)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("today bucket row after request-value backfill");
    assert_eq!(
        today_row
            .try_get::<i64, _>("valuable_success_count")
            .unwrap(),
        0
    );
    assert_eq!(
        today_row
            .try_get::<i64, _>("valuable_failure_count")
            .unwrap(),
        1
    );
    assert_eq!(
        today_row.try_get::<i64, _>("other_success_count").unwrap(),
        0
    );
    assert_eq!(
        today_row.try_get::<i64, _>("other_failure_count").unwrap(),
        0
    );
    assert_eq!(today_row.try_get::<i64, _>("unknown_count").unwrap(), 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_request_does_not_probe_usage_before_forwarding() {
    let db_path = temp_db_path("research-no-usage-probe");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-research-no-usage-probe-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let usage_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let research_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/usage",
            get({
                let usage_calls = usage_calls.clone();
                move || {
                    let usage_calls = usage_calls.clone();
                    async move {
                        usage_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        (
                            StatusCode::UNAUTHORIZED,
                            Json(serde_json::json!({
                                "error": "invalid api key",
                            })),
                        )
                    }
                }
            }),
        )
        .route(
            "/research",
            post({
                let research_calls = research_calls.clone();
                move |body: bytes::Bytes| {
                    let research_calls = research_calls.clone();
                    async move {
                        research_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let body: Value =
                            serde_json::from_slice(&body).expect("research body is json");
                        assert_eq!(
                            body.get("api_key").and_then(|v| v.as_str()),
                            Some(expected_api_key)
                        );
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "request_id": "mock-research-request",
                                "status": "success",
                            })),
                        )
                    }
                }
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);
    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test research" });

    let (resp, analysis, usage_delta) = proxy
        .proxy_http_research(
            &usage_base,
            Some("tok1"),
            None,
            &Method::POST,
            "/api/tavily/research",
            options,
            &headers,
            false,
        )
        .await
        .expect("research should forward without probing usage");
    assert_eq!(resp.status, StatusCode::OK);
    assert_eq!(analysis.status, "success");
    assert_eq!(usage_delta, None);
    assert_eq!(
        usage_calls.load(std::sync::atomic::Ordering::SeqCst),
        0
    );
    assert_eq!(
        research_calls.load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    let quarantine_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn sync_key_quota_quarantines_usage_auth_failures() {
    let db_path = temp_db_path("sync-usage-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-sync-quarantine".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seeded key id");

    let app = Router::new().route(
        "/usage",
        get(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "detail": {
                        "error": "The account associated with this API key has been deactivated."
                    }
                })),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{addr}");
    let err = proxy
        .sync_key_quota(&key_id, &usage_base, "quota_sync/test")
        .await
        .expect_err("sync should fail");
    match err {
        ProxyError::UsageHttp { status, .. } => assert_eq!(status, StatusCode::UNAUTHORIZED),
        other => panic!("expected usage http error, got {other:?}"),
    }

    let quarantine_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_http_json_endpoint_does_not_inject_bearer_auth_when_disabled() {
    let db_path = temp_db_path("http-json-bearer-disabled");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-bearer-disabled-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let app = Router::new().route(
        "/search",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.to_string();
                async move {
                    let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(api_key, expected_api_key);
                    assert!(
                        headers.get(axum::http::header::AUTHORIZATION).is_none(),
                        "upstream authorization should be absent when injection is disabled"
                    );
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        HeaderValue::from_static("Bearer th-client-token"),
    );
    let options = serde_json::json!({ "query": "test" });

    let _ = proxy
        .proxy_http_json_endpoint(
            &usage_base,
            "/search",
            Some("tok1"),
            None,
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
            false,
        )
        .await
        .expect("proxy request succeeds");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn quota_blocks_after_hourly_limit() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("quota-test");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy.create_access_token(None).await.expect("token");

    let hourly_limit = effective_token_hourly_limit();

    for _ in 0..hourly_limit {
        let verdict = proxy
            .check_token_quota(&token.id)
            .await
            .expect("quota check ok");
        assert!(verdict.allowed, "should be allowed within limit");
    }

    let verdict = proxy
        .check_token_quota(&token.id)
        .await
        .expect("quota check ok");
    assert!(!verdict.allowed, "expected hourly limit to block");
    assert_eq!(verdict.exceeded_window, Some(QuotaWindow::Hour));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn quota_window_name_reports_exhausted_when_at_limit() {
    let verdict = TokenQuotaVerdict::new(2, 2, 0, 10, 0, 100);
    assert!(verdict.allowed, "at-limit is not considered exceeded");
    assert_eq!(verdict.window_name(), Some("hour"));
    assert_eq!(verdict.state_key(), "hour");
}

#[tokio::test]
async fn hourly_any_request_limit_blocks_after_threshold() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("any-limit-test");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("any-limit"))
        .await
        .expect("create token");

    let request_limit = request_rate_limit();

    for _ in 0..request_limit {
        let verdict = proxy
            .check_token_hourly_requests(&token.id)
            .await
            .expect("hourly-any check ok");
        assert!(verdict.allowed, "should be allowed within hourly-any limit");
        assert_eq!(verdict.scope, RequestRateScope::Token);
        assert_eq!(verdict.window_minutes, request_rate_limit_window_minutes());
    }

    let verdict = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("hourly-any check ok");
    assert!(
        !verdict.allowed,
        "expected hourly-any limit to block additional requests"
    );
    assert_eq!(verdict.hourly_limit, request_limit);
    assert_eq!(verdict.scope, RequestRateScope::Token);
    assert_eq!(verdict.window_minutes, request_rate_limit_window_minutes());
    assert!(
        verdict.retry_after_seconds > 0,
        "blocked verdict should expose retry-after"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn hourly_any_request_limit_is_shared_across_bound_tokens_of_same_user() {
    let db_path = temp_db_path("any-limit-bound-user-shared");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "any-limit-shared-user".to_string(),
            username: Some("shared-user".to_string()),
            name: Some("Shared User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let primary_token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:any-limit-primary"))
        .await
        .expect("bind primary token");
    let secondary_seed = proxy
        .create_access_token(Some("linuxdo:any-limit-secondary"))
        .await
        .expect("create secondary token");
    let secondary_token = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:any-limit-secondary"),
            Some(&secondary_seed.id),
        )
        .await
        .expect("bind secondary token");

    let request_limit = request_rate_limit();
    for index in 0..request_limit {
        let token_id = if index % 2 == 0 {
            &primary_token.id
        } else {
            &secondary_token.id
        };
        let verdict = proxy
            .check_token_hourly_requests(token_id)
            .await
            .expect("shared check ok");
        assert!(verdict.allowed, "shared user window should allow request");
        assert_eq!(verdict.scope, RequestRateScope::User);
    }

    let blocked = proxy
        .check_token_hourly_requests(&secondary_token.id)
        .await
        .expect("shared limit block");
    assert!(
        !blocked.allowed,
        "same owner should share one limiter window"
    );
    assert_eq!(blocked.scope, RequestRateScope::User);
    assert_eq!(blocked.hourly_limit, request_limit);
    assert!(blocked.retry_after_seconds > 0);

    let dashboard = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("shared user dashboard");
    assert_eq!(dashboard.request_rate.scope, RequestRateScope::User);
    assert_eq!(dashboard.request_rate.used, request_limit);
    assert_eq!(dashboard.request_rate.limit, request_limit);
    assert_eq!(
        dashboard.request_rate.window_minutes,
        request_rate_limit_window_minutes()
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn hourly_any_request_limit_keeps_unbound_tokens_isolated() {
    let db_path = temp_db_path("any-limit-unbound-isolated");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let primary = proxy
        .create_access_token(Some("unbound-primary"))
        .await
        .expect("primary token");
    let secondary = proxy
        .create_access_token(Some("unbound-secondary"))
        .await
        .expect("secondary token");

    let request_limit = request_rate_limit();
    for _ in 0..request_limit {
        let verdict = proxy
            .check_token_hourly_requests(&primary.id)
            .await
            .expect("primary check ok");
        assert!(verdict.allowed);
        assert_eq!(verdict.scope, RequestRateScope::Token);
    }

    let blocked = proxy
        .check_token_hourly_requests(&primary.id)
        .await
        .expect("primary block");
    assert!(!blocked.allowed);
    assert_eq!(blocked.scope, RequestRateScope::Token);

    let secondary_verdict = proxy
        .check_token_hourly_requests(&secondary.id)
        .await
        .expect("secondary isolated check");
    assert!(
        secondary_verdict.allowed,
        "unbound token should keep an independent limiter window"
    );
    assert_eq!(secondary_verdict.scope, RequestRateScope::Token);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn hourly_any_request_limit_evicts_idle_subjects_after_window() {
    let db_path = temp_db_path("any-limit-idle-eviction");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("idle-eviction"))
        .await
        .expect("token created");

    let verdict = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("initial check ok");
    assert!(verdict.allowed);
    assert_eq!(proxy.debug_token_request_limiter_subject_count().await, 1);

    let future_ts = Utc::now().timestamp() + request_rate_limit_window_secs() + 1;
    proxy
        .debug_prune_idle_token_request_subjects_at(future_ts)
        .await;
    assert_eq!(proxy.debug_token_request_limiter_subject_count().await, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn delete_access_token_soft_deletes_and_hides_from_list() {
    let db_path = temp_db_path("token-delete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let token = proxy
        .create_access_token(Some("soft-delete-test"))
        .await
        .expect("create token");

    // Sanity check: token is visible before delete.
    let tokens_before = proxy
        .list_access_tokens()
        .await
        .expect("list tokens before delete");
    assert!(
        tokens_before.iter().any(|t| t.id == token.id),
        "token should appear in list before delete"
    );

    // Inspect raw row to confirm it's enabled and not deleted.
    let store = proxy.key_store.clone();
    let (enabled_before, deleted_at_before): (i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(&token.id)
            .fetch_one(&store.pool)
            .await
            .expect("token row exists before delete");
    assert_eq!(enabled_before, 1);
    assert!(
        deleted_at_before.is_none(),
        "deleted_at should be NULL before delete"
    );

    // Perform delete via public API (soft delete).
    proxy
        .delete_access_token(&token.id)
        .await
        .expect("delete token");

    // Row still exists but marked disabled and soft-deleted.
    let (enabled_after, deleted_at_after): (i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(&token.id)
            .fetch_one(&store.pool)
            .await
            .expect("token row exists after delete");
    assert_eq!(enabled_after, 0, "token should be disabled after delete");
    assert!(
        deleted_at_after.is_some(),
        "deleted_at should be set after delete"
    );

    // Token is no longer returned from management listing.
    let tokens_after = proxy
        .list_access_tokens()
        .await
        .expect("list tokens after delete");
    assert!(
        tokens_after.iter().all(|t| t.id != token.id),
        "soft-deleted token should not appear in list"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_counts_only_billable_logs() {
    let db_path = temp_db_path("rollup-billable");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-billable"))
        .await
        .expect("create token");

    let store = proxy.key_store.clone();
    let base_ts = 1_700_000_000i64;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(base_ts)
    .execute(&store.pool)
    .await
    .expect("insert billable log");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 0, ?)
        "#,
    )
    .bind(&token.id)
    .bind(base_ts + 10)
    .execute(&store.pool)
    .await
    .expect("insert nonbillable log");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");

    let (success, system, external, quota): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT success_count, system_failure_count, external_failure_count, quota_exhausted_count FROM token_usage_stats WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&store.pool)
    .await
    .expect("stats row after first rollup");
    assert_eq!(success, 1, "should count only billable logs");
    assert_eq!(
        system + external + quota,
        0,
        "no failure counts expected in this test"
    );

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(base_ts + 20)
    .execute(&store.pool)
    .await
    .expect("insert second billable log");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");

    let (success_after,): (i64,) =
        sqlx::query_as("SELECT success_count FROM token_usage_stats WHERE token_id = ?")
            .bind(&token.id)
            .fetch_one(&store.pool)
            .await
            .expect("stats row after second rollup");
    assert_eq!(
        success_after, 2,
        "bucket should grow by billable increments"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_is_idempotent_without_new_logs() {
    let db_path = temp_db_path("rollup-idempotent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-idempotent"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let ts = 1_700_001_000i64;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(ts)
    .execute(&store.pool)
    .await
    .expect("insert billable log");

    let first = proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");
    assert!(first.0 > 0, "first rollup should process at least one row");

    let after_first = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after first rollup");
    assert_eq!(after_first.total_requests, 1);
    assert_eq!(after_first.success_count, 1);

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");
    assert_eq!(second.0, 0, "second rollup should be a no-op");
    assert!(second.1.is_none(), "second rollup should return no max ts");

    let after_second = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after second rollup");
    assert_eq!(after_second.total_requests, 1);
    assert_eq!(after_second.success_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_processes_same_second_log_once() {
    let db_path = temp_db_path("rollup-same-second");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-same-second"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let ts = 1_700_002_000i64;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(ts)
    .execute(&store.pool)
    .await
    .expect("insert first log");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(ts)
    .execute(&store.pool)
    .await
    .expect("insert second log with same second");

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");
    assert!(second.0 > 0, "second rollup should process the new row");

    let after_second = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after second rollup");
    assert_eq!(after_second.total_requests, 2);
    assert_eq!(after_second.success_count, 2);

    let third = proxy
        .rollup_token_usage_stats()
        .await
        .expect("third rollup");
    assert_eq!(third.0, 0, "third rollup should be a no-op");

    let after_third = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after third rollup");
    assert_eq!(after_third.total_requests, 2);
    assert_eq!(after_third.success_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_migrates_legacy_timestamp_cursor() {
    let db_path = temp_db_path("rollup-legacy-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-cursor"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let base_ts = 1_700_003_000i64;

    for offset in [0_i64, 10, 20] {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(base_ts + offset)
        .execute(&store.pool)
        .await
        .expect("insert log");
    }

    // Simulate pre-v2 state with only the legacy timestamp cursor present.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .execute(&store.pool)
        .await
        .expect("delete v2 cursor");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
    .bind((base_ts + 10).to_string())
    .execute(&store.pool)
    .await
    .expect("set legacy cursor");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("rollup with migrated cursor");

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after migrated rollup");
    assert_eq!(
        summary.total_requests, 2,
        "migration should include boundary-second rows to avoid undercount on legacy_ts"
    );
    assert_eq!(summary.success_count, 2);

    let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("max log id")
    .expect("max log id should exist");
    let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .fetch_one(&store.pool)
        .await
        .expect("v2 cursor exists");
    let cursor_v2 = cursor_v2_raw
        .parse::<i64>()
        .expect("v2 cursor should be numeric");
    assert_eq!(cursor_v2, expected_last_id);

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup after migration");
    assert_eq!(second.0, 0, "should not reprocess previous logs");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_migration_handles_out_of_order_timestamps() {
    let db_path = temp_db_path("rollup-legacy-cursor-out-of-order");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-cursor-out-of-order"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let legacy_ts = 1_700_020_000i64;

    // Insert a newer log first, then an older-timestamp log second to create id/timestamp disorder.
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(legacy_ts + 100)
    .execute(&store.pool)
    .await
    .expect("insert newer log first");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(legacy_ts - 100)
    .execute(&store.pool)
    .await
    .expect("insert older log second");

    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .execute(&store.pool)
        .await
        .expect("delete v2 cursor");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
    .bind(legacy_ts.to_string())
    .execute(&store.pool)
    .await
    .expect("set legacy cursor");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("rollup with out-of-order migration");

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after migration");
    assert_eq!(
        summary.total_requests, 1,
        "migration should include all logs newer than legacy_ts even when id/timestamp are out of order"
    );
    assert_eq!(summary.success_count, 1);

    let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("max log id")
    .expect("max log id should exist");
    let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .fetch_one(&store.pool)
        .await
        .expect("v2 cursor exists");
    let cursor_v2 = cursor_v2_raw
        .parse::<i64>()
        .expect("v2 cursor should be numeric");
    assert_eq!(cursor_v2, expected_last_id);

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup after migration");
    assert_eq!(second.0, 0, "second rollup should be a no-op");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_migration_includes_same_second_boundary_logs() {
    let db_path = temp_db_path("rollup-legacy-cursor-same-second");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-cursor-same-second"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let legacy_ts = 1_700_030_000i64;

    for _ in 0..2 {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(legacy_ts)
        .execute(&store.pool)
        .await
        .expect("insert same-second log");
    }

    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .execute(&store.pool)
        .await
        .expect("delete v2 cursor");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
    .bind(legacy_ts.to_string())
    .execute(&store.pool)
    .await
    .expect("set legacy cursor");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("rollup with same-second migration boundary");

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after migration");
    assert_eq!(
        summary.total_requests, 2,
        "migration must not miss logs at the same second as legacy_ts"
    );
    assert_eq!(summary.success_count, 2);

    let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("max log id")
    .expect("max log id should exist");
    let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .fetch_one(&store.pool)
        .await
        .expect("v2 cursor exists");
    let cursor_v2 = cursor_v2_raw
        .parse::<i64>()
        .expect("v2 cursor should be numeric");
    assert_eq!(cursor_v2, expected_last_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_keeps_legacy_timestamp_cursor_monotonic() {
    let db_path = temp_db_path("rollup-legacy-ts-monotonic");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-ts-monotonic"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let newer_ts = 1_700_010_000i64;
    let older_ts = newer_ts - 3_600;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(newer_ts)
    .execute(&store.pool)
    .await
    .expect("insert newer log first");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");

    let first_legacy_ts_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .fetch_one(&store.pool)
        .await
        .expect("legacy cursor exists after first rollup");
    let first_legacy_ts = first_legacy_ts_raw
        .parse::<i64>()
        .expect("legacy ts should be numeric");
    assert_eq!(first_legacy_ts, newer_ts);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(older_ts)
    .execute(&store.pool)
    .await
    .expect("insert older log with newer id");

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");
    assert_eq!(
        second.1,
        Some(newer_ts),
        "reported last_rollup_ts should stay aligned with the clamped legacy cursor"
    );

    let second_legacy_ts_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .fetch_one(&store.pool)
        .await
        .expect("legacy cursor exists after second rollup");
    let second_legacy_ts = second_legacy_ts_raw
        .parse::<i64>()
        .expect("legacy ts should be numeric");
    assert_eq!(
        second_legacy_ts, newer_ts,
        "legacy ts must not regress when processed logs have older timestamps"
    );

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after second rollup");
    assert_eq!(summary.total_requests, 2);
    assert_eq!(summary.success_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn heal_orphan_auth_tokens_from_logs_creates_soft_deleted_token() {
    let db_path = temp_db_path("heal-orphan");
    let db_str = db_path.to_string_lossy().to_string();

    // Initialize schema.
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    // Insert an auth_token_logs entry for a token id that does not exist in auth_tokens.
    let orphan_token_id = "ZZZZ";
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1234567890)
        "#,
    )
    .bind(orphan_token_id)
    .execute(&store.pool)
    .await
    .expect("insert orphan log");

    // Clear healer meta key so that we can invoke the healer path again for this test.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_HEAL_ORPHAN_TOKENS_V1)
        .execute(&store.pool)
        .await
        .expect("delete meta gate");

    // Run healer directly.
    store
        .heal_orphan_auth_tokens_from_logs()
        .await
        .expect("heal orphan tokens");

    // Verify that a soft-deleted auth_tokens row was created for the orphan id.
    let (enabled, total_requests, deleted_at): (i64, i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, total_requests, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(orphan_token_id)
            .fetch_one(&store.pool)
            .await
            .expect("restored token row");

    assert_eq!(enabled, 0, "restored token should be disabled");
    assert_eq!(
        total_requests, 1,
        "restored token should count orphan log entries"
    );
    assert!(
        deleted_at.is_some(),
        "restored token should be marked soft-deleted"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_is_single_use() {
    let db_path = temp_db_path("oauth-state-single-use");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state("linuxdo", Some("/"), 120)
        .await
        .expect("create oauth state");
    let first = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state first");
    let second = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state second");

    assert_eq!(first, Some(Some("/".to_string())));
    assert_eq!(second, None, "oauth state must be single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_binding_hash_must_match() {
    let db_path = temp_db_path("oauth-state-binding-hash");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding("linuxdo", Some("/"), 120, Some("nonce-hash-a"))
        .await
        .expect("create oauth state");

    let wrong_hash = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-b"))
        .await
        .expect("consume oauth state with wrong hash");
    assert_eq!(wrong_hash, None, "wrong hash must not consume oauth state");

    let matched = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state with matching hash");
    assert_eq!(matched, Some(Some("/".to_string())));

    let reused = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state reused");
    assert_eq!(reused, None, "oauth state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_payload_carries_bind_token_id() {
    let db_path = temp_db_path("oauth-state-bind-token-id");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding_and_token(
            "linuxdo",
            Some("/console"),
            120,
            Some("nonce-hash-a"),
            Some("a1b2"),
        )
        .await
        .expect("create oauth state");

    let payload = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state")
        .expect("payload exists");

    assert_eq!(payload.redirect_to.as_deref(), Some("/console"));
    assert_eq!(payload.bind_token_id.as_deref(), Some("a1b2"));

    let consumed_again = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state second");
    assert!(consumed_again.is_none(), "state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

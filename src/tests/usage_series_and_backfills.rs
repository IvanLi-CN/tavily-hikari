use super::*;

#[tokio::test]
async fn system_settings_safe_defaults_disable_rollouts() {
    let db_path = temp_db_path("system-settings-api-rebalance-defaults");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = proxy
        .get_system_settings()
        .await
        .expect("get system settings");

    assert!(!settings.api_rebalance_enabled);
    assert_eq!(settings.api_rebalance_percent, 0);
    assert!(!settings.recharge_feature_enabled);
    assert!(!settings.recharge_user_enabled);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_stats_coalescer_flushes_rate5m_series_on_read() {
    let db_path = temp_db_path("request-stats-coalescer-rate5m-flush");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "rate5m-flush".to_string(),
            username: Some("rate5m_flush".to_string()),
            name: Some("Rate5m Flush".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let now = Utc::now();
    let current_bucket_start = now.timestamp() - now.timestamp().rem_euclid(SECS_PER_FIVE_MINUTES);
    let chart_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(chart_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START,
            chart_start,
        )
        .await
        .expect("set rate5m coverage");

    proxy
        .key_store
        .request_stats_coalescer
        .enqueue_auth_token_activity(
            "rate5m-flush-token",
            Some(&user.user_id),
            current_bucket_start + 30,
        )
        .await;

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load rate5m series");
    let current_bucket = series
        .points
        .iter()
        .find(|point| point.bucket_start == current_bucket_start)
        .expect("current bucket point");
    assert_eq!(current_bucket.value, Some(1));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_backfills_full_month_chart_horizon() {
    let db_path = temp_db_path("account-usage-rollup-month-chart-horizon");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "month-chart-horizon".to_string(),
            username: Some("month_chart_horizon".to_string()),
            name: Some("Month Chart Horizon".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("month-chart-horizon"))
        .await
        .expect("bind token");

    let old_month_bucket = shift_month_start_utc_ts(start_of_month(Utc::now()).timestamp(), -10);
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            result_status,
            error_message,
            created_at,
            counts_business_quota,
            business_credits,
            billing_subject,
            billing_state,
            request_user_id
        ) VALUES (?, 'POST', '/api/tavily/search', NULL, 200, 200, ?, NULL, ?, 1, ?, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind(OUTCOME_SUCCESS)
    .bind(old_month_bucket + SECS_PER_HOUR)
    .bind(7_i64)
    .bind(format!("account:{}", user.user_id))
    .bind(BILLING_STATE_CHARGED)
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert old monthly account log");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::QuotaMonth)
        .await
        .expect("load monthly series");

    let point = series
        .points
        .iter()
        .find(|point| point.bucket_start == old_month_bucket)
        .expect("old month bucket present");
    assert_eq!(point.value, Some(7));

    let old_hour_bucket = old_month_bucket + SECS_PER_HOUR;
    let old_day_bucket = local_day_bucket_start_utc_ts(old_month_bucket + SECS_PER_HOUR);
    let hourly_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::Hour,
            old_hour_bucket,
            old_hour_bucket + SECS_PER_HOUR,
        )
        .await
        .expect("load old hourly rollups");
    let daily_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::Day,
            old_day_bucket,
            shift_local_day_start_utc_ts(old_day_bucket, 1),
        )
        .await
        .expect("load old daily rollups");

    assert!(hourly_values.is_empty());
    assert!(daily_values.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_dashboard_overview_request_rate_progress_tracks_live_window() {
    let db_path = temp_db_path("user-dashboard-overview-request-rate-progress");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "overview-request-rate-progress".to_string(),
            username: Some("overview_request_rate_progress".to_string()),
            name: Some("Overview Request Rate Progress".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("overview-request-rate-progress"))
        .await
        .expect("bind token");

    for _ in 0..3 {
        proxy
            .check_token_hourly_requests(&token.id)
            .await
            .expect("record live request");
    }

    let overview = proxy
        .user_dashboard_overview(&user.user_id, None)
        .await
        .expect("load user dashboard overview");

    assert_eq!(overview.summary.request_rate.used, 3);
    assert_eq!(overview.progress.request_rate.used, 3);
    assert_eq!(
        overview.progress.request_rate.points.len(),
        (request_rate_limit_window_secs() / 5) as usize
    );
    assert_eq!(
        overview
            .progress
            .request_rate
            .points
            .last()
            .and_then(|point| point.value),
        Some(3)
    );
    assert_eq!(
        overview
            .progress
            .request_rate
            .points
            .last()
            .and_then(|point| point.limit_value),
        Some(request_rate_limit())
    );
    assert!(
        overview
            .progress
            .request_rate
            .points
            .windows(2)
            .all(|window| window[0].value.unwrap_or(0) <= window[1].value.unwrap_or(0))
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_dashboard_overview_quota_progress_preserves_future_slots_and_utc_month_days() {
    let db_path = temp_db_path("user-dashboard-overview-quota-progress");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "overview-quota-progress".to_string(),
            username: Some("overview_quota_progress".to_string()),
            name: Some("Overview Quota Progress".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("overview-quota-progress"))
        .await
        .expect("bind token");

    proxy
        .update_account_business_quota_limits(&user.user_id, 120, 900, 9_000)
        .await
        .expect("set custom quota");

    let now_ts = Utc::now().timestamp();
    let current_hour_start = now_ts - now_ts.rem_euclid(SECS_PER_HOUR);
    let current_five_minute_start = now_ts - now_ts.rem_euclid(SECS_PER_FIVE_MINUTES);
    let current_utc_day_start = utc_day_bucket_start_utc_ts(now_ts);
    let current_local_day_start = local_day_bucket_start_utc_ts(now_ts);
    let first_charge_at = current_hour_start;
    let latest_charge_at = current_five_minute_start.saturating_add(1).min(now_ts);

    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(current_hour_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");

    for (created_at, credits) in [(first_charge_at, 2_i64), (latest_charge_at, 3_i64)] {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                result_status,
                error_message,
                created_at,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                request_user_id
            ) VALUES (?, 'POST', '/api/tavily/search', NULL, 200, 200, ?, NULL, ?, 1, ?, ?, ?, ?)
            "#,
        )
        .bind(&token.id)
        .bind(OUTCOME_SUCCESS)
        .bind(created_at)
        .bind(credits)
        .bind(format!("account:{}", user.user_id))
        .bind(BILLING_STATE_CHARGED)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert charged quota log");
    }

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let utc_day_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::UtcDay,
            current_utc_day_start,
            current_utc_day_start + SECS_PER_DAY,
        )
        .await
        .expect("load utc-day rollups");
    assert_eq!(utc_day_values.get(&current_utc_day_start), Some(&5));

    let local_day_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::BusinessCredits,
            AccountUsageRollupBucketKind::Day,
            current_local_day_start,
            shift_local_day_start_utc_ts(current_local_day_start, 1),
        )
        .await
        .expect("load local-day rollups");
    assert_eq!(local_day_values.get(&current_local_day_start), Some(&5));

    let overview = proxy
        .user_dashboard_overview(&user.user_id, None)
        .await
        .expect("load user dashboard overview");

    let hourly_current_index = overview
        .progress
        .quota_hourly
        .points
        .iter()
        .rposition(|point| point.value.is_some())
        .expect("hourly current point");
    assert_eq!(
        overview.progress.quota_hourly.points[hourly_current_index].value,
        Some(5)
    );
    assert!(
        overview.progress.quota_hourly.points[hourly_current_index + 1..]
            .iter()
            .all(|point| point.value.is_none())
    );

    let daily_current_index = overview
        .progress
        .quota_daily
        .points
        .iter()
        .rposition(|point| point.value.is_some())
        .expect("daily current point");
    assert_eq!(
        overview.progress.quota_daily.points[daily_current_index].value,
        Some(5)
    );
    assert!(
        overview.progress.quota_daily.points[daily_current_index + 1..]
            .iter()
            .all(|point| point.value.is_none())
    );

    let monthly_current_index = overview
        .progress
        .quota_monthly
        .points
        .iter()
        .rposition(|point| point.value.is_some())
        .expect("monthly current point");
    assert_eq!(
        overview.progress.quota_monthly.points[monthly_current_index].value,
        Some(5)
    );
    assert!(
        overview.progress.quota_monthly.points[monthly_current_index + 1..]
            .iter()
            .all(|point| point.value.is_none())
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_preserves_history_for_existing_custom_request_limit() {
    let db_path = temp_db_path("account-limit-snapshot-backfill-custom-request-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "custom-request-limit-gap".to_string(),
            username: Some("custom_request_limit_gap".to_string()),
            name: Some("Custom Request Limit Gap".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("get system settings");
    settings.request_rate_limit = 80;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("set custom request rate");
    sqlx::query("DELETE FROM request_rate_limit_snapshots")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request snapshots");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let chart_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(chart_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START,
            chart_start,
        )
        .await
        .expect("set rate5m coverage start");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear snapshot backfill marker");

    proxy
        .key_store
        .backfill_account_limit_snapshot_history_v1()
        .await
        .expect("backfill request limit snapshot history");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load rate5m series");

    assert_eq!(series.limit, 80);
    assert_eq!(series.points.len(), 288);
    assert_eq!(
        series.points.first().and_then(|point| point.limit_value),
        Some(80)
    );
    assert_eq!(series.points[287].limit_value, Some(80));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_treats_absent_request_limit_setting_as_long_term_default()
{
    let db_path = temp_db_path("account-limit-snapshot-backfill-absent-request-limit");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "default-request-limit-backfill".to_string(),
            username: Some("default_request_limit_backfill".to_string()),
            name: Some("Default Request Limit Backfill".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let default_limit = request_rate_limit();
    sqlx::query("DELETE FROM request_rate_limit_snapshots")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request snapshots");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let chart_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(chart_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START,
            chart_start,
        )
        .await
        .expect("set rate5m coverage start");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear snapshot backfill marker");

    proxy
        .key_store
        .backfill_account_limit_snapshot_history_v1()
        .await
        .expect("backfill request limit snapshot history");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load rate5m series");

    assert_eq!(series.limit, default_limit);
    assert_eq!(
        series.points.first().and_then(|point| point.limit_value),
        Some(default_limit)
    );
    assert_eq!(series.points[287].limit_value, Some(default_limit));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_treats_persisted_default_request_limit_as_long_term_history()
 {
    let db_path = temp_db_path("account-limit-snapshot-backfill-persisted-default-request-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "persisted-default-request-gap".to_string(),
            username: Some("persisted_default_request_gap".to_string()),
            name: Some("Persisted Default Request Gap".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let default_limit = request_rate_limit();
    let settings = SystemSettings {
        request_rate_limit: default_limit,
        ..proxy
            .get_system_settings()
            .await
            .expect("get system settings")
    };
    proxy
        .set_system_settings(&settings)
        .await
        .expect("persist default request rate");
    sqlx::query("DELETE FROM request_rate_limit_snapshots")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request snapshots");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let chart_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(chart_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START,
            chart_start,
        )
        .await
        .expect("set rate5m coverage start");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear snapshot backfill marker");

    proxy
        .key_store
        .backfill_account_limit_snapshot_history_v1()
        .await
        .expect("backfill request limit snapshot history");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load rate5m series");

    assert_eq!(series.limit, default_limit);
    assert_eq!(
        series.points.first().and_then(|point| point.limit_value),
        Some(default_limit)
    );
    assert_eq!(series.points[287].limit_value, Some(default_limit));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_preserves_gaps_before_user_signup() {
    let db_path = temp_db_path("admin-user-usage-series-signup-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-signup-gap".to_string(),
            username: Some("usage_series_signup_gap".to_string()),
            name: Some("Usage Series Signup Gap".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::QuotaMonth)
        .await
        .expect("load monthly usage series");

    assert_eq!(series.points.len(), 12);
    assert!(series.points.iter().all(|point| point.value.is_none()));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_preserves_partially_covered_first_bucket_as_gap() {
    let db_path = temp_db_path("admin-user-usage-series-partial-coverage-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-partial-coverage-gap".to_string(),
            username: Some("usage_series_partial_coverage_gap".to_string()),
            name: Some("Usage Series Partial Coverage Gap".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_HOUR);
    let start = current_bucket_start - 71 * SECS_PER_HOUR;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");

    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START,
            start + 600,
        )
        .await
        .expect("set partial quota1h coverage");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Quota1h)
        .await
        .expect("load quota1h usage series");

    assert_eq!(series.points.len(), 72);
    assert_eq!(series.points[0].value, None);
    assert_eq!(series.points[1].value, Some(0));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_preserves_limit_line_before_user_signup() {
    let db_path = temp_db_path("admin-user-usage-series-signup-limit-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-signup-limit-gap".to_string(),
            username: Some("usage_series_signup_limit_gap".to_string()),
            name: Some("Usage Series Signup Limit Gap".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_HOUR);
    let start = current_bucket_start - 71 * SECS_PER_HOUR;
    let signup_at = start + 30 * SECS_PER_MINUTE;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(signup_at)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear auto quota snapshots");
    sqlx::query(
        r#"INSERT INTO account_quota_limit_snapshots
           (user_id, changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit)
           VALUES (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&user.user_id)
    .bind(signup_at)
    .bind(100_i64)
    .bind(120_i64)
    .bind(300_i64)
    .bind(2_000_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed deterministic quota snapshot");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Quota1h)
        .await
        .expect("load quota1h usage series");

    assert_eq!(series.points.len(), 72);
    assert_eq!(series.points[0].value, None);
    assert_eq!(series.points[0].limit_value, Some(120));
    assert_eq!(series.points[1].limit_value, Some(120));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_treats_unchanged_default_quota_as_long_term_history() {
    let db_path = temp_db_path("account-limit-snapshot-backfill-default-quota-long-term");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "default-quota-long-term".to_string(),
            username: Some("default_quota_long_term".to_string()),
            name: Some("Default Quota Long Term".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_HOUR);
    let chart_start = current_bucket_start - 71 * SECS_PER_HOUR;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(chart_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");

    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear quota snapshots");
    sqlx::query("DELETE FROM account_quota_limits WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear quota row");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START,
            chart_start,
        )
        .await
        .expect("set quota1h coverage start");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear snapshot backfill marker");

    proxy
        .key_store
        .backfill_account_limit_snapshot_history_v1()
        .await
        .expect("backfill quota snapshot history");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Quota1h)
        .await
        .expect("load quota1h series");

    assert_eq!(
        series.points.first().and_then(|point| point.limit_value),
        Some(series.limit)
    );
    assert_eq!(
        series.points.last().and_then(|point| point.limit_value),
        Some(series.limit)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_preserves_gaps_for_existing_custom_quota_row() {
    let db_path = temp_db_path("account-limit-snapshot-backfill-custom-quota-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "custom-quota-gap".to_string(),
            username: Some("custom_quota_gap".to_string()),
            name: Some("Custom Quota Gap".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_HOUR);
    let chart_start = current_bucket_start - 71 * SECS_PER_HOUR;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(chart_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");

    proxy
        .update_account_business_quota_limits(&user.user_id, 480, 4_800, 48_000)
        .await
        .expect("set custom quota");
    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear quota snapshots");
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START,
            chart_start,
        )
        .await
        .expect("set quota1h coverage start");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear snapshot backfill marker");

    proxy
        .key_store
        .backfill_account_limit_snapshot_history_v1()
        .await
        .expect("backfill quota snapshot history");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Quota1h)
        .await
        .expect("load quota1h series");

    assert_eq!(
        series.points.first().and_then(|point| point.limit_value),
        None
    );
    assert_eq!(
        series.points.last().and_then(|point| point.limit_value),
        Some(series.limit)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_blocked_key_limit_uses_global_base_plus_hidden_delta() {
    let db_path = temp_db_path("user-blocked-key-limit-base-delta");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "blocked-key-limit-user".to_string(),
            username: Some("blocked_key_limit_user".to_string()),
            name: Some("Blocked Key Limit User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("get system settings");
    settings.user_blocked_key_base_limit = 7;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("set blocked-key base limit");
    proxy
        .key_store
        .update_account_monthly_broken_limit(&user.user_id, 4)
        .await
        .expect("set effective blocked-key limit");

    assert_eq!(
        proxy
            .key_store
            .fetch_account_monthly_broken_limit(&user.user_id)
            .await
            .expect("fetch effective limit"),
        4
    );

    settings.user_blocked_key_base_limit = 10;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("raise blocked-key base limit");
    assert_eq!(
        proxy
            .key_store
            .fetch_account_monthly_broken_limit(&user.user_id)
            .await
            .expect("fetch shifted effective limit"),
        7
    );

    proxy
        .key_store
        .update_account_monthly_broken_limit(&user.user_id, 0)
        .await
        .expect("set negative delta clamped effective limit");
    assert_eq!(
        proxy
            .key_store
            .fetch_account_monthly_broken_limit(&user.user_id)
            .await
            .expect("fetch clamped limit"),
        0
    );
}

#[tokio::test]
async fn monthly_blocked_key_count_excludes_quota_exhausted_and_counts_only_blocked_quarantines() {
    let db_path = temp_db_path("monthly-blocked-key-count-excludes-quota");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "blocked-key-count-user".to_string(),
            username: Some("blocked_key_count_user".to_string()),
            name: Some("Blocked Key Count User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let (exhausted_key_id, _) = proxy
        .add_or_undelete_key_with_status("tvly-blocked-count-exhausted")
        .await
        .expect("create exhausted key");
    let (quota_quarantine_key_id, _) = proxy
        .add_or_undelete_key_with_status("tvly-blocked-count-quota-quarantine")
        .await
        .expect("create quota quarantine key");
    let (blocked_key_id, _) = proxy
        .add_or_undelete_key_with_status("tvly-blocked-count-account-deactivated")
        .await
        .expect("create blocked key");
    let (quota_then_blocked_key_id, _) = proxy
        .add_or_undelete_key_with_status("tvly-blocked-count-quota-then-blocked")
        .await
        .expect("create quota-then-blocked key");

    let now = Utc::now().timestamp();
    let month_start = start_of_month(Utc::now()).timestamp();
    sqlx::query("UPDATE api_keys SET status = ? WHERE id = ?")
        .bind(STATUS_EXHAUSTED)
        .bind(&exhausted_key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("mark key exhausted");
    sqlx::query(
        r#"INSERT INTO api_key_quarantines
           (id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
           VALUES (?, ?, 'system', 'quota_exhausted', 'Upstream quota exhausted', 'not a blocked key', ?, NULL),
                  (?, ?, 'system', 'account_deactivated', 'Upstream account deactivated', 'blocked key', ?, NULL),
                  (?, ?, 'system', 'account_deactivated', 'Upstream account deactivated', 'blocked key after quota', ?, NULL)"#,
    )
    .bind("blocked-count-quota-quarantine")
    .bind(&quota_quarantine_key_id)
    .bind(now)
    .bind("blocked-count-account-deactivated")
    .bind(&blocked_key_id)
    .bind(now)
    .bind("blocked-count-quota-then-blocked")
    .bind(&quota_then_blocked_key_id)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed quarantines");

    for (key_id, status, reason_code, reason_summary) in [
        (
            &exhausted_key_id,
            STATUS_EXHAUSTED,
            "quota_exhausted",
            "Upstream quota exhausted",
        ),
        (
            &quota_quarantine_key_id,
            KEY_EFFECT_QUARANTINED,
            "quota_exhausted",
            "Upstream quota exhausted",
        ),
        (
            &blocked_key_id,
            KEY_EFFECT_QUARANTINED,
            "account_deactivated",
            "Upstream account deactivated",
        ),
        (
            &quota_then_blocked_key_id,
            STATUS_EXHAUSTED,
            "quota_exhausted",
            "Old upstream quota exhausted",
        ),
    ] {
        sqlx::query(
            r#"INSERT INTO subject_key_breakages (
                subject_kind, subject_id, key_id, month_start, created_at, updated_at,
                latest_break_at, key_status, reason_code, reason_summary, source,
                breaker_token_id, breaker_user_id, breaker_user_display_name, manual_actor_display_name
            ) VALUES ('user', ?, ?, ?, ?, ?, ?, ?, ?, ?, 'auto', NULL, ?, NULL, NULL)"#,
        )
        .bind(&user.user_id)
        .bind(key_id)
        .bind(month_start)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(status)
        .bind(reason_code)
        .bind(reason_summary)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed subject key breakage");
    }

    let counts = proxy
        .key_store
        .fetch_monthly_broken_counts_for_users(std::slice::from_ref(&user.user_id), month_start)
        .await
        .expect("fetch blocked-key counts");
    assert_eq!(counts.get(&user.user_id).copied(), Some(1));

    let page = proxy
        .key_store
        .fetch_monthly_broken_keys_page("user", &user.user_id, 1, 20, month_start)
        .await
        .expect("fetch blocked-key details");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].key_id, blocked_key_id);
    assert_eq!(
        page.items[0].reason_code.as_deref(),
        Some("account_deactivated")
    );
}

#[tokio::test]
async fn startup_does_not_backfill_request_log_user_snapshots() {
    let db_path = temp_db_path("startup-skips-request-user-id-backfill");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "startup-skips-request-user-backfill".to_string(),
            username: Some("startup_skips_request_user_backfill".to_string()),
            name: Some("Startup Skips Request User Backfill".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("startup-skips-request-user-backfill"))
        .await
        .expect("bind token");

    let request_log_id = proxy
        .record_local_request_log_without_key(
            Some(&token.id),
            &Method::GET,
            "/search",
            Some("q=startup-skips-request-user-backfill"),
            StatusCode::OK,
            Some(200),
            b"{}",
            b"{}",
            OUTCOME_SUCCESS,
            None,
            &[],
            &[],
            None,
        )
        .await
        .expect("record token attempt");
    proxy
        .record_token_attempt_request_log_metadata(
            &token.id,
            &Method::GET,
            "/search",
            Some("q=startup-skips-request-user-backfill"),
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(request_log_id),
        )
        .await
        .expect("record linked token attempt");
    sqlx::query("UPDATE request_logs SET request_user_id = NULL WHERE id = ?")
        .bind(request_log_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request user id");
    drop(proxy);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let request_user_id: Option<String> =
        sqlx::query_scalar("SELECT request_user_id FROM request_logs WHERE id = ?")
            .bind(request_log_id)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("reload request user id after startup");
    assert_eq!(request_user_id, None);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_user_id_backfill_runs_in_resumable_batches() {
    let db_path = temp_db_path("request-user-id-backfill-resumable");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-user-backfill-resumable".to_string(),
            username: Some("request_user_backfill_resumable".to_string()),
            name: Some("Request User Backfill Resumable".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("request-user-backfill-resumable"))
        .await
        .expect("bind token");

    for index in 0..3 {
        let request_log_id = proxy
            .record_local_request_log_without_key(
                Some(&token.id),
                &Method::GET,
                "/search",
                Some(&format!("q=request-user-backfill-resumable-{index}")),
                StatusCode::OK,
                Some(200),
                b"{}",
                b"{}",
                OUTCOME_SUCCESS,
                None,
                &[],
                &[],
                None,
            )
            .await
            .expect("record token attempt");
        proxy
            .record_token_attempt_request_log_metadata(
                &token.id,
                &Method::GET,
                "/search",
                Some(&format!("q=request-user-backfill-resumable-{index}")),
                Some(StatusCode::OK.as_u16() as i64),
                Some(200),
                true,
                OUTCOME_SUCCESS,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(request_log_id),
            )
            .await
            .expect("record linked token attempt");
    }

    sqlx::query("UPDATE request_logs SET created_at = ?")
        .bind(Utc::now().timestamp() - 120)
        .execute(&proxy.key_store.pool)
        .await
        .expect("age request logs past backfill stability grace");
    sqlx::query("UPDATE request_logs SET request_user_id = NULL")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request user snapshots");

    let first = crate::store::run_request_user_id_backfill_with_pool(
        &proxy.key_store.pool,
        2,
        &proxy.key_store.backend_time,
    )
    .await
    .expect("run first request user id backfill batch");
    assert_eq!(first.rows_scanned, 2);
    assert_eq!(first.rows_updated, 2);
    assert_eq!(first.cursor_after, 2);

    let filled_after_first: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM request_logs
        WHERE request_user_id = ?
        "#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count filled request logs after first batch");
    assert_eq!(filled_after_first, 2);

    let second = crate::store::run_request_user_id_backfill_with_pool(
        &proxy.key_store.pool,
        2,
        &proxy.key_store.backend_time,
    )
    .await
    .expect("run second request user id backfill batch");
    assert_eq!(second.cursor_before, 2);
    assert_eq!(second.rows_scanned, 1);
    assert_eq!(second.rows_updated, 1);

    let filled_after_second: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM request_logs
        WHERE request_user_id = ?
        "#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count filled request logs after second batch");
    assert_eq!(filled_after_second, 3);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_quota1h_uses_historical_limit_snapshots() {
    let db_path = temp_db_path("admin-user-usage-series-quota-limit-snapshots");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-quota-snapshots".to_string(),
            username: Some("usage_series_quota_snapshots".to_string()),
            name: Some("Usage Series Quota Snapshots".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    proxy
        .update_account_business_quota_limits(&user.user_id, 600, 6_000, 60_000)
        .await
        .expect("update current business quota");
    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear auto snapshots");

    let now = Utc::now();
    let current_bucket_start = now.timestamp() - now.timestamp().rem_euclid(SECS_PER_HOUR);
    let start = current_bucket_start - 71 * SECS_PER_HOUR;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    sqlx::query(
        r#"INSERT INTO account_quota_limit_snapshots
           (user_id, changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit)
           VALUES (?, ?, ?, ?, ?, ?), (?, ?, ?, ?, ?, ?), (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&user.user_id)
    .bind(start + 12 * SECS_PER_HOUR + 600)
    .bind(200)
    .bind(200)
    .bind(2_000)
    .bind(20_000)
    .bind(&user.user_id)
    .bind(start + 36 * SECS_PER_HOUR + 600)
    .bind(400)
    .bind(400)
    .bind(4_000)
    .bind(40_000)
    .bind(&user.user_id)
    .bind(start + 60 * SECS_PER_HOUR + 600)
    .bind(600)
    .bind(600)
    .bind(6_000)
    .bind(60_000)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed deterministic quota snapshots");

    proxy
        .key_store
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START, start)
        .await
        .expect("set quota1h coverage");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Quota1h)
        .await
        .expect("load quota1h series");

    assert_eq!(series.limit, 600);
    assert_eq!(series.points.len(), 72);
    assert_eq!(series.points[11].limit_value, None);
    assert_eq!(series.points[12].limit_value, Some(200));
    assert_eq!(series.points[35].limit_value, Some(200));
    assert_eq!(series.points[36].limit_value, Some(400));
    assert_eq!(series.points[59].limit_value, Some(400));
    assert_eq!(series.points[60].limit_value, Some(600));
    assert!(series.points.iter().all(|point| point.value == Some(0)));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_rate5m_uses_historical_request_limit_snapshots() {
    let db_path = temp_db_path("admin-user-usage-series-rate-limit-snapshots");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-rate-snapshots".to_string(),
            username: Some("usage_series_rate_snapshots".to_string()),
            name: Some("Usage Series Rate Snapshots".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let mut settings = proxy
        .get_system_settings()
        .await
        .expect("get system settings");
    settings.request_rate_limit = 120;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("set current request rate");
    sqlx::query("DELETE FROM request_rate_limit_snapshots")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear auto request snapshots");

    let now = Utc::now();
    let current_bucket_start = now.timestamp() - now.timestamp().rem_euclid(SECS_PER_FIVE_MINUTES);
    let start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    sqlx::query(
        r#"INSERT INTO request_rate_limit_snapshots (changed_at, limit_value)
           VALUES (?, ?), (?, ?)"#,
    )
    .bind(start + 48 * SECS_PER_FIVE_MINUTES + 120)
    .bind(80)
    .bind(start + 200 * SECS_PER_FIVE_MINUTES + 120)
    .bind(120)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed deterministic request rate snapshots");

    proxy
        .key_store
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START, start)
        .await
        .expect("set rate5m coverage");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load rate5m series");

    assert_eq!(series.limit, 120);
    assert_eq!(series.points.len(), 288);
    assert_eq!(series.points[47].limit_value, None);
    assert_eq!(series.points[48].limit_value, Some(80));
    assert_eq!(series.points[199].limit_value, Some(80));
    assert_eq!(series.points[200].limit_value, Some(120));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_preserves_request_time_user_binding() {
    let db_path = temp_db_path("account-usage-rollup-request-user-snapshot");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let first_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-user-snapshot-a".to_string(),
            username: Some("request_user_snapshot_a".to_string()),
            name: Some("Request User Snapshot A".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert first user");
    let second_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-user-snapshot-b".to_string(),
            username: Some("request_user_snapshot_b".to_string()),
            name: Some("Request User Snapshot B".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert second user");
    let token = proxy
        .ensure_user_token_binding(&first_user.user_id, Some("request-user-snapshot"))
        .await
        .expect("bind token to first user");

    proxy
        .record_token_attempt(
            &token.id,
            &Method::GET,
            "/search",
            Some("q=request-user-snapshot"),
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
        )
        .await
        .expect("record token attempt");

    let (created_at, request_user_id): (i64, Option<String>) = sqlx::query_as(
        r#"
        SELECT created_at, request_user_id
        FROM auth_token_logs
        WHERE token_id = ?
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read stored request owner snapshot");
    assert_eq!(
        request_user_id.as_deref(),
        Some(first_user.user_id.as_str())
    );

    sqlx::query(
        r#"
        UPDATE user_token_bindings
        SET user_id = ?, updated_at = ?
        WHERE token_id = ?
        "#,
    )
    .bind(&second_user.user_id)
    .bind(created_at + 1)
    .bind(&token.id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("rebind token to second user");
    proxy
        .key_store
        .cache_token_binding(&token.id, Some(&second_user.user_id))
        .await;

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
    let first_user_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &first_user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load first user rollups");
    let second_user_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &second_user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load second user rollups");

    assert_eq!(first_user_values.get(&bucket_start), Some(&1));
    assert_eq!(second_user_values.get(&bucket_start), None);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_uses_current_binding_for_pre_migration_requests() {
    let db_path = temp_db_path("account-usage-rollup-pre-migration-binding-fallback");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "pre-migration-binding-fallback".to_string(),
            username: Some("pre_migration_binding_fallback".to_string()),
            name: Some("Pre Migration Binding Fallback".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("pre-migration-binding"))
        .await
        .expect("bind token");

    proxy
        .record_token_attempt(
            &token.id,
            &Method::GET,
            "/search",
            Some("q=pre-migration-binding"),
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
        )
        .await
        .expect("record token attempt");

    let created_at: i64 = sqlx::query_scalar(
        r#"
        SELECT created_at
        FROM auth_token_logs
        WHERE token_id = ?
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load pre-migration request log");
    sqlx::query(
        r#"
        UPDATE auth_token_logs
        SET request_user_id = NULL,
            billing_subject = NULL
        WHERE token_id = ?
        "#,
    )
    .bind(&token.id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("strip request-time ownership to simulate pre-migration rows");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
    let values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load rebuilt request rollups");

    assert_eq!(values.get(&bucket_start), Some(&1));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_zero_fills_inactive_rate5m_window() {
    let db_path = temp_db_path("account-usage-rollup-empty-rate5m-window");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "empty-rate5m-window".to_string(),
            username: Some("empty_rate5m_window".to_string()),
            name: Some("Empty Rate5m Window".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let window_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(window_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load empty rate5m series");

    assert_eq!(series.points.len(), 288);
    assert!(series.points.iter().all(|point| point.value == Some(0)));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_clears_stale_rate5m_buckets_without_logs() {
    let db_path = temp_db_path("account-usage-rollup-clears-stale-rate5m");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "clears-stale-rate5m".to_string(),
            username: Some("clears_stale_rate5m".to_string()),
            name: Some("Clears Stale Rate5m".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let stale_bucket_start = {
        let now = Utc::now().timestamp();
        let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
        current_bucket_start - SECS_PER_FIVE_MINUTES
    };
    sqlx::query(
        r#"
        INSERT INTO account_usage_rollup_buckets (
            user_id,
            metric_kind,
            bucket_kind,
            bucket_start,
            value,
            updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&user.user_id)
    .bind(AccountUsageRollupMetricKind::RequestCount.as_str())
    .bind(AccountUsageRollupBucketKind::FiveMinute.as_str())
    .bind(stale_bucket_start)
    .bind(9_i64)
    .bind(Utc::now().timestamp())
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert stale rate5m rollup");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            stale_bucket_start,
            stale_bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load rebuilt stale bucket");
    assert!(values.is_empty());

    let _ = std::fs::remove_file(db_path);
}

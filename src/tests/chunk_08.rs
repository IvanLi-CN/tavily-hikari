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

    let mut settings = proxy.get_system_settings().await.expect("get system settings");
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
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START, chart_start)
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
    assert_eq!(series.points.first().and_then(|point| point.limit_value), Some(80));
    assert_eq!(series.points[287].limit_value, Some(80));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_treats_absent_request_limit_setting_as_long_term_default() {
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
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START, chart_start)
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
    assert_eq!(series.points.first().and_then(|point| point.limit_value), Some(default_limit));
    assert_eq!(series.points[287].limit_value, Some(default_limit));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_limit_snapshot_backfill_treats_persisted_default_request_limit_as_partial_history() {
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
        ..proxy.get_system_settings().await.expect("get system settings")
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
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START, chart_start)
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
    assert_eq!(series.points[286].limit_value, None);
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
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START, start + 600)
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
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START, chart_start)
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
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START, chart_start)
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

    assert_eq!(series.points.first().and_then(|point| point.limit_value), None);
    assert_eq!(
        series.points.last().and_then(|point| point.limit_value),
        Some(series.limit)
    );

    let _ = std::fs::remove_file(db_path);
}

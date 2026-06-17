#[tokio::test]
async fn user_business_calls_1h_summary_and_series_track_real_upstream_requests_only() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_000_000);
    let db_path = temp_db_path("user-business-calls-1h-live-window");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "business-calls-live-window".to_string(),
            username: Some("business_calls_live_window".to_string()),
            name: Some("Business Calls Live Window".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("business-calls-live-window"))
        .await
        .expect("bind token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    let request_log_success: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES ('key-live-success', 'POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, 'http_search', ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 10 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert success request log");
    manual_clock.set_now_ts(now - 10 * 60);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=success"),
            Some(200),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
            &request_kind,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(request_log_success),
        )
        .await
        .expect("record success attempt");

    let request_log_failure: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES ('key-live-failure', 'POST', '/api/tavily/search', 500, 500, 'error', 'api:search', 'API | search', 1, ?, 'http_search', ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 5 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert failure request log");
    manual_clock.set_now_ts(now - 5 * 60);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=failure"),
            Some(500),
            Some(500),
            true,
            "error",
            Some("upstream failed"),
            &request_kind,
            Some("upstream_error"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(request_log_failure),
        )
        .await
        .expect("record failure attempt");

    let request_log_quota_exhausted: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES (NULL, 'POST', '/api/tavily/search', 429, 429, 'quota_exhausted', 'api:search', 'API | search', 1, ?, 'http_search', ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 2 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert quota exhausted request log");
    manual_clock.set_now_ts(now - 2 * 60);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=quota"),
            Some(429),
            Some(429),
            true,
            OUTCOME_QUOTA_EXHAUSTED,
            Some("quota exhausted"),
            &request_kind,
            Some("quota_exhausted"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(request_log_quota_exhausted),
        )
        .await
        .expect("record quota exhausted attempt");

    let request_log_pre_upstream: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES (NULL, 'POST', '/api/tavily/search', 429, 429, 'blocked', 'api:search', 'API | search', 1, ?, NULL, ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert pre-upstream request log");
    manual_clock.set_now_ts(now - 60);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=blocked"),
            Some(429),
            Some(429),
            true,
            "blocked",
            Some("blocked before upstream"),
            &request_kind,
            Some("pre_upstream_block"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(request_log_pre_upstream),
        )
        .await
        .expect("record pre-upstream attempt");

    manual_clock.set_now_ts(now);
    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load user dashboard summary");
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 1);
    assert_eq!(summary.business_calls_1h.total_count, 2);
    assert_eq!(summary.business_calls_1h.window_minutes, 60);

    let series = proxy
        .admin_user_business_calls_1h_series(&user.user_id)
        .await
        .expect("load business calls 1h series");
    assert_eq!(series.limit, 0);
    assert_eq!(series.points.len(), 288);
    let latest = series.points.last().expect("latest business calls point");
    assert_eq!(latest.pressure, Some(2));
    assert_eq!(latest.limit_value, None);

    let success_bucket = (now - 10 * 60) - (now - 10 * 60).rem_euclid(SECS_PER_FIVE_MINUTES);
    let failure_bucket = (now - 5 * 60) - (now - 5 * 60).rem_euclid(SECS_PER_FIVE_MINUTES);
    let success_point = series
        .points
        .iter()
        .find(|point| point.bucket_start == success_bucket)
        .expect("success bucket point");
    let failure_point = series
        .points
        .iter()
        .find(|point| point.bucket_start == failure_bucket)
        .expect("failure bucket point");
    assert_eq!(success_point.bars.success, Some(1));
    assert_eq!(success_point.bars.failure, Some(0));
    assert_eq!(failure_point.bars.success, Some(0));
    assert_eq!(failure_point.bars.failure, Some(1));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_business_calls_1h_backfill_rehydrates_recent_request_logs() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_100_000);
    let db_path = temp_db_path("user-business-calls-1h-backfill");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time.clone(),
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "business-calls-backfill".to_string(),
            username: Some("business_calls_backfill".to_string()),
            name: Some("Business Calls Backfill".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let now = manual_clock.now_ts();
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES
            ('key-backfill-success', 'POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, NULL, ?),
            ('key-backfill-failure', 'POST', '/api/tavily/search', 500, 500, 'error', 'api:search', 'API | search', 1, ?, NULL, ?),
            (NULL, 'POST', '/api/tavily/search', 429, 429, 'quota_exhausted', 'api:search', 'API | search', 1, ?, 'http_search', ?),
            (NULL, 'POST', '/api/tavily/search', 500, 500, 'error', 'api:search', 'API | search', 1, ?, NULL, ?),
            (NULL, 'POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, 'http_search', ?)
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 20 * 60)
    .bind(&user.user_id)
    .bind(now - 7 * 60)
    .bind(&user.user_id)
    .bind(now - 4 * 60)
    .bind(&user.user_id)
    .bind(now - 3 * 60)
    .bind(&user.user_id)
    .bind(now - 26 * SECS_PER_HOUR)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed request logs for backfill");
    drop(proxy);

    manual_clock.set_now_ts(now);
    let reopened = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("reopen proxy");

    let summary = reopened
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load backfilled summary");
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 1);
    assert_eq!(summary.business_calls_1h.total_count, 2);

    let series = reopened
        .admin_user_business_calls_1h_series(&user.user_id)
        .await
        .expect("load backfilled series");
    let latest = series.points.last().expect("latest backfilled point");
    assert_eq!(latest.pressure, Some(2));
    assert_eq!(latest.limit_value, None);

    let historical_success_bucket =
        (now - 20 * 60) - (now - 20 * 60).rem_euclid(SECS_PER_FIVE_MINUTES);
    let historical_success_point = series
        .points
        .iter()
        .find(|point| point.bucket_start == historical_success_bucket)
        .expect("historical success bucket point");
    assert_eq!(historical_success_point.bars.success, Some(1));
    assert_eq!(historical_success_point.bars.failure, Some(0));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_business_calls_1h_series_keeps_late_arriving_older_events_in_order() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_200_000);
    let db_path = temp_db_path("user-business-calls-1h-out-of-order-arrival");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "business-calls-out-of-order".to_string(),
            username: Some("business_calls_out_of_order".to_string()),
            name: Some("Business Calls Out Of Order".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("business-calls-out-of-order"))
        .await
        .expect("bind token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    let newer_request_log: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES ('POST', '/api/tavily/search', 500, 500, 'error', 'api:search', 'API | search', 1, ?, 'http_search', ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 5 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert newer request log");
    manual_clock.set_now_ts(now - 5 * 60);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=newer"),
            Some(500),
            Some(500),
            true,
            "error",
            Some("newer upstream failure"),
            &request_kind,
            Some("upstream_error"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(newer_request_log),
        )
        .await
        .expect("record newer attempt");

    let older_request_log: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            created_at
        ) VALUES ('POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, 'http_search', ?)
        RETURNING id
        "#,
    )
    .bind(&user.user_id)
    .bind(now - 10 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert older request log");
    manual_clock.set_now_ts(now - 4 * 60);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=older"),
            Some(200),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
            &request_kind,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(older_request_log),
        )
        .await
        .expect("record older attempt after newer arrival");

    manual_clock.set_now_ts(now);
    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load user dashboard summary");
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 1);
    assert_eq!(summary.business_calls_1h.total_count, 2);

    let series = proxy
        .admin_user_business_calls_1h_series(&user.user_id)
        .await
        .expect("load business calls 1h series");
    let latest = series.points.last().expect("latest business calls point");
    assert_eq!(latest.pressure, Some(2));

    let older_bucket = (now - 10 * 60) - (now - 10 * 60).rem_euclid(SECS_PER_FIVE_MINUTES);
    let newer_bucket = (now - 5 * 60) - (now - 5 * 60).rem_euclid(SECS_PER_FIVE_MINUTES);
    let older_point = series
        .points
        .iter()
        .find(|point| point.bucket_start == older_bucket)
        .expect("older bucket point");
    let newer_point = series
        .points
        .iter()
        .find(|point| point.bucket_start == newer_bucket)
        .expect("newer bucket point");
    assert_eq!(older_point.bars.success, Some(1));
    assert_eq!(older_point.bars.failure, Some(0));
    assert_eq!(newer_point.bars.success, Some(0));
    assert_eq!(newer_point.bars.failure, Some(1));

    let _ = std::fs::remove_file(db_path);
}

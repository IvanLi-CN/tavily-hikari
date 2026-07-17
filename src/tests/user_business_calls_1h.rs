use super::*;

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
    manual_clock.set_now_ts(now);
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
    manual_clock.set_now_ts(now);
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
    manual_clock.set_now_ts(now);
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
    manual_clock.set_now_ts(now);
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
    assert_eq!(latest.limit_value, Some(0));

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
            ('key-backfill-success', 'POST', '/api/tavily/search', 200, 200, 'success', 'api:search', 'API | search', 1, ?, 'http_search', ?),
            ('key-backfill-failure', 'POST', '/api/tavily/search', 500, 500, 'error', 'api:search', 'API | search', 1, ?, 'http_search', ?),
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

    let initial_summary = reopened
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load summary after startup backfill");
    assert_eq!(initial_summary.business_calls_1h.success_count, 1);
    assert_eq!(initial_summary.business_calls_1h.failure_count, 1);
    assert_eq!(initial_summary.business_calls_1h.total_count, 2);

    assert!(
        reopened.spawn_user_business_calls_1h_backfill_once(),
        "post-startup background backfill attempt should still schedule"
    );
    assert!(
        !reopened.spawn_user_business_calls_1h_backfill_once(),
        "concurrent background backfill attempts should dedupe"
    );

    let summary = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let summary = reopened
                .user_dashboard_summary(&user.user_id, None)
                .await
                .expect("load post-startup refreshed summary");
            if summary.business_calls_1h.total_count == 2 {
                return summary;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("post-startup business-call refresh should complete in time");
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 1);
    assert_eq!(summary.business_calls_1h.total_count, 2);

    let series = reopened
        .admin_user_business_calls_1h_series(&user.user_id)
        .await
        .expect("load backfilled series");
    let latest = series.points.last().expect("latest backfilled point");
    assert_eq!(latest.pressure, Some(2));
    assert_eq!(latest.limit_value, Some(0));

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
    manual_clock.set_now_ts(now);
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
    manual_clock.set_now_ts(now);
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

#[tokio::test]
async fn user_business_calls_1h_backfill_preserves_live_events_after_snapshot_upper_bound() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_300_000);
    let db_path = temp_db_path("user-business-calls-1h-backfill-preserves-live-events");
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
            provider_user_id: "business-calls-backfill-live-race".to_string(),
            username: Some("business_calls_backfill_live_race".to_string()),
            name: Some("Business Calls Backfill Live Race".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("business-calls-backfill-live-race"))
        .await
        .expect("bind token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    let historical_request_log_id: i64 = sqlx::query_scalar(
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
    .bind(now - 20 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert historical request log");

    let initial_summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load summary before background backfill");
    assert_eq!(initial_summary.business_calls_1h.total_count, 0);

    assert!(
        proxy.spawn_user_business_calls_1h_backfill_once(),
        "initial background backfill should schedule"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let summary = proxy
                .user_dashboard_summary(&user.user_id, None)
                .await
                .expect("load summary after initial backfill");
            if summary.business_calls_1h.total_count == 1 {
                return summary;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("initial background backfill should complete in time");

    let live_request_log_id: i64 = sqlx::query_scalar(
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
    .expect("insert live request log");
    manual_clock.set_now_ts(now);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=live"),
            Some(500),
            Some(500),
            true,
            "error",
            Some("live startup request"),
            &request_kind,
            Some("upstream_error"),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(live_request_log_id),
        )
        .await
        .expect("record live request");

    assert!(
        proxy.spawn_user_business_calls_1h_backfill_once(),
        "second background backfill should schedule"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let summary = proxy
                .user_dashboard_summary(&user.user_id, None)
                .await
                .expect("load summary after second backfill");
            if summary.business_calls_1h.total_count == 2 {
                return summary;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("second background backfill should preserve newer live event");

    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load summary after replay-safe backfill");
    assert_eq!(historical_request_log_id + 1, live_request_log_id);
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 1);
    assert_eq!(summary.business_calls_1h.total_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_business_calls_1h_backfill_dedupes_same_request_log_event() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_400_000);
    let db_path = temp_db_path("user-business-calls-1h-backfill-dedupes-same-log");
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
            provider_user_id: "business-calls-backfill-dedupe".to_string(),
            username: Some("business_calls_backfill_dedupe".to_string()),
            name: Some("Business Calls Backfill Dedupe".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("business-calls-backfill-dedupe"))
        .await
        .expect("bind token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    let request_log_id: i64 = sqlx::query_scalar(
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
    .bind(now - 6 * 60)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert request log");
    manual_clock.set_now_ts(now);
    proxy
        .record_token_attempt_with_kind_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=dedupe"),
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
            Some(request_log_id),
        )
        .await
        .expect("record live request");

    assert!(
        proxy.spawn_user_business_calls_1h_backfill_once(),
        "background backfill should schedule"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let summary = proxy
                .user_dashboard_summary(&user.user_id, None)
                .await
                .expect("load deduped summary while backfill runs");
            if summary.business_calls_1h.total_count == 1 {
                return summary;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("backfill should not double-count same request log");

    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load deduped summary");
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 0);
    assert_eq!(summary.business_calls_1h.total_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_business_calls_1h_metadata_free_token_logs_stay_out_of_live_business_window() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_500_000);
    let db_path = temp_db_path("user-business-calls-1h-no-request-log-metadata");
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
            provider_user_id: "business-calls-no-request-log-metadata".to_string(),
            username: Some("business_calls_no_request_log_metadata".to_string()),
            name: Some("Business Calls No Request Log Metadata".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(
            &user.user_id,
            Some("business-calls-no-request-log-metadata"),
        )
        .await
        .expect("bind token");
    let request_kind = TokenRequestKind::new("api:search", "API | search", None);
    let now = manual_clock.now_ts();

    manual_clock.set_now_ts(now);
    proxy
        .record_token_attempt_with_kind(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            Some("q=legacy-live"),
            Some(200),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
            &request_kind,
        )
        .await
        .expect("record metadata-free token log");

    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load summary after metadata-free token log");
    assert_eq!(summary.business_calls_1h.success_count, 0);
    assert_eq!(summary.business_calls_1h.failure_count, 0);
    assert_eq!(summary.business_calls_1h.total_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_business_calls_1h_reservations_enforce_limit_without_polluting_completed_views() {
    let (backend_time, manual_clock) = crate::BackendTime::manual_from_ts(1_700_600_000);
    let db_path = temp_db_path("user-business-calls-1h-reservations");
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
            provider_user_id: "business-calls-reservations".to_string(),
            username: Some("business_calls_reservations".to_string()),
            name: Some("Business Calls Reservations".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("business-calls-reservations"))
        .await
        .expect("bind token");
    proxy
        .update_account_business_quota_limits(&user.user_id, 1, 1_000, 10_000)
        .await
        .expect("set business call limit");

    let first_reservation = match proxy
        .reserve_token_business_calls_1h_limit(&token.id)
        .await
        .expect("reserve first request")
    {
        BusinessCalls1hReservationOutcome::Reserved(reservation) => reservation,
        other => panic!("expected first reservation to succeed, got {other:?}"),
    };

    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load summary with in-flight reservation");
    assert_eq!(summary.business_calls_1h.success_count, 0);
    assert_eq!(summary.business_calls_1h.failure_count, 0);
    assert_eq!(summary.business_calls_1h.total_count, 0);

    match proxy
        .reserve_token_business_calls_1h_limit(&token.id)
        .await
        .expect("reserve second request")
    {
        BusinessCalls1hReservationOutcome::Denied(verdict) => {
            assert!(!verdict.allowed);
            assert_eq!(verdict.summary.limit, 1);
            assert_eq!(verdict.summary.total_count, 1);
            assert_eq!(verdict.summary.success_count, 0);
            assert_eq!(verdict.summary.failure_count, 0);
        }
        other => panic!("expected second reservation to be denied, got {other:?}"),
    }

    proxy
        .release_business_calls_1h_reservation(Some(first_reservation))
        .await;

    let completed_reservation = match proxy
        .reserve_token_business_calls_1h_limit(&token.id)
        .await
        .expect("reserve replacement request")
    {
        BusinessCalls1hReservationOutcome::Reserved(reservation) => reservation,
        other => panic!("expected replacement reservation to succeed, got {other:?}"),
    };
    proxy
        .finalize_business_calls_1h_reservation_from_status(
            Some(completed_reservation),
            OUTCOME_SUCCESS,
            Some(9_001),
        )
        .await;

    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("load summary after finalize");
    assert_eq!(summary.business_calls_1h.success_count, 1);
    assert_eq!(summary.business_calls_1h.failure_count, 0);
    assert_eq!(summary.business_calls_1h.total_count, 1);

    let series = proxy
        .admin_user_business_calls_1h_series(&user.user_id)
        .await
        .expect("load series after finalize");
    let latest = series.points.last().expect("latest point");
    assert_eq!(latest.pressure, Some(1));
    assert_eq!(latest.bars.success, Some(1));
    assert_eq!(latest.bars.failure, Some(0));

    let quota_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "business-calls-reservation-quota-exhausted".to_string(),
            username: Some("business_calls_reservation_quota_exhausted".to_string()),
            name: Some("Business Calls Reservation Quota Exhausted".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert quota-exhausted user");
    let quota_token = proxy
        .ensure_user_token_binding(
            &quota_user.user_id,
            Some("business-calls-reservation-quota-exhausted"),
        )
        .await
        .expect("bind quota-exhausted token");
    proxy
        .update_account_business_quota_limits(&quota_user.user_id, 1, 1_000, 10_000)
        .await
        .expect("set quota-exhausted user limit");

    let quota_reservation = match proxy
        .reserve_token_business_calls_1h_limit(&quota_token.id)
        .await
        .expect("reserve quota-exhausted request")
    {
        BusinessCalls1hReservationOutcome::Reserved(reservation) => reservation,
        other => panic!("expected quota-exhausted reservation to succeed, got {other:?}"),
    };
    proxy
        .finalize_business_calls_1h_reservation_from_status(
            Some(quota_reservation),
            OUTCOME_QUOTA_EXHAUSTED,
            Some(9_002),
        )
        .await;

    let quota_summary = proxy
        .user_dashboard_summary(&quota_user.user_id, None)
        .await
        .expect("load quota-exhausted user summary");
    assert_eq!(quota_summary.business_calls_1h.success_count, 0);
    assert_eq!(quota_summary.business_calls_1h.failure_count, 0);
    assert_eq!(quota_summary.business_calls_1h.total_count, 0);

    let quota_replacement = match proxy
        .reserve_token_business_calls_1h_limit(&quota_token.id)
        .await
        .expect("reserve after quota-exhausted release")
    {
        BusinessCalls1hReservationOutcome::Reserved(reservation) => reservation,
        other => panic!("expected reservation after quota_exhausted to succeed, got {other:?}"),
    };
    proxy
        .release_business_calls_1h_reservation(Some(quota_replacement))
        .await;

    let ttl_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "business-calls-reservation-ttl".to_string(),
            username: Some("business_calls_reservation_ttl".to_string()),
            name: Some("Business Calls Reservation TTL".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert ttl user");
    let ttl_token = proxy
        .ensure_user_token_binding(&ttl_user.user_id, Some("business-calls-reservation-ttl"))
        .await
        .expect("bind ttl token");
    proxy
        .update_account_business_quota_limits(&ttl_user.user_id, 1, 1_000, 10_000)
        .await
        .expect("set ttl user limit");

    let expired_reservation = match proxy
        .reserve_token_business_calls_1h_limit(&ttl_token.id)
        .await
        .expect("reserve ttl request")
    {
        BusinessCalls1hReservationOutcome::Reserved(reservation) => reservation,
        other => panic!("expected ttl reservation to succeed, got {other:?}"),
    };
    manual_clock.advance_wall(Duration::from_secs(301));
    let fresh_reservation = match proxy
        .reserve_token_business_calls_1h_limit(&ttl_token.id)
        .await
        .expect("reserve request after ttl gc")
    {
        BusinessCalls1hReservationOutcome::Reserved(reservation) => reservation,
        other => panic!("expected reservation after ttl to succeed, got {other:?}"),
    };
    proxy
        .release_business_calls_1h_reservation(Some(expired_reservation))
        .await;
    proxy
        .release_business_calls_1h_reservation(Some(fresh_reservation))
        .await;

    let ttl_summary = proxy
        .user_dashboard_summary(&ttl_user.user_id, None)
        .await
        .expect("load ttl user summary");
    assert_eq!(ttl_summary.business_calls_1h.success_count, 0);
    assert_eq!(ttl_summary.business_calls_1h.failure_count, 0);
    assert_eq!(ttl_summary.business_calls_1h.total_count, 0);

    let _ = std::fs::remove_file(db_path);
}

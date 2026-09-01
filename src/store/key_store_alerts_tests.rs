use super::*;
use crate::BackendTime;
use tempfile::tempdir;

async fn seed_bound_user_and_token(
    store: &KeyStore,
    user_id: &str,
    token_id: &str,
    display_name: &str,
    username: &str,
    created_at: i64,
) {
    sqlx::query(
        "INSERT INTO users (id, display_name, username, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(display_name)
    .bind(username)
    .bind(created_at)
    .bind(created_at)
    .execute(&store.pool)
    .await
    .expect("insert user");

    sqlx::query("INSERT INTO auth_tokens (id, secret, created_at) VALUES (?, ?, ?)")
        .bind(token_id)
        .bind(format!("secret-{token_id}"))
        .bind(created_at)
        .execute(&store.pool)
        .await
        .expect("insert auth token");

    sqlx::query(
        "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(token_id)
    .bind(created_at)
    .bind(created_at)
    .execute(&store.pool)
    .await
    .expect("insert user token binding");
}

async fn insert_request_log(
    store: &KeyStore,
    token_id: &str,
    key_id: &str,
    created_at: i64,
) -> i64 {
    sqlx::query_scalar(
        r#"
        INSERT INTO observability.request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
            counts_business_quota,
            created_at
        ) VALUES (?, ?, 'POST', '/api/tavily/search', 'max_results=5', 432, 'quota_exhausted', 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 1, ?)
        RETURNING id
        "#,
    )
    .bind(key_id)
    .bind(token_id)
    .bind(created_at)
    .fetch_one(&store.pool)
    .await
    .expect("insert request log")
}

async fn insert_request_rate_alert_with_error_message(
    store: &KeyStore,
    token_id: &str,
    created_at: i64,
    request_kind_key: &str,
    request_kind_label: &str,
    error_message: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
            result_status,
            error_message,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            counts_business_quota,
            created_at
        ) VALUES (?, 'POST', '/mcp', ?, ?, ?, 'quota_exhausted', ?, 'none', 'none', 'none', 0, ?)
        "#,
    )
    .bind(token_id)
    .bind(request_kind_key)
    .bind(request_kind_label)
    .bind(request_kind_label)
    .bind(error_message)
    .bind(created_at)
    .execute(&store.pool)
    .await
    .expect("insert request-rate alert");
}

async fn insert_request_rate_alert(
    store: &KeyStore,
    token_id: &str,
    created_at: i64,
    request_kind_key: &str,
    request_kind_label: &str,
) {
    insert_request_rate_alert_with_error_message(
        store,
        token_id,
        created_at,
        request_kind_key,
        request_kind_label,
        "user request rate limit exceeded on rolling 5m window (limit 25, used 25)",
    )
    .await;
}

async fn insert_upstream_usage_limit_alert(
    store: &KeyStore,
    token_id: &str,
    key_id: &str,
    created_at: i64,
) {
    let request_log_id = insert_request_log(store, token_id, key_id, created_at).await;
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            query,
            http_status,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
            result_status,
            error_message,
            key_effect_code,
            binding_effect_code,
            selection_effect_code,
            counts_business_quota,
            api_key_id,
            request_log_id,
            created_at
        ) VALUES (?, 'POST', '/api/tavily/search', 'max_results=5', 432, 'tavily_search', 'Tavily Search', 'POST /api/tavily/search', 'quota_exhausted', 'This request exceeds your plan''s set usage limit.', 'none', 'none', 'none', 1, ?, ?, ?)
        "#,
    )
    .bind(token_id)
    .bind(key_id)
    .bind(request_log_id)
    .bind(created_at)
    .execute(&store.pool)
    .await
    .expect("insert upstream usage-limit alert");
}

fn make_alert_event(
    id: &str,
    alert_type: &str,
    occurred_at: i64,
    request_kind_key: &str,
    request_kind_label: &str,
    error_message: Option<&str>,
) -> AlertEventRecord {
    let mut event = AlertEventRecord {
        id: id.to_string(),
        alert_type: alert_type.to_string(),
        title: format!("title-{id}"),
        summary: format!("summary-{id}"),
        occurred_at,
        subject_kind: "user".to_string(),
        subject_id: "usr_test".to_string(),
        subject_label: "Test User".to_string(),
        user: Some(AlertUserRef {
            user_id: "usr_test".to_string(),
            display_name: Some("Test User".to_string()),
            username: Some("tester".to_string()),
        }),
        token: Some(AlertEntityRef {
            id: "tok_test".to_string(),
            label: "tok_test".to_string(),
        }),
        key: None,
        job: None,
        request: Some(AlertRequestRef {
            id: occurred_at,
            method: "POST".to_string(),
            path: "/mcp".to_string(),
            query: None,
        }),
        request_kind: Some(TokenRequestKind::new(
            request_kind_key,
            request_kind_label,
            Some(request_kind_label.to_string()),
        )),
        failure_kind: None,
        result_status: Some("quota_exhausted".to_string()),
        error_message: error_message.map(str::to_string),
        reason_code: None,
        reason_summary: None,
        reason_detail: None,
        source: AlertSourceRef {
            kind: "auth_token_log".to_string(),
            id: format!("source-{id}"),
        },
        semantic_window: None,
    };
    event.semantic_window = event_semantic_window(&event);
    event
}

#[test]
fn request_rate_alert_summary_names_rolling_window_and_request_kind() {
    let token = AlertEntityRef {
        id: "tok_test".to_string(),
        label: "tok_test".to_string(),
    };
    let request_kind = TokenRequestKind::new(
        "mcp_resources_list",
        "MCP resources/list",
        Some("resources/list".to_string()),
    );

    let (title, summary) = build_alert_title_and_summary(
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
        AlertTitleSummaryContext {
            subject_label: "Alice Wang",
            token: Some(&token),
            key: None,
            job: None,
            request_kind: Some(&request_kind),
            error_message: Some(
                "user request rate limit exceeded on rolling 5m window (limit 25, used 25)",
            ),
            reason_summary: None,
        },
    );

    assert_eq!(title, "Alice Wang hit the local request-rate limit");
    assert_eq!(
        summary,
        "Token tok_test was rate limited by the local rolling 5m request-rate window for MCP resources/list."
    );
}

#[test]
fn request_rate_events_merge_request_kinds_into_one_child_window() {
    let grouped = build_group_records_from_events(vec![
        make_alert_event(
            "evt-1",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_000,
            "mcp_initialize",
            "MCP initialize",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-2",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_060,
            "mcp_tools_list",
            "MCP tools/list",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-3",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_120,
            "mcp_notifications_initialized",
            "MCP notifications/initialized",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-4",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_180,
            "mcp_resources_list",
            "MCP resources/list",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
    ]);

    assert_eq!(grouped.top_level_items.len(), 1);
    let mother = &grouped.top_level_items[0];
    assert_eq!(mother.grouping_kind, "mother");
    assert_eq!(mother.child_count, 1);
    assert_eq!(mother.event_count, 4);
    assert!(mother.request_kind.is_none());

    let child = &mother.children[0];
    assert_eq!(child.grouping_kind, "child");
    assert_eq!(child.child_events.len(), 4);
    assert!(child.request_kind.is_none());
    assert_eq!(
        child.child_events[0]
            .request_kind
            .as_ref()
            .map(|value| value.key.as_str()),
        Some("mcp_resources_list")
    );
    assert_eq!(
        child.child_events[3]
            .request_kind
            .as_ref()
            .map(|value| value.key.as_str()),
        Some("mcp_initialize")
    );
}

#[test]
fn upstream_alerts_prefer_key_subject_over_token() {
    let user = AlertUserRef {
        user_id: "usr_test".to_string(),
        display_name: Some("Test User".to_string()),
        username: Some("tester".to_string()),
    };
    let token = AlertEntityRef {
        id: "tok_test".to_string(),
        label: "tok_test".to_string(),
    };
    let key = AlertEntityRef {
        id: "key_test".to_string(),
        label: "key_test".to_string(),
    };

    let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
        ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432,
        Some(&user),
        Some(&token),
        Some(&key),
        None,
    );

    assert_eq!(subject_kind, ALERT_SUBJECT_KEY);
    assert_eq!(subject_id, "key_test");
    assert_eq!(subject_label, "key_test");
}

#[test]
fn local_limit_alerts_prefer_user_subject_over_token() {
    let user = AlertUserRef {
        user_id: "usr_test".to_string(),
        display_name: Some("Test User".to_string()),
        username: Some("tester".to_string()),
    };
    let token = AlertEntityRef {
        id: "tok_test".to_string(),
        label: "tok_test".to_string(),
    };
    let key = AlertEntityRef {
        id: "key_test".to_string(),
        label: "key_test".to_string(),
    };

    let (subject_kind, subject_id, subject_label) = alert_subject_tuple(
        ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
        Some(&user),
        Some(&token),
        Some(&key),
        None,
    );

    assert_eq!(subject_kind, ALERT_SUBJECT_USER);
    assert_eq!(subject_id, "usr_test");
    assert_eq!(subject_label, "Test User");
}

#[test]
fn request_rate_contiguous_children_roll_up_into_one_mother_range() {
    let grouped = build_group_records_from_events(vec![
        make_alert_event(
            "evt-1",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_000,
            "mcp_initialize",
            "MCP initialize",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-2",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_060,
            "mcp_tools_list",
            "MCP tools/list",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-3",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_420,
            "mcp_notifications_initialized",
            "MCP notifications/initialized",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-4",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_000_480,
            "mcp_resources_list",
            "MCP resources/list",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
        make_alert_event(
            "evt-5",
            ALERT_TYPE_USER_REQUEST_RATE_LIMITED,
            1_700_001_500,
            "mcp_resources_list",
            "MCP resources/list",
            Some("user request rate limit exceeded on rolling 5m window (limit 25, used 25)"),
        ),
    ]);

    assert_eq!(grouped.top_level_items.len(), 2);
    let merged = grouped
        .top_level_items
        .iter()
        .find(|item| item.child_count == 2)
        .expect("merged mother range");
    assert_eq!(merged.grouping_kind, "mother");
    assert_eq!(merged.event_count, 4);
    assert_eq!(merged.children.len(), 2);

    let split = grouped
        .top_level_items
        .iter()
        .find(|item| item.child_count == 1)
        .expect("split mother range");
    assert_eq!(split.event_count, 1);
}

#[test]
fn quota_window_parser_recovers_hour_day_and_month_semantics() {
    let hour = make_alert_event(
        "hour",
        ALERT_TYPE_USER_QUOTA_EXHAUSTED,
        1_700_100_000,
        "tavily_search",
        "Tavily Search",
        Some("token quota exceeded on hour window (limit 100, used 100)"),
    );
    let day = make_alert_event(
        "day",
        ALERT_TYPE_USER_QUOTA_EXHAUSTED,
        1_700_100_000,
        "tavily_search",
        "Tavily Search",
        Some("token quota exceeded on day window (limit 500, used 500)"),
    );
    let month = make_alert_event(
        "month",
        ALERT_TYPE_USER_QUOTA_EXHAUSTED,
        1_700_100_000,
        "tavily_search",
        "Tavily Search",
        Some("token quota exceeded on month window (limit 5000, used 5000)"),
    );

    assert_eq!(
        hour.semantic_window.as_ref().map(|value| value.kind),
        Some(AlertSemanticWindowKind::RollingHour)
    );
    assert_eq!(
        day.semantic_window.as_ref().map(|value| value.kind),
        Some(AlertSemanticWindowKind::Day)
    );
    assert_eq!(
        month.semantic_window.as_ref().map(|value| value.kind),
        Some(AlertSemanticWindowKind::Month)
    );
    assert!(hour
        .semantic_window
        .as_ref()
        .and_then(|value| value.window_key.as_ref())
        .is_some_and(|value| value.starts_with("hour:")));
    assert!(day
        .semantic_window
        .as_ref()
        .and_then(|value| value.window_key.as_ref())
        .is_some_and(|value| value.starts_with("day:")));
    assert!(month
        .semantic_window
        .as_ref()
        .and_then(|value| value.window_key.as_ref())
        .is_some_and(|value| value.starts_with("month:")));
}

#[test]
fn quota_hour_windows_form_distinct_children_under_one_mother() {
    let grouped = build_group_records_from_events(vec![
        make_alert_event(
            "hour-1",
            ALERT_TYPE_USER_QUOTA_EXHAUSTED,
            1_700_200_000,
            "tavily_search",
            "Tavily Search",
            Some("token quota exceeded on hour window (limit 100, used 100)"),
        ),
        make_alert_event(
            "hour-2",
            ALERT_TYPE_USER_QUOTA_EXHAUSTED,
            1_700_200_010,
            "tavily_extract",
            "Tavily Extract",
            Some("token quota exceeded on hour window (limit 100, used 100)"),
        ),
    ]);

    assert_eq!(grouped.top_level_items.len(), 1);
    let mother = &grouped.top_level_items[0];
    assert_eq!(mother.grouping_kind, "mother");
    assert_eq!(mother.semantic_window_kind.as_deref(), Some("rolling_hour"));
    assert_eq!(mother.child_count, 2);
    assert_eq!(mother.event_count, 2);
}

#[test]
fn unrecoverable_quota_events_fall_back_to_compat_groups() {
    let grouped = build_group_records_from_events(vec![make_alert_event(
        "quota-compat",
        ALERT_TYPE_USER_QUOTA_EXHAUSTED,
        1_700_300_000,
        "tavily_search",
        "Tavily Search",
        Some("quota exhausted"),
    )]);

    assert_eq!(grouped.top_level_items.len(), 1);
    let group = &grouped.top_level_items[0];
    assert_eq!(group.grouping_kind, "compat");
    assert_eq!(
        group.request_kind.as_ref().map(|value| value.key.as_str()),
        Some("tavily_search")
    );
}

#[tokio::test]
async fn fetch_alert_groups_page_executes_sqlite_grouped_query_for_mother_and_compat_groups() {
    let temp_dir = tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("alerts-groups.db");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new_with_time(&db_str, BackendTime::system())
        .await
        .expect("create key store");

    let user_id = "usr_alerts_sql";
    let token_id = "tok_alerts_sql";
    let key_id = "key_alerts_sql";
    seed_bound_user_and_token(
        &store,
        user_id,
        token_id,
        "SQLite Alerts",
        "sqlite-alerts",
        1_700_000_000,
    )
    .await;

    for (created_at, request_kind_key, request_kind_label) in [
        (1_700_000_000_i64, "mcp_initialize", "MCP initialize"),
        (1_700_000_060_i64, "mcp_tools_list", "MCP tools/list"),
        (
            1_700_000_420_i64,
            "mcp_notifications_initialized",
            "MCP notifications/initialized",
        ),
        (1_700_000_480_i64, "mcp_resources_list", "MCP resources/list"),
    ] {
        insert_request_rate_alert(
            &store,
            token_id,
            created_at,
            request_kind_key,
            request_kind_label,
        )
        .await;
    }

    insert_upstream_usage_limit_alert(&store, token_id, key_id, 1_700_100_000).await;
    insert_upstream_usage_limit_alert(&store, token_id, key_id, 1_700_100_120).await;

    let page = {
        const MAX_ATTEMPTS: usize = 48;
        let mut attempt = 0;
        loop {
            match store
                .fetch_alert_groups_page(None, None, None, None, None, None, &[], 1, 20)
                .await
            {
                Ok(page) => break page,
                Err(error)
                    if crate::store::is_transient_sqlite_write_error(&error)
                        && attempt + 1 < MAX_ATTEMPTS =>
                {
                    attempt += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(error) => panic!("fetch grouped alerts page: {error}"),
            }
        }
    };

    assert_eq!(page.total, 2);

    let mother = page
        .items
        .iter()
        .find(|item| item.grouping_kind == "mother")
        .expect("semantic mother group");
    assert_eq!(mother.alert_type, ALERT_TYPE_USER_REQUEST_RATE_LIMITED);
    assert_eq!(mother.subject_kind, ALERT_SUBJECT_USER);
    assert_eq!(mother.child_count, 2);
    assert_eq!(mother.event_count, 4);
    assert_eq!(mother.children.len(), 2);
    assert!(mother.request_kind.is_none());

    let compat = page
        .items
        .iter()
        .find(|item| item.grouping_kind == "compat")
        .expect("compat group");
    assert_eq!(compat.alert_type, ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432);
    assert_eq!(compat.subject_kind, ALERT_SUBJECT_KEY);
    assert_eq!(compat.count, 2);
    assert_eq!(compat.event_count, 2);
    assert_eq!(
        compat.request_kind.as_ref().map(|value| value.key.as_str()),
        Some("api:search")
    );
}

#[tokio::test]
async fn recent_alerts_summary_uses_latest_event_window_minutes_for_business_call_caps() {
    let temp_dir = tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("alerts-recent-summary-window-minutes.db");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, _manual_time) = BackendTime::manual_from_ts(1_700_005_000);
    let store = KeyStore::new_with_time(&db_str, backend_time)
        .await
        .expect("create key store");

    let user_id = "usr_alerts_summary";
    let token_id = "tok_alerts_summary";
    seed_bound_user_and_token(
        &store,
        user_id,
        token_id,
        "Summary Alerts",
        "summary-alerts",
        1_700_000_000,
    )
    .await;

    for (created_at, request_kind_key, request_kind_label) in [
        (1_700_000_000_i64, "mcp_search", "MCP Search"),
        (1_700_000_060_i64, "mcp_search", "MCP Search"),
    ] {
        insert_request_rate_alert_with_error_message(
            &store,
            token_id,
            created_at,
            request_kind_key,
            request_kind_label,
            "business request count cap exceeded on rolling 60m window (limit 300, used 302)",
        )
        .await;
    }

    let summary = store
        .fetch_recent_alerts_summary(24)
        .await
        .expect("fetch recent alerts summary");
    let group = summary
        .top_groups
        .iter()
        .find(|item| item.alert_type == ALERT_TYPE_USER_REQUEST_RATE_LIMITED)
        .expect("request-rate grouped alert");

    assert_eq!(group.grouping_kind, "mother");
    assert_eq!(group.semantic_window_kind.as_deref(), Some("request_rate"));
    assert_eq!(group.semantic_window_minutes, Some(60));
    assert_eq!(
        group
            .latest_event
            .semantic_window
            .as_ref()
            .and_then(|value| value.window_minutes),
        Some(60)
    );
    assert!(group.latest_event.summary.contains("rolling 60m request-rate window"));
}

#[tokio::test]
async fn fetch_alert_groups_page_supports_multiple_mother_groups_without_sqlite_syntax_errors() {
    let temp_dir = tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("alerts-groups-multi-mother.db");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new_with_time(&db_str, BackendTime::system())
        .await
        .expect("create key store");

    for (user_id, token_id, created_at) in [
        ("usr_alerts_multi_a", "tok_alerts_multi_a", 1_700_000_000_i64),
        ("usr_alerts_multi_b", "tok_alerts_multi_b", 1_700_010_000_i64),
    ] {
        seed_bound_user_and_token(
            &store,
            user_id,
            token_id,
            "SQLite Alerts",
            "sqlite-alerts",
            created_at.saturating_sub(120),
        )
        .await;
    }

    for (token_id, created_at, request_kind_key, request_kind_label) in [
        (
            "tok_alerts_multi_a",
            1_700_000_000_i64,
            "mcp_initialize",
            "MCP initialize",
        ),
        (
            "tok_alerts_multi_a",
            1_700_000_060_i64,
            "mcp_tools_list",
            "MCP tools/list",
        ),
        (
            "tok_alerts_multi_b",
            1_700_010_000_i64,
            "mcp_initialize",
            "MCP initialize",
        ),
        (
            "tok_alerts_multi_b",
            1_700_010_060_i64,
            "mcp_tools_list",
            "MCP tools/list",
        ),
    ] {
        insert_request_rate_alert(
            &store,
            token_id,
            created_at,
            request_kind_key,
            request_kind_label,
        )
        .await;
    }

    let page = store
        .fetch_alert_groups_page(
            Some(ALERT_TYPE_USER_REQUEST_RATE_LIMITED),
            None,
            None,
            None,
            None,
            None,
            &[],
            1,
            20,
        )
        .await
        .expect("fetch grouped alerts page with multiple mother groups");

    let mother_groups = page
        .items
        .iter()
        .filter(|item| item.grouping_kind == "mother")
        .collect::<Vec<_>>();
    assert_eq!(mother_groups.len(), 2);
    assert!(mother_groups.iter().all(|group| group.children.len() == 1));
}

#[tokio::test]
async fn alert_candidate_query_uses_partial_time_index() {
    let temp_dir = tempdir().expect("create temp dir");
    let db_path = temp_dir.path().join("alerts-partial-index.db");
    let store = KeyStore::new_with_time(
        &db_path.to_string_lossy(),
        BackendTime::system(),
    )
    .await
    .expect("create key store");

    store
        .ensure_auth_token_logs_alert_time_index()
        .await
        .expect("ensure alert index");
    store
        .ensure_auth_token_logs_alert_time_index()
        .await
        .expect("ensure alert index idempotently");

    let rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT id
           FROM auth_token_logs INDEXED BY idx_auth_token_logs_alert_time
           WHERE created_at >= ?
             AND (failure_kind = 'upstream_rate_limited_429'
                  OR result_status = 'quota_exhausted')
           ORDER BY created_at DESC, id DESC
           LIMIT 100"#,
    )
    .bind(1_700_000_000_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain alert candidate query");
    let details = rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        details.contains("idx_auth_token_logs_alert_time"),
        "query plan did not use partial alert index: {details}"
    );
    assert!(!details.contains("SCAN auth_token_logs"), "{details}");
}

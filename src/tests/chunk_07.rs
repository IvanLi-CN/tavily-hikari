#[tokio::test]
async fn reconcile_key_health_reports_none_when_state_already_changed() {
    let db_path = temp_db_path("maintenance-repeat-noop");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-repeat-noop".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    proxy
        .key_store
        .mark_quota_exhausted(&secret)
        .await
        .expect("seed exhausted");
    let exhausted_effect = proxy
        .reconcile_key_health(
            &lease,
            "/api/tavily/search",
            &AttemptAnalysis {
                status: OUTCOME_QUOTA_EXHAUSTED,
                tavily_status_code: Some(432),
                key_health_action: KeyHealthAction::MarkExhausted,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("repeat exhausted");
    assert_eq!(exhausted_effect.code, KEY_EFFECT_NONE);

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/mcp",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("seed quarantine");
    let quarantine_effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_ERROR,
                tavily_status_code: Some(401),
                key_health_action: KeyHealthAction::Quarantine(QuarantineDecision {
                    reason_code: "account_deactivated".to_string(),
                    reason_summary: "Tavily account deactivated (HTTP 401)".to_string(),
                    reason_detail: "deactivated".to_string(),
                }),
                failure_kind: Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401.to_string()),
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("repeat quarantine");
    assert_eq!(quarantine_effect.code, KEY_EFFECT_NONE);

    let maintenance_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_key_maintenance_records WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count maintenance records");
    assert_eq!(maintenance_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconcile_key_health_does_not_restore_exhausted_key_on_error() {
    let db_path = temp_db_path("maintenance-no-restore-on-error");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-no-restore".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    proxy
        .key_store
        .mark_quota_exhausted(&secret)
        .await
        .expect("seed exhausted");

    let effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_ERROR,
                tavily_status_code: Some(429),
                key_health_action: KeyHealthAction::None,
                failure_kind: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429.to_string()),
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("error should not restore");
    assert_eq!(effect.code, KEY_EFFECT_NONE);

    let status: String = sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ? LIMIT 1")
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read key status");
    assert_eq!(status, STATUS_EXHAUSTED);

    let maintenance_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_key_maintenance_records WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count maintenance records");
    assert_eq!(maintenance_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn low_quota_432_records_monthly_depletion_at_threshold() {
    let db_path = temp_db_path("low-quota-depletion-threshold");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-low-quota-threshold".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    sqlx::query("UPDATE api_keys SET quota_limit = 1000, quota_remaining = 15 WHERE id = ?")
        .bind(&key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed quota");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret,
    };

    let effect = proxy
        .reconcile_key_health(
            &lease,
            "/api/tavily/search",
            &AttemptAnalysis {
                status: OUTCOME_QUOTA_EXHAUSTED,
                tavily_status_code: Some(432),
                key_health_action: KeyHealthAction::MarkExhausted,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("mark exhausted");
    assert_eq!(effect.code, KEY_EFFECT_MARKED_EXHAUSTED);

    let row: (i64, i64) = sqlx::query_as(
        "SELECT threshold, quota_remaining FROM api_key_low_quota_depletions WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch depletion row");
    assert_eq!(row, (15, 15));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn low_quota_432_does_not_record_above_threshold() {
    let db_path = temp_db_path("low-quota-depletion-above-threshold");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-low-quota-above-threshold".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    sqlx::query("UPDATE api_keys SET quota_limit = 1000, quota_remaining = 16 WHERE id = ?")
        .bind(&key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed quota");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret,
    };

    proxy
        .reconcile_key_health(
            &lease,
            "/api/tavily/search",
            &AttemptAnalysis {
                status: OUTCOME_QUOTA_EXHAUSTED,
                tavily_status_code: Some(432),
                key_health_action: KeyHealthAction::MarkExhausted,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("mark exhausted");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_key_low_quota_depletions WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count depletion rows");
    assert_eq!(count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn low_quota_depleted_keys_are_final_fallback_only() {
    let db_path = temp_db_path("low-quota-depletion-final-fallback");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-low-quota-active".to_string(),
            "tvly-low-quota-regular-exhausted".to_string(),
            "tvly-low-quota-depleted".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let active_id: String =
        sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = 'tvly-low-quota-active'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("active id");
    let regular_id: String = sqlx::query_scalar(
        "SELECT id FROM api_keys WHERE api_key = 'tvly-low-quota-regular-exhausted'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("regular id");
    let depleted_id: String =
        sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = 'tvly-low-quota-depleted'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("depleted id");

    proxy
        .key_store
        .mark_quota_exhausted("tvly-low-quota-regular-exhausted")
        .await
        .expect("regular exhausted");
    proxy
        .key_store
        .mark_quota_exhausted("tvly-low-quota-depleted")
        .await
        .expect("depleted exhausted");
    sqlx::query("UPDATE api_keys SET quota_limit = 1000, quota_remaining = 1 WHERE id = ?")
        .bind(&depleted_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed depleted quota");
    assert!(
        proxy
            .key_store
            .record_low_quota_depletion_if_needed(&depleted_id, 15)
            .await
            .expect("record depletion")
    );

    let lease = proxy.key_store.acquire_key().await.expect("active selected");
    assert_eq!(lease.id, active_id);

    proxy
        .key_store
        .mark_quota_exhausted("tvly-low-quota-active")
        .await
        .expect("active exhausted");
    let lease = proxy
        .key_store
        .acquire_key()
        .await
        .expect("regular exhausted selected");
    assert_ne!(lease.id, depleted_id);

    proxy
        .key_store
        .quarantine_key_by_id(
            &regular_id,
            "/api/tavily/search",
            "test_quarantine",
            "test quarantine",
            "test",
        )
        .await
        .expect("quarantine regular exhausted");
    proxy
        .key_store
        .quarantine_key_by_id(
            &active_id,
            "/api/tavily/search",
            "test_quarantine",
            "test quarantine",
            "test",
        )
        .await
        .expect("quarantine formerly active exhausted");
    let lease = proxy
        .key_store
        .acquire_key()
        .await
        .expect("depleted selected as final fallback");
    assert_eq!(lease.id, depleted_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn low_quota_depleted_key_success_does_not_auto_restore_until_next_month() {
    let db_path = temp_db_path("low-quota-depletion-no-restore");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-low-quota-no-restore".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    sqlx::query("UPDATE api_keys SET quota_limit = 1000, quota_remaining = 5 WHERE id = ?")
        .bind(&key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed quota");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_QUOTA_EXHAUSTED,
                tavily_status_code: Some(432),
                key_health_action: KeyHealthAction::MarkExhausted,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("record depletion");

    let restore_effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_SUCCESS,
                tavily_status_code: Some(200),
                key_health_action: KeyHealthAction::None,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("suppressed restore");
    assert_eq!(restore_effect.code, KEY_EFFECT_NONE);

    let status: String = sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ?")
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch status");
    assert_eq!(status, STATUS_EXHAUSTED);

    let old_month_start = start_of_month(Utc::now()).timestamp() - 31 * 24 * 60 * 60;
    sqlx::query("UPDATE api_key_low_quota_depletions SET month_start = ? WHERE key_id = ?")
        .bind(old_month_start)
        .bind(&key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("move depletion to previous month");

    let restore_effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_SUCCESS,
                tavily_status_code: Some(200),
                key_health_action: KeyHealthAction::None,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("restore after month boundary");
    assert_eq!(restore_effect.code, KEY_EFFECT_RESTORED_ACTIVE);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn usage_error_quarantine_appends_audit_record() {
    let db_path = temp_db_path("maintenance-usage-quarantine");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-usage-quarantine".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch key id");

    proxy
        .maybe_quarantine_usage_error(
            &key_id,
            "/api/tavily/usage",
            &ProxyError::UsageHttp {
                status: StatusCode::UNAUTHORIZED,
                body: "The account associated with this API key has been deactivated.".to_string(),
            },
        )
        .await
        .expect("usage quarantine");

    let op_codes = sqlx::query_scalar::<_, String>(
        "SELECT operation_code FROM api_key_maintenance_records WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch maintenance operations");
    assert_eq!(op_codes, vec![MAINTENANCE_OP_AUTO_QUARANTINE.to_string()]);

    let _ = std::fs::remove_file(db_path);
}

async fn fetch_all_api_key_ids(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT id FROM api_keys ORDER BY id ASC")
        .fetch_all(pool)
        .await
        .expect("fetch api key ids")
}

fn rank_mcp_affinity_key_ids(
    subject: &str,
    mut key_ids: Vec<String>,
    desired_count: usize,
) -> Vec<String> {
    key_ids.sort_by(|left, right| {
        let mut left_digest = Sha256::new();
        left_digest.update(subject.as_bytes());
        left_digest.update(b":");
        left_digest.update(left.as_bytes());
        let left_score: [u8; 32] = left_digest.finalize().into();

        let mut right_digest = Sha256::new();
        right_digest.update(subject.as_bytes());
        right_digest.update(b":");
        right_digest.update(right.as_bytes());
        let right_score: [u8; 32] = right_digest.finalize().into();

        right_score.cmp(&left_score).then_with(|| left.cmp(right))
    });
    key_ids.truncate(desired_count.max(1).min(key_ids.len()));
    key_ids
}

fn sha256_hex(value: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(value.as_bytes()).into();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[test]
fn mcp_session_init_retry_after_secs_supports_seconds_dates_defaults_and_clamp() {
    let now = 1_700_000_000_i64;
    let mut headers = reqwest::header::HeaderMap::new();

    assert_eq!(
        TavilyProxy::mcp_session_init_retry_after_secs(&headers, now),
        60
    );

    headers.insert(
        "retry-after",
        reqwest::header::HeaderValue::from_static("15"),
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_retry_after_secs(&headers, now),
        30
    );

    headers.insert(
        "retry-after",
        reqwest::header::HeaderValue::from_static("600"),
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_retry_after_secs(&headers, now),
        300
    );

    let future = httpdate::fmt_http_date(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs((now + 90) as u64),
    );
    headers.insert(
        "retry-after",
        reqwest::header::HeaderValue::from_str(&future).expect("future retry-after header"),
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_retry_after_secs(&headers, now),
        90
    );

    let past = httpdate::fmt_http_date(
        std::time::UNIX_EPOCH + std::time::Duration::from_secs((now - 5) as u64),
    );
    headers.insert(
        "retry-after",
        reqwest::header::HeaderValue::from_str(&past).expect("past retry-after header"),
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_retry_after_secs(&headers, now),
        30
    );

    headers.insert(
        "retry-after",
        reqwest::header::HeaderValue::from_static("not-a-number"),
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_retry_after_secs(&headers, now),
        60
    );
}

#[test]
fn mcp_session_init_candidate_order_prefers_cooldown_then_pressure_then_lru() {
    let mut candidates = vec![
        McpSessionInitCandidate {
            key_id: "stable-0".to_string(),
            stable_rank_index: 0,
            cooldown_until: Some(200),
            recent_rate_limited_count: 0,
            recent_billable_request_count: 0,
            active_session_count: 0,
            last_used_at: 100,
        },
        McpSessionInitCandidate {
            key_id: "stable-1".to_string(),
            stable_rank_index: 1,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 5,
            active_session_count: 2,
            last_used_at: 80,
        },
        McpSessionInitCandidate {
            key_id: "stable-2".to_string(),
            stable_rank_index: 2,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 2,
            active_session_count: 1,
            last_used_at: 60,
        },
        McpSessionInitCandidate {
            key_id: "stable-3".to_string(),
            stable_rank_index: 3,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 2,
            active_session_count: 1,
            last_used_at: 10,
        },
    ];

    TavilyProxy::order_mcp_session_init_candidates(&mut candidates);

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key_id.as_str())
            .collect::<Vec<_>>(),
        vec!["stable-3", "stable-2", "stable-1", "stable-0"]
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_selection_effect(&candidates).code,
        KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED,
    );
}

#[test]
fn mcp_session_init_candidate_order_uses_stable_rank_as_last_tiebreaker() {
    let mut candidates = vec![
        McpSessionInitCandidate {
            key_id: "rank-1".to_string(),
            stable_rank_index: 1,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 3,
            active_session_count: 1,
            last_used_at: 10,
        },
        McpSessionInitCandidate {
            key_id: "rank-0".to_string(),
            stable_rank_index: 0,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 3,
            active_session_count: 1,
            last_used_at: 10,
        },
    ];

    TavilyProxy::order_mcp_session_init_candidates(&mut candidates);

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key_id.as_str())
            .collect::<Vec<_>>(),
        vec!["rank-0", "rank-1"]
    );
}

#[test]
fn mcp_session_init_candidate_order_prefers_lower_recent_rate_limit_heat_before_pressure() {
    let mut candidates = vec![
        McpSessionInitCandidate {
            key_id: "cooler".to_string(),
            stable_rank_index: 1,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 9,
            active_session_count: 4,
            last_used_at: 80,
        },
        McpSessionInitCandidate {
            key_id: "hotter".to_string(),
            stable_rank_index: 0,
            cooldown_until: None,
            recent_rate_limited_count: 2,
            recent_billable_request_count: 1,
            active_session_count: 0,
            last_used_at: 1,
        },
    ];

    TavilyProxy::order_mcp_session_init_candidates(&mut candidates);

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key_id.as_str())
            .collect::<Vec<_>>(),
        vec!["cooler", "hotter"]
    );
    assert_eq!(
        TavilyProxy::mcp_session_init_selection_effect(&candidates).code,
        KEY_EFFECT_MCP_SESSION_INIT_RATE_LIMIT_AVOIDED,
    );
}

#[test]
fn http_project_affinity_candidate_order_prefers_cooldown_then_rate_limit_then_pressure_then_lru() {
    let mut candidates = vec![
        HttpProjectAffinityCandidate {
            key_id: "stable-0".to_string(),
            stable_rank_index: 0,
            cooldown_until: Some(200),
            recent_rate_limited_count: 0,
            recent_billable_request_count: 0,
            last_used_at: 100,
        },
        HttpProjectAffinityCandidate {
            key_id: "stable-1".to_string(),
            stable_rank_index: 1,
            cooldown_until: None,
            recent_rate_limited_count: 1,
            recent_billable_request_count: 1,
            last_used_at: 30,
        },
        HttpProjectAffinityCandidate {
            key_id: "stable-2".to_string(),
            stable_rank_index: 2,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 5,
            last_used_at: 10,
        },
        HttpProjectAffinityCandidate {
            key_id: "stable-3".to_string(),
            stable_rank_index: 3,
            cooldown_until: None,
            recent_rate_limited_count: 0,
            recent_billable_request_count: 1,
            last_used_at: 5,
        },
    ];

    TavilyProxy::order_http_project_affinity_candidates(&mut candidates);

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key_id.as_str())
            .collect::<Vec<_>>(),
        vec!["stable-3", "stable-2", "stable-1", "stable-0"]
    );
    assert_eq!(
        TavilyProxy::http_project_affinity_selection_effect(&candidates).code,
        KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
    );
}

#[tokio::test]
async fn http_project_affinity_reuses_existing_binding_for_same_project() {
    let db_path = temp_db_path("http-project-affinity-reuse");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-project-affinity-reuse-a".to_string(),
            "tvly-http-project-affinity-reuse-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("http-project-affinity-reuse"))
        .await
        .expect("create token");

    let first = proxy
        .acquire_key_for_http_project(Some(&token.id), Some("project-alpha"))
        .await
        .expect("select project key")
        .expect("project affinity selection");
    assert_eq!(
        first.binding_effect.code,
        KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND,
    );
    assert!(
        matches!(
            first.selection_effect.code.as_str(),
            KEY_EFFECT_NONE
                | KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED
                | KEY_EFFECT_HTTP_PROJECT_AFFINITY_RATE_LIMIT_AVOIDED
                | KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED
        ),
        "first selection should explain any avoided hotter key separately from the binding event",
    );

    let second = proxy
        .acquire_key_for_http_project(Some(&token.id), Some("project-alpha"))
        .await
        .expect("reselect project key")
        .expect("project affinity selection");
    assert_eq!(second.lease.id, first.lease.id);
    assert_eq!(
        second.binding_effect.code,
        KEY_EFFECT_HTTP_PROJECT_AFFINITY_REUSED,
    );
    assert_eq!(second.selection_effect.code, KEY_EFFECT_NONE);

    let binding = proxy
        .key_store
        .get_http_project_api_key_affinity(
            &format!("token:{}", token.id),
            &sha256_hex("project-alpha"),
        )
        .await
        .expect("load persisted binding")
        .expect("binding should exist");
    assert_eq!(binding.api_key_id, first.lease.id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn http_project_affinity_separates_distinct_projects_for_same_owner() {
    let db_path = temp_db_path("http-project-affinity-distinct-projects");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-project-distinct-a".to_string(),
            "tvly-http-project-distinct-b".to_string(),
            "tvly-http-project-distinct-c".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_mcp_session_affinity_key_count(2)
        .await
        .expect("set stable pool size");
    let token = proxy
        .create_access_token(Some("http-project-distinct-projects"))
        .await
        .expect("create token");

    let first = proxy
        .acquire_key_for_http_project(Some(&token.id), Some("project-alpha"))
        .await
        .expect("select alpha key")
        .expect("alpha selection");
    let second = proxy
        .acquire_key_for_http_project(Some(&token.id), Some("project-beta"))
        .await
        .expect("select beta key")
        .expect("beta selection");

    let alpha = proxy
        .key_store
        .get_http_project_api_key_affinity(
            &format!("token:{}", token.id),
            &sha256_hex("project-alpha"),
        )
        .await
        .expect("load alpha binding")
        .expect("alpha binding");
    let beta = proxy
        .key_store
        .get_http_project_api_key_affinity(
            &format!("token:{}", token.id),
            &sha256_hex("project-beta"),
        )
        .await
        .expect("load beta binding")
        .expect("beta binding");
    assert_eq!(alpha.api_key_id, first.lease.id);
    assert_eq!(beta.api_key_id, second.lease.id);
    assert_ne!(alpha.project_id_hash, beta.project_id_hash);

    let row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM http_project_api_key_affinity WHERE owner_subject = ?",
    )
    .bind(format!("token:{}", token.id))
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count project bindings");
    assert_eq!(row_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn http_project_affinity_same_project_isolated_by_different_users() {
    let db_path = temp_db_path("http-project-affinity-user-isolation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-project-user-isolation-a".to_string(),
            "tvly-http-project-user-isolation-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let first_token = proxy
        .create_access_token(Some("http-project-user-isolation-a"))
        .await
        .expect("create first token");
    let second_token = proxy
        .create_access_token(Some("http-project-user-isolation-b"))
        .await
        .expect("create second token");
    let now = Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO users (id, display_name, username, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("user-project-a")
    .bind("User Project A")
    .bind("user-project-a")
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert first user");
    sqlx::query(
        "INSERT INTO users (id, display_name, username, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("user-project-b")
    .bind("User Project B")
    .bind("user-project-b")
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert second user");
    sqlx::query(
        "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind("user-project-a")
    .bind(&first_token.id)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("bind first token");
    sqlx::query(
        "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind("user-project-b")
    .bind(&second_token.id)
    .bind(now)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("bind second token");
    proxy
        .key_store
        .cache_token_binding(&first_token.id, Some("user-project-a"))
        .await;
    proxy
        .key_store
        .cache_token_binding(&second_token.id, Some("user-project-b"))
        .await;

    proxy
        .acquire_key_for_http_project(Some(&first_token.id), Some("shared-project"))
        .await
        .expect("select first user project key")
        .expect("first user project selection");
    proxy
        .acquire_key_for_http_project(Some(&second_token.id), Some("shared-project"))
        .await
        .expect("select second user project key")
        .expect("second user project selection");

    let rows = sqlx::query_as::<_, (String, String)>(
        r#"SELECT owner_subject, project_id_hash
           FROM http_project_api_key_affinity
           WHERE project_id_hash = ?
           ORDER BY owner_subject ASC"#,
    )
    .bind(sha256_hex("shared-project"))
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch project rows");

    assert_eq!(
        rows,
        vec![
            (
                "user:user-project-a".to_string(),
                sha256_hex("shared-project"),
            ),
            (
                "user:user-project-b".to_string(),
                sha256_hex("shared-project"),
            ),
        ]
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn http_project_affinity_rebinds_after_cooldown() {
    let db_path = temp_db_path("http-project-affinity-cooldown-rebind");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-project-cooldown-a".to_string(),
            "tvly-http-project-cooldown-b".to_string(),
            "tvly-http-project-cooldown-c".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_mcp_session_affinity_key_count(2)
        .await
        .expect("set stable pool size");
    let token = proxy
        .create_access_token(Some("http-project-cooldown-rebind"))
        .await
        .expect("create token");
    let project_hash = sha256_hex("project-cooldown");
    let subject = format!("token:{}:project:{}", token.id, project_hash);
    let ranked = rank_mcp_affinity_key_ids(
        &subject,
        fetch_all_api_key_ids(&proxy.key_store.pool).await,
        2,
    );
    let hotter_key_id = ranked[0].clone();
    let cooler_key_id = ranked[1].clone();
    let now = Utc::now().timestamp();

    proxy
        .key_store
        .set_http_project_api_key_affinity(
            &format!("token:{}", token.id),
            &project_hash,
            &hotter_key_id,
        )
        .await
        .expect("seed project binding");
    proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &hotter_key_id,
            scope: HTTP_PROJECT_AFFINITY_BACKOFF_SCOPE,
            cooldown_until: now + 120,
            retry_after_secs: 120,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now,
        })
        .await
        .expect("arm project cooldown");

    let selection = proxy
        .acquire_key_for_http_project(Some(&token.id), Some("project-cooldown"))
        .await
        .expect("select project key after cooldown")
        .expect("project affinity selection");
    assert_eq!(selection.lease.id, cooler_key_id);
    assert_eq!(
        selection.binding_effect.code,
        KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND,
    );
    assert_eq!(
        selection.selection_effect.code,
        KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
    );

    let rebound = proxy
        .key_store
        .get_http_project_api_key_affinity(&format!("token:{}", token.id), &project_hash)
        .await
        .expect("load rebound binding")
        .expect("rebound binding should exist");
    assert_eq!(rebound.api_key_id, cooler_key_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn http_project_affinity_arms_backoff_on_429_and_avoids_hot_key_on_next_request() {
    let db_path = temp_db_path("http-project-affinity-arm-backoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-project-arm-backoff-a".to_string(),
            "tvly-http-project-arm-backoff-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_mcp_session_affinity_key_count(2)
        .await
        .expect("set stable pool size");
    let token = proxy
        .create_access_token(Some("http-project-arm-backoff"))
        .await
        .expect("create token");
    let project_id = "project-arm-backoff";
    let project_hash = sha256_hex(project_id);
    let subject = format!("token:{}:project:{}", token.id, project_hash);
    let ranked = rank_mcp_affinity_key_ids(
        &subject,
        fetch_all_api_key_ids(&proxy.key_store.pool).await,
        2,
    );
    let hotter_key_id = ranked[0].clone();
    let cooler_key_id = ranked[1].clone();
    let key_rows =
        sqlx::query_as::<_, (String, String)>("SELECT id, api_key FROM api_keys ORDER BY id ASC")
            .fetch_all(&proxy.key_store.pool)
            .await
            .expect("fetch key rows");
    let hotter_secret = key_rows
        .iter()
        .find(|(id, _)| id == &hotter_key_id)
        .map(|(_, secret)| secret.clone())
        .expect("hotter key secret");
    let cooler_secret = key_rows
        .iter()
        .find(|(id, _)| id == &cooler_key_id)
        .map(|(_, secret)| secret.clone())
        .expect("cooler key secret");

    proxy
        .key_store
        .set_http_project_api_key_affinity(
            &format!("token:{}", token.id),
            &project_hash,
            &hotter_key_id,
        )
        .await
        .expect("seed project binding");

    let app = Router::new().route(
        "/search",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let hotter_secret = hotter_secret.clone();
            let cooler_secret = cooler_secret.clone();
            async move {
                let api_key = body
                    .get("api_key")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .unwrap_or("")
                    .to_string();
                assert_eq!(api_key, auth);

                if api_key == hotter_secret {
                    let mut response_headers = HeaderMap::new();
                    response_headers.insert("retry-after", HeaderValue::from_static("90"));
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        response_headers,
                        Json(serde_json::json!({
                            "detail": { "error": "Too many requests" }
                        })),
                    )
                        .into_response()
                } else {
                    assert_eq!(api_key, cooler_secret);
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
                    )
                        .into_response()
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
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    headers.insert("x-project-id", HeaderValue::from_static(project_id));
    let options = serde_json::json!({ "query": "project backoff" });

    let (first_resp, first_analysis) = proxy
        .proxy_http_json_endpoint(
            &usage_base,
            "/search",
            Some(&token.id),
            Some(project_id),
            &Method::POST,
            "/api/tavily/search",
            options.clone(),
            &headers,
            true,
        )
        .await
        .expect("first project request should complete");
    assert_eq!(first_resp.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        first_analysis.failure_kind.as_deref(),
        Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
    );

    let cooldown = proxy
        .key_store
        .list_active_api_key_transient_backoffs(
            std::slice::from_ref(&hotter_key_id),
            HTTP_PROJECT_AFFINITY_BACKOFF_SCOPE,
            Utc::now().timestamp(),
        )
        .await
        .expect("load project cooldowns");
    assert!(cooldown.contains_key(&hotter_key_id));

    let (second_resp, second_analysis) = proxy
        .proxy_http_json_endpoint(
            &usage_base,
            "/search",
            Some(&token.id),
            Some(project_id),
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
            true,
        )
        .await
        .expect("second project request should complete");
    assert_eq!(second_resp.status, StatusCode::OK);
    assert_eq!(
        second_resp.api_key_id.as_deref(),
        Some(cooler_key_id.as_str())
    );
    assert_eq!(
        second_analysis.key_effect.code,
        KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
    );

    let rebound = proxy
        .key_store
        .get_http_project_api_key_affinity(&format!("token:{}", token.id), &project_hash)
        .await
        .expect("load rebound binding")
        .expect("binding should exist");
    assert_eq!(rebound.api_key_id, cooler_key_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mcp_session_init_backoff_store_extends_without_shortening_and_gc_cleans_expired_rows() {
    let db_path = temp_db_path("mcp-init-backoff-store");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-mcp-init-backoff-store".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch key id");
    let now = Utc::now().timestamp();

    let first = proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &key_id,
            scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
            cooldown_until: now + 60,
            retry_after_secs: 60,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now,
        })
        .await
        .expect("arm first cooldown");
    assert_eq!(
        first,
        Some(ApiKeyTransientBackoffState {
            cooldown_until: now + 60,
            retry_after_secs: 60,
        })
    );

    let shorter = proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &key_id,
            scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
            cooldown_until: now + 30,
            retry_after_secs: 30,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now: now + 1,
        })
        .await
        .expect("arm shorter cooldown");
    assert_eq!(shorter, None);

    let longer = proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &key_id,
            scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
            cooldown_until: now + 120,
            retry_after_secs: 120,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now: now + 2,
        })
        .await
        .expect("arm longer cooldown");
    assert_eq!(
        longer,
        Some(ApiKeyTransientBackoffState {
            cooldown_until: now + 120,
            retry_after_secs: 120,
        })
    );

    let active = proxy
        .key_store
        .list_active_api_key_transient_backoffs(
            std::slice::from_ref(&key_id),
            MCP_SESSION_INIT_BACKOFF_SCOPE,
            now + 3,
        )
        .await
        .expect("list active cooldowns");
    assert_eq!(
        active.get(&key_id),
        Some(&ApiKeyTransientBackoffState {
            cooldown_until: now + 120,
            retry_after_secs: 120,
        })
    );

    let deleted = proxy
        .key_store
        .delete_expired_api_key_transient_backoffs(now + 121)
        .await
        .expect("gc cooldowns");
    assert_eq!(deleted, 1);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_key_transient_backoffs")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count cooldown rows");
    assert_eq!(remaining, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn http_key_selection_ignores_mcp_session_init_backoff() {
    let db_path = temp_db_path("http-selection-ignore-mcp-backoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-selection-ignore-a".to_string(),
            "tvly-http-selection-ignore-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("http-selection-ignore-mcp-backoff"))
        .await
        .expect("create token");
    let key_ids = fetch_all_api_key_ids(&proxy.key_store.pool).await;
    let primary_key_id = key_ids[0].clone();
    let now = Utc::now().timestamp();

    proxy
        .key_store
        .set_token_primary_api_key_affinity(&token.id, None, &primary_key_id)
        .await
        .expect("set token primary affinity");
    proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &primary_key_id,
            scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
            cooldown_until: now + 120,
            retry_after_secs: 120,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now,
        })
        .await
        .expect("arm mcp init backoff");

    let lease = proxy
        .acquire_key_for(Some(&token.id))
        .await
        .expect("acquire http key");
    assert_eq!(lease.id, primary_key_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn http_429_without_project_affinity_still_arms_mcp_session_init_backoff() {
    let db_path = temp_db_path("http-no-project-arms-mcp-init-backoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-http-no-project-backoff-a".to_string(),
            "tvly-http-no-project-backoff-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("http-no-project-backoff"))
        .await
        .expect("create token");
    let key_rows =
        sqlx::query_as::<_, (String, String)>("SELECT id, api_key FROM api_keys ORDER BY id ASC")
            .fetch_all(&proxy.key_store.pool)
            .await
            .expect("fetch key rows");
    let primary_key_id = key_rows[0].0.clone();
    let primary_secret = key_rows[0].1.clone();
    proxy
        .key_store
        .set_token_primary_api_key_affinity(&token.id, None, &primary_key_id)
        .await
        .expect("set token primary affinity");

    let app = Router::new().route(
        "/search",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let primary_secret = primary_secret.clone();
            async move {
                let api_key = body
                    .get("api_key")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .unwrap_or("")
                    .to_string();
                assert_eq!(api_key, auth);
                assert_eq!(api_key, primary_secret);

                let mut response_headers = HeaderMap::new();
                response_headers.insert("retry-after", HeaderValue::from_static("45"));
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    response_headers,
                    Json(serde_json::json!({
                        "detail": { "error": "Too many requests" }
                    })),
                )
                    .into_response()
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
    headers.insert("content-type", HeaderValue::from_static("application/json"));
    let options = serde_json::json!({ "query": "legacy mcp init backoff" });

    let (resp, analysis) = proxy
        .proxy_http_json_endpoint(
            &usage_base,
            "/search",
            Some(&token.id),
            None,
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
            true,
        )
        .await
        .expect("http request should complete");
    assert_eq!(resp.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        analysis.failure_kind.as_deref(),
        Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
    );
    assert_eq!(
        analysis.key_effect.code,
        KEY_EFFECT_MCP_SESSION_INIT_BACKOFF_SET,
    );

    let cooldown = proxy
        .key_store
        .list_active_api_key_transient_backoffs(
            std::slice::from_ref(&primary_key_id),
            MCP_SESSION_INIT_BACKOFF_SCOPE,
            Utc::now().timestamp(),
        )
        .await
        .expect("load legacy mcp-init cooldowns");
    assert!(cooldown.contains_key(&primary_key_id));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_result_get_429_still_arms_mcp_session_init_backoff() {
    let db_path = temp_db_path("research-result-get-mcp-init-backoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-research-result-backoff-a".to_string(),
            "tvly-research-result-backoff-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("research-result-backoff"))
        .await
        .expect("create token");
    let request_id = "req-result-backoff";
    let key_rows =
        sqlx::query_as::<_, (String, String)>("SELECT id, api_key FROM api_keys ORDER BY id ASC")
            .fetch_all(&proxy.key_store.pool)
            .await
            .expect("fetch key rows");
    let affinity_key_id = key_rows[0].0.clone();
    let affinity_secret = key_rows[0].1.clone();
    proxy
        .record_research_request_affinity(request_id, &affinity_key_id, &token.id)
        .await
        .expect("record research affinity");

    let upstream_path = format!("/research/{request_id}");
    let app = Router::new().route(
        &upstream_path,
        get(move |headers: HeaderMap| {
            let affinity_secret = affinity_secret.clone();
            async move {
                let auth = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .unwrap_or("")
                    .to_string();
                assert_eq!(auth, affinity_secret);

                let mut response_headers = HeaderMap::new();
                response_headers.insert("retry-after", HeaderValue::from_static("30"));
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    response_headers,
                    Json(serde_json::json!({
                        "detail": { "error": "Too many requests" }
                    })),
                )
                    .into_response()
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
    let (resp, analysis) = proxy
        .proxy_http_get_endpoint(
            &usage_base,
            &upstream_path,
            Some(&token.id),
            &Method::GET,
            &format!("/api/tavily/research/{request_id}"),
            &headers,
            true,
        )
        .await
        .expect("research result request should complete");
    assert_eq!(resp.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        analysis.failure_kind.as_deref(),
        Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
    );
    assert_eq!(
        analysis.key_effect.code,
        KEY_EFFECT_MCP_SESSION_INIT_BACKOFF_SET,
    );

    let backoff_row = sqlx::query_as::<_, (Option<i64>,)>(
        r#"SELECT source_request_log_id
           FROM api_key_transient_backoffs
           WHERE key_id = ? AND scope = ?"#,
    )
    .bind(&affinity_key_id)
    .bind(MCP_SESSION_INIT_BACKOFF_SCOPE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load research result backoff row");
    assert_eq!(backoff_row.0, resp.request_log_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn mcp_session_init_uses_least_bad_key_even_when_every_pool_candidate_is_cooled() {
    let db_path = temp_db_path("mcp-init-all-hot");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-mcp-init-all-hot-a".to_string(),
            "tvly-mcp-init-all-hot-b".to_string(),
            "tvly-mcp-init-all-hot-c".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    proxy
        .set_mcp_session_affinity_key_count(2)
        .await
        .expect("set affinity count");
    let token = proxy
        .create_access_token(Some("mcp-init-all-hot"))
        .await
        .expect("create token");

    let key_ids = fetch_all_api_key_ids(&proxy.key_store.pool).await;
    let ranked = rank_mcp_affinity_key_ids(&format!("token:{}", token.id), key_ids, 2);
    let hotter_key_id = ranked[0].clone();
    let less_hot_key_id = ranked[1].clone();
    let now = Utc::now().timestamp();

    proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &hotter_key_id,
            scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
            cooldown_until: now + 180,
            retry_after_secs: 180,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now,
        })
        .await
        .expect("arm hotter cooldown");
    proxy
        .key_store
        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
            key_id: &less_hot_key_id,
            scope: MCP_SESSION_INIT_BACKOFF_SCOPE,
            cooldown_until: now + 90,
            retry_after_secs: 90,
            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            source_request_log_id: None,
            now,
        })
        .await
        .expect("arm less-hot cooldown");

    let selection = proxy
        .acquire_key_for_mcp_session_init(Some(&token.id))
        .await
        .expect("acquire mcp session init key");
    assert_eq!(selection.lease.id, less_hot_key_id);
    assert_eq!(
        selection.key_effect.code,
        KEY_EFFECT_MCP_SESSION_INIT_COOLDOWN_AVOIDED,
    );

    let _ = std::fs::remove_file(db_path);
}

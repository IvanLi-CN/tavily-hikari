#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_refresh_invalidates_cached_record() {
    let db_path = temp_db_path("proxy-affinity-hint-cache-refresh");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-cache-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let warmed = proxy
        .load_proxy_affinity_record(&key_id)
        .await
        .expect("warm affinity cache");
    assert_eq!(
        warmed.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );

    let (_, refreshed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-cache-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://18.183.246.69:8080"),
        )
        .await
        .expect("refresh hint-only affinity");
    assert_eq!(refreshed_status, ApiKeyUpsertStatus::Existed);

    let refreshed = proxy
        .load_proxy_affinity_record(&key_id)
        .await
        .expect("reload affinity after refresh");
    assert_eq!(
        refreshed.primary_proxy_key.as_deref(),
        Some("http://18.183.246.69:8080"),
        "re-importing a hinted key should evict stale cache entries before the next request"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_direct_affinity_does_not_persist() {
    let db_path = temp_db_path("proxy-affinity-hint-only-direct");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-direct",
            None,
            None,
            None,
            &geo_origin,
            Some(forward_proxy::FORWARD_PROXY_DIRECT_KEY),
        )
        .await
        .expect("key created without persisting direct hint");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query direct hint affinity row");
    assert!(
        affinity_row.0.is_none() && affinity_row.1.is_none(),
        "direct validation results must not become durable affinity records"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for direct hint-only key");
    assert!(
        !plan.is_empty(),
        "direct-only validation results should still allow runtime fallback selection"
    );

    let affinity_row_after_plan: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query direct hint affinity row after plan build");
    assert!(
        affinity_row_after_plan.0.is_none() && affinity_row_after_plan.1.is_none(),
        "runtime fallback planning must not convert direct hints into durable affinity"
    );

    let marker_updated_at_before: i64 =
        sqlx::query_scalar("SELECT updated_at FROM forward_proxy_key_affinity WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("query marker timestamp before repeat plan build");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let _ = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("rebuild direct-hint runtime plan");
    let marker_updated_at_after: i64 =
        sqlx::query_scalar("SELECT updated_at FROM forward_proxy_key_affinity WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("query marker timestamp after repeat plan build");
    assert_eq!(
        marker_updated_at_after, marker_updated_at_before,
        "explicit empty markers should not churn the database on every runtime plan build"
    );

    proxy
        .promote_proxy_affinity_secondary(&key_id, "http://18.183.246.69:8080")
        .await
        .expect("learn durable affinity after direct hint");
    let learned_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query learned affinity after first successful routed request");
    assert_eq!(
        learned_affinity.0.as_deref(),
        Some("http://18.183.246.69:8080"),
        "empty affinity markers should be replaceable once a real proxy success is observed"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_direct_affinity_keeps_direct_runtime_fallback() {
    let db_path = temp_db_path("proxy-affinity-hint-only-direct-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: true,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-direct-fallback",
            None,
            None,
            None,
            &geo_origin,
            Some(forward_proxy::FORWARD_PROXY_DIRECT_KEY),
        )
        .await
        .expect("key created with direct-only hint");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for direct-only deployment");
    assert_eq!(
        plan.len(),
        1,
        "direct-only deployments should keep one direct fallback"
    );
    assert_eq!(plan[0].key, forward_proxy::FORWARD_PROXY_DIRECT_KEY);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_plain_key_without_registration_metadata_still_synthesizes_affinity() {
    let db_path = temp_db_path("proxy-affinity-plain-key-synthesizes");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status("tvly-plain-no-registration")
        .await
        .expect("plain key created");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let before_plan: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query affinity before runtime reconciliation");
    assert!(
        before_plan.is_none(),
        "plain keys should start without an explicit affinity marker"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for plain key");
    assert!(
        !plan.is_empty(),
        "plain keys should still get a ranked runtime attempt plan"
    );

    let synthesized: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query synthesized affinity after runtime reconciliation");
    assert!(
        synthesized.0.is_some(),
        "plain keys must still materialize a durable primary affinity"
    );

    if let Some(secondary) = synthesized.1.clone() {
        proxy
            .promote_proxy_affinity_secondary(&key_id, &secondary)
            .await
            .expect("promote synthesized secondary");
        let promoted: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
        )
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("query promoted affinity");
        assert_eq!(
            promoted.0.as_deref(),
            Some(secondary.as_str()),
            "plain keys should keep the existing self-healing promotion behavior"
        );
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_stale_hint_only_proxy_affinity_clears_existing_affinity() {
    let db_path = temp_db_path("proxy-affinity-stale-hint-clears-existing");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-stale-hint-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with valid hint");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let (_, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-stale-hint-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://9.9.9.9:8080"),
        )
        .await
        .expect("existing key refreshed with stale hint");
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row after stale refresh");
    assert!(
        affinity_row.0.is_none() && affinity_row.1.is_none(),
        "stale hint-only refresh should clear the old affinity instead of keeping it"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_preserves_existing_registration_affinity()
 {
    let db_path = temp_db_path("proxy-affinity-hint-preserves-registration");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-hint-preserves-registration",
            None,
            Some("1.1.1.1"),
            Some("US Westfield (MA)"),
            &geo_origin,
        )
        .await
        .expect("key created with registration affinity");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let (_, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-preserves-registration",
            None,
            None,
            None,
            &geo_origin,
            Some("http://18.183.246.69:8080"),
        )
        .await
        .expect("existing registration key refreshed with hint-only payload");
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row after hint-only refresh");
    assert_eq!(
        affinity_row.0.as_deref(),
        Some("http://1.1.1.1:8080"),
        "hint-only refresh must not override durable registration-based affinity"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_for_same_region_uses_ranked_match_within_region_candidates() {
    let db_path = temp_db_path("proxy-affinity-region-ranked-match");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://1.1.1.1:8080".to_string(),
                    "http://1.0.0.1:8080".to_string(),
                    "http://18.183.246.69:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (subject, expected_primary) = {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.ensure_non_zero_weight();
        (0..256usize)
            .filter_map(|index| {
                let subject = format!("subject:ranked-region:{index}");
                let ranked =
                    manager.rank_candidates_for_subject(&subject, &HashSet::new(), false, 3);
                let first_hk = ranked.into_iter().find(|endpoint| {
                    matches!(
                        endpoint.key.as_str(),
                        "http://1.1.1.1:8080" | "http://1.0.0.1:8080"
                    )
                })?;
                (first_hk.key == "http://1.0.0.1:8080").then_some((subject, first_hk.key))
            })
            .next()
            .expect("find subject whose ranked HK candidate is not the first configured node")
    };

    let (affinity, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            &subject,
            &geo_origin,
            Some("103.232.214.107"),
            Some("HK"),
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("same-region proxy affinity");
    assert_eq!(
        affinity.primary_proxy_key.as_deref(),
        Some(expected_primary.as_str()),
        "same-region selection should stay inside the region-matched set and follow ranked order"
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::SameRegion),
        "same-region ranked picks should still report same_region"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn build_proxy_attempt_plan_prefers_same_region_candidates_before_other_regions() {
    let db_path = temp_db_path("proxy-attempt-plan-region-order");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let mut proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    proxy.api_key_geo_origin = geo_origin.clone();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                    "http://1.0.0.1:8080".to_string(),
                    "http://8.8.8.8:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: true,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, _) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-region-plan",
            None,
            Some("1.1.1.1"),
            Some("HK"),
            &geo_origin,
        )
        .await
        .expect("key created with region-aware proxy affinity");
    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build proxy attempt plan");
    let plan_keys = plan.into_iter().map(|item| item.key).collect::<Vec<_>>();

    assert_eq!(
        plan_keys.first().map(String::as_str),
        Some("http://1.1.1.1:8080")
    );
    let same_region_pos = plan_keys
        .iter()
        .position(|key| key == "http://1.0.0.1:8080")
        .expect("same-region backup present in plan");
    let other_region_positions = ["http://18.183.246.69:8080", "http://8.8.8.8:8080"]
        .into_iter()
        .filter_map(|key| plan_keys.iter().position(|item| item == key))
        .collect::<Vec<_>>();
    assert!(
        other_region_positions
            .iter()
            .all(|position| same_region_pos < *position),
        "same-region backup should be attempted before other-region candidates"
    );
    assert!(
        !plan_keys
            .iter()
            .any(|key| key == forward_proxy::FORWARD_PROXY_DIRECT_KEY),
        "direct should only be considered after all proxy candidates fail"
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn analyze_http_attempt_treats_2xx_as_success() {
    let body = br#"{"query":"test","results":[]}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_SUCCESS);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn analyze_http_attempt_uses_structured_status_and_marks_quota_exhausted() {
    let body = br#"{"status":432,"error":"quota_exhausted"}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.tavily_status_code, Some(432));
}

#[test]
fn analyze_http_attempt_treats_http_errors_as_error() {
    let body = br#"{"error":"upstream failed"}"#;
    let analysis = analyze_http_attempt(StatusCode::INTERNAL_SERVER_ERROR, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(500));
}

#[test]
fn analyze_http_attempt_treats_failed_status_string_as_error() {
    let body = br#"{"status":"failed"}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn analyze_http_attempt_treats_pending_status_string_as_success() {
    let body = br#"{"status":"pending"}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_SUCCESS);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn analyze_http_attempt_prioritizes_structured_status_code_for_quota_exhausted() {
    let body = br#"{"status":432,"detail":{"status":"failed"}}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.tavily_status_code, Some(432));
}

#[test]
fn analyze_http_attempt_marks_401_deactivated_as_quarantine() {
    let body =
        br#"{"detail":{"error":"The account associated with this API key has been deactivated."}}"#;
    let analysis = analyze_http_attempt(StatusCode::UNAUTHORIZED, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    match analysis.key_health_action {
        KeyHealthAction::Quarantine(decision) => {
            assert_eq!(decision.reason_code, "account_deactivated");
            assert!(decision.reason_summary.contains("HTTP 401"));
        }
        other => panic!("expected quarantine action, got {other:?}"),
    }
    assert_eq!(analysis.tavily_status_code, Some(401));
}

#[test]
fn extract_research_request_id_accepts_snake_and_camel_case() {
    let snake = br#"{"request_id":"req-snake"}"#;
    let camel = br#"{"requestId":"req-camel"}"#;
    assert_eq!(
        extract_research_request_id(snake).as_deref(),
        Some("req-snake")
    );
    assert_eq!(
        extract_research_request_id(camel).as_deref(),
        Some("req-camel")
    );
}

#[test]
fn extract_research_request_id_from_path_decodes_segment() {
    assert_eq!(
        extract_research_request_id_from_path("/research/req%2Fabc").as_deref(),
        Some("req/abc")
    );
}

#[test]
fn redact_api_key_bytes_removes_api_key_value() {
    let input = br#"{"api_key":"th-ABCD-secret","nested":{"api_key":"tvly-secret"}}"#;
    let redacted = redact_api_key_bytes(input);
    let text = String::from_utf8_lossy(&redacted);
    assert!(
        !text.contains("th-ABCD-secret") && !text.contains("tvly-secret"),
        "redacted payload should not contain original secrets"
    );
    assert!(
        text.contains("\"api_key\":\"***redacted***\""),
        "api_key fields should be replaced with placeholder"
    );
}

#[tokio::test]
async fn proxy_http_search_marks_key_exhausted_on_quota_status() {
    let db_path = temp_db_path("http-search-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-quota-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    // Mock Tavily HTTP /search that always returns structured status 432.
    let app = Router::new().route(
        "/search",
        post(|| async {
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": 432,
                    "error": "quota_exhausted",
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

    let usage_base = format!("http://{}", addr);

    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test" });

    let (_resp, analysis) = proxy
        .proxy_http_search(
            &usage_base,
            Some("tok1"),
            None,
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
        )
        .await
        .expect("proxy search succeeded");

    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.tavily_status_code, Some(432));

    // Verify that the key is marked exhausted in the database.
    let store = proxy.key_store.clone();
    let (status,): (String,) = sqlx::query_as("SELECT status FROM api_keys WHERE api_key = ?")
        .bind(expected_api_key)
        .fetch_one(&store.pool)
        .await
        .expect("key row exists");
    assert_eq!(status, STATUS_EXHAUSTED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_http_json_endpoint_injects_bearer_auth_when_enabled() {
    let db_path = temp_db_path("http-json-bearer-enabled");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-bearer-enabled-key";
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

                    let authorization = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    let expected_auth = format!("Bearer {}", expected_api_key);
                    assert_eq!(
                        authorization, expected_auth,
                        "upstream authorization should use Tavily key"
                    );
                    assert!(
                        !authorization.starts_with("Bearer th-"),
                        "upstream authorization must not be Hikari token"
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
    let headers = HeaderMap::new();
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
            true,
        )
        .await
        .expect("proxy request succeeds");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_http_json_endpoint_quarantines_key_on_401_deactivated() {
    let db_path = temp_db_path("http-json-quarantine-401");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-quarantine-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let app = Router::new().route(
        "/search",
        post(|| async {
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

    let usage_base = format!("http://{}", addr);
    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test" });

    let (_resp, analysis) = proxy
        .proxy_http_search(
            &usage_base,
            Some("tok1"),
            None,
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
        )
        .await
        .expect("proxy search succeeded");

    assert_eq!(analysis.status, OUTCOME_ERROR);
    match analysis.key_health_action {
        KeyHealthAction::Quarantine(ref decision) => {
            assert_eq!(decision.reason_code, "account_deactivated");
        }
        ref other => panic!("expected quarantine action, got {other:?}"),
    }

    let store = proxy.key_store.clone();
    let (status,): (String,) = sqlx::query_as("SELECT status FROM api_keys WHERE api_key = ?")
        .bind(expected_api_key)
        .fetch_one(&store.pool)
        .await
        .expect("key row exists");
    assert_eq!(status, STATUS_ACTIVE);

    let quarantine_row = sqlx::query(
        r#"SELECT source, reason_code, cleared_at FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&store.pool)
    .await
    .expect("quarantine row exists");
    let source: String = quarantine_row.try_get("source").expect("source");
    let reason_code: String = quarantine_row.try_get("reason_code").expect("reason_code");
    let cleared_at: Option<i64> = quarantine_row.try_get("cleared_at").expect("cleared_at");
    assert_eq!(source, "/api/tavily/search");
    assert_eq!(reason_code, "account_deactivated");
    assert!(cleared_at.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_request_quarantines_key_on_mcp_unauthorized() {
    let db_path = temp_db_path("mcp-quarantine-401");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "message": "Unauthorized: invalid api key"
                    },
                    "id": 1
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

    let expected_api_key = "tvly-mcp-quarantine-key";
    let upstream = format!("http://{addr}/mcp");
    let proxy = TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
        .await
        .expect("proxy created");

    let request = ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
        auth_token_id: Some("tok1".to_string()),
        prefer_mcp_session_affinity: false,
        pinned_api_key_id: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
    };

    let response = proxy.proxy_request(request).await.expect("proxy response");
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    let store = proxy.key_store.clone();
    let quarantine_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_request_quarantines_key_on_mcp_error_body_without_http_status() {
    let db_path = temp_db_path("mcp-quarantine-jsonrpc-error");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "message": "Unauthorized: invalid api key"
                },
                "id": 1
            }))
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let expected_api_key = "tvly-mcp-jsonrpc-error-key";
    let upstream = format!("http://{addr}/mcp");
    let proxy = TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
        .await
        .expect("proxy created");

    let request = ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
        auth_token_id: Some("tok1".to_string()),
        prefer_mcp_session_affinity: false,
        pinned_api_key_id: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
    };

    let response = proxy.proxy_request(request).await.expect("proxy response");
    assert_eq!(response.status, StatusCode::OK);

    let quarantine_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_result_keeps_affinity_when_original_key_is_quarantined() {
    let db_path = temp_db_path("research-affinity-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-research-affinity-a".to_string(),
            "tvly-research-affinity-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (affinity_key_id, _other_key_id) =
        rows.into_iter()
            .fold((None, None), |mut acc, (id, secret)| {
                if secret == "tvly-research-affinity-a" {
                    acc.0 = Some(id);
                } else if secret == "tvly-research-affinity-b" {
                    acc.1 = Some(id);
                }
                acc
            });
    let affinity_key_id = affinity_key_id.expect("affinity key exists");
    let request_id = "req-affinity-quarantine";

    proxy
        .record_research_request_affinity(request_id, &affinity_key_id, "tok1")
        .await
        .expect("record research affinity");
    proxy
        .key_store
        .quarantine_key_by_id(
            &affinity_key_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine affinity key");

    let err = proxy
        .acquire_key_for_research_request(Some("tok1"), Some(request_id))
        .await
        .expect_err("result retrieval should not fall back to a different key");
    assert!(matches!(err, ProxyError::NoAvailableKeys));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn classify_quarantine_reason_ignores_generic_unauthorized_errors() {
    let unauthorized = classify_quarantine_reason(Some(401), br#"{"error":"unauthorized"}"#);
    assert!(unauthorized.is_none());

    let forbidden = classify_quarantine_reason(Some(403), br#"{"error":"forbidden"}"#);
    assert!(forbidden.is_none());

    let invalid_payload_key =
        classify_quarantine_reason(None, br#"{"error":"invalid key \"depth\""}"#);
    assert!(invalid_payload_key.is_none());
}

#[tokio::test]
async fn quarantined_keys_are_excluded_until_admin_clears_them() {
    let db_path = temp_db_path("quarantine-acquire");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-quarantine-a".to_string(),
            "tvly-quarantine-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (first_id, _first_secret) = rows
        .into_iter()
        .find(|(_, secret)| secret == "tvly-quarantine-a")
        .expect("first key exists");

    assert!(
        proxy
            .key_store
            .try_acquire_specific_key(&first_id)
            .await
            .expect("acquire specific before quarantine")
            .is_some()
    );

    proxy
        .key_store
        .quarantine_key_by_id(
            &first_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine key");

    assert!(
        proxy
            .key_store
            .try_acquire_specific_key(&first_id)
            .await
            .expect("acquire specific after quarantine")
            .is_none()
    );

    let summary = proxy.summary().await.expect("summary");
    assert_eq!(summary.active_keys, 1);
    assert_eq!(summary.quarantined_keys, 1);

    proxy
        .clear_key_quarantine_by_id(&first_id)
        .await
        .expect("clear quarantine");

    assert!(
        proxy
            .key_store
            .try_acquire_specific_key(&first_id)
            .await
            .expect("acquire specific after clear")
            .is_some()
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn quarantine_key_by_id_is_safe_under_concurrent_calls() {
    let db_path = temp_db_path("quarantine-concurrent");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-quarantine-race".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seeded key id");
    let store = proxy.key_store.clone();

    let first = {
        let store = store.clone();
        let key_id = key_id.clone();
        async move {
            store
                .quarantine_key_by_id(
                    &key_id,
                    "/api/tavily/search",
                    "account_deactivated",
                    "Tavily account deactivated (HTTP 401)",
                    "first detail",
                )
                .await
        }
    };
    let second = {
        let store = store.clone();
        let key_id = key_id.clone();
        async move {
            store
                .quarantine_key_by_id(
                    &key_id,
                    "/api/tavily/search",
                    "account_deactivated",
                    "Tavily account deactivated (HTTP 401)",
                    "second detail",
                )
                .await
        }
    };

    let (first_result, second_result) = tokio::join!(first, second);
    first_result.expect("first quarantine succeeds");
    second_result.expect("second quarantine succeeds");

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
async fn quarantine_key_by_id_preserves_original_created_at() {
    let db_path = temp_db_path("quarantine-created-at");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-quarantine-created-at".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seeded key id");

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "first detail",
        )
        .await
        .expect("first quarantine");

    let first_created_at: i64 = sqlx::query_scalar(
        "SELECT created_at FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("first created_at");

    tokio::time::sleep(Duration::from_secs(1)).await;

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "second detail",
        )
        .await
        .expect("second quarantine");

    let second_created_at: i64 = sqlx::query_scalar(
        "SELECT created_at FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("second created_at");
    assert_eq!(second_created_at, first_created_at);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn list_keys_pending_quota_sync_skips_quarantined_keys() {
    let db_path = temp_db_path("quota-sync-skip-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-quota-sync-a".to_string(),
            "tvly-quota-sync-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (quarantined_id, active_id) =
        rows.into_iter()
            .fold((None, None), |mut acc, (id, secret)| {
                if secret == "tvly-quota-sync-a" {
                    acc.0 = Some(id);
                } else if secret == "tvly-quota-sync-b" {
                    acc.1 = Some(id);
                }
                acc
            });
    let quarantined_id = quarantined_id.expect("quarantined key exists");
    let active_id = active_id.expect("active key exists");

    proxy
        .key_store
        .quarantine_key_by_id(
            &quarantined_id,
            "/api/tavily/usage",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine key");

    let pending = proxy
        .list_keys_pending_quota_sync(24 * 60 * 60)
        .await
        .expect("list pending keys");
    assert_eq!(pending, vec![active_id]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn list_keys_pending_hot_quota_sync_only_returns_recent_stale_keys() {
    let db_path = temp_db_path("quota-sync-hot-selection");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-hot-sync-a".to_string(),
            "tvly-hot-sync-b".to_string(),
            "tvly-hot-sync-c".to_string(),
            "tvly-hot-sync-d".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let now = Utc::now().timestamp();

    for (id, secret) in rows {
        match secret.as_str() {
            "tvly-hot-sync-a" => {
                sqlx::query(
                    "UPDATE api_keys SET last_used_at = ?, quota_synced_at = ?, status = ? WHERE id = ?",
                )
                .bind(now - 10 * 60)
                .bind(now - 2 * 60 * 60)
                .bind(STATUS_ACTIVE)
                .bind(&id)
                .execute(&proxy.key_store.pool)
                .await
                .expect("mark stale recent key");
            }
            "tvly-hot-sync-b" => {
                sqlx::query(
                    "UPDATE api_keys SET last_used_at = ?, quota_synced_at = ?, status = ? WHERE id = ?",
                )
                .bind(now - 10 * 60)
                .bind(now - 5 * 60)
                .bind(STATUS_ACTIVE)
                .bind(&id)
                .execute(&proxy.key_store.pool)
                .await
                .expect("mark fresh recent key");
            }
            "tvly-hot-sync-c" => {
                sqlx::query(
                    "UPDATE api_keys SET last_used_at = ?, quota_synced_at = ?, status = ? WHERE id = ?",
                )
                .bind(now - 6 * 60 * 60)
                .bind(now - 3 * 60 * 60)
                .bind(STATUS_ACTIVE)
                .bind(&id)
                .execute(&proxy.key_store.pool)
                .await
                .expect("mark cold key");
            }
            "tvly-hot-sync-d" => {
                sqlx::query(
                    "UPDATE api_keys SET last_used_at = ?, quota_synced_at = ?, status = ? WHERE id = ?",
                )
                .bind(now - 5 * 60)
                .bind(now - 3 * 60 * 60)
                .bind(STATUS_EXHAUSTED)
                .bind(&id)
                .execute(&proxy.key_store.pool)
                .await
                .expect("mark exhausted key");
            }
            _ => {}
        }
    }

    let pending = proxy
        .list_keys_pending_hot_quota_sync(2 * 60 * 60, 15 * 60)
        .await
        .expect("list hot pending keys");
    assert_eq!(pending.len(), 1);

    let api_key: String = sqlx::query_scalar("SELECT api_key FROM api_keys WHERE id = ?")
        .bind(&pending[0])
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("resolve selected key");
    assert_eq!(api_key, "tvly-hot-sync-a");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_quota_totals_exclude_quarantined_keys() {
    let db_path = temp_db_path("summary-quota-excludes-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-summary-quota-a".to_string(),
            "tvly-summary-quota-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (quarantined_id, active_id) =
        rows.into_iter()
            .fold((None, None), |mut acc, (id, secret)| {
                if secret == "tvly-summary-quota-a" {
                    acc.0 = Some(id);
                } else if secret == "tvly-summary-quota-b" {
                    acc.1 = Some(id);
                }
                acc
            });
    let quarantined_id = quarantined_id.expect("quarantined key exists");
    let active_id = active_id.expect("active key exists");

    proxy
        .key_store
        .update_quota_for_key(&quarantined_id, 100, 80, Utc::now().timestamp())
        .await
        .expect("update quarantined key quota");
    proxy
        .key_store
        .update_quota_for_key(&active_id, 50, 40, Utc::now().timestamp())
        .await
        .expect("update active key quota");
    proxy
        .key_store
        .quarantine_key_by_id(
            &quarantined_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine key");

    let summary = proxy.summary().await.expect("summary");
    assert_eq!(summary.total_quota_limit, 50);
    assert_eq!(summary.total_quota_remaining, 40);

    let _ = std::fs::remove_file(db_path);
}

async fn insert_summary_window_bucket(
    proxy: &TavilyProxy,
    key_id: &str,
    bucket_start: i64,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO api_key_usage_buckets (
            api_key_id,
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            valuable_success_count,
            valuable_failure_count,
            other_success_count,
            other_failure_count,
            unknown_count,
            updated_at
        ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?, ?, 0, 0, 0, ?)
        "#,
    )
    .bind(key_id)
    .bind(bucket_start)
    .bind(total_requests)
    .bind(success_count)
    .bind(error_count)
    .bind(quota_exhausted_count)
    .bind(success_count)
    .bind(error_count + quota_exhausted_count)
    .bind(bucket_start + 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert summary window bucket");
}

async fn insert_dashboard_summary_rollup_day_bucket(
    proxy: &TavilyProxy,
    bucket_start: i64,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
) {
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
        ) VALUES (?, 86400, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0, 0, 0, 0, ?, 0, ?)
        "#,
    )
    .bind(bucket_start)
    .bind(total_requests)
    .bind(success_count)
    .bind(error_count)
    .bind(quota_exhausted_count)
    .bind(success_count)
    .bind(error_count + quota_exhausted_count)
    .bind(total_requests)
    .bind(bucket_start + 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert dashboard summary rollup day bucket");
}

async fn insert_summary_window_logs(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    outcome: &str,
    count: usize,
) {
    insert_summary_window_logs_with_visibility(
        proxy,
        key_id,
        created_at,
        outcome,
        count,
        REQUEST_LOG_VISIBILITY_VISIBLE,
    )
    .await;
}

async fn insert_summary_window_logs_with_visibility(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    outcome: &str,
    count: usize,
    visibility: &str,
) {
    for offset in 0..count {
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
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
            ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, ?, NULL, NULL, '[]', '[]', ?, ?)
            "#,
        )
        .bind(key_id)
        .bind(outcome)
        .bind(visibility)
        .bind(created_at + offset as i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert summary window log");
    }

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(
            Some(created_at),
            Some(created_at + count as i64),
        )
        .await
        .expect("rebuild dashboard rollup after summary log seed");
}

#[derive(Clone)]
struct DashboardHourlyLogSeed<'a> {
    created_at: i64,
    path: &'a str,
    request_kind_key: &'a str,
    request_kind_label: &'a str,
    result_status: &'a str,
    failure_kind: Option<&'a str>,
    request_body: Option<&'a [u8]>,
    visibility: &'a str,
}

async fn insert_dashboard_hourly_log(
    proxy: &TavilyProxy,
    key_id: &str,
    seed: DashboardHourlyLogSeed<'_>,
) {
    let status_code = match seed.result_status {
        OUTCOME_SUCCESS => 200,
        OUTCOME_QUOTA_EXHAUSTED => 429,
        _ => 500,
    };
    let tavily_status_code = if seed.failure_kind == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        || seed.result_status == OUTCOME_QUOTA_EXHAUSTED
    {
        Some(429)
    } else if seed.result_status == OUTCOME_SUCCESS {
        Some(200)
    } else {
        Some(500)
    };

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
            failure_kind,
            created_at
        ) VALUES (?, NULL, 'POST', ?, NULL, ?, ?, NULL, ?, ?, ?, ?, NULL, '[]', '[]', ?, ?, ?)
        "#,
    )
    .bind(key_id)
    .bind(seed.path)
    .bind(status_code)
    .bind(tavily_status_code)
    .bind(seed.result_status)
    .bind(seed.request_kind_key)
    .bind(seed.request_kind_label)
    .bind(seed.request_body)
    .bind(seed.visibility)
    .bind(seed.failure_kind)
    .bind(seed.created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert dashboard hourly log");

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(
            Some(seed.created_at),
            Some(seed.created_at + 1),
        )
        .await
        .expect("rebuild dashboard rollup after hourly log seed");
}

async fn insert_summary_window_charged_logs(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    credits: i64,
    count: usize,
) {
    for offset in 0..count {
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
                business_credits,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
            ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, ?, ?, NULL, NULL, '[]', '[]', ?, ?)
            "#,
        )
        .bind(key_id)
        .bind(OUTCOME_SUCCESS)
        .bind(credits)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .bind(created_at + offset as i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert summary window charged log");
    }

    proxy
        .key_store
        .rebuild_dashboard_request_rollup_buckets_window(
            Some(created_at),
            Some(created_at + count as i64),
        )
        .await
        .expect("rebuild dashboard rollup after charged log seed");
}

async fn insert_summary_window_maintenance_record(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    source: &str,
    operation_code: &str,
    reason_code: Option<&str>,
) {
    let reason_summary = reason_code.map(|code| format!("{code} summary"));
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
        ) VALUES (?, ?, ?, ?, ?, ?, ?, NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?)
        "#,
    )
    .bind(format!("summary-window-maint-{}", nanoid!(8)))
    .bind(key_id)
    .bind(source)
    .bind(operation_code)
    .bind(format!("{operation_code} summary"))
    .bind(reason_code)
    .bind(reason_summary)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert summary window maintenance record");
}

async fn insert_auth_token_metric_log(
    proxy: &TavilyProxy,
    token_id: &str,
    created_at: i64,
    result_status: &str,
) {
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
            counts_business_quota
        ) VALUES (?, 'POST', '/api/tavily/search', NULL, 200, 200, ?, NULL, ?, 1)
        "#,
    )
    .bind(token_id)
    .bind(result_status)
    .bind(created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert auth token metric log");
}

#[tokio::test]
async fn summary_windows_split_today_yesterday_and_month() {
    let db_path = temp_db_path("summary-windows-split");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-a".to_string()],
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

    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_SUCCESS, 9).await;
    insert_summary_window_logs(&proxy, &key_id, today_start + 3600, OUTCOME_ERROR, 2).await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        today_start + 7200,
        OUTCOME_QUOTA_EXHAUSTED,
        1,
    )
    .await;
    insert_summary_window_logs(&proxy, &key_id, yesterday_start + 60, OUTCOME_SUCCESS, 5).await;
    insert_summary_window_logs(&proxy, &key_id, yesterday_start + 3600, OUTCOME_ERROR, 1).await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        yesterday_start + 7200,
        OUTCOME_QUOTA_EXHAUSTED,
        1,
    )
    .await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        yesterday_same_time + 60,
        OUTCOME_SUCCESS,
        3,
    )
    .await;

    let mut expected_month = SummaryWindowMetrics {
        total_requests: 12,
        success_count: 9,
        error_count: 2,
        quota_exhausted_count: 1,
        valuable_success_count: 9,
        valuable_failure_count: 3,
        upstream_exhausted_key_count: 0,
        new_keys: 1,
        new_quarantines: 0,
        ..SummaryWindowMetrics::default()
    };
    expected_month.quota_charge.stale_key_count = 1;
    if yesterday_start >= local_month_start {
        expected_month.total_requests += 7;
        expected_month.success_count += 5;
        expected_month.error_count += 1;
        expected_month.quota_exhausted_count += 1;
        expected_month.valuable_success_count += 5;
        expected_month.valuable_failure_count += 2;
    }
    if yesterday_same_time + 60 >= local_month_start {
        expected_month.total_requests += 3;
        expected_month.success_count += 3;
        expected_month.valuable_success_count += 3;
    }
    if local_month_start < today_start {
        expected_month.total_requests += 3;
        expected_month.success_count += 2;
        expected_month.error_count += 1;
        expected_month.valuable_success_count += 2;
        expected_month.valuable_failure_count += 1;
        insert_dashboard_summary_rollup_day_bucket(&proxy, local_month_start, 3, 2, 1, 0).await;
    }

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");

    assert_eq!(
        summary.today,
        SummaryWindowMetrics {
            total_requests: 12,
            success_count: 9,
            error_count: 2,
            quota_exhausted_count: 1,
            valuable_success_count: 9,
            valuable_failure_count: 3,
            quota_charge: SummaryQuotaCharge {
                stale_key_count: 1,
                ..SummaryQuotaCharge::default()
            },
            ..SummaryWindowMetrics::default()
        }
    );
    assert_eq!(
        summary.yesterday,
        SummaryWindowMetrics {
            total_requests: 7,
            success_count: 5,
            error_count: 1,
            quota_exhausted_count: 1,
            valuable_success_count: 5,
            valuable_failure_count: 2,
            quota_charge: SummaryQuotaCharge {
                stale_key_count: 1,
                ..SummaryQuotaCharge::default()
            },
            ..SummaryWindowMetrics::default()
        }
    );
    assert_eq!(summary.month, expected_month);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_windows_month_counts_follow_server_timezone_boundary() {
    let db_path = temp_db_path("summary-windows-local-month");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-utc".to_string()],
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

    let utc_now = Utc::now();
    let utc_month_start = start_of_month(utc_now).timestamp();
    let local_month_start = start_of_local_month_utc_ts(utc_now.with_timezone(&Local));
    if utc_month_start == local_month_start {
        return;
    }

    let boundary_ts = utc_month_start.max(local_month_start);
    let local_month_only_ts = boundary_ts - 120;
    let utc_month_only_ts = boundary_ts + 120;

    insert_summary_window_logs(&proxy, &key_id, local_month_only_ts, OUTCOME_SUCCESS, 9).await;
    insert_summary_window_logs(&proxy, &key_id, local_month_only_ts + 30, OUTCOME_ERROR, 2).await;
    insert_summary_window_logs(&proxy, &key_id, utc_month_only_ts, OUTCOME_SUCCESS, 5).await;
    insert_summary_window_logs(&proxy, &key_id, utc_month_only_ts + 30, OUTCOME_ERROR, 2).await;

    let summary = proxy
        .summary_windows_at(utc_now.with_timezone(&Local))
        .await
        .expect("summary windows");

    let expected_total = if local_month_start < utc_month_start {
        18
    } else {
        7
    };
    let expected_success = if local_month_start < utc_month_start {
        14
    } else {
        5
    };
    let expected_error = if local_month_start < utc_month_start {
        4
    } else {
        2
    };

    assert_eq!(summary.month.total_requests, expected_total);
    assert_eq!(summary.month.success_count, expected_success);
    assert_eq!(summary.month.error_count, expected_error);
}

#[tokio::test]
async fn user_dashboard_summary_daily_metrics_follow_explicit_window() {
    let db_path = temp_db_path("user-dashboard-explicit-window");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "browser-window-user".to_string(),
            username: Some("browser_window_user".to_string()),
            name: Some("Browser Window User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:browser-window-user"))
        .await
        .expect("bind token");

    let now_ts = Utc::now().timestamp();
    let explicit_window = TimeRangeUtc {
        start: now_ts - 3_600,
        end: now_ts + 3_600,
    };
    insert_auth_token_metric_log(&proxy, &token.id, now_ts - 600, OUTCOME_SUCCESS).await;
    insert_auth_token_metric_log(&proxy, &token.id, now_ts - 300, OUTCOME_ERROR).await;
    insert_auth_token_metric_log(&proxy, &token.id, now_ts - 86_400, OUTCOME_SUCCESS).await;

    let summary = proxy
        .user_dashboard_summary(&user.user_id, Some(explicit_window))
        .await
        .expect("user dashboard summary");

    assert_eq!(summary.daily_success, 1);
    assert_eq!(summary.daily_failure, 1);
    assert_eq!(summary.monthly_success, 2);
}

#[tokio::test]
async fn user_dashboard_summary_daily_quota_includes_legacy_hour_buckets() {
    let db_path = temp_db_path("user-dashboard-legacy-hour-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-hour-user".to_string(),
            username: Some("legacy_hour_user".to_string()),
            name: Some("Legacy Hour User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy-hour-user"))
        .await
        .expect("bind token");

    let day_window = server_local_day_window_utc(Local::now());
    sqlx::query(
        r#"
        INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
        VALUES (?, ?, ?, ?), (?, ?, ?, ?)
        "#,
    )
    .bind(&user.user_id)
    .bind(day_window.start)
    .bind(GRANULARITY_DAY)
    .bind(4_i64)
    .bind(&user.user_id)
    .bind(day_window.start)
    .bind(GRANULARITY_HOUR)
    .bind(6_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account usage buckets");

    let summary = proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("user dashboard summary");

    assert_eq!(summary.quota_daily_used, 10);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn token_quota_snapshot_daily_usage_uses_server_local_day_boundary() {
    let db_path = temp_db_path("token-quota-local-day");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("local-day-quota"))
        .await
        .expect("create token");

    let now_local = Local::now();
    let today_start = start_of_local_day_utc_ts(now_local);
    let yesterday_start = previous_local_day_start_utc_ts(now_local);

    sqlx::query(
        r#"
        INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
        VALUES (?, ?, ?, ?), (?, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind(yesterday_start)
    .bind(GRANULARITY_DAY)
    .bind(9_i64)
    .bind(&token.id)
    .bind(today_start)
    .bind(GRANULARITY_DAY)
    .bind(4_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token daily quota buckets");

    let verdict = proxy
        .token_quota_snapshot(&token.id)
        .await
        .expect("token quota snapshot")
        .expect("quota verdict");

    assert_eq!(verdict.daily_used, 4);
}

#[tokio::test]
async fn token_quota_snapshot_daily_usage_includes_legacy_hour_buckets_during_cutover() {
    let db_path = temp_db_path("token-quota-legacy-hour-cutover");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("legacy-hour-cutover"))
        .await
        .expect("create token");

    let now_local = Local::now();
    let today_start = start_of_local_day_utc_ts(now_local);
    let next_day_start = next_local_day_start_utc_ts(today_start);
    let legacy_hour_bucket = today_start + SECS_PER_HOUR;

    sqlx::query(
        r#"
        INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
        VALUES (?, ?, ?, ?), (?, ?, ?, ?), (?, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind(today_start)
    .bind(GRANULARITY_DAY)
    .bind(4_i64)
    .bind(&token.id)
    .bind(legacy_hour_bucket)
    .bind(GRANULARITY_HOUR)
    .bind(6_i64)
    .bind(&token.id)
    .bind(next_day_start)
    .bind(GRANULARITY_HOUR)
    .bind(99_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed cutover quota buckets");

    let verdict = proxy
        .token_quota_snapshot(&token.id)
        .await
        .expect("token quota snapshot")
        .expect("quota verdict");

    assert_eq!(verdict.daily_used, 10);
}

#[tokio::test]
async fn summary_windows_count_distinct_upstream_exhausted_keys() {
    let db_path = temp_db_path("summary-windows-upstream-exhausted-keys");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-upstream".to_string()],
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

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at)
        VALUES (?, ?, 'active', ?)
        "#,
    )
    .bind("summary-window-extra-key")
    .bind("tvly-summary-window-extra-key")
    .bind(Utc::now().timestamp())
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert extra key");

    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at)
        VALUES (?, ?, 'active', ?)
        "#,
    )
    .bind("summary-window-month-key")
    .bind("tvly-summary-window-month-key")
    .bind(Utc::now().timestamp())
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert month key");

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
    let month_start = start_of_month(now.with_timezone(&Utc)).timestamp();

    insert_summary_window_logs(
        &proxy,
        &key_id,
        today_start + 60,
        OUTCOME_QUOTA_EXHAUSTED,
        4,
    )
    .await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        yesterday_start + 60,
        OUTCOME_QUOTA_EXHAUSTED,
        2,
    )
    .await;
    insert_summary_window_bucket(&proxy, &key_id, today_start, 4, 0, 0, 4).await;
    insert_summary_window_bucket(&proxy, &key_id, yesterday_start, 2, 0, 0, 2).await;

    insert_summary_window_maintenance_record(
        &proxy,
        &key_id,
        today_start + 60,
        MAINTENANCE_SOURCE_SYSTEM,
        MAINTENANCE_OP_AUTO_MARK_EXHAUSTED,
        Some(OUTCOME_QUOTA_EXHAUSTED),
    )
    .await;
    insert_summary_window_maintenance_record(
        &proxy,
        &key_id,
        today_start + 120,
        MAINTENANCE_SOURCE_SYSTEM,
        MAINTENANCE_OP_AUTO_MARK_EXHAUSTED,
        Some(OUTCOME_QUOTA_EXHAUSTED),
    )
    .await;
    insert_summary_window_maintenance_record(
        &proxy,
        &key_id,
        today_start + 180,
        MAINTENANCE_SOURCE_ADMIN,
        MAINTENANCE_OP_MANUAL_MARK_EXHAUSTED,
        Some("manual_mark_exhausted"),
    )
    .await;
    insert_summary_window_maintenance_record(
        &proxy,
        &key_id,
        yesterday_start + 60,
        MAINTENANCE_SOURCE_SYSTEM,
        MAINTENANCE_OP_AUTO_MARK_EXHAUSTED,
        Some(OUTCOME_QUOTA_EXHAUSTED),
    )
    .await;
    insert_summary_window_maintenance_record(
        &proxy,
        "summary-window-extra-key",
        yesterday_start + 120,
        MAINTENANCE_SOURCE_SYSTEM,
        MAINTENANCE_OP_AUTO_MARK_EXHAUSTED,
        Some(OUTCOME_QUOTA_EXHAUSTED),
    )
    .await;
    insert_summary_window_maintenance_record(
        &proxy,
        "summary-window-extra-key",
        yesterday_same_time + 120,
        MAINTENANCE_SOURCE_ADMIN,
        MAINTENANCE_OP_MANUAL_MARK_EXHAUSTED,
        Some("manual_mark_exhausted"),
    )
    .await;

    let mut expected_month_upstream_exhausted = if month_start <= yesterday_start { 2 } else { 1 };
    if month_start < yesterday_start {
        insert_summary_window_maintenance_record(
            &proxy,
            "summary-window-month-key",
            month_start + 60,
            MAINTENANCE_SOURCE_SYSTEM,
            MAINTENANCE_OP_AUTO_MARK_EXHAUSTED,
            Some(OUTCOME_QUOTA_EXHAUSTED),
        )
        .await;
        expected_month_upstream_exhausted += 1;
    }

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");

    assert_eq!(summary.today.quota_exhausted_count, 4);
    assert_eq!(summary.today.upstream_exhausted_key_count, 1);
    assert_eq!(summary.yesterday.quota_exhausted_count, 2);
    assert_eq!(summary.yesterday.upstream_exhausted_key_count, 2);
    assert_eq!(
        summary.month.upstream_exhausted_key_count,
        expected_month_upstream_exhausted
    );

    let _ = std::fs::remove_file(db_path);
}


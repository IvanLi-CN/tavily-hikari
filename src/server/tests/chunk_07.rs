    #[tokio::test]
    async fn admin_dashboard_sse_snapshot_refreshes_when_disabled_token_feed_breaks() {
        let db_path = temp_db_path("admin-dashboard-snapshot-disabled-token-feed-error");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-dashboard-disabled-token-feed-error".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let admin_password = "admin-dashboard-disabled-token-feed-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build client");

        let login_resp = client
            .post(format!("http://{}/api/admin/login", admin_addr))
            .json(&serde_json::json!({ "password": admin_password }))
            .send()
            .await
            .expect("admin login");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
            .expect("admin session cookie");

        let mut events_resp = client
            .get(format!("http://{}/api/events", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("admin events request");
        assert_eq!(events_resp.status(), reqwest::StatusCode::OK);

        let mut first_text = String::new();
        while !first_text.contains("\n\n") {
            let chunk = events_resp
                .chunk()
                .await
                .expect("read initial event chunk")
                .expect("initial snapshot chunk exists");
            first_text.push_str(std::str::from_utf8(&chunk).expect("initial snapshot chunk utf8"));
        }

        let initial_snapshot = first_text
            .split("\n\n")
            .find(|chunk| chunk.contains("event: snapshot"))
            .expect("initial snapshot event");
        let initial_data = initial_snapshot
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("initial snapshot data");
        let initial_json: serde_json::Value =
            serde_json::from_str(initial_data).expect("initial snapshot payload json");
        assert_eq!(
            initial_json
                .get("tokenCoverage")
                .and_then(|value| value.as_str()),
            Some("ok")
        );

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");

        sqlx::query("DROP TABLE auth_tokens")
            .execute(&pool)
            .await
            .expect("drop auth_tokens");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        let mut buffer = String::new();
        let mut refreshed_snapshot: Option<serde_json::Value> = None;
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let chunk = tokio::time::timeout(remaining, events_resp.chunk())
                .await
                .expect("await refreshed event chunk in time")
                .expect("read refreshed event chunk")
                .expect("refreshed event chunk exists");
            buffer.push_str(std::str::from_utf8(&chunk).expect("refreshed event chunk utf8"));
            while let Some((event_chunk, rest)) = buffer.split_once("\n\n") {
                let event_chunk = event_chunk.to_string();
                buffer = rest.to_string();
                if !event_chunk.contains("event: snapshot") {
                    continue;
                }
                let Some(data) = event_chunk
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                else {
                    continue;
                };
                let payload: serde_json::Value =
                    serde_json::from_str(data).expect("refreshed snapshot payload json");
                if payload
                    .get("tokenCoverage")
                    .and_then(|value| value.as_str())
                    == Some("error")
                {
                    refreshed_snapshot = Some(payload);
                    break;
                }
            }
            if refreshed_snapshot.is_some() {
                break;
            }
        }

        let refreshed_snapshot = refreshed_snapshot.expect("token coverage snapshot refresh");
        assert_eq!(
            refreshed_snapshot
                .get("tokenCoverage")
                .and_then(|value| value.as_str()),
            Some("error")
        );
        assert_eq!(
            refreshed_snapshot
                .get("disabledTokens")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );

        drop(events_resp);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_user_management_lists_details_and_updates_quota() {
        let db_path = temp_db_path("admin-users");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-alice".to_string(),
                username: Some("alice".to_string()),
                name: Some("Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");
        let bob = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-bob".to_string(),
                username: Some("bob".to_string()),
                name: Some("Bob".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert bob");
        let _charlie = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-users-charlie".to_string(),
                username: Some("charlie".to_string()),
                name: Some("Charlie".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(0),
                raw_payload_json: None,
            })
            .await
            .expect("upsert charlie");

        let alice_token = proxy
            .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
            .await
            .expect("bind alice token");
        let _bob_token = proxy
            .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob"))
            .await
            .expect("bind bob token");
        let vip_tag = proxy
            .create_user_tag(
                "vip_plus",
                "VIP+",
                Some("star"),
                "quota_delta",
                5,
                10,
                20,
                30,
            )
            .await
            .expect("create vip tag");
        proxy
            .bind_user_tag_to_user(&alice.user_id, &vip_tag.id)
            .await
            .expect("bind vip tag");

        let _ = proxy
            .check_token_hourly_requests(&alice_token.id)
            .await
            .expect("seed hourly-any");
        let _ = proxy
            .check_token_quota(&alice_token.id)
            .await
            .expect("seed business quota");
        proxy
            .record_token_attempt(
                &alice_token.id,
                &Method::POST,
                "/mcp",
                None,
                Some(200),
                Some(0),
                true,
                "success",
                None,
            )
            .await
            .expect("record success");
        proxy
            .record_token_attempt(
                &alice_token.id,
                &Method::POST,
                "/mcp",
                None,
                Some(500),
                Some(-32001),
                true,
                "error",
                Some("upstream error"),
            )
            .await
            .expect("record error");
        for index in 0..4 {
            let api_key_id = proxy
                .add_or_undelete_key(&format!("tvly-admin-users-associated-key-{index}"))
                .await
                .expect("create associated api key");
            let pending_binding_log_id = proxy
                .record_pending_billing_attempt(
                    &alice_token.id,
                    &Method::POST,
                    "/api/tavily/search",
                    None,
                    Some(200),
                    Some(200),
                    true,
                    "success",
                    None,
                    1,
                    Some(&api_key_id),
                )
                .await
                .expect("record pending associated key binding");
            proxy
                .settle_pending_billing_attempt(pending_binding_log_id)
                .await
                .expect("settle associated key binding");
        }

        let addr = spawn_admin_users_server(proxy, true).await;
        let client = Client::new();

        let list_url = format!("http://{}/api/users?page=1&per_page=20", addr);
        let list_resp = client
            .get(&list_url)
            .send()
            .await
            .expect("list users request");
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);
        let list_body: serde_json::Value = list_resp.json().await.expect("list users json");
        let items = list_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("items is array");
        let default_ordered_user_ids: Vec<String> = items
            .iter()
            .filter_map(|item| {
                item.get("userId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .collect();
        let alice_item = items
            .iter()
            .find(|item| {
                item.get("userId")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == alice.user_id)
            })
            .expect("alice row exists");
        assert_eq!(
            alice_item
                .get("tokenCount")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            alice_item
                .get("apiKeyCount")
                .and_then(|value| value.as_i64()),
            Some(4)
        );
        assert!(
            alice_item
                .get("hourlyAnyUsed")
                .and_then(|value| value.as_i64())
                .unwrap_or_default()
                >= 1
        );
        assert!(
            alice_item
                .get("quotaHourlyUsed")
                .and_then(|value| value.as_i64())
                .unwrap_or_default()
                >= 1
        );
        let list_tags = alice_item
            .get("tags")
            .and_then(|value| value.as_array())
            .expect("list tags array");
        assert!(
            list_tags.iter().any(|value| {
                value.get("displayName").and_then(|it| it.as_str()) == Some("VIP+")
            })
        );
        assert!(list_tags.iter().any(|value| {
            value.get("systemKey").and_then(|it| it.as_str()) == Some("linuxdo_l2")
        }));

        let tag_search_url = format!(
            "http://{}/api/users?page=1&per_page=20&q={}",
            addr,
            urlencoding::encode("VIP+")
        );
        let tag_search_resp = client
            .get(&tag_search_url)
            .send()
            .await
            .expect("tag search request");
        assert_eq!(tag_search_resp.status(), reqwest::StatusCode::OK);
        let tag_search_body: serde_json::Value =
            tag_search_resp.json().await.expect("tag search json");
        let tag_search_items = tag_search_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("tag search items array");
        assert!(tag_search_items.iter().any(|item| {
            item.get("userId")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value == alice.user_id)
        }));

        let order_only_url = format!("http://{}/api/users?page=1&per_page=20&order=asc", addr);
        let order_only_resp = client
            .get(&order_only_url)
            .send()
            .await
            .expect("order-only list request");
        assert_eq!(order_only_resp.status(), reqwest::StatusCode::OK);
        let order_only_body: serde_json::Value =
            order_only_resp.json().await.expect("order-only list json");
        let order_only_user_ids: Vec<String> = order_only_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("order-only items array")
            .iter()
            .filter_map(|item| {
                item.get("userId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .collect();
        assert_eq!(order_only_user_ids, default_ordered_user_ids);

        let detail_url = format!("http://{}/api/users/{}", addr, alice.user_id);
        let detail_resp = client
            .get(&detail_url)
            .send()
            .await
            .expect("user detail request");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("user detail json");
        let before_hourly_any_used = detail_body
            .get("hourlyAnyUsed")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        assert_eq!(
            detail_body
                .get("apiKeyCount")
                .and_then(|value| value.as_i64()),
            Some(4)
        );
        let tokens = detail_body
            .get("tokens")
            .and_then(|value| value.as_array())
            .expect("tokens is array");
        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens
                .first()
                .and_then(|value| value.get("tokenId"))
                .and_then(|value| value.as_str()),
            Some(alice_token.id.as_str())
        );
        let detail_tags = detail_body
            .get("tags")
            .and_then(|value| value.as_array())
            .expect("detail tags array");
        assert_eq!(detail_tags.len(), 2);
        let system_tag = detail_tags
            .iter()
            .find(|value| value.get("systemKey").and_then(|it| it.as_str()) == Some("linuxdo_l2"))
            .expect("linuxdo system tag in detail");
        let system_hourly_any_delta = system_tag
            .get("hourlyAnyDelta")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let system_hourly_delta = system_tag
            .get("hourlyDelta")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let system_daily_delta = system_tag
            .get("dailyDelta")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let system_monthly_delta = system_tag
            .get("monthlyDelta")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        let quota_base = detail_body.get("quotaBase").expect("quotaBase present");
        let effective_quota = detail_body
            .get("effectiveQuota")
            .expect("effectiveQuota present");
        let quota_base_hourly_any_before = quota_base
            .get("hourlyAnyLimit")
            .and_then(|value| value.as_i64())
            .expect("base hourlyAny limit before patch");
        let effective_hourly_any_before = effective_quota
            .get("hourlyAnyLimit")
            .and_then(|value| value.as_i64())
            .expect("effective hourlyAny limit before patch");
        assert_eq!(
            effective_quota
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64()),
            quota_base
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64())
                .map(|value| value + system_hourly_any_delta + 5)
        );
        assert_eq!(
            effective_quota
                .get("hourlyLimit")
                .and_then(|value| value.as_i64()),
            quota_base
                .get("hourlyLimit")
                .and_then(|value| value.as_i64())
                .map(|value| value + system_hourly_delta + 10)
        );
        assert!(
            detail_body
                .get("quotaBreakdown")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().any(|entry| {
                    entry.get("tagId").and_then(|value| value.as_str()) == Some(vip_tag.id.as_str())
                }))
        );

        let patch_url = format!("http://{}/api/users/{}/quota", addr, alice.user_id);
        let patch_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyAnyLimit": 123,
                "hourlyLimit": 45,
                "dailyLimit": 678,
                "monthlyLimit": 910,
            }))
            .send()
            .await
            .expect("patch user quota request");
        assert_eq!(patch_resp.status(), reqwest::StatusCode::NO_CONTENT);

        let detail_after_resp = client
            .get(&detail_url)
            .send()
            .await
            .expect("user detail after patch request");
        assert_eq!(detail_after_resp.status(), reqwest::StatusCode::OK);
        let detail_after: serde_json::Value = detail_after_resp
            .json()
            .await
            .expect("user detail after patch json");
        assert_eq!(
            detail_after
                .get("requestRate")
                .and_then(|value| value.get("limit"))
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit())
        );
        assert_eq!(
            detail_after
                .get("requestRate")
                .and_then(|value| value.get("windowMinutes"))
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit_window_minutes())
        );
        assert_eq!(
            detail_after
                .get("requestRate")
                .and_then(|value| value.get("scope"))
                .and_then(|value| value.as_str()),
            Some("user")
        );
        assert_eq!(
            detail_after
                .get("quotaBase")
                .and_then(|value| value.get("hourlyAnyLimit"))
                .and_then(|value| value.as_i64()),
            Some(quota_base_hourly_any_before)
        );
        assert_eq!(
            detail_after
                .get("quotaBase")
                .and_then(|value| value.get("hourlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(45)
        );
        assert_eq!(
            detail_after
                .get("quotaBase")
                .and_then(|value| value.get("dailyLimit"))
                .and_then(|value| value.as_i64()),
            Some(678)
        );
        assert_eq!(
            detail_after
                .get("quotaBase")
                .and_then(|value| value.get("monthlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(910)
        );
        assert_eq!(
            detail_after
                .get("effectiveQuota")
                .and_then(|value| value.get("hourlyAnyLimit"))
                .and_then(|value| value.as_i64()),
            Some(effective_hourly_any_before)
        );
        assert_eq!(
            detail_after
                .get("effectiveQuota")
                .and_then(|value| value.get("hourlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(45 + system_hourly_delta + 10)
        );
        assert_eq!(
            detail_after
                .get("effectiveQuota")
                .and_then(|value| value.get("dailyLimit"))
                .and_then(|value| value.as_i64()),
            Some(678 + system_daily_delta + 20)
        );
        assert_eq!(
            detail_after
                .get("effectiveQuota")
                .and_then(|value| value.get("monthlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(910 + system_monthly_delta + 30)
        );
        assert_eq!(
            detail_after
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit())
        );
        assert_eq!(
            detail_after
                .get("quotaHourlyLimit")
                .and_then(|value| value.as_i64()),
            Some(45 + system_hourly_delta + 10)
        );
        assert_eq!(
            detail_after
                .get("quotaDailyLimit")
                .and_then(|value| value.as_i64()),
            Some(678 + system_daily_delta + 20)
        );
        assert_eq!(
            detail_after
                .get("quotaMonthlyLimit")
                .and_then(|value| value.as_i64()),
            Some(910 + system_monthly_delta + 30)
        );
        assert_eq!(
            detail_after
                .get("hourlyAnyUsed")
                .and_then(|value| value.as_i64()),
            Some(before_hourly_any_used)
        );

        let invalid_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyAnyLimit": -1,
                "hourlyLimit": 45,
                "dailyLimit": 678,
                "monthlyLimit": 910,
            }))
            .send()
            .await
            .expect("legacy hourlyAny patch request");
        assert_eq!(
            invalid_resp.status(),
            reqwest::StatusCode::NO_CONTENT,
            "legacy hourlyAnyLimit should be ignored instead of rejected"
        );

        let invalid_business_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyAnyLimit": 999,
                "hourlyLimit": -1,
                "dailyLimit": 678,
                "monthlyLimit": 910,
            }))
            .send()
            .await
            .expect("invalid business patch request");
        assert_eq!(invalid_business_resp.status(), reqwest::StatusCode::BAD_REQUEST);

        let omitted_legacy_resp = client
            .patch(&patch_url)
            .json(&serde_json::json!({
                "hourlyLimit": 46,
                "dailyLimit": 679,
                "monthlyLimit": 911,
            }))
            .send()
            .await
            .expect("omitted legacy hourlyAny patch request");
        assert_eq!(
            omitted_legacy_resp.status(),
            reqwest::StatusCode::NO_CONTENT,
            "missing hourlyAnyLimit should be accepted and ignored"
        );

        let detail_omitted_resp = client
            .get(&detail_url)
            .send()
            .await
            .expect("user detail after omitted legacy patch request");
        assert_eq!(detail_omitted_resp.status(), reqwest::StatusCode::OK);
        let detail_omitted: serde_json::Value = detail_omitted_resp
            .json()
            .await
            .expect("user detail after omitted legacy patch json");
        assert_eq!(
            detail_omitted
                .get("quotaBase")
                .and_then(|value| value.get("hourlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(46)
        );
        assert_eq!(
            detail_omitted
                .get("quotaBase")
                .and_then(|value| value.get("dailyLimit"))
                .and_then(|value| value.as_i64()),
            Some(679)
        );
        assert_eq!(
            detail_omitted
                .get("quotaBase")
                .and_then(|value| value.get("monthlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(911)
        );
        assert_eq!(
            detail_omitted
                .get("quotaBase")
                .and_then(|value| value.get("hourlyAnyLimit"))
                .and_then(|value| value.as_i64()),
            Some(quota_base_hourly_any_before)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_user_tag_crud_and_system_guards_work() {
        let db_path = temp_db_path("admin-user-tags");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-user-tags-user".to_string(),
                username: Some("taguser".to_string()),
                name: Some("Tag User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(4),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:taguser"))
            .await
            .expect("bind token");

        let addr = spawn_admin_users_server(proxy, true).await;
        let client = Client::new();

        let list_tags_resp = client
            .get(format!("http://{}/api/user-tags", addr))
            .send()
            .await
            .expect("list user tags request");
        assert_eq!(list_tags_resp.status(), reqwest::StatusCode::OK);
        let list_tags_body: serde_json::Value =
            list_tags_resp.json().await.expect("list user tags json");
        let items = list_tags_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("user tags items array");
        assert_eq!(items.len(), 5);
        let system_tag = items
            .iter()
            .find(|item| {
                item.get("systemKey").and_then(|value| value.as_str()) == Some("linuxdo_l4")
            })
            .expect("linuxdo_l4 system tag present");
        assert!(
            system_tag
                .get("hourlyAnyDelta")
                .and_then(|value| value.as_i64())
                .is_some_and(|value| value > 0)
        );
        assert!(
            system_tag
                .get("hourlyDelta")
                .and_then(|value| value.as_i64())
                .is_some_and(|value| value > 0)
        );
        assert!(
            system_tag
                .get("dailyDelta")
                .and_then(|value| value.as_i64())
                .is_some_and(|value| value > 0)
        );
        assert!(
            system_tag
                .get("monthlyDelta")
                .and_then(|value| value.as_i64())
                .is_some_and(|value| value > 0)
        );
        let system_tag_id = system_tag
            .get("id")
            .and_then(|value| value.as_str())
            .expect("system tag id")
            .to_string();

        let update_system_effect_resp = client
            .patch(format!("http://{}/api/user-tags/{}", addr, system_tag_id))
            .json(&serde_json::json!({
                "name": "linuxdo_l4",
                "displayName": "L4",
                "icon": "linuxdo",
                "effectKind": "quota_delta",
                "hourlyAnyDelta": 0,
                "hourlyDelta": 0,
                "dailyDelta": 0,
                "monthlyDelta": 0,
            }))
            .send()
            .await
            .expect("update system tag effect request");
        assert_eq!(update_system_effect_resp.status(), reqwest::StatusCode::OK);

        let update_system_name_resp = client
            .patch(format!("http://{}/api/user-tags/{}", addr, system_tag_id))
            .json(&serde_json::json!({
                "name": "linuxdo_l4",
                "displayName": "L4 blocked",
                "icon": "linuxdo",
                "effectKind": "quota_delta",
                "hourlyAnyDelta": 0,
                "hourlyDelta": 0,
                "dailyDelta": 0,
                "monthlyDelta": 0,
            }))
            .send()
            .await
            .expect("update system tag display name request");
        assert_eq!(
            update_system_name_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        let bind_system_resp = client
            .post(format!("http://{}/api/users/{}/tags", addr, user.user_id))
            .json(&serde_json::json!({ "tagId": system_tag_id }))
            .send()
            .await
            .expect("bind system tag request");
        assert_eq!(bind_system_resp.status(), reqwest::StatusCode::BAD_REQUEST);

        let create_custom_resp = client
            .post(format!("http://{}/api/user-tags", addr))
            .json(&serde_json::json!({
                "name": "suspended_manual",
                "displayName": "Suspended",
                "icon": "ban",
                "effectKind": "quota_delta",
                "hourlyAnyDelta": -9,
                "hourlyDelta": -9,
                "dailyDelta": -9,
                "monthlyDelta": -9,
            }))
            .send()
            .await
            .expect("create custom tag request");
        assert_eq!(create_custom_resp.status(), reqwest::StatusCode::OK);
        let custom_tag: serde_json::Value =
            create_custom_resp.json().await.expect("custom tag json");
        let custom_tag_id = custom_tag
            .get("id")
            .and_then(|value| value.as_str())
            .expect("custom tag id")
            .to_string();

        let update_custom_resp = client
            .patch(format!("http://{}/api/user-tags/{}", addr, custom_tag_id))
            .json(&serde_json::json!({
                "name": "suspended_manual",
                "displayName": "Suspended Now",
                "icon": "ban",
                "effectKind": "block_all",
                "hourlyAnyDelta": 0,
                "hourlyDelta": 0,
                "dailyDelta": 0,
                "monthlyDelta": 0,
            }))
            .send()
            .await
            .expect("update custom tag request");
        assert_eq!(update_custom_resp.status(), reqwest::StatusCode::OK);

        let bind_custom_resp = client
            .post(format!("http://{}/api/users/{}/tags", addr, user.user_id))
            .json(&serde_json::json!({ "tagId": custom_tag_id }))
            .send()
            .await
            .expect("bind custom tag request");
        assert_eq!(bind_custom_resp.status(), reqwest::StatusCode::NO_CONTENT);

        let filtered_users_resp = client
            .get(format!(
                "http://{}/api/users?page=1&per_page=20&q=Suspended%20Now&tagId={}",
                addr, custom_tag_id
            ))
            .send()
            .await
            .expect("filtered users request");
        assert_eq!(filtered_users_resp.status(), reqwest::StatusCode::OK);
        let filtered_users_body: serde_json::Value = filtered_users_resp
            .json()
            .await
            .expect("filtered users json");
        assert_eq!(
            filtered_users_body
                .get("total")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            filtered_users_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.get("userId"))
                .and_then(|value| value.as_str()),
            Some(user.user_id.as_str())
        );

        let detail_resp = client
            .get(format!("http://{}/api/users/{}", addr, user.user_id))
            .send()
            .await
            .expect("detail request");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("detail json");
        assert_eq!(
            detail_body
                .get("effectiveQuota")
                .and_then(|value| value.get("hourlyAnyLimit"))
                .and_then(|value| value.as_i64()),
            Some(0)
        );
        assert_eq!(
            detail_body
                .get("effectiveQuota")
                .and_then(|value| value.get("monthlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(0)
        );
        let breakdown_entries = detail_body
            .get("quotaBreakdown")
            .and_then(|value| value.as_array())
            .expect("quotaBreakdown array");
        assert!(breakdown_entries.iter().any(|entry| {
            entry.get("effectKind").and_then(|value| value.as_str()) == Some("block_all")
        }));
        assert!(breakdown_entries.iter().any(|entry| {
            entry.get("kind").and_then(|value| value.as_str()) == Some("effective")
                && entry.get("hourlyAnyDelta").and_then(|value| value.as_i64()) == Some(0)
                && entry.get("monthlyDelta").and_then(|value| value.as_i64()) == Some(0)
        }));

        let unbind_system_resp = client
            .delete(format!(
                "http://{}/api/users/{}/tags/{}",
                addr, user.user_id, system_tag_id
            ))
            .send()
            .await
            .expect("unbind system tag request");
        assert_eq!(
            unbind_system_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        let delete_system_resp = client
            .delete(format!("http://{}/api/user-tags/{}", addr, system_tag_id))
            .send()
            .await
            .expect("delete system tag request");
        assert_eq!(
            delete_system_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        let delete_custom_resp = client
            .delete(format!("http://{}/api/user-tags/{}", addr, custom_tag_id))
            .send()
            .await
            .expect("delete custom tag request");
        assert_eq!(delete_custom_resp.status(), reqwest::StatusCode::NO_CONTENT);

        let filtered_users_after_delete_resp = client
            .get(format!(
                "http://{}/api/users?page=1&per_page=20&tagId={}",
                addr, custom_tag_id
            ))
            .send()
            .await
            .expect("filtered users after delete request");
        assert_eq!(
            filtered_users_after_delete_resp.status(),
            reqwest::StatusCode::OK
        );
        let filtered_users_after_delete: serde_json::Value = filtered_users_after_delete_resp
            .json()
            .await
            .expect("filtered users after delete json");
        assert_eq!(
            filtered_users_after_delete
                .get("total")
                .and_then(|value| value.as_i64()),
            Some(0)
        );

        let detail_after_resp = client
            .get(format!("http://{}/api/users/{}", addr, user.user_id))
            .send()
            .await
            .expect("detail after delete request");
        assert_eq!(detail_after_resp.status(), reqwest::StatusCode::OK);
        let detail_after: serde_json::Value = detail_after_resp
            .json()
            .await
            .expect("detail after delete json");
        assert!(
            detail_after
                .get("tags")
                .and_then(|value| value.as_array())
                .is_some_and(|tags| tags.iter().all(|tag| {
                    tag.get("tagId").and_then(|value| value.as_str())
                        != Some(custom_tag_id.as_str())
                }))
        );
        assert_eq!(
            detail_after
                .get("effectiveQuota")
                .and_then(|value| value.get("monthlyLimit"))
                .and_then(|value| value.as_i64()),
            Some(0)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_token_management_returns_owner_summary() {
        let db_path = temp_db_path("admin-token-owners");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-token-owner-alice".to_string(),
                username: Some("alice".to_string()),
                name: Some("Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");

        let bound = proxy
            .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
            .await
            .expect("bind alice token");
        let unbound = proxy
            .create_access_token(Some("manual-unbound"))
            .await
            .expect("create unbound token");

        let addr = spawn_admin_tokens_server(proxy, true).await;
        let client = Client::new();

        let list_resp = client
            .get(format!("http://{}/api/tokens?page=1&per_page=20", addr))
            .send()
            .await
            .expect("list tokens request");
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);
        let list_body: serde_json::Value = list_resp.json().await.expect("list tokens json");
        let items = list_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("items is array");

        let bound_item = items
            .iter()
            .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(bound.id.as_str()))
            .expect("bound item exists");
        assert_eq!(
            bound_item
                .get("owner")
                .and_then(|value| value.get("userId"))
                .and_then(|value| value.as_str()),
            Some(alice.user_id.as_str())
        );
        assert_eq!(
            bound_item
                .get("owner")
                .and_then(|value| value.get("displayName"))
                .and_then(|value| value.as_str()),
            Some("Alice")
        );
        assert_eq!(
            bound_item
                .get("owner")
                .and_then(|value| value.get("username"))
                .and_then(|value| value.as_str()),
            Some("alice")
        );

        let unbound_item = items
            .iter()
            .find(|item| {
                item.get("id").and_then(|value| value.as_str()) == Some(unbound.id.as_str())
            })
            .expect("unbound item exists");
        assert!(
            unbound_item
                .get("owner")
                .is_some_and(|value| value.is_null()),
            "unbound token owner should be null"
        );

        let detail_resp = client
            .get(format!("http://{}/api/tokens/{}", addr, bound.id))
            .send()
            .await
            .expect("token detail request");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("token detail json");
        assert_eq!(
            detail_body
                .get("owner")
                .and_then(|value| value.get("userId"))
                .and_then(|value| value.as_str()),
            Some(alice.user_id.as_str())
        );

        let unbound_detail_resp = client
            .get(format!("http://{}/api/tokens/{}", addr, unbound.id))
            .send()
            .await
            .expect("unbound token detail request");
        assert_eq!(unbound_detail_resp.status(), reqwest::StatusCode::OK);
        let unbound_detail: serde_json::Value = unbound_detail_resp
            .json()
            .await
            .expect("unbound token detail json");
        assert!(
            unbound_detail
                .get("owner")
                .is_some_and(|value| value.is_null()),
            "unbound token detail owner should be null"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_unbound_token_usage_lists_only_unbound_tokens_with_search_sort_and_pagination() {
        let db_path = temp_db_path("admin-unbound-token-usage");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-unbound-token-usage-alice".to_string(),
                username: Some("alice".to_string()),
                name: Some("Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");

        let bound = proxy
            .ensure_user_token_binding(&alice.user_id, Some("bound-owner"))
            .await
            .expect("bind alice token");
        let unbound_primary = proxy
            .create_access_token(Some("manual-unbound-alpha"))
            .await
            .expect("create primary unbound token");
        let grouped_unbound = proxy
            .create_access_tokens_batch("ops", 1, Some("grouped-unbound"))
            .await
            .expect("create grouped unbound token")
            .into_iter()
            .next()
            .expect("grouped token exists");
        let never_used_unbound = proxy
            .create_access_token(Some("never-used-unbound"))
            .await
            .expect("create never-used unbound token");

        for _ in 0..2 {
            let _ = proxy
                .check_token_hourly_requests(&unbound_primary.id)
                .await
                .expect("seed primary hourly-any");
        }
        for _ in 0..3 {
            let _ = proxy
                .check_token_quota(&unbound_primary.id)
                .await
                .expect("seed primary quota");
        }
        for _ in 0..2 {
            proxy
                .record_token_attempt(
                    &unbound_primary.id,
                    &Method::POST,
                    "/mcp",
                    None,
                    Some(200),
                    Some(0),
                    true,
                    "success",
                    None,
                )
                .await
                .expect("record primary success");
        }
        proxy
            .record_token_attempt(
                &unbound_primary.id,
                &Method::POST,
                "/mcp",
                None,
                Some(500),
                Some(-32001),
                true,
                "error",
                Some("upstream error"),
            )
            .await
            .expect("record primary error");

        let _ = proxy
            .check_token_hourly_requests(&grouped_unbound.id)
            .await
            .expect("seed grouped hourly-any");
        let _ = proxy
            .check_token_quota(&grouped_unbound.id)
            .await
            .expect("seed grouped quota");
        proxy
            .record_token_attempt(
                &grouped_unbound.id,
                &Method::POST,
                "/mcp",
                None,
                Some(200),
                Some(0),
                true,
                "success",
                None,
            )
            .await
            .expect("record grouped success");

        let _ = proxy
            .check_token_hourly_requests(&bound.id)
            .await
            .expect("seed bound hourly-any");
        let _ = proxy
            .check_token_quota(&bound.id)
            .await
            .expect("seed bound quota");
        proxy
            .record_token_attempt(
                &bound.id,
                &Method::POST,
                "/mcp",
                None,
                Some(200),
                Some(0),
                true,
                "success",
                None,
            )
            .await
            .expect("record bound success");

        let (breakage_key_a_id, _) = proxy
            .add_or_undelete_key_with_status("tvly-unbound-breakage-sort-key-a")
            .await
            .expect("create breakage key a");
        let (breakage_key_b_id, _) = proxy
            .add_or_undelete_key_with_status("tvly-unbound-breakage-sort-key-b")
            .await
            .expect("create breakage key b");
        let now = chrono::Utc::now().timestamp();
        let pool = connect_sqlite_test_pool(&db_str).await;
        sqlx::query(
            r#"INSERT INTO token_api_key_bindings (token_id, api_key_id, created_at, updated_at, last_success_at)
               VALUES (?, ?, ?, ?, ?), (?, ?, ?, ?, ?), (?, ?, ?, ?, ?)"#,
        )
        .bind(&unbound_primary.id)
        .bind(&breakage_key_a_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(&grouped_unbound.id)
        .bind(&breakage_key_a_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(&grouped_unbound.id)
        .bind(&breakage_key_b_id)
        .bind(now)
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed token api key bindings");
        proxy
            .mark_key_quota_exhausted_by_secret("tvly-unbound-breakage-sort-key-a")
            .await
            .expect("mark breakage key a exhausted");
        proxy
            .mark_key_quota_exhausted_by_secret("tvly-unbound-breakage-sort-key-b")
            .await
            .expect("mark breakage key b exhausted");
        sqlx::query(
            r#"INSERT INTO api_key_quarantines
               (id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, 'system', 'account_deactivated', 'Upstream account deactivated', 'blocked-key test fixture', ?, NULL),
                      (?, ?, 'system', 'account_deactivated', 'Upstream account deactivated', 'blocked-key test fixture', ?, NULL)"#,
        )
        .bind("unbound-breakage-sort-quarantine-a")
        .bind(&breakage_key_a_id)
        .bind(now)
        .bind("unbound-breakage-sort-quarantine-b")
        .bind(&breakage_key_b_id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("seed active blocked-key quarantines");
        sqlx::query(
            r#"UPDATE subject_key_breakages
               SET key_status = 'quarantined',
                   reason_code = 'account_deactivated',
                   reason_summary = 'Upstream account deactivated',
                   source = 'auto',
                   updated_at = ?,
                   latest_break_at = ?
               WHERE key_id IN (?, ?)"#,
        )
        .bind(now)
        .bind(now)
        .bind(&breakage_key_a_id)
        .bind(&breakage_key_b_id)
        .execute(&pool)
        .await
        .expect("promote breakage fixtures to blocked-key reasons");

        let addr = spawn_admin_tokens_server(proxy, true).await;
        let client = Client::new();

        let list_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=1&per_page=20",
                addr
            ))
            .send()
            .await
            .expect("list unbound token usage request");
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);
        let list_body: serde_json::Value = list_resp
            .json()
            .await
            .expect("list unbound token usage json");
        let items = list_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("items array");
        let ids: Vec<&str> = items
            .iter()
            .filter_map(|item| item.get("tokenId").and_then(|value| value.as_str()))
            .collect();
        assert_eq!(
            list_body.get("total").and_then(|value| value.as_i64()),
            Some(3)
        );
        assert!(ids.contains(&unbound_primary.id.as_str()));
        assert!(ids.contains(&grouped_unbound.id.as_str()));
        assert!(ids.contains(&never_used_unbound.id.as_str()));
        assert!(!ids.contains(&bound.id.as_str()));

        let search_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=1&per_page=20&q={}",
                addr,
                urlencoding::encode("ops")
            ))
            .send()
            .await
            .expect("search unbound token usage request");
        assert_eq!(search_resp.status(), reqwest::StatusCode::OK);
        let search_body: serde_json::Value = search_resp
            .json()
            .await
            .expect("search unbound token usage json");
        let search_items = search_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("search items array");
        assert_eq!(search_items.len(), 1);
        assert_eq!(
            search_items[0]
                .get("tokenId")
                .and_then(|value| value.as_str()),
            Some(grouped_unbound.id.as_str())
        );
        assert_eq!(
            search_items[0]
                .get("group")
                .and_then(|value| value.as_str()),
            Some("ops")
        );

        let sorted_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=1&per_page=20&sort=dailySuccessRate&order=desc",
                addr
            ))
            .send()
            .await
            .expect("sort unbound token usage request");
        assert_eq!(sorted_resp.status(), reqwest::StatusCode::OK);
        let sorted_body: serde_json::Value = sorted_resp
            .json()
            .await
            .expect("sort unbound token usage json");
        let sorted_items = sorted_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("sorted items array");
        assert_eq!(
            sorted_items[0]
                .get("tokenId")
                .and_then(|value| value.as_str()),
            Some(grouped_unbound.id.as_str())
        );
        assert_eq!(
            sorted_items[0]
                .get("dailySuccess")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            sorted_items[0]
                .get("dailyFailure")
                .and_then(|value| value.as_i64()),
            Some(0)
        );

        let broken_sorted_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=1&per_page=20&sort=monthlyBrokenCount&order=desc",
                addr
            ))
            .send()
            .await
            .expect("monthly broken sort unbound token usage request");
        assert_eq!(broken_sorted_resp.status(), reqwest::StatusCode::OK);
        let broken_sorted_body: serde_json::Value = broken_sorted_resp
            .json()
            .await
            .expect("monthly broken sort unbound token usage json");
        let broken_sorted_items = broken_sorted_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("monthly broken sorted items array");
        assert_eq!(
            broken_sorted_items[0]
                .get("tokenId")
                .and_then(|value| value.as_str()),
            Some(grouped_unbound.id.as_str())
        );
        assert_eq!(
            broken_sorted_items[0]
                .get("monthlyBrokenCount")
                .and_then(|value| value.as_i64()),
            Some(2)
        );
        assert_eq!(
            broken_sorted_items[1]
                .get("tokenId")
                .and_then(|value| value.as_str()),
            Some(unbound_primary.id.as_str())
        );
        assert_eq!(
            broken_sorted_items[1]
                .get("monthlyBrokenCount")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert!(
            broken_sorted_items[2]
                .get("monthlyBrokenCount")
                .is_some_and(|value| value.is_null())
        );

        let last_used_asc_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=1&per_page=20&sort=lastUsedAt&order=asc",
                addr
            ))
            .send()
            .await
            .expect("last-used asc unbound token usage request");
        assert_eq!(last_used_asc_resp.status(), reqwest::StatusCode::OK);
        let last_used_asc_body: serde_json::Value = last_used_asc_resp
            .json()
            .await
            .expect("last-used asc unbound token usage json");
        let last_used_asc_items = last_used_asc_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("last-used asc items array");
        assert_eq!(
            last_used_asc_items[0]
                .get("tokenId")
                .and_then(|value| value.as_str()),
            Some(never_used_unbound.id.as_str())
        );
        assert!(
            last_used_asc_items[0]
                .get("lastUsedAt")
                .is_some_and(|value| value.is_null())
        );

        let paged_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=2&per_page=1&sort=quotaMonthlyUsed&order=desc",
                addr
            ))
            .send()
            .await
            .expect("paged unbound token usage request");
        assert_eq!(paged_resp.status(), reqwest::StatusCode::OK);
        let paged_body: serde_json::Value = paged_resp
            .json()
            .await
            .expect("paged unbound token usage json");
        let paged_items = paged_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("paged items array");
        assert_eq!(
            paged_body.get("total").and_then(|value| value.as_i64()),
            Some(3)
        );
        assert_eq!(
            paged_body.get("page").and_then(|value| value.as_i64()),
            Some(2)
        );
        assert_eq!(paged_items.len(), 1);
        assert_eq!(
            paged_items[0]
                .get("tokenId")
                .and_then(|value| value.as_str()),
            Some(grouped_unbound.id.as_str())
        );

        let empty_resp = client
            .get(format!(
                "http://{}/api/tokens/unbound-usage?page=1&per_page=20&q={}",
                addr,
                urlencoding::encode("missing-token")
            ))
            .send()
            .await
            .expect("empty unbound token usage request");
        assert_eq!(empty_resp.status(), reqwest::StatusCode::OK);
        let empty_body: serde_json::Value = empty_resp
            .json()
            .await
            .expect("empty unbound token usage json");
        assert_eq!(
            empty_body.get("total").and_then(|value| value.as_i64()),
            Some(0)
        );
        assert!(
            empty_body
                .get("items")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.is_empty())
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_token_log_views_include_business_credits() {
        let db_path = temp_db_path("admin-token-log-business-credits");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let token = proxy
            .create_access_token(Some("admin-token-log-business-credits"))
            .await
            .expect("create token");

        proxy
            .record_token_attempt(
                &token.id,
                &Method::POST,
                "/mcp/sse",
                None,
                Some(200),
                Some(0),
                false,
                "success",
                None,
            )
            .await
            .expect("record legacy log without credits");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open db pool");
        sqlx::query(
            r#"
            UPDATE auth_token_logs
            SET request_kind_key = 'mcp:raw:/mcp',
                request_kind_label = 'MCP | /mcp'
            WHERE token_id = ?
              AND path = '/mcp/sse'
            "#,
        )
        .bind(&token.id)
        .execute(&pool)
        .await
        .expect("downgrade legacy mcp raw row to stale root fallback");

        proxy
            .record_token_attempt(
                &token.id,
                &Method::POST,
                "/api/tavily/search",
                None,
                Some(200),
                Some(200),
                true,
                "success",
                None,
            )
            .await
            .expect("record api search log");
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                created_at,
                counts_business_quota,
                billing_state
            ) VALUES (?, 'POST', '/mcp', NULL, 202, NULL, 'mcp:notifications/initialized', 'MCP | notifications/initialized', NULL, 'unknown', NULL, NULL, 'none', NULL, ?, 0, 'none')
            "#,
        )
        .bind(&token.id)
        .bind(Utc::now().timestamp() + 2)
        .execute(&pool)
        .await
        .expect("insert neutral token log");
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                created_at,
                counts_business_quota,
                billing_state
            ) VALUES (?, 'DELETE', '/mcp', NULL, 405, 405, 'mcp:session-delete-unsupported', 'MCP | session delete unsupported', NULL, 'error', 'Method Not Allowed: Session termination not supported', 'mcp_method_405', 'none', NULL, ?, 0, 'none')
            "#,
        )
        .bind(&token.id)
        .bind(Utc::now().timestamp() + 3)
        .execute(&pool)
        .await
        .expect("insert session delete neutral token log");

        let mcp_search_kind = classify_token_request_kind(
            "/mcp",
            Some(
                br#"{
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": { "name": "tavily-search" }
                }"#,
            ),
        );
        let charged_log_id = proxy
            .record_pending_billing_attempt_with_kind(
                &token.id,
                &Method::POST,
                "/mcp",
                None,
                Some(200),
                Some(0),
                true,
                "success",
                None,
                4,
                &mcp_search_kind,
                None,
            )
            .await
            .expect("record pending billing log");
        assert_eq!(
            proxy
                .settle_pending_billing_attempt(charged_log_id)
                .await
                .expect("settle pending billing log"),
            PendingBillingSettleOutcome::Charged
        );

        let addr = spawn_admin_tokens_server(proxy, true).await;
        let client = Client::new();

        let logs_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs?limit=20",
                addr, token.id
            ))
            .send()
            .await
            .expect("logs request");
        assert_eq!(logs_resp.status(), reqwest::StatusCode::OK);
        let logs_body: serde_json::Value = logs_resp.json().await.expect("logs json");
        let logs = logs_body.as_array().expect("logs array");
        assert_eq!(logs.len(), 5);
        let charged_log = logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:search")
            })
            .expect("mcp search log");
        assert_eq!(
            charged_log
                .get("business_credits")
                .and_then(|value| value.as_i64()),
            Some(4)
        );
        assert_eq!(
            charged_log
                .get("request_kind_label")
                .and_then(|value| value.as_str()),
            Some("MCP | search")
        );
        let legacy_log = logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:unsupported-path")
            })
            .expect("canonical unsupported-path log");
        assert_eq!(
            legacy_log
                .get("request_kind_label")
                .and_then(|value| value.as_str()),
            Some("MCP | unsupported path")
        );
        assert_eq!(
            legacy_log
                .get("request_kind_detail")
                .and_then(|value| value.as_str()),
            Some("/mcp/sse")
        );
        assert!(
            legacy_log.get("legacyRequestKindKey").is_none(),
            "token log payload should not expose legacy request-kind snapshots"
        );
        let neutral_log = logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:notifications/initialized")
            })
            .expect("neutral mcp notification log");
        assert_eq!(
            neutral_log
                .get("operationalClass")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );
        assert_eq!(
            neutral_log
                .get("requestKindBillingGroup")
                .and_then(|value| value.as_str()),
            Some("non_billable")
        );
        let session_delete_log = logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:session-delete-unsupported")
            })
            .expect("session delete token log");
        assert_eq!(
            session_delete_log
                .get("result_status")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );

        let page_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0",
                addr, token.id
            ))
            .send()
            .await
            .expect("logs page request");
        assert_eq!(page_resp.status(), reqwest::StatusCode::OK);
        let page_body: serde_json::Value = page_resp.json().await.expect("logs page json");
        let items = page_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("logs page items");
        let request_kind_options = page_body
            .get("request_kind_options")
            .and_then(|value| value.as_array())
            .expect("request kind options array");
        assert_eq!(items.len(), 5);
        assert_eq!(request_kind_options.len(), 5);
        let search_option = request_kind_options
            .iter()
            .find(|value| {
                value
                    .get("key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:search")
            })
            .expect("mcp search option");
        assert_eq!(
            search_option
                .get("protocol_group")
                .and_then(|value| value.as_str()),
            Some("mcp")
        );
        assert_eq!(
            search_option
                .get("billing_group")
                .and_then(|value| value.as_str()),
            Some("billable")
        );
        assert_eq!(
            search_option.get("count").and_then(|value| value.as_i64()),
            Some(1)
        );
        let legacy_option = request_kind_options
            .iter()
            .find(|value| {
                value
                    .get("key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:unsupported-path")
            })
            .expect("unsupported-path option");
        assert_eq!(
            legacy_option
                .get("protocol_group")
                .and_then(|value| value.as_str()),
            Some("mcp")
        );
        assert_eq!(
            legacy_option
                .get("billing_group")
                .and_then(|value| value.as_str()),
            Some("non_billable")
        );
        assert_eq!(
            legacy_option.get("count").and_then(|value| value.as_i64()),
            Some(1)
        );
        let session_delete_option = request_kind_options
            .iter()
            .find(|value| {
                value
                    .get("key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:session-delete-unsupported")
            })
            .expect("session-delete option");
        assert_eq!(
            session_delete_option
                .get("billing_group")
                .and_then(|value| value.as_str()),
            Some("non_billable")
        );
        assert_eq!(
            session_delete_option
                .get("count")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        let page_search_log = items
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:search")
            })
            .expect("paged mcp search log");
        assert_eq!(
            page_search_log
                .get("business_credits")
                .and_then(|value| value.as_i64()),
            Some(4)
        );
        assert_eq!(
            page_search_log
                .get("request_kind_label")
                .and_then(|value| value.as_str()),
            Some("MCP | search")
        );
        assert_eq!(
            page_search_log
                .get("operationalClass")
                .and_then(|value| value.as_str()),
            Some("success")
        );
        assert_eq!(
            page_search_log
                .get("requestKindProtocolGroup")
                .and_then(|value| value.as_str()),
            Some("mcp")
        );
        assert_eq!(
            page_search_log
                .get("requestKindBillingGroup")
                .and_then(|value| value.as_str()),
            Some("billable")
        );
        let paged_legacy_log = items
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:unsupported-path")
            })
            .expect("paged unsupported-path log");
        assert_eq!(
            paged_legacy_log
                .get("request_kind_label")
                .and_then(|value| value.as_str()),
            Some("MCP | unsupported path")
        );
        assert!(
            paged_legacy_log.get("legacyRequestKindKey").is_none(),
            "paged token log payload should not expose legacy request-kind snapshots"
        );
        let paged_session_delete_log = items
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:session-delete-unsupported")
            })
            .expect("paged session delete log");
        assert_eq!(
            paged_session_delete_log
                .get("result_status")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );

        let neutral_page_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0&operational_class=neutral",
                addr, token.id
            ))
            .send()
            .await
            .expect("neutral token logs page request");
        assert_eq!(neutral_page_resp.status(), reqwest::StatusCode::OK);
        let neutral_page_body: serde_json::Value = neutral_page_resp
            .json()
            .await
            .expect("neutral token logs page json");
        let neutral_items = neutral_page_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("neutral token logs page items");
        assert_eq!(neutral_items.len(), 3);
        let neutral_kinds = neutral_items
            .iter()
            .filter_map(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|inner| inner.as_str())
            })
            .collect::<Vec<_>>();
        assert!(neutral_kinds.contains(&"mcp:notifications/initialized"));
        assert!(neutral_kinds.contains(&"mcp:session-delete-unsupported"));
        assert!(neutral_kinds.contains(&"mcp:unsupported-path"));

        let neutral_result_page_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0&result=neutral",
                addr, token.id
            ))
            .send()
            .await
            .expect("neutral result token logs page request");
        assert_eq!(neutral_result_page_resp.status(), reqwest::StatusCode::OK);
        let neutral_result_page_body: serde_json::Value = neutral_result_page_resp
            .json()
            .await
            .expect("neutral result token logs page json");
        let neutral_result_items = neutral_result_page_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("neutral result token logs items");
        assert_eq!(neutral_result_items.len(), 3);
        assert!(neutral_result_items.iter().any(|value| {
            value.get("request_kind_key").and_then(|kind| kind.as_str())
                == Some("mcp:session-delete-unsupported")
        }));
        assert!(
            neutral_result_items
                .iter()
                .find(|value| {
                    value.get("request_kind_key").and_then(|kind| kind.as_str())
                        == Some("mcp:session-delete-unsupported")
                })
                .and_then(|value| value
                    .get("result_status")
                    .and_then(|status| status.as_str()))
                == Some("neutral")
        );

        let error_result_page_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0&result=error",
                addr, token.id
            ))
            .send()
            .await
            .expect("error result token logs page request");
        assert_eq!(error_result_page_resp.status(), reqwest::StatusCode::OK);
        let error_result_page_body: serde_json::Value = error_result_page_resp
            .json()
            .await
            .expect("error result token logs page json");
        let error_result_items = error_result_page_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("error result token logs items");
        assert!(error_result_items.iter().all(|value| {
            value.get("request_kind_key").and_then(|kind| kind.as_str())
                != Some("mcp:session-delete-unsupported")
        }));

        let filtered_page_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0&request_kind=api%3Asearch&request_kind=mcp%3Asearch",
                addr, token.id
            ))
            .send()
            .await
            .expect("filtered logs page request");
        assert_eq!(filtered_page_resp.status(), reqwest::StatusCode::OK);
        let filtered_page_body: serde_json::Value = filtered_page_resp
            .json()
            .await
            .expect("filtered logs page json");
        let filtered_items = filtered_page_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("filtered logs page items");
        assert_eq!(filtered_items.len(), 2);
        let filtered_keys = filtered_items
            .iter()
            .filter_map(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .map(str::to_string)
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            filtered_keys,
            std::collections::BTreeSet::from(["api:search".to_string(), "mcp:search".to_string(),])
        );

        let filtered_legacy_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0&request_kind=mcp%3Araw%3A%2Fmcp%2Fsse",
                addr, token.id
            ))
            .send()
            .await
            .expect("filtered legacy logs page request");
        assert_eq!(filtered_legacy_resp.status(), reqwest::StatusCode::OK);
        let filtered_legacy_body: serde_json::Value = filtered_legacy_resp
            .json()
            .await
            .expect("filtered legacy logs page json");
        let filtered_legacy_items = filtered_legacy_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("filtered legacy logs page items");
        assert_eq!(filtered_legacy_items.len(), 1);
        assert_eq!(
            filtered_legacy_items[0]
                .get("request_kind_label")
                .and_then(|value| value.as_str()),
            Some("MCP | unsupported path")
        );

        let mut events_resp = client
            .get(format!("http://{}/api/tokens/{}/events", addr, token.id))
            .send()
            .await
            .expect("events request");
        assert_eq!(events_resp.status(), reqwest::StatusCode::OK);
        let snapshot_event = read_sse_event_until(
            &mut events_resp,
            |chunk| chunk.contains("data: "),
            "token snapshot event",
        )
        .await;
        let snapshot_line = snapshot_event
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("snapshot data line");
        let snapshot_json: serde_json::Value =
            serde_json::from_str(snapshot_line).expect("snapshot payload json");
        let snapshot_logs = snapshot_json
            .get("logs")
            .and_then(|value| value.as_array())
            .expect("snapshot logs array");
        assert_eq!(snapshot_logs.len(), 5);
        let snapshot_search_log = snapshot_logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:search")
            })
            .expect("snapshot mcp search log");
        assert_eq!(
            snapshot_search_log
                .get("business_credits")
                .and_then(|value| value.as_i64()),
            Some(4)
        );
        assert_eq!(
            snapshot_search_log
                .get("request_kind_label")
                .and_then(|value| value.as_str()),
            Some("MCP | search")
        );
        let snapshot_neutral_log = snapshot_logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:notifications/initialized")
            })
            .expect("snapshot neutral log");
        assert_eq!(
            snapshot_neutral_log
                .get("operationalClass")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );
        let snapshot_session_delete_log = snapshot_logs
            .iter()
            .find(|value| {
                value
                    .get("request_kind_key")
                    .and_then(|kind| kind.as_str())
                    .is_some_and(|kind| kind == "mcp:session-delete-unsupported")
            })
            .expect("snapshot session delete log");
        assert_eq!(
            snapshot_session_delete_log
                .get("operationalClass")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );
        assert_eq!(
            snapshot_session_delete_log
                .get("result_status")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );
        drop(events_resp);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn token_log_details_return_linked_bodies_and_page_results_keep_null_payloads() {
        let db_path = temp_db_path("token-log-details-linked");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-token-log-details-linked".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let token = proxy
            .create_access_token(Some("token-log-details-linked"))
            .await
            .expect("create token");
        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key exists")
            .id;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let created_at = Utc::now().timestamp();
        let request_log_id: i64 = sqlx::query_scalar(
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
                request_kind_detail,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
            ) VALUES (?, ?, 'POST', '/mcp', NULL, 200, 200, NULL, 'success', 'mcp:search', 'MCP | search', NULL, 2, NULL, 'none', NULL, ?, ?, '["x-request-id"]', '[]', 'visible', ?)
            RETURNING id
            "#,
        )
        .bind(&key_id)
        .bind(&token.id)
        .bind(br#"{"tool":"search"}"#.to_vec())
        .bind(br#"{"result":"ok"}"#.to_vec())
        .bind(created_at)
        .fetch_one(&pool)
        .await
        .expect("insert request log");

        let token_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/mcp', NULL, 200, 200, 'mcp:search', 'MCP | search', NULL, 'success', NULL, NULL, 'none', NULL, 1, 2, 'charged', ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(request_log_id)
        .bind(created_at + 1)
        .fetch_one(&pool)
        .await
        .expect("insert token log");

        let addr = spawn_admin_tokens_server(proxy, true).await;
        let client = Client::new();

        let page_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/page?page=1&per_page=20&since=0",
                addr, token.id
            ))
            .send()
            .await
            .expect("token logs page");
        assert_eq!(page_resp.status(), reqwest::StatusCode::OK);
        let page_body: serde_json::Value = page_resp.json().await.expect("token logs page json");
        let page_item = page_body
            .get("items")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("id")
                        .and_then(|value| value.as_i64())
                        .is_some_and(|value| value == token_log_id)
                })
            })
            .expect("inserted token page item");
        assert!(
            page_item
                .get("request_body")
                .is_some_and(|value| value.is_null())
        );
        assert!(
            page_item
                .get("response_body")
                .is_some_and(|value| value.is_null())
        );

        let detail_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/{}/details",
                addr, token.id, token_log_id
            ))
            .send()
            .await
            .expect("token log detail");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("token detail json");
        assert_eq!(
            detail_body
                .get("request_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"tool":"search"}"#)
        );
        assert_eq!(
            detail_body
                .get("response_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"result":"ok"}"#)
        );

        let wrong_scope_resp = client
            .get(format!(
                "http://{}/api/tokens/wrong-token/logs/{}/details",
                addr, token_log_id
            ))
            .send()
            .await
            .expect("wrong token detail request");
        assert_eq!(wrong_scope_resp.status(), reqwest::StatusCode::NOT_FOUND);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn token_log_details_return_null_bodies_when_no_request_log_is_linked() {
        let db_path = temp_db_path("token-log-details-unlinked");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-token-log-details-unlinked".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let token = proxy
            .create_access_token(Some("token-log-details-unlinked"))
            .await
            .expect("create token");
        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key exists")
            .id;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let token_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/mcp', NULL, 200, 202, 'mcp:notifications/initialized', 'MCP | notifications/initialized', NULL, 'success', NULL, NULL, 'none', NULL, 0, NULL, 'none', ?, NULL, ?)
            RETURNING id
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(3_000_i64)
        .fetch_one(&pool)
        .await
        .expect("insert token log without request link");

        let addr = spawn_admin_tokens_server(proxy, true).await;
        let client = Client::new();

        let detail_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/{}/details",
                addr, token.id, token_log_id
            ))
            .send()
            .await
            .expect("token log detail");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("token detail json");
        assert!(
            detail_body
                .get("request_body")
                .is_some_and(|value| value.is_null())
        );
        assert!(
            detail_body
                .get("response_body")
                .is_some_and(|value| value.is_null())
        );

        let _ = std::fs::remove_file(db_path);
    }

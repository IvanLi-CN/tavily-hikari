    #[test]
    fn session_delete_unsupported_views_render_neutral_result_status() {
        let token_record = TokenLogRecord {
            id: 3,
            key_id: Some("MZlj".to_string()),
            method: "DELETE".to_string(),
            path: "/mcp".to_string(),
            query: None,
            http_status: Some(405),
            mcp_status: Some(405),
            business_credits: None,
            request_kind_key: "mcp:session-delete-unsupported".to_string(),
            request_kind_label: "MCP | session delete unsupported".to_string(),
            request_kind_detail: None,
            counts_business_quota: false,
            result_status: "error".to_string(),
            error_message: Some("Session termination not supported".to_string()),
            failure_kind: Some("mcp_method_405".to_string()),
            key_effect_code: "none".to_string(),
            key_effect_summary: Some("No automatic status change".to_string()),
            binding_effect_code: "none".to_string(),
            binding_effect_summary: None,
            selection_effect_code: "none".to_string(),
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            created_at: 1_700_000_002,
        };
        let public_view = PublicTokenLogView::from_record(token_record.clone(), UiLanguage::Zh);
        assert_eq!(public_view.result_status, "neutral");

        let admin_token_view = TokenLogView::from(token_record.clone());
        assert_eq!(admin_token_view.result_status, "neutral");
        assert_eq!(admin_token_view.operational_class, "neutral");

        let request_view = RequestLogView::from_request_record(
            RequestLogRecord {
                id: 4,
                key_id: Some("MZlj".to_string()),
                auth_token_id: Some("token-session-delete".to_string()),
                method: "DELETE".to_string(),
                path: "/mcp".to_string(),
                query: None,
                status_code: Some(405),
                tavily_status_code: Some(405),
                error_message: Some("Session termination not supported".to_string()),
                business_credits: None,
                request_kind_key: "mcp:session-delete-unsupported".to_string(),
                request_kind_label: "MCP | session delete unsupported".to_string(),
                request_kind_detail: None,
                request_kind_protocol_group: "mcp".to_string(),
                request_kind_billing_group: "non_billable".to_string(),
                result_status: "error".to_string(),
                failure_kind: Some("mcp_method_405".to_string()),
                key_effect_code: "none".to_string(),
                key_effect_summary: Some("No automatic status change".to_string()),
                binding_effect_code: "none".to_string(),
                binding_effect_summary: None,
                selection_effect_code: "none".to_string(),
                selection_effect_summary: None,
                operational_class: "neutral".to_string(),
                request_body: Vec::new(),
                response_body: br#"{"jsonrpc":"2.0","id":"server-error","error":{"code":-32600,"message":"Method Not Allowed: Session termination not supported"}}"#.to_vec(),
                created_at: 1_700_000_003,
                forwarded_headers: Vec::new(),
                dropped_headers: Vec::new(),
                gateway_mode: None,
                experiment_variant: None,
                proxy_session_id: None,
                routing_subject_hash: None,
                upstream_operation: None,
                fallback_reason: None,
            },
            false,
        );
        assert_eq!(request_view.result_status, "neutral");
        assert_eq!(request_view.operational_class, "neutral");
    }

    #[test]
    fn admin_token_log_view_exposes_failure_kind_and_key_effect_fields() {
        let view = TokenLogView::from(TokenLogRecord {
            id: 2,
            key_id: Some("Qn8R".to_string()),
            method: "POST".to_string(),
            path: "/api/tavily/search".to_string(),
            query: None,
            http_status: Some(401),
            mcp_status: Some(401),
            business_credits: Some(1),
            request_kind_key: "api:search".to_string(),
            request_kind_label: "API | search".to_string(),
            request_kind_detail: None,
            counts_business_quota: true,
            result_status: "error".to_string(),
            error_message: Some("account deactivated".to_string()),
            failure_kind: Some("upstream_account_deactivated_401".to_string()),
            key_effect_code: "quarantined".to_string(),
            key_effect_summary: Some("The system automatically quarantined this key".to_string()),
            binding_effect_code: "none".to_string(),
            binding_effect_summary: None,
            selection_effect_code: "none".to_string(),
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            created_at: 1_700_000_001,
        });

        let json = serde_json::to_value(&view).expect("serialize admin token log view");
        let object = json
            .as_object()
            .expect("admin token log should serialize to object");
        assert_eq!(
            object.get("failure_kind").and_then(|value| value.as_str()),
            Some("upstream_account_deactivated_401"),
        );
        assert_eq!(
            object
                .get("key_effect_code")
                .and_then(|value| value.as_str()),
            Some("quarantined"),
        );
        assert_eq!(
            object
                .get("key_effect_summary")
                .and_then(|value| value.as_str()),
            Some("The system automatically quarantined this key"),
        );
        assert_eq!(
            object
                .get("binding_effect_code")
                .and_then(|value| value.as_str()),
            Some("none"),
        );
        assert_eq!(
            object
                .get("selection_effect_code")
                .and_then(|value| value.as_str()),
            Some("none"),
        );
        assert_eq!(
            object
                .get("operationalClass")
                .and_then(|value| value.as_str()),
            Some("upstream_error"),
        );
        assert_eq!(
            object
                .get("requestKindProtocolGroup")
                .and_then(|value| value.as_str()),
            Some("api"),
        );
        assert_eq!(
            object
                .get("requestKindBillingGroup")
                .and_then(|value| value.as_str()),
            Some("billable"),
        );
    }

    #[tokio::test]
    async fn api_key_detail_requires_admin_auth() {
        let db_path = temp_db_path("api-key-detail-auth");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-detail-auth".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key")
            .id;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&db_path)
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal),
            )
            .await
            .expect("open db pool");
        sqlx::query(
            r#"INSERT INTO api_key_quarantines
               (key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
        )
        .bind(&key_id)
        .bind("/api/tavily/search")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("The account associated with this API key has been deactivated.")
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("insert quarantine");
        let admin_password = "detail-auth-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build client");

        let unauth_resp = client
            .get(format!("http://{}/api/keys/{}", admin_addr, key_id))
            .send()
            .await
            .expect("unauth key detail request");
        assert_eq!(unauth_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let login_resp = client
            .post(format!("http://{}/api/admin/login", admin_addr))
            .json(&serde_json::json!({ "password": admin_password }))
            .send()
            .await
            .expect("admin login");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
            .expect("admin session cookie");

        let auth_resp = client
            .get(format!("http://{}/api/keys/{}", admin_addr, key_id))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("authed key detail request");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = auth_resp.json().await.expect("detail json");
        assert_eq!(
            detail_body
                .get("quarantine")
                .and_then(|value| value.get("reasonDetail"))
                .and_then(|value| value.as_str()),
            Some("The account associated with this API key has been deactivated.")
        );

        let list_resp = client
            .get(format!("http://{}/api/keys", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("authed key list request");
        assert_eq!(list_resp.status(), reqwest::StatusCode::OK);
        let list_body: serde_json::Value = list_resp.json().await.expect("list json");
        let listed = list_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("key list items array")
            .iter()
            .find(|value| value.get("id").and_then(|v| v.as_str()) == Some(key_id.as_str()))
            .expect("key in list");
        assert_eq!(
            listed
                .get("quarantine")
                .and_then(|value| value.get("reasonDetail")),
            None
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_key_list_supports_pagination_filters_and_facets() {
        let db_path = temp_db_path("admin-key-list-pagination");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-pagination-seed".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let seeded_key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list initial key metrics")
            .into_iter()
            .next()
            .expect("seeded key")
            .id;
        proxy
            .soft_delete_key_by_id(&seeded_key_id)
            .await
            .expect("soft delete seeded key");

        let (alpha_active_id, _) = proxy
            .add_or_undelete_key_with_status_in_group(
                "tvly-pagination-alpha-active",
                Some("team-a"),
            )
            .await
            .expect("create alpha active");
        let (alpha_quarantined_id, _) = proxy
            .add_or_undelete_key_with_status_in_group(
                "tvly-pagination-alpha-quarantine",
                Some("team-a"),
            )
            .await
            .expect("create alpha quarantined");
        let (beta_disabled_id, _) = proxy
            .add_or_undelete_key_with_status_in_group(
                "tvly-pagination-beta-disabled",
                Some("team-b"),
            )
            .await
            .expect("create beta disabled");
        let (beta_active_id, _) = proxy
            .add_or_undelete_key_with_status_in_group("tvly-pagination-beta-active", Some("team-b"))
            .await
            .expect("create beta active");
        let (ungrouped_exhausted_id, _) = proxy
            .add_or_undelete_key_with_status_in_group("tvly-pagination-gamma-exhausted", None)
            .await
            .expect("create gamma exhausted");

        proxy
            .disable_key_by_id(&beta_disabled_id)
            .await
            .expect("disable beta key");
        proxy
            .mark_key_quota_exhausted_by_secret("tvly-pagination-gamma-exhausted")
            .await
            .expect("mark gamma exhausted");

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

        for (key_id, last_used_at) in [
            (&alpha_active_id, 500_i64),
            (&alpha_quarantined_id, 400_i64),
            (&beta_active_id, 300_i64),
            (&beta_disabled_id, 200_i64),
            (&ungrouped_exhausted_id, 100_i64),
        ] {
            sqlx::query(
                r#"UPDATE api_keys
                   SET last_used_at = ?, status_changed_at = ?
                   WHERE id = ?"#,
            )
            .bind(last_used_at)
            .bind(last_used_at)
            .bind(key_id)
            .execute(&pool)
            .await
            .expect("update last_used_at");
        }

        for (key_id, registration_ip, registration_region) in [
            (&alpha_active_id, Some("8.8.8.8"), Some("US")),
            (
                &alpha_quarantined_id,
                Some("8.8.4.4"),
                Some("US Westfield (MA)"),
            ),
            (&beta_active_id, Some("8.8.8.8"), Some("US")),
            (&beta_disabled_id, Some("9.9.9.9"), None),
            (&ungrouped_exhausted_id, None, None),
        ] {
            sqlx::query(
                r#"UPDATE api_keys
                   SET registration_ip = ?, registration_region = ?
                   WHERE id = ?"#,
            )
            .bind(registration_ip)
            .bind(registration_region)
            .bind(key_id)
            .execute(&pool)
            .await
            .expect("update registration metadata");
        }

        sqlx::query(
            r#"INSERT INTO api_key_quarantines
               (key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
        )
        .bind(&alpha_quarantined_id)
        .bind("/api/tavily/search")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("The account associated with this API key has been deactivated.")
        .bind(401_i64)
        .execute(&pool)
        .await
        .expect("insert quarantine");

        let admin_password = "key-pagination-password";
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

        let page_one_resp = client
            .get(format!("http://{}/api/keys?page=1&per_page=2", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("page one request");
        assert_eq!(page_one_resp.status(), reqwest::StatusCode::OK);
        let page_one_body: serde_json::Value = page_one_resp.json().await.expect("page one json");
        assert_eq!(
            page_one_body.get("total").and_then(|value| value.as_i64()),
            Some(5)
        );
        assert_eq!(
            page_one_body.get("page").and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            page_one_body
                .get("perPage")
                .and_then(|value| value.as_i64()),
            Some(2)
        );
        let page_one_items = page_one_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("page one items");
        assert_eq!(page_one_items.len(), 2);
        assert_eq!(
            page_one_items[0].get("id").and_then(|value| value.as_str()),
            Some(alpha_active_id.as_str())
        );
        assert_eq!(
            page_one_items[1].get("id").and_then(|value| value.as_str()),
            Some(alpha_quarantined_id.as_str())
        );

        let group_facets = page_one_body
            .get("facets")
            .and_then(|value| value.get("groups"))
            .and_then(|value| value.as_array())
            .expect("group facets");
        let group_counts = group_facets
            .iter()
            .map(|value| {
                (
                    value
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                    value
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or_default(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(group_counts.get("").copied(), Some(1));
        assert_eq!(group_counts.get("team-a").copied(), Some(2));
        assert_eq!(group_counts.get("team-b").copied(), Some(2));

        let status_facets = page_one_body
            .get("facets")
            .and_then(|value| value.get("statuses"))
            .and_then(|value| value.as_array())
            .expect("status facets");
        let status_counts = status_facets
            .iter()
            .map(|value| {
                (
                    value
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                    value
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or_default(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(status_counts.get("active").copied(), Some(2));
        assert_eq!(status_counts.get("disabled").copied(), Some(1));
        assert_eq!(status_counts.get("exhausted").copied(), Some(1));
        assert_eq!(status_counts.get("quarantined").copied(), Some(1));

        let region_facets = page_one_body
            .get("facets")
            .and_then(|value| value.get("regions"))
            .and_then(|value| value.as_array())
            .expect("region facets");
        let region_counts = region_facets
            .iter()
            .map(|value| {
                (
                    value
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                    value
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or_default(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(region_counts.len(), 2);
        assert_eq!(region_counts.get("US Westfield (MA)").copied(), Some(1));
        assert_eq!(region_counts.get("US").copied(), Some(2));

        let filtered_resp = client
            .get(format!(
                "http://{}/api/keys?page=1&per_page=10&group=team-a&group=team-b&status=quarantined&status=disabled",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("filtered request");
        assert_eq!(filtered_resp.status(), reqwest::StatusCode::OK);
        let filtered_body: serde_json::Value = filtered_resp.json().await.expect("filtered json");
        assert_eq!(
            filtered_body.get("total").and_then(|value| value.as_i64()),
            Some(2)
        );
        let filtered_items = filtered_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("filtered items");
        assert_eq!(filtered_items.len(), 2);
        assert_eq!(
            filtered_items[0].get("id").and_then(|value| value.as_str()),
            Some(alpha_quarantined_id.as_str())
        );
        assert_eq!(
            filtered_items[1].get("id").and_then(|value| value.as_str()),
            Some(beta_disabled_id.as_str())
        );

        let filtered_group_counts = filtered_body
            .get("facets")
            .and_then(|value| value.get("groups"))
            .and_then(|value| value.as_array())
            .expect("filtered group facets")
            .iter()
            .map(|value| {
                (
                    value
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                    value
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or_default(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(filtered_group_counts.get("team-a").copied(), Some(1));
        assert_eq!(filtered_group_counts.get("team-b").copied(), Some(1));
        assert_eq!(filtered_group_counts.len(), 2);

        let filtered_status_counts = filtered_body
            .get("facets")
            .and_then(|value| value.get("statuses"))
            .and_then(|value| value.as_array())
            .expect("filtered status facets")
            .iter()
            .map(|value| {
                (
                    value
                        .get("value")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default(),
                    value
                        .get("count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or_default(),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(filtered_status_counts.get("active").copied(), Some(2));
        assert_eq!(filtered_status_counts.get("disabled").copied(), Some(1));
        assert_eq!(filtered_status_counts.get("quarantined").copied(), Some(1));
        assert_eq!(filtered_status_counts.len(), 3);

        let registration_filtered_resp = client
            .get(format!(
                "http://{}/api/keys?page=1&per_page=10&registration_ip=8.8.8.8&region=US",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("registration filtered request");
        assert_eq!(registration_filtered_resp.status(), reqwest::StatusCode::OK);
        let registration_filtered_body: serde_json::Value = registration_filtered_resp
            .json()
            .await
            .expect("registration filtered json");
        assert_eq!(
            registration_filtered_body
                .get("total")
                .and_then(|value| value.as_i64()),
            Some(2)
        );
        let registration_filtered_items = registration_filtered_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("registration filtered items");
        assert_eq!(registration_filtered_items.len(), 2);
        assert_eq!(
            registration_filtered_items[0]
                .get("registration_ip")
                .and_then(|value| value.as_str()),
            Some("8.8.8.8")
        );
        assert_eq!(
            registration_filtered_items[0]
                .get("registration_region")
                .and_then(|value| value.as_str()),
            Some("US")
        );

        let detail_resp = client
            .get(format!(
                "http://{}/api/keys/{}",
                admin_addr, alpha_active_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("detail request");
        assert_eq!(detail_resp.status(), reqwest::StatusCode::OK);
        let detail_body: serde_json::Value = detail_resp.json().await.expect("detail json");
        assert_eq!(
            detail_body
                .get("registration_ip")
                .and_then(|value| value.as_str()),
            Some("8.8.8.8")
        );
        assert_eq!(
            detail_body
                .get("registration_region")
                .and_then(|value| value.as_str()),
            Some("US")
        );

        let ungrouped_resp = client
            .get(format!(
                "http://{}/api/keys?page=1&per_page=10&group=&status=exhausted",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("ungrouped request");
        assert_eq!(ungrouped_resp.status(), reqwest::StatusCode::OK);
        let ungrouped_body: serde_json::Value =
            ungrouped_resp.json().await.expect("ungrouped json");
        let ungrouped_items = ungrouped_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("ungrouped items");
        assert_eq!(
            ungrouped_body.get("total").and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(ungrouped_items.len(), 1);
        assert_eq!(
            ungrouped_items[0]
                .get("id")
                .and_then(|value| value.as_str()),
            Some(ungrouped_exhausted_id.as_str())
        );

        let clamped_resp = client
            .get(format!("http://{}/api/keys?page=99&per_page=2", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("clamped request");
        assert_eq!(clamped_resp.status(), reqwest::StatusCode::OK);
        let clamped_body: serde_json::Value = clamped_resp.json().await.expect("clamped json");
        assert_eq!(
            clamped_body.get("page").and_then(|value| value.as_i64()),
            Some(3)
        );
        let clamped_items = clamped_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("clamped items");
        assert_eq!(clamped_items.len(), 1);
        assert_eq!(
            clamped_items[0].get("id").and_then(|value| value.as_str()),
            Some(ungrouped_exhausted_id.as_str())
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn public_summary_hides_quarantined_and_temporary_isolated_counts_without_admin_auth() {
        let db_path = temp_db_path("public-summary-quarantine-temp-isolated");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-summary-public-quarantine".to_string(),
                "tvly-summary-public-temp-isolated".to_string(),
            ],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let rows = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2, "test fixture should seed two API keys");
        let quarantined_key_id = rows[0].id.clone();
        let temporary_isolated_key_id = rows[1].id.clone();

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
            r#"INSERT INTO api_key_quarantines
               (key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
        )
        .bind(&quarantined_key_id)
        .bind("/api/tavily/search")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("The account associated with this API key has been deactivated.")
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("insert quarantine");
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"INSERT INTO api_key_transient_backoffs
               (key_id, scope, cooldown_until, retry_after_secs, reason_code, source_request_log_id, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?, NULL, ?, ?)"#,
        )
        .bind(&temporary_isolated_key_id)
        .bind("http_global")
        .bind(now + 600)
        .bind(600)
        .bind("upstream_unknown_403")
        .bind(now)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert transient backoff");

        let admin_password = "summary-admin-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build client");

        let public_resp = client
            .get(format!("http://{}/api/summary", admin_addr))
            .send()
            .await
            .expect("public summary request");
        assert_eq!(public_resp.status(), reqwest::StatusCode::OK);
        let public_body: serde_json::Value = public_resp.json().await.expect("public summary json");
        assert_eq!(
            public_body.get("active_keys").and_then(|v| v.as_i64()),
            Some(1)
        );
        assert_eq!(
            public_body.get("quarantined_keys").and_then(|v| v.as_i64()),
            Some(0)
        );
        assert_eq!(
            public_body
                .get("temporary_isolated_keys")
                .and_then(|v| v.as_i64()),
            Some(0)
        );

        let login_resp = client
            .post(format!("http://{}/api/admin/login", admin_addr))
            .json(&serde_json::json!({ "password": admin_password }))
            .send()
            .await
            .expect("admin login");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
            .expect("admin session cookie");

        let admin_resp = client
            .get(format!("http://{}/api/summary", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("admin summary request");
        assert_eq!(admin_resp.status(), reqwest::StatusCode::OK);
        let admin_body: serde_json::Value = admin_resp.json().await.expect("admin summary json");
        assert_eq!(
            admin_body.get("quarantined_keys").and_then(|v| v.as_i64()),
            Some(1)
        );
        assert_eq!(
            admin_body.get("active_keys").and_then(|v| v.as_i64()),
            Some(0)
        );
        assert_eq!(
            admin_body
                .get("temporary_isolated_keys")
                .and_then(|v| v.as_i64()),
            Some(1)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn summary_windows_requires_admin_auth() {
        let db_path = temp_db_path("summary-windows-auth");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-summary-window-auth".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let admin_password = "summary-window-admin-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build client");

        let public_resp = client
            .get(format!("http://{}/api/summary/windows", admin_addr))
            .send()
            .await
            .expect("public summary windows request");
        assert_eq!(public_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn dashboard_overview_requires_admin_auth() {
        let db_path = temp_db_path("dashboard-overview-auth");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-dashboard-overview-auth".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let admin_password = "dashboard-overview-admin-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build client");

        let public_resp = client
            .get(format!("http://{}/api/dashboard/overview", admin_addr))
            .send()
            .await
            .expect("public dashboard overview request");
        assert_eq!(public_resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn dashboard_overview_returns_lightweight_segments() {
        let db_path = temp_db_path("dashboard-overview-lightweight");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-dashboard-overview-lightweight".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key exists")
            .id;

        for index in 0..6 {
            let note = format!("disabled-token-{index}");
            let created = proxy
                .create_access_token(Some(&note))
                .await
                .expect("create access token");
            proxy
                .set_access_token_enabled(&created.id, false)
                .await
                .expect("disable access token");
        }

        let job_id = proxy
            .scheduled_job_start("quota_sync/manual", Some(&key_id), 1)
            .await
            .expect("start job");
        proxy
            .scheduled_job_finish(job_id, "failed", Some("quota sync failed"))
            .await
            .expect("finish job");

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

        sqlx::query("UPDATE api_keys SET status = 'exhausted', status_changed_at = ? WHERE id = ?")
            .bind(Utc::now().timestamp())
            .bind(&key_id)
            .execute(&pool)
            .await
            .expect("mark key exhausted");

        let log_base = Utc::now().timestamp();
        for (offset, result_status) in [
            "success",
            "error",
            "success",
            "quota_exhausted",
            "success",
            "success",
            "error",
        ]
        .into_iter()
        .enumerate()
        {
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
                    created_at
                ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, ?, NULL, NULL, '[]', '[]', ?)
                "#,
            )
            .bind(&key_id)
            .bind(result_status)
            .bind(log_base + offset as i64)
            .execute(&pool)
            .await
            .expect("insert request log");
        }

        let admin_password = "dashboard-overview-lightweight-password";
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

        let resp = client
            .get(format!("http://{}/api/dashboard/overview", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("dashboard overview request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("dashboard overview json");
        assert!(body.get("summary").is_some(), "summary should exist");
        assert!(
            body.get("summaryWindows").is_some(),
            "summary windows should exist"
        );
        assert!(body.get("siteStatus").is_some(), "site status should exist");
        assert!(
            body.get("forwardProxy").is_some(),
            "forward proxy should exist"
        );
        assert!(body.get("trend").is_some(), "trend should exist");
        assert!(
            body.get("hourlyRequestWindow").is_some(),
            "hourly request window should exist"
        );
        assert!(
            body.get("exhaustedKeys").is_some(),
            "exhausted keys should exist"
        );
        assert!(body.get("recentLogs").is_some(), "recent logs should exist");
        assert!(body.get("recentJobs").is_some(), "recent jobs should exist");
        assert!(
            body.get("disabledTokens").is_some(),
            "disabled tokens should exist"
        );
        assert_eq!(
            body.get("tokenCoverage").and_then(|value| value.as_str()),
            Some("truncated")
        );
        assert!(
            body.get("keys").is_none(),
            "overview should not expose legacy keys alias"
        );
        assert!(
            body.get("logs").is_none(),
            "overview should not expose legacy logs alias"
        );
        assert_eq!(
            body.pointer("/exhaustedKeys/0/id")
                .and_then(|value| value.as_str()),
            Some(key_id.as_str())
        );
        assert_eq!(
            body.pointer("/hourlyRequestWindow/visibleBuckets")
                .and_then(|value| value.as_i64()),
            Some(25)
        );
        assert_eq!(
            body.pointer("/hourlyRequestWindow/retainedBuckets")
                .and_then(|value| value.as_i64()),
            Some(49)
        );
        assert_eq!(
            body.pointer("/recentJobs/0/status")
                .and_then(|value| value.as_str()),
            Some("failed")
        );
        assert_eq!(
            body.get("recentLogs")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(5)
        );
        assert_eq!(
            body.pointer("/trend/request")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(8)
        );
        assert_eq!(
            body.pointer("/trend/error")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(8)
        );
        assert_eq!(
            body.pointer("/trend/request")
                .and_then(|value| value.as_array())
                .map(|values| values
                    .iter()
                    .filter_map(|value| value.as_i64())
                    .sum::<i64>()),
            Some(7)
        );
        assert_eq!(
            body.pointer("/trend/error")
                .and_then(|value| value.as_array())
                .map(|values| values
                    .iter()
                    .filter_map(|value| value.as_i64())
                    .sum::<i64>()),
            Some(3)
        );
        assert_eq!(
            body.get("disabledTokens")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(5)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn dashboard_overview_degrades_optional_feeds_without_failing_core_summary() {
        let db_path = temp_db_path("dashboard-overview-optional-feed-degrade");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-dashboard-overview-optional-feed-degrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        proxy
            .create_access_token(Some("disabled-feed"))
            .await
            .expect("create access token");

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

        sqlx::query("DROP TABLE scheduled_jobs")
            .execute(&pool)
            .await
            .expect("drop scheduled_jobs");
        sqlx::query("DROP TABLE auth_tokens")
            .execute(&pool)
            .await
            .expect("drop auth_tokens");

        let admin_password = "dashboard-overview-optional-feed-password";
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

        let resp = client
            .get(format!("http://{}/api/dashboard/overview", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("dashboard overview request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("dashboard overview json");
        assert!(body.get("summary").is_some(), "summary should still exist");
        assert!(
            body.get("summaryWindows").is_some(),
            "summary windows should still exist"
        );
        assert!(
            body.get("siteStatus").is_some(),
            "site status should still exist"
        );
        assert!(
            body.get("hourlyRequestWindow").is_some(),
            "hourly request window should still exist"
        );
        assert_eq!(
            body.get("recentJobs")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );
        assert_eq!(
            body.get("disabledTokens")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );
        assert_eq!(
            body.get("tokenCoverage").and_then(|value| value.as_str()),
            Some("error")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn summary_windows_returns_today_yesterday_and_month_buckets() {
        let db_path = temp_db_path("summary-windows-admin");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-summary-window-admin".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key exists")
            .id;

        let local_day_start = |value: chrono::DateTime<Local>| -> i64 {
            let naive = value
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .expect("valid local midnight");
            match Local.from_local_datetime(&naive) {
                chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
                chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
                chrono::LocalResult::None => value.with_timezone(&Utc).timestamp(),
            }
        };
        let local_previous_day_start = |value: chrono::DateTime<Local>| -> i64 {
            let previous_date = value
                .date_naive()
                .pred_opt()
                .unwrap_or_else(|| value.date_naive());
            let naive = previous_date
                .and_hms_opt(0, 0, 0)
                .expect("valid local midnight");
            match Local.from_local_datetime(&naive) {
                chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc).timestamp(),
                chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc).timestamp(),
                chrono::LocalResult::None => value.with_timezone(&Utc).timestamp(),
            }
        };
        let local_month_start = |value: chrono::DateTime<Local>| -> i64 {
            Local
                .with_ymd_and_hms(value.year(), value.month(), 1, 0, 0, 0)
                .single()
                .expect("valid start of local month")
                .with_timezone(&Utc)
                .timestamp()
        };

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
        let today_start = local_day_start(now);
        let yesterday_start = local_previous_day_start(now);
        let month_start = local_month_start(now);
        let current_utc_ts = Local::now().with_timezone(&Utc).timestamp();
        let today_window_anchor = (current_utc_ts - 5).max(today_start + 30);
        let today_log_start = (today_window_anchor - 9).max(today_start);
        let yesterday_window_anchor = today_window_anchor - 86_400;
        let yesterday_log_start = (yesterday_window_anchor - 3).max(yesterday_start);

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
            INSERT INTO api_keys (
                id,
                api_key,
                status,
                created_at,
                status_changed_at,
                deleted_at
            ) VALUES (?, ?, 'disabled', ?, ?, ?)
            "#,
        )
        .bind("summary-window-deleted")
        .bind("tvly-summary-window-deleted")
        .bind(today_start + 120)
        .bind(today_start + 120)
        .bind(today_start + 180)
        .execute(&pool)
        .await
        .expect("insert deleted key created this month");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id,
                api_key,
                status,
                created_at
            ) VALUES (?, ?, 'active', ?)
            "#,
        )
        .bind("summary-window-extra")
        .bind("tvly-summary-window-extra")
        .bind(today_start + 90)
        .execute(&pool)
        .await
        .expect("insert extra key");

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
                updated_at
            ) VALUES
                (?, ?, 86400, 10, 8, 1, 1, ?),
                (?, ?, 86400, 4, 3, 1, 0, ?)
            "#,
        )
        .bind(&key_id)
        .bind(today_start)
        .bind(today_start + 60)
        .bind(&key_id)
        .bind(yesterday_start)
        .bind(yesterday_start + 60)
        .execute(&pool)
        .await
        .expect("insert summary window buckets");

        for offset in 0..8_i64 {
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
                    created_at
                ) VALUES (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '[]', '[]', ?)
                "#,
            )
            .bind(&key_id)
            .bind(today_log_start + offset)
            .execute(&pool)
            .await
            .expect("insert today success log");
        }
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
                created_at
            ) VALUES
                (?, NULL, 'GET', '/api/tavily/search', NULL, 500, 500, 'boom', 'error', NULL, NULL, '[]', '[]', ?),
                (?, NULL, 'GET', '/api/tavily/search', NULL, 429, 429, 'quota', 'quota_exhausted', NULL, NULL, '[]', '[]', ?),
                (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '[]', '[]', ?),
                (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '[]', '[]', ?),
                (?, NULL, 'GET', '/api/tavily/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '[]', '[]', ?),
                (?, NULL, 'GET', '/api/tavily/search', NULL, 500, 500, 'boom', 'error', NULL, NULL, '[]', '[]', ?)
            "#,
        )
        .bind(&key_id)
        .bind(today_window_anchor - 1)
        .bind(&key_id)
        .bind(today_window_anchor)
        .bind(&key_id)
        .bind(yesterday_log_start)
        .bind(&key_id)
        .bind(yesterday_log_start + 1)
        .bind(&key_id)
        .bind(yesterday_log_start + 2)
        .bind(&key_id)
        .bind(yesterday_window_anchor)
        .execute(&pool)
        .await
        .expect("insert summary window logs");

        let today_minute_bucket = today_log_start.div_euclid(60) * 60;
        let yesterday_minute_bucket = yesterday_log_start.div_euclid(60) * 60;
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
                (?, 60, 10, 8, 1, 1, 8, 2, 0, 0, 0, 0, 0, 0, 0, 10, 0, ?),
                (?, 60, 4, 3, 1, 0, 3, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, ?),
                (?, 86400, 4, 3, 1, 0, 3, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, ?)
            "#,
        )
        .bind(today_minute_bucket)
        .bind(today_minute_bucket + 59)
        .bind(yesterday_minute_bucket)
        .bind(yesterday_minute_bucket + 59)
        .bind(yesterday_start)
        .bind(yesterday_start + 59)
        .execute(&pool)
        .await
        .expect("insert dashboard rollup buckets");

        sqlx::query(
            r#"
            INSERT INTO api_key_quarantines (
                id, key_id, source, reason_code, reason_summary, reason_detail, created_at, cleared_at
            ) VALUES (?, ?, 'usage', 'quota_exhausted', 'quota exhausted', 'month quarantine', ?, NULL)
            "#,
        )
        .bind("summary-window-quarantine")
        .bind(&key_id)
        .bind(today_start + 30)
        .execute(&pool)
        .await
        .expect("insert summary window quarantine");

        let month_only_upstream_ts = if month_start < yesterday_start {
            month_start + 90
        } else {
            today_window_anchor + 90
        };

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
                (?, ?, 'system', 'auto_mark_exhausted', '自动标记为 exhausted', 'quota_exhausted', '上游额度耗尽', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
                (?, ?, 'system', 'auto_mark_exhausted', '自动标记为 exhausted', 'quota_exhausted', '上游额度耗尽', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
                (?, ?, 'system', 'auto_mark_exhausted', '自动标记为 exhausted', 'quota_exhausted', '上游额度耗尽', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
                (?, ?, 'admin', 'manual_mark_exhausted', '管理员手动标记 exhausted', 'manual_mark_exhausted', '确认该 Key 额度耗尽', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?),
                (?, ?, 'system', 'auto_mark_exhausted', '自动标记为 exhausted', 'quota_exhausted', '上游额度耗尽', NULL, NULL, NULL, NULL, NULL, NULL, 'active', 'exhausted', 0, 0, ?)
            "#,
        )
        .bind("summary-window-maint-today-a")
        .bind(&key_id)
        .bind(today_window_anchor - 1)
        .bind("summary-window-maint-today-b")
        .bind(&key_id)
        .bind(today_window_anchor)
        .bind("summary-window-maint-yesterday")
        .bind(&key_id)
        .bind(yesterday_window_anchor)
        .bind("summary-window-maint-manual")
        .bind(&key_id)
        .bind(today_window_anchor + 1)
        .bind("summary-window-maint-month")
        .bind("summary-window-extra")
        .bind(month_only_upstream_ts)
        .execute(&pool)
        .await
        .expect("insert summary window maintenance records");

        let admin_password = "summary-window-admin-password";
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

        let resp = client
            .get(format!("http://{}/api/summary/windows", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("summary windows request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("summary windows json");
        let today_total = body
            .pointer("/today/total_requests")
            .and_then(|v| v.as_i64());
        let yesterday_total = body
            .pointer("/yesterday/total_requests")
            .and_then(|v| v.as_i64());
        match (today_total, yesterday_total) {
            (Some(10), Some(4)) => {}
            // The endpoint uses `Local::now()` at request time. If the test setup and the
            // request happen across a local midnight boundary, the rows we inserted for the
            // previous "today" window legitimately move into the response's "yesterday"
            // bucket instead of "today".
            (Some(0), Some(10)) => {}
            _ => panic!(
                "unexpected summary window totals: today={today_total:?} yesterday={yesterday_total:?}"
            ),
        }
        let month_expected = if month_start <= yesterday_start {
            14
        } else {
            10
        };
        assert_eq!(
            body.pointer("/month/total_requests")
                .and_then(|v| v.as_i64()),
            Some(month_expected)
        );
        assert!(
            body.pointer("/today/upstream_exhausted_key_count")
                .and_then(|v| v.as_i64())
                .is_some(),
            "today upstream exhausted key count should exist"
        );
        assert!(
            body.pointer("/yesterday/upstream_exhausted_key_count")
                .and_then(|v| v.as_i64())
                .is_some(),
            "yesterday upstream exhausted key count should exist"
        );
        assert!(
            body.pointer("/month/upstream_exhausted_key_count")
                .and_then(|v| v.as_i64())
                .is_some(),
            "month upstream exhausted key count should exist"
        );
        for pointer in [
            "/today/quota_charge/local_estimated_credits",
            "/today/quota_charge/upstream_actual_credits",
            "/today/quota_charge/sampled_key_count",
            "/today/quota_charge/stale_key_count",
            "/month/quota_charge/local_estimated_credits",
        ] {
            assert!(
                body.pointer(pointer).and_then(|v| v.as_i64()).is_some(),
                "summary windows should expose {pointer}"
            );
        }
        for pointer in [
            "/today/valuable_success_count",
            "/today/valuable_failure_count",
            "/today/other_success_count",
            "/today/other_failure_count",
            "/today/unknown_count",
            "/yesterday/valuable_success_count",
            "/month/valuable_success_count",
            "/month/unknown_count",
        ] {
            assert!(
                body.pointer(pointer).and_then(|v| v.as_i64()).is_some(),
                "summary windows should expose {pointer}"
            );
        }
        assert_eq!(
            body.pointer("/month/new_keys").and_then(|v| v.as_i64()),
            Some(3)
        );
        assert_eq!(
            body.pointer("/month/new_quarantines")
                .and_then(|v| v.as_i64()),
            Some(1)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_logs_endpoint_returns_unfiltered_and_filtered_pages() {
        let db_path = temp_db_path("admin-logs-page");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-logs-page".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key exists")
            .id;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let now = chrono::Utc::now().timestamp();
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
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES
                (?, 'token-success', 'POST', '/search', 'q=koha', 200, 200, NULL, 'success', NULL, 'none', NULL, X'7B7D', X'5B5D', '[]', '[]', ?),
                (?, 'token-error', 'POST', '/search', 'q=bug', 401, 401, 'account deactivated', 'error', 'upstream_account_deactivated_401', 'quarantined', 'The system automatically quarantined this key', X'7B7D', X'5B5D', '[\"x-forwarded-for\"]', '[\"authorization\"]', ?),
                (?, 'token-search-initialize', 'POST', '/mcp', NULL, 200, 200, NULL, 'success', NULL, 'none', NULL, ?, X'5B5D', '[]', '[]', ?),
                (?, 'token-batch-mixed', 'POST', '/mcp', NULL, 200, 200, NULL, 'success', NULL, 'none', NULL, ?, X'5B5D', '[]', '[]', ?)
            "#,
        )
        .bind(&key_id)
        .bind(now - 200)
        .bind(&key_id)
        .bind(now - 150)
        .bind(&key_id)
        .bind(br#"{"jsonrpc":"2.0","id":"search-like-control-plane","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"how to initialize rust logging","search_depth":"basic"}}}"#.as_slice())
        .bind(now - 100)
        .bind(&key_id)
        .bind(br#"[{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":"mixed-batch-search","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"mixed batch should stay billable","search_depth":"basic"}}}]"#.as_slice())
        .bind(now - 50)
        .execute(&pool)
        .await
        .expect("insert request logs");
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
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, 'token-neutral', 'POST', '/mcp', NULL, 202, NULL, NULL, 'unknown', NULL, 'none', NULL, ?, X'5B5D', '[]', '[]', ?)
            "#,
        )
        .bind(&key_id)
        .bind(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.as_slice())
        .bind(now - 25)
        .execute(&pool)
        .await
        .expect("insert neutral request log");
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
                request_kind_detail,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, 'token-session-delete', 'DELETE', '/mcp', NULL, 405, 405, 'Method Not Allowed: Session termination not supported', 'error', 'mcp:session-delete-unsupported', 'MCP | session delete unsupported', NULL, NULL, 'mcp_method_405', 'none', NULL, X'7B7D', ?, '[]', '[]', ?)
            "#,
        )
        .bind(&key_id)
        .bind(
            br#"{"error":"Method Not Allowed","message":"Method Not Allowed: Session termination not supported"}"#.as_slice(),
        )
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert session delete neutral request log");

        let admin_password = "admin-logs-page-password";
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

        let unfiltered_resp = client
            .get(format!("http://{}/api/logs?page=1&per_page=20", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("unfiltered admin logs request");
        assert_eq!(unfiltered_resp.status(), reqwest::StatusCode::OK);
        let unfiltered_body: serde_json::Value = unfiltered_resp
            .json()
            .await
            .expect("unfiltered admin logs json");
        assert_eq!(
            unfiltered_body
                .get("total")
                .and_then(|value| value.as_i64()),
            Some(6)
        );
        let unfiltered_items = unfiltered_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("unfiltered admin log items");
        assert_eq!(
            unfiltered_body
                .pointer("/items/0/failure_kind")
                .and_then(|value| value.as_str()),
            Some("mcp_method_405")
        );
        let unfiltered_error_log = unfiltered_items
            .iter()
            .find(|item| {
                item.get("auth_token_id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == "token-error")
            })
            .expect("unfiltered error log");
        assert_eq!(
            unfiltered_error_log
                .get("failure_kind")
                .and_then(|value| value.as_str()),
            Some("upstream_account_deactivated_401")
        );
        assert_eq!(
            unfiltered_error_log
                .get("key_effect_code")
                .and_then(|value| value.as_str()),
            Some("quarantined")
        );
        assert_eq!(
            unfiltered_error_log
                .get("key_effect_summary")
                .and_then(|value| value.as_str()),
            Some("The system automatically quarantined this key")
        );
        assert_eq!(
            unfiltered_body
                .pointer("/items/0/operationalClass")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );
        assert_eq!(
            unfiltered_body
                .pointer("/items/0/requestKindProtocolGroup")
                .and_then(|value| value.as_str()),
            Some("mcp")
        );
        assert_eq!(
            unfiltered_body
                .pointer("/items/0/requestKindBillingGroup")
                .and_then(|value| value.as_str()),
            Some("non_billable")
        );
        assert_eq!(
            unfiltered_body
                .pointer("/items/0/result_status")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );

        let success_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&result=success",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("filtered admin logs request");
        assert_eq!(success_resp.status(), reqwest::StatusCode::OK);
        let success_body: serde_json::Value =
            success_resp.json().await.expect("filtered admin logs json");
        assert_eq!(
            success_body.get("total").and_then(|value| value.as_i64()),
            Some(3)
        );
        assert_eq!(
            success_body
                .pointer("/items/0/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-batch-mixed")
        );
        assert_eq!(
            success_body
                .pointer("/items/1/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-search-initialize")
        );
        assert_eq!(
            success_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.get(1))
                .and_then(|item| item.get("key_effect_code"))
                .and_then(|value| value.as_str()),
            Some("none")
        );
        assert!(
            success_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.get(1))
                .and_then(|item| item.get("failure_kind"))
                .is_some_and(|value| value.is_null()),
            "success log should expose a null failure_kind field"
        );
        assert_eq!(
            success_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.get(1))
                .and_then(|item| item.get("operationalClass"))
                .and_then(|value| value.as_str()),
            Some("success")
        );
        assert_eq!(
            success_body
                .pointer("/items/2/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-success")
        );

        let neutral_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&operational_class=neutral",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("neutral admin logs request");
        assert_eq!(neutral_resp.status(), reqwest::StatusCode::OK);
        let neutral_body: serde_json::Value =
            neutral_resp.json().await.expect("neutral admin logs json");
        assert_eq!(
            neutral_body.get("total").and_then(|value| value.as_i64()),
            Some(2)
        );
        assert_eq!(
            neutral_body
                .pointer("/items/0/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-session-delete")
        );
        assert_eq!(
            neutral_body
                .pointer("/items/0/request_kind_key")
                .and_then(|value| value.as_str()),
            Some("mcp:session-delete-unsupported")
        );
        assert_eq!(
            neutral_body
                .pointer("/items/0/result_status")
                .and_then(|value| value.as_str()),
            Some("neutral")
        );
        assert!(
            neutral_body
                .get("items")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().all(|item| {
                    item.get("auth_token_id").and_then(|value| value.as_str())
                        != Some("token-search-initialize")
                })),
            "billable MCP search rows must not leak into the neutral filter"
        );
        assert!(
            neutral_body
                .get("items")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().all(|item| {
                    item.get("auth_token_id").and_then(|value| value.as_str())
                        != Some("token-batch-mixed")
                })),
            "mixed MCP batches must not leak into the neutral filter"
        );

        let neutral_result_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&result=neutral",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("neutral result admin logs request");
        assert_eq!(neutral_result_resp.status(), reqwest::StatusCode::OK);
        let neutral_result_body: serde_json::Value = neutral_result_resp
            .json()
            .await
            .expect("neutral result admin logs json");
        assert_eq!(
            neutral_result_body
                .get("total")
                .and_then(|value| value.as_i64()),
            Some(2)
        );
        assert!(
            neutral_result_body
                .get("items")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().any(|item| {
                    item.get("request_kind_key")
                        .and_then(|value| value.as_str())
                        == Some("mcp:session-delete-unsupported")
                }))
        );
        assert!(
            neutral_result_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| {
                    items.iter().find(|item| {
                        item.get("request_kind_key")
                            .and_then(|value| value.as_str())
                            == Some("mcp:session-delete-unsupported")
                    })
                })
                .and_then(|item| item.get("result_status").and_then(|value| value.as_str()))
                == Some("neutral")
        );

        let error_result_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&result=error",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("error result admin logs request");
        assert_eq!(error_result_resp.status(), reqwest::StatusCode::OK);
        let error_result_body: serde_json::Value = error_result_resp
            .json()
            .await
            .expect("error result admin logs json");
        assert!(
            error_result_body
                .get("items")
                .and_then(|value| value.as_array())
                .is_some_and(|items| items.iter().all(|item| {
                    item.get("request_kind_key")
                        .and_then(|value| value.as_str())
                        != Some("mcp:session-delete-unsupported")
                }))
        );

        let key_neutral_logs_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/page?page=1&per_page=20&result=neutral",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("key neutral logs request");
        assert_eq!(key_neutral_logs_resp.status(), reqwest::StatusCode::OK);
        let key_neutral_logs_body: serde_json::Value = key_neutral_logs_resp
            .json()
            .await
            .expect("key neutral logs json");
        assert!(
            key_neutral_logs_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| {
                    items.iter().find(|item| {
                        item.get("request_kind_key")
                            .and_then(|value| value.as_str())
                            == Some("mcp:session-delete-unsupported")
                    })
                })
                .and_then(|item| item.get("result_status").and_then(|value| value.as_str()))
                == Some("neutral")
        );

        let upstream_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&operational_class=upstream_error",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("upstream admin logs request");
        assert_eq!(upstream_resp.status(), reqwest::StatusCode::OK);
        let upstream_body: serde_json::Value = upstream_resp
            .json()
            .await
            .expect("upstream admin logs json");
        assert_eq!(
            upstream_body.get("total").and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            upstream_body
                .pointer("/items/0/operationalClass")
                .and_then(|value| value.as_str()),
            Some("upstream_error")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_logs_cursor_and_catalog_endpoints_expose_retention_without_blocking_page_counts()
    {
        let db_path = temp_db_path("admin-logs-cursor-catalog");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-logs-cursor-catalog".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let key_id = proxy
            .list_api_key_metrics()
            .await
            .expect("list api key metrics")
            .into_iter()
            .next()
            .expect("seeded key exists")
            .id;

        let pool = connect_sqlite_test_pool(&db_str).await;
        let now = chrono::Utc::now().timestamp();
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
            ) VALUES
                (?, 'token-a', 'POST', '/api/tavily/search', 'q=a', 200, 200, NULL, 'success', 'api:search', 'API | search', NULL, 2, NULL, 'none', NULL, X'7B7D', X'5B5D', '[]', '[]', 'visible', ?),
                (?, 'token-b', 'POST', '/api/tavily/search', 'q=b', 500, 500, 'boom', 'error', 'api:search', 'API | search', NULL, NULL, 'upstream_500', 'quarantined', 'The system automatically quarantined this key', X'7B7D', X'5B5D', '[]', '[]', 'visible', ?),
                (?, 'token-c', 'POST', '/api/tavily/extract', 'q=c', 200, 200, NULL, 'success', 'api:extract', 'API | extract', NULL, 3, NULL, 'none', NULL, X'7B7D', X'5B5D', '[]', '[]', 'visible', ?)
            "#,
        )
        .bind(&key_id)
        .bind(now - 2)
        .bind(&key_id)
        .bind(now - 1)
        .bind(&key_id)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert request logs");

        let proxy_page = proxy
            .request_logs_list(
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                RequestLogsCursorDirection::Older,
                20,
            )
            .await
            .expect("proxy cursor logs");
        assert!(
            proxy_page
                .items
                .iter()
                .all(|item| item.request_body.is_empty() && item.response_body.is_empty()),
            "cursor list records should not carry request/response blobs"
        );

        let admin_password = "admin-logs-cursor-catalog-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let (client, admin_cookie) = login_builtin_admin_cookie(admin_addr, admin_password).await;

        let catalog_resp = client
            .get(format!("http://{}/api/logs/catalog", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("request logs catalog");
        assert_eq!(catalog_resp.status(), reqwest::StatusCode::OK);
        let catalog_body: serde_json::Value = catalog_resp.json().await.expect("catalog json");
        assert_eq!(
            catalog_body
                .get("retentionDays")
                .and_then(|value| value.as_i64()),
            Some(effective_request_logs_retention_days())
        );
        assert!(
            catalog_body
                .pointer("/requestKindOptions/0/key")
                .and_then(|value| value.as_str())
                .is_some(),
            "catalog should expose request kind options"
        );
        let filtered_catalog_resp = client
            .get(format!(
                "http://{}/api/logs/catalog?request_kind=api:extract&result=success",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("filtered request logs catalog");
        assert_eq!(filtered_catalog_resp.status(), reqwest::StatusCode::OK);
        let filtered_catalog: serde_json::Value = filtered_catalog_resp
            .json()
            .await
            .expect("filtered catalog json");
        assert_eq!(
            filtered_catalog
                .pointer("/requestKindOptions/0/key")
                .and_then(|value| value.as_str()),
            Some("api:extract")
        );
        assert_eq!(
            filtered_catalog
                .pointer("/facets/results/0/value")
                .and_then(|value| value.as_str()),
            Some("success")
        );
        assert_eq!(
            filtered_catalog
                .pointer("/facets/tokens/0/value")
                .and_then(|value| value.as_str()),
            Some("token-c")
        );
        let key_list_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/list?limit=5&since=0&operational_class=success",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("key logs list");
        assert_eq!(key_list_resp.status(), reqwest::StatusCode::OK);
        let key_list_body: serde_json::Value =
            key_list_resp.json().await.expect("key logs list json");
        assert_eq!(
            key_list_body
                .get("items")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert_eq!(
            key_list_body
                .pointer("/items/0/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-c")
        );
        assert_eq!(
            key_list_body
                .pointer("/items/1/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-a")
        );
        let invalid_key_list_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/list?limit=5&since=0&operational_class=not-real",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("invalid key logs list");
        assert_eq!(
            invalid_key_list_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );
        let invalid_catalog_resp = client
            .get(format!(
                "http://{}/api/logs/catalog?operational_class=definitely-not-valid",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("invalid request logs catalog");
        assert_eq!(
            invalid_catalog_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        let page_resp = client
            .get(format!("http://{}/api/logs/list?limit=2", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("request logs list");
        assert_eq!(page_resp.status(), reqwest::StatusCode::OK);
        let page_body: serde_json::Value = page_resp.json().await.expect("page json");
        assert!(
            page_body.get("total").is_none(),
            "cursor endpoint should not expose total counts"
        );
        assert_eq!(
            page_body.get("pageSize").and_then(|value| value.as_i64()),
            Some(2)
        );
        assert_eq!(
            page_body
                .pointer("/items/0/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-c")
        );
        assert_eq!(
            page_body
                .pointer("/items/1/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-b")
        );
        assert_eq!(
            page_body.get("hasOlder").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            page_body.get("hasNewer").and_then(|value| value.as_bool()),
            Some(false)
        );
        let next_cursor = page_body
            .get("nextCursor")
            .and_then(|value| value.as_str())
            .expect("next cursor");

        let older_resp = client
            .get(format!(
                "http://{}/api/logs/list?limit=2&cursor={}&direction=older",
                admin_addr, next_cursor
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("older request logs list");
        assert_eq!(older_resp.status(), reqwest::StatusCode::OK);
        let older_body: serde_json::Value = older_resp.json().await.expect("older page json");
        assert_eq!(
            older_body
                .pointer("/items/0/auth_token_id")
                .and_then(|value| value.as_str()),
            Some("token-a")
        );
        assert_eq!(
            older_body.get("hasOlder").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            older_body.get("hasNewer").and_then(|value| value.as_bool()),
            Some(true)
        );

        sqlx::query("DELETE FROM request_logs WHERE auth_token_id = 'token-a'")
            .execute(&pool)
            .await
            .expect("delete oldest request log");

        let recovery_resp = client
            .get(format!(
                "http://{}/api/logs/list?limit=2&cursor={}&direction=older",
                admin_addr, next_cursor
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("request logs recovery page");
        assert_eq!(recovery_resp.status(), reqwest::StatusCode::OK);
        let recovery_body: serde_json::Value =
            recovery_resp.json().await.expect("recovery page json");
        assert_eq!(
            recovery_body
                .get("items")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );
        assert_eq!(
            recovery_body
                .get("hasOlder")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            recovery_body
                .get("hasNewer")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            recovery_body
                .get("prevCursor")
                .and_then(|value| value.as_str()),
            Some(next_cursor)
        );

        let _ = std::fs::remove_file(db_path);
    }

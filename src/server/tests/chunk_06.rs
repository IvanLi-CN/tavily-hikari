    #[tokio::test]
    async fn key_and_token_logs_catalog_scope_and_cache_invalidation_work() {
        let db_path = temp_db_path("key-token-logs-catalog");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-key-token-logs-catalog";
        let (upstream_addr, _hits) =
            spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
        let upstream = format!("http://{upstream_addr}");

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let token = proxy
            .create_access_token(Some("catalog-scope"))
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

        let usage_base = format!("http://{}", upstream_addr);
        let proxy_addr = spawn_proxy_server(proxy.clone(), usage_base).await;
        let admin_password = "key-token-logs-catalog-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let (client, admin_cookie) = login_builtin_admin_cookie(admin_addr, admin_password).await;

        let since = Utc::now() - ChronoDuration::minutes(5);
        let until = Utc::now() + ChronoDuration::minutes(5);
        let token_since = since.to_rfc3339();
        let token_until = until.to_rfc3339();
        let pool = connect_sqlite_test_pool(&db_str).await;

        let empty_key_catalog_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/catalog?since=0",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("empty key catalog");
        assert_eq!(empty_key_catalog_resp.status(), reqwest::StatusCode::OK);
        let empty_key_catalog: serde_json::Value = empty_key_catalog_resp
            .json()
            .await
            .expect("empty key catalog json");
        assert_eq!(
            empty_key_catalog
                .pointer("/facets/tokens")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );

        let empty_token_catalog_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/catalog?since={}&until={}",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until)
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("empty token catalog");
        assert_eq!(empty_token_catalog_resp.status(), reqwest::StatusCode::OK);
        let empty_token_catalog: serde_json::Value = empty_token_catalog_resp
            .json()
            .await
            .expect("empty token catalog json");
        assert_eq!(
            empty_token_catalog
                .get("retentionDays")
                .and_then(|value| value.as_i64()),
            Some(effective_auth_token_log_retention_days())
        );
        assert_eq!(
            empty_token_catalog
                .pointer("/facets/keys")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );

        let search_resp = client
            .post(format!("http://{}/api/tavily/search", proxy_addr))
            .header("Authorization", format!("Bearer {}", token.token))
            .json(&serde_json::json!({ "query": "catalog invalidation" }))
            .send()
            .await
            .expect("search request");
        assert_eq!(search_resp.status(), reqwest::StatusCode::OK);

        let key_catalog_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/catalog?since=0",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("key catalog");
        assert_eq!(key_catalog_resp.status(), reqwest::StatusCode::OK);
        let key_catalog: serde_json::Value =
            key_catalog_resp.json().await.expect("key catalog json");
        assert_eq!(
            key_catalog
                .get("retentionDays")
                .and_then(|value| value.as_i64()),
            Some(effective_request_logs_retention_days())
        );
        assert_eq!(
            key_catalog
                .pointer("/facets/tokens/0/value")
                .and_then(|value| value.as_str()),
            Some(token.id.as_str())
        );
        let filtered_key_catalog_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/catalog?since=0&request_kind=api:search&result=success",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("filtered key catalog");
        assert_eq!(filtered_key_catalog_resp.status(), reqwest::StatusCode::OK);
        let filtered_key_catalog: serde_json::Value = filtered_key_catalog_resp
            .json()
            .await
            .expect("filtered key catalog json");
        assert_eq!(
            filtered_key_catalog
                .pointer("/requestKindOptions/0/key")
                .and_then(|value| value.as_str()),
            Some("api:search")
        );
        let invalid_key_catalog_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/catalog?since=0&operational_class=totally-invalid",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("invalid key catalog");
        assert_eq!(
            invalid_key_catalog_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        let token_catalog_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/catalog?since={}&until={}",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until)
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("token catalog");
        assert_eq!(token_catalog_resp.status(), reqwest::StatusCode::OK);
        let token_catalog: serde_json::Value =
            token_catalog_resp.json().await.expect("token catalog json");
        assert_eq!(
            token_catalog
                .pointer("/facets/keys/0/value")
                .and_then(|value| value.as_str()),
            Some(key_id.as_str())
        );
        assert!(
            token_catalog
                .pointer("/requestKindOptions/0/key")
                .and_then(|value| value.as_str())
                .is_some(),
            "token catalog should refresh after a new token log is written"
        );
        let filtered_token_catalog_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/catalog?since={}&until={}&request_kind=api:search&result=success&key_id={}",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until),
                key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("filtered token catalog");
        assert_eq!(
            filtered_token_catalog_resp.status(),
            reqwest::StatusCode::OK
        );
        let filtered_token_catalog: serde_json::Value = filtered_token_catalog_resp
            .json()
            .await
            .expect("filtered token catalog json");
        assert_eq!(
            filtered_token_catalog
                .pointer("/requestKindOptions/0/key")
                .and_then(|value| value.as_str()),
            Some("api:search")
        );
        assert_eq!(
            filtered_token_catalog
                .pointer("/facets/keys/0/value")
                .and_then(|value| value.as_str()),
            Some(key_id.as_str())
        );
        let invalid_token_catalog_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/catalog?since={}&until={}&operational_class=not-real",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until)
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("invalid token catalog");
        assert_eq!(
            invalid_token_catalog_resp.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

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
                counts_business_quota,
                business_credits,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES
                (?, 'POST', '/mcp', 'seed=a', 200, 200, 'mcp:search', 'MCP | search', NULL, 'success', NULL, NULL, 'none', NULL, 1, 1, 'charged', ?, NULL, ?),
                (?, 'POST', '/mcp', 'seed=b', 200, 200, 'mcp:search', 'MCP | search', NULL, 'success', NULL, NULL, 'none', NULL, 1, 1, 'charged', ?, NULL, ?),
                (?, 'POST', '/mcp', 'seed=c', 200, 200, 'mcp:search', 'MCP | search', NULL, 'success', NULL, NULL, 'none', NULL, 1, 1, 'charged', ?, NULL, ?)
            "#,
        )
        .bind(&token.id)
        .bind(&key_id)
        .bind(since.timestamp() + 1)
        .bind(&token.id)
        .bind(&key_id)
        .bind(since.timestamp() + 2)
        .bind(&token.id)
        .bind(&key_id)
        .bind(since.timestamp() + 3)
        .execute(&pool)
        .await
        .expect("seed token logs");

        let token_cursor_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/list?limit=2&since={}&until={}&request_kind=mcp:search",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until)
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("token cursor list");
        assert_eq!(token_cursor_resp.status(), reqwest::StatusCode::OK);
        let token_cursor_body: serde_json::Value = token_cursor_resp
            .json()
            .await
            .expect("token cursor list json");
        let token_next_cursor = token_cursor_body
            .get("nextCursor")
            .and_then(|value| value.as_str())
            .expect("token next cursor")
            .to_string();

        sqlx::query("DELETE FROM auth_token_logs WHERE token_id = ? AND query = 'seed=a'")
            .bind(&token.id)
            .execute(&pool)
            .await
            .expect("delete oldest token log");

        let token_recovery_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/list?limit=2&since={}&until={}&request_kind=mcp:search&cursor={}&direction=older",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until),
                token_next_cursor
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("token recovery list");
        assert_eq!(token_recovery_resp.status(), reqwest::StatusCode::OK);
        let token_recovery_body: serde_json::Value = token_recovery_resp
            .json()
            .await
            .expect("token recovery list json");
        assert_eq!(
            token_recovery_body
                .get("items")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );
        assert_eq!(
            token_recovery_body
                .get("hasOlder")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            token_recovery_body
                .get("hasNewer")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            token_recovery_body
                .get("prevCursor")
                .and_then(|value| value.as_str()),
            Some(token_next_cursor.as_str())
        );

        let token_list_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/list?limit=1&since={}&until={}",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until)
            ))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("token list");
        assert_eq!(token_list_resp.status(), reqwest::StatusCode::OK);
        let token_list: serde_json::Value = token_list_resp.json().await.expect("token list json");
        assert!(
            token_list.get("total").is_none(),
            "token cursor endpoint should not expose total counts"
        );
        assert_eq!(
            token_list
                .pointer("/items/0/auth_token_id")
                .and_then(|value| value.as_str()),
            Some(token.id.as_str())
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn log_endpoints_allow_combining_binding_and_selection_filters() {
        let db_path = temp_db_path("compound-effect-filters");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-compound-effect-filters";
        let (upstream_addr, _hits) =
            spawn_http_search_mock_with_usage(expected_api_key.to_string()).await;
        let upstream = format!("http://{upstream_addr}");

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let token = proxy
            .create_access_token(Some("compound-effect-filters"))
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

        let admin_password = "compound-effect-filters-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let (client, admin_cookie) = login_builtin_admin_cookie(admin_addr, admin_password).await;
        let pool = connect_sqlite_test_pool(&db_str).await;

        let binding_effect = "http_project_affinity_rebound";
        let selection_effect = "http_project_affinity_cooldown_avoided";
        let created_at = Utc::now().timestamp();
        let since = Utc::now() - ChronoDuration::minutes(5);
        let until = Utc::now() + ChronoDuration::minutes(5);
        let token_since = since.to_rfc3339();
        let token_until = until.to_rfc3339();

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
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
            ) VALUES (?, ?, 'POST', '/api/tavily/search', 'q=compound', 200, 200, NULL, 'success', 'api:search', 'API | search', 'none', ?, ?, X'7B7D', X'5B5D', '[]', '[]', 'visible', ?)
            RETURNING id
            "#,
        )
        .bind(&key_id)
        .bind(&token.id)
        .bind(binding_effect)
        .bind(selection_effect)
        .bind(created_at)
        .fetch_one(&pool)
        .await
        .expect("insert request log");

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
                result_status,
                key_effect_code,
                binding_effect_code,
                selection_effect_code,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', 'q=compound', 200, 200, 'api:search', 'API | search', 'success', 'none', ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&token.id)
        .bind(binding_effect)
        .bind(selection_effect)
        .bind(&key_id)
        .bind(request_log_id)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert token log");

        let compound_query =
            format!("binding_effect={binding_effect}&selection_effect={selection_effect}");

        let admin_list_resp = client
            .get(format!(
                "http://{}/api/logs/list?limit=5&{}",
                admin_addr, compound_query
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("compound admin list");
        assert_eq!(admin_list_resp.status(), reqwest::StatusCode::OK);
        let admin_list: serde_json::Value = admin_list_resp.json().await.expect("admin list json");
        assert_eq!(
            admin_list
                .pointer("/items/0/binding_effect_code")
                .and_then(|value| value.as_str()),
            Some(binding_effect)
        );
        assert_eq!(
            admin_list
                .pointer("/items/0/selection_effect_code")
                .and_then(|value| value.as_str()),
            Some(selection_effect)
        );

        let admin_catalog_resp = client
            .get(format!(
                "http://{}/api/logs/catalog?{}",
                admin_addr, compound_query
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("compound admin catalog");
        assert_eq!(admin_catalog_resp.status(), reqwest::StatusCode::OK);
        let admin_catalog: serde_json::Value =
            admin_catalog_resp.json().await.expect("admin catalog json");
        assert_eq!(
            admin_catalog
                .pointer("/facets/bindingEffects/0/value")
                .and_then(|value| value.as_str()),
            Some(binding_effect)
        );
        assert_eq!(
            admin_catalog
                .pointer("/facets/selectionEffects/0/value")
                .and_then(|value| value.as_str()),
            Some(selection_effect)
        );

        let key_list_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/list?limit=5&since=0&{}",
                admin_addr, key_id, compound_query
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("compound key list");
        assert_eq!(key_list_resp.status(), reqwest::StatusCode::OK);
        let key_list: serde_json::Value = key_list_resp.json().await.expect("key list json");
        assert_eq!(
            key_list
                .pointer("/items/0/binding_effect_code")
                .and_then(|value| value.as_str()),
            Some(binding_effect)
        );

        let key_catalog_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/catalog?since=0&{}",
                admin_addr, key_id, compound_query
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("compound key catalog");
        assert_eq!(key_catalog_resp.status(), reqwest::StatusCode::OK);

        let token_list_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/list?limit=5&since={}&until={}&{}",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until),
                compound_query
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("compound token list");
        assert_eq!(token_list_resp.status(), reqwest::StatusCode::OK);
        let token_list: serde_json::Value = token_list_resp.json().await.expect("token list json");
        assert_eq!(
            token_list
                .pointer("/items/0/binding_effect_code")
                .and_then(|value| value.as_str()),
            Some(binding_effect)
        );
        assert_eq!(
            token_list
                .pointer("/items/0/selection_effect_code")
                .and_then(|value| value.as_str()),
            Some(selection_effect)
        );

        let token_catalog_resp = client
            .get(format!(
                "http://{}/api/tokens/{}/logs/catalog?since={}&until={}&{}",
                admin_addr,
                token.id,
                urlencoding::encode(&token_since),
                urlencoding::encode(&token_until),
                compound_query
            ))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("compound token catalog");
        assert_eq!(token_catalog_resp.status(), reqwest::StatusCode::OK);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_and_key_log_details_return_scoped_bodies_while_list_pages_keep_null_payloads() {
        let db_path = temp_db_path("admin-key-log-details");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-key-log-details".to_string()],
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
            ) VALUES (?, 'tok-admin-key-detail', 'POST', '/api/tavily/search', NULL, 200, 200, NULL, 'success', 'api:search', 'API | search', NULL, 2, NULL, 'none', NULL, ?, ?, '["x-request-id"]', '["authorization"]', 'visible', ?)
            RETURNING id
            "#,
        )
        .bind(&key_id)
        .bind(br#"{"query":"incident review"}"#.to_vec())
        .bind(br#"{"answer":"stable"}"#.to_vec())
        .bind(1_000_i64)
        .fetch_one(&pool)
        .await
        .expect("insert request log");

        let admin_password = "admin-key-log-details-password";
        let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
        let (client, admin_cookie) = login_builtin_admin_cookie(admin_addr, admin_password).await;

        let logs_resp = client
            .get(format!("http://{}/api/logs?page=1&per_page=20", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("fetch admin logs page");
        assert_eq!(logs_resp.status(), reqwest::StatusCode::OK);
        let logs_body: serde_json::Value = logs_resp.json().await.expect("admin logs json");
        let inserted_admin_log = logs_body
            .get("items")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("id")
                        .and_then(|value| value.as_i64())
                        .is_some_and(|value| value == request_log_id)
                })
            })
            .expect("inserted admin log");
        assert!(
            inserted_admin_log
                .get("request_body")
                .is_some_and(|value| value.is_null())
        );
        assert!(
            inserted_admin_log
                .get("response_body")
                .is_some_and(|value| value.is_null())
        );

        let logs_with_bodies_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&include_bodies=true",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("fetch admin logs page with bodies");
        assert_eq!(logs_with_bodies_resp.status(), reqwest::StatusCode::OK);
        let logs_with_bodies: serde_json::Value = logs_with_bodies_resp
            .json()
            .await
            .expect("admin logs with bodies json");
        let inserted_admin_log_with_bodies = logs_with_bodies
            .get("items")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("id")
                        .and_then(|value| value.as_i64())
                        .is_some_and(|value| value == request_log_id)
                })
            })
            .expect("inserted admin log with bodies");
        assert_eq!(
            inserted_admin_log_with_bodies
                .get("request_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"query":"incident review"}"#)
        );
        assert_eq!(
            inserted_admin_log_with_bodies
                .get("response_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"answer":"stable"}"#)
        );

        let log_detail_resp = client
            .get(format!(
                "http://{}/api/logs/{}/details",
                admin_addr, request_log_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("fetch admin log detail");
        assert_eq!(log_detail_resp.status(), reqwest::StatusCode::OK);
        let log_detail_body: serde_json::Value =
            log_detail_resp.json().await.expect("admin log detail json");
        assert_eq!(
            log_detail_body
                .get("request_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"query":"incident review"}"#)
        );
        assert_eq!(
            log_detail_body
                .get("response_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"answer":"stable"}"#)
        );

        let key_logs_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/page?page=1&per_page=20",
                admin_addr, key_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("fetch key logs page");
        assert_eq!(key_logs_resp.status(), reqwest::StatusCode::OK);
        let key_logs_body: serde_json::Value = key_logs_resp.json().await.expect("key logs json");
        let inserted_key_log = key_logs_body
            .get("items")
            .and_then(|value| value.as_array())
            .and_then(|items| {
                items.iter().find(|item| {
                    item.get("id")
                        .and_then(|value| value.as_i64())
                        .is_some_and(|value| value == request_log_id)
                })
            })
            .expect("inserted key log");
        assert!(
            inserted_key_log
                .get("request_body")
                .is_some_and(|value| value.is_null())
        );
        assert!(
            inserted_key_log
                .get("response_body")
                .is_some_and(|value| value.is_null())
        );

        let key_log_detail_resp = client
            .get(format!(
                "http://{}/api/keys/{}/logs/{}/details",
                admin_addr, key_id, request_log_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("fetch key log detail");
        assert_eq!(key_log_detail_resp.status(), reqwest::StatusCode::OK);
        let key_log_detail_body: serde_json::Value = key_log_detail_resp
            .json()
            .await
            .expect("key log detail json");
        assert_eq!(
            key_log_detail_body
                .get("request_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"query":"incident review"}"#)
        );
        assert_eq!(
            key_log_detail_body
                .get("response_body")
                .and_then(|value| value.as_str()),
            Some(r#"{"answer":"stable"}"#)
        );

        let wrong_scope_resp = client
            .get(format!(
                "http://{}/api/keys/wrong-scope/logs/{}/details",
                admin_addr, request_log_id
            ))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("fetch wrong-scope key log detail");
        assert_eq!(wrong_scope_resp.status(), reqwest::StatusCode::NOT_FOUND);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_logs_endpoint_uses_canonical_request_kind_for_filters_and_view_metadata() {
        let db_path = temp_db_path("admin-logs-canonical-request-kind");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-logs-canonical-request-kind".to_string()],
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
                request_kind_key,
                request_kind_label,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES
                (?, 'token-backfilled-billable', 'POST', '/mcp', NULL, 200, 200, NULL, 'success', NULL, 'none', NULL, 'mcp:search', 'MCP | search', X'6E6F742D6A736F6E', X'5B5D', '[]', '[]', ?),
                (?, 'token-backfilled-neutral', 'POST', '/mcp', NULL, 200, 200, NULL, 'success', NULL, 'none', NULL, 'mcp:notifications/initialized', 'MCP | notifications/initialized', X'6E6F742D6A736F6E', X'5B5D', '[]', '[]', ?)
            "#,
        )
        .bind(&key_id)
        .bind(100_i64)
        .bind(&key_id)
        .bind(200_i64)
        .execute(&pool)
        .await
        .expect("insert canonical request log rows");

        let admin_password = "admin-logs-canonical-password";
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

        let success_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&operational_class=success",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .send()
            .await
            .expect("success admin logs request");
        assert_eq!(success_resp.status(), reqwest::StatusCode::OK);
        let success_body: serde_json::Value =
            success_resp.json().await.expect("success admin logs json");
        let success_items = success_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("success admin log items");
        let billable_log = success_items
            .iter()
            .find(|item| {
                item.get("auth_token_id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == "token-backfilled-billable")
            })
            .expect("billable canonical request log");
        assert_eq!(
            billable_log
                .get("operationalClass")
                .and_then(|value| value.as_str()),
            Some("success")
        );
        assert_eq!(
            billable_log
                .get("requestKindBillingGroup")
                .and_then(|value| value.as_str()),
            Some("billable")
        );
        assert!(
            success_items.iter().all(|item| {
                item.get("auth_token_id").and_then(|value| value.as_str())
                    != Some("token-backfilled-neutral")
            }),
            "neutral canonical rows must not leak into the success filter"
        );

        let neutral_resp = client
            .get(format!(
                "http://{}/api/logs?page=1&per_page=20&operational_class=neutral",
                admin_addr
            ))
            .header(reqwest::header::COOKIE, admin_cookie)
            .send()
            .await
            .expect("neutral admin logs request");
        assert_eq!(neutral_resp.status(), reqwest::StatusCode::OK);
        let neutral_body: serde_json::Value =
            neutral_resp.json().await.expect("neutral admin logs json");
        let neutral_items = neutral_body
            .get("items")
            .and_then(|value| value.as_array())
            .expect("neutral admin log items");
        let neutral_log = neutral_items
            .iter()
            .find(|item| {
                item.get("auth_token_id")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == "token-backfilled-neutral")
            })
            .expect("neutral canonical request log");
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
        assert!(
            neutral_items.iter().all(|item| {
                item.get("auth_token_id").and_then(|value| value.as_str())
                    != Some("token-backfilled-billable")
            }),
            "billable canonical rows must not leak into the neutral filter"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_schema_upgrade_backfills_created_at_best_effort() {
        let db_path = temp_db_path("api-keys-created-at-upgrade");
        let db_str = db_path.to_string_lossy().to_string();

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
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy api_keys");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, status_changed_at, last_used_at, quota_limit, quota_remaining, quota_synced_at, deleted_at
            ) VALUES (?, ?, 'active', ?, ?, NULL, NULL, ?, NULL)
            "#,
        )
        .bind("k123")
        .bind("tvly-created-at-legacy")
        .bind(360_i64)
        .bind(420_i64)
        .bind(540_i64)
        .execute(&pool)
        .await
        .expect("insert legacy api key");

        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT NOT NULL,
                auth_token_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                request_body BLOB,
                response_body BLOB,
                forwarded_headers TEXT,
                dropped_headers TEXT,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy request_logs");

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
                error_message, result_status, request_body, response_body, forwarded_headers,
                dropped_headers, created_at
            ) VALUES (?, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
            "#,
        )
        .bind("k123")
        .bind(180_i64)
        .execute(&pool)
        .await
        .expect("insert legacy request log");
        drop(pool);

        let _proxy = TavilyProxy::with_endpoint(
            vec!["tvly-created-at-upgrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("upgrade proxy");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let upgraded_pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open upgraded db pool");

        let created_at =
            sqlx::query_scalar::<_, i64>("SELECT created_at FROM api_keys WHERE id = ? LIMIT 1")
                .bind("k123")
                .fetch_one(&upgraded_pool)
                .await
                .expect("read created_at");

        assert_eq!(created_at, 180);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_dashboard_sse_snapshot_includes_overview_segments() {
        let db_path = temp_db_path("admin-dashboard-snapshot-overview");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-dashboard-overview".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");

        let admin_password = "admin-dashboard-overview-password";
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

        let snapshot_event = read_sse_event_until(
            &mut events_resp,
            |chunk| chunk.contains("event: snapshot"),
            "admin snapshot event",
        )
        .await;
        let snapshot_line = snapshot_event
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("snapshot data line");
        let snapshot_json: serde_json::Value =
            serde_json::from_str(snapshot_line).expect("snapshot payload json");

        assert!(
            snapshot_json.get("summary").is_some(),
            "summary should exist"
        );
        assert!(
            snapshot_json.get("summaryWindows").is_some(),
            "summaryWindows should exist"
        );
        assert!(
            snapshot_json.get("siteStatus").is_some(),
            "siteStatus should exist"
        );
        assert!(
            snapshot_json.get("forwardProxy").is_some(),
            "forwardProxy should exist"
        );
        assert!(snapshot_json.get("trend").is_some(), "trend should exist");
        assert!(
            snapshot_json.get("hourlyRequestWindow").is_some(),
            "hourly request window should exist"
        );
        assert_eq!(
            snapshot_json
                .pointer("/summaryWindows/month/new_keys")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert!(
            snapshot_json
                .pointer("/summaryWindows/today/upstream_exhausted_key_count")
                .and_then(|value| value.as_i64())
                .is_some(),
            "snapshot summary windows should expose upstream exhausted key counts"
        );
        assert!(
            snapshot_json
                .pointer("/summaryWindows/today/valuable_success_count")
                .and_then(|value| value.as_i64())
                .is_some(),
            "snapshot summary windows should expose valuable success counts"
        );
        assert!(
            snapshot_json
                .pointer("/summaryWindows/month/unknown_count")
                .and_then(|value| value.as_i64())
                .is_some(),
            "snapshot summary windows should expose unknown request counts"
        );
        assert!(
            snapshot_json
                .pointer("/summaryWindows/today/quota_charge/local_estimated_credits")
                .and_then(|value| value.as_i64())
                .is_some(),
            "snapshot summary windows should expose quota charge local estimates"
        );
        assert!(
            snapshot_json
                .pointer("/summaryWindows/month/quota_charge/upstream_actual_credits")
                .and_then(|value| value.as_i64())
                .is_some(),
            "snapshot summary windows should expose quota charge upstream actual values"
        );
        assert_eq!(
            snapshot_json
                .pointer("/siteStatus/totalProxyNodes")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            snapshot_json
                .pointer("/forwardProxy/availableNodes")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        assert!(
            snapshot_json.get("exhaustedKeys").is_some(),
            "snapshot should expose exhausted keys"
        );
        assert!(
            snapshot_json.get("recentLogs").is_some(),
            "snapshot should expose recent logs"
        );
        assert!(
            snapshot_json.get("recentJobs").is_some(),
            "snapshot should expose recent jobs"
        );
        assert!(
            snapshot_json.get("disabledTokens").is_some(),
            "snapshot should expose disabled tokens"
        );
        assert!(
            snapshot_json.get("tokenCoverage").is_some(),
            "snapshot should expose token coverage"
        );
        assert_eq!(
            snapshot_json
                .pointer("/trend/request")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(8)
        );
        assert_eq!(
            snapshot_json
                .pointer("/trend/error")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(8)
        );
        assert_eq!(
            snapshot_json
                .pointer("/hourlyRequestWindow/buckets")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(49)
        );
        let exhausted_key_count = snapshot_json
            .get("exhaustedKeys")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .expect("exhausted keys array");
        let legacy_key_count = snapshot_json
            .get("keys")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .expect("legacy keys array");
        assert_eq!(
            exhausted_key_count, legacy_key_count,
            "legacy keys alias should mirror exhausted keys"
        );
        assert!(
            legacy_key_count <= 5,
            "snapshot should keep keys payload lightweight"
        );
        let recent_log_count = snapshot_json
            .get("recentLogs")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .expect("recent logs array");
        let legacy_log_count = snapshot_json
            .get("logs")
            .and_then(|value| value.as_array())
            .map(Vec::len)
            .expect("legacy logs array");
        assert_eq!(
            recent_log_count, legacy_log_count,
            "legacy logs alias should mirror recent logs"
        );
        assert!(
            legacy_log_count <= 5,
            "snapshot should keep logs payload lightweight"
        );

        drop(events_resp);
        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn request_logs_legacy_api_key_migration_preserves_request_log_foreign_keys() {
        let db_path = temp_db_path("request-logs-legacy-fk-safe-migration");
        let db_str = db_path.to_string_lossy().to_string();

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
            .expect("open legacy db pool");

        sqlx::query(
            r#"
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy api_keys");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, status_changed_at, last_used_at, quota_limit, quota_remaining, quota_synced_at, deleted_at
            ) VALUES (?, ?, 'active', NULL, ?, NULL, NULL, NULL, NULL)
            "#,
        )
        .bind("k-legacy")
        .bind("tvly-legacy-ref")
        .bind(420_i64)
        .execute(&pool)
        .await
        .expect("insert legacy api key");

        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                error_message TEXT,
                request_body BLOB,
                response_body BLOB,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy request_logs");

        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key, method, path, query, status_code, error_message, request_body, response_body, created_at
            ) VALUES (?, 'POST', '/search', NULL, 200, NULL, NULL, NULL, ?)
            RETURNING id
            "#,
        )
        .bind("tvly-legacy-ref")
        .bind(180_i64)
        .fetch_one(&pool)
        .await
        .expect("insert legacy request log");

        create_request_log_reference_tables(&pool).await;
        insert_request_log_reference_rows(&pool, "k-legacy", request_log_id).await;
        drop(pool);

        let _proxy = TavilyProxy::with_endpoint(
            vec!["tvly-legacy-ref-upgrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("upgrade proxy");

        let upgraded_pool = connect_sqlite_test_pool(&db_str).await;

        let request_row = sqlx::query(
            "SELECT id, api_key_id, auth_token_id, visibility, key_effect_code FROM request_logs WHERE id = ?",
        )
        .bind(request_log_id)
        .fetch_one(&upgraded_pool)
        .await
        .expect("read migrated request log");
        assert_eq!(request_row.try_get::<i64, _>("id").unwrap(), request_log_id);
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("api_key_id")
                .unwrap(),
            Some("k-legacy".to_string())
        );
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("auth_token_id")
                .unwrap(),
            None
        );
        assert_eq!(
            request_row.try_get::<String, _>("visibility").unwrap(),
            "visible"
        );
        assert_eq!(
            request_row.try_get::<String, _>("key_effect_code").unwrap(),
            "none"
        );
        assert!(
            !sqlite_column_exists(&upgraded_pool, "request_logs", "legacy_request_kind_key").await,
            "request_logs should drop legacy_request_kind_key during api_key rebuild"
        );
        assert!(
            !sqlite_column_exists(&upgraded_pool, "request_logs", "legacy_request_kind_label")
                .await,
            "request_logs should drop legacy_request_kind_label during api_key rebuild"
        );
        assert!(
            !sqlite_column_exists(&upgraded_pool, "request_logs", "legacy_request_kind_detail")
                .await,
            "request_logs should drop legacy_request_kind_detail during api_key rebuild"
        );

        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT request_log_id FROM auth_token_logs ORDER BY id DESC LIMIT 1",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read auth_token_logs reference"),
            Some(request_log_id)
        );
        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT request_log_id FROM api_key_maintenance_records WHERE id = 'maint-ref'",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read maintenance reference"),
            Some(request_log_id)
        );

        let fk_violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&upgraded_pool)
            .await
            .expect("run foreign_key_check");
        assert!(
            fk_violations.is_empty(),
            "migration should preserve all request_log references"
        );

        let api_key_column_still_exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM pragma_table_info('request_logs') WHERE name = 'api_key' LIMIT 1",
        )
        .fetch_optional(&upgraded_pool)
        .await
        .expect("probe api_key column")
        .is_some();
        assert!(
            !api_key_column_still_exists,
            "legacy api_key column should be removed after rebuild"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn request_logs_legacy_api_key_migration_rolls_back_when_fk_check_fails() {
        let db_path = temp_db_path("request-logs-legacy-fk-safe-migration-rollback");
        let db_str = db_path.to_string_lossy().to_string();

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
            .expect("open rollback db pool");

        sqlx::query(
            r#"
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create rollback api_keys");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, status_changed_at, last_used_at, quota_limit, quota_remaining, quota_synced_at, deleted_at
            ) VALUES (?, ?, 'active', NULL, ?, NULL, NULL, NULL, NULL)
            "#,
        )
        .bind("k-rollback")
        .bind("tvly-rollback-ref")
        .bind(512_i64)
        .execute(&pool)
        .await
        .expect("insert rollback api key");

        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                error_message TEXT,
                request_body BLOB,
                response_body BLOB,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create rollback request_logs");

        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key, method, path, query, status_code, error_message, request_body, response_body, created_at
            ) VALUES (?, 'POST', '/search', NULL, 200, NULL, NULL, NULL, ?)
            RETURNING id
            "#,
        )
        .bind("tvly-rollback-ref")
        .bind(220_i64)
        .fetch_one(&pool)
        .await
        .expect("insert rollback request log");

        create_request_log_reference_tables(&pool).await;

        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .expect("disable foreign keys for corruption fixture");
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                result_status,
                request_log_id,
                created_at
            ) VALUES ('tok-ref', 'POST', '/mcp', 'error', ?, ?)
            "#,
        )
        .bind(request_log_id + 999)
        .bind(221_i64)
        .execute(&pool)
        .await
        .expect("insert invalid auth_token_logs reference");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("reenable foreign keys after corruption fixture");
        drop(pool);

        let err = TavilyProxy::with_endpoint(
            vec!["tvly-rollback-ref-upgrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect_err("startup migration should fail on invalid preserved foreign keys");
        assert!(
            err.to_string()
                .contains("request_logs schema migration produced invalid preserved references"),
            "unexpected migration error: {err}"
        );

        let upgraded_pool = connect_sqlite_test_pool(&db_str).await;

        let api_key_column_still_exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM pragma_table_info('request_logs') WHERE name = 'api_key' LIMIT 1",
        )
        .fetch_optional(&upgraded_pool)
        .await
        .expect("probe rollback api_key column")
        .is_some();
        assert!(
            api_key_column_still_exists,
            "failed migration should leave the legacy request_logs schema intact"
        );

        let rebuilt_table_exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs_new' LIMIT 1",
        )
        .fetch_optional(&upgraded_pool)
        .await
        .expect("probe request_logs_new after rollback")
        .is_some();
        assert!(
            !rebuilt_table_exists,
            "failed migration should not leave request_logs_new behind"
        );

        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT request_log_id FROM auth_token_logs ORDER BY id DESC LIMIT 1",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read invalid preserved auth_token_logs reference"),
            Some(request_log_id + 999)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn request_logs_migration_ignores_unrelated_auth_token_log_orphans() {
        let db_path = temp_db_path("request-logs-ignore-unrelated-auth-token-log-orphans");
        let db_str = db_path.to_string_lossy().to_string();

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
            .expect("open unrelated orphan db pool");

        sqlx::query(
            r#"
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create unrelated orphan api_keys");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, status_changed_at, last_used_at, quota_limit, quota_remaining, quota_synced_at, deleted_at
            ) VALUES (?, ?, 'active', NULL, ?, NULL, NULL, NULL, NULL)
            "#,
        )
        .bind("k-unrelated")
        .bind("tvly-unrelated-ref")
        .bind(640_i64)
        .execute(&pool)
        .await
        .expect("insert unrelated orphan api key");

        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key TEXT NOT NULL,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                error_message TEXT,
                request_body BLOB,
                response_body BLOB,
                created_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create unrelated orphan request_logs");

        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key, method, path, query, status_code, error_message, request_body, response_body, created_at
            ) VALUES (?, 'POST', '/search', NULL, 200, NULL, NULL, NULL, ?)
            RETURNING id
            "#,
        )
        .bind("tvly-unrelated-ref")
        .bind(240_i64)
        .fetch_one(&pool)
        .await
        .expect("insert unrelated orphan request log");

        create_request_log_reference_tables(&pool).await;
        insert_request_log_reference_rows(&pool, "k-unrelated", request_log_id).await;

        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .expect("disable foreign keys for unrelated orphan fixture");
        sqlx::query(
            "UPDATE api_key_maintenance_records SET auth_token_log_id = ? WHERE id = 'maint-ref'",
        )
        .bind(999_999_i64)
        .execute(&pool)
        .await
        .expect("corrupt unrelated auth_token_log_id reference");
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("reenable foreign keys after unrelated orphan fixture");
        drop(pool);

        let _proxy = TavilyProxy::with_endpoint(
            vec!["tvly-unrelated-ref-upgrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("request_logs migration should ignore unrelated auth_token_log orphans");

        let upgraded_pool = connect_sqlite_test_pool(&db_str).await;
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT api_key_id FROM request_logs WHERE id = ?",
            )
            .bind(request_log_id)
            .fetch_one(&upgraded_pool)
            .await
            .expect("read migrated unrelated orphan request log"),
            Some("k-unrelated".to_string())
        );
        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT request_log_id FROM api_key_maintenance_records WHERE id = 'maint-ref'",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read preserved maintenance request_log_id"),
            Some(request_log_id)
        );
        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT auth_token_log_id FROM api_key_maintenance_records WHERE id = 'maint-ref'",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read preserved unrelated orphan auth_token_log_id"),
            Some(999_999_i64)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn request_logs_not_null_api_key_migration_preserves_references_and_accepts_null_keys() {
        let db_path = temp_db_path("request-logs-nullable-fk-safe-migration");
        let db_str = db_path.to_string_lossy().to_string();

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
            .expect("open not-null db pool");

        sqlx::query(
            r#"
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create api_keys");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, status_changed_at, last_used_at, quota_limit, quota_remaining, quota_synced_at, deleted_at
            ) VALUES (?, ?, 'active', NULL, ?, NULL, NULL, NULL, NULL)
            "#,
        )
        .bind("k-modern")
        .bind("tvly-modern-ref")
        .bind(900_i64)
        .execute(&pool)
        .await
        .expect("insert api key");

        sqlx::query(
            r#"
            CREATE TABLE request_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT NOT NULL,
                auth_token_id TEXT,
                method TEXT NOT NULL,
                path TEXT NOT NULL,
                query TEXT,
                status_code INTEGER,
                tavily_status_code INTEGER,
                error_message TEXT,
                result_status TEXT NOT NULL DEFAULT 'unknown',
                request_kind_key TEXT,
                request_kind_label TEXT,
                request_kind_detail TEXT,
                business_credits INTEGER,
                failure_kind TEXT,
                key_effect_code TEXT NOT NULL DEFAULT 'none',
                key_effect_summary TEXT,
                request_body BLOB,
                response_body BLOB,
                forwarded_headers TEXT,
                dropped_headers TEXT,
                visibility TEXT NOT NULL DEFAULT 'visible',
                created_at INTEGER NOT NULL,
                FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create modern legacy request_logs");

        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
                error_message, result_status, request_kind_key, request_kind_label, request_kind_detail,
                business_credits, failure_kind, key_effect_code, key_effect_summary, request_body,
                response_body, forwarded_headers, dropped_headers, visibility, created_at
            ) VALUES (
                ?, ?, 'POST', '/mcp', NULL, 404, 404,
                'legacy error', 'error', 'mcp:raw:/mcp/search', 'MCP | /mcp/search', NULL,
                NULL, 'mcp_path_404', 'none', NULL, X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
            )
            RETURNING id
            "#,
        )
        .bind("k-modern")
        .bind("tok-modern")
        .bind(901_i64)
        .fetch_one(&pool)
        .await
        .expect("insert request log");

        create_request_log_reference_tables(&pool).await;
        insert_request_log_reference_rows(&pool, "k-modern", request_log_id).await;
        drop(pool);

        let _proxy = TavilyProxy::with_endpoint(
            vec!["tvly-modern-ref-upgrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("upgrade proxy");

        let upgraded_pool = connect_sqlite_test_pool(&db_str).await;

        let api_key_not_null: i64 = sqlx::query_scalar(
            r#"SELECT "notnull" FROM pragma_table_info('request_logs') WHERE name = 'api_key_id'"#,
        )
        .fetch_one(&upgraded_pool)
        .await
        .expect("read api_key_id notnull");
        assert_eq!(
            api_key_not_null, 0,
            "api_key_id should be nullable after migration"
        );
        assert!(
            !sqlite_column_exists(&upgraded_pool, "request_logs", "legacy_request_kind_key").await,
            "request_logs should not re-add legacy_request_kind_key after rebuild"
        );
        assert!(
            !sqlite_column_exists(&upgraded_pool, "request_logs", "legacy_request_kind_label")
                .await,
            "request_logs should not re-add legacy_request_kind_label after rebuild"
        );
        assert!(
            !sqlite_column_exists(&upgraded_pool, "request_logs", "legacy_request_kind_detail")
                .await,
            "request_logs should not re-add legacy_request_kind_detail after rebuild"
        );

        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT request_log_id FROM auth_token_logs ORDER BY id DESC LIMIT 1",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read auth_token_logs reference"),
            Some(request_log_id)
        );
        assert_eq!(
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT request_log_id FROM api_key_maintenance_records WHERE id = 'maint-ref'",
            )
            .fetch_one(&upgraded_pool)
            .await
            .expect("read maintenance reference"),
            Some(request_log_id)
        );

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
            ) VALUES (
                NULL,
                'tok-null-key',
                'POST',
                '/mcp/search',
                NULL,
                404,
                404,
                'Not Found',
                'error',
                'mcp:raw:/mcp/search',
                'MCP | /mcp/search',
                NULL,
                NULL,
                'mcp_path_404',
                'none',
                NULL,
                X'7B7D',
                X'4E6F7420466F756E64',
                '[]',
                '[]',
                'visible',
                ?
            )
            "#,
        )
        .bind(902_i64)
        .execute(&upgraded_pool)
        .await
        .expect("insert null-key request log after migration");

        let fk_violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&upgraded_pool)
            .await
            .expect("run foreign_key_check");
        assert!(
            fk_violations.is_empty(),
            "nullable migration should preserve request_log references"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_created_at_backfill_only_runs_once() {
        let db_path = temp_db_path("api-keys-created-at-backfill-once");
        let db_str = db_path.to_string_lossy().to_string();

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
            CREATE TABLE api_keys (
                id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL UNIQUE,
                status TEXT NOT NULL DEFAULT 'active',
                status_changed_at INTEGER,
                last_used_at INTEGER NOT NULL DEFAULT 0,
                quota_limit INTEGER,
                quota_remaining INTEGER,
                quota_synced_at INTEGER,
                deleted_at INTEGER
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create legacy api_keys");

        sqlx::query(
            r#"
            INSERT INTO api_keys (
                id, api_key, status, status_changed_at, last_used_at, quota_limit, quota_remaining, quota_synced_at, deleted_at
            ) VALUES (?, ?, 'active', NULL, 0, NULL, NULL, NULL, NULL)
            "#,
        )
        .bind("k-once")
        .bind("tvly-created-at-once")
        .execute(&pool)
        .await
        .expect("insert legacy api key");
        drop(pool);

        let _proxy = TavilyProxy::with_endpoint(
            vec!["tvly-created-at-once-upgrade".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("first upgrade");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let upgraded_pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open upgraded db pool");

        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code,
                error_message, result_status, request_body, response_body, forwarded_headers,
                dropped_headers, created_at
            ) VALUES (?, NULL, 'POST', '/search', NULL, 200, 200, NULL, 'success', NULL, NULL, '', '', ?)
            "#,
        )
        .bind("k-once")
        .bind(1_234_i64)
        .execute(&upgraded_pool)
        .await
        .expect("insert late request log");
        drop(upgraded_pool);

        let _proxy = TavilyProxy::with_endpoint(
            vec!["tvly-created-at-once-second".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("second upgrade");

        let options = SqliteConnectOptions::new()
            .filename(&db_str)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let verify_pool = SqlitePoolOptions::new()
            .min_connections(1)
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("open verify db pool");

        let created_at =
            sqlx::query_scalar::<_, i64>("SELECT created_at FROM api_keys WHERE id = ? LIMIT 1")
                .bind("k-once")
                .fetch_one(&verify_pool)
                .await
                .expect("read created_at");

        assert_eq!(
            created_at, 0,
            "keys without evidence during the one-time migration must not be retroactively reclassified on later restarts"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn compute_signatures_tracks_quarantined_key_count() {
        let db_path = temp_db_path("summary-signatures-quarantine");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec![
                "tvly-signature-a".to_string(),
                "tvly-signature-b".to_string(),
            ],
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
        .bind(&key_id)
        .bind("/api/tavily/search")
        .bind("account_deactivated")
        .bind("Tavily account deactivated (HTTP 401)")
        .bind("The account associated with this API key has been deactivated.")
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("quarantine key");

        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        let (sig, latest_id) = compute_signatures(&state)
            .await
            .expect("compute signatures");
        let sig = sig.expect("summary signature");
        assert_eq!(sig.summary[4], 1);
        assert_eq!(sig.summary[5], 0);
        assert_eq!(sig.summary[6], 1);
        assert!(latest_id.is_none());

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_dashboard_sse_snapshot_refreshes_when_quota_totals_change() {
        let db_path = temp_db_path("admin-dashboard-snapshot-quota-change");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-admin-dashboard-quota".to_string()],
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

        let admin_password = "admin-dashboard-quota-password";
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

        let initial_snapshot = read_sse_event_until(
            &mut events_resp,
            |chunk| chunk.contains("event: snapshot"),
            "initial admin snapshot event",
        )
        .await;
        let initial_data = initial_snapshot
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("initial snapshot data");
        let initial_json: serde_json::Value =
            serde_json::from_str(initial_data).expect("initial snapshot payload json");
        assert_eq!(
            initial_json
                .pointer("/siteStatus/remainingQuota")
                .and_then(|value| value.as_i64()),
            Some(0)
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

        sqlx::query(
            "UPDATE api_keys SET quota_limit = ?, quota_remaining = ?, quota_synced_at = ? WHERE id = ?",
        )
        .bind(2_000_i64)
        .bind(1_234_i64)
        .bind(Utc::now().timestamp())
        .bind(&key_id)
        .execute(&pool)
        .await
        .expect("update quota totals");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
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
                    .pointer("/siteStatus/remainingQuota")
                    .and_then(|value| value.as_i64())
                    == Some(1_234)
                {
                    refreshed_snapshot = Some(payload);
                    break;
                }
            }
            if refreshed_snapshot.is_some() {
                break;
            }
        }

        let refreshed_snapshot = refreshed_snapshot.expect("quota snapshot refresh");
        assert_eq!(
            refreshed_snapshot
                .pointer("/siteStatus/remainingQuota")
                .and_then(|value| value.as_i64()),
            Some(1_234)
        );
        assert_eq!(
            refreshed_snapshot
                .pointer("/siteStatus/totalQuotaLimit")
                .and_then(|value| value.as_i64()),
            Some(2_000)
        );

        drop(events_resp);
        let _ = std::fs::remove_file(db_path);
    }


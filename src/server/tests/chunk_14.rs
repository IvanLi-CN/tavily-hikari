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

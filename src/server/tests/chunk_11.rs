    #[tokio::test]
    async fn mcp_primary_rebind_only_revokes_sessions_on_the_old_key() {
        let db_path = temp_db_path("mcp-session-rebind-scoped");
        let db_str = db_path.to_string_lossy().to_string();

        let first_api_key = "tvly-mcp-rebind-scope-a".to_string();
        let second_api_key = "tvly-mcp-rebind-scope-b".to_string();
        let third_api_key = "tvly-mcp-rebind-scope-c".to_string();
        let (upstream_addr, calls) = spawn_mock_mcp_upstream_for_session_headers(vec![
            first_api_key.clone(),
            second_api_key.clone(),
            third_api_key.clone(),
        ])
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![
                first_api_key.clone(),
                second_api_key.clone(),
                third_api_key.clone(),
            ],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        proxy
            .set_mcp_session_affinity_key_count(2)
            .await
            .expect("set affinity count");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "mcp-rebind-scope-user".to_string(),
                username: Some("rebind-user".to_string()),
                name: Some("Rebind User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let first_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:mcp-rebind-scope-first"))
            .await
            .expect("bind first token");
        let second_seed = proxy
            .create_access_token(Some("linuxdo:mcp-rebind-scope-second"))
            .await
            .expect("create second token");
        let second_token = proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:mcp-rebind-scope-second"),
                Some(&second_seed.id),
            )
            .await
            .expect("bind second token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let first_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, first_token.token
        );
        let second_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, second_token.token
        );

        let first_initialize = client
            .post(&first_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-rebind-scope-1",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("first initialize");
        assert!(first_initialize.status().is_success());
        let first_proxy_session_id = first_initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("first proxy session id")
            .to_string();

        let second_initialize = client
            .post(&second_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-rebind-scope-2",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("second initialize");
        assert!(second_initialize.status().is_success());
        let second_proxy_session_id = second_initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("second proxy session id")
            .to_string();

        let pool = connect_sqlite_test_pool(&db_str).await;
        let first_key_id: String = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&first_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("first key id");
        let second_key_id: String = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&second_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("second key id");
        assert_ne!(
            first_key_id, second_key_id,
            "affinity pool should spread the first two sessions"
        );

        proxy
            .seed_user_primary_api_key_affinity_for_test(&user.user_id, &first_key_id)
            .await
            .expect("seed primary affinity to the first session key");
        proxy
            .disable_key_by_id(&first_key_id)
            .await
            .expect("disable old primary key");

        let rebound = proxy
            .acquire_key_id_for_test(Some(&first_token.id))
            .await
            .expect("rebind primary key");
        assert_ne!(
            rebound, first_key_id,
            "rebind should move away from the disabled key"
        );

        let stale_follow_up = client
            .post(&first_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &first_proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-rebind-scope-stale",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("stale follow-up");
        assert_eq!(stale_follow_up.status(), reqwest::StatusCode::NOT_FOUND);

        let healthy_follow_up = client
            .post(&second_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &second_proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-rebind-scope-healthy",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("healthy follow-up");
        assert_eq!(healthy_follow_up.status(), reqwest::StatusCode::OK);

        let first_revoke_reason: Option<String> = sqlx::query_scalar(
            r#"SELECT revoke_reason
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&first_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("first revoke reason");
        let second_revoke_reason: Option<String> = sqlx::query_scalar(
            r#"SELECT revoke_reason
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&second_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("second revoke reason");
        assert_eq!(
            first_revoke_reason.as_deref(),
            Some("primary_api_key_rebound")
        );
        assert_eq!(second_revoke_reason, None);

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded
                .iter()
                .filter(|call| call.method == "tools/list")
                .count(),
            1,
            "only the healthy session should reach upstream after the rebind",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_session_affinity_pool_balances_new_sessions_and_keeps_existing_sessions_alive() {
        let db_path = temp_db_path("mcp-session-affinity-pool");
        let db_str = db_path.to_string_lossy().to_string();

        let first_api_key = "tvly-mcp-affinity-a".to_string();
        let second_api_key = "tvly-mcp-affinity-b".to_string();
        let third_api_key = "tvly-mcp-affinity-c".to_string();
        let (upstream_addr, calls) = spawn_mock_mcp_upstream_for_session_headers(vec![
            first_api_key.clone(),
            second_api_key.clone(),
            third_api_key.clone(),
        ])
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![
                first_api_key.clone(),
                second_api_key.clone(),
                third_api_key.clone(),
            ],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        proxy
            .set_mcp_session_affinity_key_count(2)
            .await
            .expect("set affinity count");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "mcp-session-affinity-user".to_string(),
                username: Some("affinity-user".to_string()),
                name: Some("Affinity User".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert user");
        let first_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:mcp-affinity-first"))
            .await
            .expect("bind first token");
        let second_seed = proxy
            .create_access_token(Some("linuxdo:mcp-affinity-second"))
            .await
            .expect("create second token");
        let second_token = proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:mcp-affinity-second"),
                Some(&second_seed.id),
            )
            .await
            .expect("bind second token");
        let third_seed = proxy
            .create_access_token(Some("linuxdo:mcp-affinity-third"))
            .await
            .expect("create third token");
        let third_token = proxy
            .ensure_user_token_binding_with_preferred(
                &user.user_id,
                Some("linuxdo:mcp-affinity-third"),
                Some(&third_seed.id),
            )
            .await
            .expect("bind third token");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let mut ranked_keys = fetch_api_key_rows(&pool).await;
        let subject = format!("user:{}", user.user_id);
        ranked_keys.sort_by(|(left_id, _), (right_id, _)| {
            let mut left_digest = Sha256::new();
            left_digest.update(subject.as_bytes());
            left_digest.update(b":");
            left_digest.update(left_id.as_bytes());
            let left_score: [u8; 32] = left_digest.finalize().into();

            let mut right_digest = Sha256::new();
            right_digest.update(subject.as_bytes());
            right_digest.update(b":");
            right_digest.update(right_id.as_bytes());
            let right_score: [u8; 32] = right_digest.finalize().into();

            right_score
                .cmp(&left_score)
                .then_with(|| left_id.cmp(right_id))
        });
        let ranked_secrets = ranked_keys
            .iter()
            .map(|(_, api_key)| api_key.clone())
            .collect::<Vec<_>>();
        let top_one_key = ranked_secrets[0].clone();
        let top_two_keys = ranked_secrets[..2].to_vec();
        let second_pool_key = ranked_secrets[1].clone();

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();

        let first_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, first_token.token
        );
        let second_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, second_token.token
        );
        let third_url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, third_token.token
        );

        let first_initialize = client
            .post(&first_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-affinity-1",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("first initialize");
        assert!(first_initialize.status().is_success());

        let second_initialize = client
            .post(&second_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-affinity-2",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("second initialize");
        assert!(second_initialize.status().is_success());
        let second_proxy_session_id = second_initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("second proxy session id")
            .to_string();

        proxy
            .set_mcp_session_affinity_key_count(1)
            .await
            .expect("shrink affinity count");

        let pinned_follow_up = client
            .post(&second_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", &second_proxy_session_id)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-affinity-existing",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("existing session follow-up");
        assert_eq!(pinned_follow_up.status(), reqwest::StatusCode::OK);

        let third_initialize = client
            .post(&third_url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-affinity-3",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("third initialize");
        assert!(third_initialize.status().is_success());

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            4,
            "expected two initializes, one existing-session call, and one new initialize"
        );
        assert_eq!(recorded[0].method, "initialize");
        assert_eq!(recorded[1].method, "initialize");
        assert_eq!(recorded[2].method, "tools/list");
        assert_eq!(recorded[3].method, "initialize");
        assert_eq!(
            recorded[0].tavily_api_key.as_deref(),
            Some(top_one_key.as_str())
        );
        assert_eq!(
            recorded[1].tavily_api_key.as_deref(),
            Some(second_pool_key.as_str())
        );
        assert_eq!(
            recorded[2].tavily_api_key.as_deref(),
            Some(second_pool_key.as_str())
        );
        assert_eq!(
            recorded[3].tavily_api_key.as_deref(),
            Some(top_one_key.as_str())
        );
        assert!(
            recorded[..2].iter().all(|call| call
                .tavily_api_key
                .as_ref()
                .is_some_and(|key| top_two_keys.contains(key))),
            "new sessions should stay inside the stable top-2 pool"
        );

        let stored_upstream_key: String = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               WHERE proxy_session_id = ?
               LIMIT 1"#,
        )
        .bind(&second_proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("stored pinned upstream key");
        let second_pool_key_id = ranked_keys[1].0.clone();
        assert_eq!(stored_upstream_key, second_pool_key_id);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_session_affinity_pool_balances_concurrent_initializes() {
        let db_path = temp_db_path("mcp-session-affinity-concurrent");
        let db_str = db_path.to_string_lossy().to_string();

        let first_api_key = "tvly-mcp-affinity-concurrent-a".to_string();
        let second_api_key = "tvly-mcp-affinity-concurrent-b".to_string();
        let third_api_key = "tvly-mcp-affinity-concurrent-c".to_string();
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers_with_initialize_delay(
                vec![
                    first_api_key.clone(),
                    second_api_key.clone(),
                    third_api_key.clone(),
                ],
                Duration::from_millis(150),
            )
            .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(
            vec![
                first_api_key.clone(),
                second_api_key.clone(),
                third_api_key.clone(),
            ],
            &upstream,
            &db_str,
        )
        .await
        .expect("proxy created");
        proxy
            .set_mcp_session_affinity_key_count(2)
            .await
            .expect("set affinity count");
        let access_token = proxy
            .create_access_token(Some("mcp-session-affinity-concurrent"))
            .await
            .expect("create access token");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let mut ranked_keys = fetch_api_key_rows(&pool).await;
        let subject = format!("token:{}", access_token.id);
        ranked_keys.sort_by(|(left_id, _), (right_id, _)| {
            let mut left_digest = Sha256::new();
            left_digest.update(subject.as_bytes());
            left_digest.update(b":");
            left_digest.update(left_id.as_bytes());
            let left_score: [u8; 32] = left_digest.finalize().into();

            let mut right_digest = Sha256::new();
            right_digest.update(subject.as_bytes());
            right_digest.update(b":");
            right_digest.update(right_id.as_bytes());
            let right_score: [u8; 32] = right_digest.finalize().into();

            right_score
                .cmp(&left_score)
                .then_with(|| left_id.cmp(right_id))
        });
        let top_two_key_ids = ranked_keys
            .iter()
            .take(2)
            .map(|(key_id, _)| key_id.clone())
            .collect::<Vec<_>>();
        let top_two_key_secrets = ranked_keys
            .iter()
            .take(2)
            .map(|(_, api_key)| api_key.clone())
            .collect::<Vec<_>>();

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let request = |id: &str| {
            client
                .post(&url)
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-03-26")
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {}
                    }
                }))
        };

        let (first_initialize, second_initialize) = tokio::join!(
            request("init-affinity-concurrent-1").send(),
            request("init-affinity-concurrent-2").send(),
        );
        assert!(
            first_initialize
                .expect("first concurrent initialize")
                .status()
                .is_success()
        );
        assert!(
            second_initialize
                .expect("second concurrent initialize")
                .status()
                .is_success()
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(recorded.len(), 2, "expected two initialize calls");
        assert!(
            recorded.iter().all(|call| call.method == "initialize"),
            "only initialize calls should be recorded",
        );
        assert!(
            recorded.iter().all(|call| {
                call.tavily_api_key
                    .as_ref()
                    .is_some_and(|key| top_two_key_secrets.contains(key))
            }),
            "concurrent initializes should stay inside the stable top-2 pool",
        );
        assert_ne!(
            recorded[0].tavily_api_key, recorded[1].tavily_api_key,
            "serialized initialize scheduling should spread concurrent sessions across pool keys",
        );

        let stored_key_ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT upstream_key_id
               FROM mcp_sessions
               ORDER BY created_at ASC, proxy_session_id ASC"#,
        )
        .fetch_all(&pool)
        .await
        .expect("stored upstream keys");
        assert_eq!(stored_key_ids.len(), 2);
        assert!(
            stored_key_ids
                .iter()
                .all(|key_id| top_two_key_ids.contains(key_id)),
            "stored sessions should remain inside the stable top-2 pool",
        );
        assert_ne!(
            stored_key_ids[0], stored_key_ids[1],
            "serialized initialize scheduling should persist distinct upstream keys",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_session_gc_deletes_expired_and_old_revoked_records_after_seven_days() {
        let db_path = temp_db_path("mcp-session-gc");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint(vec!["tvly-mcp-session-gc".to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-session-gc"))
            .await
            .expect("create access token");

        let pool = connect_sqlite_test_pool(&db_str).await;
        let key_id: String = sqlx::query_scalar(
            r#"SELECT id
               FROM api_keys
               WHERE api_key = ?
               LIMIT 1"#,
        )
        .bind("tvly-mcp-session-gc")
        .fetch_one(&pool)
        .await
        .expect("key id");

        let now = Utc::now().timestamp();
        let retention_secs = 7 * 24 * 60 * 60;

        for (proxy_session_id, expires_at, revoked_at, revoke_reason, updated_at) in [
            ("session-expired", now - 60, None, None, now - 60),
            (
                "session-revoked-old",
                now + 3_600,
                Some(now - 120),
                Some("manual_cleanup"),
                now - retention_secs - 1,
            ),
            ("session-active", now + 3_600, None, None, now),
        ] {
            sqlx::query(
                r#"INSERT INTO mcp_sessions (
                       proxy_session_id,
                       upstream_session_id,
                       upstream_key_id,
                       auth_token_id,
                       user_id,
                       protocol_version,
                       last_event_id,
                       created_at,
                       updated_at,
                       expires_at,
                       revoked_at,
                       revoke_reason
                   ) VALUES (?, ?, ?, ?, NULL, ?, NULL, ?, ?, ?, ?, ?)"#,
            )
            .bind(proxy_session_id)
            .bind(format!("upstream-{proxy_session_id}"))
            .bind(&key_id)
            .bind(&access_token.id)
            .bind("2025-03-26")
            .bind(now - 120)
            .bind(updated_at)
            .bind(expires_at)
            .bind(revoked_at)
            .bind(revoke_reason)
            .execute(&pool)
            .await
            .expect("insert mcp session fixture");
        }

        let deleted = proxy.gc_mcp_sessions().await.expect("gc mcp sessions");
        assert_eq!(deleted, 2);

        let remaining: Vec<String> = sqlx::query_scalar(
            r#"SELECT proxy_session_id
               FROM mcp_sessions
               ORDER BY proxy_session_id ASC"#,
        )
        .fetch_all(&pool)
        .await
        .expect("remaining mcp sessions");
        assert_eq!(remaining, vec!["session-active".to_string()]);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_follow_up_retries_once_after_retry_after_and_keeps_session_pinned() {
        let db_path = temp_db_path("mcp-follow-up-retry-after");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-mcp-follow-up-retry-after";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_retry_after_once(expected_api_key.to_string(), 1)
                .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-follow-up-retry-after"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-retry",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "browser-probe", "version": "0.1.0" }
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(initialize.status().is_success());
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        let initialized = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .send()
            .await
            .expect("notifications/initialized request");
        assert_eq!(initialized.status(), StatusCode::ACCEPTED);

        let tools_list = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-retry",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("tools/list request");
        assert_eq!(tools_list.status(), StatusCode::OK);
        let tools_list_body: serde_json::Value =
            tools_list.json().await.expect("decode tools/list response");
        assert_eq!(
            tools_list_body["result"]["tools"].as_array().map(Vec::len),
            Some(1),
            "follow-up request should succeed after one Retry-After retry",
        );

        let recorded = calls
            .lock()
            .expect("retry-after calls lock poisoned")
            .clone();
        let tools_list_calls = recorded
            .iter()
            .filter(|call| call.method == "tools/list")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(tools_list_calls.len(), 2);
        assert!(
            tools_list_calls.iter().all(|call| {
                call.upstream_session_id_header.as_deref() == Some("upstream-session-123")
                    && call.tavily_api_key == expected_api_key
            }),
            "retry should stay on the same pinned upstream session and key",
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let (rate_limited_until, last_rate_limited_at, last_rate_limit_reason): (
            Option<i64>,
            Option<i64>,
            Option<String>,
        ) = sqlx::query_as(
            r#"
            SELECT rate_limited_until, last_rate_limited_at, last_rate_limit_reason
            FROM mcp_sessions
            WHERE proxy_session_id = ?
            LIMIT 1
            "#,
        )
        .bind(&proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("fetch mcp session rate limit state");
        assert_eq!(rate_limited_until, None);
        assert!(last_rate_limited_at.is_some());
        assert_eq!(
            last_rate_limit_reason.as_deref(),
            Some("upstream_rate_limited_429")
        );

        let retry_request_effects: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT key_effect_code
            FROM request_logs
            WHERE key_effect_code IN (
                'mcp_session_init_backoff_set',
                'mcp_session_retry_scheduled',
                'mcp_session_retry_waited'
            )
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("load retry request log effects");
        assert_eq!(
            retry_request_effects.last().map(String::as_str),
            Some("mcp_session_retry_waited")
        );
        assert!(
            retry_request_effects
                .iter()
                .any(|code| code == "mcp_session_init_backoff_set"
                    || code == "mcp_session_retry_scheduled"),
            "first rate-limited follow-up attempt should retain a retry/backoff audit marker"
        );

        let latest_token_retry_effect: String = sqlx::query_scalar(
            "SELECT key_effect_code FROM auth_token_logs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("load latest token retry effect");
        assert_eq!(latest_token_retry_effect, "mcp_session_retry_waited");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_follow_up_requests_are_serialized_per_proxy_session() {
        let db_path = temp_db_path("mcp-follow-up-serialized");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-mcp-follow-up-serialized";
        let (upstream_addr, max_in_flight) =
            spawn_mock_mcp_upstream_for_serialized_session_requests(
                expected_api_key.to_string(),
                Duration::from_millis(120),
            )
            .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-follow-up-serialized"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-serialize",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "browser-probe", "version": "0.1.0" }
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert!(initialize.status().is_success());
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        let initialized = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .send()
            .await
            .expect("notifications/initialized request");
        assert_eq!(initialized.status(), StatusCode::ACCEPTED);

        let send_tools_list = |id: &'static str| {
            client
                .post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-03-26")
                .header("mcp-session-id", proxy_session_id.as_str())
                .json(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/list"
                }))
                .send()
        };

        let (first, second) = tokio::join!(
            send_tools_list("tools-serialize-1"),
            send_tools_list("tools-serialize-2")
        );
        assert_eq!(
            first.expect("first serialized tools/list").status(),
            StatusCode::OK
        );
        assert_eq!(
            second.expect("second serialized tools/list").status(),
            StatusCode::OK
        );
        assert_eq!(
            max_in_flight.load(Ordering::SeqCst),
            1,
            "same proxy session should never send concurrent follow-up requests upstream",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_initialized_notification_is_forwarded_after_initialize() {
        let db_path = temp_db_path("mcp-initialized-notification-forwarding");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-mcp-initialized-key";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let access_token = proxy
            .create_access_token(Some("mcp-initialized-notification"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-2",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "browser-probe", "version": "0.1.0" }
                }
            }))
            .send()
            .await
            .expect("initialize request");

        assert!(initialize.status().is_success());
        let session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        let initialized = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", session_id.as_str())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .send()
            .await
            .expect("notifications/initialized request");

        assert_eq!(
            initialized.status(),
            StatusCode::ACCEPTED,
            "notifications/initialized should allow 202 empty body"
        );
        let initialized_body = initialized
            .text()
            .await
            .expect("read notifications/initialized response body");
        assert!(
            initialized_body.trim().is_empty(),
            "notifications/initialized should keep an empty 202 body"
        );

        let tools_list = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", session_id.as_str())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-2",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("tools/list request");

        assert!(tools_list.status().is_success());

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            3,
            "expected initialize + initialized + tools/list"
        );
        assert_eq!(recorded[0].method, "initialize");
        assert_eq!(recorded[1].method, "notifications/initialized");
        assert_eq!(recorded[1].session_id.as_deref(), Some("session-123"));
        assert_eq!(recorded[1].protocol_version.as_deref(), Some("2025-03-26"));
        assert_eq!(recorded[1].last_event_id, None);
        assert_eq!(recorded[2].method, "tools/list");
        assert_eq!(recorded[2].session_id.as_deref(), Some("session-123"));
        assert_eq!(recorded[2].protocol_version.as_deref(), Some("2025-03-26"));
        assert_eq!(recorded[2].last_event_id, None);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_initialize_rebalance_percent_100_uses_local_facade() {
        let db_path = temp_db_path("mcp-rebalance-local-init");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-local-init";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr =
            spawn_rebalance_gateway_mock(expected_api_key.to_string(), seen.clone()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-local-init"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "browser-probe", "version": "0.1.0" }
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();
        let initialize_body: Value = initialize
            .json()
            .await
            .expect("decode rebalance initialize response");
        assert_eq!(
            initialize_body["result"]["capabilities"]["prompts"]["listChanged"].as_bool(),
            Some(false),
            "rebalance initialize should advertise prompts/list parity"
        );
        assert!(
            initialize_body["result"]["capabilities"]
                .get("resources")
                .is_some(),
            "rebalance initialize should advertise resources parity"
        );

        let tools_list = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-tools-list",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("tools/list request");
        assert_eq!(tools_list.status(), StatusCode::OK);
        let body: Value = tools_list.json().await.expect("decode tools/list response");
        assert_eq!(
            body["result"]["tools"].as_array().map(Vec::len),
            Some(5),
            "rebalance tools/list should be served locally with five Tavily tools"
        );
        let tools = body["result"]["tools"]
            .as_array()
            .expect("rebalance tools/list should include a tools array");
        let tool_by_name = tools
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name").and_then(Value::as_str)?;
                Some((name, tool))
            })
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            tool_by_name
                .get("tavily_search")
                .and_then(|tool| tool.get("inputSchema")),
            Some(&json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"],
                "additionalProperties": true
            })),
            "rebalance search schema should advertise required query"
        );
        assert_eq!(
            tool_by_name
                .get("tavily_extract")
                .and_then(|tool| tool.get("inputSchema")),
            Some(&json!({
                "type": "object",
                "properties": {
                    "urls": {
                        "oneOf": [
                            { "type": "string" },
                            {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        ]
                    }
                },
                "required": ["urls"],
                "additionalProperties": true
            })),
            "rebalance extract schema should advertise required urls"
        );
        assert_eq!(
            tool_by_name
                .get("tavily_crawl")
                .and_then(|tool| tool.get("inputSchema")),
            Some(&json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"],
                "additionalProperties": true
            })),
            "rebalance crawl schema should advertise required url"
        );
        assert_eq!(
            tool_by_name
                .get("tavily_map")
                .and_then(|tool| tool.get("inputSchema")),
            Some(&json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"],
                "additionalProperties": true
            })),
            "rebalance map schema should advertise required url"
        );
        assert_eq!(
            tool_by_name
                .get("tavily_research")
                .and_then(|tool| tool.get("inputSchema")),
            Some(&json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                },
                "required": ["input"],
                "additionalProperties": true
            })),
            "rebalance research schema should advertise required input"
        );

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "initialize + tools/list should stay local under rebalance mode"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let row = sqlx::query(
            r#"
            SELECT gateway_mode, experiment_variant, upstream_session_id, upstream_key_id
            FROM mcp_sessions
            WHERE proxy_session_id = ?
            LIMIT 1
            "#,
        )
        .bind(&proxy_session_id)
        .fetch_one(&pool)
        .await
        .expect("fetch rebalance mcp session");
        assert_eq!(
            row.try_get::<String, _>("gateway_mode").unwrap(),
            tavily_hikari::MCP_GATEWAY_MODE_REBALANCE
        );
        assert_eq!(
            row.try_get::<String, _>("experiment_variant").unwrap(),
            tavily_hikari::MCP_EXPERIMENT_VARIANT_REBALANCE
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("upstream_session_id")
                .unwrap(),
            None
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("upstream_key_id").unwrap(),
            None
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_control_plane_lists_stay_local_and_return_empty_results() {
        let db_path = temp_db_path("mcp-rebalance-control-plane-parity");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-control-plane-parity";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr =
            spawn_rebalance_gateway_mock(expected_api_key.to_string(), seen.clone()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-control-plane-parity"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-parity-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "browser-probe", "version": "0.1.0" }
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        for (method_name, response_id, expected_field) in [
            ("prompts/list", "rebalance-prompts-list", "prompts"),
            ("resources/list", "rebalance-resources-list", "resources"),
            (
                "resources/templates/list",
                "rebalance-resource-templates-list",
                "resourceTemplates",
            ),
        ] {
            let response = client
                .post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-03-26")
                .header("mcp-session-id", proxy_session_id.as_str())
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": response_id,
                    "method": method_name,
                }))
                .send()
                .await
                .unwrap_or_else(|err| panic!("{method_name} request failed: {err}"));
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{method_name} should succeed under rebalance parity"
            );
            let body: Value = response
                .json()
                .await
                .unwrap_or_else(|err| panic!("decode {method_name} response: {err}"));
            assert_eq!(
                body["result"][expected_field].as_array().map(Vec::len),
                Some(0),
                "{method_name} should return an empty {expected_field} list"
            );
        }

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "initialize + prompts/resources parity methods should stay local under rebalance mode"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_tools_call_search_uses_http_upstream_and_strict_headers() {
        let db_path = temp_db_path("mcp-rebalance-search-http");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-search-http";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr =
            spawn_rebalance_gateway_mock(expected_api_key.to_string(), seen.clone()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-search-http"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-search-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": { "name": "browser-probe", "version": "0.1.0" }
                }
            }))
            .send()
            .await
            .expect("initialize request");
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        let search = client
            .post(&url)
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .header("x-forwarded-for", "198.51.100.1")
            .header("cookie", "session=should-not-leak")
            .header("sec-fetch-mode", "cors")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-search-call",
                "method": "tools/call",
                "params": {
                    "name": "tavily_search",
                    "arguments": {
                        "query": "rebalance strict header test",
                        "search_depth": "basic"
                    }
                }
            }))
            .send()
            .await
            .expect("rebalance search request");
        assert_eq!(search.status(), StatusCode::OK);
        let body: Value = search.json().await.expect("decode rebalance search body");
        assert_eq!(
            body["result"]["structuredContent"]["usage"]["credits"].as_i64(),
            Some(1)
        );

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.iter().filter(|call| call.path == "/mcp").count(),
            0,
            "rebalance search should not hit upstream /mcp"
        );
        let search_calls = recorded
            .iter()
            .filter(|call| call.path == "/search")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(search_calls.len(), 1, "expected one upstream /search call");
        let search_call = &search_calls[0];
        assert!(
            !search_call.headers.contains_key("mcp-session-id"),
            "rebalance HTTP call must not forward mcp-session-id"
        );
        assert!(
            !search_call.headers.contains_key("x-forwarded-for"),
            "rebalance HTTP call must not leak x-forwarded-for"
        );
        assert!(
            !search_call.headers.contains_key("cookie"),
            "rebalance HTTP call must not leak cookies"
        );
        assert!(
            !search_call.headers.contains_key("sec-fetch-mode"),
            "rebalance HTTP call must not leak browser sec-* headers"
        );
        assert!(
            search_call.headers.contains_key("authorization"),
            "rebalance HTTP call must send Authorization"
        );
        assert!(
            search_call.headers.contains_key("accept"),
            "rebalance HTTP call must keep Accept"
        );
        assert!(
            search_call.headers.contains_key("content-type"),
            "rebalance HTTP call must keep Content-Type"
        );
        assert!(
            search_call.headers.contains_key("user-agent"),
            "rebalance HTTP call must inject a dedicated User-Agent"
        );
        assert_eq!(
            search_call.body.get("query").and_then(Value::as_str),
            Some("rebalance strict header test"),
            "rebalance HTTP call should forward the Tavily tool payload as JSON"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let request_row = sqlx::query(
            r#"
            SELECT gateway_mode, experiment_variant, proxy_session_id, upstream_operation
            FROM request_logs
            WHERE path = '/mcp'
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("fetch latest request log");
        assert_eq!(
            request_row.try_get::<String, _>("gateway_mode").unwrap(),
            tavily_hikari::MCP_GATEWAY_MODE_REBALANCE
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("experiment_variant")
                .unwrap(),
            tavily_hikari::MCP_EXPERIMENT_VARIANT_REBALANCE
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("proxy_session_id")
                .unwrap(),
            proxy_session_id
        );
        assert_eq!(
            request_row
                .try_get::<String, _>("upstream_operation")
                .unwrap(),
            "http_search"
        );

        let token_row = sqlx::query(
            r#"
            SELECT gateway_mode, experiment_variant, proxy_session_id, upstream_operation
            FROM auth_token_logs
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("fetch latest token log");
        assert_eq!(
            token_row.try_get::<String, _>("gateway_mode").unwrap(),
            tavily_hikari::MCP_GATEWAY_MODE_REBALANCE
        );
        assert_eq!(
            token_row
                .try_get::<String, _>("experiment_variant")
                .unwrap(),
            tavily_hikari::MCP_EXPERIMENT_VARIANT_REBALANCE
        );
        assert_eq!(
            token_row.try_get::<String, _>("proxy_session_id").unwrap(),
            proxy_session_id
        );
        assert_eq!(
            token_row
                .try_get::<String, _>("upstream_operation")
                .unwrap(),
            "http_search"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_tool_errors_use_top_level_is_error_and_content_array() {
        let db_path = temp_db_path("mcp-rebalance-search-http-error");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-search-http-error";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr = spawn_rebalance_gateway_http_error_mock(
            expected_api_key.to_string(),
            seen,
            StatusCode::BAD_REQUEST,
            json!({
                "status": 400,
                "detail": "bad query"
            }),
        )
        .await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-search-http-error"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-error-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        let search = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-search-error",
                "method": "tools/call",
                "params": {
                    "name": "tavily_search",
                    "arguments": {
                        "query": "bad input"
                    }
                }
            }))
            .send()
            .await
            .expect("rebalance search request");
        assert_eq!(search.status(), StatusCode::OK);
        let body: Value = search
            .json()
            .await
            .expect("decode rebalance error response");
        assert_eq!(body["result"]["isError"].as_bool(), Some(true));
        assert!(
            body["result"]["content"].is_array(),
            "rebalance error responses must keep a top-level content array"
        );
        assert_eq!(
            body["result"]["structuredContent"]["isError"].as_bool(),
            None,
            "isError must not be nested inside structuredContent"
        );
        assert_eq!(
            body["result"]["structuredContent"]["status"].as_i64(),
            Some(400)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_parse_error_returns_jsonrpc_parse_error() {
        let db_path = temp_db_path("mcp-rebalance-parse-error");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-parse-error";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr =
            spawn_rebalance_gateway_mock(expected_api_key.to_string(), seen.clone()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-parse-error"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let response = client
            .post(&url)
            .header("content-type", "application/json")
            .body("{")
            .send()
            .await
            .expect("parse-error request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = response.json().await.expect("decode parse-error body");
        assert_eq!(body["error"]["code"].as_i64(), Some(-32700));
        assert_eq!(body["error"]["message"].as_str(), Some("Parse error"));

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "parse errors must be rejected locally before any upstream hit"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_empty_batch_returns_invalid_request() {
        let db_path = temp_db_path("mcp-rebalance-empty-batch");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-empty-batch";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr =
            spawn_rebalance_gateway_mock(expected_api_key.to_string(), seen.clone()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-empty-batch"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let response = client
            .post(&url)
            .header("content-type", "application/json")
            .json(&json!([]))
            .send()
            .await
            .expect("empty-batch request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = response.json().await.expect("decode empty-batch body");
        assert_eq!(body["error"]["code"].as_i64(), Some(-32600));
        assert_eq!(body["error"]["message"].as_str(), Some("Invalid Request"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_response_only_batch_is_rejected_locally() {
        let db_path = temp_db_path("mcp-rebalance-response-only-batch");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-response-only-batch";
        let seen: RecordedRebalanceGatewayCalls = Arc::new(Mutex::new(Vec::new()));
        let upstream_addr =
            spawn_rebalance_gateway_mock(expected_api_key.to_string(), seen.clone()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: request_rate_limit(),
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: true,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-response-only-batch"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );
        let client = Client::new();

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "rebalance-response-only-init",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .expect("initialize response should expose mcp-session-id")
            .to_string();

        let response = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&json!([
                {
                    "jsonrpc": "2.0",
                    "id": "server-request-1",
                    "result": { "ok": true }
                }
            ]))
            .send()
            .await
            .expect("response-only batch request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = response
            .json()
            .await
            .expect("decode response-only batch body");
        assert_eq!(body["error"]["code"].as_i64(), Some(-32600));

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "response-only batches must be rejected locally"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_tools_call_follow_up_without_session_header_is_rejected_locally() {
        let db_path = temp_db_path("mcp-tools-call-follow-up-missing-session-id");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-mcp-tools-call-follow-up-missing-session-id";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-tools-call-follow-up-missing-session-id"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-tools-call-missing-session-id",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);

        let rejected = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-call-missing-session-id",
                "method": "tools/call",
                "params": {
                    "name": "tavily-search",
                    "arguments": {
                        "query": "missing session id follow-up",
                        "search_depth": "basic"
                    }
                }
            }))
            .send()
            .await
            .expect("tools/call follow-up without session id");
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let body: Value = rejected
            .json()
            .await
            .expect("parse missing-session-id tools/call body");
        assert_eq!(
            body.get("error").and_then(|value| value.as_str()),
            Some("session_required")
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            1,
            "missing-session-id tools/call follow-up should be rejected locally"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_response_only_follow_up_with_revoked_session_returns_not_found() {
        let db_path = temp_db_path("mcp-response-only-revoked-session");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-mcp-response-only-revoked-session";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy = TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
            .await
            .expect("proxy created");
        let mut settings = proxy
            .get_system_settings()
            .await
            .expect("load system settings");
        settings.rebalance_mcp_enabled = true;
        settings.rebalance_mcp_session_percent = 100;
        proxy
            .set_system_settings(&settings)
            .await
            .expect("enable rebalance session routing");
        let access_token = proxy
            .create_access_token(Some("mcp-response-only-revoked-session"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-revoked-response-only",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);
        let proxy_session_id = initialize
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .expect("initialize should return mcp-session-id");

        proxy
            .revoke_mcp_session(&proxy_session_id, "test_revoked")
            .await
            .expect("revoke session");

        let response_only = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "server-request-1",
                "result": { "ok": true }
            }))
            .send()
            .await
            .expect("response-only request");
        assert_eq!(response_only.status(), StatusCode::NOT_FOUND);
        let body: Value = response_only
            .json()
            .await
            .expect("parse response-only revoked-session body");
        assert_eq!(
            body.get("error").and_then(|value| value.as_str()),
            Some("session_unavailable")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_follow_up_without_session_header_is_rejected_locally() {
        let db_path = temp_db_path("mcp-follow-up-missing-session-id");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-mcp-follow-up-missing-session-id";
        let (upstream_addr, calls) =
            spawn_mock_mcp_upstream_for_session_headers(vec![expected_api_key.to_string()]).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("mcp-follow-up-missing-session-id"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;
        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, access_token.token
        );

        let initialize = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "init-missing-session-id",
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {}
                }
            }))
            .send()
            .await
            .expect("initialize request");
        assert_eq!(initialize.status(), StatusCode::OK);

        let rejected = client
            .post(&url)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "tools-missing-session-id",
                "method": "tools/list"
            }))
            .send()
            .await
            .expect("follow-up request without session id");
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let body: Value = rejected.json().await.expect("parse missing-session-id body");
        assert_eq!(
            body.get("error").and_then(|value| value.as_str()),
            Some("session_required")
        );

        let recorded = calls
            .lock()
            .expect("session header calls lock poisoned")
            .clone();
        assert_eq!(
            recorded.len(),
            1,
            "missing-session-id follow-up should be rejected locally"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rejects_invalid_token_in_query_param() {
        let db_path = temp_db_path("e2e-query-token-invalid");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-e2e-upstream-key-invalid";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");

        let proxy_addr =
            spawn_proxy_server(proxy.clone(), "https://api.tavily.com".to_string()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?tavilyApiKey={}",
            proxy_addr, "th-invalid-unknown"
        );
        let resp = client
            .post(url)
            .body("{}")
            .send()
            .await
            .expect("request to proxy succeeds");

        assert_eq!(
            resp.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "expected 401 for invalid query param token"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_query_token_is_stripped_from_persisted_logs() {
        let db_path = temp_db_path("e2e-query-token-log-redaction");
        let db_str = db_path.to_string_lossy().to_string();

        let expected_api_key = "tvly-e2e-query-token-log-redaction-key";
        let upstream_addr = spawn_mock_upstream(expected_api_key.to_string()).await;
        let upstream = format!("http://{}", upstream_addr);

        let proxy =
            TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
                .await
                .expect("proxy created");
        let access_token = proxy
            .create_access_token(Some("e2e-query-token-log-redaction"))
            .await
            .expect("create access token");

        let proxy_addr = spawn_proxy_server(proxy.clone(), upstream.clone()).await;

        let client = Client::new();
        let url = format!(
            "http://{}/mcp?foo=1&tavilyApiKey={}&bar=2",
            proxy_addr, access_token.token
        );
        let resp = client
            .post(url)
            .json(&serde_json::json!({ "method": "tools/list" }))
            .send()
            .await
            .expect("request to proxy succeeds");

        assert!(
            resp.status().is_success(),
            "expected successful MCP response"
        );

        let logs = proxy
            .token_recent_logs(&access_token.id, 5, None)
            .await
            .expect("read token logs");
        let latest = logs.first().expect("a request log should be recorded");

        assert_eq!(latest.path, "/mcp");
        assert_eq!(latest.query.as_deref(), Some("foo=1&bar=2"));
        assert!(
            latest
                .query
                .as_deref()
                .is_none_or(|query| !query.contains(&access_token.token)),
            "persisted query log should never contain the raw access token"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_settings_and_stats_endpoints_work() {
        let db_path = temp_db_path("admin-forward-proxy-settings");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let fake_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let settings = client
            .get(format!("http://{addr}/api/settings"))
            .send()
            .await
            .expect("get settings");
        assert_eq!(settings.status(), StatusCode::OK);
        let settings_body = settings
            .json::<serde_json::Value>()
            .await
            .expect("decode settings");
        assert_eq!(
            settings_body["forwardProxy"]["insertDirect"].as_bool(),
            Some(true)
        );
        assert_eq!(
            settings_body["systemSettings"]["mcpSessionAffinityKeyCount"].as_i64(),
            Some(5)
        );
        assert_eq!(
            settings_body["systemSettings"]["requestRateLimit"].as_i64(),
            Some(request_rate_limit())
        );
        assert_eq!(
            settings_body["systemSettings"]["rebalanceMcpEnabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            settings_body["systemSettings"]["rebalanceMcpSessionPercent"].as_i64(),
            Some(100)
        );
        assert_eq!(
            settings_body["systemSettings"]["userBlockedKeyBaseLimit"].as_i64(),
            Some(tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT)
        );

        let updated_system = client
            .put(format!("http://{addr}/api/settings/system"))
            .json(&serde_json::json!({
                "requestRateLimit": 72,
                "mcpSessionAffinityKeyCount": 3,
                "rebalanceMcpEnabled": true,
                "rebalanceMcpSessionPercent": 35,
                "userBlockedKeyBaseLimit": 8,
            }))
            .send()
            .await
            .expect("update system settings");
        assert_eq!(updated_system.status(), StatusCode::OK);
        let updated_system_body = updated_system
            .json::<serde_json::Value>()
            .await
            .expect("decode updated system settings");
        assert_eq!(
            updated_system_body["mcpSessionAffinityKeyCount"].as_i64(),
            Some(3)
        );
        assert_eq!(updated_system_body["requestRateLimit"].as_i64(), Some(72));
        assert_eq!(
            updated_system_body["rebalanceMcpEnabled"].as_bool(),
            Some(true)
        );
        assert_eq!(
            updated_system_body["rebalanceMcpSessionPercent"].as_i64(),
            Some(35)
        );
        assert_eq!(updated_system_body["userBlockedKeyBaseLimit"].as_i64(), Some(8));

        let persisted_settings = client
            .get(format!("http://{addr}/api/settings"))
            .send()
            .await
            .expect("get persisted settings");
        assert_eq!(persisted_settings.status(), StatusCode::OK);
        let persisted_settings_body = persisted_settings
            .json::<serde_json::Value>()
            .await
            .expect("decode persisted settings");
        assert_eq!(
            persisted_settings_body["systemSettings"]["mcpSessionAffinityKeyCount"].as_i64(),
            Some(3)
        );
        assert_eq!(
            persisted_settings_body["systemSettings"]["requestRateLimit"].as_i64(),
            Some(72)
        );
        assert_eq!(
            persisted_settings_body["systemSettings"]["rebalanceMcpEnabled"].as_bool(),
            Some(true)
        );
        assert_eq!(
            persisted_settings_body["systemSettings"]["rebalanceMcpSessionPercent"].as_i64(),
            Some(35)
        );
        assert_eq!(
            persisted_settings_body["systemSettings"]["userBlockedKeyBaseLimit"].as_i64(),
            Some(8)
        );

        let updated = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [format!("http://{}", fake_proxy_addr)],
                "subscriptionUrls": [],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": true,
            }))
            .send()
            .await
            .expect("update settings");
        assert_eq!(updated.status(), StatusCode::OK);
        let updated_body = updated
            .json::<serde_json::Value>()
            .await
            .expect("decode updated settings");
        assert_eq!(
            updated_body["proxyUrls"].as_array().map(Vec::len),
            Some(1),
            "manual proxy url should persist",
        );
        assert!(
            updated_body["nodes"]
                .as_array()
                .is_some_and(|nodes| nodes.len() >= 2),
            "manual node plus direct node should be visible",
        );

        let stats = client
            .get(format!("http://{addr}/api/stats/forward-proxy"))
            .send()
            .await
            .expect("get stats");
        assert_eq!(stats.status(), StatusCode::OK);
        let stats_body = stats
            .json::<serde_json::Value>()
            .await
            .expect("decode stats");
        assert!(
            stats_body["nodes"]
                .as_array()
                .is_some_and(|nodes| !nodes.is_empty()),
            "stats should include at least one node",
        );

        let summary = client
            .get(format!("http://{addr}/api/stats/forward-proxy/summary"))
            .send()
            .await
            .expect("get dashboard summary");
        assert_eq!(summary.status(), StatusCode::OK);
        let summary_body = summary
            .json::<serde_json::Value>()
            .await
            .expect("decode dashboard summary");
        assert!(
            summary_body["totalNodes"]
                .as_i64()
                .is_some_and(|value| value >= 1),
            "dashboard summary should expose total node count",
        );
        assert!(
            summary_body["availableNodes"]
                .as_i64()
                .is_some_and(|value| value >= 0),
            "dashboard summary should expose available node count",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_system_settings_put_preserves_request_rate_limit_when_legacy_payload_omits_it() {
        let db_path = temp_db_path("admin-system-settings-legacy-request-rate");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        proxy
            .set_system_settings(&tavily_hikari::SystemSettings {
                request_rate_limit: 88,
                mcp_session_affinity_key_count: 5,
                rebalance_mcp_enabled: false,
                rebalance_mcp_session_percent: 100,
                user_blocked_key_base_limit: 7,
            })
            .await
            .expect("seed system settings");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let updated = client
            .put(format!("http://{addr}/api/settings/system"))
            .json(&serde_json::json!({
                "mcpSessionAffinityKeyCount": 3,
                "rebalanceMcpEnabled": true,
                "rebalanceMcpSessionPercent": 40,
            }))
            .send()
            .await
            .expect("update system settings");

        assert_eq!(updated.status(), StatusCode::OK);
        let updated_body = updated
            .json::<serde_json::Value>()
            .await
            .expect("decode updated system settings");
        assert_eq!(updated_body["requestRateLimit"].as_i64(), Some(88));
        assert_eq!(updated_body["mcpSessionAffinityKeyCount"].as_i64(), Some(3));
        assert_eq!(updated_body["rebalanceMcpEnabled"].as_bool(), Some(true));
        assert_eq!(updated_body["rebalanceMcpSessionPercent"].as_i64(), Some(40));
        assert_eq!(updated_body["userBlockedKeyBaseLimit"].as_i64(), Some(7));

        let persisted = client
            .get(format!("http://{addr}/api/settings"))
            .send()
            .await
            .expect("get settings");
        assert_eq!(persisted.status(), StatusCode::OK);
        let persisted_body = persisted
            .json::<serde_json::Value>()
            .await
            .expect("decode persisted settings");
        assert_eq!(
            persisted_body["systemSettings"]["requestRateLimit"].as_i64(),
            Some(88)
        );
        assert_eq!(
            persisted_body["systemSettings"]["userBlockedKeyBaseLimit"].as_i64(),
            Some(7)
        );

        let _ = std::fs::remove_file(db_path);
    }

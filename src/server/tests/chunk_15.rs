    #[tokio::test]
    async fn admin_token_list_filters_and_batch_mutations() {
        let db_path = temp_db_path("admin-token-filters-batch");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let alice = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "admin-token-filter-alice".to_string(),
                username: Some("filter_alice".to_string()),
                name: Some("Filter Alice".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert alice");

        let bound = proxy
            .ensure_user_token_binding(&alice.user_id, Some("team-bound"))
            .await
            .expect("bind alice token");
        let unbound = proxy
            .create_access_tokens_batch("ops", 1, Some("manual freeze candidate"))
            .await
            .expect("create grouped token")
            .into_iter()
            .next()
            .expect("grouped token");
        let plain = proxy
            .create_access_token(Some("plain token"))
            .await
            .expect("create plain token");

        proxy
            .set_access_token_enabled(&unbound.id, false)
            .await
            .expect("freeze grouped token");

        let addr = spawn_admin_tokens_server(proxy, true).await;
        let client = Client::new();

        let bound_resp = client
            .get(format!(
                "http://{}/api/tokens?owner=bound&q=filter_alice&per_page=20",
                addr
            ))
            .send()
            .await
            .expect("bound filter request");
        assert_eq!(bound_resp.status(), reqwest::StatusCode::OK);
        let bound_body: serde_json::Value = bound_resp.json().await.expect("bound filter json");
        assert_eq!(bound_body.get("total").and_then(|value| value.as_i64()), Some(1));
        assert_eq!(
            bound_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(|value| value.as_str()),
            Some(bound.id.as_str())
        );

        let frozen_resp = client
            .get(format!(
                "http://{}/api/tokens?group=ops&enabled=frozen&per_page=20",
                addr
            ))
            .send()
            .await
            .expect("frozen filter request");
        assert_eq!(frozen_resp.status(), reqwest::StatusCode::OK);
        let frozen_body: serde_json::Value = frozen_resp.json().await.expect("frozen filter json");
        assert_eq!(
            frozen_body.get("total").and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            frozen_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(|value| value.as_str()),
            Some(unbound.id.as_str())
        );

        let ungrouped_resp = client
            .get(format!(
                "http://{}/api/tokens?no_group=true&owner=unbound&per_page=20",
                addr
            ))
            .send()
            .await
            .expect("ungrouped filter request");
        assert_eq!(ungrouped_resp.status(), reqwest::StatusCode::OK);
        let ungrouped_body: serde_json::Value =
            ungrouped_resp.json().await.expect("ungrouped filter json");
        assert_eq!(
            ungrouped_body.get("total").and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            ungrouped_body
                .get("items")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|item| item.get("id"))
                .and_then(|value| value.as_str()),
            Some(plain.id.as_str())
        );

        let activate_resp = client
            .patch(format!("http://{}/api/tokens/batch/status", addr))
            .json(&serde_json::json!({
                "ids": [unbound.id, "missing-token"],
                "enabled": true
            }))
            .send()
            .await
            .expect("batch activate request");
        assert_eq!(activate_resp.status(), reqwest::StatusCode::OK);
        let activate_body: serde_json::Value =
            activate_resp.json().await.expect("batch activate json");
        assert_eq!(
            activate_body.get("updated").and_then(|value| value.as_i64()),
            Some(1)
        );
        assert_eq!(
            activate_body
                .get("missing")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_str()),
            Some("missing-token")
        );

        let delete_resp = client
            .delete(format!("http://{}/api/tokens/batch", addr))
            .json(&serde_json::json!({ "ids": [plain.id] }))
            .send()
            .await
            .expect("batch delete request");
        assert_eq!(delete_resp.status(), reqwest::StatusCode::OK);
        let delete_body: serde_json::Value = delete_resp.json().await.expect("batch delete json");
        assert_eq!(
            delete_body.get("updated").and_then(|value| value.as_i64()),
            Some(1)
        );

        let after_delete_resp = client
            .get(format!(
                "http://{}/api/tokens?q=plain%20token&per_page=20",
                addr
            ))
            .send()
            .await
            .expect("after delete request");
        assert_eq!(after_delete_resp.status(), reqwest::StatusCode::OK);
        let after_delete_body: serde_json::Value =
            after_delete_resp.json().await.expect("after delete json");
        assert_eq!(
            after_delete_body
                .get("total")
                .and_then(|value| value.as_i64()),
            Some(0)
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    #[cfg(web_assets_embedded)]
    async fn registration_paused_route_falls_back_to_local_index_even_when_embedded_dedicated_spa_exists(
    ) {
        assert!(
            tavily_hikari::web_assets::embedded_bytes("registration-paused.html").is_some(),
            "embedded registration paused asset should exist for this regression test",
        );

        let db_path = temp_db_path("registration-paused-route-embedded-fallback");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("create proxy");
        let static_dir = temp_static_dir("registration-paused-embedded-fallback");
        std::fs::remove_file(static_dir.join("registration-paused.html"))
            .expect("remove dedicated registration paused spa");
        let state = Arc::new(AppState {
            proxy,
            static_dir: Some(static_dir),
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: linuxdo_oauth_options_for_test(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        let app = Router::new()
            .route("/registration-paused", get(serve_registration_paused_index))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener addr");
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .expect("serve app");
        });

        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let resp = client
            .get(format!("http://{}/registration-paused", addr))
            .send()
            .await
            .expect("registration paused request");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let html = resp.text().await.expect("registration paused html");
        assert!(html.contains("<title>index</title>"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn maintenance_worker_limits_remote_io_jobs_to_one_active_run() {
        let db_path = temp_db_path("maintenance-worker-remote-io-slot");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-block-a".to_string(), "tvly-block-b".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let pool = connect_sqlite_test_pool(&db_str).await;
        let key_rows = fetch_api_key_rows(&pool).await;
        let first_key_id = key_rows
            .iter()
            .find(|(_, secret)| secret == "tvly-block-a")
            .map(|(id, _)| id.clone())
            .expect("first blocking key id");
        let second_key_id = key_rows
            .iter()
            .find(|(_, secret)| secret == "tvly-block-b")
            .map(|(id, _)| id.clone())
            .expect("second blocking key id");
        let (upstream_addr, hits, release_tx) = spawn_usage_blocking_mock_server().await;
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin: true,
            usage_base: format!("http://{upstream_addr}"),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });
        spawn_maintenance_worker(state.clone());

        let first_job_id = enqueue_scheduled_job(
            state.as_ref(),
            "quota_sync",
            Some(&first_key_id),
            TRIGGER_SOURCE_SCHEDULER,
        )
        .await
        .expect("enqueue first quota sync");
        let second_job_id = enqueue_scheduled_job(
            state.as_ref(),
            "quota_sync",
            Some(&second_key_id),
            TRIGGER_SOURCE_SCHEDULER,
        )
        .await
        .expect("enqueue second quota sync");

        let running_snapshot_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let rows: Vec<(i64, String)> =
                sqlx::query_as("SELECT id, status FROM scheduled_jobs ORDER BY id ASC")
                    .fetch_all(&pool)
                    .await
                    .expect("fetch scheduled job statuses");
            let running_count = rows.iter().filter(|(_, status)| status == "running").count();
            let queued_count = rows.iter().filter(|(_, status)| status == "queued").count();
            let first_status = rows
                .iter()
                .find(|(id, _)| *id == first_job_id)
                .map(|(_, status)| status.as_str());
            let second_status = rows
                .iter()
                .find(|(id, _)| *id == second_job_id)
                .map(|(_, status)| status.as_str());
            if hits.load(Ordering::SeqCst) == 1
                && running_count == 1
                && queued_count == 1
                && first_status == Some("running")
                && second_status == Some("queued")
            {
                break;
            }
            assert!(
                std::time::Instant::now() < running_snapshot_deadline,
                "expected one running quota job and one queued quota job, rows={rows:?}, hits={}",
                hits.load(Ordering::SeqCst)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        release_tx.send(true).expect("release blocking usage server");
        let completion_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let rows: Vec<(i64, String)> =
                sqlx::query_as("SELECT id, status FROM scheduled_jobs ORDER BY id ASC")
                    .fetch_all(&pool)
                    .await
                    .expect("fetch completed scheduled job statuses");
            if rows.iter().all(|(_, status)| status == "success") {
                break;
            }
            assert!(
                std::time::Instant::now() < completion_deadline,
                "expected both quota jobs to finish successfully, rows={rows:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    }

    #[tokio::test]
    async fn maintenance_worker_can_finish_request_logs_gc_while_quota_sync_waits_on_remote_io() {
        let db_path = temp_db_path("maintenance-worker-request-logs-gc-during-quota-sync");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-block-a".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let pool = connect_sqlite_test_pool(&db_str).await;
        let key_id: String =
            sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = 'tvly-block-a' LIMIT 1")
                .fetch_one(&pool)
                .await
                .expect("fetch blocking quota sync key id");
        let (upstream_addr, hits, release_tx) = spawn_usage_blocking_mock_server().await;
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: true,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin: true,
            usage_base: format!("http://{upstream_addr}"),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });
        spawn_maintenance_worker(state.clone());

        let quota_job_id = enqueue_scheduled_job(
            state.as_ref(),
            "quota_sync",
            Some(&key_id),
            TRIGGER_SOURCE_SCHEDULER,
        )
        .await
        .expect("enqueue blocking quota sync");

        let quota_running_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let quota_job = state
                .proxy
                .scheduled_job_by_id(quota_job_id)
                .await
                .expect("fetch quota job")
                .expect("quota job row");
            if hits.load(Ordering::SeqCst) == 1 && quota_job.status == "running" {
                break;
            }
            assert!(
                std::time::Instant::now() < quota_running_deadline,
                "expected blocking quota sync job to enter running, status={}, hits={}",
                quota_job.status,
                hits.load(Ordering::SeqCst)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let gc_job_id = enqueue_scheduled_job(
            state.as_ref(),
            "request_logs_gc",
            None,
            TRIGGER_SOURCE_MANUAL,
        )
        .await
        .expect("enqueue request logs gc");
        let gc_completion_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let quota_job = state
                .proxy
                .scheduled_job_by_id(quota_job_id)
                .await
                .expect("fetch quota job while gc runs")
                .expect("quota job row while gc runs");
            let gc_job = state
                .proxy
                .scheduled_job_by_id(gc_job_id)
                .await
                .expect("fetch request logs gc job")
                .expect("request logs gc job row");
            if quota_job.status == "running" && gc_job.status == "success" {
                break;
            }
            assert!(
                std::time::Instant::now() < gc_completion_deadline,
                "expected request_logs_gc to finish while quota sync remains running, quota_status={}, gc_status={}",
                quota_job.status,
                gc_job.status
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        release_tx.send(true).expect("release blocking usage server");
        let quota_completion_deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let quota_job = state
                .proxy
                .scheduled_job_by_id(quota_job_id)
                .await
                .expect("fetch quota job after release")
                .expect("quota job row after release");
            if quota_job.status == "success" {
                break;
            }
            assert!(
                std::time::Instant::now() < quota_completion_deadline,
                "expected quota sync to finish after release, status={}",
                quota_job.status
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    }

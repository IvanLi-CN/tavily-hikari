    #[tokio::test]
    async fn api_keys_batch_does_not_override_existing_group() {
        let db_path = temp_db_path("keys-batch-group-no-override");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        // Existing key already belongs to a group.
        proxy
            .add_or_undelete_key_in_group("tvly-existing", Some("old"))
            .await
            .expect("existing key created in old group");

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

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({ "api_keys": ["tvly-existing"], "group": "new" }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary exists");
        assert_eq!(summary.get("existed").and_then(|v| v.as_u64()), Some(1));

        let group_name: Option<String> =
            sqlx::query_scalar("SELECT group_name FROM api_keys WHERE api_key = ?")
                .bind("tvly-existing")
                .fetch_one(&pool)
                .await
                .expect("tvly-existing exists");
        assert_eq!(
            group_name.as_deref(),
            Some("old"),
            "group_name should not be overridden for existing keys"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_structured_items_store_first_registration_metadata() {
        let db_path = temp_db_path("keys-batch-registration-metadata");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let geo_addr = spawn_api_key_geo_mock_server().await;

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server_with_geo_origin(
            proxy,
            forward_auth,
            false,
            format!("http://{geo_addr}/geo"),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    { "api_key": "tvly-registration-first", "registration_ip": "8.8.8.8" },
                    { "api_key": "tvly-registration-private", "registration_ip": "10.0.0.1" },
                    { "api_key": "tvly-registration-first", "registration_ip": "1.1.1.1" }
                ],
                "group": "ops"
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary exists");
        assert_eq!(summary.get("created").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(
            summary.get("duplicate_in_input").and_then(|v| v.as_u64()),
            Some(1)
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

        let first_row: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT registration_ip, registration_region FROM api_keys WHERE api_key = ?",
        )
        .bind("tvly-registration-first")
        .fetch_one(&pool)
        .await
        .expect("first key metadata");
        assert_eq!(first_row.0.as_deref(), Some("8.8.8.8"));
        assert_eq!(first_row.1.as_deref(), Some("US"));

        let private_row: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT registration_ip, registration_region FROM api_keys WHERE api_key = ?",
        )
        .bind("tvly-registration-private")
        .fetch_one(&pool)
        .await
        .expect("private key metadata");
        assert!(private_row.0.is_none(), "private ip should not be stored");
        assert!(private_row.1.is_none(), "private region should stay empty");

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_structured_items_persist_assigned_proxy_hint_without_registration_metadata()
     {
        let db_path = temp_db_path("keys-batch-assigned-proxy-hint");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
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
                },
                false,
            )
            .await
            .expect("proxy settings updated");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    {
                        "api_key": "tvly-assigned-proxy-hint",
                        "assigned_proxy_key": "http://1.1.1.1:8080"
                    }
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

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

        let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            r#"
            SELECT api_keys.registration_ip,
                   api_keys.registration_region,
                   forward_proxy_key_affinity.primary_proxy_key
              FROM api_keys
              LEFT JOIN forward_proxy_key_affinity
                ON forward_proxy_key_affinity.key_id = api_keys.id
             WHERE api_keys.api_key = ?
            "#,
        )
        .bind("tvly-assigned-proxy-hint")
        .fetch_one(&pool)
        .await
        .expect("hint-only key exists");
        assert!(
            row.0.is_none(),
            "hint-only batch import should not store registration_ip"
        );
        assert!(
            row.1.is_none(),
            "hint-only batch import should not fabricate registration_region"
        );
        assert_eq!(row.2.as_deref(), Some("http://1.1.1.1:8080"));

        let runtime_row: (String, i64) = sqlx::query_as(
            "SELECT resolved_ip_source, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
        )
        .bind("http://1.1.1.1:8080")
        .fetch_one(&pool)
        .await
        .expect("hint-only runtime row");
        assert!(runtime_row.0.is_empty());
        assert_eq!(runtime_row.1, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_registration_metadata_does_not_eagerly_prewarm_forward_proxy_geo_cache()
    {
        let db_path = temp_db_path("keys-batch-registration-geo-warm");
        let db_str = db_path.to_string_lossy().to_string();
        let geo_addr = spawn_api_key_geo_mock_server().await;

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec!["http://127.0.0.1:1".to_string()],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("proxy settings updated");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server_with_geo_origin(
            proxy,
            forward_auth,
            false,
            format!("http://{geo_addr}/geo"),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    { "api_key": "tvly-batch-geo-a", "registration_ip": "8.8.8.8" },
                    { "api_key": "tvly-batch-geo-b", "registration_ip": "1.1.1.1" }
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

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

        let runtime_row: (String, String, String, i64) = sqlx::query_as(
            "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
        )
        .bind("http://127.0.0.1:1")
        .fetch_one(&pool)
        .await
        .expect("registration batch runtime row");
        assert!(runtime_row.0.is_empty());
        assert_eq!(runtime_row.1, "[]");
        assert_eq!(runtime_row.2, "[]");
        assert_eq!(runtime_row.3, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn forward_proxy_geo_refresh_job_records_scheduled_job_and_skips_direct() {
        let db_path = temp_db_path("forward-proxy-geo-refresh-job");
        let db_str = db_path.to_string_lossy().to_string();
        let geo_addr = spawn_api_key_geo_mock_server().await;
        let traced_proxy_addr = spawn_fake_forward_proxy_with_body(
            StatusCode::OK,
            "ip=1.1.1.1
loc=US
colo=LAX
"
            .to_string(),
        )
        .await;
        let traced_proxy = format!("http://{traced_proxy_addr}");
        let dead_proxy = "http://127.0.0.1:1".to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![traced_proxy.clone(), dead_proxy.clone()],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: true,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("proxy settings updated");

        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: format!("http://{geo_addr}/geo"),
        });

        run_forward_proxy_geo_refresh_job(state.clone()).await;

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

        let job_row: (String, String, Option<String>) = sqlx::query_as(
            "SELECT job_type, status, message FROM scheduled_jobs ORDER BY id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .expect("load geo refresh job row");
        assert_eq!(job_row.0, "forward_proxy_geo_refresh");
        assert_eq!(job_row.1, "success");
        assert!(
            job_row
                .2
                .as_deref()
                .is_some_and(|message| message.contains("refreshed_candidates=2"))
        );

        let traced_row: (String, String, String, i64) = sqlx::query_as(
            "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
        )
        .bind(&traced_proxy)
        .fetch_one(&pool)
        .await
        .expect("load traced runtime row");
        assert_eq!(traced_row.0, "trace");
        assert_eq!(traced_row.1, "[\"1.1.1.1\"]");
        assert_eq!(traced_row.2, "[\"US Westfield (MA)\"]");
        assert!(traced_row.3 > 0);

        let dead_row: (String, String, String, i64) = sqlx::query_as(
            "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
        )
        .bind(&dead_proxy)
        .fetch_one(&pool)
        .await
        .expect("load dead runtime row");
        assert_eq!(dead_row.0, "negative");
        assert_eq!(dead_row.1, "[]");
        assert_eq!(dead_row.2, "[]");
        assert!(dead_row.3 > 0);

        let direct_row: (String, i64) = sqlx::query_as(
            "SELECT resolved_ip_source, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
        )
        .bind("__direct__")
        .fetch_one(&pool)
        .await
        .expect("load direct runtime row");
        assert!(direct_row.0.is_empty());
        assert_eq!(direct_row.1, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn forward_proxy_geo_refresh_scheduler_runs_immediately_before_24h_sleep() {
        let db_path = temp_db_path("forward-proxy-geo-refresh-scheduler-immediate");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec!["http://127.0.0.1:1".to_string()],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("proxy settings updated");

        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "http://127.0.0.1:9/geo".to_string(),
        });

        let handle = spawn_forward_proxy_geo_refresh_scheduler(state.clone());

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

        let mut saw_job = false;
        for _ in 0..30 {
            let row = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'forward_proxy_geo_refresh'",
            )
            .fetch_one(&pool)
            .await
            .expect("count geo refresh jobs");
            if row > 0 {
                saw_job = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        handle.abort();

        assert!(
            saw_job,
            "scheduler should run a GEO refresh immediately instead of waiting 24h for the first cycle"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn forward_proxy_geo_refresh_scheduler_rechecks_due_state_without_busy_looping() {
        let db_path = temp_db_path("forward-proxy-geo-refresh-scheduler-no-busy-loop");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy_url = "http://proxy.invalid:8080".to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![proxy_url.clone()],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("proxy settings updated");

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
            "UPDATE forward_proxy_runtime SET resolved_ip_source = 'trace', resolved_ips_json = '[\"127.0.0.1\"]', resolved_regions_json = '[]', geo_refreshed_at = ? WHERE proxy_key = ?",
        )
        .bind(chrono::Utc::now().timestamp())
        .bind(&proxy_url)
        .execute(&pool)
        .await
        .expect("seed due non-global trace runtime state");

        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "http://127.0.0.1:9/geo".to_string(),
        });

        let handle = spawn_forward_proxy_geo_refresh_scheduler(state.clone());

        let mut saw_job = false;
        for _ in 0..30 {
            let row = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'forward_proxy_geo_refresh'",
            )
            .fetch_one(&pool)
            .await
            .expect("count geo refresh jobs");
            if row > 0 {
                saw_job = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            saw_job,
            "due GEO state should still trigger a refresh job promptly"
        );

        tokio::time::sleep(Duration::from_millis(250)).await;
        handle.abort();

        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'forward_proxy_geo_refresh'",
        )
        .fetch_one(&pool)
        .await
        .expect("recount geo refresh jobs");
        assert_eq!(
            row, 1,
            "scheduler should wait for the recheck interval after a due run instead of busy-looping"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn forward_proxy_geo_refresh_scheduler_skips_immediate_run_when_runtime_is_fresh() {
        let db_path = temp_db_path("forward-proxy-geo-refresh-scheduler-fresh");
        let db_str = db_path.to_string_lossy().to_string();
        let geo_addr = spawn_api_key_geo_mock_server().await;
        let traced_proxy_addr = spawn_fake_forward_proxy_with_body(
            StatusCode::OK,
            "ip=1.1.1.1
loc=US
colo=LAX
"
            .to_string(),
        )
        .await;
        let traced_proxy = format!("http://{traced_proxy_addr}");

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
                ForwardProxySettings {
                    proxy_urls: vec![traced_proxy.clone()],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,
                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("proxy settings updated");
        proxy
            .refresh_forward_proxy_geo_metadata(&format!("http://{geo_addr}/geo"), true)
            .await
            .expect("seed fresh GEO runtime metadata");

        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: format!("http://{geo_addr}/geo"),
        });

        let handle = spawn_forward_proxy_geo_refresh_scheduler(state.clone());

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

        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();

        let row = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'forward_proxy_geo_refresh'",
        )
        .fetch_one(&pool)
        .await
        .expect("count geo refresh jobs");
        assert_eq!(
            row, 0,
            "fresh runtime GEO metadata should not trigger an extra startup refresh job"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_structured_items_ignore_stale_assigned_proxy_hint_without_registration_metadata()
     {
        let db_path = temp_db_path("keys-batch-stale-assigned-proxy-hint");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .update_forward_proxy_settings(
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
                },
                false,
            )
            .await
            .expect("proxy settings updated");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    {
                        "api_key": "tvly-stale-assigned-proxy-hint",
                        "assigned_proxy_key": "http://9.9.9.9:8080"
                    }
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

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

        let affinity_row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT forward_proxy_key_affinity.primary_proxy_key,
                   forward_proxy_key_affinity.secondary_proxy_key
              FROM api_keys
              LEFT JOIN forward_proxy_key_affinity
                ON forward_proxy_key_affinity.key_id = api_keys.id
             WHERE api_keys.api_key = ?
            "#,
        )
        .bind("tvly-stale-assigned-proxy-hint")
        .fetch_optional(&pool)
        .await
        .expect("stale hint-only key exists");
        assert!(
            affinity_row
                .as_ref()
                .is_some_and(|row| row.0.is_none() && row.1.is_none()),
            "stale assigned_proxy_key should not bind a fallback affinity row"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_structured_items_ignore_direct_assigned_proxy_hint_without_registration_metadata()
     {
        let db_path = temp_db_path("keys-batch-direct-assigned-proxy-hint");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server(proxy, forward_auth, false).await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    {
                        "api_key": "tvly-direct-assigned-proxy-hint",
                        "assigned_proxy_key": "__direct__"
                    }
                ]
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

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

        let affinity_row: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT forward_proxy_key_affinity.primary_proxy_key,
                   forward_proxy_key_affinity.secondary_proxy_key
              FROM api_keys
              LEFT JOIN forward_proxy_key_affinity
                ON forward_proxy_key_affinity.key_id = api_keys.id
             WHERE api_keys.api_key = ?
            "#,
        )
        .bind("tvly-direct-assigned-proxy-hint")
        .fetch_optional(&pool)
        .await
        .expect("direct hint-only key exists");
        assert!(
            affinity_row
                .as_ref()
                .is_some_and(|row| row.0.is_none() && row.1.is_none()),
            "direct validation hints should not become durable affinity rows"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn api_keys_batch_updates_existing_registration_metadata_without_overriding_group() {
        let db_path = temp_db_path("keys-batch-update-existing-registration");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .add_or_undelete_key_with_status_in_group_and_registration(
                "tvly-existing",
                Some("old"),
                Some("8.8.8.8"),
                Some("US"),
            )
            .await
            .expect("existing key created");
        let geo_addr = spawn_api_key_geo_mock_server().await;

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

        let forward_auth = ForwardAuthConfig::new(
            Some(HeaderName::from_static("x-forward-user")),
            Some("admin".to_string()),
            None,
            None,
        );
        let addr = spawn_keys_admin_server_with_geo_origin(
            proxy,
            forward_auth,
            false,
            format!("http://{geo_addr}/geo"),
        )
        .await;

        let client = Client::new();
        let url = format!("http://{}/api/keys/batch", addr);
        let resp = client
            .post(url)
            .header("x-forward-user", "admin")
            .json(&serde_json::json!({
                "items": [
                    { "api_key": "tvly-existing", "registration_ip": "1.1.1.1" }
                ],
                "group": "new"
            }))
            .send()
            .await
            .expect("request succeeds");

        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let body: serde_json::Value = resp.json().await.expect("parse json body");
        let summary = body.get("summary").expect("summary exists");
        assert_eq!(summary.get("existed").and_then(|v| v.as_u64()), Some(1));

        let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE api_key = ?",
        )
        .bind("tvly-existing")
        .fetch_one(&pool)
        .await
        .expect("existing key exists");
        assert_eq!(
            row.0.as_deref(),
            Some("old"),
            "group_name should not be overridden for existing keys"
        );
        assert_eq!(row.1.as_deref(), Some("1.1.1.1"));
        assert_eq!(row.2.as_deref(), Some("US Westfield (MA)"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn builtin_admin_login_allows_admin_endpoints_and_logout_revokes() {
        let db_path = temp_db_path("builtin-admin-login");
        let db_str = db_path.to_string_lossy().to_string();

        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let password = "pw-123";
        let addr = spawn_builtin_keys_admin_server(proxy, password).await;

        let client = Client::new();
        let keys_url = format!("http://{}/api/keys/batch", addr);

        let resp = client
            .post(&keys_url)
            .json(&serde_json::json!({ "api_keys": ["k1"] }))
            .send()
            .await
            .expect("request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let login_url = format!("http://{}/api/admin/login", addr);
        let resp = client
            .post(&login_url)
            .json(&serde_json::json!({ "password": "wrong" }))
            .send()
            .await
            .expect("login request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let resp = client
            .post(&login_url)
            .json(&serde_json::json!({ "password": password }))
            .send()
            .await
            .expect("login request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let set_cookie = resp
            .headers()
            .get(reqwest::header::SET_COOKIE)
            .expect("set-cookie header")
            .to_str()
            .expect("set-cookie header string");
        let cookie = set_cookie
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string();

        let resp = client
            .post(&keys_url)
            .header(reqwest::header::COOKIE, cookie.clone())
            .json(&serde_json::json!({ "api_keys": ["k1"] }))
            .send()
            .await
            .expect("request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);

        let logout_url = format!("http://{}/api/admin/logout", addr);
        let resp = client
            .post(&logout_url)
            .header(reqwest::header::COOKIE, cookie.clone())
            .send()
            .await
            .expect("logout request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

        let resp = client
            .post(&keys_url)
            .header(reqwest::header::COOKIE, cookie)
            .json(&serde_json::json!({ "api_keys": ["k2"] }))
            .send()
            .await
            .expect("request succeeds");
        assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn user_profile_and_user_token_reflect_linuxdo_session() {
        let db_path = temp_db_path("linuxdo-profile-token");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-user-1".to_string(),
                username: Some("linuxdo_alice".to_string()),
                name: Some("LinuxDO Alice".to_string()),
                avatar_template: Some(
                    "/user_avatar/connect.linux.do/linuxdo_alice/{size}/1_2.png".to_string(),
                ),
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert oauth user");
        let bound_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_alice"))
            .await
            .expect("ensure token binding");
        let session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create user session");

        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = "http://oauth.internal:3000/oauth2/authorize".to_string();
        oauth_options.userinfo_url = "http://discourse.internal:3000/api/user".to_string();

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let client = Client::new();

        let profile_url = format!("http://{}/api/profile", addr);
        let anonymous_profile_resp = client
            .get(&profile_url)
            .send()
            .await
            .expect("anonymous profile request");
        assert_eq!(anonymous_profile_resp.status(), reqwest::StatusCode::OK);
        let anonymous_profile: serde_json::Value = anonymous_profile_resp
            .json()
            .await
            .expect("anonymous profile json");
        assert_eq!(
            anonymous_profile.get("userLoggedIn"),
            Some(&serde_json::Value::Bool(false))
        );

        let user_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", session.token);
        let logged_in_profile_resp = client
            .get(&profile_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("logged-in profile request");
        assert_eq!(logged_in_profile_resp.status(), reqwest::StatusCode::OK);
        let logged_in_profile: serde_json::Value = logged_in_profile_resp
            .json()
            .await
            .expect("logged-in profile json");
        assert_eq!(
            logged_in_profile.get("userLoggedIn"),
            Some(&serde_json::Value::Bool(true))
        );
        assert_eq!(
            logged_in_profile.get("userProvider"),
            Some(&serde_json::Value::String("linuxdo".to_string()))
        );
        assert_eq!(
            logged_in_profile.get("userDisplayName"),
            Some(&serde_json::Value::String("LinuxDO Alice".to_string()))
        );
        assert_eq!(
            logged_in_profile.get("userAvatarUrl"),
            Some(&serde_json::Value::String(
                "https://connect.linux.do/user_avatar/connect.linux.do/linuxdo_alice/96/1_2.png"
                    .to_string(),
            ))
        );

        let token_url = format!("http://{}/api/user/token", addr);
        let unauth_resp = client
            .get(&token_url)
            .send()
            .await
            .expect("user token anonymous request");
        assert_eq!(unauth_resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let token_resp = client
            .get(&token_url)
            .header(reqwest::header::COOKIE, user_cookie)
            .send()
            .await
            .expect("user token request");
        assert_eq!(token_resp.status(), reqwest::StatusCode::OK);
        let token_body: serde_json::Value = token_resp.json().await.expect("user token json");
        assert_eq!(
            token_body.get("token").and_then(|value| value.as_str()),
            Some(bound_token.token.as_str())
        );

        let user_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", session.token);
        let no_redirect = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");
        let root_url = format!("http://{}/", addr);
        let root_resp = no_redirect
            .get(&root_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("root request with user session");
        assert_eq!(root_resp.status(), reqwest::StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(
            root_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/console")
        );

        let dashboard_url = format!("http://{}/api/user/dashboard", addr);
        let dashboard_resp = client
            .get(&dashboard_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user dashboard request");
        assert_eq!(dashboard_resp.status(), reqwest::StatusCode::OK);
        let dashboard_body: serde_json::Value =
            dashboard_resp.json().await.expect("user dashboard json");

        let tokens_url = format!("http://{}/api/user/tokens", addr);
        let tokens_resp = client
            .get(&tokens_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user tokens request");
        assert_eq!(tokens_resp.status(), reqwest::StatusCode::OK);
        let tokens_body: serde_json::Value = tokens_resp.json().await.expect("user tokens json");
        let items = tokens_body.as_array().expect("tokens response is array");
        assert_eq!(items.len(), 1);
        let first_item = items.first().expect("user token row");
        assert_eq!(
            dashboard_body
                .get("requestRate")
                .and_then(|value| value.get("limit"))
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit())
        );
        assert_eq!(
            dashboard_body
                .get("requestRate")
                .and_then(|value| value.get("windowMinutes"))
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit_window_minutes())
        );
        assert_eq!(
            dashboard_body
                .get("requestRate")
                .and_then(|value| value.get("scope"))
                .and_then(|value| value.as_str()),
            Some("user")
        );
        assert_eq!(
            dashboard_body
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64()),
            first_item
                .get("hourlyAnyLimit")
                .and_then(|value| value.as_i64())
        );
        assert_eq!(
            first_item
                .get("requestRate")
                .and_then(|value| value.get("limit"))
                .and_then(|value| value.as_i64()),
            Some(request_rate_limit())
        );
        assert_eq!(
            first_item
                .get("requestRate")
                .and_then(|value| value.get("scope"))
                .and_then(|value| value.as_str()),
            Some("user")
        );
        assert_eq!(
            first_item.get("tokenId").and_then(|value| value.as_str()),
            Some(bound_token.id.as_str())
        );

        let token_detail_url = format!("http://{}/api/user/tokens/{}", addr, bound_token.id);
        let token_detail_resp = client
            .get(&token_detail_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token detail request");
        assert_eq!(token_detail_resp.status(), reqwest::StatusCode::OK);

        let token_secret_url = format!("http://{}/api/user/tokens/{}/secret", addr, bound_token.id);
        let token_secret_resp = client
            .get(&token_secret_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token secret request");
        assert_eq!(token_secret_resp.status(), reqwest::StatusCode::OK);
        let token_secret_body: serde_json::Value = token_secret_resp
            .json()
            .await
            .expect("user token secret json");
        assert_eq!(
            token_secret_body
                .get("token")
                .and_then(|value| value.as_str()),
            Some(bound_token.token.as_str())
        );

        let token_logs_url = format!(
            "http://{}/api/user/tokens/{}/logs?limit=20",
            addr, bound_token.id
        );
        let token_logs_resp = client
            .get(&token_logs_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token logs request");
        assert_eq!(token_logs_resp.status(), reqwest::StatusCode::OK);

        let forbidden_detail_url = format!("http://{}/api/user/tokens/notmine", addr);
        let forbidden_detail_resp = client
            .get(&forbidden_detail_url)
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("forbidden token detail request");
        assert_eq!(
            forbidden_detail_resp.status(),
            reqwest::StatusCode::NOT_FOUND
        );

        let unauth_dashboard = client
            .get(&dashboard_url)
            .send()
            .await
            .expect("unauth dashboard request");
        assert_eq!(unauth_dashboard.status(), reqwest::StatusCode::UNAUTHORIZED);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn user_token_events_stream_snapshot_and_enforce_owner_scope() {
        let db_path = temp_db_path("linuxdo-user-token-events");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let owner = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-owner-1".to_string(),
                username: Some("owner".to_string()),
                name: Some("Owner".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("upsert owner");
        let outsider = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-outsider-1".to_string(),
                username: Some("outsider".to_string()),
                name: Some("Outsider".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("upsert outsider");

        let bound_token = proxy
            .ensure_user_token_binding(&owner.user_id, Some("linuxdo:owner"))
            .await
            .expect("ensure owner token binding");
        let owner_session = proxy
            .create_user_session(&owner, 3600)
            .await
            .expect("create owner session");
        let outsider_session = proxy
            .create_user_session(&outsider, 3600)
            .await
            .expect("create outsider session");

        let pool = connect_sqlite_test_pool(&db_str).await;
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
                created_at
            ) VALUES (?, 'POST', '/mcp', 'q=health', 200, 200, 'success', NULL, ?)
            "#,
        )
        .bind(&bound_token.id)
        .bind(Utc::now().timestamp())
        .execute(&pool)
        .await
        .expect("insert first auth token log");

        let addr = spawn_user_oauth_server(proxy).await;
        let client = Client::new();
        let owner_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", owner_session.token);
        let outsider_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", outsider_session.token);
        let events_url = format!("http://{}/api/user/tokens/{}/events", addr, bound_token.id);

        let anonymous = client
            .get(&events_url)
            .send()
            .await
            .expect("anonymous events request");
        assert_eq!(anonymous.status(), reqwest::StatusCode::UNAUTHORIZED);

        let outsider_resp = client
            .get(&events_url)
            .header(reqwest::header::COOKIE, outsider_cookie)
            .send()
            .await
            .expect("outsider events request");
        assert_eq!(outsider_resp.status(), reqwest::StatusCode::NOT_FOUND);

        let mut response = client
            .get(&events_url)
            .header(reqwest::header::COOKIE, owner_cookie)
            .send()
            .await
            .expect("owner events request");
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(content_type.contains("text/event-stream"));

        let first_snapshot_chunk = read_sse_event_until(
            &mut response,
            |chunk| chunk.contains("event: snapshot"),
            "owner token events initial snapshot",
        )
        .await;
        let first_snapshot_data = first_snapshot_chunk
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");
        let first_snapshot: serde_json::Value =
            serde_json::from_str(&first_snapshot_data).expect("decode first user token snapshot");
        assert_eq!(
            first_snapshot["token"]["tokenId"].as_str(),
            Some(bound_token.id.as_str())
        );
        assert_eq!(
            first_snapshot["logs"].as_array().map(Vec::len),
            Some(1),
            "initial snapshot should include the current recent logs",
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
                result_status,
                error_message,
                created_at
            ) VALUES (?, 'POST', '/api/tavily/search', 'q=live', 200, 200, 'success', NULL, ?)
            "#,
        )
        .bind(&bound_token.id)
        .bind(Utc::now().timestamp() + 1)
        .execute(&pool)
        .await
        .expect("insert second auth token log");

        let refreshed_snapshot_chunk = read_sse_event_until(
            &mut response,
            |chunk| chunk.contains("event: snapshot"),
            "owner token events refreshed snapshot",
        )
        .await;
        let refreshed_snapshot_data = refreshed_snapshot_chunk
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("\n");
        let refreshed_snapshot: serde_json::Value = serde_json::from_str(&refreshed_snapshot_data)
            .expect("decode refreshed user token snapshot");
        assert_eq!(
            refreshed_snapshot["logs"].as_array().map(Vec::len),
            Some(2),
            "refreshed snapshot should include the new token log",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn profile_exposes_allow_registration_setting() {
        let db_path = temp_db_path("linuxdo-profile-allow-registration");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .set_allow_registration(false)
            .await
            .expect("disable registration");

        let addr = spawn_user_oauth_server(proxy).await;
        let profile_resp = Client::new()
            .get(format!("http://{}/api/profile", addr))
            .send()
            .await
            .expect("profile request");
        assert_eq!(profile_resp.status(), reqwest::StatusCode::OK);
        let profile_body: serde_json::Value = profile_resp.json().await.expect("profile json");
        assert_eq!(
            profile_body.get("allowRegistration"),
            Some(&serde_json::Value::Bool(false))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn console_route_serves_spa_when_user_oauth_is_disabled() {
        let db_path = temp_db_path("console-route-disabled");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("create proxy");

        let addr =
            spawn_user_oauth_server_with_options(proxy, LinuxDoOAuthOptions::disabled()).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let console_resp = client
            .get(format!("http://{}/console", addr))
            .send()
            .await
            .expect("console request");
        assert_eq!(console_resp.status(), reqwest::StatusCode::OK);
        let console_html = console_resp.text().await.expect("console html");
        assert!(console_html.contains("<title>console</title>"));

        let profile_resp = client
            .get(format!("http://{}/api/profile", addr))
            .send()
            .await
            .expect("profile request");
        assert_eq!(profile_resp.status(), reqwest::StatusCode::OK);
        let profile_body: serde_json::Value = profile_resp.json().await.expect("profile json");
        assert!(profile_body.get("userLoggedIn").is_none());

        let dashboard_resp = client
            .get(format!("http://{}/api/user/dashboard", addr))
            .send()
            .await
            .expect("dashboard request");
        assert_eq!(dashboard_resp.status(), reqwest::StatusCode::NOT_FOUND);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn console_deep_link_route_serves_spa_when_user_oauth_is_disabled() {
        let db_path = temp_db_path("console-deep-link-disabled");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("create proxy");

        let addr =
            spawn_user_oauth_server_with_options(proxy, LinuxDoOAuthOptions::disabled()).await;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let console_resp = client
            .get(format!("http://{}/console/tokens/a1b2", addr))
            .send()
            .await
            .expect("console deep-link request");
        assert_eq!(console_resp.status(), reqwest::StatusCode::OK);
        let console_html = console_resp.text().await.expect("console deep-link html");
        assert!(console_html.contains("<title>console</title>"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn registration_paused_route_serves_dedicated_spa() {
        let db_path = temp_db_path("registration-paused-route");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("create proxy");

        let addr = spawn_user_oauth_server(proxy).await;
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
        assert!(html.contains("<title>registration-paused</title>"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn registration_paused_route_falls_back_to_index_when_dedicated_spa_is_missing() {
        let db_path = temp_db_path("registration-paused-route-fallback");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("create proxy");
        let static_dir = temp_static_dir("registration-paused-fallback");
        std::fs::remove_file(static_dir.join("registration-paused.html"))
            .expect("remove dedicated registration paused spa");
        let state = Arc::new(AppState {
            proxy,
            static_dir: Some(static_dir),
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: linuxdo_oauth_options_for_test(),
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
    async fn post_linuxdo_auth_persists_preferred_token_id_in_oauth_state() {
        let db_path = temp_db_path("linuxdo-auth-post-preferred-token");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let preferred = proxy
            .create_access_token(Some("linuxdo:preferred"))
            .await
            .expect("create preferred token");

        let addr = spawn_user_oauth_server(proxy).await;
        let no_redirect = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_url = format!("http://{}/auth/linuxdo", addr);
        let response = no_redirect
            .post(&auth_url)
            .form(&[("token", preferred.token.clone())])
            .send()
            .await
            .expect("post linuxdo auth");

        assert_eq!(response.status(), reqwest::StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("location header");
        let location_url = reqwest::Url::parse(location).expect("parse redirect location");
        let state_value = location_url
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("state query param");

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

        let (bind_token_id,): (Option<String>,) =
            sqlx::query_as("SELECT bind_token_id FROM oauth_login_states WHERE state = ? LIMIT 1")
                .bind(state_value)
                .fetch_one(&pool)
                .await
                .expect("query oauth state");
        assert_eq!(
            bind_token_id.as_deref(),
            Some(preferred.id.as_str()),
            "preferred token id should be persisted in oauth state"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn post_linuxdo_auth_follow_redirect_uses_get_method() {
        let db_path = temp_db_path("linuxdo-auth-post-follow-redirect-method");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let preferred = proxy
            .create_access_token(Some("linuxdo:preferred"))
            .await
            .expect("create preferred token");

        let method_probe = Arc::new(Mutex::new(None));
        let oauth_upstream =
            spawn_linuxdo_authorize_method_probe_server(method_probe.clone()).await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let client = Client::new();
        let auth_url = format!("http://{}/auth/linuxdo", addr);
        let response = client
            .post(&auth_url)
            .form(&[("token", preferred.token.clone())])
            .send()
            .await
            .expect("post linuxdo auth");

        assert_eq!(
            response.status(),
            reqwest::StatusCode::OK,
            "redirect follow should succeed when authorize endpoint receives GET"
        );
        assert_eq!(
            *method_probe.lock().expect("method probe lock poisoned"),
            Some(Method::GET),
            "authorize endpoint should be called with GET (303 See Other redirect)"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_binds_preferred_without_unbinding_existing_end_to_end() {
        let db_path = temp_db_path("linuxdo-callback-rebind-preferred-e2e");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-e2e-user".to_string(),
                username: Some("linuxdo_e2e".to_string()),
                name: Some("LinuxDO E2E".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let preferred = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_e2e"))
            .await
            .expect("create preferred binding");
        let mistaken = proxy
            .create_access_token(Some("linuxdo:mistaken"))
            .await
            .expect("create mistaken token");

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
            "UPDATE user_token_bindings SET token_id = ?, updated_at = ? WHERE user_id = ?",
        )
        .bind(&mistaken.id)
        .bind(Utc::now().timestamp() - 30)
        .bind(&user.user_id)
        .execute(&pool)
        .await
        .expect("simulate mistaken historical binding");

        let oauth_upstream =
            spawn_linuxdo_oauth_mock_server("linuxdo-e2e-user", "linuxdo_e2e", "LinuxDO E2E").await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let no_redirect = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_url = format!("http://{}/auth/linuxdo", addr);
        let auth_resp = no_redirect
            .post(&auth_url)
            .form(&[("token", preferred.token.clone())])
            .send()
            .await
            .expect("start linuxdo oauth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);

        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_url = format!(
            "http://{}/auth/linuxdo/callback?code=e2e-code&state={state}",
            addr
        );
        let callback_resp = no_redirect
            .get(&callback_url)
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            callback_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/console")
        );

        let user_cookie = find_cookie_pair(callback_resp.headers(), USER_SESSION_COOKIE_NAME)
            .expect("user session cookie");
        let token_resp = Client::new()
            .get(format!("http://{}/api/user/token", addr))
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("get user token");
        assert_eq!(token_resp.status(), reqwest::StatusCode::OK);
        let token_body: serde_json::Value = token_resp.json().await.expect("token body");
        assert_eq!(
            token_body.get("token").and_then(|value| value.as_str()),
            Some(preferred.token.as_str())
        );

        let (binding_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
                .bind(&user.user_id)
                .fetch_one(&pool)
                .await
                .expect("count user bindings");
        assert_eq!(
            binding_count, 2,
            "preferred token should be added while keeping existing bound token"
        );

        let preferred_owner = sqlx::query_scalar::<_, Option<String>>(
            "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
        )
        .bind(&preferred.id)
        .fetch_optional(&pool)
        .await
        .expect("query preferred owner")
        .flatten();
        assert_eq!(
            preferred_owner.as_deref(),
            Some(user.user_id.as_str()),
            "preferred token should belong to the current user"
        );

        let mistaken_owner = sqlx::query_scalar::<_, Option<String>>(
            "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
        )
        .bind(&mistaken.id)
        .fetch_optional(&pool)
        .await
        .expect("query mistaken owner")
        .flatten();
        assert_eq!(
            mistaken_owner.as_deref(),
            Some(user.user_id.as_str()),
            "existing token should remain bound to the same user"
        );

        let tokens_resp = Client::new()
            .get(format!("http://{}/api/user/tokens", addr))
            .header(reqwest::header::COOKIE, user_cookie)
            .send()
            .await
            .expect("get user tokens");
        assert_eq!(tokens_resp.status(), reqwest::StatusCode::OK);
        let token_items: Vec<serde_json::Value> =
            tokens_resp.json().await.expect("token list body");
        let token_ids: std::collections::HashSet<String> = token_items
            .into_iter()
            .filter_map(|item| {
                item.get("tokenId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .collect();
        assert!(
            token_ids.contains(&preferred.id),
            "preferred token should appear in user token list"
        );
        assert!(
            token_ids.contains(&mistaken.id),
            "existing token should stay in user token list"
        );

        let (enabled, deleted_at): (i64, Option<i64>) =
            sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ? LIMIT 1")
                .bind(&mistaken.id)
                .fetch_one(&pool)
                .await
                .expect("query mistaken token state");
        assert_eq!(enabled, 1, "mistaken token should stay active");
        assert!(
            deleted_at.is_none(),
            "mistaken token should stay non-deleted"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_redirects_first_time_user_when_registration_is_disabled() {
        let db_path = temp_db_path("linuxdo-callback-registration-paused-new-user");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .set_allow_registration(false)
            .await
            .expect("disable registration");

        let oauth_upstream = spawn_linuxdo_oauth_mock_server(
            "linuxdo-new-user",
            "linuxdo_new_user",
            "LinuxDO New User",
        )
        .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_resp = client
            .get(format!("http://{}/auth/linuxdo", addr))
            .send()
            .await
            .expect("start linuxdo auth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);

        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_resp = client
            .get(format!(
                "http://{}/auth/linuxdo/callback?code=e2e-code&state={state}",
                addr
            ))
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            callback_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/registration-paused")
        );
        let clear_binding_cookie =
            find_cookie_pair(callback_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
                .expect("cleared binding cookie");
        assert!(
            clear_binding_cookie.ends_with('='),
            "expected oauth binding cookie to be cleared"
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
        let oauth_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM oauth_accounts")
            .fetch_one(&pool)
            .await
            .expect("count oauth accounts");
        let session_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_sessions")
            .fetch_one(&pool)
            .await
            .expect("count user sessions");
        let binding_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_token_bindings")
            .fetch_one(&pool)
            .await
            .expect("count token bindings");
        assert_eq!(oauth_count, 0);
        assert_eq!(session_count, 0);
        assert_eq!(binding_count, 0);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_keeps_paused_route_when_registration_page_is_missing() {
        let db_path = temp_db_path("linuxdo-callback-registration-paused-fallback");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .set_allow_registration(false)
            .await
            .expect("disable registration");

        let oauth_upstream = spawn_linuxdo_oauth_mock_server(
            "linuxdo-new-user-fallback",
            "linuxdo_new_user_fallback",
            "LinuxDO New User Fallback",
        )
        .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let static_dir = temp_static_dir("linuxdo-user-oauth-fallback");
        std::fs::remove_file(static_dir.join("registration-paused.html"))
            .expect("remove dedicated paused page");
        let state = Arc::new(AppState {
            proxy,
            static_dir: Some(static_dir),
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            linuxdo_oauth: oauth_options,
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
        });

        let app = Router::new()
            .route("/", get(serve_index))
            .route(
                "/auth/linuxdo",
                get(get_linuxdo_auth).post(post_linuxdo_auth),
            )
            .route("/auth/linuxdo/callback", get(get_linuxdo_callback))
            .route("/api/profile", get(get_profile))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service())
                .await
                .unwrap();
        });

        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_resp = client
            .get(format!("http://{}/auth/linuxdo", addr))
            .send()
            .await
            .expect("start linuxdo auth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);

        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_resp = client
            .get(format!(
                "http://{}/auth/linuxdo/callback?code=e2e-code&state={state}",
                addr
            ))
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            callback_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/registration-paused")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn linuxdo_callback_allows_existing_user_when_registration_is_disabled() {
        let db_path = temp_db_path("linuxdo-callback-registration-paused-existing-user");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-existing-user".to_string(),
                username: Some("linuxdo_existing".to_string()),
                name: Some("LinuxDO Existing".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(2),
                raw_payload_json: None,
            })
            .await
            .expect("seed existing user");
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_existing"))
            .await
            .expect("seed token binding");
        proxy
            .set_allow_registration(false)
            .await
            .expect("disable registration");

        let oauth_upstream = spawn_linuxdo_oauth_mock_server(
            "linuxdo-existing-user",
            "linuxdo_existing",
            "LinuxDO Existing",
        )
        .await;
        let mut oauth_options = linuxdo_oauth_options_for_test();
        oauth_options.authorize_url = format!("http://{oauth_upstream}/oauth2/authorize");
        oauth_options.token_url = format!("http://{oauth_upstream}/oauth2/token");
        oauth_options.userinfo_url = format!("http://{oauth_upstream}/api/user");

        let addr = spawn_user_oauth_server_with_options(proxy, oauth_options).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("build no-redirect client");

        let auth_resp = client
            .get(format!("http://{}/auth/linuxdo", addr))
            .send()
            .await
            .expect("start linuxdo auth");
        assert_eq!(auth_resp.status(), reqwest::StatusCode::SEE_OTHER);

        let location = auth_resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .expect("auth redirect location");
        let state = reqwest::Url::parse(location)
            .expect("parse redirect url")
            .query_pairs()
            .find_map(|(k, v)| (k == "state").then(|| v.into_owned()))
            .expect("oauth state");
        let binding_cookie = find_cookie_pair(auth_resp.headers(), OAUTH_LOGIN_BINDING_COOKIE_NAME)
            .expect("oauth binding cookie");

        let callback_resp = client
            .get(format!(
                "http://{}/auth/linuxdo/callback?code=e2e-code&state={state}",
                addr
            ))
            .header(reqwest::header::COOKIE, binding_cookie)
            .send()
            .await
            .expect("oauth callback");
        assert_eq!(
            callback_resp.status(),
            reqwest::StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            callback_resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok()),
            Some("/console")
        );
        let user_cookie = find_cookie_pair(callback_resp.headers(), USER_SESSION_COOKIE_NAME)
            .expect("user session cookie");
        assert!(
            user_cookie.starts_with(&format!("{USER_SESSION_COOKIE_NAME}=")),
            "expected existing user login to create a user session"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn revoking_user_sessions_does_not_break_builtin_admin_session() {
        let db_path = temp_db_path("user-session-revoke-vs-admin-session");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");

        let user = proxy
            .upsert_oauth_account(&OAuthAccountProfile {
                provider: "linuxdo".to_string(),
                provider_user_id: "linuxdo-revoke-user".to_string(),
                username: Some("linuxdo_revoke".to_string()),
                name: Some("LinuxDO Revoke".to_string()),
                avatar_template: None,
                active: true,
                trust_level: Some(1),
                raw_payload_json: None,
            })
            .await
            .expect("seed oauth account");
        let _user_token = proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:linuxdo_revoke"))
            .await
            .expect("ensure user token");
        let user_session = proxy
            .create_user_session(&user, 3600)
            .await
            .expect("create user session");

        let user_addr = spawn_user_oauth_server(proxy.clone()).await;
        let admin_password = "pw-user-revoke-admin";
        let admin_addr = spawn_builtin_keys_admin_server(proxy.clone(), admin_password).await;
        let client = Client::new();

        let user_cookie = format!("{USER_SESSION_COOKIE_NAME}={}", user_session.token);
        let before_user_resp = client
            .get(format!("http://{}/api/user/token", user_addr))
            .header(reqwest::header::COOKIE, user_cookie.clone())
            .send()
            .await
            .expect("user token before revoke");
        assert_eq!(before_user_resp.status(), reqwest::StatusCode::OK);

        let login_resp = client
            .post(format!("http://{}/api/admin/login", admin_addr))
            .json(&serde_json::json!({ "password": admin_password }))
            .send()
            .await
            .expect("admin login");
        assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
        let admin_cookie = find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME)
            .expect("admin session cookie");

        let admin_before_resp = client
            .post(format!("http://{}/api/keys/batch", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie.clone())
            .json(&serde_json::json!({ "api_keys": ["k-user-revoke-admin"] }))
            .send()
            .await
            .expect("admin endpoint before revoke");
        assert_eq!(admin_before_resp.status(), reqwest::StatusCode::OK);

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
        sqlx::query("UPDATE user_sessions SET revoked_at = ? WHERE revoked_at IS NULL")
            .bind(Utc::now().timestamp())
            .execute(&pool)
            .await
            .expect("revoke user sessions");

        let after_user_resp = client
            .get(format!("http://{}/api/user/token", user_addr))
            .header(reqwest::header::COOKIE, user_cookie)
            .send()
            .await
            .expect("user token after revoke");
        assert_eq!(after_user_resp.status(), reqwest::StatusCode::UNAUTHORIZED);

        let admin_after_resp = client
            .post(format!("http://{}/api/keys/batch", admin_addr))
            .header(reqwest::header::COOKIE, admin_cookie)
            .json(&serde_json::json!({ "api_keys": ["k-user-revoke-admin-2"] }))
            .send()
            .await
            .expect("admin endpoint after revoke");
        assert_eq!(admin_after_resp.status(), reqwest::StatusCode::OK);

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn public_token_log_view_keeps_original_field_shape_and_appends_guidance() {
        let record = TokenLogRecord {
            id: 1,
            key_id: Some("MZli".to_string()),
            method: "POST".to_string(),
            path: "/mcp".to_string(),
            query: Some("token=secret".to_string()),
            http_status: Some(200),
            mcp_status: Some(429),
            business_credits: None,
            request_kind_key: "mcp:search".to_string(),
            request_kind_label: "MCP | search".to_string(),
            request_kind_detail: None,
            counts_business_quota: true,
            result_status: "error".to_string(),
            error_message: Some("Search failed".to_string()),
            failure_kind: Some("upstream_rate_limited_429".to_string()),
            key_effect_code: "none".to_string(),
            key_effect_summary: None,
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
            created_at: 1_700_000_000,
        };
        let view = PublicTokenLogView::from_record(record.clone(), UiLanguage::En);

        let json = serde_json::to_value(&view).expect("serialize public token log view");
        let object = json
            .as_object()
            .expect("public token log should serialize to object");
        assert!(object.get("failureKind").is_none());
        assert!(object.get("keyEffectCode").is_none());
        assert!(object.get("keyEffectSummary").is_none());
        assert!(
            object
                .get("errorMessage")
                .and_then(|value| value.as_str())
                .is_some_and(|value| value.contains("Suggested handling: Tavily is rate limiting")),
        );

        let zh_view = PublicTokenLogView::from_record(record, UiLanguage::Zh);
        assert!(
            serde_json::to_value(&zh_view)
                .ok()
                .and_then(|value| value
                    .get("errorMessage")
                    .and_then(|inner| inner.as_str())
                    .map(str::to_string))
                .is_some_and(|value| value.contains("建议：这是 Tavily 限流")),
        );
    }


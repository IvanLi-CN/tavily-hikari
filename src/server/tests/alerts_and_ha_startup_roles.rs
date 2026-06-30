use super::*;
use super::core_support_and_parsing::*;

#[tokio::test]
async fn standby_server_startup_does_not_spawn_business_scheduled_jobs() {
    let db_path = temp_db_path("ha-standby-no-business-schedulers");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-standby-no-business-schedulers".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
            mode: tavily_hikari::HaMode::ActiveStandby,
            node_id: "node-standby-startup".to_string(),
            database_path: Some(db_str.clone()),
            sync_source_url: Some("http://127.0.0.1:59999".to_string()),
            internal_token: Some("ha-test-token".to_string()),
            sync_interval_secs: 5,
            ..tavily_hikari::HaConfig::default()
        }),
        dev_open_admin: true,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });

    let spawned = spawn_background_tasks_for_current_role(state).await;
    assert!(!spawned, "standby role must skip business background tasks");

    let queued_jobs = proxy
        .fetch_queued_scheduled_jobs(32)
        .await
        .expect("fetch queued jobs");
    assert!(
        queued_jobs.is_empty(),
        "standby startup must not enqueue business scheduled jobs: {:?}",
        queued_jobs
            .iter()
            .map(|job| (job.job_type.clone(), job.trigger_source.clone()))
            .collect::<Vec<_>>()
    );

    let pool = connect_sqlite_test_pool(&db_str).await;
    let status: Vec<(String, String)> =
        sqlx::query_as("SELECT job_type, status FROM scheduled_jobs ORDER BY id ASC")
            .fetch_all(&pool)
            .await
            .expect("query scheduled jobs after standby startup");
    assert!(
        status.is_empty(),
        "standby startup must not create scheduled job rows, got {:?}",
        status
    );
    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn startup_restores_persisted_ha_source_settings_before_role_check() {
    let edgeone_app = Router::new().fallback(post(|| async {
        Json(serde_json::json!({
            "Response": {
                "AccelerationDomains": [
                    {
                        "OriginDetail": {
                            "Origin": "gz.ivanli.cc",
                            "OriginProtocol": "HTTPS",
                            "HttpsOriginPort": 1443
                        }
                    }
                ],
                "RequestId": "edgeone-startup-persisted-source"
            }
        }))
    }));
    let edgeone_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let edgeone_addr = edgeone_listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(edgeone_listener, edgeone_app.into_make_service())
            .await
            .unwrap();
    });

    let db_path = temp_db_path("ha-startup-persisted-source-settings");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-startup-persisted-source".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let persisted_source_settings = tavily_hikari::HaSourceSettingsView {
        source_kind: tavily_hikari::HaSourceKind::Direct,
        direct_origin_scheme: Some(tavily_hikari::OriginScheme::Https),
        direct_origin_host: Some("gz.ivanli.cc".to_string()),
        direct_origin_port: Some(1443),
        origin_group_id: None,
        target: Some("gz.ivanli.cc:1443".to_string()),
    };
    proxy
        .persist_ha_node_state(
            "gz-101",
            tavily_hikari::HaNodeRole::FullMaster,
            Some("gz.ivanli.cc:1443"),
            Some(&persisted_source_settings),
            None,
        )
        .await
        .expect("persist previous active state");
    proxy
        .flush_ha_state_writes()
        .await
        .expect("flush previous active state");

    let ha = tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "gz-101".to_string(),
        database_path: Some(db_str.clone()),
        node_public_scheme: Some("https".to_string()),
        node_public_host: Some("gz.ivanli.cc".to_string()),
        node_public_port: Some(443),
        edgeone_zone_id: Some("zone-test".to_string()),
        edgeone_domain: Some("hikari.example.test".to_string()),
        edgeone_expected_origin_scheme: Some("https".to_string()),
        edgeone_expected_origin_host: Some("gz.ivanli.cc".to_string()),
        edgeone_expected_origin_port: Some(443),
        edgeone_secret_id: Some("secret-id".to_string()),
        edgeone_secret_key: Some("secret-key".to_string()),
        edgeone_api_endpoint: format!("http://{edgeone_addr}"),
        ..tavily_hikari::HaConfig::default()
    });

    let status = initialize_ha_startup_state(&proxy, &ha).await;
    assert_eq!(status.role, tavily_hikari::HaNodeRole::FullMaster);
    assert_eq!(status.edgeone_origin.as_deref(), Some("gz.ivanli.cc:1443"));
    assert_eq!(status.edgeone_expected_origin.as_deref(), Some("gz.ivanli.cc:443"));
    assert_eq!(status.edgeone_expected_target.as_deref(), Some("gz.ivanli.cc:1443"));
    assert_eq!(
        status
            .ha_source_effective
            .as_ref()
            .and_then(|settings| settings.target.as_deref()),
        Some("gz.ivanli.cc:1443")
    );
    assert!(status.recovery_status.is_none());

    let pool = connect_sqlite_test_pool(&db_str).await;
    let row: (String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT role, ha_direct_origin_port, edgeone_origin FROM ha_node_state WHERE id = 'local'",
    )
    .fetch_one(&pool)
    .await
    .expect("read persisted startup state");
    assert_eq!(row.0, "full_master");
    assert_eq!(row.1, Some(1443));
    assert_eq!(row.2.as_deref(), Some("gz.ivanli.cc:1443"));
    pool.close().await;

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn persist_ha_status_snapshot_spawns_post_ready_pressure_rebuild_for_serving_roles() {
    let db_path = temp_db_path("ha-post-ready-pressure-rebuild");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-post-ready-pressure-rebuild".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "ha-post-ready-rebuild".to_string(),
            username: Some("ha_post_ready_rebuild".to_string()),
            name: Some("HA Post Ready Rebuild".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert post-ready rebuild user");
    let pool = connect_sqlite_test_pool(&db_str).await;
    let now = proxy.backend_time().now_ts();
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            method,
            path,
            status_code,
            tavily_status_code,
            result_status,
            request_kind_key,
            request_kind_label,
            counts_business_quota,
            request_user_id,
            upstream_operation,
            visibility,
            created_at
        ) VALUES ('POST', '/api/tavily/search', 200, 200, ?, 'api:search', 'API | search', 1, ?, ?, ?, ?)
        "#,
    )
    .bind("success")
    .bind(&user.user_id)
    .bind("search")
    .bind(tavily_hikari::REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(now - 60)
    .execute(&pool)
    .await
    .expect("insert pressure request log for post-ready rebuild");

    let state = Arc::new(AppState {
        proxy: proxy.clone(),
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
            mode: tavily_hikari::HaMode::ActiveStandby,
            node_id: "node-post-ready-rebuild".to_string(),
            database_path: Some(db_str.clone()),
            ..tavily_hikari::HaConfig::default()
        }),
        dev_open_admin: true,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });
    let status = tavily_hikari::HaStatusView {
        mode: tavily_hikari::HaMode::ActiveStandby,
        node_id: "node-post-ready-rebuild".to_string(),
        node_public_origin: None,
        role: tavily_hikari::HaNodeRole::FullMaster,
        degraded: false,
        allows_basic_business: true,
        allows_full_writes: true,
        edgeone_domain: None,
        edgeone_origin: None,
        edgeone_expected_origin: None,
        edgeone_current_target: None,
        edgeone_expected_target: None,
        edgeone_current_source_kind: None,
        edgeone_expected_source_kind: None,
        edgeone_current_origin_group_id: None,
        edgeone_expected_origin_group_id: None,
        ha_source_defaults: None,
        ha_source_override: None,
        ha_source_effective: None,
        edgeone_api_configured: false,
        last_edgeone_check_at: None,
        last_sync_at: None,
        sync_lag_seconds: None,
        recovery_status: None,
        message: Some("promoted into business-serving role".to_string()),
        peer_nodes: Vec::new(),
        planned_cutover_eligible: false,
    };

    persist_ha_status_snapshot(&state, &status)
        .await
        .expect("persist serving HA status snapshot");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let bucket_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM observability.server_pressure_buckets WHERE bucket_kind = 'five_minute'",
            )
            .fetch_one(&pool)
            .await
            .expect("count rebuilt server pressure buckets");
            if bucket_count >= 1 {
                return bucket_count;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("serving HA snapshot should trigger background pressure rebuild");

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let summary = proxy
                .user_dashboard_summary(&user.user_id, None)
                .await
                .expect("load user dashboard summary after post-ready backfill");
            if summary.business_calls_1h.total_count == 1 {
                return summary;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("serving HA snapshot should trigger background business-call backfill");

    pool.close().await;
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

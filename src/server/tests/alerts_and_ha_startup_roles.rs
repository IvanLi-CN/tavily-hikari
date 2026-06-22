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

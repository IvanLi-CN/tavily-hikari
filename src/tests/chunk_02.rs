#[tokio::test]
async fn add_or_undelete_key_with_status_keeps_tx_clean_after_insert_failure() {
    let db_path = temp_db_path("api-key-upsert-clean-tx-after-failure");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

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
        CREATE TRIGGER fail_insert_api_key
        BEFORE INSERT ON api_keys
        WHEN NEW.api_key = 'tvly-force-fail'
        BEGIN
            SELECT RAISE(ABORT, 'boom');
        END;
        "#,
    )
    .execute(&pool)
    .await
    .expect("create fail trigger");

    let first_err = proxy
        .add_or_undelete_key_with_status_in_group("tvly-force-fail", Some("team-a"))
        .await
        .expect_err("first key should fail due to trigger");
    assert!(
        first_err.to_string().contains("boom"),
        "error should include trigger message"
    );

    let (second_id, second_status) = proxy
        .add_or_undelete_key_with_status_in_group("tvly-after-failure", Some("team-a"))
        .await
        .expect("second key insert should not be polluted by previous failure");
    assert_eq!(second_status, ApiKeyUpsertStatus::Created);
    assert!(!second_id.is_empty(), "second key id must be present");

    let inserted_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE api_key = 'tvly-after-failure'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count inserted keys");
    assert_eq!(
        inserted_count, 1,
        "follow-up insert must succeed even after previous tx failure"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_status_refreshes_existing_registration_metadata_only() {
    let db_path = temp_db_path("api-key-upsert-refresh-registration");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-existing",
            Some("old"),
            Some("8.8.8.8"),
            Some("US"),
        )
        .await
        .expect("existing key created");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let (same_key_id, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-existing",
            Some("new"),
            Some("8.8.4.4"),
            Some("US Westfield (MA)"),
        )
        .await
        .expect("existing key refreshed");
    assert_eq!(same_key_id, key_id);
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch refreshed key");
    assert_eq!(row.0.as_deref(), Some("old"));
    assert_eq!(row.1.as_deref(), Some("8.8.4.4"));
    assert_eq!(row.2.as_deref(), Some("US Westfield (MA)"));

    proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-existing",
            None,
            Some("2606:4700:4700::1111"),
            None,
        )
        .await
        .expect("existing key refreshed to empty region");

    let refreshed_row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch refreshed key after region clear");
    assert_eq!(refreshed_row.0.as_deref(), Some("old"));
    assert_eq!(refreshed_row.1.as_deref(), Some("2606:4700:4700::1111"));
    assert!(
        refreshed_row.2.is_none(),
        "region should clear when the new registration ip has no resolved region"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_for_registration_prefers_exact_ip_then_region() {
    let db_path = temp_db_path("proxy-affinity-registration-selection");
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
                    "http://8.8.8.8:8080".to_string(),
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

    let (exact, exact_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:exact",
            &geo_origin,
            Some("18.183.246.69"),
            Some("JP Tokyo (13)"),
            None,
        )
        .await
        .expect("exact proxy affinity");
    assert_eq!(
        exact.primary_proxy_key.as_deref(),
        Some("http://18.183.246.69:8080"),
        "exact IP match should win before region matching"
    );
    assert_eq!(
        exact_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp),
        "exact IP selections should expose registration_ip match kind"
    );

    let (region, region_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:region",
            &geo_origin,
            Some("103.232.214.107"),
            Some("HK"),
            None,
        )
        .await
        .expect("region proxy affinity");
    assert_eq!(
        region.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080"),
        "same-region proxy should win when no exact IP node exists"
    );
    assert_eq!(
        region_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::SameRegion),
        "same-region selections should expose same_region match kind"
    );

    let (fallback, fallback_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:fallback",
            &geo_origin,
            Some("103.232.214.107"),
            Some("ZZ"),
            None,
        )
        .await
        .expect("fallback proxy affinity");
    assert!(
        fallback.primary_proxy_key.is_some(),
        "selection should still fall back to a selectable proxy node"
    );
    assert_eq!(
        fallback_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other),
        "fallback selections should expose other match kind"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_persists_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-persist");
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

    let (record, _preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:persist-runtime-geo",
            &geo_origin,
            Some("8.8.8.8"),
            Some("HK"),
            None,
        )
        .await
        .expect("registration-aware affinity");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load persisted runtime geo metadata");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode persisted resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode persisted resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_persists_forward_proxy_runtime_geo_metadata_from_trace_exit_ip() {
    let db_path = temp_db_path("proxy-runtime-geo-persist-trace-exit");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let fake_proxy_addr =
        spawn_fake_forward_proxy_with_body("ip=1.1.1.1\nloc=US\ncolo=LAX\n".to_string()).await;
    let proxy_url = format!("http://{fake_proxy_addr}");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:persist-runtime-geo-trace-exit",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("registration-aware affinity from trace exit ip");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some(proxy_url.as_str())
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load persisted trace-driven runtime geo metadata");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode persisted trace resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode persisted trace resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_persists_forward_proxy_runtime_geo_metadata_for_xray_route() {
    let db_path = temp_db_path("proxy-runtime-geo-persist-xray");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let raw_proxy_url =
        "vless://0688fa59-e971-4278-8c03-4b35821a71dc@1.1.1.1:443?encryption=none#hk";
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![raw_proxy_url.to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let endpoint = manager
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.raw_url.as_deref() == Some(raw_proxy_url))
            .expect("xray endpoint");
        let endpoint_key = endpoint.key.clone();
        let route_url = Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route");
        endpoint.endpoint_url = Some(route_url.clone());
        let runtime = manager
            .runtime
            .get_mut(&endpoint_key)
            .expect("xray runtime state");
        runtime.endpoint_url = Some(route_url.to_string());
        runtime.available = true;
        runtime.last_error = None;
    }

    let (record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:persist-runtime-geo-xray",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("registration-aware affinity for xray route");
    let primary = record.primary_proxy_key.expect("primary proxy key");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&primary)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load persisted runtime geo metadata for xray route");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode persisted resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode persisted resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_reuses_persisted_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-reuse");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
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
    .normalized();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
    }

    proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:seed-runtime-geo",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("seed persisted runtime geo metadata");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }

    let (record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:reuse-runtime-geo",
            "http://127.0.0.1:9/geo",
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should reuse persisted runtime geo metadata");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn update_forward_proxy_settings_rejects_invalid_egress_socks5_url() {
    let db_path = temp_db_path("invalid-egress-socks5-url");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let result = proxy
        .update_forward_proxy_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: true,
                egress_socks5_enabled: true,
                egress_socks5_url: "socks5h://user:pass@127".to_string(),
            },
            true,
        )
        .await;

    match result {
        Err(ProxyError::Other(message)) => {
            assert!(
                message.contains("valid socks5:// or socks5h:// URL"),
                "unexpected validation error: {message}",
            );
        }
        other => panic!("expected invalid egress socks5 URL to be rejected, got {other:?}"),
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_refreshes_incomplete_persisted_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-incomplete");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
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
    .normalized();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
    }

    proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:seed-runtime-geo-incomplete",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("seed persisted runtime geo metadata");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_regions_json = '[]', geo_refreshed_at = 0 WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .execute(&proxy.key_store.pool)
    .await
    .expect("clear persisted runtime regions");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }

    let (_record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:refresh-runtime-geo-incomplete",
            &geo_origin,
            None,
            Some("HK"),
            None,
        )
        .await
        .expect("selection should refresh incomplete persisted runtime geo metadata");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::SameRegion)
    );

    let row: String = sqlx::query_scalar(
        "SELECT resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load refreshed runtime region metadata");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row).expect("decode refreshed resolved regions");
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_refreshes_legacy_host_based_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-legacy-host");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
        proxy_urls: vec!["http://1.1.1.1:8080".to_string()],
        subscription_urls: Vec::new(),
        subscription_update_interval_secs: 3600,
        insert_direct: false,

        egress_socks5_enabled: false,
        egress_socks5_url: String::new(),
    }
    .normalized();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
    }

    proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:seed-runtime-geo-legacy",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("seed persisted runtime geo metadata");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ips_json = '[\"1.1.1.1\"]', resolved_regions_json = '[\"HK\"]', resolved_ip_source = '' WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy host-based runtime geo metadata");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }
    reloaded
        .set_forward_proxy_trace_override_for_test(
            "http://1.1.1.1:8080",
            "1.0.0.1",
            "TEST / 1.0.0.1",
        )
        .await;

    let (record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:refresh-runtime-geo-legacy",
            &geo_origin,
            Some("1.0.0.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should refresh legacy host-based runtime geo metadata");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String, String) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load refreshed legacy runtime geo metadata");
    assert_eq!(row.0, "trace".to_string());
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.1).expect("decode refreshed legacy resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.2).expect("decode refreshed legacy resolved regions");
    assert_eq!(resolved_ips, vec!["1.0.0.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_does_not_match_legacy_host_based_runtime_geo_when_trace_fails() {
    let db_path = temp_db_path("proxy-runtime-geo-ignore-legacy-host");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://127.0.0.1:1".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
        proxy_urls: vec![proxy_url.clone()],
        subscription_urls: Vec::new(),
        subscription_update_interval_secs: 3600,
        insert_direct: false,

        egress_socks5_enabled: false,
        egress_socks5_url: String::new(),
    }
    .normalized();
    let endpoint_key = {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
        manager
            .endpoints
            .iter()
            .find(|endpoint| {
                endpoint.endpoint_url.as_ref().map(Url::to_string) == Some(proxy_url.clone())
                    || endpoint.key == proxy_url
            })
            .map(|endpoint| endpoint.key.clone())
            .unwrap_or_else(|| proxy_url.clone())
    };
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&endpoint_key)
            .cloned()
            .expect("persisted runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist initial runtime state");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ips_json = '[\"1.1.1.1\"]', resolved_regions_json = '[\"HK\"]', resolved_ip_source = '' WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy host-based runtime geo metadata");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }

    let (record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:ignore-runtime-geo-legacy",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should ignore legacy host-based runtime geo metadata");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some(endpoint_key.as_str())
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other)
    );

    let row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load stale runtime source");
    assert_eq!(row.0, "negative");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.1).expect("decode negative resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.2).expect("decode negative resolved regions");
    assert!(resolved_ips.is_empty());
    assert!(resolved_regions.is_empty());
    assert!(
        row.3 > 0,
        "trace failures should persist a negative geo placeholder timestamp"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_reuses_negative_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-reuse-negative");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://127.0.0.1:1".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (_first_record, first_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:negative-cache-first",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("first selection should persist negative placeholder");
    assert_eq!(
        first_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other)
    );

    let first_row: (String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load first negative runtime row");
    assert_eq!(first_row.0, "negative");
    assert!(first_row.1 > 0);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let (_second_record, second_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:negative-cache-second",
            "http://127.0.0.1:9/geo",
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("second selection should reuse negative placeholder");
    assert_eq!(
        second_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other)
    );

    let second_row: (String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load second negative runtime row");
    assert_eq!(second_row.0, "negative");
    assert_eq!(
        second_row.1, first_row.1,
        "negative GEO placeholders should be reused without retracing on each request"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_retries_stale_negative_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-retry-stale-negative");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "negative".to_string();
        runtime.resolved_ips = Vec::new();
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at =
            Utc::now().timestamp() - (FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS + 1);
    }
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&proxy_url)
            .cloned()
            .expect("persisted runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist stale negative runtime state");
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.1.1.1", "TEST / 1.1.1.1")
        .await;

    let (_record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:retry-stale-negative-cache",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should retry stale negative placeholders");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String, String) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed runtime row");
    assert_eq!(row.0, "trace");
    assert_eq!(row.1, "[\"1.1.1.1\"]");
    assert_eq!(row.2, "[\"HK\"]");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_marks_trace_without_region_as_retriable_trace_cache() {
    let db_path = temp_db_path("proxy-runtime-geo-trace-without-region");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "8.8.8.8", "TEST / 8.8.8.8")
        .await;

    let (_first_record, first_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:trace-without-region-first",
            "http://127.0.0.1:9/geo",
            Some("8.8.8.8"),
            Some("HK"),
            None,
        )
        .await
        .expect("first selection should persist retriable trace cache when GEO lookup is empty");
    assert_eq!(
        first_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let first_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load retriable trace cache row");
    assert_eq!(first_row.0, "trace");
    assert_eq!(first_row.1, "[\"8.8.8.8\"]");
    assert_eq!(first_row.2, "[]");
    assert!(first_row.3 > 0);

    proxy
        .forward_proxy_trace_overrides
        .lock()
        .await
        .remove(&proxy_url);
    tokio::time::sleep(Duration::from_secs(1)).await;

    let (_second_record, second_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:trace-without-region-second",
            "http://127.0.0.1:9/geo",
            Some("8.8.8.8"),
            Some("HK"),
            None,
        )
        .await
        .expect("second selection should reuse cached trace IPs when GEO lookup stays empty");
    assert_eq!(
        second_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let second_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("reload retriable trace cache row");
    assert_eq!(second_row.0, "trace");
    assert_eq!(second_row.1, "[\"8.8.8.8\"]");
    assert_eq!(second_row.2, "[]");
    assert_eq!(
        second_row.3, first_row.3,
        "region lookup retries should reuse cached trace IPs without rerunning trace"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn persist_forward_proxy_geo_candidates_preserves_runtime_health_columns() {
    let db_path = temp_db_path("proxy-runtime-geo-preserve-health");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://1.1.1.1:8080".to_string();

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

    let endpoint = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key == proxy_url)
            .cloned()
            .expect("forward proxy endpoint")
    };

    sqlx::query(
        "UPDATE forward_proxy_runtime SET weight = 9.25, success_ema = 0.11, latency_ema_ms = 321.0, consecutive_failures = 7, is_penalized = 1 WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed updated runtime health metrics");

    proxy
        .persist_forward_proxy_geo_candidates(&[ForwardProxyGeoCandidate {
            endpoint,
            host_ips: vec!["1.1.1.1".to_string()],
            regions: vec!["HK".to_string()],
            source: ForwardProxyGeoSource::Trace,
            geo_refreshed_at: Utc::now().timestamp(),
        }])
        .await
        .expect("persist geo metadata only");

    let row: (f64, f64, Option<f64>, i64, i64, String, String, String) = sqlx::query_as(
        "SELECT weight, success_ema, latency_ema_ms, consecutive_failures, is_penalized, resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load runtime row after geo-only update");
    assert_eq!(row.0, 9.25);
    assert_eq!(row.1, 0.11);
    assert_eq!(row.2, Some(321.0));
    assert_eq!(row.3, 7);
    assert_eq!(row.4, 1);
    assert_eq!(row.5, "trace");
    assert_eq!(row.6, "[\"1.1.1.1\"]");
    assert_eq!(row.7, "[\"HK\"]");

    let manager = proxy.forward_proxy.lock().await;
    let runtime = manager
        .runtime(&proxy_url)
        .expect("runtime state should still exist");
    assert!(runtime.weight.is_finite());
    assert_eq!(runtime.resolved_ip_source, "trace");
    assert_eq!(runtime.resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(runtime.resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_keeps_in_memory_match_when_runtime_geo_persist_fails() {
    let db_path = temp_db_path("proxy-runtime-geo-persist-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.0.0.1", "TEST / 1.0.0.1")
        .await;

    sqlx::query("DROP TABLE forward_proxy_runtime")
        .execute(&proxy.key_store.pool)
        .await
        .expect("drop runtime table to force persist failure");

    let (record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:runtime-geo-persist-fallback",
            &geo_origin,
            Some("1.0.0.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should still succeed when runtime geo persistence fails");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some(proxy_url.as_str())
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    proxy
        .forward_proxy_trace_overrides
        .lock()
        .await
        .remove(&proxy_url);

    let (_cached_record, cached_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:runtime-geo-persist-fallback-reuse",
            "http://127.0.0.1:9/geo",
            Some("1.0.0.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should retain in-memory GEO cache after persist failure");
    assert_eq!(
        cached_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn record_forward_proxy_attempt_preserves_geo_metadata_written_by_other_tasks() {
    let db_path = temp_db_path("proxy-runtime-geo-preserve-on-health-write");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://1.1.1.1:8080".to_string();
    let refreshed_at = Utc::now().timestamp();

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

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ip_source = 'trace', resolved_ips_json = '[\"1.1.1.1\"]', resolved_regions_json = '[\"HK\"]', geo_refreshed_at = ? WHERE proxy_key = ?",
    )
    .bind(refreshed_at)
    .bind(&proxy_url)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed fresher GEO metadata in store");

    proxy
        .record_forward_proxy_attempt_inner(&proxy_url, true, Some(12.0), None, false)
        .await
        .expect("record attempt should not clobber stored GEO metadata");

    let row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load runtime row after health-only persist");
    assert_eq!(row.0, "trace");
    assert_eq!(row.1, "[\"1.1.1.1\"]");
    assert_eq!(row.2, "[\"HK\"]");
    assert_eq!(row.3, refreshed_at);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn force_refresh_replaces_stale_trace_with_negative_placeholder() {
    let db_path = temp_db_path("proxy-runtime-geo-force-refresh-negative");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.0.0.1", "TEST / 1.0.0.1")
        .await;
    proxy
        .refresh_forward_proxy_geo_metadata(&geo_origin, true)
        .await
        .expect("first force refresh should persist trace metadata");

    let first_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load initial trace runtime row");
    assert_eq!(first_row.0, "trace");
    assert_eq!(first_row.1, "[\"1.0.0.1\"]");
    assert_eq!(first_row.2, "[\"HK\"]");
    assert!(first_row.3 > 0);

    proxy
        .forward_proxy_trace_overrides
        .lock()
        .await
        .remove(&proxy_url);
    tokio::time::sleep(Duration::from_secs(1)).await;

    proxy
        .refresh_forward_proxy_geo_metadata(&geo_origin, true)
        .await
        .expect("second force refresh should replace stale trace with negative placeholder");

    let second_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed negative runtime row");
    assert_eq!(second_row.0, "negative");
    assert_eq!(second_row.1, "[]");
    assert_eq!(second_row.2, "[]");
    assert!(
        second_row.3 > first_row.3,
        "force refresh failures should replace stale trace data with a fresh negative placeholder timestamp"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_retraces_non_global_trace_cache_without_regions() {
    let db_path = temp_db_path("proxy-runtime-geo-retrace-loopback-trace");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let bad_geo_origin = "http://127.0.0.1:9/geo";
    let refreshed_at = Utc::now().timestamp();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["127.0.0.1".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at = refreshed_at;
    }
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&proxy_url)
            .cloned()
            .expect("persisted runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist seeded runtime state");
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "8.8.8.8", "TEST / 8.8.8.8")
        .await;

    let (_record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:retrace-loopback-trace-cache",
            bad_geo_origin,
            Some("8.8.8.8"),
            Some("US"),
            None,
        )
        .await
        .expect("selection should retrace non-global cached trace IPs");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String, String) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed runtime row");
    assert_eq!(row.0, "trace");
    assert_eq!(row.1, "[\"8.8.8.8\"]");
    assert_eq!(row.2, "[]");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_tracks_remaining_ttl() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-ttl");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.1.1.1", "TEST / 1.1.1.1")
        .await;
    proxy
        .refresh_forward_proxy_geo_metadata(&geo_origin, true)
        .await
        .expect("seed fresh GEO runtime metadata");

    {
        let mut manager = proxy.forward_proxy.lock().await;
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.geo_refreshed_at = Utc::now().timestamp() - (max_age_secs - 5);
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert!(
        (0..=5).contains(&wait_secs),
        "scheduler should wait only the remaining TTL before the first 24h GEO refresh, got {wait_secs}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_backs_off_recent_incomplete_trace_cache() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-incomplete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["8.8.8.8".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at = Utc::now().timestamp();
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert!(
        (1..=FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS).contains(&wait_secs),
        "recent incomplete trace metadata should back off briefly instead of hot-looping, got {wait_secs}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_retries_stale_incomplete_trace_cache_immediately() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-stale-incomplete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["8.8.8.8".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at =
            Utc::now().timestamp() - (FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS + 1);
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert_eq!(
        wait_secs, 0,
        "stale incomplete trace metadata should be retried immediately once the cooldown expires"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_does_not_back_off_non_global_trace_cache() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-non-global");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["127.0.0.1".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at = Utc::now().timestamp();
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert_eq!(
        wait_secs, 0,
        "non-global incomplete trace metadata should be retried immediately instead of entering the cooldown"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_refreshes_stale_loopback_runtime_geo_metadata_for_xray_route() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-loopback-xray");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let raw_proxy_url =
        "vless://0688fa59-e971-4278-8c03-4b35821a71dc@1.1.1.1:443?encryption=none#hk";
    let endpoint_key = {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![raw_proxy_url.to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let endpoint = manager
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.raw_url.as_deref() == Some(raw_proxy_url))
            .expect("xray endpoint");
        let endpoint_key = endpoint.key.clone();
        let route_url = Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route");
        endpoint.endpoint_url = Some(route_url.clone());
        let runtime = manager
            .runtime
            .get_mut(&endpoint_key)
            .expect("xray runtime state");
        runtime.endpoint_url = Some(route_url.to_string());
        runtime.available = true;
        runtime.last_error = None;
        endpoint_key
    };
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&endpoint_key)
            .cloned()
            .expect("persisted xray runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist initial xray runtime state");

    let updated = sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ips_json = '[\"127.0.0.1\"]', resolved_regions_json = '[]' WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed stale loopback runtime geo metadata");
    assert_eq!(
        updated.rows_affected(),
        1,
        "should seed an existing runtime row"
    );

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![raw_proxy_url.to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let endpoint = manager
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.raw_url.as_deref() == Some(raw_proxy_url))
            .expect("reloaded xray endpoint");
        let route_url = Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route");
        endpoint.endpoint_url = Some(route_url.clone());
        let runtime = manager
            .runtime
            .get_mut(&endpoint_key)
            .expect("reloaded xray runtime state");
        runtime.endpoint_url = Some(route_url.to_string());
        runtime.available = true;
        runtime.last_error = None;
    }

    let (_record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:refresh-runtime-geo-loopback-xray",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should refresh stale loopback runtime geo metadata");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load refreshed xray runtime geo metadata");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode refreshed xray resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode refreshed xray resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_registration_proxy_affinity_persists_and_refreshes() {
    let db_path = temp_db_path("proxy-affinity-registration-persist");
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
            "tvly-affinity",
            Some("alpha"),
            Some("18.183.246.69"),
            Some("JP Tokyo (13)"),
            &geo_origin,
        )
        .await
        .expect("key created with proxy affinity");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let created_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load created affinity");
    assert_eq!(
        created_affinity.0.as_deref(),
        Some("http://18.183.246.69:8080")
    );

    let (same_key_id, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-affinity",
            Some("beta"),
            Some("1.1.1.1"),
            Some("HK"),
            &geo_origin,
        )
        .await
        .expect("key refreshed with new proxy affinity");
    assert_eq!(same_key_id, key_id);
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed key");
    assert_eq!(row.0.as_deref(), Some("alpha"));
    assert_eq!(row.1.as_deref(), Some("1.1.1.1"));
    assert_eq!(row.2.as_deref(), Some("HK"));

    let refreshed_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed affinity");
    assert_eq!(refreshed_affinity.0.as_deref(), Some("http://1.1.1.1:8080"));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_registration_proxy_affinity_hint_keeps_validation_fallback() {
    let db_path = temp_db_path("proxy-affinity-registration-hint");
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
                    "http://8.8.8.8:8080".to_string(),
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

    let (key_id, _) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hinted-fallback",
            None,
            Some("9.9.9.9"),
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hinted proxy affinity");

    let created_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load hinted affinity");
    assert_eq!(
        created_affinity.0.as_deref(),
        Some("http://1.1.1.1:8080"),
        "fallback imports should preserve the proxy chosen during validation"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn key_sticky_nodes_preview_is_read_only_but_uses_effective_assignment() {
    let db_path = temp_db_path("sticky-nodes-preview-read-only");
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

    let (key_id, _) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-sticky-preview",
            None,
            Some("18.183.246.69"),
            Some("JP Tokyo (13)"),
        )
        .await
        .expect("key created without persisted proxy affinity");

    let sticky_nodes = proxy
        .key_sticky_nodes(&key_id)
        .await
        .expect("load sticky node preview");
    assert_eq!(sticky_nodes.nodes.len(), 2);
    assert_eq!(sticky_nodes.nodes[0].role, "primary");
    assert_eq!(
        sticky_nodes.nodes[0].node.key, "http://18.183.246.69:8080",
        "preview should reflect the same effective primary node the request path would pick"
    );

    let persisted_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM forward_proxy_key_affinity WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count affinity rows");
    assert_eq!(
        persisted_count, 0,
        "admin sticky-node preview must not persist or mutate forward proxy affinity"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_persists_across_upsert_paths() {
    let db_path = temp_db_path("proxy-affinity-hint-only-upsert");
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
            "tvly-hint-only",
            Some("alpha"),
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hint-only affinity");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let created_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load created hint-only affinity");
    assert_eq!(created_affinity.0.as_deref(), Some("http://1.1.1.1:8080"));

    let (same_key_id, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-only",
            Some("beta"),
            None,
            None,
            &geo_origin,
            Some("http://18.183.246.69:8080"),
        )
        .await
        .expect("key refreshed with hint-only affinity");
    assert_eq!(same_key_id, key_id);
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let refreshed_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed hint-only affinity");
    assert_eq!(
        refreshed_affinity.0.as_deref(),
        Some("http://18.183.246.69:8080")
    );

    proxy
        .soft_delete_key_by_id(&key_id)
        .await
        .expect("soft delete key before undelete");

    let (_, undeleted_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-only",
            Some("gamma"),
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key undeleted with hint-only affinity");
    assert_eq!(undeleted_status, ApiKeyUpsertStatus::Undeleted);

    let undeleted_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load undeleted hint-only affinity");
    assert_eq!(undeleted_affinity.0.as_deref(), Some("http://1.1.1.1:8080"));

    let row: (Option<i64>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT deleted_at, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load undeleted key");
    assert!(row.0.is_none(), "undelete should clear deleted_at");
    assert!(
        row.1.is_none() && row.2.is_none(),
        "hint-only imports should not fabricate registration metadata"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_stale_hint_only_proxy_affinity_does_not_persist_fallback() {
    let db_path = temp_db_path("proxy-affinity-stale-hint-only");
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
            "tvly-stale-hint-only",
            None,
            None,
            None,
            &geo_origin,
            Some("http://9.9.9.9:8080"),
        )
        .await
        .expect("key created without persisting stale hint");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row");
    assert!(
        affinity_row.0.is_none() && affinity_row.1.is_none(),
        "stale hint-only imports must not silently bind a fallback node"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for stale hint-only key");
    assert!(
        !plan.is_empty(),
        "keys without durable affinity should still get a runtime fallback plan"
    );

    let affinity_row_after_plan: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row after building stale hint plan");
    assert!(
        affinity_row_after_plan.0.is_none() && affinity_row_after_plan.1.is_none(),
        "runtime fallback planning must not backfill durable affinity for stale hints"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_keeps_selected_node_when_temporarily_unavailable()
 {
    let db_path = temp_db_path("proxy-affinity-hint-only-unavailable");
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
        manager
            .runtime
            .get_mut("http://1.1.1.1:8080")
            .expect("runtime for selected node")
            .available = false;
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-unavailable",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with unavailable hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query persisted unavailable hint affinity");
    assert_eq!(affinity_row.0.as_deref(), Some("http://1.1.1.1:8080"));

    let reconciled = proxy
        .reconcile_proxy_affinity_record(&key_id)
        .await
        .expect("reconcile unavailable hint affinity");
    assert_eq!(
        reconciled.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080"),
        "temporary outages should not discard a caller-pinned hint-only primary"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for unavailable hint-only key");
    assert!(
        plan.iter()
            .all(|candidate| candidate.key != "http://1.1.1.1:8080"),
        "temporarily unavailable pinned nodes should stay durable but not be retried until healthy"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_does_not_route_through_zero_weight_primary()
 {
    let db_path = temp_db_path("proxy-affinity-hint-only-zero-weight");
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
        manager
            .runtime
            .get_mut("http://1.1.1.1:8080")
            .expect("runtime for selected node")
            .weight = 0.0;
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-zero-weight",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with zero-weight hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for zero-weight hint-only key");
    assert!(
        plan.iter()
            .all(|candidate| candidate.key != "http://1.1.1.1:8080"),
        "zero-weight pinned nodes should stay durable but not bypass routing weight gates"
    );

    let reconciled = proxy
        .reconcile_proxy_affinity_record(&key_id)
        .await
        .expect("reconcile zero-weight hint affinity");
    assert_eq!(
        reconciled.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080"),
        "runtime weight changes should not erase the stored hint-only primary"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_rebuilds_when_primary_disappears() {
    let db_path = temp_db_path("proxy-affinity-hint-only-rebuilds");
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
            "tvly-hint-rebuilds",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec!["http://18.183.246.69:8080".to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let reconciled = proxy
        .reconcile_proxy_affinity_record(&key_id)
        .await
        .expect("reconcile hint-only affinity after primary removal");
    assert_eq!(
        reconciled.primary_proxy_key.as_deref(),
        Some("http://18.183.246.69:8080"),
        "when a hinted primary disappears entirely, the key should heal onto a remaining candidate"
    );

    let _ = std::fs::remove_file(db_path);
}


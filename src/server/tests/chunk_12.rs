    #[tokio::test]
    async fn admin_system_settings_reject_invalid_affinity_count() {
        let db_path = temp_db_path("admin-system-settings-invalid");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .put(format!("http://{addr}/api/settings/system"))
            .json(&serde_json::json!({
                "mcpSessionAffinityKeyCount": 0,
            }))
            .send()
            .await
            .expect("update invalid system settings");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.expect("invalid body");
        assert!(
            body.contains("mcp_session_affinity_key_count"),
            "expected range validation error, got {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_system_settings_reject_invalid_rebalance_percent() {
        let db_path = temp_db_path("admin-system-settings-invalid-rebalance-percent");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .put(format!("http://{addr}/api/settings/system"))
            .json(&serde_json::json!({
                "mcpSessionAffinityKeyCount": 5,
                "rebalanceMcpEnabled": true,
                "rebalanceMcpSessionPercent": 101,
            }))
            .send()
            .await
            .expect("update invalid rebalance percent");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.expect("invalid body");
        assert!(
            body.contains("rebalance_mcp_session_percent"),
            "expected range validation error, got {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_system_settings_reject_invalid_request_rate_limit() {
        let db_path = temp_db_path("admin-system-settings-invalid-request-rate");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .put(format!("http://{addr}/api/settings/system"))
            .json(&serde_json::json!({
                "requestRateLimit": 0,
                "mcpSessionAffinityKeyCount": 5,
                "rebalanceMcpEnabled": false,
                "rebalanceMcpSessionPercent": 100,
            }))
            .send()
            .await
            .expect("update invalid request-rate limit");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response.text().await.expect("invalid body");
        assert!(
            body.contains("request_rate_limit"),
            "expected request-rate validation error, got {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_settings_and_stats_expose_persisted_geo_metadata() {
        let db_path = temp_db_path("admin-forward-proxy-geo-metadata");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let geo_addr = spawn_api_key_geo_mock_server().await;
        let fake_proxy_addr = spawn_fake_forward_proxy_with_body(
            StatusCode::OK,
            "ip=1.1.1.1\nloc=US\ncolo=LAX\n".to_string(),
        )
        .await;
        let _geo_origin_guard =
            EnvVarGuard::set("API_KEY_IP_GEO_ORIGIN", &format!("http://{geo_addr}/geo"));
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server_with_geo_origin(
            proxy,
            usage_base,
            true,
            "https://api.country.is".to_string(),
        )
        .await;

        let client = Client::new();
        let updated = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [format!("http://{}", fake_proxy_addr)],
                "subscriptionUrls": [],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
                "skipBootstrapProbe": true,
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
            updated_body["nodes"][0]["resolvedIps"][0].as_str(),
            Some("1.1.1.1")
        );
        assert_eq!(
            updated_body["nodes"][0]["resolvedRegions"][0].as_str(),
            Some("US Westfield (MA)")
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
        assert_eq!(
            stats_body["nodes"][0]["resolvedIps"][0].as_str(),
            Some("1.1.1.1")
        );
        assert_eq!(
            stats_body["nodes"][0]["resolvedRegions"][0].as_str(),
            Some("US Westfield (MA)")
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_proxy_accepts_reachable_404() {
        let db_path = temp_db_path("admin-forward-proxy-validate-proxy");
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
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .json(&serde_json::json!({
                "kind": "proxyUrl",
                "value": format!("http://{}", fake_proxy_addr),
            }))
            .send()
            .await
            .expect("validate proxy");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode validation");
        assert_eq!(body["ok"].as_bool(), Some(true));
        assert_eq!(body["discoveredNodes"].as_u64(), Some(1));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_proxy_returns_node_trace_metadata() {
        let db_path = temp_db_path("admin-forward-proxy-validate-proxy-trace");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let fake_proxy_addr = spawn_fake_forward_proxy_with_body(
            StatusCode::OK,
            "ip=203.0.113.8\nloc=JP\ncolo=NRT\n".to_string(),
        )
        .await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .json(&serde_json::json!({
                "kind": "proxyUrl",
                "value": format!("http://{}", fake_proxy_addr),
            }))
            .send()
            .await
            .expect("validate proxy");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode validation");
        assert_eq!(body["ok"].as_bool(), Some(true));
        let expected_display_name = fake_proxy_addr.to_string();
        assert_eq!(
            body["nodes"][0]["displayName"].as_str(),
            Some(expected_display_name.as_str())
        );
        assert_eq!(body["nodes"][0]["ip"].as_str(), Some("203.0.113.8"));
        assert_eq!(body["nodes"][0]["location"].as_str(), Some("JP / NRT"));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_proxy_streams_progress_events() {
        let db_path = temp_db_path("admin-forward-proxy-validate-proxy-sse");
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
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&serde_json::json!({
                "kind": "proxyUrl",
                "value": format!("http://{}", fake_proxy_addr),
            }))
            .send()
            .await
            .expect("validate proxy");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("read sse body");
        assert!(
            body.contains(
                "\"type\":\"phase\",\"operation\":\"validate\",\"phaseKey\":\"parse_input\""
            ),
            "expected parse_input phase, got: {body}"
        );
        assert!(
            body.contains(
                "\"type\":\"phase\",\"operation\":\"validate\",\"phaseKey\":\"probe_nodes\""
            ),
            "expected probe_nodes phase, got: {body}"
        );
        assert!(
            body.contains("\"type\":\"complete\",\"operation\":\"validate\""),
            "expected complete event, got: {body}"
        );
        assert!(
            body.contains("\"nodes\":["),
            "expected validation payload to include node rows, got: {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_subscription_accepts_reachable_404() {
        let db_path = temp_db_path("admin-forward-proxy-validate-subscription");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let fake_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let subscription_addr =
            spawn_forward_proxy_subscription_server(format!("http://{}\n", fake_proxy_addr)).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .json(&serde_json::json!({
                "kind": "subscriptionUrl",
                "value": format!("http://{}/subscription", subscription_addr),
            }))
            .send()
            .await
            .expect("validate subscription");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode validation");
        assert_eq!(body["ok"].as_bool(), Some(true));
        assert_eq!(body["discoveredNodes"].as_u64(), Some(1));
        assert_eq!(body["nodes"].as_array().map(Vec::len), Some(1));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_subscription_does_not_hang_on_stalled_trace_body() {
        let db_path = temp_db_path("admin-forward-proxy-validate-subscription-trace-timeout");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let fake_proxy_addr = spawn_fake_forward_proxy_with_stalled_body(StatusCode::OK).await;
        let subscription_addr =
            spawn_forward_proxy_subscription_server(format!("http://{}\n", fake_proxy_addr)).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .expect("build client");
        let started = std::time::Instant::now();
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .json(&serde_json::json!({
                "kind": "subscriptionUrl",
                "value": format!("http://{}/subscription", subscription_addr),
            }))
            .send()
            .await
            .expect("validate subscription");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode validation");
        assert_eq!(body["ok"].as_bool(), Some(true));
        assert_eq!(body["nodes"][0]["ok"].as_bool(), Some(true));
        assert!(body["nodes"][0]["ip"].is_null());
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "validation should finish even if trace body stalls; elapsed {:?}",
            started.elapsed()
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_subscription_probes_every_node_and_returns_all_rows() {
        let db_path = temp_db_path("admin-forward-proxy-validate-subscription-all-nodes");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let failing_proxy_addr = spawn_fake_forward_proxy(StatusCode::INTERNAL_SERVER_ERROR).await;
        let healthy_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let subscription_addr = spawn_forward_proxy_subscription_server(format!(
            "http://{}\nhttp://{}\n",
            failing_proxy_addr, healthy_proxy_addr
        ))
        .await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .json(&serde_json::json!({
                "kind": "subscriptionUrl",
                "value": format!("http://{}/subscription", subscription_addr),
            }))
            .send()
            .await
            .expect("validate subscription");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode validation");
        assert_eq!(body["ok"].as_bool(), Some(true));
        assert_eq!(body["discoveredNodes"].as_u64(), Some(2));
        let nodes = body["nodes"].as_array().expect("nodes array");
        assert_eq!(
            nodes.len(),
            2,
            "expected all probed nodes to be returned: {body}"
        );
        assert_eq!(
            nodes[0]["displayName"].as_str(),
            Some(failing_proxy_addr.to_string().as_str())
        );
        assert_eq!(nodes[0]["ok"].as_bool(), Some(false));
        assert_eq!(
            nodes[1]["displayName"].as_str(),
            Some(healthy_proxy_addr.to_string().as_str())
        );
        assert_eq!(nodes[1]["ok"].as_bool(), Some(true));

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_subscription_streams_full_probe_progress() {
        let db_path = temp_db_path("admin-forward-proxy-validate-subscription-sse");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let first_proxy_addr = spawn_fake_forward_proxy(StatusCode::INTERNAL_SERVER_ERROR).await;
        let second_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let subscription_addr = spawn_forward_proxy_subscription_server(format!(
            "http://{}\nhttp://{}\n",
            first_proxy_addr, second_proxy_addr
        ))
        .await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&serde_json::json!({
                "kind": "subscriptionUrl",
                "value": format!("http://{}/subscription", subscription_addr),
            }))
            .send()
            .await
            .expect("validate subscription");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("read sse body");
        assert!(
            body.contains("\"type\":\"nodes\",\"operation\":\"validate\",\"nodes\":["),
            "expected initial nodes event, got: {body}"
        );
        assert!(
            body.contains(
                "\"phaseKey\":\"probe_nodes\",\"label\":\"Probing nodes\",\"current\":1,\"total\":2"
            ),
            "expected first probe progress event, got: {body}"
        );
        assert!(
            body.contains(
                "\"phaseKey\":\"probe_nodes\",\"label\":\"Probing nodes\",\"current\":2,\"total\":2"
            ),
            "expected final probe progress event, got: {body}"
        );
        assert!(
            body.contains("\"complete\",\"operation\":\"validate\""),
            "expected completion event, got: {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_subscription_stops_after_client_disconnect() {
        let db_path = temp_db_path("admin-forward-proxy-validate-subscription-cancel");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let hit_count = Arc::new(AtomicUsize::new(0));
        let proxy_a = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(40),
            hit_count.clone(),
        )
        .await;
        let proxy_b = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(40),
            hit_count.clone(),
        )
        .await;
        let proxy_c = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(40),
            hit_count.clone(),
        )
        .await;
        let proxy_d = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(40),
            hit_count.clone(),
        )
        .await;
        let subscription_addr = spawn_forward_proxy_subscription_server(format!(
            "http://{}\nhttp://{}\nhttp://{}\nhttp://{}\n",
            proxy_a, proxy_b, proxy_c, proxy_d
        ))
        .await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let mut response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&serde_json::json!({
                "kind": "subscriptionUrl",
                "value": format!("http://{}/subscription", subscription_addr),
            }))
            .send()
            .await
            .expect("validate subscription");
        assert_eq!(response.status(), StatusCode::OK);
        let mut first_body = String::new();
        while !first_body.contains("\"type\":\"nodes\"") {
            let chunk = response
                .chunk()
                .await
                .expect("read sse chunk")
                .expect("expected sse chunk before disconnect");
            first_body.push_str(String::from_utf8_lossy(&chunk).as_ref());
        }
        assert!(
            first_body.contains("\"type\":\"nodes\""),
            "expected initial nodes event before disconnect, got: {first_body}"
        );
        drop(response);

        tokio::time::sleep(Duration::from_millis(700)).await;
        assert!(
            hit_count.load(Ordering::SeqCst) <= 3,
            "validation should stop shortly after disconnect; observed {} probe requests",
            hit_count.load(Ordering::SeqCst),
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_validate_subscription_surfaces_probe_failure_reason() {
        let db_path = temp_db_path("admin-forward-proxy-validate-subscription-failure");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let fake_proxy_addr = spawn_fake_forward_proxy(StatusCode::INTERNAL_SERVER_ERROR).await;
        let subscription_addr =
            spawn_forward_proxy_subscription_server(format!("http://{}\n", fake_proxy_addr)).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .post(format!("http://{addr}/api/settings/forward-proxy/validate"))
            .json(&serde_json::json!({
                "kind": "subscriptionUrl",
                "value": format!("http://{}/subscription", subscription_addr),
            }))
            .send()
            .await
            .expect("validate subscription");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode validation");
        assert_eq!(body["ok"].as_bool(), Some(false));
        let message = body["message"].as_str().unwrap_or_default();
        assert!(
            message.contains("subscription proxy probe failed"),
            "expected subscription probe failure context, got: {message}"
        );
        assert!(
            message.contains("validation probe returned status 500 Internal Server Error"),
            "expected concrete 500 failure context, got: {message}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_settings_allow_disabling_direct_for_manual_nodes() {
        let db_path = temp_db_path("admin-forward-proxy-no-direct");
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
        let response = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [format!("http://{}", fake_proxy_addr)],
                "subscriptionUrls": [],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("update settings");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode response");
        assert_eq!(body["insertDirect"].as_bool(), Some(false));
        let nodes = body["nodes"].as_array().expect("nodes array");
        assert_eq!(nodes.len(), 1, "direct should not be injected");
        assert!(
            nodes
                .iter()
                .all(|node| node["key"].as_str() != Some("__direct__"))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_settings_stream_progress_events() {
        let db_path = temp_db_path("admin-forward-proxy-settings-sse");
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
        let response = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&serde_json::json!({
                "proxyUrls": [format!("http://{}", fake_proxy_addr)],
                "subscriptionUrls": [],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": true,
            }))
            .send()
            .await
            .expect("stream settings update");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("read sse body");
        assert!(
            body.contains(
                "\"type\":\"phase\",\"operation\":\"save\",\"phaseKey\":\"save_settings\""
            ),
            "expected save_settings phase, got: {body}"
        );
        assert!(
            body.contains(
                "\"type\":\"phase\",\"operation\":\"save\",\"phaseKey\":\"bootstrap_probe\""
            ),
            "expected bootstrap_probe phase, got: {body}"
        );
        assert!(
            body.contains("\"type\":\"complete\",\"operation\":\"save\""),
            "expected complete event, got: {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_settings_can_skip_bootstrap_probe_after_validation() {
        let db_path = temp_db_path("admin-forward-proxy-settings-skip-bootstrap");
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
        let response = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&serde_json::json!({
                "proxyUrls": [format!("http://{}", fake_proxy_addr)],
                "subscriptionUrls": [],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": true,
                "skipBootstrapProbe": true,
            }))
            .send()
            .await
            .expect("stream settings update");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("read sse body");
        assert!(
            body.contains("\"phaseKey\":\"bootstrap_probe\""),
            "expected bootstrap phase marker, got: {body}"
        );
        assert!(
            body.contains("Skipped after recent validation"),
            "expected skipped bootstrap detail, got: {body}"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_incremental_subscription_add_only_refreshes_new_sources() {
        let db_path = temp_db_path("admin-forward-proxy-incremental-add");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let sub1_proxy_hits = Arc::new(AtomicUsize::new(0));
        let sub2_proxy_hits = Arc::new(AtomicUsize::new(0));
        let sub1_proxy_addr = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(0),
            sub1_proxy_hits.clone(),
        )
        .await;
        let sub2_proxy_addr = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(0),
            sub2_proxy_hits.clone(),
        )
        .await;
        let sub1_hits = Arc::new(AtomicUsize::new(0));
        let sub2_hits = Arc::new(AtomicUsize::new(0));
        let sub1_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", sub1_proxy_addr),
        )));
        let sub2_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", sub2_proxy_addr),
        )));
        let sub1_addr =
            spawn_counted_forward_proxy_subscription_server(sub1_state, sub1_hits.clone()).await;
        let sub2_addr =
            spawn_counted_forward_proxy_subscription_server(sub2_state, sub2_hits.clone()).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let first = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [],
                "subscriptionUrls": [format!("http://{}/subscription", sub1_addr)],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("seed first subscription");
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(sub1_hits.load(Ordering::SeqCst), 1);
        assert_eq!(sub2_hits.load(Ordering::SeqCst), 0);
        assert_eq!(sub1_proxy_hits.load(Ordering::SeqCst), 1);
        assert_eq!(sub2_proxy_hits.load(Ordering::SeqCst), 0);

        let second = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [],
                "subscriptionUrls": [
                    format!("http://{}/subscription", sub1_addr),
                    format!("http://{}/subscription", sub2_addr),
                ],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("append second subscription");
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(
            sub1_hits.load(Ordering::SeqCst),
            1,
            "unchanged subscription should not be refetched on add",
        );
        assert_eq!(sub2_hits.load(Ordering::SeqCst), 1);
        assert_eq!(
            sub1_proxy_hits.load(Ordering::SeqCst),
            1,
            "existing nodes should not be re-probed on add",
        );
        assert_eq!(sub2_proxy_hits.load(Ordering::SeqCst), 1);

        let stats = client
            .get(format!("http://{addr}/api/stats/forward-proxy"))
            .send()
            .await
            .expect("get stats");
        assert_eq!(stats.status(), StatusCode::OK);
        let body = stats
            .json::<serde_json::Value>()
            .await
            .expect("decode stats");
        let nodes = body["nodes"].as_array().expect("stats nodes");
        assert!(
            nodes
                .iter()
                .any(|node| node["endpointUrl"].as_str()
                    == Some(&format!("http://{sub1_proxy_addr}/"))),
            "first subscription node should remain active",
        );
        assert!(
            nodes
                .iter()
                .any(|node| node["endpointUrl"].as_str()
                    == Some(&format!("http://{sub2_proxy_addr}/"))),
            "new subscription node should become active immediately",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_interval_only_save_does_not_refresh_subscriptions() {
        let db_path = temp_db_path("admin-forward-proxy-interval-only");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let proxy_hits = Arc::new(AtomicUsize::new(0));
        let fake_proxy_addr = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(0),
            proxy_hits.clone(),
        )
        .await;
        let subscription_hits = Arc::new(AtomicUsize::new(0));
        let subscription_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", fake_proxy_addr),
        )));
        let subscription_addr = spawn_counted_forward_proxy_subscription_server(
            subscription_state,
            subscription_hits.clone(),
        )
        .await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let payload = serde_json::json!({
            "proxyUrls": [],
            "subscriptionUrls": [format!("http://{}/subscription", subscription_addr)],
            "subscriptionUpdateIntervalSecs": 3600,
            "insertDirect": false,
        });
        let first = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&payload)
            .send()
            .await
            .expect("seed subscription");
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);

        let second = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [],
                "subscriptionUrls": [format!("http://{}/subscription", subscription_addr)],
                "subscriptionUpdateIntervalSecs": 60,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("update interval only");
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_removing_subscription_drops_only_removed_nodes() {
        let db_path = temp_db_path("admin-forward-proxy-remove-subscription");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let sub1_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let sub2_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let sub1_hits = Arc::new(AtomicUsize::new(0));
        let sub2_hits = Arc::new(AtomicUsize::new(0));
        let sub1_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", sub1_proxy_addr),
        )));
        let sub2_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", sub2_proxy_addr),
        )));
        let sub1_addr =
            spawn_counted_forward_proxy_subscription_server(sub1_state, sub1_hits.clone()).await;
        let sub2_addr =
            spawn_counted_forward_proxy_subscription_server(sub2_state, sub2_hits.clone()).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let first = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [],
                "subscriptionUrls": [
                    format!("http://{}/subscription", sub1_addr),
                    format!("http://{}/subscription", sub2_addr),
                ],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("seed subscriptions");
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(sub1_hits.load(Ordering::SeqCst), 1);
        assert_eq!(sub2_hits.load(Ordering::SeqCst), 1);

        let second = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [],
                "subscriptionUrls": [format!("http://{}/subscription", sub2_addr)],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("remove first subscription");
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(sub1_hits.load(Ordering::SeqCst), 1);
        assert_eq!(sub2_hits.load(Ordering::SeqCst), 1);

        let stats = client
            .get(format!("http://{addr}/api/stats/forward-proxy"))
            .send()
            .await
            .expect("get stats");
        assert_eq!(stats.status(), StatusCode::OK);
        let body = stats
            .json::<serde_json::Value>()
            .await
            .expect("decode stats");
        let nodes = body["nodes"].as_array().expect("stats nodes");
        assert!(
            nodes
                .iter()
                .all(|node| node["endpointUrl"].as_str()
                    != Some(&format!("http://{sub1_proxy_addr}/"))),
            "removed subscription nodes should disappear immediately",
        );
        assert!(
            nodes
                .iter()
                .any(|node| node["endpointUrl"].as_str()
                    == Some(&format!("http://{sub2_proxy_addr}/"))),
            "unchanged subscription nodes should remain active",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_revalidate_refreshes_all_subscriptions_and_probes_all_nodes() {
        let db_path = temp_db_path("admin-forward-proxy-revalidate");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let subscription_proxy_hits = Arc::new(AtomicUsize::new(0));
        let manual_proxy_hits = Arc::new(AtomicUsize::new(0));
        let subscription_proxy_addr = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(0),
            subscription_proxy_hits.clone(),
        )
        .await;
        let manual_proxy_addr = spawn_counted_fake_forward_proxy(
            StatusCode::NOT_FOUND,
            Duration::from_millis(0),
            manual_proxy_hits.clone(),
        )
        .await;
        let subscription_hits = Arc::new(AtomicUsize::new(0));
        let subscription_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", subscription_proxy_addr),
        )));
        let subscription_addr = spawn_counted_forward_proxy_subscription_server(
            subscription_state,
            subscription_hits.clone(),
        )
        .await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let first = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [format!("http://{}", manual_proxy_addr)],
                "subscriptionUrls": [format!("http://{}/subscription", subscription_addr)],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": false,
            }))
            .send()
            .await
            .expect("seed proxy pool");
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(subscription_hits.load(Ordering::SeqCst), 1);
        assert_eq!(subscription_proxy_hits.load(Ordering::SeqCst), 1);
        assert_eq!(manual_proxy_hits.load(Ordering::SeqCst), 1);

        let response = client
            .post(format!(
                "http://{addr}/api/settings/forward-proxy/revalidate"
            ))
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("revalidate settings");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.text().await.expect("read sse body");
        assert!(
            body.contains("\"type\":\"phase\",\"operation\":\"revalidate\",\"phaseKey\":\"refresh_subscription\""),
            "expected refresh_subscription phase, got: {body}"
        );
        assert!(
            body.contains(
                "\"type\":\"phase\",\"operation\":\"revalidate\",\"phaseKey\":\"probe_nodes\""
            ),
            "expected probe_nodes phase, got: {body}"
        );
        assert!(
            body.contains("\"type\":\"complete\",\"operation\":\"revalidate\""),
            "expected revalidate completion event, got: {body}"
        );
        assert_eq!(subscription_hits.load(Ordering::SeqCst), 2);
        assert_eq!(subscription_proxy_hits.load(Ordering::SeqCst), 2);
        assert_eq!(manual_proxy_hits.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_failed_subscription_refresh_keeps_previous_nodes() {
        let db_path = temp_db_path("admin-forward-proxy-subscription-preserve");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let fake_proxy_addr = spawn_fake_forward_proxy(StatusCode::NOT_FOUND).await;
        let subscription_state = Arc::new(Mutex::new((
            StatusCode::OK,
            format!("http://{}\n", fake_proxy_addr),
        )));
        let subscription_addr =
            spawn_mutable_forward_proxy_subscription_server(subscription_state.clone()).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let payload = serde_json::json!({
            "proxyUrls": [],
            "subscriptionUrls": [format!("http://{}/subscription", subscription_addr)],
            "subscriptionUpdateIntervalSecs": 3600,
            "insertDirect": false,
        });

        let first = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&payload)
            .send()
            .await
            .expect("seed settings");
        assert_eq!(first.status(), StatusCode::OK);

        {
            let mut guard = subscription_state.lock().expect("subscription state lock");
            guard.0 = StatusCode::INTERNAL_SERVER_ERROR;
            guard.1 = "boom".to_string();
        }

        let failed = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&payload)
            .send()
            .await
            .expect("refresh settings");
        assert_eq!(failed.status(), StatusCode::OK);

        let stats = client
            .get(format!("http://{addr}/api/stats/forward-proxy"))
            .send()
            .await
            .expect("get stats");
        assert_eq!(stats.status(), StatusCode::OK);
        let body = stats
            .json::<serde_json::Value>()
            .await
            .expect("decode stats");
        let nodes = body["nodes"].as_array().expect("stats nodes");
        assert!(
            nodes
                .iter()
                .any(|node| node["endpointUrl"].as_str()
                    == Some(&format!("http://{fake_proxy_addr}/"))),
            "previously refreshed subscription node should remain active",
        );
        assert!(
            nodes
                .iter()
                .all(|node| node["key"].as_str() != Some("__direct__"))
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn admin_forward_proxy_settings_save_succeeds_when_subscription_refresh_fails() {
        let db_path = temp_db_path("admin-forward-proxy-subscription-save");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let subscription_state = Arc::new(Mutex::new((
            StatusCode::INTERNAL_SERVER_ERROR,
            "boom".to_string(),
        )));
        let subscription_addr =
            spawn_mutable_forward_proxy_subscription_server(subscription_state.clone()).await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let usage_base = format!("http://{}", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint::<Vec<String>, String>(Vec::new(), &upstream, &db_str)
                .await
                .expect("create proxy");
        let addr = spawn_admin_forward_proxy_server(proxy, usage_base, true).await;

        let client = Client::new();
        let response = client
            .put(format!("http://{addr}/api/settings/forward-proxy"))
            .json(&serde_json::json!({
                "proxyUrls": [],
                "subscriptionUrls": [format!("http://{}/subscription", subscription_addr)],
                "subscriptionUpdateIntervalSecs": 3600,
                "insertDirect": true,
            }))
            .send()
            .await
            .expect("save settings");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("decode response");
        assert_eq!(
            body["subscriptionUrls"]
                .as_array()
                .map(|values| values.len()),
            Some(1),
            "subscription setting should still persist",
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
        let nodes = stats_body["nodes"].as_array().expect("stats nodes");
        assert!(
            nodes
                .iter()
                .any(|node| node["key"].as_str() == Some("__direct__")),
            "direct fallback should remain available while subscription refresh is down",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn proxy_request_does_not_bypass_proxy_pool_when_direct_is_disabled() {
        let db_path = temp_db_path("forward-proxy-no-direct-fallback");
        let db_str = db_path.to_string_lossy().to_string();
        let upstream_addr = spawn_forward_proxy_probe_upstream().await;
        let upstream = format!("http://{}/mcp", upstream_addr);
        let proxy =
            TavilyProxy::with_endpoint(vec!["tvly-test-key".to_string()], &upstream, &db_str)
                .await
                .expect("create proxy");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead_addr = listener.local_addr().unwrap();
        drop(listener);

        proxy
            .update_forward_proxy_settings(
                tavily_hikari::ForwardProxySettings {
                    proxy_urls: vec![format!("http://{}", dead_addr)],
                    subscription_urls: Vec::new(),
                    subscription_update_interval_secs: 3600,
                    insert_direct: false,

                    egress_socks5_enabled: false,
                    egress_socks5_url: String::new(),
                },
                false,
            )
            .await
            .expect("disable direct fallback");

        let result = proxy
            .proxy_request(tavily_hikari::ProxyRequest {
                method: Method::GET,
                path: "/mcp".to_string(),
                query: None,
                headers: HeaderMap::new(),
                body: bytes::Bytes::new(),
                auth_token_id: None,
                prefer_mcp_session_affinity: false,
                pinned_api_key_id: None,
                gateway_mode: None,
                experiment_variant: None,
                proxy_session_id: None,
                routing_subject_hash: None,
                upstream_operation: None,
                fallback_reason: None,
            })
            .await;

        assert!(
            matches!(result, Err(ProxyError::Http(_)) | Err(ProxyError::Other(_))),
            "request should fail instead of silently falling back to direct",
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn extract_token_from_query_none_or_empty() {
        let (q, t) = extract_token_from_query(None);
        assert_eq!(q, None);
        assert_eq!(t, None);

        let (q, t) = extract_token_from_query(Some(""));
        assert_eq!(q, None);
        assert_eq!(t, None);
    }

    #[test]
    fn extract_token_from_query_single_param_case_insensitive() {
        let (q, t) = extract_token_from_query(Some("TavilyApiKey=th-abc-xyz"));
        assert_eq!(q, None, "no other params → query should be None");
        assert_eq!(t.as_deref(), Some("th-abc-xyz"));
    }

    #[test]
    fn extract_token_from_query_strips_param_and_preserves_others() {
        let (q, t) = extract_token_from_query(Some("foo=1&tavilyApiKey=th-abc-xyz&bar=2"));
        assert_eq!(t.as_deref(), Some("th-abc-xyz"));
        // Order should be preserved for non-auth params.
        assert_eq!(q.as_deref(), Some("foo=1&bar=2"));
    }

    #[test]
    fn extract_token_from_query_uses_first_non_empty_token() {
        let (q, t) =
            extract_token_from_query(Some("tavilyApiKey=&tavilyApiKey=th-abc-xyz&foo=bar"));
        assert_eq!(t.as_deref(), Some("th-abc-xyz"));
        assert_eq!(q.as_deref(), Some("foo=bar"));
    }

    #[test]
    fn extract_token_from_query_ignores_additional_token_params() {
        let (q, t) = extract_token_from_query(Some("tavilyApiKey=th-1&tavilyApiKey=th-2&foo=bar"));
        assert_eq!(t.as_deref(), Some("th-1"));
        assert_eq!(q.as_deref(), Some("foo=bar"));
    }


    #[tokio::test]
    async fn mcp_rebalance_tools_call_rejects_invalid_arguments_locally() {
        let db_path = temp_db_path("mcp-rebalance-invalid-tool-arguments");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-invalid-tool-arguments";
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
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-invalid-tool-arguments"))
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
                "id": "rebalance-invalid-args-init",
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

        let invalid_cases = vec![
            (
                "missing-arguments",
                json!({
                    "jsonrpc": "2.0",
                    "id": "missing-arguments",
                    "method": "tools/call",
                    "params": { "name": "tavily_search" }
                }),
                "tavily_search",
            ),
            (
                "non-object-arguments",
                json!({
                    "jsonrpc": "2.0",
                    "id": "non-object-arguments",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_search",
                        "arguments": "raw-search-args"
                    }
                }),
                "tavily_search",
            ),
            (
                "missing-query",
                json!({
                    "jsonrpc": "2.0",
                    "id": "missing-query",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_search",
                        "arguments": {}
                    }
                }),
                "tavily_search",
            ),
            (
                "bad-query-type",
                json!({
                    "jsonrpc": "2.0",
                    "id": "bad-query-type",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_search",
                        "arguments": { "query": 42 }
                    }
                }),
                "tavily_search",
            ),
            (
                "missing-urls",
                json!({
                    "jsonrpc": "2.0",
                    "id": "missing-urls",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_extract",
                        "arguments": {}
                    }
                }),
                "tavily_extract",
            ),
            (
                "bad-urls-type",
                json!({
                    "jsonrpc": "2.0",
                    "id": "bad-urls-type",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_extract",
                        "arguments": { "urls": [1, "https://example.com"] }
                    }
                }),
                "tavily_extract",
            ),
            (
                "missing-crawl-url",
                json!({
                    "jsonrpc": "2.0",
                    "id": "missing-crawl-url",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_crawl",
                        "arguments": {}
                    }
                }),
                "tavily_crawl",
            ),
            (
                "missing-map-url",
                json!({
                    "jsonrpc": "2.0",
                    "id": "missing-map-url",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_map",
                        "arguments": {}
                    }
                }),
                "tavily_map",
            ),
            (
                "missing-research-input",
                json!({
                    "jsonrpc": "2.0",
                    "id": "missing-research-input",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_research",
                        "arguments": {}
                    }
                }),
                "tavily_research",
            ),
        ];

        for (case_id, payload, tool_name) in invalid_cases {
            let response = client
                .post(&url)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .header("mcp-protocol-version", "2025-03-26")
                .header("mcp-session-id", proxy_session_id.as_str())
                .json(&payload)
                .send()
                .await
                .unwrap_or_else(|err| panic!("{case_id} request should complete: {err}"));
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{case_id} should return an official-style tool error envelope"
            );
            let body = decode_sse_json_response(response).await;
            assert_eq!(
                body["result"]["isError"].as_bool(),
                Some(true),
                "{case_id} should return result.isError=true"
            );
            assert!(
                body["result"]["content"].as_array().is_some(),
                "{case_id} should return a content array"
            );
            let message = body["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_else(|| panic!("{case_id} should include error message"));
            assert!(
                message.contains(tool_name),
                "{case_id} error should reference tool name, got {message}"
            );
        }

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "invalid rebalance tool args should never hit upstream"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let rows = sqlx::query(
            r#"
            SELECT status_code, failure_kind, fallback_reason, request_body
            FROM request_logs
            WHERE path = '/mcp'
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&pool)
        .await
        .expect("fetch rebalance invalid argument request logs");
        assert_eq!(
            rows.len(),
            1 + 9,
            "initialize plus each invalid tool call should be logged locally"
        );
        for row in rows.iter().skip(1) {
            assert_eq!(
                row.try_get::<Option<i64>, _>("status_code").unwrap(),
                Some(200),
                "invalid tool arguments should log official-style HTTP 200 status"
            );
            assert_eq!(
                row.try_get::<Option<String>, _>("failure_kind")
                    .unwrap()
                    .as_deref(),
                Some("tool_argument_validation"),
                "invalid tool arguments should keep the canonical client failure kind"
            );
            assert_eq!(
                row.try_get::<Option<String>, _>("fallback_reason")
                    .unwrap()
                    .as_deref(),
                Some("invalid_tool_arguments"),
                "invalid tool arguments should preserve the local fallback reason"
            );
            let request_body = row
                .try_get::<Option<Vec<u8>>, _>("request_body")
                .unwrap()
                .expect("logged request body");
            let request_body = String::from_utf8(request_body).expect("request body utf-8");
            assert!(
                request_body.contains("tools/call"),
                "invalid tool calls should persist the original request body"
            );
        }

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_unknown_tool_logs_canonical_failure_kind() {
        let db_path = temp_db_path("mcp-rebalance-unknown-tool");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-unknown-tool";
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
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-unknown-tool"))
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
                "id": "unknown-tool-init",
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
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2025-03-26")
            .header("mcp-session-id", proxy_session_id.as_str())
            .json(&json!({
                "jsonrpc": "2.0",
                "id": "unknown-tool-call",
                "method": "tools/call",
                "params": {
                    "name": "totally_not_real",
                    "arguments": {}
                }
            }))
            .send()
            .await
            .expect("unknown tool request");
        assert_eq!(response.status(), StatusCode::OK);
        let body = decode_sse_json_response(response).await;
        assert_eq!(body["result"]["isError"].as_bool(), Some(true));
        assert_eq!(
            body["result"]["content"][0]["text"].as_str(),
            Some("Not found: Unknown tool: 'totally_not_real'")
        );

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "unknown rebalance tool should never hit upstream"
        );

        let pool = connect_sqlite_test_pool(&db_str).await;
        let row = sqlx::query(
            r#"
            SELECT status_code, failure_kind, fallback_reason
            FROM request_logs
            WHERE path = '/mcp'
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .fetch_one(&pool)
        .await
        .expect("fetch unknown tool request log");
        assert_eq!(
            row.try_get::<Option<i64>, _>("status_code").unwrap(),
            Some(200),
            "unknown tool should log official-style HTTP 200 status"
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("failure_kind")
                .unwrap()
                .as_deref(),
            Some("unknown_tool_name"),
            "unknown tool should log the canonical failure kind"
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("fallback_reason")
                .unwrap()
                .as_deref(),
            Some("unknown_tool"),
            "unknown tool should preserve the local fallback reason"
        );

        let _ = std::fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn mcp_rebalance_initialize_batch_uses_local_validation_for_invalid_tool_args() {
        let db_path = temp_db_path("mcp-rebalance-initialize-batch-invalid-args");
        let db_str = db_path.to_string_lossy().to_string();
        let expected_api_key = "tvly-rebalance-initialize-batch-invalid-args";
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
                user_blocked_key_base_limit: tavily_hikari::USER_MONTHLY_BROKEN_LIMIT_DEFAULT,
            })
            .await
            .expect("enable rebalance mcp");
        let access_token = proxy
            .create_access_token(Some("mcp-rebalance-initialize-batch-invalid-args"))
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
            .header("mcp-protocol-version", "2025-03-26")
            .json(&json!([
                {
                    "jsonrpc": "2.0",
                    "id": "rebalance-batch-init",
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-03-26",
                        "capabilities": {}
                    }
                },
                {
                    "jsonrpc": "2.0",
                    "id": "rebalance-batch-invalid-search",
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_search",
                        "arguments": {}
                    }
                }
            ]))
            .send()
            .await
            .expect("initialize batch request");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "initialize batch should return aggregated JSON-RPC responses"
        );
        assert!(
            response.headers().get("mcp-session-id").is_some(),
            "initialize batch should still mint a proxy mcp-session-id"
        );
        let body = decode_sse_json_response(response).await;
        let items = body
            .as_array()
            .expect("initialize batch should return a JSON-RPC array");
        assert_eq!(items.len(), 2, "initialize batch should return two responses");
        assert_eq!(
            items[1]["result"]["isError"].as_bool(),
            Some(true),
            "invalid rebalance tool args should return an official-style tool error even in an initialize batch"
        );

        let recorded = seen
            .lock()
            .expect("rebalance gateway calls lock poisoned")
            .clone();
        assert!(
            recorded.is_empty(),
            "initialize batch invalid tool args should never hit upstream"
        );

        let _ = std::fs::remove_file(db_path);
    }

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

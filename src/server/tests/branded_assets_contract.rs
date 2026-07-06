use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

#[tokio::test]
async fn branded_assets_are_served_from_assets_contract_and_favicon_remains_available() {
    let db_path = temp_db_path("branded-assets-contract");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create proxy");
    let static_dir = temp_static_dir("branded-assets-contract");
    let state = Arc::new(AppState {
        proxy,
        static_dir: Some(static_dir),
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: linuxdo_oauth_options_for_test(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: false,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });

    let app = Router::new()
        .route("/assets/*path", get(serve_asset))
        .route("/favicon.svg", get(serve_favicon))
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

    let client = Client::new();

    for path in [
        "/assets/relay-mesh-lockup-light.png",
        "/assets/relay-mesh-lockup-dark.png",
        "/assets/relay-mesh-mark-light.svg",
        "/assets/linuxdo-logo.svg",
        "/favicon.svg",
    ] {
        let resp = client
            .get(format!("http://{addr}{path}"))
            .send()
            .await
            .unwrap_or_else(|_| panic!("request succeeds for {path}"));
        assert_eq!(resp.status(), reqwest::StatusCode::OK, "status for {path}");
    }

    let favicon = client
        .get(format!("http://{addr}/favicon.svg"))
        .send()
        .await
        .expect("favicon request");
    let favicon_body = favicon.text().await.expect("favicon body");
    assert!(favicon_body.contains("assets/relay-mesh-mark-light.png"));

    let _ = std::fs::remove_file(db_path);
}

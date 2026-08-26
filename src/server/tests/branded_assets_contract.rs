use super::*;
use super::core_support_and_parsing::*;
use super::upstream_support_and_manual_jobs::*;

#[test]
fn relay_mesh_mark_svgs_are_true_vectors() {
    for svg in [
        include_str!("../../../web/public/assets/relay-mesh-mark-light.svg"),
        include_str!("../../../web/public/assets/relay-mesh-mark-dark.svg"),
    ] {
        assert!(svg.contains("<circle"), "mark SVG should contain vector circles");
        assert!(svg.contains("<line"), "mark SVG should contain vector lines");
        assert_eq!(svg.matches("<line ").count(), 25);
        assert_eq!(svg.matches("r=\"16.7\"").count(), 7);
        assert!(svg.contains("id=\"edge-top-right\""));
        assert!(!svg.contains("<image"), "mark SVG must not embed an image");
        assert!(!svg.contains("data:"), "mark SVG must not embed raster data");
    }
}

#[test]
fn relay_mesh_mark_geometry_is_semantically_decomposed() {
    let svg = include_str!(
        "../../../web/brand/relay-mesh/reference/approved-mark-geometry-mono.svg"
    );

    assert_eq!(svg.matches("<circle").count(), 7);
    assert_eq!(svg.matches("<line ").count(), 25);
    for group in ["outer-network", "center-spokes", "nodes", "search-symbol"] {
        assert!(
            svg.contains(&format!("id=\"{group}\"")),
            "mark geometry should contain the {group} group"
        );
    }
    assert!(!svg.contains("<image"), "mark geometry must not embed an image");
    assert!(!svg.contains("data:"), "mark geometry must not embed raster data");
    assert!(!svg.contains("href="), "mark geometry must not link raster data");
    assert_eq!(svg.matches("r=\"16.7\"").count(), 6);
    assert!(svg.contains("id=\"edge-top-right\""));
}

#[test]
fn relay_mesh_lockup_svgs_are_true_vectors() {
    for svg in [
        include_str!("../../../web/public/assets/relay-mesh-lockup-light.svg"),
        include_str!("../../../web/public/assets/relay-mesh-lockup-dark.svg"),
        include_str!("../../../web/public/assets/relay-mesh-mobile-logo-light.svg"),
        include_str!("../../../web/public/assets/relay-mesh-mobile-logo-dark.svg"),
    ] {
        assert!(svg.contains("<path"), "lockup SVG should contain outlined text paths");
        assert!(svg.contains("id=\"mark-artwork\""));
        assert!(svg.contains("id=\"wordmark\""));
        assert!(!svg.contains("<text"), "lockup SVG must not depend on installed fonts");
        assert!(!svg.contains("<image"), "lockup SVG must not embed an image");
        assert!(!svg.contains("data:"), "lockup SVG must not embed raster data");
        assert!(!svg.contains("href="), "lockup SVG must not link raster data");
    }

    let full = include_str!("../../../web/public/assets/relay-mesh-lockup-light.svg");
    let compact = include_str!("../../../web/public/assets/relay-mesh-mobile-logo-light.svg");
    assert!(full.contains("id=\"tagline\""));
    assert!(full.contains("id=\"tagline-flow-gradient\""));
    assert!(full.contains("transform=\"translate(0 -34)\""));
    for group in [
        "tagline-primary",
        "tagline-separator",
        "tagline-secondary",
    ] {
        assert!(full.contains(&format!("id=\"{group}\"")));
        assert!(!compact.contains(group));
    }
    assert!(full.contains("#6D28D9"));
    assert!(full.contains("#0369A1"));
    assert!(!compact.contains("id=\"tagline\""));
    assert!(!compact.contains("tagline-flow-gradient"));

    let dark = include_str!("../../../web/public/assets/relay-mesh-lockup-dark.svg");
    assert!(dark.contains("#A78BFA"));
    assert!(dark.contains("#38BDF8"));
}

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
        remote_attempt_admission: new_remote_attempt_admission(),
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
        "/assets/relay-mesh-lockup-light.svg",
        "/assets/relay-mesh-lockup-dark.svg",
        "/assets/relay-mesh-mobile-logo-light.svg",
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

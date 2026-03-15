#[allow(clippy::too_many_arguments)]
pub async fn serve(
    addr: SocketAddr,
    proxy: TavilyProxy,
    static_dir: Option<PathBuf>,
    forward_auth: ForwardAuthConfig,
    admin_auth: AdminAuthOptions,
    dev_open_admin: bool,
    usage_base: String,
    api_key_ip_geo_origin: String,
    linuxdo_oauth: LinuxDoOAuthOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let AdminAuthOptions {
        forward_auth_enabled,
        builtin_auth_enabled,
        builtin_auth_password,
        builtin_auth_password_hash,
    } = admin_auth;
    let builtin_admin = BuiltinAdminAuth::new(
        builtin_auth_enabled,
        builtin_auth_password,
        builtin_auth_password_hash,
    );
    let state = Arc::new(AppState {
        proxy,
        static_dir: static_dir.clone(),
        forward_auth,
        forward_auth_enabled,
        builtin_admin,
        linuxdo_oauth,
        dev_open_admin,
        usage_base: usage_base.clone(),
        api_key_ip_geo_origin,
    });

    println!(
        "Admin auth modes: forward_enabled={} builtin_enabled={} dev_open_admin={}",
        state.forward_auth_enabled,
        state.builtin_admin.is_enabled(),
        state.dev_open_admin
    );

    if !state.forward_auth_enabled {
        println!("Forward-Auth: disabled (ADMIN_AUTH_FORWARD_ENABLED=false)");
    } else if let Some(h) = state.forward_auth.user_header() {
        println!(
            "Forward-Auth: header='{}' admin_value='{}'",
            h,
            state.forward_auth.admin_value().unwrap_or("<none>")
        );
    } else {
        println!(
            "Forward-Auth: disabled (no user header), admin_override={} dev_open_admin={}",
            state.forward_auth.admin_override_name().unwrap_or("<none>"),
            state.dev_open_admin
        );
    }

    println!(
        "LinuxDo OAuth: enabled={} configured={} redirect={}",
        state.linuxdo_oauth.enabled,
        state.linuxdo_oauth.is_enabled_and_configured(),
        state
            .linuxdo_oauth
            .redirect_url
            .as_deref()
            .unwrap_or("<none>")
    );

    let mut router = Router::new()
        .route("/health", get(health_check))
        .route("/api/debug/headers", get(debug_headers))
        .route("/api/debug/is-admin", get(debug_is_admin))
        .route("/api/debug/forward-auth", get(get_forward_auth_debug))
        .route("/api/debug/admin", get(get_admin_debug))
        .route("/api/public/events", get(sse_public))
        .route("/api/public/logs", get(get_public_logs))
        .route("/api/token/metrics", get(get_token_metrics_public))
        .route("/api/events", get(sse_dashboard))
        .route("/api/version", get(get_versions))
        .route("/api/profile", get(get_profile))
        .route("/auth/linuxdo", get(get_linuxdo_auth).post(post_linuxdo_auth))
        .route("/auth/linuxdo/callback", get(get_linuxdo_callback))
        .route("/api/user/logout", post(post_user_logout))
        .route("/api/user/token", get(get_user_token))
        .route("/api/user/dashboard", get(get_user_dashboard))
        .route("/api/user/tokens", get(get_user_tokens))
        .route("/api/user/tokens/:id", get(get_user_token_detail))
        .route("/api/user/tokens/:id/secret", get(get_user_token_secret))
        .route("/api/user/tokens/:id/logs", get(get_user_token_logs))
        .route("/api/admin/registration", get(get_admin_registration_settings))
        .route(
            "/api/admin/registration",
            patch(patch_admin_registration_settings),
        )
        .route("/api/admin/login", post(post_admin_login))
        .route("/api/admin/logout", post(post_admin_logout))
        .route("/api/tavily/search", post(tavily_http_search))
        .route("/api/tavily/extract", post(tavily_http_extract))
        .route("/api/tavily/crawl", post(tavily_http_crawl))
        .route("/api/tavily/map", post(tavily_http_map))
        .route("/api/tavily/research", post(tavily_http_research))
        .route(
            "/api/tavily/research/:request_id",
            get(tavily_http_research_result),
        )
        .route("/api/tavily/usage", get(tavily_http_usage))
        .route("/api/summary", get(fetch_summary))
        .route("/api/summary/windows", get(fetch_summary_windows))
        .route("/api/settings", get(get_settings))
        .route("/api/settings/forward-proxy", put(put_forward_proxy_settings))
        .route(
            "/api/settings/forward-proxy/validate",
            post(post_forward_proxy_candidate_validation),
        )
        .route(
            "/api/stats/forward-proxy/summary",
            get(get_forward_proxy_dashboard_summary),
        )
        .route("/api/stats/forward-proxy", get(get_forward_proxy_live_stats))
        .route("/api/public/metrics", get(get_public_metrics))
        .route("/api/keys", get(list_keys))
        .route("/api/keys", post(create_api_key))
        .route("/api/keys/validate", post(post_validate_api_keys))
        .route("/api/keys/batch", post(create_api_keys_batch))
        .route("/api/keys/:id", get(get_api_key_detail))
        .route("/api/keys/:id/quarantine", delete(delete_api_key_quarantine))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/keys/:id/secret", get(get_api_key_secret))
        .route("/api/keys/:id", delete(delete_api_key))
        .route("/api/keys/:id/status", patch(update_api_key_status))
        .route("/api/jobs", get(list_jobs))
        .route("/api/logs", get(list_logs))
        .route("/api/user-tags", get(list_user_tags))
        .route("/api/user-tags", post(create_user_tag))
        .route("/api/user-tags/:tag_id", patch(update_user_tag))
        .route("/api/user-tags/:tag_id", delete(delete_user_tag))
        .route("/api/users", get(list_users))
        .route("/api/users/:id", get(get_user_detail))
        .route("/api/users/:id/quota", patch(update_user_quota))
        .route("/api/users/:id/tags", post(bind_user_tag))
        .route("/api/users/:id/tags/:tag_id", delete(unbind_user_tag))
        // Key details
        .route("/api/keys/:id/metrics", get(get_key_metrics))
        .route("/api/keys/:id/logs", get(get_key_logs))
        // Token details
        .route("/api/tokens/:id", get(get_token_detail))
        .route("/api/tokens/:id/metrics", get(get_token_metrics))
        .route(
            "/api/tokens/:id/metrics/usage-series",
            get(get_token_usage_series),
        )
        .route(
            "/api/tokens/:id/metrics/hourly",
            get(get_token_hourly_breakdown),
        )
        .route("/api/tokens/leaderboard", get(get_token_leaderboard))
        .route("/api/tokens/:id/logs", get(get_token_logs))
        .route("/api/tokens/:id/logs/page", get(get_token_logs_page))
        .route("/api/tokens/:id/events", get(sse_token))
        // Access token management (admin only)
        .route("/api/tokens", get(list_tokens))
        .route("/api/tokens", post(create_token))
        .route("/api/tokens/groups", get(list_token_groups))
        .route("/api/tokens/batch", post(create_tokens_batch))
        .route("/api/tokens/:id", delete(delete_token))
        .route("/api/tokens/:id/status", patch(update_token_status))
        .route("/api/tokens/:id/note", patch(update_token_note))
        .route("/api/tokens/:id/secret", get(get_token_secret))
        .route("/api/tokens/:id/secret/rotate", post(rotate_token_secret));

    if let Some(dir) = static_dir.as_ref() {
        if dir.is_dir() {
            let index_file = dir.join("index.html");
            if index_file.exists() {
                router = router.nest_service("/assets", ServeDir::new(dir.join("assets")));
                router = router.route("/", get(serve_index));
                router = router.route("/admin", get(serve_admin_index));
                router = router.route("/admin/", get(serve_admin_index));
                router = router.route("/console", get(serve_console_index));
                router = router.route("/console/", get(serve_console_index));
                router = router.route("/console.html", get(serve_console_index));
                router = router.route("/admin/*path", get(serve_admin_index));
                router = router.route("/login", get(serve_login));
                router = router.route("/login/", get(serve_login));
                router = router.route("/login.html", get(serve_login));
                router = router.route(
                    "/registration-paused",
                    get(serve_registration_paused_index),
                );
                router = router.route(
                    "/registration-paused/",
                    get(serve_registration_paused_index),
                );
                router = router.route(
                    "/registration-paused.html",
                    get(serve_registration_paused_index),
                );
                router =
                    router.route_service("/favicon.svg", ServeFile::new(dir.join("favicon.svg")));
                router = router.route_service(
                    "/linuxdo-logo.svg",
                    ServeFile::new(dir.join("linuxdo-logo.svg")),
                );
            } else {
                eprintln!(
                    "static index.html not found at {} — skip serving SPA",
                    index_file.display()
                );
            }
        } else {
            eprintln!("static dir '{}' is not a directory", dir.display());
        }
    }

    router = router
        .route("/mcp", any(proxy_handler))
        .route("/mcp/*path", any(proxy_handler));

    // 404 landing page that updates URL back to original via history API
    router = router.route("/__404", get(not_found_landing));

    // Fallback: if UA/Accept 支持 HTML 则重定向到 __404；否则返回纯 404
    async fn supports_html(headers: &HeaderMap) -> bool {
        let accept = headers
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if accept.contains("text/html") {
            return true;
        }
        let ua = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        ua.contains("mozilla/")
    }

    router = router.fallback(|req: Request<Body>| async move {
        let headers = req.headers().clone();
        if supports_html(&headers).await {
            // 302 for GET/HEAD; 303 for others
            let uri = req.uri();
            let pq = uri
                .path_and_query()
                .map(|v| v.as_str())
                .unwrap_or(uri.path());
            let target = format!("/__404?path={}", urlencoding::encode(pq));
            match *req.method() {
                Method::GET | Method::HEAD => Redirect::temporary(&target).into_response(),
                _ => Redirect::to(&target).into_response(), // 303 See Other
            }
        } else {
            (StatusCode::NOT_FOUND, Body::empty()).into_response()
        }
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound_addr = listener.local_addr()?;
    println!("Tavily proxy listening on http://{bound_addr}");

    // Spawn background schedulers
    spawn_quota_sync_scheduler(state.clone());
    spawn_token_usage_rollup_scheduler(state.clone());
    spawn_auth_token_logs_gc_scheduler(state.clone());
    spawn_request_logs_gc_scheduler(state.clone());
    spawn_forward_proxy_maintenance_scheduler(state.clone());

    axum::serve(
        listener,
        router
            .with_state(state)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    println!("Server shut down gracefully.");
    Ok(())
}

async fn wait_for_ctrl_c() -> &'static str {
    match signal::ctrl_c().await {
        Ok(()) => "ctrl_c",
        Err(err) => {
            eprintln!("Failed to listen for Ctrl+C: {err}");
            "ctrl_c_error"
        }
    }
}

#[cfg(unix)]
async fn wait_for_sigterm() -> &'static str {
    match unix_signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            sigterm.recv().await;
            "sigterm"
        }
        Err(err) => {
            eprintln!("Failed to listen for SIGTERM: {err}");
            wait_for_ctrl_c().await
        }
    }
}

async fn shutdown_signal() {
    let signal = {
        #[cfg(unix)]
        {
            tokio::select! {
                reason = wait_for_ctrl_c() => reason,
                reason = wait_for_sigterm() => reason,
            }
        }

        #[cfg(not(unix))]
        {
            wait_for_ctrl_c().await
        }
    };

    println!("Shutdown signal ({signal}) received, waiting for in-flight requests to finish...");
}

const BODY_LIMIT: usize = 16 * 1024 * 1024; // 16 MiB 默认限制
const DEFAULT_LOG_LIMIT: usize = 200;

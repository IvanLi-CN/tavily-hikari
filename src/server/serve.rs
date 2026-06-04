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
    ha_config: tavily_hikari::HaConfig,
    linuxdo_oauth: LinuxDoOAuthOptions,
    linuxdo_credit: LinuxDoCreditOptions,
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
    let ha = tavily_hikari::HaRuntime::new(ha_config);
    let previous_ha_role = proxy.get_persisted_ha_node_role().await.unwrap_or_else(|err| {
        eprintln!("HA persisted role lookup warning: {err}");
        None
    });
    let startup_ha_status = reconcile_ha_startup_role(&ha, previous_ha_role).await;
    if let Err(err) = proxy
        .persist_ha_node_state(
            &startup_ha_status.node_id,
            startup_ha_status.role,
            startup_ha_status.edgeone_origin.as_deref(),
            startup_ha_status.message.as_deref(),
        )
        .await
    {
        eprintln!("HA startup node state persist warning: {err}");
    }
    let state = Arc::new(AppState {
        proxy,
        static_dir: static_dir.clone(),
        forward_auth,
        forward_auth_enabled,
        builtin_admin,
        linuxdo_oauth,
        linuxdo_credit,
        ha,
        dev_open_admin,
        usage_base: usage_base.clone(),
        api_key_ip_geo_origin,
    });
    spawn_ha_standby_sync_task(state.clone());
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
    let (linuxdo_user_sync_hour, linuxdo_user_sync_minute) = state.linuxdo_oauth.user_sync_time();
    println!(
        "LinuxDo user sync: scheduler_enabled={} oauth_ready={} refresh_token_key={} at={:02}:{:02}",
        state.linuxdo_oauth.is_user_sync_scheduler_enabled(),
        state.linuxdo_oauth.is_enabled_and_configured(),
        state.linuxdo_oauth.has_refresh_token_crypt_key(),
        linuxdo_user_sync_hour,
        linuxdo_user_sync_minute,
    );
    println!(
        "LinuxDo Credit: enabled={} configured={} submit_url={}",
        state.linuxdo_credit.enabled,
        state.linuxdo_credit.is_enabled_and_configured(),
        state.linuxdo_credit.submit_url
    );
    let ha_status = state.ha.status().await;
    println!(
        "HA: mode={:?} node={} role={:?} origin={:?} edgeone_domain={:?}",
        ha_status.mode,
        ha_status.node_id,
        ha_status.role,
        ha_status.edgeone_origin,
        ha_status.edgeone_domain
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
        .route("/api/ha/status", get(get_public_ha_status))
        .route("/api/events", get(sse_dashboard))
        .route("/api/version", get(get_versions))
        .route("/api/profile", get(get_profile))
        .route("/api/dashboard/overview", get(get_dashboard_overview))
        .route("/auth/linuxdo", get(get_linuxdo_auth).post(post_linuxdo_auth))
        .route("/auth/linuxdo/callback", get(get_linuxdo_callback))
        .route("/api/user/logout", post(post_user_logout))
        .route("/api/user/token", get(get_user_token))
        .route("/api/user/dashboard", get(get_user_dashboard))
        .route(
            "/api/user/debug-info-sharing",
            put(put_user_debug_info_sharing),
        )
        .route("/api/user/announcements", get(get_user_announcements))
        .route(
            "/api/user/announcements/history",
            get(get_user_announcement_history),
        )
        .route("/api/user/recharge/config", get(get_user_recharge_config))
        .route(
            "/api/user/recharge/orders",
            get(get_user_recharge_orders).post(post_user_recharge_order),
        )
        .route(
            "/api/user/recharge/orders/:out_trade_no",
            get(get_user_recharge_order),
        )
        .route("/api/linuxdo-credit/notify", get(get_linuxdo_credit_notify))
        .route("/api/user/tokens", get(get_user_tokens))
        .route("/api/user/tokens/:id", get(get_user_token_detail))
        .route("/api/user/tokens/:id/secret", get(get_user_token_secret))
        .route(
            "/api/user/tokens/:id/secret/rotate",
            post(rotate_user_token_secret),
        )
        .route("/api/user/tokens/:id/logs", get(get_user_token_logs))
        .route("/api/user/tokens/:id/events", get(sse_user_token))
        .route("/api/admin/registration", get(get_admin_registration_settings))
        .route(
            "/api/admin/registration",
            patch(patch_admin_registration_settings),
        )
        .route("/api/admin/login", post(post_admin_login))
        .route("/api/admin/logout", post(post_admin_logout))
        .route("/api/admin/ha/status", get(get_admin_ha_status))
        .route(
            "/api/admin/ha/snapshot",
            get(get_admin_ha_snapshot)
                .put(put_admin_ha_snapshot)
                .layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/admin/ha/baseline", get(get_admin_ha_baseline))
        .route("/api/admin/ha/events", get(get_admin_ha_events))
        .route("/api/admin/ha/events/ack", post(post_admin_ha_events_ack))
        .route("/api/admin/ha/promote", post(post_admin_ha_promote))
        .route("/api/admin/ha/finalize", post(post_admin_ha_finalize))
        .route(
            "/api/admin/ha/recovery/import",
            post(post_admin_ha_recovery_import),
        )
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
        .route("/api/settings/system", put(put_system_settings))
        .route("/api/admin/recharges", get(get_admin_recharges))
        .route(
            "/api/admin/recharges/:out_trade_no/refund",
            post(post_admin_recharge_refund),
        )
        .route(
            "/api/admin/recharges/:out_trade_no/refund-only",
            post(post_admin_recharge_refund_only),
        )
        .route("/api/admin/totp", get(get_admin_totp_status))
        .route("/api/admin/totp/setup", post(post_admin_totp_setup))
        .route("/api/admin/totp/confirm", post(post_admin_totp_confirm))
        .route("/api/admin/totp/reset", post(post_admin_totp_reset))
        .route("/api/admin/totp/disable", post(post_admin_totp_disable))
        .route(
            "/api/settings/client-ip/observed-headers",
            get(get_observed_client_ip_requests),
        )
        .route("/api/settings/forward-proxy", put(put_forward_proxy_settings))
        .route(
            "/api/settings/forward-proxy/validate",
            post(post_forward_proxy_candidate_validation),
        )
        .route(
            "/api/settings/forward-proxy/revalidate",
            post(post_forward_proxy_revalidate),
        )
        .route(
            "/api/settings/forward-proxy/nodes/state",
            post(post_forward_proxy_node_state),
        )
        .route(
            "/api/stats/forward-proxy/summary",
            get(get_forward_proxy_dashboard_summary),
        )
        .route(
            "/api/stats/forward-proxy/errors",
            get(get_forward_proxy_error_stats),
        )
        .route("/api/stats/forward-proxy", get(get_forward_proxy_live_stats))
        .route("/api/public/metrics", get(get_public_metrics))
        .route("/api/keys", get(list_keys))
        .route("/api/keys", post(create_api_key))
        .route("/api/keys/validate", post(post_validate_api_keys))
        .route("/api/keys/batch", post(create_api_keys_batch))
        .route("/api/keys/bulk-actions", post(post_api_key_bulk_actions))
        .route("/api/keys/:id", get(get_api_key_detail))
        .route("/api/keys/:id/quarantine", delete(delete_api_key_quarantine))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .route("/api/keys/:id/secret", get(get_api_key_secret))
        .route("/api/keys/:id", delete(delete_api_key))
        .route("/api/keys/:id/status", patch(update_api_key_status))
        .route("/api/jobs", get(list_jobs))
        .route("/api/logs", get(list_logs))
        .route("/api/logs/list", get(list_logs_cursor))
        .route("/api/logs/catalog", get(get_logs_catalog))
        .route("/api/logs/:log_id/details", get(get_log_details))
        .route("/api/announcements", get(get_announcements))
        .route("/api/announcements", post(create_announcement))
        .route("/api/announcements/:id", patch(update_announcement))
        .route(
            "/api/announcements/:id/publish",
            post(publish_announcement),
        )
        .route(
            "/api/announcements/:id/archive",
            post(archive_announcement),
        )
        .route("/api/alerts/catalog", get(get_alert_catalog))
        .route("/api/alerts/events", get(get_alert_events))
        .route("/api/alerts/groups", get(get_alert_groups))
        .route("/api/user-tags", get(list_user_tags))
        .route("/api/user-tags", post(create_user_tag))
        .route("/api/user-tags/:tag_id", patch(update_user_tag))
        .route("/api/user-tags/:tag_id", delete(delete_user_tag))
        .route("/api/users", get(list_users))
        .route("/api/users/:id", get(get_user_detail))
        .route("/api/users/:id/usage-series", get(get_user_usage_series))
        .route("/api/users/:id/tokens", post(create_user_token))
        .route("/api/users/:id/tokens/:token_id", delete(delete_user_token))
        .route("/api/users/:id/quota", patch(update_user_quota))
        .route(
            "/api/users/:id/broken-key-limit",
            patch(update_user_broken_key_limit),
        )
        .route("/api/users/:id/broken-keys", get(get_user_monthly_broken_keys))
        .route("/api/users/:id/tags", post(bind_user_tag))
        .route("/api/users/:id/tags/:tag_id", delete(unbind_user_tag))
        // Key details
        .route("/api/keys/:id/metrics", get(get_key_metrics))
        .route("/api/keys/:id/logs", get(get_key_logs))
        .route("/api/keys/:id/logs/list", get(get_key_logs_list))
        .route("/api/keys/:id/logs/catalog", get(get_key_logs_catalog))
        .route("/api/keys/:id/logs/page", get(get_key_logs_page))
        .route("/api/keys/:id/logs/:log_id/details", get(get_key_log_details))
        .route("/api/keys/:id/sticky-users", get(get_key_sticky_users))
        .route("/api/keys/:id/sticky-nodes", get(get_key_sticky_nodes))
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
        .route("/api/tokens/unbound-usage", get(list_unbound_token_usage))
        .route("/api/tokens/:id/logs", get(get_token_logs))
        .route("/api/tokens/:id/logs/list", get(get_token_logs_list))
        .route("/api/tokens/:id/logs/catalog", get(get_token_logs_catalog))
        .route("/api/tokens/:id/logs/page", get(get_token_logs_page))
        .route("/api/tokens/:id/logs/:log_id/details", get(get_token_log_details))
        .route("/api/tokens/:id/broken-keys", get(get_token_monthly_broken_keys))
        .route("/api/tokens/:id/events", get(sse_token))
        // Access token management (admin only)
        .route("/api/tokens", get(list_tokens))
        .route("/api/tokens", post(create_token))
        .route("/api/tokens/groups", get(list_token_groups))
        .route("/api/tokens/batch", post(create_tokens_batch))
        .route("/api/tokens/batch/status", patch(update_tokens_status_batch))
        .route("/api/tokens/batch", delete(delete_tokens_batch))
        .route("/api/tokens/:id", delete(delete_token))
        .route("/api/tokens/:id/status", patch(update_token_status))
        .route("/api/tokens/:id/note", patch(update_token_note))
        .route("/api/tokens/:id/secret", get(get_token_secret))
        .route("/api/tokens/:id/secret/rotate", post(rotate_token_secret))
        .route("/", get(serve_index))
        .route("/admin", get(serve_admin_index))
        .route("/admin/", get(serve_admin_index))
        .route("/admin/*path", get(serve_admin_index))
        .route("/console", get(serve_console_index))
        .route("/console/", get(serve_console_index))
        .route("/console.html", get(serve_console_index))
        .route("/console/*path", get(serve_console_index))
        .route("/login", get(serve_login))
        .route("/login/", get(serve_login))
        .route("/login.html", get(serve_login))
        .route(
            "/registration-paused",
            get(serve_registration_paused_index),
        )
        .route(
            "/registration-paused/",
            get(serve_registration_paused_index),
        )
        .route(
            "/registration-paused.html",
            get(serve_registration_paused_index),
        )
        .route("/favicon.svg", get(serve_favicon))
        .route("/linuxdo-logo.svg", get(serve_linuxdo_logo))
        .route("/version.json", get(serve_version_json))
        .route("/assets", get(serve_assets_root))
        .route("/assets/*path", get(serve_asset));

    router = router
        .route("/mcp", any(proxy_handler))
        .route("/mcp/*path", any(mcp_subpath_reject_handler));

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
    spawn_mcp_sessions_gc_scheduler(state.clone());
    spawn_mcp_session_init_backoffs_gc_scheduler(state.clone());
    spawn_request_logs_gc_scheduler(state.clone());
    if state.linuxdo_oauth.is_user_sync_scheduler_enabled() {
        spawn_linuxdo_user_status_sync_scheduler(state.clone());
    }
    spawn_linuxdo_user_tag_binding_refresh_scheduler(state.clone());
    let _forward_proxy_geo_refresh_scheduler = spawn_forward_proxy_geo_refresh_scheduler(state.clone());
    spawn_forward_proxy_maintenance_scheduler(state.clone());
    spawn_ha_edgeone_authority_task(state.clone());

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

async fn reconcile_ha_startup_role(
    ha: &tavily_hikari::HaRuntime,
    previous_ha_role: Option<tavily_hikari::HaNodeRole>,
) -> tavily_hikari::HaStatusView {
    let startup_role_checked = match ha.refresh_startup_role().await {
        Ok(()) => true,
        Err(err) => {
            eprintln!("HA startup role check warning: {err}");
            false
        }
    };
    let mut status = ha.status().await;
    if startup_role_checked
        && status.edgeone_api_configured
        && matches!(
            previous_ha_role,
            Some(
                tavily_hikari::HaNodeRole::FullMaster
                    | tavily_hikari::HaNodeRole::ProvisionalMaster
            )
        )
        && status.role == tavily_hikari::HaNodeRole::Standby
    {
        status = ha
            .enter_recovery(
                "previous active node restarted after EdgeOne origin moved; recovery import required"
                    .to_string(),
            )
            .await;
    }
    status
}

fn spawn_ha_standby_sync_task(state: Arc<AppState>) {
    let Some(source_url) = state.ha.sync_source_url() else {
        return;
    };
    let Some(internal_token) = state.ha.internal_token() else {
        eprintln!(
            "HA standby sync disabled: HA_INTERNAL_TOKEN is required when HA_SYNC_SOURCE_URL is set"
        );
        return;
    };
    let interval = std::time::Duration::from_secs(state.ha.sync_interval_secs().max(1));
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            if state.ha.role().await == tavily_hikari::HaNodeRole::Standby
                && let Err(err) =
                    run_ha_standby_sync_once(&state, &client, &source_url, &internal_token).await
            {
                eprintln!("HA standby sync failed: {err}");
            }
            tokio::time::sleep(interval).await;
        }
    });
}

async fn run_ha_standby_sync_once(
    state: &Arc<AppState>,
    client: &reqwest::Client,
    source_url: &str,
    internal_token: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let local_node_id = state.ha.status().await.node_id;
    let applied_seq = state
        .proxy
        .get_ha_sync_watermark("standby_applied_seq")
        .await?
        .unwrap_or(0);
    let baseline_applied = state
        .proxy
        .get_ha_sync_watermark("standby_baseline_applied")
        .await?
        .unwrap_or(0)
        > 0;
    let mut next_seq = applied_seq;
    if !baseline_applied {
        let target = format!("{}/api/admin/ha/baseline", source_url.trim_end_matches('/'));
        let response = client
            .get(target)
            .header("x-ha-internal-token", internal_token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(format!("baseline request failed with {}", response.status()).into());
        }
        let compressed = response.bytes().await?;
        let decoded = zstd::stream::decode_all(compressed.as_ref())?;
        let ndjson = String::from_utf8(decoded)?;
        let result = state.proxy.apply_ha_baseline_ndjson(&ndjson).await?;
        next_seq = result.high_watermark;
        state
            .proxy
            .persist_ha_sync_watermark(
                "standby_baseline",
                Some(source_url),
                Some(&local_node_id),
                result.high_watermark,
                Some(&format!("rows={}", result.row_count)),
            )
            .await?;
        state
            .proxy
            .persist_ha_sync_watermark(
                "standby_applied_seq",
                Some(source_url),
                Some(&local_node_id),
                result.high_watermark,
                Some("baseline"),
            )
            .await?;
        state
            .proxy
            .persist_ha_sync_watermark(
                "standby_baseline_applied",
                Some(source_url),
                Some(&local_node_id),
                1,
                Some("baseline applied"),
            )
            .await?;
    }

    let target = format!(
        "{}/api/admin/ha/events?after={}&limit=1000",
        source_url.trim_end_matches('/'),
        next_seq
    );
    let response = client
        .get(target)
        .header("x-ha-internal-token", internal_token)
        .send()
        .await?;
    if matches!(
        response.status(),
        reqwest::StatusCode::GONE | reqwest::StatusCode::PAYLOAD_TOO_LARGE
    ) {
        let reset_detail = if response.status() == reqwest::StatusCode::PAYLOAD_TOO_LARGE {
            "events batch too large; baseline required"
        } else {
            "retention window missed; baseline required"
        };
        state
            .proxy
            .persist_ha_sync_watermark(
                "standby_applied_seq",
                Some(source_url),
                Some(&local_node_id),
                0,
                Some(reset_detail),
            )
            .await?;
        state
            .proxy
            .persist_ha_sync_watermark(
                "standby_baseline_applied",
                Some(source_url),
                Some(&local_node_id),
                0,
                Some(reset_detail),
            )
            .await?;
        return Ok(());
    }
    if !response.status().is_success() {
        return Err(format!("events request failed with {}", response.status()).into());
    }
    let compressed = response.bytes().await?;
    let decoded = zstd::stream::decode_all(compressed.as_ref())?;
    let ndjson = String::from_utf8(decoded)?;
    let result = state.proxy.apply_ha_events_ndjson(&ndjson).await?;
    if result.high_watermark > next_seq {
        next_seq = result.high_watermark;
        state
            .proxy
            .persist_ha_sync_watermark(
                "standby_applied_seq",
                Some(source_url),
                Some(&local_node_id),
                next_seq,
                Some(&format!("events={}", result.row_count)),
            )
            .await?;
    }
    let ack_target = format!(
        "{}/api/admin/ha/events/ack",
        source_url.trim_end_matches('/')
    );
    let _ = client
        .post(ack_target)
        .header("x-ha-internal-token", internal_token)
        .json(&serde_json::json!({
            "peerNodeId": local_node_id,
            "ackedSeq": next_seq
        }))
        .send()
        .await?;
    Ok(())
}

fn spawn_ha_edgeone_authority_task(state: Arc<AppState>) {
    if !state.ha.edgeone_authority_enabled() {
        return;
    }
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            match state.ha.refresh_authoritative_role().await {
                Ok(status) => {
                    if let Err(err) = state
                        .proxy
                        .persist_ha_node_state(
                            &status.node_id,
                            status.role,
                            status.edgeone_origin.as_deref(),
                            status.message.as_deref(),
                        )
                        .await
                    {
                        eprintln!("HA authority state persist failed: {err}");
                    }
                }
                Err(err) => {
                    eprintln!("HA authority refresh failed: {err}");
                }
            }
        }
    });
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

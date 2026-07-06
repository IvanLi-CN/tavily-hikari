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
        passkey_auth_enabled,
        passkey_rp_id,
        passkey_rp_origin,
        passkey_challenge_ttl_secs,
        passkey_session_max_age_secs,
    } = admin_auth;
    let builtin_admin = BuiltinAdminAuth::new(
        builtin_auth_enabled,
        builtin_auth_password,
        builtin_auth_password_hash,
    );
    match proxy.get_admin_password_settings().await {
        Ok(settings) => builtin_admin.apply_persisted_settings(settings),
        Err(err) => tracing::warn!(
            component = "startup",
            event = "admin_password_settings_load_failed",
            err = %err,
            "admin password settings load failed; using startup configuration"
        ),
    }
    let admin_passkey = AdminPasskeyOptions {
        enabled: passkey_auth_enabled,
        rp_id: passkey_rp_id,
        rp_origin: passkey_rp_origin,
        challenge_ttl_secs: passkey_challenge_ttl_secs.max(60),
        session_max_age_secs: passkey_session_max_age_secs.max(60),
    };
    let ha = tavily_hikari::HaRuntime::new(ha_config);
    let startup_ha_status = initialize_ha_startup_state(&proxy, &ha).await;
    if let Err(err) = sync_forward_proxy_runtime_for_status(proxy.clone(), &startup_ha_status).await {
        tracing::warn!(
            component = "forward_proxy",
            event = "startup_runtime_role_sync_failed",
            role = startup_ha_status.role.as_str(),
            err = %err,
            "forward-proxy startup runtime role sync failed"
        );
    }
    let state = Arc::new(AppState {
        proxy,
        static_dir: static_dir.clone(),
        forward_auth,
        forward_auth_enabled,
        builtin_admin,
        admin_passkey,
        linuxdo_oauth,
        linuxdo_credit,
        ha,
        dev_open_admin,
        usage_base: usage_base.clone(),
        api_key_ip_geo_origin,
        dashboard_overview_cache: new_dashboard_overview_cache(),
    });
    match state.proxy.abandon_active_scheduled_jobs().await {
        Ok(count) if count > 0 => {
            tracing::warn!(
                component = "scheduler",
                event = "stale_jobs_abandoned",
                count,
                "scheduled-jobs: abandoned stale queued/running jobs from previous process"
            )
        }
        Ok(_) => {}
        Err(err) => tracing::warn!(
            component = "scheduler",
            event = "stale_jobs_cleanup_failed",
            err = %err,
            "scheduled-jobs: stale queued/running cleanup warning"
        ),
    }
    spawn_ha_standby_sync_task(state.clone());
    tracing::info!(
        component = "startup",
        event = "admin_auth_modes",
        forward_enabled = state.forward_auth_enabled,
        builtin_enabled = state.builtin_admin.is_enabled(),
        passkey_enabled = state.admin_passkey.enabled,
        passkey_configured = state.admin_passkey.is_configured(),
        dev_open_admin = state.dev_open_admin,
        "configured admin auth modes"
    );

    if !state.forward_auth_enabled {
        tracing::info!(
            component = "startup",
            event = "forward_auth_configuration",
            enabled = false,
            reason = "ADMIN_AUTH_FORWARD_ENABLED=false",
            "forward auth disabled"
        );
    } else if let Some(h) = state.forward_auth.user_header() {
        tracing::info!(
            component = "startup",
            event = "forward_auth_configuration",
            enabled = true,
            header = %h,
            admin_value_present = state.forward_auth.admin_value().is_some(),
            "forward auth enabled"
        );
    } else {
        tracing::warn!(
            component = "startup",
            event = "forward_auth_configuration",
            enabled = false,
            reason = "missing_user_header",
            admin_override_present = state.forward_auth.admin_override_name().is_some(),
            dev_open_admin = state.dev_open_admin,
            "forward auth disabled because no user header is configured"
        );
    }

    tracing::info!(
        component = "startup",
        event = "linuxdo_oauth_configuration",
        enabled = state.linuxdo_oauth.enabled,
        configured = state.linuxdo_oauth.is_enabled_and_configured(),
        redirect_configured = state.linuxdo_oauth.redirect_url.is_some(),
        "linuxdo oauth configuration loaded"
    );
    let (linuxdo_user_sync_hour, linuxdo_user_sync_minute) = state.linuxdo_oauth.user_sync_time();
    tracing::info!(
        component = "startup",
        event = "linuxdo_user_sync_configuration",
        scheduler_enabled = state.linuxdo_oauth.is_user_sync_scheduler_enabled(),
        oauth_ready = state.linuxdo_oauth.is_enabled_and_configured(),
        refresh_token_key = state.linuxdo_oauth.has_refresh_token_crypt_key(),
        sync_hour = linuxdo_user_sync_hour,
        sync_minute = linuxdo_user_sync_minute,
        "linuxdo user sync configuration loaded"
    );
    tracing::info!(
        component = "startup",
        event = "linuxdo_credit_configuration",
        enabled = state.linuxdo_credit.enabled,
        configured = state.linuxdo_credit.is_enabled_and_configured(),
        submit_url = %state.linuxdo_credit.submit_url,
        "linuxdo credit configuration loaded"
    );
    let ha_status = state.ha.status().await;
    tracing::info!(
        component = "ha",
        event = "startup_status",
        mode = ?ha_status.mode,
        node_id = %ha_status.node_id,
        role = ?ha_status.role,
        origin = ?ha_status.edgeone_origin,
        edgeone_domain = ?ha_status.edgeone_domain,
        "ha startup status loaded"
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
        .route("/api/internal/ha/status", get(get_internal_ha_status))
        .route("/api/internal/ha/leader", post(post_internal_ha_leader))
        .route(
            "/api/internal/ha/mcp-sessions/:proxy_session_id",
            get(get_internal_ha_mcp_session),
        )
        .route(
            "/api/internal/ha/research-requests/:request_id",
            get(get_internal_ha_research_request),
        )
        .route("/api/events", get(sse_dashboard))
        .route("/api/version", get(get_versions))
        .route("/api/profile", get(get_profile))
        .route("/api/dashboard/overview", get(get_dashboard_overview))
        .route("/auth/linuxdo", get(get_linuxdo_auth).post(post_linuxdo_auth))
        .route("/auth/linuxdo/callback", get(get_linuxdo_callback))
        .route("/auth/linuxdo/finalize", post(post_linuxdo_finalize))
        .route("/api/user/logout", post(post_user_logout))
        .route("/api/user/token", get(get_user_token))
        .route("/api/user/dashboard", get(get_user_dashboard))
        .route("/api/user/dashboard/overview", get(get_user_dashboard_overview))
        .route("/api/user/dashboard/events", get(sse_user_dashboard))
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
        .route("/api/user/recharge/quote", post(post_user_recharge_quote))
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
        .route(
            "/api/admin/password",
            get(get_admin_password)
                .put(put_admin_password)
                .patch(patch_admin_password)
                .delete(delete_admin_password),
        )
        .route(
            "/api/admin/passkey/authentication/start",
            post(post_admin_passkey_authentication_start),
        )
        .route(
            "/api/admin/passkey/authentication/finish",
            post(post_admin_passkey_authentication_finish),
        )
        .route(
            "/api/admin/passkey/reset/:token/registration/start",
            post(post_admin_passkey_reset_registration_start),
        )
        .route(
            "/api/admin/passkey/reset/:token/registration/finish",
            post(post_admin_passkey_reset_registration_finish),
        )
        .route("/api/admin/passkeys", get(get_admin_passkeys))
        .route(
            "/api/admin/passkeys/:credential_id",
            patch(patch_admin_passkey).delete(delete_admin_passkey),
        )
        .route(
            "/api/admin/passkeys/registration/start",
            post(post_admin_passkey_registration_start),
        )
        .route(
            "/api/admin/passkeys/registration/finish",
            post(post_admin_passkey_registration_finish),
        )
        .route("/api/admin/ha/status", get(get_admin_ha_status))
        .route("/api/admin/ha/source", put(put_admin_ha_source_settings))
        .route(
            "/api/admin/ha/snapshot",
            get(get_admin_ha_snapshot)
                .put(put_admin_ha_snapshot)
                .layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/admin/ha/baseline", get(get_admin_ha_baseline))
        .route("/api/admin/ha/events", get(get_admin_ha_events))
        .route("/api/admin/ha/events/ack", post(post_admin_ha_events_ack))
        .route("/api/admin/ha/timeline", get(get_admin_ha_timeline))
        .route("/api/admin/ha/nodes/:node_id", get(get_admin_ha_node_detail))
        .route("/api/admin/ha/promote", post(post_admin_ha_promote))
        .route(
            "/api/admin/ha/planned-cutover",
            post(post_admin_ha_planned_cutover),
        )
        .route("/api/admin/ha/finalize", post(post_admin_ha_finalize))
        .route("/api/internal/ha/finalize", post(post_internal_ha_finalize))
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
        .route("/api/analysis/pressure", get(get_analysis_pressure_snapshot))
        .route("/api/users/rankings", get(get_user_rankings))
        .route("/api/users/rankings/events", get(sse_user_rankings))
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
        .route(
            "/api/settings/forward-proxy",
            get(get_forward_proxy_settings).put(put_forward_proxy_settings),
        )
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
        .route("/api/jobs/trigger", post(post_trigger_job))
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
        .route(
            "/api/users/:id/entitlements",
            get(list_user_entitlements).post(create_user_entitlement),
        )
        .route("/api/users/:id/usage-series", get(get_user_usage_series))
        .route("/api/users/:id/tokens", post(create_user_token))
        .route("/api/users/:id/tokens/:token_id", delete(delete_user_token))
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
        .route("/index.html", get(serve_public_index_shell))
        .route("/admin", get(serve_admin_index))
        .route("/admin/", get(serve_admin_index))
        .route("/admin.html", get(serve_admin_shell))
        .route("/admin/*path", get(serve_admin_index))
        .route("/console", get(serve_console_index))
        .route("/console/", get(serve_console_index))
        .route("/console.html", get(serve_console_shell))
        .route("/console/*path", get(serve_console_shell))
        .route("/login", get(serve_login))
        .route("/login/", get(serve_login))
        .route("/login.html", get(serve_login_shell))
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
            get(serve_registration_paused_shell),
        )
        .route("/favicon.svg", get(serve_favicon))
        .route("/version.json", get(serve_version_json))
        .route("/manifest.webmanifest", get(serve_public_manifest))
        .route("/manifest-admin.webmanifest", get(serve_admin_manifest))
        .route("/sw-public.js", get(serve_public_sw))
        .route("/sw-admin.js", get(serve_admin_sw))
        .route("/pwa/*path", get(serve_pwa_asset))
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
    tracing::info!(
        component = "startup",
        event = "server_listening",
        bind_addr = %bound_addr,
        "tavily proxy listening"
    );

    // Always-on HA tasks must stay available on standby/recovery so health, role
    // refresh, and pull-sync keep working even while business traffic is fenced.
    spawn_ha_edgeone_authority_task(state.clone());
    spawn_ha_control_plane_gc_task(state.clone());
    spawn_background_tasks_for_current_role(state.clone()).await;
    let _ = spawn_post_ready_serving_tasks_for_status(state.clone(), &startup_ha_status);

    axum::serve(
        listener,
        router
            .layer(axum::middleware::from_fn(db_maintenance_http_gate))
            .with_state(state)
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!(
        component = "shutdown",
        event = "server_shutdown_complete",
        "server shut down gracefully"
    );
    Ok(())
}

async fn reconcile_ha_startup_role(
    proxy: &TavilyProxy,
    ha: &tavily_hikari::HaRuntime,
    previous_ha_role: Option<tavily_hikari::HaNodeRole>,
) -> tavily_hikari::HaStatusView {
    let startup_role_checked = match ha.refresh_startup_role().await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(
                component = "ha",
                event = "startup_role_check_failed",
                err = %err,
                "HA startup role check warning"
            );
            false
        }
    };
    if ha.dual_active_enabled() {
        let leader = proxy.get_ha_full_master_node_id().await.unwrap_or_else(|err| {
            tracing::warn!(
                component = "ha",
                event = "leader_key_lookup_failed",
                err = %err,
                "HA leader key lookup warning"
            );
            None
        });
        let persisted_role = previous_ha_role.unwrap_or(tavily_hikari::HaNodeRole::Standby);
        if leader.is_none()
            && persisted_role == tavily_hikari::HaNodeRole::FullMaster
            && let Err(err) = proxy
                .set_ha_full_master_node_id(&ha.status().await.node_id)
                .await
        {
            tracing::warn!(
                component = "ha",
                event = "leader_key_seed_failed",
                err = %err,
                "HA leader key seed warning"
            );
        }
        let leader = proxy.get_ha_full_master_node_id().await.unwrap_or(None);
        if let Err(err) = ha.apply_dual_active_leader(leader).await {
            tracing::warn!(
                component = "ha",
                event = "dual_active_leader_apply_failed",
                err = %err,
                "HA dual-active leader apply warning"
            );
        }
    }
    if previous_ha_role == Some(tavily_hikari::HaNodeRole::Recovery) {
        return ha
            .enter_recovery("previous recovery role persisted; recovery import required".to_string())
            .await;
    }
    let mut status = ha.status().await;
    if startup_role_checked
        && status.edgeone_api_configured
        && !status.dual_active_enabled
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

async fn initialize_ha_startup_state(
    proxy: &TavilyProxy,
    ha: &tavily_hikari::HaRuntime,
) -> tavily_hikari::HaStatusView {
    let previous_ha_role = proxy.get_persisted_ha_node_role().await.unwrap_or_else(|err| {
        tracing::warn!(
            component = "ha",
            event = "persisted_role_lookup_failed",
            err = %err,
            "HA persisted role lookup warning"
        );
        None
    });
    let persisted_ha_source_settings = proxy.get_ha_source_settings().await.unwrap_or_else(|err| {
        tracing::warn!(
            component = "ha",
            event = "source_settings_lookup_failed",
            err = %err,
            "HA source settings lookup warning"
        );
        None
    });
    if let Some(settings) = persisted_ha_source_settings
        && let Err(err) = ha.set_local_source_settings(Some(settings)).await
    {
        tracing::warn!(
            component = "ha",
            event = "source_settings_restore_failed",
            err = %err,
            "HA source settings restore warning"
        );
    }
    // Startup role evaluation must compare against the restored node-local HA source settings.
    let startup_ha_status = reconcile_ha_startup_role(proxy, ha, previous_ha_role).await;
    if let Err(err) = async {
        proxy
            .persist_ha_node_state(
                &startup_ha_status.node_id,
                startup_ha_status.role,
                startup_ha_status.edgeone_origin.as_deref(),
                startup_ha_status.ha_source_effective.as_ref(),
                startup_ha_status.message.as_deref(),
            )
            .await?;
        proxy.flush_ha_state_writes().await
    }
    .await
    {
        tracing::warn!(
            component = "ha",
            event = "startup_node_state_persist_failed",
            err = %err,
            "HA startup node state persist warning"
        );
    }
    startup_ha_status
}

async fn sync_forward_proxy_runtime_for_status(
    proxy: TavilyProxy,
    status: &tavily_hikari::HaStatusView,
) -> Result<(), ProxyError> {
    let allows_basic_business = status.allows_basic_business;
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(async move {
            if allows_basic_business {
                proxy.ensure_forward_proxy_runtime_started().await
            } else {
                proxy.shutdown_forward_proxy_runtime().await
            }
        })
    })
    .await
    .map_err(|err| ProxyError::Other(format!("forward-proxy runtime role sync join failed: {err}")))?
}

fn spawn_ha_standby_sync_task(state: Arc<AppState>) {
    if state.ha.dual_active_enabled() {
        spawn_ha_peer_sync_task(state);
        return;
    }
    let Some(source_url) = state.ha.sync_source_url() else {
        return;
    };
    let Some(internal_token) = state.ha.internal_token() else {
        tracing::warn!(
            component = "ha",
            event = "standby_sync_disabled",
            reason = "missing_internal_token",
            source_url = source_url,
            "HA standby sync disabled because HA_INTERNAL_TOKEN is required when HA_SYNC_SOURCE_URL is set"
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
                tracing::warn!(
                    component = "ha",
                    event = "standby_sync_failed",
                    source_url = source_url,
                    err = %err,
                    "HA standby sync failed"
                );
            }
            state.proxy.backend_time().sleep(interval).await;
        }
    });
}

fn spawn_ha_peer_sync_task(state: Arc<AppState>) {
    let Some(internal_token) = state.ha.internal_token() else {
        tracing::warn!(
            component = "ha",
            event = "peer_sync_disabled",
            reason = "missing_internal_token",
            "HA peer sync disabled because HA_INTERNAL_TOKEN is required in dual-active mode"
        );
        return;
    };
    let interval = std::time::Duration::from_secs(state.ha.sync_interval_secs().max(1));
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            let status = state.ha.status().await;
            if ha_peer_sync_should_run(&status)
                && let Err(err) = run_ha_peer_sync_once(&state, &client, &internal_token).await
            {
                tracing::warn!(
                    component = "ha",
                    event = "peer_sync_failed",
                    err = %err,
                    "HA peer sync failed"
                );
            }
            state.proxy.backend_time().sleep(interval).await;
        }
    });
}

fn ha_peer_sync_should_run(status: &tavily_hikari::HaStatusView) -> bool {
    matches!(
        status.role,
        tavily_hikari::HaNodeRole::FullMaster | tavily_hikari::HaNodeRole::Standby
    )
}

fn is_ha_retryable_foreign_key_gap(
    err: &(dyn std::error::Error + 'static),
) -> bool {
    let mut current = Some(err);
    while let Some(source) = current {
        if let Some(ProxyError::Database(sqlx::Error::Database(db_err))) =
            source.downcast_ref::<ProxyError>()
        {
            if db_err
                .code()
                .as_deref()
                .is_some_and(|code| code == "787" || code == "SQLITE_CONSTRAINT_FOREIGNKEY")
            {
                return true;
            }
            if db_err
                .message()
                .to_ascii_lowercase()
                .contains("foreign key constraint failed")
            {
                return true;
            }
        }
        current = source.source();
    }
    false
}

async fn run_ha_standby_sync_once(
    state: &Arc<AppState>,
    client: &reqwest::Client,
    source_url: &str,
    internal_token: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let local_node_id = state.ha.status().await.node_id;
    for channel in [
        tavily_hikari::HaSyncChannel::Control,
        tavily_hikari::HaSyncChannel::Billing,
        tavily_hikari::HaSyncChannel::Runtime,
    ] {
        let seq_key = format!("standby_{}_applied_seq", channel.as_str());
        let baseline_key = format!("standby_{}_baseline_applied", channel.as_str());
        let baseline_report_key = format!("standby_{}_baseline", channel.as_str());
        let applied_seq = state
            .proxy
            .get_ha_sync_watermark(&seq_key)
            .await?
            .unwrap_or(0);
        let baseline_applied = state
            .proxy
            .get_ha_sync_watermark(&baseline_key)
            .await?
            .unwrap_or(0)
            > 0;
        let mut next_seq = applied_seq;

        if !baseline_applied {
            let baseline_started = Instant::now();
            let target = format!(
                "{}/api/admin/ha/baseline?channel={}",
                source_url.trim_end_matches('/'),
                channel.as_str()
            );
            let response = client
                .get(target)
                .header("x-ha-internal-token", internal_token)
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(format!(
                    "baseline request failed for {} with {}",
                    channel.as_str(),
                    response.status()
                )
                .into());
            }
            let result = apply_ha_baseline_response_stream(
                state.as_ref(),
                channel,
                response,
                tavily_hikari::HaBaselineApplyMode::Replace,
                None,
            )
            .await?;
            next_seq = result.high_watermark;
            let outbox = state
                .proxy
                .ha_channel_outbox_stats(channel, Some(&local_node_id))
                .await?;
            let memory = tavily_hikari::capture_runtime_memory_snapshot();
            tracing::info!(
                component = "ha",
                event = "standby_sync_baseline_completed",
                elapsed_ms = baseline_started.elapsed().as_millis() as u64,
                source_url,
                channel = channel.as_str(),
                row_count = result.row_count as u64,
                payload_bytes = result.payload_bytes as u64,
                high_watermark = result.high_watermark,
                baseline_applied = true,
                outbox_row_count = outbox.row_count,
                outbox_oldest_age_secs = outbox.oldest_age_secs,
                outbox_ack_lag = outbox.ack_lag,
                memory_current_bytes = memory.memory_current_bytes.unwrap_or_default(),
                memory_limit_bytes = memory.memory_limit_bytes.unwrap_or_default(),
                headroom_bytes = memory.headroom_bytes.unwrap_or_default(),
                process_rss_bytes = memory.process_rss_bytes.unwrap_or_default(),
                child_process_rss_bytes = memory.child_process_rss_bytes.unwrap_or_default(),
                process_group_rss_bytes = memory.process_group_rss_bytes.unwrap_or_default(),
                process_hwm_bytes = memory.process_hwm_bytes.unwrap_or_default(),
                process_swap_bytes = memory.process_swap_bytes.unwrap_or_default(),
                "ha perf"
            );
            state
                .proxy
                .persist_ha_sync_watermark(
                    &baseline_report_key,
                    Some(source_url),
                    Some(&local_node_id),
                    result.high_watermark,
                    Some(&format!("rows={}", result.row_count)),
                )
                .await?;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &seq_key,
                    Some(source_url),
                    Some(&local_node_id),
                    result.high_watermark,
                    Some("baseline"),
                )
                .await?;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &baseline_key,
                    Some(source_url),
                    Some(&local_node_id),
                    1,
                    Some("baseline applied"),
                )
                .await?;
            state.proxy.flush_ha_state_writes().await?;
        }

        let target = format!(
            "{}/api/admin/ha/events?channel={}&after={}&limit=1000",
            source_url.trim_end_matches('/'),
            channel.as_str(),
            next_seq
        );
        let events_started = Instant::now();
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
                    &seq_key,
                    Some(source_url),
                    Some(&local_node_id),
                    0,
                    Some(reset_detail),
                )
                .await?;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &baseline_key,
                    Some(source_url),
                    Some(&local_node_id),
                    0,
                    Some(reset_detail),
                )
                .await?;
            state.proxy.flush_ha_state_writes().await?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "events request failed for {} with {}",
                channel.as_str(),
                response.status()
            )
            .into());
        }
        let result =
            match apply_ha_events_response_stream(state.as_ref(), channel, response, None).await {
            Ok(result) => result,
            Err(err) if is_ha_retryable_foreign_key_gap(&*err) => {
                let reset_detail =
                    "foreign key gap during events apply; baseline required";
                state
                    .proxy
                    .persist_ha_sync_watermark(
                        &seq_key,
                        Some(source_url),
                        Some(&local_node_id),
                        0,
                        Some(reset_detail),
                    )
                    .await?;
                state
                    .proxy
                    .persist_ha_sync_watermark(
                        &baseline_key,
                        Some(source_url),
                        Some(&local_node_id),
                        0,
                        Some(reset_detail),
                    )
                    .await?;
                state.proxy.flush_ha_state_writes().await?;
                continue;
            }
            Err(err) => return Err(err),
        };
        if result.high_watermark > next_seq {
            next_seq = result.high_watermark;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &seq_key,
                    Some(source_url),
                    Some(&local_node_id),
                    next_seq,
                    Some(&format!("events={}", result.row_count)),
                )
                .await?;
            state.proxy.flush_ha_state_writes().await?;
        }
        let outbox = state
            .proxy
            .ha_channel_outbox_stats(channel, Some(&local_node_id))
            .await?;
        let memory = tavily_hikari::capture_runtime_memory_snapshot();
        tracing::info!(
            component = "ha",
            event = "standby_sync_events_completed",
            elapsed_ms = events_started.elapsed().as_millis() as u64,
            source_url,
            channel = channel.as_str(),
            row_count = result.row_count as u64,
            payload_bytes = result.payload_bytes as u64,
            high_watermark = result.high_watermark,
            after_seq = applied_seq,
            next_seq,
            outbox_row_count = outbox.row_count,
            outbox_oldest_age_secs = outbox.oldest_age_secs,
            outbox_ack_lag = outbox.ack_lag,
            memory_current_bytes = memory.memory_current_bytes.unwrap_or_default(),
            memory_limit_bytes = memory.memory_limit_bytes.unwrap_or_default(),
            headroom_bytes = memory.headroom_bytes.unwrap_or_default(),
            process_rss_bytes = memory.process_rss_bytes.unwrap_or_default(),
            child_process_rss_bytes = memory.child_process_rss_bytes.unwrap_or_default(),
            process_group_rss_bytes = memory.process_group_rss_bytes.unwrap_or_default(),
            process_hwm_bytes = memory.process_hwm_bytes.unwrap_or_default(),
            process_swap_bytes = memory.process_swap_bytes.unwrap_or_default(),
            "ha perf"
        );
        let ack_target = format!("{}/api/admin/ha/events/ack", source_url.trim_end_matches('/'));
        let _ = client
            .post(ack_target)
            .header("x-ha-internal-token", internal_token)
            .json(&serde_json::json!({
                "channel": channel,
                "peerNodeId": local_node_id,
                "ackedSeq": next_seq
            }))
            .send()
            .await?;
    }
    state.ha.mark_sync_success().await;
    state.proxy.flush_ha_state_writes().await?;
    Ok(())
}

async fn run_ha_peer_sync_once(
    state: &Arc<AppState>,
    client: &reqwest::Client,
    internal_token: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let status = state.ha.status().await;
    let local_node_id = status.node_id.clone();
    let mut peer_views = Vec::new();
    let mut control_source_node_id: Option<String> = None;
    let peer_configs = state
        .ha
        .peer_nodes()
        .into_iter()
        .filter(|peer| peer.node_id != local_node_id)
        .collect::<Vec<_>>();
    if peer_configs.is_empty() {
        return Err("HA peer sync has no configured peers".into());
    }
    for peer in peer_configs {
        match fetch_internal_ha_status(client, &peer, internal_token).await {
            Ok(peer_status) => {
                if peer_status.allows_full_writes && control_source_node_id.is_none() {
                    control_source_node_id = Some(peer.node_id.clone());
                }
                peer_views.push((peer, peer_status));
            }
            Err(err) => {
                tracing::warn!(
                    component = "ha",
                    event = "peer_status_discovery_failed",
                    peer_node_id = peer.node_id,
                    err = %err,
                    "HA peer status discovery failed"
                );
            }
        }
    }
    if peer_views.is_empty() {
        return Err("HA peer sync reached no peers".into());
    }
    for (peer, peer_status) in peer_views {
        let is_control_source =
            control_source_node_id.as_deref() == Some(peer.node_id.as_str())
                || peer_status.allows_full_writes;
        let channels = if is_control_source {
            vec![
                tavily_hikari::HaSyncChannel::Control,
                tavily_hikari::HaSyncChannel::Billing,
                tavily_hikari::HaSyncChannel::Runtime,
            ]
        } else {
            vec![
                tavily_hikari::HaSyncChannel::Billing,
                tavily_hikari::HaSyncChannel::Runtime,
            ]
        };
        run_ha_sync_once_for_peer(
            state,
            client,
            &peer.admin_base_url,
            &peer.node_id,
            internal_token,
            &channels,
        )
        .await?;
    }
    state.ha.mark_sync_success().await;
    state.proxy.flush_ha_state_writes().await?;
    Ok(())
}

async fn run_ha_sync_once_for_peer(
    state: &Arc<AppState>,
    client: &reqwest::Client,
    source_url: &str,
    peer_node_id: &str,
    internal_token: &str,
    channels: &[tavily_hikari::HaSyncChannel],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let local_node_id = state.ha.status().await.node_id;
    for &channel in channels {
        let seq_key = format!("peer_{peer_node_id}_{}_applied_seq", channel.as_str());
        let baseline_key = format!("peer_{peer_node_id}_{}_baseline_applied", channel.as_str());
        let baseline_report_key = format!("peer_{peer_node_id}_{}_baseline", channel.as_str());
        let applied_seq = state
            .proxy
            .get_ha_sync_watermark(&seq_key)
            .await?
            .unwrap_or(0);
        let baseline_applied = state
            .proxy
            .get_ha_sync_watermark(&baseline_key)
            .await?
            .unwrap_or(0)
            > 0;
        let mut next_seq = applied_seq;

        if !baseline_applied {
            let target = format!(
                "{}/api/admin/ha/baseline?channel={}",
                source_url.trim_end_matches('/'),
                channel.as_str()
            );
            let response = client
                .get(target)
                .header("x-ha-internal-token", internal_token)
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(format!(
                    "peer baseline request failed for {} from {} with {}",
                    channel.as_str(),
                    peer_node_id,
                    response.status()
                )
                .into());
            }
            let baseline_mode = if state.ha.dual_active_enabled()
                && channel != tavily_hikari::HaSyncChannel::Control
            {
                tavily_hikari::HaBaselineApplyMode::Upsert
            } else {
                tavily_hikari::HaBaselineApplyMode::Replace
            };
            let peer_import_node_id = if state.ha.dual_active_enabled()
                && matches!(
                    channel,
                    tavily_hikari::HaSyncChannel::Billing | tavily_hikari::HaSyncChannel::Runtime
                )
            {
                Some(peer_node_id)
            } else {
                None
            };
            let result = apply_ha_baseline_response_stream(
                state.as_ref(),
                channel,
                response,
                baseline_mode,
                peer_import_node_id,
            )
            .await?;
            next_seq = result.high_watermark;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &baseline_report_key,
                    Some(source_url),
                    Some(&local_node_id),
                    result.high_watermark,
                    Some(&format!("rows={}", result.row_count)),
                )
                .await?;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &seq_key,
                    Some(source_url),
                    Some(&local_node_id),
                    result.high_watermark,
                    Some("baseline"),
                )
                .await?;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &baseline_key,
                    Some(source_url),
                    Some(&local_node_id),
                    1,
                    Some("baseline applied"),
                )
                .await?;
            state.proxy.flush_ha_state_writes().await?;
        }

        let target = format!(
            "{}/api/admin/ha/events?channel={}&after={}&limit=1000",
            source_url.trim_end_matches('/'),
            channel.as_str(),
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
                    &seq_key,
                    Some(source_url),
                    Some(&local_node_id),
                    0,
                    Some(reset_detail),
                )
                .await?;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &baseline_key,
                    Some(source_url),
                    Some(&local_node_id),
                    0,
                    Some(reset_detail),
                )
                .await?;
            state.proxy.flush_ha_state_writes().await?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "peer events request failed for {} from {} with {}",
                channel.as_str(),
                peer_node_id,
                response.status()
            )
            .into());
        }
        let peer_import_node_id = if state.ha.dual_active_enabled()
            && matches!(
                channel,
                tavily_hikari::HaSyncChannel::Billing | tavily_hikari::HaSyncChannel::Runtime
            )
        {
            Some(peer_node_id)
        } else {
            None
        };
        let result = match apply_ha_events_response_stream(
            state.as_ref(),
            channel,
            response,
            peer_import_node_id,
        )
        .await
        {
            Ok(result) => result,
            Err(err) if is_ha_retryable_foreign_key_gap(&*err) => {
                let reset_detail = "foreign key gap during events apply; baseline required";
                state
                    .proxy
                    .persist_ha_sync_watermark(
                        &seq_key,
                        Some(source_url),
                        Some(&local_node_id),
                        0,
                        Some(reset_detail),
                    )
                    .await?;
                state
                    .proxy
                    .persist_ha_sync_watermark(
                        &baseline_key,
                        Some(source_url),
                        Some(&local_node_id),
                        0,
                        Some(reset_detail),
                    )
                    .await?;
                state.proxy.flush_ha_state_writes().await?;
                continue;
            }
            Err(err) => return Err(err),
        };
        if result.high_watermark > next_seq {
            next_seq = result.high_watermark;
            state
                .proxy
                .persist_ha_sync_watermark(
                    &seq_key,
                    Some(source_url),
                    Some(&local_node_id),
                    next_seq,
                    Some(&format!("events={}", result.row_count)),
                )
                .await?;
            state.proxy.flush_ha_state_writes().await?;
        }
        let ack_target = format!("{}/api/admin/ha/events/ack", source_url.trim_end_matches('/'));
        let _ = client
            .post(ack_target)
            .header("x-ha-internal-token", internal_token)
            .json(&serde_json::json!({
                "channel": channel,
                "peerNodeId": local_node_id,
                "ackedSeq": next_seq
            }))
            .send()
            .await?;
    }
    Ok(())
}

fn spawn_business_background_tasks(state: Arc<AppState>) {
    spawn_maintenance_worker(state.clone());
    spawn_quota_sync_scheduler(state.clone());
    spawn_token_usage_rollup_scheduler(state.clone());
    spawn_auth_token_logs_gc_scheduler(state.clone());
    spawn_ha_outbox_gc_scheduler(state.clone());
    spawn_mcp_sessions_gc_scheduler(state.clone());
    spawn_mcp_session_init_backoffs_gc_scheduler(state.clone());
    spawn_request_logs_gc_scheduler(state.clone());
    if state.linuxdo_oauth.is_user_sync_scheduler_enabled() {
        spawn_linuxdo_user_status_sync_scheduler(state.clone());
    }
    spawn_linuxdo_user_tag_binding_refresh_scheduler(state.clone());
    let _forward_proxy_geo_refresh_scheduler = spawn_forward_proxy_geo_refresh_scheduler(state.clone());
    spawn_forward_proxy_maintenance_scheduler(state.clone());
    spawn_db_compaction_scheduler(state);
}

fn background_tasks_disabled_via_env() -> bool {
    std::env::var("TAVILY_DISABLE_BACKGROUND_TASKS")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

async fn spawn_background_tasks_for_current_role(state: Arc<AppState>) -> bool {
    if !state.ha.allows_full_writes().await {
        return false;
    }
    if background_tasks_disabled_via_env() {
        tracing::info!(
            component = "startup",
            event = "background_tasks_disabled_via_env",
            "background tasks disabled via TAVILY_DISABLE_BACKGROUND_TASKS"
        );
        return false;
    }
    spawn_business_background_tasks(state);
    true
}

fn spawn_post_ready_serving_tasks_for_status(
    state: Arc<AppState>,
    status: &tavily_hikari::HaStatusView,
) -> bool {
    if !status.allows_full_writes {
        state
            .proxy
            .reset_post_ready_serving_tasks_for_writable_tenure();
        return false;
    }
    if background_tasks_disabled_via_env() {
        tracing::info!(
            component = "startup",
            event = "post_ready_tasks_disabled_via_env",
            "post-ready non-core tasks disabled via TAVILY_DISABLE_BACKGROUND_TASKS"
        );
        return false;
    }
    match state
        .proxy
        .claim_post_ready_serving_tasks_for_writable_tenure()
    {
        tavily_hikari::PostReadyServingTasksClaim::Start => {}
        tavily_hikari::PostReadyServingTasksClaim::Suppressed { should_log } => {
            if should_log {
                tracing::info!(
                    component = "startup",
                    event = "post_ready_tasks_suppressed",
                    role = status.role.as_str(),
                    node_id = %status.node_id,
                    pressure_spawned = false,
                    business_calls_spawned = false,
                    reason = "already_started_for_writable_tenure",
                    "post-ready non-core tasks already started for current writable tenure"
                );
            }
            return false;
        }
    }
    let pressure = state.proxy.spawn_server_pressure_buckets_rebuild_once();
    let business_calls = state.proxy.spawn_user_business_calls_1h_backfill_once();
    tracing::info!(
        component = "startup",
        event = "post_ready_tasks_started",
        role = status.role.as_str(),
        node_id = %status.node_id,
        pressure_spawned = pressure,
        business_calls_spawned = business_calls,
        reason = "writable_tenure_entered",
        "post-ready non-core tasks started for writable tenure"
    );
    pressure || business_calls
}

async fn refresh_admin_password_state_after_ha_apply(
    state: &AppState,
    channel: tavily_hikari::HaSyncChannel,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if channel != tavily_hikari::HaSyncChannel::Control {
        return Ok(());
    }
    let settings = state.proxy.get_admin_password_settings().await?;
    state.builtin_admin.apply_persisted_settings(settings);
    Ok(())
}

async fn apply_ha_baseline_response_stream(
    state: &AppState,
    channel: tavily_hikari::HaSyncChannel,
    response: reqwest::Response,
    mode: tavily_hikari::HaBaselineApplyMode,
    peer_import_node_id: Option<&str>,
) -> Result<tavily_hikari::HaApplyResult, Box<dyn std::error::Error + Send + Sync>> {
    let started = Instant::now();
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let reader = StreamReader::new(stream);
    let decoder = ZstdDecoder::new(BufReader::new(reader));
    let mut lines = BufReader::new(decoder).lines();
    let mut session = state
        .proxy
        .begin_ha_baseline_apply_with_mode(channel, mode)
        .await?;
    loop {
        let next_line = match lines.next_line().await {
            Ok(next_line) => next_line,
            Err(err) => {
                let _ = session.abort().await;
                return Err(err.into());
            }
        };
        let Some(line) = next_line else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let apply_result = if let Some(peer_node_id) = peer_import_node_id {
            session.apply_line_with_peer_import(trimmed, peer_node_id).await
        } else {
            session.apply_line(trimmed).await
        };
        if let Err(err) = apply_result {
            let _ = session.abort().await;
            return Err(err.into());
        }
    }
    let result = session.finish().await.map_err(Box::<dyn std::error::Error + Send + Sync>::from)?;
    refresh_admin_password_state_after_ha_apply(state, channel).await?;
    let outbox = state.proxy.ha_channel_outbox_stats(channel, None).await?;
    let memory = tavily_hikari::capture_runtime_memory_snapshot();
    tracing::info!(
        component = "ha",
        event = "baseline_import_completed",
        elapsed_ms = started.elapsed().as_millis() as u64,
        channel = channel.as_str(),
        baseline_mode = match mode {
            tavily_hikari::HaBaselineApplyMode::Replace => "replace",
            tavily_hikari::HaBaselineApplyMode::Upsert => "upsert",
        },
        row_count = result.row_count as u64,
        peer_import_node = peer_import_node_id.unwrap_or(""),
        payload_bytes = result.payload_bytes as u64,
        outbox_row_count = outbox.row_count,
        outbox_oldest_age_secs = outbox.oldest_age_secs,
        outbox_ack_lag = outbox.ack_lag,
        memory_current_bytes = memory.memory_current_bytes.unwrap_or_default(),
        memory_limit_bytes = memory.memory_limit_bytes.unwrap_or_default(),
        headroom_bytes = memory.headroom_bytes.unwrap_or_default(),
        process_rss_bytes = memory.process_rss_bytes.unwrap_or_default(),
        child_process_rss_bytes = memory.child_process_rss_bytes.unwrap_or_default(),
        process_group_rss_bytes = memory.process_group_rss_bytes.unwrap_or_default(),
        process_hwm_bytes = memory.process_hwm_bytes.unwrap_or_default(),
        process_swap_bytes = memory.process_swap_bytes.unwrap_or_default(),
        "ha perf"
    );
    Ok(result)
}

async fn apply_ha_events_response_stream(
    state: &AppState,
    channel: tavily_hikari::HaSyncChannel,
    response: reqwest::Response,
    peer_import_node_id: Option<&str>,
) -> Result<tavily_hikari::HaApplyResult, Box<dyn std::error::Error + Send + Sync>> {
    let started = Instant::now();
    let stream = response
        .bytes_stream()
        .map(|chunk| chunk.map_err(std::io::Error::other));
    let reader = StreamReader::new(stream);
    let decoder = ZstdDecoder::new(BufReader::new(reader));
    let mut lines = BufReader::new(decoder).lines();
    let mut session = state.proxy.begin_ha_events_apply(channel).await?;
    loop {
        let next_line = match lines.next_line().await {
            Ok(next_line) => next_line,
            Err(err) => {
                let _ = session.abort().await;
                return Err(err.into());
            }
        };
        let Some(line) = next_line else {
            break;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let apply_result = if let Some(peer_node_id) = peer_import_node_id {
            session.apply_line_with_peer_import(trimmed, peer_node_id).await
        } else {
            session.apply_line(trimmed).await
        };
        if let Err(err) = apply_result {
            let _ = session.abort().await;
            return Err(err.into());
        }
    }
    let result = session.finish().await.map_err(Box::<dyn std::error::Error + Send + Sync>::from)?;
    refresh_admin_password_state_after_ha_apply(state, channel).await?;
    let outbox = state.proxy.ha_channel_outbox_stats(channel, None).await?;
    let memory = tavily_hikari::capture_runtime_memory_snapshot();
    tracing::info!(
        component = "ha",
        event = "events_import_completed",
        elapsed_ms = started.elapsed().as_millis() as u64,
        channel = channel.as_str(),
        row_count = result.row_count as u64,
        peer_import_node = peer_import_node_id.unwrap_or(""),
        payload_bytes = result.payload_bytes as u64,
        outbox_row_count = outbox.row_count,
        outbox_oldest_age_secs = outbox.oldest_age_secs,
        outbox_ack_lag = outbox.ack_lag,
        memory_current_bytes = memory.memory_current_bytes.unwrap_or_default(),
        memory_limit_bytes = memory.memory_limit_bytes.unwrap_or_default(),
        headroom_bytes = memory.headroom_bytes.unwrap_or_default(),
        process_rss_bytes = memory.process_rss_bytes.unwrap_or_default(),
        child_process_rss_bytes = memory.child_process_rss_bytes.unwrap_or_default(),
        process_group_rss_bytes = memory.process_group_rss_bytes.unwrap_or_default(),
        process_hwm_bytes = memory.process_hwm_bytes.unwrap_or_default(),
        process_swap_bytes = memory.process_swap_bytes.unwrap_or_default(),
        "ha perf"
    );
    Ok(result)
}

fn spawn_ha_edgeone_authority_task(state: Arc<AppState>) {
    if !state.ha.edgeone_authority_enabled() {
        return;
    }
    tokio::spawn(async move {
        loop {
            state
                .proxy
                .backend_time()
                .sleep(std::time::Duration::from_secs(5))
                .await;
            match state.ha.refresh_authoritative_role().await {
                Ok(mut status) => {
                    if state.ha.dual_active_enabled() {
                        let leader = state.proxy.get_ha_full_master_node_id().await.unwrap_or_else(|err| {
                            tracing::warn!(
                                component = "ha",
                                event = "leader_key_lookup_failed",
                                err = %err,
                                "HA leader key lookup warning"
                            );
                            None
                        });
                        if let Err(err) = state.ha.apply_dual_active_leader(leader).await {
                            tracing::warn!(
                                component = "ha",
                                event = "dual_active_leader_apply_failed",
                                err = %err,
                                "HA dual-active leader apply warning"
                            );
                        }
                        status = state.ha.status().await;
                    }
                    if !status.allows_full_writes {
                        state
                            .proxy
                            .reset_post_ready_serving_tasks_for_writable_tenure();
                    }
                    if let Err(err) =
                        sync_forward_proxy_runtime_for_status(state.proxy.clone(), &status).await
                    {
                        tracing::warn!(
                            component = "forward_proxy",
                            event = "authority_runtime_role_sync_failed",
                            role = status.role.as_str(),
                            err = %err,
                            "forward-proxy authority runtime role sync failed"
                        );
                    } else {
                        let _ = spawn_post_ready_serving_tasks_for_status(state.clone(), &status);
                    }
                    let node_id = status.node_id.clone();
                    let edgeone_origin = status.edgeone_origin.clone();
                    let source_effective = status.ha_source_effective.clone();
                    let message = status.message.clone();
                    if let Err(err) = state
                        .proxy
                        .persist_ha_node_state(
                            &node_id,
                            status.role,
                            edgeone_origin.as_deref(),
                            source_effective.as_ref(),
                            message.as_deref(),
                        )
                        .await
                    {
                        tracing::warn!(
                            component = "ha",
                            event = "authority_state_persist_failed",
                            err = %err,
                            "HA authority state persist failed"
                        );
                    } else if let Err(err) = state.proxy.flush_ha_state_writes().await {
                        tracing::warn!(
                            component = "ha",
                            event = "authority_state_persist_failed",
                            err = %err,
                            "HA authority state persist failed"
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        component = "ha",
                        event = "authority_refresh_failed",
                        err = %err,
                        "HA authority refresh failed"
                    );
                }
            }
        }
    });
}

fn spawn_ha_control_plane_gc_task(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            state
                .proxy
                .backend_time()
                .sleep(std::time::Duration::from_secs(60 * 60))
                .await;
            if let Err(err) = state.proxy.gc_ha_control_plane_events().await {
                tracing::warn!(
                    component = "ha",
                    event = "control_plane_gc_failed",
                    err = %err,
                    "HA control-plane event GC failed"
                );
            }
        }
    });
}

async fn wait_for_ctrl_c() -> &'static str {
    match signal::ctrl_c().await {
        Ok(()) => "ctrl_c",
        Err(err) => {
            tracing::error!(
                component = "shutdown",
                event = "ctrl_c_listener_failed",
                err = %err,
                "Failed to listen for Ctrl+C"
            );
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
            tracing::error!(
                component = "shutdown",
                event = "sigterm_listener_failed",
                err = %err,
                "Failed to listen for SIGTERM"
            );
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

    tracing::info!(
        component = "shutdown",
        event = "shutdown_signal_received",
        signal,
        "shutdown signal received, waiting for in-flight requests to finish"
    );
}

const BODY_LIMIT: usize = 16 * 1024 * 1024; // 16 MiB 默认限制
const DEFAULT_LOG_LIMIT: usize = 200;

#[cfg(test)]
mod serve_tests {
    use super::*;
    use tavily_hikari::DEFAULT_UPSTREAM;

    #[tokio::test]
    async fn ha_control_apply_refreshes_in_memory_admin_password_state() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("admin-password-ha-refresh.db");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-ha-admin-password-refresh".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let builtin_admin = BuiltinAdminAuth::new(true, Some("env-password".to_string()), None);
        let state = AppState {
            proxy: proxy.clone(),
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin,
            admin_passkey: AdminPasskeyOptions::disabled(),
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
            dashboard_overview_cache: new_dashboard_overview_cache(),
        };

        let session_token = state
            .builtin_admin
            .login("env-password")
            .expect("env password login should succeed");
        state.builtin_admin.remember_session(session_token.clone());
        let mut session_headers = HeaderMap::new();
        session_headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{BUILTIN_ADMIN_COOKIE_NAME}={session_token}"))
                .expect("cookie header should be valid"),
        );
        assert!(state.builtin_admin.is_admin(&session_headers));

        proxy
            .set_admin_login_totp_required(true)
            .await
            .expect("enable login TOTP requirement in store");
        refresh_admin_password_state_after_ha_apply(
            &state,
            tavily_hikari::HaSyncChannel::Control,
        )
        .await
        .expect("refresh login TOTP state");
        assert!(state.builtin_admin.login_totp_required());
        assert!(!state.builtin_admin.is_admin(&session_headers));
        assert!(state.builtin_admin.login("env-password").is_some());

        proxy
            .disable_admin_password_preserving_login(true, false)
            .await
            .expect("disable password in store");
        refresh_admin_password_state_after_ha_apply(
            &state,
            tavily_hikari::HaSyncChannel::Control,
        )
        .await
        .expect("refresh disabled password state");
        assert!(!state.builtin_admin.is_enabled());
        assert!(state.builtin_admin.login("env-password").is_none());

        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{db_str}"))
            .await
            .expect("connect test db");
        sqlx::query("DELETE FROM admin_password_settings")
            .execute(&pool)
            .await
            .expect("delete persisted password settings");
        pool.close().await;
        refresh_admin_password_state_after_ha_apply(
            &state,
            tavily_hikari::HaSyncChannel::Control,
        )
        .await
        .expect("refresh missing password state");
        assert!(state.builtin_admin.is_enabled());
        assert!(state.builtin_admin.login("env-password").is_some());
    }

    #[tokio::test]
    async fn passkey_admin_session_sets_maintenance_actor() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("passkey-maintenance-actor.db");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            vec!["tvly-passkey-maintenance-actor".to_string()],
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        proxy
            .upsert_admin_passkey_credential(
                "credential-actor-1234567890",
                r#"{"credential":1}"#,
                None,
            )
            .await
            .expect("insert passkey credential");
        let session = proxy
            .create_admin_passkey_session(Some("credential-actor-1234567890"), 120)
            .await
            .expect("create passkey session");
        let state = AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions {
                enabled: true,
                rp_id: Some("example.com".to_string()),
                rp_origin: Some("https://example.com".to_string()),
                challenge_ttl_secs: 300,
                session_max_age_secs: 120,
            },
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
            dashboard_overview_cache: new_dashboard_overview_cache(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{ADMIN_PASSKEY_COOKIE_NAME}={}", session.token))
                .expect("cookie header should be valid"),
        );

        assert!(is_admin_request(&state, &headers).await);
        let actor = admin_maintenance_actor(&state, &headers, None).await;

        assert_eq!(
            actor.actor_display_name.as_deref(),
            Some("admin-passkey:credential-actor")
        );
    }

    #[tokio::test]
    async fn passkey_authentication_start_is_available_on_standby() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join("standby-passkey-auth.db");
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(
            Vec::<String>::new(),
            DEFAULT_UPSTREAM,
            &db_str,
        )
        .await
        .expect("proxy created");
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(None, None, None, None),
            forward_auth_enabled: false,
            builtin_admin: BuiltinAdminAuth::new(false, None, None),
            admin_passkey: AdminPasskeyOptions {
                enabled: true,
                rp_id: Some("hikari.example.com".to_string()),
                rp_origin: Some("https://hikari.example.com".to_string()),
                challenge_ttl_secs: 300,
                session_max_age_secs: 60 * 60,
            },
            linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig {
                mode: tavily_hikari::HaMode::ActiveStandby,
                node_id: "node-passkey-standby".to_string(),
                database_path: Some(db_str),
                sync_source_url: Some("http://127.0.0.1:59999".to_string()),
                internal_token: Some("ha-test-token".to_string()),
                sync_interval_secs: 5,
                ..tavily_hikari::HaConfig::default()
            }),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
            dashboard_overview_cache: new_dashboard_overview_cache(),
        });

        assert!(!state.ha.allows_full_writes().await);

        let result = post_admin_passkey_authentication_start(State(state)).await;

        assert_eq!(result.expect_err("no passkey credentials yet"), StatusCode::CONFLICT);
    }
}

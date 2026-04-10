#[derive(Deserialize)]
struct JobsQuery {
    limit: Option<usize>,
    group: Option<String>,
    page: Option<usize>,
    per_page: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedJobsView {
    items: Vec<JobLogView>,
    total: i64,
    page: usize,
    per_page: usize,
    group_counts: JobGroupCountsView,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobGroupCountsView {
    all: i64,
    quota: i64,
    usage: i64,
    logs: i64,
    geo: i64,
    linuxdo: i64,
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<JobsQuery>,
) -> Result<Json<PaginatedJobsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let page = q.page.unwrap_or(1).max(1);
    let per_page = q.per_page.or(q.limit).unwrap_or(10).clamp(1, 100);
    let group = q.group.as_deref().unwrap_or("all");

    state
        .proxy
        .list_recent_jobs_paginated(group, page, per_page)
        .await
        .map(|(items, total, group_counts)| {
            let view_items = items
                .into_iter()
                .map(|j| JobLogView {
                    id: j.id,
                    job_type: j.job_type,
                    key_id: j.key_id,
                    key_group: j.key_group,
                    status: j.status,
                    attempt: j.attempt,
                    message: j.message,
                    started_at: j.started_at,
                    finished_at: j.finished_at,
                })
                .collect();
            Json(PaginatedJobsView {
                items: view_items,
                total,
                page,
                per_page,
                group_counts: JobGroupCountsView {
                    all: group_counts.all,
                    quota: group_counts.quota,
                    usage: group_counts.usage,
                    logs: group_counts.logs,
                    geo: group_counts.geo,
                    linuxdo: group_counts.linuxdo,
                },
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

// ---- Key detail & manual quota sync ----

async fn get_api_key_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeyView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let items = state
        .proxy
        .get_api_key_metric(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(found) = items {
        Ok(Json(ApiKeyView::from_detail(found)))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn post_sync_key_usage(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    match run_manual_key_quota_sync(state.as_ref(), &id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT.into_response()),
        Err(err) => Ok((
            err.status_code,
            Json(json!({
                "error": err.error_code,
                "detail": err.detail,
            })),
        )
            .into_response()),
    }
}

#[derive(Debug)]
struct ManualQuotaSyncError {
    status_code: StatusCode,
    error_code: &'static str,
    detail: String,
}

impl ManualQuotaSyncError {
    fn new(status_code: StatusCode, error_code: &'static str, detail: String) -> Self {
        Self {
            status_code,
            error_code,
            detail,
        }
    }
}

async fn run_manual_key_quota_sync(
    state: &AppState,
    key_id: &str,
) -> Result<(), ManualQuotaSyncError> {
    let job_id = state
        .proxy
        .scheduled_job_start("quota_sync/manual", Some(key_id), 1)
        .await
        .map_err(|err| {
            ManualQuotaSyncError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "sync_failed",
                err.to_string(),
            )
        })?;

    match state
        .proxy
        .sync_key_quota(key_id, &state.usage_base, "quota_sync/manual")
        .await
    {
        Ok((limit, remaining)) => {
            let msg = format!("limit={limit} remaining={remaining}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "success", Some(&msg))
                .await;
            Ok(())
        }
        Err(ProxyError::QuotaDataMissing { reason }) => {
            let msg = format!("quota_data_missing: {reason}");
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&msg))
                .await;
            Err(ManualQuotaSyncError::new(
                StatusCode::BAD_REQUEST,
                "quota_data_missing",
                reason,
            ))
        }
        Err(ProxyError::UsageHttp { status, body }) => {
            let detail = format!("Tavily usage request failed with {status}: {body}");
            let http_status = if status == reqwest::StatusCode::UNAUTHORIZED {
                StatusCode::UNAUTHORIZED
            } else if status == reqwest::StatusCode::FORBIDDEN {
                StatusCode::FORBIDDEN
            } else if status.is_client_error() {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&detail))
                .await;
            Err(ManualQuotaSyncError::new(
                http_status,
                "usage_http",
                detail,
            ))
        }
        Err(err) => {
            let reason = err.to_string();
            let _ = state
                .proxy
                .scheduled_job_finish(job_id, "error", Some(&reason))
                .await;
            Err(ManualQuotaSyncError::new(
                StatusCode::BAD_GATEWAY,
                "sync_failed",
                reason,
            ))
        }
    }
}

async fn delete_api_key_quarantine(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }

    state
        .proxy
        .clear_key_quarantine_by_id_with_actor(
            &id,
            admin_maintenance_actor(state.as_ref(), &headers, None).await,
        )
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|err| {
            eprintln!("clear api key quarantine error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionView {
    backend: String,
    frontend: String,
}

async fn get_versions(State(state): State<Arc<AppState>>) -> Result<Json<VersionView>, StatusCode> {
    let (backend, frontend) = detect_versions(state.static_dir.as_deref());
    Ok(Json(VersionView { backend, frontend }))
}

#[derive(Debug, Serialize)]
struct AdminDebug {
    dev_open_admin: bool,
}

async fn get_admin_debug(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminDebug>, StatusCode> {
    Ok(Json(AdminDebug {
        dev_open_admin: state.dev_open_admin,
    }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    display_name: Option<String>,
    is_admin: bool,
    forward_auth_enabled: bool,
    builtin_auth_enabled: bool,
    allow_registration: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_logged_in: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_avatar_url: Option<String>,
}

fn resolve_linuxdo_avatar_url(
    cfg: &LinuxDoOAuthOptions,
    avatar_template: Option<&str>,
) -> Option<String> {
    let template = avatar_template
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .replace("{size}", "96");

    if let Ok(url) = reqwest::Url::parse(&template)
        && matches!(url.scheme(), "http" | "https")
    {
        return resolve_absolute_linuxdo_avatar_url(cfg, url.as_str());
    }
    if template.starts_with("//") {
        return resolve_absolute_linuxdo_avatar_url(cfg, &format!("https:{template}"));
    }

    let base = linuxdo_avatar_origin(cfg, &template)?;
    join_avatar_path(&base, &template)
}

fn resolve_absolute_linuxdo_avatar_url(
    cfg: &LinuxDoOAuthOptions,
    template: &str,
) -> Option<String> {
    let mut url = reqwest::Url::parse(template).ok()?;
    if origin_is_public_browser_safe(&url) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_fragment(None);
        return Some(url.to_string());
    }

    let mut relative = url.path().to_string();
    if let Some(query) = url.query() {
        relative.push('?');
        relative.push_str(query);
    }
    let base = linuxdo_avatar_origin(cfg, &relative)?;
    let normalized_relative = normalize_avatar_relative_path(&base, &relative);
    join_avatar_path(&base, &normalized_relative)
}

fn linuxdo_avatar_origin(cfg: &LinuxDoOAuthOptions, template: &str) -> Option<reqwest::Url> {
    linuxdo_avatar_template_origin(template)
        .or_else(|| linuxdo_public_oauth_origin(cfg))
}

fn linuxdo_avatar_template_origin(template: &str) -> Option<reqwest::Url> {
    template
        .trim_start_matches('/')
        .strip_prefix("user_avatar/")
        .and_then(|value| value.split('/').next())
        .filter(|value| !value.is_empty())
        .and_then(|host| origin_from_url(&format!("https://{host}")))
        .filter(origin_is_public_browser_safe)
}

fn linuxdo_public_oauth_origin(cfg: &LinuxDoOAuthOptions) -> Option<reqwest::Url> {
    [
        cfg.userinfo_url.as_str(),
        cfg.authorize_url.as_str(),
        cfg.token_url.as_str(),
    ]
    .into_iter()
    .filter_map(oauth_origin_from_url)
    .find(origin_is_public_browser_safe)
}

fn origin_is_public_browser_safe(origin: &reqwest::Url) -> bool {
    if origin.scheme() != "https" {
        return false;
    }

    let Some(host) = origin.host_str() else {
        return false;
    };
    let host_no_brackets = host.trim_start_matches('[').trim_end_matches(']');
    let canonical_host = host_no_brackets.trim_end_matches('.');
    if canonical_host.is_empty()
        || canonical_host.eq_ignore_ascii_case("localhost")
        || canonical_host.ends_with(".localhost")
        || canonical_host.eq_ignore_ascii_case("lvh.me")
        || canonical_host.ends_with(".lvh.me")
        || canonical_host.ends_with(".local")
        || canonical_host.ends_with(".internal")
        || canonical_host.eq_ignore_ascii_case("localtest.me")
        || canonical_host.ends_with(".localtest.me")
    {
        return false;
    }

    match canonical_host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ipv4_is_public_browser_safe(ip),
        Ok(std::net::IpAddr::V6(ip)) => ipv6_is_public_browser_safe(ip),
        Err(_) => {
            if let Some(ip) = encoded_ipv4_host(canonical_host) {
                return ipv4_is_public_browser_safe(ip);
            }
            hostname_labels_look_public(canonical_host)
        }
    }
}

fn hostname_labels_look_public(host: &str) -> bool {
    let labels = host
        .split('.')
        .filter(|label| !label.is_empty())
        .map(|label| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if labels.len() < 2 {
        return false;
    }

    let suspicious_private_labels = [
        "cluster",
        "corp",
        "home",
        "intra",
        "internal",
        "lan",
        "localhost",
        "local",
        "office",
        "priv",
        "private",
        "svc",
        "vpn",
    ];

    labels[..labels.len().saturating_sub(2)]
        .iter()
        .all(|label| !suspicious_private_labels.contains(&label.as_str()))
}

fn ipv4_is_public_browser_safe(ip: std::net::Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    let is_current_network = a == 0;
    let is_shared = a == 100 && (b & 0b1100_0000) == 0b0100_0000;
    let is_ietf_protocol_assignment = a == 192 && b == 0 && c == 0;
    let is_documentation = (a == 192 && b == 0 && c == 2)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113);
    let is_benchmarking = a == 198 && (b == 18 || b == 19);
    let is_6to4_relay = a == 192 && b == 88 && c == 99;
    let is_multicast_or_reserved = a >= 224;

    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || is_current_network
        || is_shared
        || is_ietf_protocol_assignment
        || is_documentation
        || is_benchmarking
        || is_6to4_relay
        || is_multicast_or_reserved)
}

fn ipv6_is_public_browser_safe(ip: std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    let is_link_local = (segments[0] & 0xffc0) == 0xfe80;
    let is_site_local = (segments[0] & 0xffc0) == 0xfec0;
    let is_multicast = (segments[0] & 0xff00) == 0xff00;
    let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;

    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || is_link_local
        || is_site_local
        || is_multicast
        || is_documentation)
}

fn encoded_ipv4_host(host: &str) -> Option<std::net::Ipv4Addr> {
    let labels = host.split('.').collect::<Vec<_>>();
    let rebinding_suffix_len = rebinding_domain_suffix_len(&labels)?;
    let rebinding_prefix = &labels[..labels.len().checked_sub(rebinding_suffix_len)?];
    if rebinding_prefix.is_empty() {
        return None;
    }

    for window_size in 1..=4 {
        for window in rebinding_prefix.windows(window_size) {
            if let Some(ip) = ipv4_addr_from_encoded_parts(window) {
                return Some(ip);
            }
        }
    }

    for label in rebinding_prefix {
        let dashed = label.split('-').collect::<Vec<_>>();
        for window_size in 1..=4 {
            for window in dashed.windows(window_size) {
                if let Some(ip) = ipv4_addr_from_encoded_parts(window) {
                    return Some(ip);
                }
            }
        }
    }

    None
}

fn rebinding_domain_suffix_len(labels: &[&str]) -> Option<usize> {
    if labels.len() < 2 {
        return None;
    }

    let domain = labels[labels.len() - 2..]
        .iter()
        .map(|label| label.to_ascii_lowercase())
        .collect::<Vec<_>>();
    match domain.as_slice() {
        [prefix, suffix] if prefix == "nip" && suffix == "io" => Some(2),
        [prefix, suffix] if prefix == "sslip" && suffix == "io" => Some(2),
        [prefix, suffix] if prefix == "xip" && suffix == "io" => Some(2),
        _ => None,
    }
}

fn ipv4_addr_from_encoded_parts(parts: &[&str]) -> Option<std::net::Ipv4Addr> {
    match parts.len() {
        1 => {
            let value = parse_ipv4_number(parts[0])?;
            Some(std::net::Ipv4Addr::from(value))
        }
        2 => {
            let first = parse_ipv4_number(parts[0])?;
            let second = parse_ipv4_number(parts[1])?;
            if first > 0xff || second > 0x00ff_ffff {
                return None;
            }
            Some(std::net::Ipv4Addr::new(
                first as u8,
                ((second >> 16) & 0xff) as u8,
                ((second >> 8) & 0xff) as u8,
                (second & 0xff) as u8,
            ))
        }
        3 => {
            let first = parse_ipv4_number(parts[0])?;
            let second = parse_ipv4_number(parts[1])?;
            let third = parse_ipv4_number(parts[2])?;
            if first > 0xff || second > 0xff || third > 0xffff {
                return None;
            }
            Some(std::net::Ipv4Addr::new(
                first as u8,
                second as u8,
                ((third >> 8) & 0xff) as u8,
                (third & 0xff) as u8,
            ))
        }
        4 => {
            let mut octets = [0_u8; 4];
            for (index, part) in parts.iter().enumerate() {
                let value = parse_ipv4_number(part)?;
                if value > 0xff {
                    return None;
                }
                octets[index] = value as u8;
            }
            Some(std::net::Ipv4Addr::new(
                octets[0], octets[1], octets[2], octets[3],
            ))
        }
        _ => None,
    }
}

fn parse_ipv4_number(value: &str) -> Option<u32> {
    if value.is_empty() {
        return None;
    }

    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        if hex.is_empty() {
            return None;
        }
        return u32::from_str_radix(hex, 16).ok();
    }

    if value.len() > 1 && value.starts_with('0') {
        return u32::from_str_radix(value, 8).ok();
    }

    value.parse::<u32>().ok()
}

fn normalize_avatar_relative_path(base: &reqwest::Url, value: &str) -> String {
    let trimmed = value.trim_start_matches('/');
    let base_path = base.path().trim_end_matches('/');
    if base_path.is_empty() {
        return trimmed.to_string();
    }

    let base_without_leading = base_path.trim_start_matches('/');
    if trimmed == base_without_leading {
        return String::new();
    }
    if let Some(stripped) = trimmed.strip_prefix(base_without_leading) {
        if stripped.is_empty() {
            return String::new();
        }
        if let Some(next) = stripped.strip_prefix('/') {
            return next.to_string();
        }
    }

    trimmed.to_string()
}

fn origin_from_url(value: &str) -> Option<reqwest::Url> {
    let mut origin = reqwest::Url::parse(value).ok()?;
    let _ = origin.set_username("");
    let _ = origin.set_password(None);
    origin.set_path("/");
    origin.set_query(None);
    origin.set_fragment(None);
    Some(origin)
}

fn oauth_origin_from_url(value: &str) -> Option<reqwest::Url> {
    let mut origin = reqwest::Url::parse(value).ok()?;
    let _ = origin.set_username("");
    let _ = origin.set_password(None);
    origin.set_path(&oauth_origin_base_path(origin.path()));
    origin.set_query(None);
    origin.set_fragment(None);
    Some(origin)
}

fn oauth_origin_base_path(path: &str) -> String {
    for suffix in ["/api/user", "/oauth2/authorize", "/oauth2/token"] {
        if let Some(prefix) = path.strip_suffix(suffix) {
            if prefix.is_empty() {
                return "/".to_string();
            }
            return format!("{}/", prefix.trim_end_matches('/'));
        }
    }
    "/".to_string()
}

fn join_avatar_path(base: &reqwest::Url, value: &str) -> Option<String> {
    let relative = value.trim_start_matches('/');
    let next = if relative.is_empty() {
        base.clone()
    } else {
        base.join(relative).ok()?
    };
    Some(next.to_string())
}

#[cfg(test)]
mod avatar_url_tests {
    use super::*;

    #[test]
    fn resolve_linuxdo_avatar_url_prefers_public_host_from_template() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "http://oauth.internal:3000/oauth2/authorize".to_string();
        cfg.userinfo_url = "http://discourse.internal:3000/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/connect.linux.do/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://connect.linux.do/user_avatar/connect.linux.do/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_falls_back_to_configured_origin_for_hostless_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png"))
                .as_deref(),
            Some("https://forum.example.com/avatar/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_preserves_oauth_subpaths_for_hostless_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/discourse/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/discourse/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png"))
                .as_deref(),
            Some("https://forum.example.com/discourse/avatar/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_private_oauth_origins_for_hostless_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "http://oauth.internal:3000/oauth2/authorize".to_string();
        cfg.token_url = "http://oauth.internal:3000/oauth2/token".to_string();
        cfg.userinfo_url = "http://discourse.internal:3000/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_salvages_unsafe_absolute_templates() {
        let cfg = LinuxDoOAuthOptions::disabled();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("http://oauth.internal:3000/user_avatar/connect.linux.do/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://connect.linux.do/user_avatar/connect.linux.do/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_treats_absolute_schemes_case_insensitively() {
        let cfg = LinuxDoOAuthOptions::disabled();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("HTTPS://cdn.example.com/user_avatar/connect.linux.do/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://cdn.example.com/user_avatar/connect.linux.do/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_ignores_private_hosts_embedded_in_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/discourse.internal/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/user_avatar/discourse.internal/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_private_ipv6_origins() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://[fe80::1]/oauth2/authorize".to_string();
        cfg.token_url = "https://[fe80::1]/oauth2/token".to_string();
        cfg.userinfo_url = "https://[fe80::1]/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_sanitizes_protocol_relative_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "http://oauth.internal:3000/oauth2/authorize".to_string();
        cfg.token_url = "http://oauth.internal:3000/oauth2/token".to_string();
        cfg.userinfo_url = "http://discourse.internal:3000/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("//127.0.0.1/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_non_global_ipv4_origins() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://100.64.0.1/oauth2/authorize".to_string();
        cfg.token_url = "https://100.64.0.1/oauth2/token".to_string();
        cfg.userinfo_url = "https://100.64.0.1/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_single_label_oauth_origins() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.userinfo_url = "https://discourse/api/user".to_string();
        cfg.authorize_url = "https://oauth/oauth2/authorize".to_string();
        cfg.token_url = "https://oauth/oauth2/token".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_ignores_single_label_hosts_embedded_in_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/discourse/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/user_avatar/discourse/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_keeps_numbered_public_cdn_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://203-0-113.example.com/oauth2/authorize".to_string();
        cfg.token_url = "https://203-0-113.example.com/oauth2/token".to_string();
        cfg.userinfo_url = "https://203-0-113.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")).as_deref(),
            Some("https://203-0-113.example.com/avatar/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_keeps_dotted_public_cdn_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://cdn.203.0.113.example.com/oauth2/authorize".to_string();
        cfg.token_url = "https://cdn.203.0.113.example.com/oauth2/token".to_string();
        cfg.userinfo_url = "https://cdn.203.0.113.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")).as_deref(),
            Some("https://cdn.203.0.113.example.com/avatar/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_private_rebinding_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://127.0.0.1.nip.io/oauth2/authorize".to_string();
        cfg.token_url = "https://127.0.0.1.nip.io/oauth2/token".to_string();
        cfg.userinfo_url = "https://127.0.0.1.nip.io/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_private_xip_rebinding_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://127.0.0.1.xip.io/oauth2/authorize".to_string();
        cfg.token_url = "https://127.0.0.1.xip.io/oauth2/token".to_string();
        cfg.userinfo_url = "https://127.0.0.1.xip.io/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_private_rebinding_subdomains() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://foo.127.0.0.1.nip.io/oauth2/authorize".to_string();
        cfg.token_url = "https://foo.127.0.0.1.nip.io/oauth2/token".to_string();
        cfg.userinfo_url = "https://foo.127.0.0.1.nip.io/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_private_split_horizon_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.corp.example.com/oauth2/authorize".to_string();
        cfg.token_url = "https://forum.corp.example.com/oauth2/token".to_string();
        cfg.userinfo_url = "https://forum.corp.example.com/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_dashed_rebinding_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://foo-127-0-0-1.sslip.io/oauth2/authorize".to_string();
        cfg.token_url = "https://foo-127-0-0-1.sslip.io/oauth2/token".to_string();
        cfg.userinfo_url = "https://foo-127-0-0-1.sslip.io/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_loopback_alias_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://oauth.lvh.me/oauth2/authorize".to_string();
        cfg.token_url = "https://oauth.lvh.me/oauth2/token".to_string();
        cfg.userinfo_url = "https://oauth.lvh.me/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_trailing_dot_loopback_alias_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://foo.localhost./oauth2/authorize".to_string();
        cfg.token_url = "https://foo.localhost./oauth2/token".to_string();
        cfg.userinfo_url = "https://foo.localhost./api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_omits_hostless_letter_avatar_proxy_without_public_origin() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://oauth.internal/oauth2/authorize".to_string();
        cfg.token_url = "https://oauth.internal/oauth2/token".to_string();
        cfg.userinfo_url = "https://oauth.internal/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(&cfg, Some("/letter_avatar_proxy/v4/letter/a/96.png")),
            None
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_ignores_rebinding_hosts_embedded_in_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/192-168-1-1.sslip.io/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/user_avatar/192-168-1-1.sslip.io/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_ignores_rebinding_subdomains_embedded_in_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/foo.127-0-0-1.sslip.io/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/user_avatar/foo.127-0-0-1.sslip.io/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_ignores_dashed_rebinding_hosts_embedded_in_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/foo-127-0-0-1.sslip.io/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/user_avatar/foo-127-0-0-1.sslip.io/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_avoids_decimal_rebinding_hosts() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://2130706433.nip.io/oauth2/authorize".to_string();
        cfg.token_url = "https://2130706433.nip.io/oauth2/token".to_string();
        cfg.userinfo_url = "https://2130706433.nip.io/api/user".to_string();

        assert_eq!(resolve_linuxdo_avatar_url(&cfg, Some("/avatar/linuxdo_alice/{size}/1_2.png")), None);
    }

    #[test]
    fn resolve_linuxdo_avatar_url_ignores_hex_rebinding_hosts_embedded_in_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/oauth2/authorize".to_string();
        cfg.userinfo_url = "https://forum.example.com/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("/user_avatar/0x7f000001.sslip.io/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/user_avatar/0x7f000001.sslip.io/linuxdo_alice/96/1_2.png"),
        );
    }

    #[test]
    fn resolve_linuxdo_avatar_url_keeps_single_oauth_subpath_for_unsafe_absolute_templates() {
        let mut cfg = LinuxDoOAuthOptions::disabled();
        cfg.authorize_url = "https://forum.example.com/discourse/oauth2/authorize".to_string();
        cfg.token_url = "https://forum.example.com/discourse/oauth2/token".to_string();
        cfg.userinfo_url = "https://forum.example.com/discourse/api/user".to_string();

        assert_eq!(
            resolve_linuxdo_avatar_url(
                &cfg,
                Some("http://oauth.internal/discourse/avatar/linuxdo_alice/{size}/1_2.png"),
            )
            .as_deref(),
            Some("https://forum.example.com/discourse/avatar/linuxdo_alice/96/1_2.png"),
        );
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardAuthDebugView {
    enabled: bool,
    user_header: Option<String>,
    admin_value: Option<String>,
    nickname_header: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminRegistrationSettingsView {
    allow_registration: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAdminRegistrationSettingsRequest {
    allow_registration: bool,
}

async fn get_forward_auth_debug(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ForwardAuthDebugView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let cfg = &state.forward_auth;
    Ok(Json(ForwardAuthDebugView {
        enabled: state.forward_auth_enabled && cfg.is_enabled(),
        user_header: cfg.user_header().map(|h| h.to_string()),
        admin_value: None,
        nickname_header: cfg.nickname_header().map(|h| h.to_string()),
    }))
}

async fn debug_headers(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<serde_json::Value>), StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut map = serde_json::Map::new();
    for (k, v) in headers.iter() {
        map.insert(
            k.as_str().to_string(),
            serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
        );
    }
    Ok((StatusCode::OK, Json(serde_json::Value::Object(map))))
}

async fn get_profile(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ProfileView>, StatusCode> {
    let config = &state.forward_auth;
    let allow_registration = state.proxy.allow_registration().await.map_err(|err| {
        eprintln!("get allow registration setting error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let forward_auth_enabled = state.forward_auth_enabled && config.is_enabled();
    let builtin_auth_enabled = state.builtin_admin.is_enabled();

    if state.dev_open_admin {
        return Ok(Json(ProfileView {
            display_name: Some("dev-mode".to_string()),
            is_admin: true,
            forward_auth_enabled,
            builtin_auth_enabled,
            allow_registration,
            user_logged_in: None,
            user_provider: None,
            user_display_name: None,
            user_avatar_url: None,
        }));
    }

    let forward_user_value = if forward_auth_enabled {
        config.user_value(&headers).map(str::to_string)
    } else {
        None
    };

    let forward_nickname = if forward_auth_enabled {
        config
            .nickname_value(&headers)
            .or_else(|| forward_user_value.clone())
    } else {
        None
    };

    let is_admin = is_admin_request(state.as_ref(), &headers);

    let display_name = forward_nickname
        .or_else(|| config.admin_override_name().map(str::to_string))
        .or_else(|| is_admin.then(|| "admin".to_string()));

    let user_session = resolve_user_session(state.as_ref(), &headers).await;
    let user_logged_in = if state.linuxdo_oauth.is_enabled_and_configured() {
        Some(user_session.is_some())
    } else {
        None
    };
    let user_provider = user_session
        .as_ref()
        .map(|session| session.user.provider.clone());
    let user_display_name = user_session.as_ref().and_then(|session| {
        session
            .user
            .display_name
            .clone()
            .or_else(|| session.user.username.clone())
    });
    let user_avatar_url = user_session.as_ref().and_then(|session| {
        if session.user.provider == "linuxdo" {
            resolve_linuxdo_avatar_url(&state.linuxdo_oauth, session.user.avatar_template.as_deref())
        } else {
            None
        }
    });

    Ok(Json(ProfileView {
        display_name,
        is_admin,
        forward_auth_enabled,
        builtin_auth_enabled,
        allow_registration,
        user_logged_in,
        user_provider,
        user_display_name,
        user_avatar_url,
    }))
}

async fn get_admin_registration_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminRegistrationSettingsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let allow_registration = state.proxy.allow_registration().await.map_err(|err| {
        eprintln!("get admin registration settings error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(AdminRegistrationSettingsView { allow_registration }))
}

async fn patch_admin_registration_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    payload: Result<Json<UpdateAdminRegistrationSettingsRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<AdminRegistrationSettingsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let Json(payload) = payload.map_err(|err| {
        eprintln!("patch admin registration settings payload error: {err}");
        StatusCode::BAD_REQUEST
    })?;
    let allow_registration = state
        .proxy
        .set_allow_registration(payload.allow_registration)
        .await
        .map_err(|err| {
            eprintln!("patch admin registration settings error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(AdminRegistrationSettingsView { allow_registration }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminLoginRequest {
    password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminLoginResponse {
    ok: bool,
}

fn session_set_cookie(token: &str, secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = BUILTIN_ADMIN_COOKIE_NAME,
        max_age = BUILTIN_ADMIN_SESSION_MAX_AGE_SECS,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn session_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = BUILTIN_ADMIN_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn user_session_set_cookie(
    token: &str,
    max_age_secs: i64,
    secure: bool,
) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = USER_SESSION_COOKIE_NAME,
        max_age = max_age_secs.max(60),
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn user_session_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = USER_SESSION_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn oauth_login_binding_set_cookie(
    token: &str,
    max_age_secs: i64,
    secure: bool,
) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = OAUTH_LOGIN_BINDING_COOKIE_NAME,
        max_age = max_age_secs.max(60),
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn oauth_login_binding_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = OAUTH_LOGIN_BINDING_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn new_cookie_nonce() -> String {
    use base64::Engine as _;
    use rand::RngCore as _;

    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn hash_oauth_binding(nonce: &str) -> String {
    use base64::Engine as _;
    let digest = Sha256::digest(nonce.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn map_oauth_upstream_transport_error(err: &reqwest::Error) -> StatusCode {
    if err.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn map_oauth_upstream_status(status: reqwest::StatusCode) -> StatusCode {
    if status.is_server_error() {
        return StatusCode::BAD_GATEWAY;
    }
    match status {
        reqwest::StatusCode::BAD_REQUEST => StatusCode::BAD_REQUEST,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            StatusCode::UNAUTHORIZED
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    }
}

async fn post_admin_login(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminLoginRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !state.builtin_admin.is_enabled() {
        return Err(StatusCode::NOT_FOUND);
    }
    let password = payload.password.trim();
    let Some(token) = state.builtin_admin.login(password) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    state.builtin_admin.remember_session(token.clone());
    let cookie = session_set_cookie(&token, wants_secure_cookie(&headers))?;
    Ok((
        StatusCode::OK,
        [(SET_COOKIE, cookie)],
        Json(AdminLoginResponse { ok: true }),
    )
        .into_response())
}

async fn post_admin_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    if !state.builtin_admin.is_enabled() {
        return Err(StatusCode::NOT_FOUND);
    }
    state.builtin_admin.forget_session(&headers);
    let cookie = session_clear_cookie(wants_secure_cookie(&headers))?;
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cookie)]).into_response())
}

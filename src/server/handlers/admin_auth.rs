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
    db: i64,
    geo: i64,
    linuxdo: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TriggerJobRequest {
    job_type: String,
    key_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TriggerJobResponse {
    job_id: i64,
    job_type: String,
    trigger_source: String,
    status: String,
    coalesced: bool,
    promoted: bool,
}

fn manual_trigger_requires_key(job_type: &str) -> bool {
    matches!(job_type, "quota_sync")
}

fn manual_trigger_supported(job_type: &str) -> bool {
    matches!(
        job_type,
        "quota_sync"
            | "token_usage_rollup"
            | "auth_token_logs_gc"
            | "ha_outbox_gc"
            | "request_logs_gc"
            | "mcp_sessions_gc"
            | "mcp_session_init_backoffs_gc"
            | "linuxdo_user_status_sync"
            | "linuxdo_user_tag_binding_refresh"
            | "forward_proxy_geo_refresh"
            | "db_compaction"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum ManualTriggerKeyIdError {
    Required,
    NotSupported,
}

fn manual_trigger_key_id_for_claim(
    job_type: &str,
    key_id: Option<String>,
) -> Result<Option<String>, ManualTriggerKeyIdError> {
    if manual_trigger_requires_key(job_type) {
        if key_id.is_some() {
            return Ok(key_id);
        }
        return Err(ManualTriggerKeyIdError::Required);
    }

    if key_id.is_some() {
        return Err(ManualTriggerKeyIdError::NotSupported);
    }

    Ok(None)
}

fn manual_trigger_key_id_error_response(
    job_type: &str,
    err: ManualTriggerKeyIdError,
) -> Response<Body> {
    match err {
        ManualTriggerKeyIdError::Required => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "key_id_required",
                "detail": "keyId is required for quota_sync"
            })),
        )
            .into_response(),
        ManualTriggerKeyIdError::NotSupported => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "key_id_not_supported",
                "detail": format!("keyId is not supported for {job_type}")
            })),
        )
            .into_response(),
    }
}

fn scheduled_job_is_terminal(status: &str) -> bool {
    !matches!(status, "queued" | "running")
}

async fn wait_for_scheduled_job_terminal(
    state: &AppState,
    job_id: i64,
    timeout: Duration,
) -> Result<JobLog, ProxyError> {
    let deadline = state.proxy.backend_time().deadline_after(timeout);
    loop {
        let Some(job) = state.proxy.scheduled_job_by_id(job_id).await? else {
            return Err(ProxyError::Other(format!(
                "scheduled job {job_id} disappeared before completion"
            )));
        };
        if scheduled_job_is_terminal(&job.status) {
            return Ok(job);
        }
        if state.proxy.backend_time().instant_now() >= deadline {
            return Err(ProxyError::Other(format!(
                "scheduled job {job_id} did not finish within {}s",
                timeout.as_secs()
            )));
        }
        state
            .proxy
            .backend_time()
            .sleep(Duration::from_millis(250))
            .await;
    }
}

async fn list_jobs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<JobsQuery>,
) -> Result<Json<PaginatedJobsView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
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
                    trigger_source: j.trigger_source,
                    key_id: j.key_id,
                    key_group: j.key_group,
                    status: j.status,
                    attempt: j.attempt,
                    message: j.message,
                    queued_at: j.queued_at,
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
                    db: group_counts.db,
                    geo: group_counts.geo,
                    linuxdo: group_counts.linuxdo,
                },
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn post_trigger_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<TriggerJobRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    if require_full_master_write(state.as_ref()).await.is_err() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let job_type = payload.job_type.trim().to_string();
    if !manual_trigger_supported(&job_type) {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "unsupported_job_type",
                "detail": format!("manual trigger is not supported for {job_type}")
            })),
        )
            .into_response());
    }
    let key_id = payload
        .key_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let key_id = match manual_trigger_key_id_for_claim(&job_type, key_id) {
        Ok(key_id) => key_id,
        Err(err) => return Ok(manual_trigger_key_id_error_response(&job_type, err)),
    };

    match enqueue_scheduled_job_result(
        state.as_ref(),
        &job_type,
        key_id.as_deref(),
        TRIGGER_SOURCE_MANUAL,
    )
    .await
    {
        Ok(job) => Ok((
            StatusCode::ACCEPTED,
            Json(TriggerJobResponse {
                job_id: job.job_id,
                job_type,
                trigger_source: job.trigger_source,
                status: job.status,
                coalesced: !job.created,
                promoted: job.promoted,
            }),
        )
            .into_response()),
        Err(err) => {
            eprintln!("manual job trigger error: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(test)]
mod manual_trigger_tests {
    use super::{ManualTriggerKeyIdError, manual_trigger_key_id_for_claim};

    #[test]
    fn manual_global_jobs_reject_key_id() {
        let err =
            manual_trigger_key_id_for_claim("db_compaction", Some("key-1".to_string())).unwrap_err();
        assert_eq!(err, ManualTriggerKeyIdError::NotSupported);
    }

    #[test]
    fn manual_quota_sync_requires_key_id() {
        let err = manual_trigger_key_id_for_claim("quota_sync", None).unwrap_err();
        assert_eq!(err, ManualTriggerKeyIdError::Required);

        let key_id = manual_trigger_key_id_for_claim("quota_sync", Some("key-1".to_string()))
            .expect("quota key accepted");
        assert_eq!(key_id.as_deref(), Some("key-1"));
    }
}

// ---- Key detail & manual quota sync ----

async fn get_api_key_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<ApiKeyView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
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
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    match run_manual_key_quota_sync(state.clone(), &id).await {
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
    state: Arc<AppState>,
    key_id: &str,
) -> Result<(), ManualQuotaSyncError> {
    let job_id = enqueue_scheduled_job(
        state.as_ref(),
        "quota_sync",
        Some(key_id),
        TRIGGER_SOURCE_MANUAL,
    )
    .await
    .map_err(|err| {
        ManualQuotaSyncError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "sync_failed",
            err.to_string(),
        )
    })?;

    let job = wait_for_scheduled_job_terminal(
        state.as_ref(),
        job_id,
        Duration::from_secs(QUOTA_SYNC_JOB_TIMEOUT_SECS + 15),
    )
    .await
    .map_err(|err| {
        ManualQuotaSyncError::new(
            StatusCode::BAD_GATEWAY,
            "sync_failed",
            err.to_string(),
        )
    })?;

    if job.status == "success" {
        return Ok(());
    }

    let detail = job
        .message
        .clone()
        .unwrap_or_else(|| format!("quota_sync failed with status {}", job.status));
    if let Some(reason) = detail.strip_prefix("quota_data_missing: ") {
        return Err(ManualQuotaSyncError::new(
            StatusCode::BAD_REQUEST,
            "quota_data_missing",
            reason.to_string(),
        ));
    }
    if let Some(rest) = detail.strip_prefix("usage_http ") {
        let (status_text, body) = rest
            .split_once(": ")
            .map(|(status, body)| (status.trim(), body.to_string()))
            .unwrap_or((rest.trim(), String::new()));
        let status = status_text
            .split_whitespace()
            .next()
            .unwrap_or(status_text)
            .parse::<reqwest::StatusCode>()
            .unwrap_or(reqwest::StatusCode::BAD_GATEWAY);
        let http_status = if status == reqwest::StatusCode::UNAUTHORIZED {
            StatusCode::UNAUTHORIZED
        } else if status == reqwest::StatusCode::FORBIDDEN {
            StatusCode::FORBIDDEN
        } else if status.is_client_error() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::BAD_GATEWAY
        };
        let detail = if body.is_empty() {
            format!("Tavily usage request failed with {status}")
        } else {
            format!("Tavily usage request failed with {status}: {body}")
        };
        return Err(ManualQuotaSyncError::new(http_status, "usage_http", detail));
    }

    Err(ManualQuotaSyncError::new(
        if job.status == "abandoned" {
            StatusCode::SERVICE_UNAVAILABLE
        } else {
            StatusCode::BAD_GATEWAY
        },
        "sync_failed",
        detail,
    ))
}

async fn delete_api_key_quarantine(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
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
    passkey_auth_enabled: bool,
    admin_login_totp_required: bool,
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
    if !is_admin_request(state.as_ref(), &headers).await {
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
    if !is_admin_request(state.as_ref(), &headers).await {
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
    let admin_login_totp_required = state.builtin_admin.login_totp_required();
    let passkey_auth_enabled = state.admin_passkey.is_configured()
        && state.proxy.admin_passkey_enabled().await.map_err(|err| {
            eprintln!("get admin passkey enabled error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if state.dev_open_admin {
        return Ok(Json(ProfileView {
            display_name: Some("dev-mode".to_string()),
            is_admin: true,
            forward_auth_enabled,
            builtin_auth_enabled,
            passkey_auth_enabled,
            admin_login_totp_required,
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

    let is_admin = is_admin_request(state.as_ref(), &headers).await;

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
        passkey_auth_enabled,
        admin_login_totp_required,
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
    if !is_admin_request(state.as_ref(), &headers).await {
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
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    if require_full_master_write(state.as_ref()).await.is_err() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
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
    totp_code: Option<String>,
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

fn passkey_session_set_cookie(
    token: &str,
    max_age_secs: i64,
    secure: bool,
) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}",
        name = ADMIN_PASSKEY_COOKIE_NAME,
        max_age = max_age_secs.max(60),
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn passkey_session_clear_cookie(secure: bool) -> Result<HeaderValue, StatusCode> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
        name = ADMIN_PASSKEY_COOKIE_NAME,
        secure = secure
    );
    HeaderValue::from_str(&cookie).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyAuthenticationStartResponse {
    challenge_id: String,
    public_key: RequestChallengeResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyAuthenticationFinishRequest {
    challenge_id: String,
    credential: PublicKeyCredential,
    totp_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyAuthenticationFinishResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyRegistrationStartResponse {
    challenge_id: String,
    public_key: CreationChallengeResponse,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyRegistrationFinishRequest {
    challenge_id: String,
    credential: RegisterPublicKeyCredential,
    label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyRegistrationFinishResponse {
    ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyCredentialView {
    credential_id: String,
    label: Option<String>,
    created_at: i64,
    updated_at: i64,
    last_used_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeysView {
    configured: bool,
    enabled: bool,
    credential_count: usize,
    credentials: Vec<AdminPasskeyCredentialView>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasskeyLabelUpdateRequest {
    label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasswordStatusView {
    enabled: bool,
    updated_at: Option<i64>,
    login_totp_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasswordSetRequest {
    password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminPasswordSettingsPatchRequest {
    login_totp_required: bool,
}

fn admin_passkey_unavailable() -> StatusCode {
    StatusCode::NOT_FOUND
}

async fn require_admin_credential_write(state: &AppState) -> Result<(), StatusCode> {
    require_full_master_write(state)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

fn forward_auth_can_admin_login(state: &AppState) -> bool {
    state.forward_auth_enabled
        && state.forward_auth.user_header().is_some()
        && state.forward_auth.admin_value().is_some()
}

fn external_admin_login_available(state: &AppState) -> bool {
    state.dev_open_admin || forward_auth_can_admin_login(state)
}

fn map_admin_login_method_error(context: &str, err: tavily_hikari::ProxyError) -> StatusCode {
    match err {
        tavily_hikari::ProxyError::LastAdminLoginMethod => StatusCode::CONFLICT,
        err => {
            eprintln!("{context}: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn admin_password_status_view(state: &AppState) -> AdminPasswordStatusView {
    let status = state.builtin_admin.status();
    AdminPasswordStatusView {
        enabled: status.enabled,
        updated_at: status.updated_at,
        login_totp_required: status.login_totp_required,
    }
}

async fn get_admin_passkeys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminPasskeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    let configured = state.admin_passkey.is_configured();
    let credentials = if configured {
        state
            .proxy
            .list_active_admin_passkey_credentials()
            .await
            .map_err(|err| {
                eprintln!("list admin passkeys error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else {
        Vec::new()
    };
    let credential_views = credentials
        .into_iter()
        .map(|record| AdminPasskeyCredentialView {
            credential_id: record.credential_id,
            label: record.label,
            created_at: record.created_at,
            updated_at: record.updated_at,
            last_used_at: record.last_used_at,
        })
        .collect::<Vec<_>>();
    Ok(Json(AdminPasskeysView {
        configured,
        enabled: configured && !credential_views.is_empty(),
        credential_count: credential_views.len(),
        credentials: credential_views,
    }))
}

async fn patch_admin_passkey(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(credential_id): Path<String>,
    Json(payload): Json<AdminPasskeyLabelUpdateRequest>,
) -> Result<Json<AdminPasskeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    if !state.admin_passkey.is_configured() {
        return Err(admin_passkey_unavailable());
    }
    let credential_id = credential_id.trim();
    if credential_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let updated = state
        .proxy
        .update_admin_passkey_credential_label(credential_id, payload.label.as_deref())
        .await
        .map_err(|err| {
            eprintln!("update admin passkey label error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !updated {
        return Err(StatusCode::NOT_FOUND);
    }
    get_admin_passkeys(State(state), headers).await
}

async fn delete_admin_passkey(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(credential_id): Path<String>,
) -> Result<Json<AdminPasskeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    if !state.admin_passkey.is_configured() {
        return Err(admin_passkey_unavailable());
    }
    let credential_id = credential_id.trim();
    if credential_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let revoked = state
        .proxy
        .revoke_admin_passkey_credential_preserving_login(
            credential_id,
            external_admin_login_available(state.as_ref()),
            state.builtin_admin.is_enabled(),
        )
        .await
        .map_err(|err| map_admin_login_method_error("revoke admin passkey credential error", err))?;
    if !revoked {
        return Err(StatusCode::NOT_FOUND);
    }
    state
        .proxy
        .revoke_admin_passkey_sessions_for_credential(credential_id)
        .await
        .map_err(|err| {
            eprintln!("revoke admin passkey sessions for credential error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    get_admin_passkeys(State(state), headers).await
}

fn deserialize_passkey(record: &tavily_hikari::AdminPasskeyCredentialRecord) -> Result<Passkey, StatusCode> {
    serde_json::from_str(&record.passkey_json).map_err(|err| {
        eprintln!("stored admin passkey credential decode error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

fn credential_id_for_passkey(passkey: &Passkey) -> String {
    let bytes: &[u8] = passkey.cred_id().as_ref();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

async fn post_admin_passkey_authentication_start(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AdminPasskeyAuthenticationStartResponse>, StatusCode> {
    if !state.admin_passkey.is_configured() {
        return Err(admin_passkey_unavailable());
    }
    let credentials = state
        .proxy
        .list_active_admin_passkey_credentials()
        .await
        .map_err(|err| {
            eprintln!("list admin passkey credentials error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if credentials.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    let passkeys = credentials
        .iter()
        .map(deserialize_passkey)
        .collect::<Result<Vec<_>, _>>()?;
    let webauthn = state.admin_passkey.webauthn().map_err(|err| {
        eprintln!("build admin passkey webauthn error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let (public_key, passkey_state) = webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(|err| {
            eprintln!("start admin passkey auth error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let state_json = serde_json::to_string(&passkey_state).map_err(|err| {
        eprintln!("serialize admin passkey auth state error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let challenge = state
        .proxy
        .insert_admin_passkey_challenge(
            tavily_hikari::AdminPasskeyChallengeKind::Authentication,
            None,
            &state_json,
            state.admin_passkey.challenge_ttl_secs,
        )
        .await
        .map_err(|err| {
            eprintln!("insert admin passkey auth challenge error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(AdminPasskeyAuthenticationStartResponse {
        challenge_id: challenge.id,
        public_key,
    }))
}

async fn post_admin_passkey_authentication_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminPasskeyAuthenticationFinishRequest>,
) -> Result<Response<Body>, StatusCode> {
    if !state.admin_passkey.is_configured() {
        return Err(admin_passkey_unavailable());
    }
    let challenge = state
        .proxy
        .consume_admin_passkey_challenge(
            payload.challenge_id.trim(),
            tavily_hikari::AdminPasskeyChallengeKind::Authentication,
        )
        .await
        .map_err(|err| {
            eprintln!("consume admin passkey auth challenge error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let passkey_state: PasskeyAuthentication =
        serde_json::from_str(&challenge.state_json).map_err(|err| {
            eprintln!("decode admin passkey auth state error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let webauthn = state.admin_passkey.webauthn().map_err(|err| {
        eprintln!("build admin passkey webauthn error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let auth_result = webauthn
        .finish_passkey_authentication(&payload.credential, &passkey_state)
        .map_err(|err| {
            eprintln!("finish admin passkey auth error: {err}");
            StatusCode::UNAUTHORIZED
        })?;
    let credential_id_bytes: &[u8] = auth_result.cred_id().as_ref();
    let credential_id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(credential_id_bytes);
    let mut passkey = state
        .proxy
        .list_active_admin_passkey_credentials()
        .await
        .map_err(|err| {
            eprintln!("list admin passkey credentials after auth error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .iter()
        .find(|record| record.credential_id == credential_id)
        .map(deserialize_passkey)
        .transpose()?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    passkey.update_credential(&auth_result);
    let passkey_json = serde_json::to_string(&passkey).map_err(|err| {
        eprintln!("serialize admin passkey after auth error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let updated = state
        .proxy
        .update_admin_passkey_credential_after_auth(&credential_id, &passkey_json)
        .await
        .map_err(|err| {
            eprintln!("update admin passkey after auth error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    if !updated {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if state.builtin_admin.login_totp_required() {
        let code = payload.totp_code.as_deref().unwrap_or_default();
        verify_admin_totp_for_sensitive_action(state.as_ref(), code)
            .await
            .map_err(|(status, _)| status)?;
    }
    let session = state
        .proxy
        .create_admin_passkey_session(
            Some(&credential_id),
            state.admin_passkey.session_max_age_secs,
        )
        .await
        .map_err(|err| {
            eprintln!("create admin passkey session error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let cookie = passkey_session_set_cookie(
        &session.token,
        state.admin_passkey.session_max_age_secs,
        wants_secure_cookie(&headers),
    )?;
    Ok((
        StatusCode::OK,
        [(SET_COOKIE, cookie)],
        Json(AdminPasskeyAuthenticationFinishResponse { ok: true }),
    )
        .into_response())
}

async fn post_admin_passkey_registration_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminPasskeyRegistrationStartResponse>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    start_admin_passkey_registration(state, None).await
}

async fn post_admin_passkey_reset_registration_start(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
) -> Result<Json<AdminPasskeyRegistrationStartResponse>, StatusCode> {
    require_admin_credential_write(state.as_ref()).await?;
    let reset = active_admin_passkey_reset_token(state.as_ref(), &token).await?;
    start_admin_passkey_registration(state, Some(reset.token_hash)).await
}

async fn active_admin_passkey_reset_token(
    state: &AppState,
    token: &str,
) -> Result<tavily_hikari::AdminPasskeyResetTokenRecord, StatusCode> {
    let token = token.trim();
    if token.is_empty() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state
        .proxy
        .get_active_admin_passkey_reset_token(token)
        .await
        .map_err(|err| {
            eprintln!("get admin passkey reset token error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::UNAUTHORIZED)
}

async fn start_admin_passkey_registration(
    state: Arc<AppState>,
    reset_token_hash: Option<String>,
) -> Result<Json<AdminPasskeyRegistrationStartResponse>, StatusCode> {
    if !state.admin_passkey.is_configured() {
        return Err(admin_passkey_unavailable());
    }
    let existing = state
        .proxy
        .list_active_admin_passkey_credentials()
        .await
        .map_err(|err| {
            eprintln!("list admin passkeys for registration error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let exclude_credentials = existing
        .iter()
        .map(deserialize_passkey)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|passkey| passkey.cred_id().clone())
        .collect::<Vec<_>>();
    let webauthn = state.admin_passkey.webauthn().map_err(|err| {
        eprintln!("build admin passkey webauthn error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let (public_key, passkey_state) = webauthn
        .start_passkey_registration(
            Uuid::nil(),
            "admin",
            "Tavily Hikari Admin",
            Some(exclude_credentials),
        )
        .map_err(|err| {
            eprintln!("start admin passkey registration error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let state_json = serde_json::to_string(&passkey_state).map_err(|err| {
        eprintln!("serialize admin passkey registration state error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let challenge = state
        .proxy
        .insert_admin_passkey_challenge(
            tavily_hikari::AdminPasskeyChallengeKind::Registration,
            reset_token_hash.as_deref(),
            &state_json,
            state.admin_passkey.challenge_ttl_secs,
        )
        .await
        .map_err(|err| {
            eprintln!("insert admin passkey registration challenge error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(AdminPasskeyRegistrationStartResponse {
        challenge_id: challenge.id,
        public_key,
    }))
}

async fn post_admin_passkey_registration_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminPasskeyRegistrationFinishRequest>,
) -> Result<Json<AdminPasskeyRegistrationFinishResponse>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    finish_admin_passkey_registration(state, payload, None).await
}

async fn post_admin_passkey_reset_registration_finish(
    State(state): State<Arc<AppState>>,
    Path(token): Path<String>,
    Json(payload): Json<AdminPasskeyRegistrationFinishRequest>,
) -> Result<Json<AdminPasskeyRegistrationFinishResponse>, StatusCode> {
    require_admin_credential_write(state.as_ref()).await?;
    let reset = active_admin_passkey_reset_token(state.as_ref(), &token).await?;
    finish_admin_passkey_registration(state, payload, Some(reset.token_hash)).await
}

async fn finish_admin_passkey_registration(
    state: Arc<AppState>,
    payload: AdminPasskeyRegistrationFinishRequest,
    reset_token_hash: Option<String>,
) -> Result<Json<AdminPasskeyRegistrationFinishResponse>, StatusCode> {
    if !state.admin_passkey.is_configured() {
        return Err(admin_passkey_unavailable());
    }
    let challenge = state
        .proxy
        .consume_admin_passkey_challenge(
            payload.challenge_id.trim(),
            tavily_hikari::AdminPasskeyChallengeKind::Registration,
        )
        .await
        .map_err(|err| {
            eprintln!("consume admin passkey registration challenge error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::UNAUTHORIZED)?;
    match (challenge.reset_token.as_deref(), reset_token_hash.as_deref()) {
        (None, None) => {}
        (Some(challenge_hash), Some(reset_hash)) if challenge_hash == reset_hash => {}
        _ => return Err(StatusCode::UNAUTHORIZED),
    }
    let passkey_state: PasskeyRegistration =
        serde_json::from_str(&challenge.state_json).map_err(|err| {
            eprintln!("decode admin passkey registration state error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let webauthn = state.admin_passkey.webauthn().map_err(|err| {
        eprintln!("build admin passkey webauthn error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let passkey = webauthn
        .finish_passkey_registration(&payload.credential, &passkey_state)
        .map_err(|err| {
            eprintln!("finish admin passkey registration error: {err}");
            StatusCode::UNAUTHORIZED
        })?;
    let credential_id = credential_id_for_passkey(&passkey);
    let passkey_json = serde_json::to_string(&passkey).map_err(|err| {
        eprintln!("serialize admin passkey credential error: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let previous_credentials = if reset_token_hash.is_some() {
        state
            .proxy
            .list_active_admin_passkey_credentials()
            .await
            .map_err(|err| {
                eprintln!("list old admin passkeys for reset error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else {
        Vec::new()
    };
    if let Some(token_hash) = reset_token_hash.as_deref() {
        let old_credential_ids = previous_credentials
            .iter()
            .map(|record| record.credential_id.clone())
            .collect::<Vec<_>>();
        let completed = state
            .proxy
            .complete_admin_passkey_reset_registration(
                token_hash,
                &credential_id,
                &passkey_json,
                payload.label.as_deref(),
                &old_credential_ids,
            )
            .await
            .map_err(|err| {
                eprintln!("complete admin passkey reset registration error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        if !completed {
            return Err(StatusCode::UNAUTHORIZED);
        }
        state.builtin_admin.clear_sessions();
        return Ok(Json(AdminPasskeyRegistrationFinishResponse { ok: true }));
    }
    state
        .proxy
        .upsert_admin_passkey_credential(
            &credential_id,
            &passkey_json,
            payload.label.as_deref(),
        )
        .await
        .map_err(|err| {
            eprintln!("store admin passkey credential error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    for record in previous_credentials {
        state
            .proxy
            .revoke_admin_passkey_credential_preserving_login(&record.credential_id, true, true)
            .await
            .map_err(|err| {
                eprintln!("revoke old admin passkey after reset error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        state
            .proxy
            .revoke_admin_passkey_sessions_for_credential(&record.credential_id)
            .await
            .map_err(|err| {
                eprintln!("revoke old admin passkey sessions after reset error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }
    Ok(Json(AdminPasskeyRegistrationFinishResponse { ok: true }))
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

async fn get_admin_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminPasswordStatusView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Json(admin_password_status_view(state.as_ref())))
}

async fn put_admin_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminPasswordSetRequest>,
) -> Result<Json<AdminPasswordStatusView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    let password = payload.password.trim();
    if password.len() < 8 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|err| {
            eprintln!("hash admin password error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .to_string();
    let settings = state
        .proxy
        .set_admin_password_hash(&password_hash)
        .await
        .map_err(|err| {
            eprintln!("set admin password hash error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    state
        .builtin_admin
        .set_password_hash(password_hash, Some(settings.updated_at));
    Ok(Json(admin_password_status_view(state.as_ref())))
}

async fn patch_admin_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AdminPasswordSettingsPatchRequest>,
) -> Result<Json<AdminPasswordStatusView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    if payload.login_totp_required && !admin_totp_is_ready_for_login(state.as_ref()).await? {
        return Err(StatusCode::CONFLICT);
    }
    if payload.login_totp_required {
        state
            .proxy
            .revoke_all_admin_passkey_sessions()
            .await
            .map_err(|err| {
                eprintln!("revoke admin passkey sessions before requiring TOTP error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }
    let settings = state
        .proxy
        .set_admin_login_totp_required(payload.login_totp_required)
        .await
        .map_err(|err| {
            eprintln!("set admin login totp requirement error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    state
        .builtin_admin
        .set_login_totp_required(settings.login_totp_required, Some(settings.updated_at));
    if settings.login_totp_required {
        state.builtin_admin.clear_sessions();
    }
    Ok(Json(admin_password_status_view(state.as_ref())))
}

async fn delete_admin_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminPasswordStatusView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers).await {
        return Err(StatusCode::FORBIDDEN);
    }
    require_admin_credential_write(state.as_ref()).await?;
    let settings = state
        .proxy
        .disable_admin_password_preserving_login(
            external_admin_login_available(state.as_ref()),
            state.admin_passkey.is_configured(),
        )
        .await
        .map_err(|err| map_admin_login_method_error("disable admin password error", err))?;
    state
        .builtin_admin
        .disable_password(Some(settings.updated_at));
    Ok(Json(admin_password_status_view(state.as_ref())))
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
    if state.builtin_admin.login_totp_required() {
        let code = payload.totp_code.as_deref().unwrap_or_default();
        verify_admin_totp_for_sensitive_action(state.as_ref(), code)
            .await
            .map_err(|(status, _)| status)?;
    }
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
    if !state.builtin_admin.is_enabled() && !state.admin_passkey.is_configured() {
        return Err(StatusCode::NOT_FOUND);
    }
    state.builtin_admin.forget_session(&headers);
    if let Some(token) = cookie_value(&headers, ADMIN_PASSKEY_COOKIE_NAME) {
        state
            .proxy
            .revoke_admin_passkey_session(&token)
            .await
            .map_err(|err| {
                eprintln!("revoke admin passkey session error: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }
    let secure = wants_secure_cookie(&headers);
    let builtin_cookie = session_clear_cookie(secure)?;
    let passkey_cookie = passkey_session_clear_cookie(secure)?;
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(SET_COOKIE, builtin_cookie), (SET_COOKIE, passkey_cookie)]),
    )
        .into_response())
}

#[cfg(test)]
mod admin_auth_last_login_method_tests {
    use super::*;

    fn configured_passkey_options() -> AdminPasskeyOptions {
        AdminPasskeyOptions {
            enabled: true,
            rp_id: Some("example.com".to_string()),
            rp_origin: Some("https://example.com".to_string()),
            challenge_ttl_secs: 300,
            session_max_age_secs: 60 * 60 * 24 * 14,
        }
    }

    async fn test_state(
        prefix: &str,
        builtin_admin: BuiltinAdminAuth,
        admin_passkey: AdminPasskeyOptions,
        forward_auth_enabled: bool,
    ) -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let db_path = temp_dir.path().join(format!("{prefix}.db"));
        let db_path = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), tavily_hikari::DEFAULT_UPSTREAM, &db_path)
            .await
            .expect("proxy created");
        let state = Arc::new(AppState {
            proxy,
            static_dir: None,
            forward_auth: ForwardAuthConfig::new(
                Some(HeaderName::from_static("x-forward-user")),
                Some("admin".to_string()),
                None,
                None,
            ),
            forward_auth_enabled,
            builtin_admin,
            admin_passkey,
            linuxdo_oauth: LinuxDoOAuthOptions {
                enabled: false,
                client_id: None,
                client_secret: None,
                authorize_url: "https://connect.linux.do/oauth2/authorize".to_string(),
                token_url: "https://connect.linux.do/oauth2/token".to_string(),
                userinfo_url: "https://connect.linux.do/api/user".to_string(),
                scope: "user".to_string(),
                redirect_url: None,
                refresh_token_crypt_key: Some(*b"0123456789abcdef0123456789abcdef"),
                user_sync_enabled: false,
                user_sync_at: (6, 20),
                session_max_age_secs: 3600,
                login_state_ttl_secs: 600,
            },
            linuxdo_credit: LinuxDoCreditOptions::disabled(),
            ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
            dev_open_admin: false,
            usage_base: "http://127.0.0.1:58088".to_string(),
            api_key_ip_geo_origin: "https://api.country.is".to_string(),
            dashboard_overview_cache: new_dashboard_overview_cache(),
        });
        (state, temp_dir)
    }

    fn headers_from_set_cookie(response: &Response<Body>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            response
                .headers()
                .get(SET_COOKIE)
                .expect("set-cookie header")
                .clone(),
        );
        headers
    }

    #[tokio::test]
    async fn password_delete_rejects_removing_last_admin_login_method() {
        let (state, _temp_dir) = test_state(
            "last-password-delete",
            BuiltinAdminAuth::new(true, Some("pw-123456".to_string()), None),
            AdminPasskeyOptions::disabled(),
            false,
        )
        .await;
        let login = post_admin_login(
            State(state.clone()),
            HeaderMap::new(),
            Json(AdminLoginRequest {
                password: "pw-123456".to_string(),
                totp_code: None,
            }),
        )
        .await
        .expect("password login succeeds");

        let result = delete_admin_password(State(state.clone()), headers_from_set_cookie(&login)).await;

        assert!(matches!(result, Err(StatusCode::CONFLICT)));
        assert!(state.builtin_admin.is_enabled());
    }

    #[tokio::test]
    async fn passkey_delete_rejects_removing_last_admin_login_method() {
        let (state, _temp_dir) = test_state(
            "last-passkey-delete",
            BuiltinAdminAuth::new(false, None, None),
            configured_passkey_options(),
            false,
        )
        .await;
        state
            .proxy
            .upsert_admin_passkey_credential("credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert passkey credential");
        let session = state
            .proxy
            .create_admin_passkey_session(Some("credential-1"), 120)
            .await
            .expect("create passkey session");
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_str(&format!("{ADMIN_PASSKEY_COOKIE_NAME}={}", session.token))
                .expect("cookie header"),
        );

        let result = delete_admin_passkey(
            State(state.clone()),
            headers,
            Path("credential-1".to_string()),
        )
        .await;

        assert!(matches!(result, Err(StatusCode::CONFLICT)));
        let credentials = state
            .proxy
            .list_active_admin_passkey_credentials()
            .await
            .expect("list credentials");
        assert_eq!(credentials.len(), 1);
    }

    #[tokio::test]
    async fn requiring_login_totp_revokes_existing_admin_sessions() {
        let (state, _temp_dir) = test_state(
            "login-totp-revokes-sessions",
            BuiltinAdminAuth::new(true, Some("pw-123456".to_string()), None),
            configured_passkey_options(),
            false,
        )
        .await;
        state
            .proxy
            .set_admin_totp_secret_record("ciphertext", "nonce", 1_700_000_000)
            .await
            .expect("seed TOTP secret");
        state
            .proxy
            .upsert_admin_passkey_credential("credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert passkey credential");
        let passkey_session = state
            .proxy
            .create_admin_passkey_session(Some("credential-1"), 120)
            .await
            .expect("create passkey session");
        let login = post_admin_login(
            State(state.clone()),
            HeaderMap::new(),
            Json(AdminLoginRequest {
                password: "pw-123456".to_string(),
                totp_code: None,
            }),
        )
        .await
        .expect("password login succeeds");
        let builtin_headers = headers_from_set_cookie(&login);
        assert!(is_admin_request(state.as_ref(), &builtin_headers).await);

        let _ = patch_admin_password(
            State(state.clone()),
            builtin_headers.clone(),
            Json(AdminPasswordSettingsPatchRequest {
                login_totp_required: true,
            }),
        )
        .await
        .expect("enable login TOTP");

        assert!(!is_admin_request(state.as_ref(), &builtin_headers).await);
        assert!(
            state
                .proxy
                .get_active_admin_passkey_session(&passkey_session.token)
                .await
                .expect("lookup passkey session")
                .is_none()
        );
    }
}

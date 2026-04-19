fn detect_versions(static_dir: Option<&FsPath>) -> (String, String) {
    let backend_base = option_env!("APP_EFFECTIVE_VERSION")
        .map(|s| s.to_string())
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let backend = if cfg!(debug_assertions) {
        format!("{}-dev", backend_base)
    } else {
        backend_base
    };

    // Try reading version.json produced by front-end build
    let frontend_from_dist = static_dir.and_then(|dir| {
        let path = dir.join("version.json");
        fs::File::open(&path).ok().and_then(|mut f| {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| {
                        v.get("version")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
            } else {
                None
            }
        })
    });

    // Fallback to web/package.json for dev setups
    let frontend = frontend_from_dist
        .or_else(|| {
            let path = FsPath::new("web").join("package.json");
            fs::File::open(&path).ok().and_then(|mut f| {
                let mut s = String::new();
                if f.read_to_string(&mut s).is_ok() {
                    serde_json::from_str::<serde_json::Value>(&s)
                        .ok()
                        .and_then(|v| {
                            v.get("version")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        })
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    let frontend = if cfg!(debug_assertions) {
        format!("{}-dev", frontend)
    } else {
        frontend
    };

    (backend, frontend)
}

async fn list_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> Result<Json<PaginatedApiKeysView>, StatusCode> {
    if !is_admin_request(state.as_ref(), &headers) {
        return Err(StatusCode::FORBIDDEN);
    }
    let query = ListKeysQuery::from_query(uri.query());
    state
        .proxy
        .list_api_key_metrics_paged(
            query.page.unwrap_or(1),
            query.per_page.unwrap_or(20),
            &query.group,
            &query.status,
            query.registration_ip.as_deref(),
            &query.region,
        )
        .await
        .map(|result| {
            Json(PaginatedApiKeysView {
                items: result
                    .items
                    .into_iter()
                    .map(ApiKeyView::from_list)
                    .collect(),
                total: result.total,
                page: result.page,
                per_page: result.per_page,
                facets: ApiKeyFacetsView {
                    groups: result
                        .facets
                        .groups
                        .into_iter()
                        .map(|facet| ApiKeyFacetCountView {
                            value: facet.value,
                            count: facet.count,
                        })
                        .collect(),
                    statuses: result
                        .facets
                        .statuses
                        .into_iter()
                        .map(|facet| ApiKeyFacetCountView {
                            value: facet.value,
                            count: facet.count,
                        })
                        .collect(),
                    regions: result
                        .facets
                        .regions
                        .into_iter()
                        .map(|facet| ApiKeyFacetCountView {
                            value: facet.value,
                            count: facet.count,
                        })
                        .collect(),
                },
            })
        })
        .map_err(|err| {
            eprintln!("list keys error: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

#[derive(Debug, Default)]
struct ListKeysQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    group: Vec<String>,
    status: Vec<String>,
    registration_ip: Option<String>,
    region: Vec<String>,
}

impl ListKeysQuery {
    fn from_query(raw_query: Option<&str>) -> Self {
        let mut query = Self::default();
        let Some(raw_query) = raw_query else {
            return query;
        };

        for (key, value) in url::form_urlencoded::parse(raw_query.as_bytes()) {
            match key.as_ref() {
                "page" => {
                    if let Ok(parsed) = value.parse::<i64>() {
                        query.page = Some(parsed);
                    }
                }
                "per_page" => {
                    if let Ok(parsed) = value.parse::<i64>() {
                        query.per_page = Some(parsed);
                    }
                }
                "group" => query.group.push(value.into_owned()),
                "status" => query.status.push(value.into_owned()),
                "registration_ip" => {
                    let value = value.trim();
                    if !value.is_empty() {
                        query.registration_ip = Some(value.to_string());
                    }
                }
                "region" => query.region.push(value.into_owned()),
                _ => {}
            }
        }

        query
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyFacetCountView {
    value: String,
    count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiKeyFacetsView {
    groups: Vec<ApiKeyFacetCountView>,
    statuses: Vec<ApiKeyFacetCountView>,
    regions: Vec<ApiKeyFacetCountView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedApiKeysView {
    items: Vec<ApiKeyView>,
    total: i64,
    page: i64,
    per_page: i64,
    facets: ApiKeyFacetsView,
}

#[derive(Debug, Deserialize)]
struct CreateKeyRequest {
    api_key: String,
    group: Option<String>,
    registration_ip: Option<String>,
    assigned_proxy_key: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateKeyResponse {
    id: String,
}

const API_KEYS_BATCH_LIMIT: usize = 1000;

#[derive(Debug, Deserialize)]
struct BatchCreateKeysRequest {
    api_keys: Option<Vec<String>>,
    items: Option<Vec<BatchCreateKeyItem>>,
    group: Option<String>,
    exhausted_api_keys: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct BatchCreateKeyItem {
    api_key: String,
    registration_ip: Option<String>,
    assigned_proxy_key: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedBatchCreateKeyItem {
    api_key: String,
    registration_ip: Option<String>,
    assigned_proxy_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BulkApiKeyActionRequest {
    action: String,
    #[serde(default)]
    key_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct BulkApiKeyActionSummary {
    requested: u64,
    succeeded: u64,
    skipped: u64,
    failed: u64,
}

#[derive(Debug, Clone, Serialize)]
struct BulkApiKeyActionResult {
    key_id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BulkApiKeyActionResponse {
    summary: BulkApiKeyActionSummary,
    results: Vec<BulkApiKeyActionResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum BulkApiKeySyncProgressEvent {
    Phase {
        #[serde(rename = "phaseKey")]
        phase_key: &'static str,
        label: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        current: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<u64>,
    },
    Item {
        #[serde(rename = "keyId")]
        key_id: String,
        status: String,
        current: u64,
        total: u64,
        summary: BulkApiKeyActionSummary,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Complete {
        payload: BulkApiKeyActionResponse,
    },
    Error {
        message: String,
        #[serde(rename = "phaseKey")]
        #[serde(skip_serializing_if = "Option::is_none")]
        phase_key: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
enum BulkApiKeyActionKind {
    Delete,
    ClearQuarantine,
    SyncUsage,
}

impl BulkApiKeyActionKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "delete" => Some(Self::Delete),
            "clear_quarantine" => Some(Self::ClearQuarantine),
            "sync_usage" => Some(Self::SyncUsage),
            _ => None,
        }
    }
}

impl BatchCreateKeysRequest {
    fn into_items(self) -> Vec<BatchCreateKeyItem> {
        if let Some(items) = self.items {
            return items;
        }

        self.api_keys
            .unwrap_or_default()
            .into_iter()
            .map(|api_key| BatchCreateKeyItem {
                api_key,
                registration_ip: None,
                assigned_proxy_key: None,
            })
            .collect()
    }
}

const API_KEY_IP_GEO_BATCH_FIELDS: &str = "?fields=city,subdivision,asn";
const API_KEY_IP_GEO_BATCH_SIZE: usize = 100;
const API_KEY_IP_GEO_HTTP_TIMEOUT_SECS: u64 = 10;
const API_KEY_IP_GEO_CONNECT_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Deserialize)]
struct CountryIsBatchEntry {
    ip: String,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    subdivision: Option<String>,
}

fn normalize_ip_string(raw: &str) -> Option<String> {
    raw.trim().parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

fn normalize_global_registration_ip(raw: &str) -> Option<String> {
    let normalized = normalize_ip_string(raw)?;
    if is_global_geo_ip(&normalized) {
        Some(normalized)
    } else {
        None
    }
}

fn is_global_geo_ip(raw: &str) -> bool {
    match raw.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => is_public_ipv4(ip),
        Ok(IpAddr::V6(ip)) => is_public_ipv6(ip),
        Err(_) => false,
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
    {
        return false;
    }

    let [a, b, c, _d] = ip.octets();
    if a == 0 {
        return false;
    }
    if a == 100 && (64..=127).contains(&b) {
        return false;
    }
    if a == 192 && b == 0 && c == 0 {
        return false;
    }
    if a == 198 && (b == 18 || b == 19) {
        return false;
    }
    if a >= 240 {
        return false;
    }

    true
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4() {
        return is_public_ipv4(v4);
    }

    let segments = ip.segments();
    let is_documentation = segments[0] == 0x2001 && segments[1] == 0x0db8;
    !ip.is_loopback()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && !ip.is_unique_local()
        && !ip.is_unicast_link_local()
        && !is_documentation
}

fn trim_or_empty(value: Option<String>) -> String {
    value
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn looks_like_subdivision_code(raw: &str) -> bool {
    let raw = raw.trim();
    let len = raw.len();
    if !(2..=3).contains(&len) {
        return false;
    }
    raw.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn format_registration_region(country: &str, subdivision: &str, city: &str) -> Option<String> {
    let mut parts = Vec::new();
    if !country.is_empty() {
        parts.push(country.to_string());
    }
    if !subdivision.is_empty() {
        if looks_like_subdivision_code(subdivision) && !city.is_empty() {
            parts.push(format!("{city} ({subdivision})"));
        } else {
            parts.push(subdivision.to_string());
        }
    } else if parts.is_empty() && !city.is_empty() {
        parts.push(city.to_string());
    }
    let result = parts.join(" ").trim().to_string();
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn build_registration_geo_batch_url(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    if origin.contains('?') {
        format!("{origin}&{}", API_KEY_IP_GEO_BATCH_FIELDS.trim_start_matches('?'))
    } else {
        format!("{origin}{API_KEY_IP_GEO_BATCH_FIELDS}")
    }
}

async fn resolve_registration_regions(
    origin: &str,
    ips: &[String],
) -> HashMap<String, String> {
    let pending = ips
        .iter()
        .filter_map(|ip| normalize_global_registration_ip(ip))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return HashMap::new();
    }

    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(API_KEY_IP_GEO_CONNECT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(API_KEY_IP_GEO_HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            eprintln!("build api key geo resolver client error: {err}");
            return HashMap::new();
        }
    };
    let batch_url = build_registration_geo_batch_url(origin);
    let mut resolved = HashMap::new();

    'batch_lookup: for batch in pending.chunks(API_KEY_IP_GEO_BATCH_SIZE) {
        let mut attempt = 0usize;
        let response = loop {
            match client.post(&batch_url).json(batch).send().await {
                Ok(response) if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt == 0 => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    continue;
                }
                Ok(response) => break response,
                Err(err) if attempt == 0 => {
                    attempt += 1;
                    eprintln!("api key geo lookup request error, retrying once: {err}");
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(err) => {
                    eprintln!("api key geo lookup request error: {err}");
                    continue 'batch_lookup;
                }
            }
        };

        let status = response.status();
        if !status.is_success() {
            eprintln!("api key geo lookup returned status: {status}");
            continue;
        }

        let entries = match response.json::<Vec<CountryIsBatchEntry>>().await {
            Ok(entries) => entries,
            Err(err) => {
                eprintln!("api key geo lookup decode error: {err}");
                continue;
            }
        };

        for entry in entries {
            let Some(ip) = normalize_ip_string(&entry.ip) else {
                continue;
            };
            let region = format_registration_region(
                trim_or_empty(entry.country).as_str(),
                trim_or_empty(entry.subdivision).as_str(),
                trim_or_empty(entry.city).as_str(),
            );
            if let Some(region) = region {
                resolved.insert(ip, region);
            }
        }
    }

    resolved
}

#[derive(Debug, Default, Serialize)]
struct BatchCreateKeysSummary {
    input_lines: u64,
    valid_lines: u64,
    unique_in_input: u64,
    created: u64,
    undeleted: u64,
    existed: u64,
    duplicate_in_input: u64,
    failed: u64,
    ignored_empty: u64,
}

#[derive(Debug, Serialize)]
struct BatchCreateKeysResult {
    api_key: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    marked_exhausted: Option<bool>,
}

#[derive(Debug, Serialize)]
struct BatchCreateKeysResponse {
    summary: BatchCreateKeysSummary,
    results: Vec<BatchCreateKeysResult>,
}

#[derive(Debug, Deserialize)]
struct ValidateKeysRequest {
    #[serde(default)]
    api_keys: Vec<String>,
    #[serde(default)]
    items: Vec<ValidateKeyItemInput>,
}

#[derive(Debug, Deserialize)]
struct ValidateKeyItemInput {
    api_key: String,
    #[serde(default)]
    registration_ip: Option<String>,
}

#[derive(Debug)]
struct NormalizedValidateKeyItem {
    api_key: String,
    registration_ip: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct ValidateKeysSummary {
    input_lines: u64,
    valid_lines: u64,
    unique_in_input: u64,
    duplicate_in_input: u64,
    ok: u64,
    exhausted: u64,
    invalid: u64,
    error: u64,
}

#[derive(Debug, Serialize)]
struct ValidateKeyResult {
    api_key: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    registration_region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assigned_proxy_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assigned_proxy_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    assigned_proxy_match_kind: Option<tavily_hikari::AssignedProxyMatchKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quota_remaining: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct ValidateKeysResponse {
    summary: ValidateKeysSummary,
    results: Vec<ValidateKeyResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsResponse {
    forward_proxy: Option<tavily_hikari::ForwardProxySettingsResponse>,
    system_settings: tavily_hikari::SystemSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForwardProxySettingsUpdatePayload {
    #[serde(default)]
    proxy_urls: Vec<String>,
    #[serde(default)]
    subscription_urls: Vec<String>,
    #[serde(default = "default_forward_proxy_subscription_update_interval_secs")]
    subscription_update_interval_secs: u64,
    #[serde(default = "default_forward_proxy_insert_direct")]
    insert_direct: bool,
    #[serde(default)]
    egress_socks5_enabled: bool,
    #[serde(default)]
    egress_socks5_url: String,
    #[serde(default)]
    skip_bootstrap_probe: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemSettingsUpdatePayload {
    request_rate_limit: Option<i64>,
    mcp_session_affinity_key_count: i64,
    #[serde(default)]
    rebalance_mcp_enabled: bool,
    #[serde(default = "default_rebalance_mcp_session_percent")]
    rebalance_mcp_session_percent: i64,
}

fn default_rebalance_mcp_session_percent() -> i64 {
    tavily_hikari::REBALANCE_MCP_SESSION_PERCENT_DEFAULT
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
enum ForwardProxyValidationKindPayload {
    ProxyUrl,
    SubscriptionUrl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForwardProxyValidationPayload {
    kind: ForwardProxyValidationKindPayload,
    value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardProxyValidationView {
    ok: bool,
    message: String,
    normalized_value: Option<String>,
    discovered_nodes: Option<usize>,
    latency_ms: Option<f64>,
    error_code: Option<String>,
    nodes: Vec<ForwardProxyValidationNodeView>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ForwardProxyValidationNodeView {
    display_name: String,
    ok: bool,
    latency_ms: Option<f64>,
    ip: Option<String>,
    location: Option<String>,
    message: Option<String>,
}

#[derive(Clone)]
struct ForwardProxyStreamCancelGuard(tavily_hikari::ForwardProxyCancellation);

impl ForwardProxyStreamCancelGuard {
    fn new(cancellation: tavily_hikari::ForwardProxyCancellation) -> Self {
        Self(cancellation)
    }
}

impl Drop for ForwardProxyStreamCancelGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

fn default_forward_proxy_subscription_update_interval_secs() -> u64 {
    3600
}

fn default_forward_proxy_insert_direct() -> bool {
    true
}

fn request_accepts_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| {
            accept
                .split(',')
                .map(str::trim)
                .filter_map(|item| item.split(';').next())
                .map(str::trim)
                .any(|item| item.eq_ignore_ascii_case("text/event-stream"))
        })
}

fn build_forward_proxy_validation_view(
    validation: tavily_hikari::ForwardProxyValidationResponse,
) -> ForwardProxyValidationView {
    let tavily_hikari::ForwardProxyValidationResponse {
        ok,
        normalized_values,
        discovered_nodes,
        latency_ms,
        results,
        first_error,
    } = validation;
    let result = results.into_iter().next();
    if let Some(result) = result {
        return ForwardProxyValidationView {
            ok: result.ok,
            message: result.message,
            normalized_value: result.normalized_value,
            discovered_nodes: result.discovered_nodes,
            latency_ms: result.latency_ms,
            error_code: result.error_code,
            nodes: result
                .nodes
                .into_iter()
                .map(|node| ForwardProxyValidationNodeView {
                    display_name: node.display_name,
                    ok: node.ok,
                    latency_ms: node.latency_ms,
                    ip: node.ip,
                    location: node.location,
                    message: node.message,
                })
                .collect(),
        };
    }

    if let Some(error) = first_error {
        return ForwardProxyValidationView {
            ok: false,
            message: error.message,
            normalized_value: None,
            discovered_nodes: Some(discovered_nodes),
            latency_ms,
            error_code: Some(error.code),
            nodes: Vec::new(),
        };
    }

    ForwardProxyValidationView {
        ok,
        message: if ok {
            "validation succeeded".to_string()
        } else {
            "validation failed".to_string()
        },
        normalized_value: normalized_values.into_iter().next(),
        discovered_nodes: Some(discovered_nodes),
        latency_ms,
        error_code: None,
        nodes: Vec::new(),
    }
}

pub fn default_xray_binary() -> String {
    DEFAULT_XRAY_BINARY.to_string()
}

pub fn default_xray_runtime_dir(database_path: &str) -> PathBuf {
    let db_path = PathBuf::from(database_path);
    db_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(DEFAULT_XRAY_RUNTIME_DIR)
}

pub fn derive_probe_url(_upstream: &Url) -> Url {
    Url::parse(FORWARD_PROXY_VALIDATION_PROBE_URL)
        .expect("forward proxy validation probe url should be a valid absolute url")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxySettings {
    #[serde(default)]
    pub proxy_urls: Vec<String>,
    #[serde(default)]
    pub subscription_urls: Vec<String>,
    #[serde(default = "default_forward_proxy_subscription_interval_secs")]
    pub subscription_update_interval_secs: u64,
    #[serde(default = "default_forward_proxy_insert_direct")]
    pub insert_direct: bool,
    #[serde(default)]
    pub egress_socks5_enabled: bool,
    #[serde(default)]
    pub egress_socks5_url: String,
}

impl Default for ForwardProxySettings {
    fn default() -> Self {
        Self {
            proxy_urls: Vec::new(),
            subscription_urls: Vec::new(),
            subscription_update_interval_secs: default_forward_proxy_subscription_interval_secs(),
            insert_direct: default_forward_proxy_insert_direct(),
            egress_socks5_enabled: false,
            egress_socks5_url: String::new(),
        }
    }
}

impl ForwardProxySettings {
    pub fn normalized(self) -> Self {
        Self {
            proxy_urls: normalize_proxy_url_entries(self.proxy_urls),
            subscription_urls: normalize_subscription_entries(self.subscription_urls),
            subscription_update_interval_secs: self
                .subscription_update_interval_secs
                .clamp(60, 7 * 24 * 60 * 60),
            insert_direct: self.insert_direct,
            egress_socks5_enabled: self.egress_socks5_enabled,
            egress_socks5_url: normalize_egress_socks5_url(self.egress_socks5_url),
        }
    }

    pub fn effective_egress_socks5_url(&self) -> Option<Url> {
        if !self.egress_socks5_enabled {
            return None;
        }
        parse_egress_socks5_url(self.egress_socks5_url.trim())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxySettingsUpdateRequest {
    #[serde(default)]
    pub proxy_urls: Vec<String>,
    #[serde(default)]
    pub subscription_urls: Vec<String>,
    #[serde(default = "default_forward_proxy_subscription_interval_secs")]
    pub subscription_update_interval_secs: u64,
    #[serde(default = "default_forward_proxy_insert_direct")]
    pub insert_direct: bool,
    #[serde(default)]
    pub egress_socks5_enabled: bool,
    #[serde(default)]
    pub egress_socks5_url: String,
}

impl From<ForwardProxySettingsUpdateRequest> for ForwardProxySettings {
    fn from(value: ForwardProxySettingsUpdateRequest) -> Self {
        Self {
            proxy_urls: value.proxy_urls,
            subscription_urls: value.subscription_urls,
            subscription_update_interval_secs: value.subscription_update_interval_secs,
            insert_direct: value.insert_direct,
            egress_socks5_enabled: value.egress_socks5_enabled,
            egress_socks5_url: value.egress_socks5_url,
        }
        .normalized()
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardProxyValidationKind {
    ProxyUrl,
    SubscriptionUrl,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyCandidateValidationRequest {
    pub kind: ForwardProxyValidationKind,
    pub value: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyCandidateValidationResponse {
    pub ok: bool,
    pub message: String,
    pub normalized_value: Option<String>,
    pub discovered_nodes: Option<usize>,
    pub latency_ms: Option<f64>,
}

impl ForwardProxyCandidateValidationResponse {
    pub fn success(
        message: impl Into<String>,
        normalized_value: Option<String>,
        discovered_nodes: Option<usize>,
        latency_ms: Option<f64>,
    ) -> Self {
        Self {
            ok: true,
            message: message.into(),
            normalized_value,
            discovered_nodes,
            latency_ms,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            normalized_value: None,
            discovered_nodes: None,
            latency_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyValidationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyValidationProbeResult {
    pub value: String,
    pub normalized_value: Option<String>,
    pub ok: bool,
    pub discovered_nodes: Option<usize>,
    pub latency_ms: Option<f64>,
    pub error_code: Option<String>,
    pub message: String,
    #[serde(default)]
    pub nodes: Vec<ForwardProxyValidationNodeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyValidationResponse {
    pub ok: bool,
    pub normalized_values: Vec<String>,
    pub discovered_nodes: usize,
    pub latency_ms: Option<f64>,
    pub results: Vec<ForwardProxyValidationProbeResult>,
    pub first_error: Option<ForwardProxyValidationError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyValidationNodeResult {
    pub display_name: String,
    pub protocol: String,
    pub ok: bool,
    pub latency_ms: Option<f64>,
    pub ip: Option<String>,
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ForwardProxyAffinityRecord {
    pub primary_proxy_key: Option<String>,
    pub secondary_proxy_key: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardProxyProtocol {
    Direct,
    Http,
    Https,
    Socks5,
    Socks5h,
    Vmess,
    Vless,
    Trojan,
    Shadowsocks,
}

impl ForwardProxyProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Http => "http",
            Self::Https => "https",
            Self::Socks5 => "socks5",
            Self::Socks5h => "socks5h",
            Self::Vmess => "vmess",
            Self::Vless => "vless",
            Self::Trojan => "trojan",
            Self::Shadowsocks => "ss",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForwardProxyEndpoint {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub protocol: ForwardProxyProtocol,
    pub endpoint_url: Option<Url>,
    pub raw_url: Option<String>,
    pub uses_local_relay: bool,
    pub manual_present: bool,
    pub subscription_sources: BTreeSet<String>,
    relay_handle: Option<Weak<SharedXrayRelayHandle>>,
}

impl ForwardProxyEndpoint {
    pub fn direct() -> Self {
        Self {
            key: FORWARD_PROXY_DIRECT_KEY.to_string(),
            source: FORWARD_PROXY_SOURCE_DIRECT.to_string(),
            display_name: FORWARD_PROXY_DIRECT_LABEL.to_string(),
            protocol: ForwardProxyProtocol::Direct,
            endpoint_url: None,
            raw_url: None,
            uses_local_relay: false,
            manual_present: false,
            subscription_sources: BTreeSet::new(),
            relay_handle: None,
        }
    }

    pub fn new_manual(
        key: String,
        display_name: String,
        protocol: ForwardProxyProtocol,
        endpoint_url: Option<Url>,
        raw_url: Option<String>,
    ) -> Self {
        Self {
            key,
            source: FORWARD_PROXY_SOURCE_MANUAL.to_string(),
            display_name,
            protocol,
            endpoint_url,
            raw_url,
            uses_local_relay: false,
            manual_present: true,
            subscription_sources: BTreeSet::new(),
            relay_handle: None,
        }
    }

    pub fn new_subscription(
        key: String,
        display_name: String,
        protocol: ForwardProxyProtocol,
        endpoint_url: Option<Url>,
        raw_url: Option<String>,
        subscription_source: String,
    ) -> Self {
        let mut endpoint = Self {
            key,
            source: FORWARD_PROXY_SOURCE_SUBSCRIPTION.to_string(),
            display_name,
            protocol,
            endpoint_url,
            raw_url,
            uses_local_relay: false,
            manual_present: false,
            subscription_sources: BTreeSet::from([subscription_source]),
            relay_handle: None,
        };
        endpoint.refresh_source();
        endpoint
    }

    pub fn refresh_source(&mut self) {
        self.source = if self.is_direct() {
            FORWARD_PROXY_SOURCE_DIRECT.to_string()
        } else if self.manual_present {
            FORWARD_PROXY_SOURCE_MANUAL.to_string()
        } else if !self.subscription_sources.is_empty() {
            FORWARD_PROXY_SOURCE_SUBSCRIPTION.to_string()
        } else {
            FORWARD_PROXY_SOURCE_MANUAL.to_string()
        };
    }

    pub fn is_subscription_backed(&self) -> bool {
        !self.subscription_sources.is_empty()
    }

    pub fn is_selectable(&self) -> bool {
        self.protocol == ForwardProxyProtocol::Direct || self.endpoint_url.is_some()
    }

    pub fn is_direct(&self) -> bool {
        self.protocol == ForwardProxyProtocol::Direct
    }

    pub fn requires_xray(&self) -> bool {
        matches!(
            self.protocol,
            ForwardProxyProtocol::Vmess
                | ForwardProxyProtocol::Vless
                | ForwardProxyProtocol::Trojan
                | ForwardProxyProtocol::Shadowsocks
        )
    }

    pub fn needs_local_relay(&self, egress_socks5_url: Option<&Url>) -> bool {
        !self.is_direct() && (self.requires_xray() || egress_socks5_url.is_some())
    }

    pub fn absorb_duplicate(&mut self, mut other: ForwardProxyEndpoint) {
        let prefer_other_fields = !self.manual_present && other.manual_present;
        self.manual_present |= other.manual_present;
        self.subscription_sources
            .append(&mut other.subscription_sources);
        self.uses_local_relay |= other.uses_local_relay;
        if prefer_other_fields {
            self.display_name = other.display_name;
            self.protocol = other.protocol;
            self.endpoint_url = other.endpoint_url;
            self.raw_url = other.raw_url;
            self.uses_local_relay = other.uses_local_relay;
            self.relay_handle = None;
        }
        self.refresh_source();
    }

    pub(crate) fn relay_handle(&self) -> Option<Arc<SharedXrayRelayHandle>> {
        self.relay_handle.as_ref().and_then(Weak::upgrade)
    }

    fn set_relay_handle(&mut self, relay_handle: Option<&Arc<SharedXrayRelayHandle>>) {
        self.relay_handle = relay_handle.map(Arc::downgrade);
    }
}

pub fn endpoint_host(endpoint: &ForwardProxyEndpoint) -> Option<String> {
    if endpoint.requires_xray() || endpoint.uses_local_relay {
        return endpoint
            .raw_url
            .as_deref()
            .and_then(raw_endpoint_host)
            .or_else(|| {
                endpoint
                    .endpoint_url
                    .as_ref()
                    .and_then(|url| url.host_str().map(ToOwned::to_owned))
            });
    }
    if let Some(url) = endpoint.endpoint_url.as_ref() {
        return url.host_str().map(ToOwned::to_owned);
    }
    endpoint.raw_url.as_deref().and_then(raw_endpoint_host)
}

fn endpoint_transport_url(endpoint: &ForwardProxyEndpoint) -> Option<Url> {
    endpoint
        .raw_url
        .as_deref()
        .and_then(parse_forward_proxy_entry)
        .and_then(|parsed| parsed.endpoint_url)
        .or_else(|| endpoint.endpoint_url.clone())
}

fn raw_endpoint_host(raw: &str) -> Option<String> {
    if !raw.contains("://") {
        return Url::parse(&format!("http://{raw}"))
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned));
    }
    let (scheme_raw, _) = raw.split_once("://")?;
    match scheme_raw.to_ascii_lowercase().as_str() {
        "http" | "https" | "socks5" | "socks5h" | "socks" | "vless" | "trojan" => Url::parse(raw)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned)),
        "vmess" => parse_vmess_share_link(raw)
            .ok()
            .map(|parsed| parsed.address),
        "ss" => parse_shadowsocks_share_link(raw)
            .ok()
            .map(|parsed| parsed.host),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct ForwardProxyRuntimeState {
    pub proxy_key: String,
    pub display_name: String,
    pub source: String,
    pub kind: String,
    pub endpoint_url: Option<String>,
    pub resolved_ip_source: String,
    pub resolved_ips: Vec<String>,
    pub resolved_regions: Vec<String>,
    pub geo_refreshed_at: i64,
    pub available: bool,
    pub last_error: Option<String>,
    pub weight: f64,
    pub success_ema: f64,
    pub latency_ema_ms: Option<f64>,
    pub consecutive_failures: u32,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyRuntimeGeoMetadataUpdate {
    pub proxy_key: String,
    pub display_name: String,
    pub source: String,
    pub endpoint_url: Option<String>,
    pub resolved_ip_source: String,
    pub resolved_ips: Vec<String>,
    pub resolved_regions: Vec<String>,
    pub geo_refreshed_at: i64,
    pub weight: f64,
    pub success_ema: f64,
    pub latency_ema_ms: Option<f64>,
    pub consecutive_failures: u32,
    pub is_penalized: bool,
}

impl ForwardProxyRuntimeState {
    pub fn default_for_endpoint(endpoint: &ForwardProxyEndpoint) -> Self {
        Self {
            proxy_key: endpoint.key.clone(),
            display_name: endpoint.display_name.clone(),
            source: endpoint.source.clone(),
            kind: endpoint.protocol.as_str().to_string(),
            endpoint_url: endpoint
                .endpoint_url
                .as_ref()
                .map(Url::to_string)
                .or_else(|| endpoint.raw_url.clone()),
            resolved_ip_source: String::new(),
            resolved_ips: Vec::new(),
            resolved_regions: Vec::new(),
            geo_refreshed_at: 0,
            available: endpoint.is_selectable(),
            last_error: if endpoint.is_selectable() {
                None
            } else {
                Some("xray_missing".to_string())
            },
            weight: if endpoint.key == FORWARD_PROXY_DIRECT_KEY {
                1.0
            } else {
                0.8
            },
            success_ema: 0.65,
            latency_ema_ms: None,
            consecutive_failures: 0,
        }
    }

    pub fn is_penalized(&self) -> bool {
        self.weight <= 0.0
    }
}

#[derive(Debug, FromRow)]
struct ForwardProxySettingsRow {
    proxy_urls_json: Option<String>,
    subscription_urls_json: Option<String>,
    subscription_update_interval_secs: Option<i64>,
    insert_direct: Option<i64>,
    egress_socks5_enabled: Option<i64>,
    egress_socks5_url: Option<String>,
}

impl From<ForwardProxySettingsRow> for ForwardProxySettings {
    fn from(value: ForwardProxySettingsRow) -> Self {
        let proxy_urls = decode_string_vec_json(value.proxy_urls_json.as_deref());
        let subscription_urls = decode_string_vec_json(value.subscription_urls_json.as_deref());
        let interval = value
            .subscription_update_interval_secs
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or_else(default_forward_proxy_subscription_interval_secs);
        let insert_direct = value
            .insert_direct
            .map(|value| value != 0)
            .unwrap_or_else(default_forward_proxy_insert_direct);
        let egress_socks5_enabled = value.egress_socks5_enabled.is_some_and(|value| value != 0);
        let egress_socks5_url = value.egress_socks5_url.unwrap_or_default();
        Self {
            proxy_urls,
            subscription_urls,
            subscription_update_interval_secs: interval,
            insert_direct,
            egress_socks5_enabled,
            egress_socks5_url,
        }
        .normalized()
    }
}

#[derive(Debug, FromRow)]
struct ForwardProxyRuntimeRow {
    proxy_key: String,
    display_name: String,
    source: String,
    endpoint_url: Option<String>,
    resolved_ip_source: Option<String>,
    resolved_ips_json: Option<String>,
    resolved_regions_json: Option<String>,
    geo_refreshed_at: Option<i64>,
    weight: f64,
    success_ema: f64,
    latency_ema_ms: Option<f64>,
    consecutive_failures: i64,
}

impl From<ForwardProxyRuntimeRow> for ForwardProxyRuntimeState {
    fn from(value: ForwardProxyRuntimeRow) -> Self {
        Self {
            proxy_key: value.proxy_key,
            display_name: value.display_name,
            source: value.source,
            kind: "unknown".to_string(),
            endpoint_url: value.endpoint_url,
            resolved_ip_source: value
                .resolved_ip_source
                .unwrap_or_default()
                .trim()
                .to_string(),
            resolved_ips: decode_string_vec_json(value.resolved_ips_json.as_deref()),
            resolved_regions: decode_string_vec_json(value.resolved_regions_json.as_deref()),
            geo_refreshed_at: value.geo_refreshed_at.unwrap_or_default().max(0),
            available: true,
            last_error: None,
            weight: value
                .weight
                .clamp(FORWARD_PROXY_WEIGHT_MIN, FORWARD_PROXY_WEIGHT_MAX),
            success_ema: value.success_ema.clamp(0.0, 1.0),
            latency_ema_ms: value
                .latency_ema_ms
                .filter(|value| value.is_finite() && *value >= 0.0),
            consecutive_failures: value.consecutive_failures.max(0) as u32,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForwardProxyManager {
    pub settings: ForwardProxySettings,
    pub endpoints: Vec<ForwardProxyEndpoint>,
    pub runtime: HashMap<String, ForwardProxyRuntimeState>,
    pub selection_counter: u64,
    pub requests_since_probe: u64,
    pub probe_in_flight: bool,
    pub last_probe_at: i64,
    pub last_subscription_refresh_at: Option<i64>,
}


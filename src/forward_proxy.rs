#![allow(dead_code)]

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine;
use chrono::{TimeZone, Utc};
use reqwest::{Client, Proxy, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, QueryBuilder, Row, Sqlite, SqlitePool};
use tokio::{
    net::TcpStream,
    process::{Child, Command},
    sync::RwLock,
    time::{sleep, timeout},
};

use crate::{KeyStore, ProxyError};

pub const DEFAULT_XRAY_BINARY: &str = "xray";
pub const DEFAULT_XRAY_RUNTIME_DIR: &str = "data/xray-runtime";

const FORWARD_PROXY_SETTINGS_SINGLETON_ID: i64 = 1;
pub const DEFAULT_FORWARD_PROXY_INSERT_DIRECT: bool = true;
pub const DEFAULT_FORWARD_PROXY_SUBSCRIPTION_INTERVAL_SECS: u64 = 60 * 60;
pub const FORWARD_PROXY_DEFAULT_PRIMARY_CANDIDATE_COUNT: usize = 3;
pub const FORWARD_PROXY_DEFAULT_SECONDARY_CANDIDATE_COUNT: usize = 3;
const FORWARD_PROXY_WEIGHT_RECOVERY: f64 = 0.6;
const FORWARD_PROXY_WEIGHT_SUCCESS_BONUS: f64 = 0.45;
const FORWARD_PROXY_WEIGHT_FAILURE_PENALTY_BASE: f64 = 0.9;
const FORWARD_PROXY_WEIGHT_FAILURE_PENALTY_STEP: f64 = 0.35;
const FORWARD_PROXY_WEIGHT_MIN: f64 = -12.0;
const FORWARD_PROXY_WEIGHT_MAX: f64 = 12.0;
const FORWARD_PROXY_PROBE_EVERY_REQUESTS: u64 = 100;
const FORWARD_PROXY_PROBE_INTERVAL_SECS: i64 = 30 * 60;
const FORWARD_PROXY_PROBE_RECOVERY_WEIGHT: f64 = 0.4;
pub const FORWARD_PROXY_VALIDATION_TIMEOUT_SECS: u64 = 5;
pub const FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS: u64 = 60;
// Use a public plain-HTTP probe target so both real proxies and our test doubles
// can exercise reachability without relying on CONNECT support to localhost.
const FORWARD_PROXY_VALIDATION_PROBE_URL: &str = "http://example.com/";
pub const FORWARD_PROXY_DIRECT_KEY: &str = "__direct__";
pub const FORWARD_PROXY_DIRECT_LABEL: &str = "Direct";
pub const FORWARD_PROXY_SOURCE_MANUAL: &str = "manual";
pub const FORWARD_PROXY_SOURCE_SUBSCRIPTION: &str = "subscription";
pub const FORWARD_PROXY_SOURCE_DIRECT: &str = "direct";
pub const FORWARD_PROXY_FAILURE_SEND_ERROR: &str = "send_error";
pub const FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT: &str = "handshake_timeout";
pub const FORWARD_PROXY_FAILURE_STREAM_ERROR: &str = "stream_error";
pub const FORWARD_PROXY_FAILURE_UPSTREAM_HTTP_429: &str = "upstream_http_429";
pub const FORWARD_PROXY_FAILURE_UPSTREAM_HTTP_5XX: &str = "upstream_http_5xx";
const XRAY_PROXY_READY_TIMEOUT_MS: u64 = 3_000;

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
}

impl Default for ForwardProxySettings {
    fn default() -> Self {
        Self {
            proxy_urls: Vec::new(),
            subscription_urls: Vec::new(),
            subscription_update_interval_secs: default_forward_proxy_subscription_interval_secs(),
            insert_direct: default_forward_proxy_insert_direct(),
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
        }
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
}

impl From<ForwardProxySettingsUpdateRequest> for ForwardProxySettings {
    fn from(value: ForwardProxySettingsUpdateRequest) -> Self {
        Self {
            proxy_urls: value.proxy_urls,
            subscription_urls: value.subscription_urls,
            subscription_update_interval_secs: value.subscription_update_interval_secs,
            insert_direct: value.insert_direct,
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
    pub manual_present: bool,
    pub subscription_sources: BTreeSet<String>,
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
            manual_present: false,
            subscription_sources: BTreeSet::new(),
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
            manual_present: true,
            subscription_sources: BTreeSet::new(),
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
            manual_present: false,
            subscription_sources: BTreeSet::from([subscription_source]),
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

    pub fn absorb_duplicate(&mut self, mut other: ForwardProxyEndpoint) {
        let prefer_other_fields = !self.manual_present && other.manual_present;
        self.manual_present |= other.manual_present;
        self.subscription_sources
            .append(&mut other.subscription_sources);
        if prefer_other_fields {
            self.display_name = other.display_name;
            self.protocol = other.protocol;
            self.endpoint_url = other.endpoint_url;
            self.raw_url = other.raw_url;
        }
        self.refresh_source();
    }
}

pub fn endpoint_host(endpoint: &ForwardProxyEndpoint) -> Option<String> {
    if endpoint.requires_xray() {
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
    pub available: bool,
    pub last_error: Option<String>,
    pub weight: f64,
    pub success_ema: f64,
    pub latency_ema_ms: Option<f64>,
    pub consecutive_failures: u32,
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
        Self {
            proxy_urls,
            subscription_urls,
            subscription_update_interval_secs: interval,
            insert_direct,
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

impl ForwardProxyManager {
    pub fn new(
        settings: ForwardProxySettings,
        runtime_rows: Vec<ForwardProxyRuntimeState>,
    ) -> Self {
        let runtime = runtime_rows
            .into_iter()
            .map(|entry| (entry.proxy_key.clone(), entry))
            .collect::<HashMap<_, _>>();
        let mut manager = Self {
            settings,
            endpoints: Vec::new(),
            runtime,
            selection_counter: 0,
            requests_since_probe: 0,
            probe_in_flight: false,
            last_probe_at: Utc::now().timestamp() - FORWARD_PROXY_PROBE_INTERVAL_SECS,
            last_subscription_refresh_at: None,
        };
        manager.rebuild_endpoints(Vec::new());
        manager
    }

    pub fn apply_settings(&mut self, settings: ForwardProxySettings) {
        self.settings = settings;
        self.rebuild_endpoints(Vec::new());
    }

    pub fn update_settings_only(&mut self, settings: ForwardProxySettings) {
        self.settings = settings;
    }

    pub fn apply_subscription_refresh(
        &mut self,
        subscription_proxy_urls: &HashMap<String, Vec<String>>,
    ) {
        let mut subscription_endpoints = Vec::new();
        for (subscription_source, proxy_urls) in subscription_proxy_urls {
            subscription_endpoints.extend(normalize_subscription_endpoints_from_urls(
                proxy_urls,
                subscription_source,
            ));
        }
        self.rebuild_endpoints(subscription_endpoints);
        self.last_subscription_refresh_at = Some(Utc::now().timestamp());
    }

    pub fn rebuild_endpoints(&mut self, subscription_endpoints: Vec<ForwardProxyEndpoint>) {
        let manual = normalize_proxy_endpoints_from_urls(&self.settings.proxy_urls);
        let mut merged: Vec<ForwardProxyEndpoint> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        for endpoint in manual.into_iter().chain(subscription_endpoints.into_iter()) {
            if let Some(index) = positions.get(&endpoint.key).copied() {
                merged[index].absorb_duplicate(endpoint);
            } else {
                positions.insert(endpoint.key.clone(), merged.len());
                merged.push(endpoint);
            }
        }
        if self.settings.insert_direct {
            merged.push(ForwardProxyEndpoint::direct());
        }
        if merged.is_empty()
            && self.settings.proxy_urls.is_empty()
            && self.settings.subscription_urls.is_empty()
        {
            merged.push(ForwardProxyEndpoint::direct());
        }
        self.endpoints = merged;

        for endpoint in &self.endpoints {
            match self.runtime.entry(endpoint.key.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    let runtime = occupied.get_mut();
                    runtime.display_name = endpoint.display_name.clone();
                    runtime.source = endpoint.source.clone();
                    runtime.kind = endpoint.protocol.as_str().to_string();
                    runtime.endpoint_url = endpoint
                        .endpoint_url
                        .as_ref()
                        .map(Url::to_string)
                        .or_else(|| endpoint.raw_url.clone());
                    runtime.available = endpoint.is_selectable();
                    runtime.last_error = if endpoint.is_selectable() {
                        None
                    } else {
                        Some("xray_missing".to_string())
                    };
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(ForwardProxyRuntimeState::default_for_endpoint(endpoint));
                }
            }
        }
        self.ensure_non_zero_weight();
    }

    pub fn apply_incremental_settings(
        &mut self,
        settings: ForwardProxySettings,
        fetched_subscriptions: &HashMap<String, Vec<String>>,
    ) -> Vec<ForwardProxyEndpoint> {
        let previous_keys = self
            .endpoints
            .iter()
            .map(|endpoint| endpoint.key.clone())
            .collect::<HashSet<_>>();
        self.settings = settings.clone();

        let manual_by_key = normalize_proxy_endpoints_from_urls(&settings.proxy_urls)
            .into_iter()
            .map(|endpoint| (endpoint.key.clone(), endpoint))
            .collect::<HashMap<_, _>>();
        let desired_subscription_sources = settings
            .subscription_urls
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        let mut merged = Vec::new();
        let mut seen = HashSet::new();

        for mut endpoint in self.endpoints.clone() {
            if endpoint.is_direct() {
                continue;
            }
            endpoint.manual_present = manual_by_key.contains_key(&endpoint.key);
            endpoint
                .subscription_sources
                .retain(|source| desired_subscription_sources.contains(source));
            if endpoint.manual_present || endpoint.is_subscription_backed() {
                endpoint.refresh_source();
                if seen.insert(endpoint.key.clone()) {
                    merged.push(endpoint);
                }
            }
        }

        for (key, manual_endpoint) in &manual_by_key {
            if let Some(existing) = merged.iter_mut().find(|endpoint| endpoint.key == *key) {
                existing.manual_present = true;
                existing.display_name = manual_endpoint.display_name.clone();
                existing.protocol = manual_endpoint.protocol;
                existing.raw_url = manual_endpoint.raw_url.clone();
                existing.refresh_source();
                continue;
            }
            if seen.insert(key.clone()) {
                merged.push(manual_endpoint.clone());
            }
        }

        for (subscription_source, proxy_urls) in fetched_subscriptions {
            for mut endpoint in
                normalize_subscription_endpoints_from_urls(proxy_urls, subscription_source)
            {
                if let Some(existing) = merged
                    .iter_mut()
                    .find(|candidate| candidate.key == endpoint.key)
                {
                    existing
                        .subscription_sources
                        .append(&mut endpoint.subscription_sources);
                    existing.refresh_source();
                    continue;
                }
                if seen.insert(endpoint.key.clone()) {
                    merged.push(endpoint);
                }
            }
        }

        if !fetched_subscriptions.is_empty() {
            self.last_subscription_refresh_at = Some(Utc::now().timestamp());
        }

        if settings.insert_direct {
            merged.push(ForwardProxyEndpoint::direct());
        }
        if merged.is_empty()
            && settings.proxy_urls.is_empty()
            && settings.subscription_urls.is_empty()
        {
            merged.push(ForwardProxyEndpoint::direct());
        }

        self.endpoints = merged;
        for endpoint in &self.endpoints {
            match self.runtime.entry(endpoint.key.clone()) {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    let runtime = occupied.get_mut();
                    runtime.display_name = endpoint.display_name.clone();
                    runtime.source = endpoint.source.clone();
                    runtime.kind = endpoint.protocol.as_str().to_string();
                    runtime.endpoint_url = endpoint
                        .endpoint_url
                        .as_ref()
                        .map(Url::to_string)
                        .or_else(|| endpoint.raw_url.clone());
                    runtime.available = endpoint.is_selectable();
                    runtime.last_error = if endpoint.is_selectable() {
                        None
                    } else {
                        Some("xray_missing".to_string())
                    };
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert(ForwardProxyRuntimeState::default_for_endpoint(endpoint));
                }
            }
        }
        self.ensure_non_zero_weight();

        self.endpoints
            .iter()
            .filter(|endpoint| !previous_keys.contains(&endpoint.key))
            .cloned()
            .collect()
    }

    pub fn ensure_non_zero_weight(&mut self) {
        let selectable_keys = self.selectable_endpoint_keys();
        let mut positive_count = self
            .runtime
            .values()
            .filter(|entry| {
                selectable_keys.contains(entry.proxy_key.as_str()) && entry.weight > 0.0
            })
            .count();
        if positive_count >= 1 {
            return;
        }
        let mut candidates = self
            .runtime
            .values()
            .filter(|entry| selectable_keys.contains(entry.proxy_key.as_str()))
            .map(|entry| (entry.proxy_key.clone(), entry.weight))
            .collect::<Vec<_>>();
        candidates.sort_by(|lhs, rhs| rhs.1.total_cmp(&lhs.1));
        for (proxy_key, _) in candidates {
            if let Some(entry) = self.runtime.get_mut(&proxy_key)
                && entry.weight <= 0.0
            {
                entry.weight = FORWARD_PROXY_PROBE_RECOVERY_WEIGHT;
                positive_count += 1;
            }
            if positive_count >= 1 {
                break;
            }
        }
    }

    fn selectable_endpoint_keys(&self) -> HashSet<&str> {
        self.endpoints
            .iter()
            .filter(|endpoint| endpoint.is_selectable())
            .map(|endpoint| endpoint.key.as_str())
            .collect::<HashSet<_>>()
    }

    pub fn snapshot_runtime(&self) -> Vec<ForwardProxyRuntimeState> {
        self.endpoints
            .iter()
            .filter_map(|endpoint| self.runtime.get(&endpoint.key).cloned())
            .collect()
    }

    pub fn endpoint_by_key(&self, key: &str) -> Option<ForwardProxyEndpoint> {
        self.endpoints
            .iter()
            .find(|endpoint| endpoint.key == key)
            .cloned()
    }

    pub fn endpoint(&self, key: &str) -> Option<&ForwardProxyEndpoint> {
        self.endpoints.iter().find(|endpoint| endpoint.key == key)
    }

    pub fn runtime(&self, key: &str) -> Option<&ForwardProxyRuntimeState> {
        self.runtime.get(key)
    }

    pub fn select_proxy(&mut self) -> SelectedForwardProxy {
        self.selection_counter = self.selection_counter.wrapping_add(1);
        self.note_request();
        self.ensure_non_zero_weight();

        let mut candidates = Vec::new();
        let mut total_weight = 0.0f64;
        for endpoint in &self.endpoints {
            if !endpoint.is_selectable() {
                continue;
            }
            if let Some(runtime) = self.runtime.get(&endpoint.key)
                && runtime.weight > 0.0
                && runtime.weight.is_finite()
            {
                total_weight += runtime.weight;
                candidates.push((endpoint, runtime.weight));
            }
        }

        if candidates.is_empty() {
            let fallback = self
                .endpoints
                .iter()
                .find(|endpoint| endpoint.protocol == ForwardProxyProtocol::Direct)
                .cloned()
                .or_else(|| {
                    self.endpoints
                        .iter()
                        .find(|endpoint| endpoint.is_selectable())
                        .cloned()
                })
                .or_else(|| self.endpoints.first().cloned())
                .unwrap_or_else(ForwardProxyEndpoint::direct);
            return SelectedForwardProxy::from_endpoint(&fallback);
        }

        let random = deterministic_unit_f64(self.selection_counter);
        let mut threshold = random * total_weight;
        let mut last_candidate = candidates[0].0;
        for (endpoint, weight) in candidates {
            last_candidate = endpoint;
            if threshold <= weight {
                return SelectedForwardProxy::from_endpoint(endpoint);
            }
            threshold -= weight;
        }
        SelectedForwardProxy::from_endpoint(last_candidate)
    }

    pub fn note_request(&mut self) {
        self.requests_since_probe = self.requests_since_probe.saturating_add(1);
    }

    pub fn record_attempt(
        &mut self,
        proxy_key: &str,
        success: bool,
        latency_ms: Option<f64>,
        failure_kind: Option<&str>,
    ) {
        if !self
            .endpoints
            .iter()
            .any(|endpoint| endpoint.key == proxy_key)
        {
            return;
        }
        let Some(runtime) = self.runtime.get_mut(proxy_key) else {
            return;
        };
        update_runtime_ema(runtime, success, latency_ms);
        if success {
            runtime.consecutive_failures = 0;
            runtime.available = true;
            runtime.last_error = None;
            let latency_penalty = runtime
                .latency_ema_ms
                .map(|value| (value / 2500.0).min(0.6))
                .unwrap_or(0.0);
            runtime.weight += FORWARD_PROXY_WEIGHT_SUCCESS_BONUS - latency_penalty;
            if runtime.weight <= 0.0 {
                runtime.weight = FORWARD_PROXY_PROBE_RECOVERY_WEIGHT;
            }
        } else {
            runtime.consecutive_failures = runtime.consecutive_failures.saturating_add(1);
            runtime.available = false;
            runtime.last_error = failure_kind.map(ToOwned::to_owned);
            let failure_penalty = FORWARD_PROXY_WEIGHT_FAILURE_PENALTY_BASE
                + f64::from(runtime.consecutive_failures.saturating_sub(1))
                    * FORWARD_PROXY_WEIGHT_FAILURE_PENALTY_STEP;
            runtime.weight -= failure_penalty;
        }
        runtime.weight = runtime
            .weight
            .clamp(FORWARD_PROXY_WEIGHT_MIN, FORWARD_PROXY_WEIGHT_MAX);
        if success && runtime.weight < FORWARD_PROXY_WEIGHT_RECOVERY {
            runtime.weight = runtime.weight.max(FORWARD_PROXY_WEIGHT_RECOVERY * 0.5);
        }
        self.ensure_non_zero_weight();
    }

    pub fn should_refresh_subscriptions(&self) -> bool {
        if self.settings.subscription_urls.is_empty() {
            return false;
        }
        let Some(last_refresh_at) = self.last_subscription_refresh_at else {
            return true;
        };
        let interval =
            i64::try_from(self.settings.subscription_update_interval_secs).unwrap_or(i64::MAX);
        (Utc::now().timestamp() - last_refresh_at) >= interval
    }

    pub fn should_probe_penalized_proxy(&self) -> bool {
        let selectable_keys = self.selectable_endpoint_keys();
        let has_penalized = self.runtime.values().any(|entry| {
            selectable_keys.contains(entry.proxy_key.as_str()) && entry.is_penalized()
        });
        if !has_penalized || self.probe_in_flight {
            return false;
        }
        self.requests_since_probe >= FORWARD_PROXY_PROBE_EVERY_REQUESTS
            || (Utc::now().timestamp() - self.last_probe_at) >= FORWARD_PROXY_PROBE_INTERVAL_SECS
    }

    pub fn mark_probe_started(&mut self) -> Option<SelectedForwardProxy> {
        if !self.should_probe_penalized_proxy() {
            return None;
        }
        let selectable_keys = self.selectable_endpoint_keys();
        let selected = self
            .runtime
            .values()
            .filter(|entry| {
                entry.is_penalized() && selectable_keys.contains(entry.proxy_key.as_str())
            })
            .max_by(|lhs, rhs| lhs.weight.total_cmp(&rhs.weight))
            .and_then(|entry| {
                self.endpoints
                    .iter()
                    .find(|item| item.key == entry.proxy_key)
            })
            .cloned()?;
        self.probe_in_flight = true;
        self.requests_since_probe = 0;
        self.last_probe_at = Utc::now().timestamp();
        Some(SelectedForwardProxy::from_endpoint(&selected))
    }

    pub fn mark_probe_finished(&mut self) {
        self.probe_in_flight = false;
        self.last_probe_at = Utc::now().timestamp();
    }

    pub fn rank_candidates_for_subject(
        &self,
        subject: &str,
        exclude: &HashSet<String>,
        allow_direct: bool,
        limit: usize,
    ) -> Vec<ForwardProxyEndpoint> {
        let seed = stable_hash_u64(subject);
        let mut candidates = self
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.is_selectable())
            .filter(|endpoint| allow_direct || !endpoint.is_direct())
            .filter(|endpoint| !exclude.contains(&endpoint.key))
            .filter_map(|endpoint| {
                let runtime = self.runtime.get(&endpoint.key)?;
                if !runtime.available || !runtime.weight.is_finite() {
                    return None;
                }
                let score = runtime.weight + runtime.success_ema * 4.0
                    - runtime
                        .latency_ema_ms
                        .map(|latency| (latency / 1000.0).min(1.5))
                        .unwrap_or(0.0)
                    - if endpoint.is_direct() { 50.0 } else { 0.0 }
                    + deterministic_unit_f64(seed ^ stable_hash_u64(&endpoint.key)) * 0.05;
                Some((score, endpoint.clone()))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|lhs, rhs| rhs.0.total_cmp(&lhs.0));
        candidates
            .into_iter()
            .take(limit.max(1))
            .map(|(_, endpoint)| endpoint)
            .collect()
    }
}

fn update_runtime_ema(
    runtime: &mut ForwardProxyRuntimeState,
    success: bool,
    latency_ms: Option<f64>,
) {
    runtime.success_ema = runtime.success_ema * 0.9 + if success { 0.1 } else { 0.0 };
    if let Some(latency_ms) = latency_ms.filter(|value| value.is_finite() && *value >= 0.0) {
        runtime.latency_ema_ms = Some(match runtime.latency_ema_ms {
            Some(previous) => previous * 0.8 + latency_ms * 0.2,
            None => latency_ms,
        });
    }
}

#[derive(Debug, Clone)]
pub struct SelectedForwardProxy {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub kind: String,
    pub endpoint_url: Option<Url>,
    pub endpoint_url_raw: Option<String>,
}

impl SelectedForwardProxy {
    pub fn from_endpoint(endpoint: &ForwardProxyEndpoint) -> Self {
        Self {
            key: endpoint.key.clone(),
            source: endpoint.source.clone(),
            display_name: endpoint.display_name.clone(),
            kind: endpoint.protocol.as_str().to_string(),
            endpoint_url: endpoint.endpoint_url.clone(),
            endpoint_url_raw: endpoint.raw_url.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ForwardProxyClientPool {
    direct_client: Client,
    clients: Arc<RwLock<HashMap<String, Client>>>,
}

impl ForwardProxyClientPool {
    pub fn new() -> Result<Self, ProxyError> {
        let direct_client = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(ProxyError::Http)?;
        Ok(Self {
            direct_client,
            clients: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn direct_client(&self) -> Client {
        self.direct_client.clone()
    }

    pub async fn client_for(&self, endpoint_url: Option<&Url>) -> Result<Client, ProxyError> {
        let Some(endpoint_url) = endpoint_url else {
            return Ok(self.direct_client());
        };
        let key = endpoint_url.as_str().to_string();
        if let Some(client) = self.clients.read().await.get(&key).cloned() {
            return Ok(client);
        }
        let built = Client::builder()
            .pool_idle_timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .proxy(Proxy::all(endpoint_url.as_str()).map_err(|err| {
                ProxyError::Other(format!(
                    "invalid forward proxy endpoint {endpoint_url}: {err}"
                ))
            })?)
            .build()
            .map_err(ProxyError::Http)?;
        self.clients.write().await.insert(key, built.clone());
        Ok(built)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ForwardProxyRuntimeConfig {
    pub xray_binary: &'static str,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyAssignmentCounts {
    pub primary: i64,
    pub secondary: i64,
}

#[derive(Debug, Clone)]
struct ForwardProxyKeyAffinity {
    primary_proxy_key: Option<String>,
    secondary_proxy_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ForwardProxyAttemptWindowStats {
    pub attempts: i64,
    pub success_count: i64,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyWindowStatsResponse {
    pub attempts: i64,
    pub success_rate: Option<f64>,
    pub avg_latency_ms: Option<f64>,
}

impl From<ForwardProxyAttemptWindowStats> for ForwardProxyWindowStatsResponse {
    fn from(value: ForwardProxyAttemptWindowStats) -> Self {
        let success_rate = if value.attempts > 0 {
            Some((value.success_count as f64) / (value.attempts as f64))
        } else {
            None
        };
        Self {
            attempts: value.attempts,
            success_rate,
            avg_latency_ms: value.avg_latency_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyStatsResponse {
    pub one_minute: ForwardProxyWindowStatsResponse,
    pub fifteen_minutes: ForwardProxyWindowStatsResponse,
    pub one_hour: ForwardProxyWindowStatsResponse,
    pub one_day: ForwardProxyWindowStatsResponse,
    pub seven_days: ForwardProxyWindowStatsResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyNodeResponse {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub endpoint_url: Option<String>,
    pub resolved_ips: Vec<String>,
    pub resolved_regions: Vec<String>,
    pub weight: f64,
    pub available: bool,
    pub last_error: Option<String>,
    pub penalized: bool,
    pub primary_assignment_count: i64,
    pub secondary_assignment_count: i64,
    pub stats: ForwardProxyStatsResponse,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxySettingsResponse {
    pub proxy_urls: Vec<String>,
    pub subscription_urls: Vec<String>,
    pub subscription_update_interval_secs: u64,
    pub insert_direct: bool,
    pub nodes: Vec<ForwardProxyNodeResponse>,
}

#[derive(Debug, Clone, Default)]
struct ForwardProxyHourlyStatsPoint {
    success_count: i64,
    failure_count: i64,
}

#[derive(Debug, Clone)]
struct ForwardProxyWeightHourlyStatsPoint {
    sample_count: i64,
    min_weight: f64,
    max_weight: f64,
    avg_weight: f64,
    last_weight: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyHourlyBucketResponse {
    pub bucket_start: String,
    pub bucket_end: String,
    pub success_count: i64,
    pub failure_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyWeightHourlyBucketResponse {
    pub bucket_start: String,
    pub bucket_end: String,
    pub sample_count: i64,
    pub min_weight: f64,
    pub max_weight: f64,
    pub avg_weight: f64,
    pub last_weight: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyLiveNodeResponse {
    pub key: String,
    pub source: String,
    pub display_name: String,
    pub endpoint_url: Option<String>,
    pub resolved_ips: Vec<String>,
    pub resolved_regions: Vec<String>,
    pub weight: f64,
    pub available: bool,
    pub last_error: Option<String>,
    pub penalized: bool,
    pub primary_assignment_count: i64,
    pub secondary_assignment_count: i64,
    pub stats: ForwardProxyStatsResponse,
    pub last24h: Vec<ForwardProxyHourlyBucketResponse>,
    pub weight24h: Vec<ForwardProxyWeightHourlyBucketResponse>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardProxyLiveStatsResponse {
    pub range_start: String,
    pub range_end: String,
    pub bucket_seconds: i64,
    pub nodes: Vec<ForwardProxyLiveNodeResponse>,
}

#[derive(Debug)]
struct XrayInstance {
    local_proxy_url: Url,
    config_path: PathBuf,
    child: Child,
}

#[derive(Debug, Default)]
pub struct XraySupervisor {
    pub binary: String,
    pub runtime_dir: PathBuf,
    instances: HashMap<String, XrayInstance>,
}

impl XraySupervisor {
    pub fn new(binary: String, runtime_dir: PathBuf) -> Self {
        Self {
            binary,
            runtime_dir,
            instances: HashMap::new(),
        }
    }

    pub async fn sync_endpoints(
        &mut self,
        endpoints: &mut [ForwardProxyEndpoint],
    ) -> Result<(), ProxyError> {
        let _ = fs::create_dir_all(&self.runtime_dir);
        let desired_keys = endpoints
            .iter()
            .filter(|endpoint| endpoint.requires_xray())
            .map(|endpoint| endpoint.key.clone())
            .collect::<HashSet<_>>();
        let stale_keys = self
            .instances
            .keys()
            .filter(|key| !desired_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();

        for endpoint in endpoints {
            if !endpoint.requires_xray() {
                continue;
            }
            match self.ensure_instance(endpoint).await {
                Ok(route_url) => endpoint.endpoint_url = Some(route_url),
                Err(_) => endpoint.endpoint_url = None,
            }
        }

        for key in stale_keys {
            self.remove_instance(&key).await;
        }
        Ok(())
    }

    pub async fn shutdown_all(&mut self) {
        let keys = self.instances.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            self.remove_instance(&key).await;
        }
    }

    pub async fn ensure_instance(
        &mut self,
        endpoint: &ForwardProxyEndpoint,
    ) -> Result<Url, ProxyError> {
        if let Some(instance) = self.instances.get_mut(&endpoint.key) {
            match instance.child.try_wait() {
                Ok(None) => return Ok(instance.local_proxy_url.clone()),
                Ok(Some(_)) => {}
                Err(_) => {}
            }
        }
        self.remove_instance(&endpoint.key).await;
        self.spawn_instance(endpoint).await
    }

    async fn spawn_instance(&mut self, endpoint: &ForwardProxyEndpoint) -> Result<Url, ProxyError> {
        let outbound = build_xray_outbound_for_endpoint(endpoint)?;
        let local_port = pick_unused_local_port()?;
        let _ = fs::create_dir_all(&self.runtime_dir);
        let config_path = self.runtime_dir.join(format!(
            "forward-proxy-{:016x}.json",
            stable_hash_u64(&endpoint.key)
        ));
        let config = build_xray_instance_config(local_port, outbound);
        let serialized = serde_json::to_vec_pretty(&config)
            .map_err(|err| ProxyError::Other(format!("failed to serialize xray config: {err}")))?;
        fs::write(&config_path, serialized).map_err(|err| {
            ProxyError::Other(format!(
                "failed to write xray config {}: {err}",
                config_path.display()
            ))
        })?;

        let mut child = Command::new(&self.binary)
            .arg("run")
            .arg("-c")
            .arg(&config_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                ProxyError::Other(format!(
                    "failed to start xray binary {}: {err}",
                    self.binary
                ))
            })?;

        if let Err(err) = wait_for_xray_proxy_ready(
            &mut child,
            local_port,
            Duration::from_millis(XRAY_PROXY_READY_TIMEOUT_MS),
        )
        .await
        {
            let _ = terminate_child_process(&mut child, Duration::from_secs(2)).await;
            let stderr_tail = child.wait_with_output().await.ok().and_then(|output| {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if stderr.is_empty() {
                    None
                } else {
                    Some(
                        stderr
                            .lines()
                            .rev()
                            .take(3)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<Vec<_>>()
                            .join(" | "),
                    )
                }
            });
            let _ = fs::remove_file(&config_path);
            return Err(if let Some(stderr_tail) = stderr_tail {
                ProxyError::Other(format!("{err} ({stderr_tail})"))
            } else {
                err
            });
        }

        let local_proxy_url =
            Url::parse(&format!("socks5h://127.0.0.1:{local_port}")).map_err(|err| {
                ProxyError::Other(format!("failed to build local xray socks endpoint: {err}"))
            })?;
        self.instances.insert(
            endpoint.key.clone(),
            XrayInstance {
                local_proxy_url: local_proxy_url.clone(),
                config_path,
                child,
            },
        );
        Ok(local_proxy_url)
    }

    pub async fn remove_instance(&mut self, key: &str) {
        if let Some(mut instance) = self.instances.remove(key) {
            let _ = terminate_child_process(&mut instance.child, Duration::from_secs(2)).await;
            let _ = fs::remove_file(&instance.config_path);
        }
    }
}

pub async fn ensure_forward_proxy_schema(pool: &SqlitePool) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_settings (
            id INTEGER PRIMARY KEY,
            proxy_urls_json TEXT NOT NULL DEFAULT '[]',
            subscription_urls_json TEXT NOT NULL DEFAULT '[]',
            subscription_update_interval_secs INTEGER NOT NULL DEFAULT 3600,
            insert_direct INTEGER NOT NULL DEFAULT 1,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_runtime (
            proxy_key TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            source TEXT NOT NULL,
            endpoint_url TEXT,
            resolved_ip_source TEXT NOT NULL DEFAULT '',
            resolved_ips_json TEXT NOT NULL DEFAULT '[]',
            resolved_regions_json TEXT NOT NULL DEFAULT '[]',
            weight REAL NOT NULL,
            success_ema REAL NOT NULL,
            latency_ema_ms REAL,
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            is_penalized INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    ensure_forward_proxy_runtime_column(pool, "resolved_ips_json", "TEXT NOT NULL DEFAULT '[]'")
        .await?;
    ensure_forward_proxy_runtime_column(
        pool,
        "resolved_regions_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )
    .await?;
    ensure_forward_proxy_runtime_column(pool, "resolved_ip_source", "TEXT NOT NULL DEFAULT ''")
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_attempts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            proxy_key TEXT NOT NULL,
            is_success INTEGER NOT NULL,
            latency_ms REAL,
            failure_kind TEXT,
            is_probe INTEGER NOT NULL DEFAULT 0,
            occurred_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_weight_hourly (
            proxy_key TEXT NOT NULL,
            bucket_start_epoch INTEGER NOT NULL,
            sample_count INTEGER NOT NULL,
            min_weight REAL NOT NULL,
            max_weight REAL NOT NULL,
            avg_weight REAL NOT NULL,
            last_weight REAL NOT NULL,
            last_sample_epoch_us INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
            PRIMARY KEY (proxy_key, bucket_start_epoch)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS forward_proxy_key_affinity (
            key_id TEXT PRIMARY KEY,
            primary_proxy_key TEXT,
            secondary_proxy_key TEXT,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_forward_proxy_attempts_proxy_time
           ON forward_proxy_attempts (proxy_key, occurred_at)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_forward_proxy_attempts_time_proxy
           ON forward_proxy_attempts (occurred_at, proxy_key)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_forward_proxy_weight_hourly_time_proxy
           ON forward_proxy_weight_hourly (bucket_start_epoch, proxy_key)"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO forward_proxy_settings (
            id,
            proxy_urls_json,
            subscription_urls_json,
            subscription_update_interval_secs,
            insert_direct,
            updated_at
        ) VALUES (?1, '[]', '[]', ?2, ?3, strftime('%s', 'now'))
        "#,
    )
    .bind(FORWARD_PROXY_SETTINGS_SINGLETON_ID)
    .bind(DEFAULT_FORWARD_PROXY_SUBSCRIPTION_INTERVAL_SECS as i64)
    .bind(DEFAULT_FORWARD_PROXY_INSERT_DIRECT as i64)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn load_forward_proxy_settings(
    pool: &SqlitePool,
) -> Result<ForwardProxySettings, ProxyError> {
    let row = sqlx::query_as::<_, ForwardProxySettingsRow>(
        r#"
        SELECT proxy_urls_json, subscription_urls_json, subscription_update_interval_secs, insert_direct
        FROM forward_proxy_settings
        WHERE id = ?1
        LIMIT 1
        "#,
    )
    .bind(FORWARD_PROXY_SETTINGS_SINGLETON_ID)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Into::into).unwrap_or_default())
}

pub async fn save_forward_proxy_settings(
    pool: &SqlitePool,
    settings: ForwardProxySettings,
) -> Result<(), ProxyError> {
    let normalized = settings.normalized();
    let proxy_urls_json = serde_json::to_string(&normalized.proxy_urls).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy urls: {err}"))
    })?;
    let subscription_urls_json =
        serde_json::to_string(&normalized.subscription_urls).map_err(|err| {
            ProxyError::Other(format!(
                "failed to serialize forward proxy subscription urls: {err}"
            ))
        })?;
    sqlx::query(
        r#"
        UPDATE forward_proxy_settings
        SET proxy_urls_json = ?1,
            subscription_urls_json = ?2,
            subscription_update_interval_secs = ?3,
            insert_direct = ?4,
            updated_at = strftime('%s', 'now')
        WHERE id = ?5
        "#,
    )
    .bind(proxy_urls_json)
    .bind(subscription_urls_json)
    .bind(normalized.subscription_update_interval_secs as i64)
    .bind(normalized.insert_direct as i64)
    .bind(FORWARD_PROXY_SETTINGS_SINGLETON_ID)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_forward_proxy_runtime_states(
    pool: &SqlitePool,
) -> Result<Vec<ForwardProxyRuntimeState>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyRuntimeRow>(
        r#"
        SELECT
            proxy_key,
            display_name,
            source,
            endpoint_url,
            resolved_ip_source,
            resolved_ips_json,
            resolved_regions_json,
            weight,
            success_ema,
            latency_ema_ms,
            consecutive_failures
        FROM forward_proxy_runtime
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub async fn persist_forward_proxy_runtime_snapshot(
    pool: &SqlitePool,
    runtime_snapshot: Vec<ForwardProxyRuntimeState>,
) -> Result<(), ProxyError> {
    let active_keys = runtime_snapshot
        .iter()
        .map(|entry| entry.proxy_key.clone())
        .collect::<Vec<_>>();
    delete_forward_proxy_runtime_rows_not_in(pool, &active_keys).await?;
    for runtime in &runtime_snapshot {
        persist_forward_proxy_runtime_state(pool, runtime).await?;
    }
    Ok(())
}

pub async fn persist_forward_proxy_runtime_state(
    pool: &SqlitePool,
    state: &ForwardProxyRuntimeState,
) -> Result<(), ProxyError> {
    let resolved_ips_json = serde_json::to_string(&state.resolved_ips).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy ips: {err}"))
    })?;
    let resolved_regions_json = serde_json::to_string(&state.resolved_regions).map_err(|err| {
        ProxyError::Other(format!("failed to serialize forward proxy regions: {err}"))
    })?;
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_runtime (
            proxy_key,
            display_name,
            source,
            endpoint_url,
            resolved_ip_source,
            resolved_ips_json,
            resolved_regions_json,
            weight,
            success_ema,
            latency_ema_ms,
            consecutive_failures,
            is_penalized,
            updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, strftime('%s', 'now'))
        ON CONFLICT(proxy_key) DO UPDATE SET
            display_name = excluded.display_name,
            source = excluded.source,
            endpoint_url = excluded.endpoint_url,
            resolved_ip_source = excluded.resolved_ip_source,
            resolved_ips_json = excluded.resolved_ips_json,
            resolved_regions_json = excluded.resolved_regions_json,
            weight = excluded.weight,
            success_ema = excluded.success_ema,
            latency_ema_ms = excluded.latency_ema_ms,
            consecutive_failures = excluded.consecutive_failures,
            is_penalized = excluded.is_penalized,
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(&state.proxy_key)
    .bind(&state.display_name)
    .bind(&state.source)
    .bind(&state.endpoint_url)
    .bind(&state.resolved_ip_source)
    .bind(resolved_ips_json)
    .bind(resolved_regions_json)
    .bind(state.weight)
    .bind(state.success_ema)
    .bind(state.latency_ema_ms)
    .bind(i64::from(state.consecutive_failures))
    .bind(state.is_penalized() as i64)
    .execute(pool)
    .await?;
    Ok(())
}

async fn ensure_forward_proxy_runtime_column(
    pool: &SqlitePool,
    column_name: &str,
    column_def: &str,
) -> Result<(), ProxyError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pragma_table_info('forward_proxy_runtime') WHERE name = ?1",
    )
    .bind(column_name)
    .fetch_one(pool)
    .await?;
    if exists == 0 {
        sqlx::query(&format!(
            "ALTER TABLE forward_proxy_runtime ADD COLUMN {column_name} {column_def}"
        ))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn delete_forward_proxy_runtime_rows_not_in(
    pool: &SqlitePool,
    active_keys: &[String],
) -> Result<(), ProxyError> {
    if active_keys.is_empty() {
        sqlx::query("DELETE FROM forward_proxy_runtime")
            .execute(pool)
            .await?;
        return Ok(());
    }
    let mut builder =
        QueryBuilder::<Sqlite>::new("DELETE FROM forward_proxy_runtime WHERE proxy_key NOT IN (");
    {
        let mut separated = builder.separated(", ");
        for key in active_keys {
            separated.push_bind(key);
        }
    }
    builder.push(")");
    builder.build().execute(pool).await?;
    Ok(())
}

pub async fn insert_forward_proxy_attempt(
    pool: &SqlitePool,
    proxy_key: &str,
    success: bool,
    latency_ms: Option<f64>,
    failure_kind: Option<&str>,
    is_probe: bool,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_attempts (proxy_key, is_success, latency_ms, failure_kind, is_probe, occurred_at)
        VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s', 'now'))
        "#,
    )
    .bind(proxy_key)
    .bind(success as i64)
    .bind(latency_ms)
    .bind(failure_kind)
    .bind(is_probe as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_forward_proxy_weight_hourly_bucket(
    pool: &SqlitePool,
    proxy_key: &str,
    bucket_start_epoch: i64,
    weight: f64,
    sample_epoch_us: i64,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_weight_hourly (
            proxy_key,
            bucket_start_epoch,
            sample_count,
            min_weight,
            max_weight,
            avg_weight,
            last_weight,
            last_sample_epoch_us,
            updated_at
        ) VALUES (?1, ?2, 1, ?3, ?3, ?3, ?3, ?4, strftime('%s', 'now'))
        ON CONFLICT(proxy_key, bucket_start_epoch) DO UPDATE SET
            sample_count = forward_proxy_weight_hourly.sample_count + 1,
            min_weight = MIN(forward_proxy_weight_hourly.min_weight, excluded.min_weight),
            max_weight = MAX(forward_proxy_weight_hourly.max_weight, excluded.max_weight),
            avg_weight = ((forward_proxy_weight_hourly.avg_weight * forward_proxy_weight_hourly.sample_count) + excluded.avg_weight)
                / (forward_proxy_weight_hourly.sample_count + 1),
            last_weight = CASE WHEN excluded.last_sample_epoch_us >= forward_proxy_weight_hourly.last_sample_epoch_us
                THEN excluded.last_weight ELSE forward_proxy_weight_hourly.last_weight END,
            last_sample_epoch_us = MAX(forward_proxy_weight_hourly.last_sample_epoch_us, excluded.last_sample_epoch_us),
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(proxy_key)
    .bind(bucket_start_epoch)
    .bind(weight)
    .bind(sample_epoch_us)
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_forward_proxy_affinity(
    pool: &SqlitePool,
    key_id: &str,
) -> Result<Option<ForwardProxyKeyAffinity>, ProxyError> {
    let row = sqlx::query(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ? LIMIT 1",
    )
    .bind(key_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| ForwardProxyKeyAffinity {
        primary_proxy_key: row
            .try_get("primary_proxy_key")
            .ok()
            .filter(|value: &String| !value.trim().is_empty()),
        secondary_proxy_key: row
            .try_get("secondary_proxy_key")
            .ok()
            .filter(|value: &String| !value.trim().is_empty()),
    }))
}

async fn save_forward_proxy_affinity(
    pool: &SqlitePool,
    key_id: &str,
    affinity: &ForwardProxyKeyAffinity,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO forward_proxy_key_affinity (key_id, primary_proxy_key, secondary_proxy_key, updated_at)
        VALUES (?1, ?2, ?3, strftime('%s', 'now'))
        ON CONFLICT(key_id) DO UPDATE SET
            primary_proxy_key = excluded.primary_proxy_key,
            secondary_proxy_key = excluded.secondary_proxy_key,
            updated_at = strftime('%s', 'now')
        "#,
    )
    .bind(key_id)
    .bind(&affinity.primary_proxy_key)
    .bind(&affinity.secondary_proxy_key)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_forward_proxy_key_affinity(
    pool: &SqlitePool,
    key_id: &str,
) -> Result<Option<ForwardProxyAffinityRecord>, ProxyError> {
    Ok(load_forward_proxy_affinity(pool, key_id)
        .await?
        .map(|record| ForwardProxyAffinityRecord {
            primary_proxy_key: record.primary_proxy_key,
            secondary_proxy_key: record.secondary_proxy_key,
            updated_at: Utc::now().timestamp(),
        }))
}

pub async fn save_forward_proxy_key_affinity(
    pool: &SqlitePool,
    key_id: &str,
    record: &ForwardProxyAffinityRecord,
) -> Result<(), ProxyError> {
    save_forward_proxy_affinity(
        pool,
        key_id,
        &ForwardProxyKeyAffinity {
            primary_proxy_key: record.primary_proxy_key.clone(),
            secondary_proxy_key: record.secondary_proxy_key.clone(),
        },
    )
    .await
}

pub async fn sync_manager_runtime_to_store(
    key_store: &KeyStore,
    manager: &ForwardProxyManager,
) -> Result<(), ProxyError> {
    persist_forward_proxy_runtime_snapshot(&key_store.pool, manager.snapshot_runtime()).await
}

async fn load_forward_proxy_assignment_counts(
    pool: &SqlitePool,
) -> Result<HashMap<String, ForwardProxyAssignmentCounts>, ProxyError> {
    let rows = sqlx::query(
        r#"
        SELECT
            proxy_key,
            SUM(primary_count) AS primary_count,
            SUM(secondary_count) AS secondary_count
        FROM (
            SELECT primary_proxy_key AS proxy_key, 1 AS primary_count, 0 AS secondary_count
            FROM forward_proxy_key_affinity
            WHERE primary_proxy_key IS NOT NULL AND primary_proxy_key != ''
            UNION ALL
            SELECT secondary_proxy_key AS proxy_key, 0 AS primary_count, 1 AS secondary_count
            FROM forward_proxy_key_affinity
            WHERE secondary_proxy_key IS NOT NULL AND secondary_proxy_key != ''
        )
        GROUP BY proxy_key
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut counts = HashMap::new();
    for row in rows {
        let proxy_key: String = row.try_get("proxy_key")?;
        let primary: i64 = row.try_get::<i64, _>("primary_count")?;
        let secondary: i64 = row.try_get::<i64, _>("secondary_count")?;
        counts.insert(
            proxy_key,
            ForwardProxyAssignmentCounts { primary, secondary },
        );
    }
    Ok(counts)
}

#[derive(Debug, FromRow)]
struct ForwardProxyAttemptStatsRow {
    proxy_key: String,
    attempts: i64,
    success_count: i64,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, FromRow)]
struct ForwardProxyHourlyStatsRow {
    proxy_key: String,
    bucket_start_epoch: i64,
    success_count: i64,
    failure_count: i64,
}

#[derive(Debug, FromRow)]
struct ForwardProxyWeightHourlyStatsRow {
    proxy_key: String,
    bucket_start_epoch: i64,
    sample_count: i64,
    min_weight: f64,
    max_weight: f64,
    avg_weight: f64,
    last_weight: f64,
}

#[derive(Debug, FromRow)]
struct ForwardProxyWeightLastBeforeRangeRow {
    proxy_key: String,
    last_weight: f64,
}

async fn query_forward_proxy_window_stats(
    pool: &SqlitePool,
    since_epoch: i64,
) -> Result<HashMap<String, ForwardProxyAttemptWindowStats>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyAttemptStatsRow>(
        r#"
        SELECT proxy_key,
               COUNT(*) AS attempts,
               SUM(CASE WHEN is_success != 0 THEN 1 ELSE 0 END) AS success_count,
               AVG(CASE WHEN is_success != 0 THEN latency_ms END) AS avg_latency_ms
        FROM forward_proxy_attempts
        WHERE occurred_at >= ?1
        GROUP BY proxy_key
        "#,
    )
    .bind(since_epoch)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.proxy_key,
                ForwardProxyAttemptWindowStats {
                    attempts: row.attempts,
                    success_count: row.success_count,
                    avg_latency_ms: row.avg_latency_ms,
                },
            )
        })
        .collect())
}

async fn query_forward_proxy_hourly_stats(
    pool: &SqlitePool,
    range_start_epoch: i64,
    range_end_epoch: i64,
) -> Result<HashMap<String, HashMap<i64, ForwardProxyHourlyStatsPoint>>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyHourlyStatsRow>(
        r#"
        SELECT proxy_key,
               (occurred_at / 3600) * 3600 AS bucket_start_epoch,
               SUM(CASE WHEN is_success != 0 THEN 1 ELSE 0 END) AS success_count,
               SUM(CASE WHEN is_success = 0 THEN 1 ELSE 0 END) AS failure_count
        FROM forward_proxy_attempts
        WHERE occurred_at >= ?1 AND occurred_at < ?2
        GROUP BY proxy_key, bucket_start_epoch
        "#,
    )
    .bind(range_start_epoch)
    .bind(range_end_epoch)
    .fetch_all(pool)
    .await?;

    let mut grouped = HashMap::new();
    for row in rows {
        grouped
            .entry(row.proxy_key)
            .or_insert_with(HashMap::new)
            .insert(
                row.bucket_start_epoch,
                ForwardProxyHourlyStatsPoint {
                    success_count: row.success_count,
                    failure_count: row.failure_count,
                },
            );
    }
    Ok(grouped)
}

async fn query_forward_proxy_weight_hourly_stats(
    pool: &SqlitePool,
    range_start_epoch: i64,
    range_end_epoch: i64,
) -> Result<HashMap<String, HashMap<i64, ForwardProxyWeightHourlyStatsPoint>>, ProxyError> {
    let rows = sqlx::query_as::<_, ForwardProxyWeightHourlyStatsRow>(
        r#"
        SELECT proxy_key, bucket_start_epoch, sample_count, min_weight, max_weight, avg_weight, last_weight
        FROM forward_proxy_weight_hourly
        WHERE bucket_start_epoch >= ?1 AND bucket_start_epoch < ?2
        "#,
    )
    .bind(range_start_epoch)
    .bind(range_end_epoch)
    .fetch_all(pool)
    .await?;
    let mut grouped = HashMap::new();
    for row in rows {
        grouped
            .entry(row.proxy_key)
            .or_insert_with(HashMap::new)
            .insert(
                row.bucket_start_epoch,
                ForwardProxyWeightHourlyStatsPoint {
                    sample_count: row.sample_count,
                    min_weight: row.min_weight,
                    max_weight: row.max_weight,
                    avg_weight: row.avg_weight,
                    last_weight: row.last_weight,
                },
            );
    }
    Ok(grouped)
}

async fn query_forward_proxy_weight_last_before(
    pool: &SqlitePool,
    range_start_epoch: i64,
    proxy_keys: &[String],
) -> Result<HashMap<String, f64>, ProxyError> {
    if proxy_keys.is_empty() {
        return Ok(HashMap::new());
    }
    let mut builder = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT latest.proxy_key, latest.last_weight
        FROM forward_proxy_weight_hourly AS latest
        INNER JOIN (
            SELECT proxy_key, MAX(bucket_start_epoch) AS bucket_start_epoch
            FROM forward_proxy_weight_hourly
            WHERE bucket_start_epoch < "#,
    );
    builder.push_bind(range_start_epoch);
    builder.push(" AND proxy_key IN (");
    {
        let mut separated = builder.separated(", ");
        for key in proxy_keys {
            separated.push_bind(key);
        }
    }
    builder.push(") GROUP BY proxy_key) AS prior ON latest.proxy_key = prior.proxy_key AND latest.bucket_start_epoch = prior.bucket_start_epoch");
    let rows = builder
        .build_query_as::<ForwardProxyWeightLastBeforeRangeRow>()
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.proxy_key, row.last_weight))
        .collect())
}

pub async fn build_forward_proxy_settings_response(
    pool: &SqlitePool,
    manager: &ForwardProxyManager,
) -> Result<ForwardProxySettingsResponse, ProxyError> {
    let settings = manager.settings.clone();
    let runtime_rows = manager.snapshot_runtime();
    let counts = load_forward_proxy_assignment_counts(pool).await?;
    let now = Utc::now().timestamp();
    let windows = [60, 15 * 60, 3600, 24 * 3600, 7 * 24 * 3600];
    let mut window_maps = Vec::new();
    for seconds in windows {
        window_maps.push(query_forward_proxy_window_stats(pool, now - seconds).await?);
    }
    let mut nodes =
        runtime_rows
            .into_iter()
            .map(|runtime| {
                let stats_for = |index: usize| {
                    window_maps[index]
                        .get(&runtime.proxy_key)
                        .cloned()
                        .map(ForwardProxyWindowStatsResponse::from)
                        .unwrap_or_default()
                };
                let assignment = counts.get(&runtime.proxy_key).cloned().unwrap_or(
                    ForwardProxyAssignmentCounts {
                        primary: 0,
                        secondary: 0,
                    },
                );
                ForwardProxyNodeResponse {
                    key: runtime.proxy_key.clone(),
                    source: runtime.source.clone(),
                    display_name: runtime.display_name.clone(),
                    endpoint_url: runtime.endpoint_url.clone(),
                    resolved_ips: runtime.resolved_ips.clone(),
                    resolved_regions: runtime.resolved_regions.clone(),
                    weight: runtime.weight,
                    available: runtime.available,
                    last_error: runtime.last_error.clone(),
                    penalized: runtime.is_penalized(),
                    primary_assignment_count: assignment.primary,
                    secondary_assignment_count: assignment.secondary,
                    stats: ForwardProxyStatsResponse {
                        one_minute: stats_for(0),
                        fifteen_minutes: stats_for(1),
                        one_hour: stats_for(2),
                        one_day: stats_for(3),
                        seven_days: stats_for(4),
                    },
                }
            })
            .collect::<Vec<_>>();
    nodes.sort_by(|lhs, rhs| lhs.display_name.cmp(&rhs.display_name));
    Ok(ForwardProxySettingsResponse {
        proxy_urls: settings.proxy_urls,
        subscription_urls: settings.subscription_urls,
        subscription_update_interval_secs: settings.subscription_update_interval_secs,
        insert_direct: settings.insert_direct,
        nodes,
    })
}

pub async fn build_forward_proxy_live_stats_response(
    pool: &SqlitePool,
    manager: &ForwardProxyManager,
) -> Result<ForwardProxyLiveStatsResponse, ProxyError> {
    const BUCKET_SECONDS: i64 = 3600;
    const BUCKET_COUNT: i64 = 24;
    let runtime_rows = manager.snapshot_runtime();
    let runtime_proxy_keys = runtime_rows
        .iter()
        .map(|runtime| runtime.proxy_key.clone())
        .collect::<Vec<_>>();
    let counts = load_forward_proxy_assignment_counts(pool).await?;
    let now_epoch = Utc::now().timestamp();
    let windows = [60, 15 * 60, 3600, 24 * 3600, 7 * 24 * 3600];
    let mut window_maps = Vec::new();
    for seconds in windows {
        window_maps.push(query_forward_proxy_window_stats(pool, now_epoch - seconds).await?);
    }
    let range_end_epoch = align_bucket_epoch(now_epoch, BUCKET_SECONDS, 0) + BUCKET_SECONDS;
    let range_start_epoch = range_end_epoch - BUCKET_COUNT * BUCKET_SECONDS;
    let hourly_map =
        query_forward_proxy_hourly_stats(pool, range_start_epoch, range_end_epoch).await?;
    let weight_hourly_map =
        query_forward_proxy_weight_hourly_stats(pool, range_start_epoch, range_end_epoch).await?;
    let weight_carry_map =
        query_forward_proxy_weight_last_before(pool, range_start_epoch, &runtime_proxy_keys)
            .await?;

    let mut nodes = Vec::new();
    for runtime in runtime_rows {
        let key = runtime.proxy_key.clone();
        let assignment = counts
            .get(&key)
            .cloned()
            .unwrap_or(ForwardProxyAssignmentCounts {
                primary: 0,
                secondary: 0,
            });
        let stats_key = key.clone();
        let stats_for = |index: usize| {
            window_maps[index]
                .get(&stats_key)
                .cloned()
                .map(ForwardProxyWindowStatsResponse::from)
                .unwrap_or_default()
        };
        let hourly = hourly_map.get(&key);
        let weight_hourly = weight_hourly_map.get(&key);
        let mut carry_weight = weight_carry_map
            .get(&key)
            .copied()
            .unwrap_or(runtime.weight);
        let penalized = runtime.is_penalized();
        let stats = ForwardProxyStatsResponse {
            one_minute: stats_for(0),
            fifteen_minutes: stats_for(1),
            one_hour: stats_for(2),
            one_day: stats_for(3),
            seven_days: stats_for(4),
        };
        let last24h = (0..BUCKET_COUNT)
            .map(|index| {
                let bucket_start_epoch = range_start_epoch + index * BUCKET_SECONDS;
                let bucket_end_epoch = bucket_start_epoch + BUCKET_SECONDS;
                let point = hourly
                    .and_then(|items| items.get(&bucket_start_epoch))
                    .cloned()
                    .unwrap_or_default();
                Ok(ForwardProxyHourlyBucketResponse {
                    bucket_start: format_utc_iso(bucket_start_epoch)?,
                    bucket_end: format_utc_iso(bucket_end_epoch)?,
                    success_count: point.success_count,
                    failure_count: point.failure_count,
                })
            })
            .collect::<Result<Vec<_>, ProxyError>>()?;
        let weight24h = (0..BUCKET_COUNT)
            .map(|index| {
                let bucket_start_epoch = range_start_epoch + index * BUCKET_SECONDS;
                let bucket_end_epoch = bucket_start_epoch + BUCKET_SECONDS;
                let point = weight_hourly.and_then(|items| items.get(&bucket_start_epoch));
                let (sample_count, min_weight, max_weight, avg_weight, last_weight) =
                    if let Some(point) = point {
                        carry_weight = point.last_weight;
                        (
                            point.sample_count,
                            point.min_weight,
                            point.max_weight,
                            point.avg_weight,
                            point.last_weight,
                        )
                    } else {
                        (0, carry_weight, carry_weight, carry_weight, carry_weight)
                    };
                Ok(ForwardProxyWeightHourlyBucketResponse {
                    bucket_start: format_utc_iso(bucket_start_epoch)?,
                    bucket_end: format_utc_iso(bucket_end_epoch)?,
                    sample_count,
                    min_weight,
                    max_weight,
                    avg_weight,
                    last_weight,
                })
            })
            .collect::<Result<Vec<_>, ProxyError>>()?;
        nodes.push(ForwardProxyLiveNodeResponse {
            key,
            source: runtime.source,
            display_name: runtime.display_name,
            endpoint_url: runtime.endpoint_url,
            resolved_ips: runtime.resolved_ips,
            resolved_regions: runtime.resolved_regions,
            weight: runtime.weight,
            available: runtime.available,
            last_error: runtime.last_error,
            penalized,
            primary_assignment_count: assignment.primary,
            secondary_assignment_count: assignment.secondary,
            stats,
            last24h,
            weight24h,
        });
    }
    nodes.sort_by(|lhs, rhs| lhs.display_name.cmp(&rhs.display_name));
    Ok(ForwardProxyLiveStatsResponse {
        range_start: format_utc_iso(range_start_epoch)?,
        range_end: format_utc_iso(range_end_epoch)?,
        bucket_seconds: BUCKET_SECONDS,
        nodes,
    })
}

fn default_forward_proxy_subscription_interval_secs() -> u64 {
    DEFAULT_FORWARD_PROXY_SUBSCRIPTION_INTERVAL_SECS
}

fn default_forward_proxy_insert_direct() -> bool {
    DEFAULT_FORWARD_PROXY_INSERT_DIRECT
}

fn decode_string_vec_json(raw: Option<&str>) -> Vec<String> {
    match raw {
        Some(serialized) => serde_json::from_str::<Vec<String>>(serialized).unwrap_or_default(),
        None => Vec::new(),
    }
}

pub fn normalize_subscription_entries(raw_entries: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for entry in raw_entries {
        for token in split_proxy_entry_tokens(&entry) {
            let Ok(url) = Url::parse(token) else {
                continue;
            };
            if !matches!(url.scheme(), "http" | "https") {
                continue;
            }
            let canonical = url.to_string();
            if seen.insert(canonical.clone()) {
                normalized.push(canonical);
            }
        }
    }
    normalized
}

pub fn normalize_proxy_url_entries(raw_entries: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for entry in raw_entries {
        for token in split_proxy_entry_tokens(&entry) {
            if let Some(parsed) = parse_forward_proxy_entry(token)
                && seen.insert(parsed.normalized.clone())
            {
                normalized.push(parsed.normalized);
            }
        }
    }
    normalized
}

fn split_proxy_entry_tokens(raw: &str) -> Vec<&str> {
    raw.split(['\n', ',', ';'])
        .map(str::trim)
        .filter(|token| !token.is_empty() && !token.starts_with('#'))
        .collect()
}

pub fn normalize_proxy_endpoints_from_urls(urls: &[String]) -> Vec<ForwardProxyEndpoint> {
    let mut seen = HashSet::new();
    let mut endpoints = Vec::new();
    for raw in urls {
        if let Some(parsed) = parse_forward_proxy_entry(raw) {
            let key = parsed.normalized.clone();
            if !seen.insert(key.clone()) {
                continue;
            }
            endpoints.push(ForwardProxyEndpoint::new_manual(
                key,
                parsed.display_name,
                parsed.protocol,
                parsed.endpoint_url,
                Some(parsed.normalized),
            ));
        }
    }
    endpoints
}

pub fn normalize_subscription_endpoints_from_urls(
    urls: &[String],
    subscription_source: &str,
) -> Vec<ForwardProxyEndpoint> {
    let mut seen = HashSet::new();
    let mut endpoints = Vec::new();
    for raw in urls {
        if let Some(parsed) = parse_forward_proxy_entry(raw) {
            let key = parsed.normalized.clone();
            if !seen.insert(key.clone()) {
                continue;
            }
            endpoints.push(ForwardProxyEndpoint::new_subscription(
                key,
                parsed.display_name,
                parsed.protocol,
                parsed.endpoint_url,
                Some(parsed.normalized),
                subscription_source.to_string(),
            ));
        }
    }
    endpoints
}

#[derive(Debug, Clone)]
pub struct ParsedForwardProxyEntry {
    pub normalized: String,
    pub display_name: String,
    pub protocol: ForwardProxyProtocol,
    pub endpoint_url: Option<Url>,
}

pub fn parse_forward_proxy_entry(raw: &str) -> Option<ParsedForwardProxyEntry> {
    let candidate = raw.trim();
    if candidate.is_empty() {
        return None;
    }
    if !candidate.contains("://") {
        return parse_native_forward_proxy(&format!("http://{candidate}"));
    }
    let (scheme_raw, _) = candidate.split_once("://")?;
    let scheme = scheme_raw.to_ascii_lowercase();
    match scheme.as_str() {
        "http" | "https" | "socks5" | "socks5h" | "socks" => parse_native_forward_proxy(candidate),
        "vmess" => parse_vmess_forward_proxy(candidate),
        "vless" => parse_vless_forward_proxy(candidate),
        "trojan" => parse_trojan_forward_proxy(candidate),
        "ss" => parse_shadowsocks_forward_proxy(candidate),
        _ => None,
    }
}

fn parse_native_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let parsed = Url::parse(candidate).ok()?;
    let raw_scheme = parsed.scheme();
    let (protocol, normalized_scheme) = match raw_scheme {
        "http" => (ForwardProxyProtocol::Http, "http"),
        "https" => (ForwardProxyProtocol::Https, "https"),
        "socks5" | "socks" => (ForwardProxyProtocol::Socks5, "socks5"),
        "socks5h" => (ForwardProxyProtocol::Socks5h, "socks5h"),
        _ => return None,
    };
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    let mut normalized = format!("{normalized_scheme}://");
    if !parsed.username().is_empty() {
        normalized.push_str(parsed.username());
        if let Some(password) = parsed.password() {
            normalized.push(':');
            normalized.push_str(password);
        }
        normalized.push('@');
    }
    if host.contains(':') {
        normalized.push('[');
        normalized.push_str(host);
        normalized.push(']');
    } else {
        normalized.push_str(&host.to_ascii_lowercase());
    }
    normalized.push(':');
    normalized.push_str(&port.to_string());
    let endpoint_url = Url::parse(&normalized).ok()?;
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name: format!("{host}:{port}"),
        protocol,
        endpoint_url: Some(endpoint_url),
    })
}

fn parse_vmess_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "vmess")?;
    let parsed = parse_vmess_share_link(&normalized).ok()?;
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name: parsed.display_name,
        protocol: ForwardProxyProtocol::Vmess,
        endpoint_url: None,
    })
}

fn parse_vless_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "vless")?;
    let parsed = Url::parse(&normalized).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    let display_name =
        proxy_display_name_from_url(&parsed).unwrap_or_else(|| format!("{host}:{port}"));
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name,
        protocol: ForwardProxyProtocol::Vless,
        endpoint_url: None,
    })
}

fn parse_trojan_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "trojan")?;
    let parsed = Url::parse(&normalized).ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default()?;
    let display_name =
        proxy_display_name_from_url(&parsed).unwrap_or_else(|| format!("{host}:{port}"));
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name,
        protocol: ForwardProxyProtocol::Trojan,
        endpoint_url: None,
    })
}

fn parse_shadowsocks_forward_proxy(candidate: &str) -> Option<ParsedForwardProxyEntry> {
    let normalized = normalize_share_link_scheme(candidate, "ss")?;
    let parsed = parse_shadowsocks_share_link(&normalized).ok()?;
    Some(ParsedForwardProxyEntry {
        normalized,
        display_name: parsed.display_name,
        protocol: ForwardProxyProtocol::Shadowsocks,
        endpoint_url: None,
    })
}

fn proxy_display_name_from_url(url: &Url) -> Option<String> {
    if let Some(fragment) = url.fragment() {
        let decoded = percent_decode_once_lossy(fragment);
        if !decoded.trim().is_empty() {
            return Some(decoded);
        }
    }
    let host = url.host_str()?;
    let port = url.port_or_known_default()?;
    Some(format!("{host}:{port}"))
}

fn normalize_share_link_scheme(candidate: &str, scheme: &str) -> Option<String> {
    let (_, remainder) = candidate.split_once("://")?;
    let normalized = format!("{scheme}://{}", remainder.trim());
    if normalized.len() <= scheme.len() + 3 {
        return None;
    }
    Some(normalized)
}

fn decode_base64_any(raw: &str) -> Option<Vec<u8>> {
    let compact = raw
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    if compact.is_empty() {
        return None;
    }
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(compact.as_bytes()) {
            return Some(decoded);
        }
    }
    None
}

fn decode_base64_string(raw: &str) -> Option<String> {
    decode_base64_any(raw).and_then(|bytes| String::from_utf8(bytes).ok())
}

#[derive(Debug, Clone)]
struct VmessShareLink {
    address: String,
    port: u16,
    id: String,
    alter_id: u32,
    security: String,
    network: String,
    host: Option<String>,
    path: Option<String>,
    tls_mode: Option<String>,
    sni: Option<String>,
    alpn: Option<Vec<String>>,
    fingerprint: Option<String>,
    display_name: String,
}

fn parse_vmess_share_link(raw: &str) -> Result<VmessShareLink, ProxyError> {
    let payload = raw
        .strip_prefix("vmess://")
        .ok_or_else(|| ProxyError::Other("invalid vmess share link".to_string()))?;
    let decoded = decode_base64_string(payload)
        .ok_or_else(|| ProxyError::Other("failed to decode vmess payload".to_string()))?;
    let value: Value = serde_json::from_str(&decoded)
        .map_err(|err| ProxyError::Other(format!("invalid vmess json payload: {err}")))?;

    let address = value
        .get("add")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::Other("vmess payload missing add".to_string()))?
        .to_string();
    let port = parse_port_value(value.get("port"))
        .ok_or_else(|| ProxyError::Other("vmess payload missing port".to_string()))?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ProxyError::Other("vmess payload missing id".to_string()))?
        .to_string();
    let alter_id = parse_u32_value(value.get("aid")).unwrap_or(0);
    let security = value
        .get("scy")
        .and_then(Value::as_str)
        .or_else(|| value.get("security").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto")
        .to_string();
    let network = value
        .get("net")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("tcp")
        .to_ascii_lowercase();
    let host = value
        .get("host")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let tls_mode = value
        .get("tls")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let sni = value
        .get("sni")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let alpn = value
        .get("alpn")
        .and_then(Value::as_str)
        .map(parse_alpn_csv)
        .filter(|items| !items.is_empty());
    let fingerprint = value
        .get("fp")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let display_name = value
        .get("ps")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{address}:{port}"));
    Ok(VmessShareLink {
        address,
        port,
        id,
        alter_id,
        security,
        network,
        host,
        path,
        tls_mode,
        sni,
        alpn,
        fingerprint,
        display_name,
    })
}

fn parse_u32_value(value: Option<&Value>) -> Option<u32> {
    match value {
        Some(Value::Number(num)) => num.as_u64().and_then(|value| u32::try_from(value).ok()),
        Some(Value::String(raw)) => raw.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn parse_port_value(value: Option<&Value>) -> Option<u16> {
    match value {
        Some(Value::Number(num)) => num.as_u64().and_then(|value| u16::try_from(value).ok()),
        Some(Value::String(raw)) => raw.trim().parse::<u16>().ok(),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct ShadowsocksShareLink {
    method: String,
    password: String,
    host: String,
    port: u16,
    display_name: String,
}

fn parse_shadowsocks_share_link(raw: &str) -> Result<ShadowsocksShareLink, ProxyError> {
    let normalized = raw
        .strip_prefix("ss://")
        .ok_or_else(|| ProxyError::Other("invalid shadowsocks share link".to_string()))?;
    let (main, fragment) = split_once_first(normalized, '#');
    let (main, _) = split_once_first(main, '?');
    let display_name = fragment
        .map(percent_decode_once_lossy)
        .filter(|value| !value.trim().is_empty());

    if let Ok(url) = Url::parse(raw)
        && let Some(host) = url.host_str()
        && let Some(port) = url.port_or_known_default()
    {
        let credentials = if !url.username().is_empty() && url.password().is_some() {
            Some((
                percent_decode_once_lossy(url.username()),
                percent_decode_once_lossy(url.password().unwrap_or_default()),
            ))
        } else if !url.username().is_empty() {
            let username = percent_decode_once_lossy(url.username());
            decode_base64_string(&username).and_then(|decoded| {
                let (method, password) = decoded.split_once(':')?;
                Some((method.to_string(), password.to_string()))
            })
        } else {
            None
        };
        if let Some((method, password)) = credentials {
            return Ok(ShadowsocksShareLink {
                method,
                password,
                host: host.to_string(),
                port,
                display_name: display_name
                    .clone()
                    .unwrap_or_else(|| format!("{host}:{port}")),
            });
        }
    }

    let decoded_main = if main.contains('@') {
        main.to_string()
    } else {
        let main_for_decode = percent_decode_once_lossy(main);
        decode_base64_string(&main_for_decode)
            .ok_or_else(|| ProxyError::Other("failed to decode shadowsocks payload".to_string()))?
    };

    let (credential, host_port) = decoded_main
        .rsplit_once('@')
        .ok_or_else(|| ProxyError::Other("invalid shadowsocks payload".to_string()))?;
    let (method, password) = if let Some((method, password)) = credential.split_once(':') {
        (
            percent_decode_once_lossy(method),
            percent_decode_once_lossy(password),
        )
    } else {
        let decoded_credential = decode_base64_string(credential).ok_or_else(|| {
            ProxyError::Other("failed to decode shadowsocks credentials".to_string())
        })?;
        let (method, password) = decoded_credential
            .split_once(':')
            .ok_or_else(|| ProxyError::Other("invalid shadowsocks credentials".to_string()))?;
        (
            percent_decode_once_lossy(method),
            percent_decode_once_lossy(password),
        )
    };
    let parsed_host = Url::parse(&format!("http://{host_port}"))
        .map_err(|err| ProxyError::Other(format!("invalid shadowsocks server endpoint: {err}")))?;
    let host = parsed_host
        .host_str()
        .ok_or_else(|| ProxyError::Other("shadowsocks host missing".to_string()))?
        .to_string();
    let port = parsed_host
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("shadowsocks port missing".to_string()))?;
    Ok(ShadowsocksShareLink {
        method,
        password,
        host: host.clone(),
        port,
        display_name: display_name.unwrap_or_else(|| format!("{host}:{port}")),
    })
}

fn split_once_first(raw: &str, delimiter: char) -> (&str, Option<&str>) {
    if let Some((lhs, rhs)) = raw.split_once(delimiter) {
        (lhs, Some(rhs))
    } else {
        (raw, None)
    }
}

fn parse_alpn_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn percent_decode_once_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'%'
            && idx + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                decode_hex_nibble(bytes[idx + 1]),
                decode_hex_nibble(bytes[idx + 2]),
            )
        {
            decoded.push((hi << 4) | lo);
            idx += 3;
            continue;
        }
        decoded.push(bytes[idx]);
        idx += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn deterministic_unit_f64(seed: u64) -> f64 {
    let mut value = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    value ^= value >> 33;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53);
    value ^= value >> 33;
    (value as f64) / (u64::MAX as f64)
}

fn align_bucket_epoch(epoch: i64, bucket_seconds: i64, offset_seconds: i64) -> i64 {
    if bucket_seconds <= 0 {
        return epoch;
    }
    (epoch - offset_seconds).div_euclid(bucket_seconds) * bucket_seconds + offset_seconds
}

fn format_utc_iso(epoch: i64) -> Result<String, ProxyError> {
    let dt = Utc
        .timestamp_opt(epoch, 0)
        .single()
        .ok_or_else(|| ProxyError::Other(format!("invalid epoch {epoch}")))?;
    Ok(dt.to_rfc3339())
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

pub async fn fetch_subscription_proxy_urls(
    client: &Client,
    subscription_url: &str,
    request_timeout: Duration,
) -> Result<Vec<String>, ProxyError> {
    let response = timeout(request_timeout, client.get(subscription_url).send())
        .await
        .map_err(|_| ProxyError::Other("subscription request timed out".to_string()))?
        .map_err(ProxyError::Http)?;
    if !response.status().is_success() {
        return Err(ProxyError::Other(format!(
            "subscription url returned status {}: {}",
            response.status(),
            subscription_url
        )));
    }
    let body = timeout(request_timeout, response.text())
        .await
        .map_err(|_| ProxyError::Other("subscription body read timed out".to_string()))?
        .map_err(ProxyError::Http)?;
    let urls = parse_proxy_urls_from_subscription_body(&body);
    if urls.is_empty() && subscription_body_uses_unsupported_structure(&body) {
        return Err(ProxyError::Other(
            "subscription contains no supported proxy entries".to_string(),
        ));
    }
    Ok(urls)
}

pub(crate) async fn fetch_subscription_proxy_urls_with_validation_budget(
    client: &Client,
    subscription_url: &str,
    total_timeout: Duration,
    started: Instant,
) -> Result<Vec<String>, ProxyError> {
    let request_timeout = remaining_timeout_budget(total_timeout, started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?;
    let response = timeout(request_timeout, client.get(subscription_url).send())
        .await
        .map_err(|_| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?
        .map_err(ProxyError::Http)?;
    if !response.status().is_success() {
        return Err(ProxyError::Other(format!(
            "subscription url returned status {}: {}",
            response.status(),
            subscription_url
        )));
    }
    let read_timeout = remaining_timeout_budget(total_timeout, started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?;
    let body = timeout(read_timeout, response.text())
        .await
        .map_err(|_| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                total_timeout.as_millis()
            ))
        })?
        .map_err(ProxyError::Http)?;
    let urls = parse_proxy_urls_from_subscription_body(&body);
    if urls.is_empty() && subscription_body_uses_unsupported_structure(&body) {
        return Err(ProxyError::Other(
            "subscription contains no supported proxy entries".to_string(),
        ));
    }
    Ok(urls)
}

fn parse_proxy_urls_from_subscription_body(raw: &str) -> Vec<String> {
    let decoded = decode_subscription_payload(raw);
    if subscription_body_uses_unsupported_structure(&decoded) {
        return Vec::new();
    }
    normalize_proxy_url_entries(vec![decoded])
}

fn subscription_body_uses_unsupported_structure(raw: &str) -> bool {
    raw.lines().map(str::trim).any(|line| {
        line == "proxies:"
            || line == "proxy-providers:"
            || line == "proxy-groups:"
            || line == "rule-providers:"
    })
}

pub fn decode_subscription_payload(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.contains("://")
        || trimmed
            .lines()
            .filter(|line| !line.trim().is_empty())
            .any(|line| line.contains("://"))
    {
        return trimmed.to_string();
    }
    let compact = trimmed
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::STANDARD_NO_PAD,
        base64::engine::general_purpose::URL_SAFE,
        base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(compact.as_bytes())
            && let Ok(text) = String::from_utf8(decoded)
            && text.contains("://")
        {
            return text;
        }
    }
    trimmed.to_string()
}

fn is_validation_probe_reachable_status(status: StatusCode) -> bool {
    status.is_success()
        || status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status == StatusCode::NOT_FOUND
}

fn forward_proxy_validation_timeout(kind: ForwardProxyValidationKind) -> Duration {
    match kind {
        ForwardProxyValidationKind::ProxyUrl => {
            Duration::from_secs(FORWARD_PROXY_VALIDATION_TIMEOUT_SECS)
        }
        ForwardProxyValidationKind::SubscriptionUrl => {
            Duration::from_secs(FORWARD_PROXY_SUBSCRIPTION_VALIDATION_TIMEOUT_SECS)
        }
    }
}

fn remaining_timeout_budget(total_timeout: Duration, elapsed: Duration) -> Option<Duration> {
    total_timeout.checked_sub(elapsed)
}

fn classify_forward_proxy_error(err: &ProxyError) -> &'static str {
    match err {
        ProxyError::Http(source) if source.is_timeout() => FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT,
        ProxyError::Http(_) => FORWARD_PROXY_FAILURE_SEND_ERROR,
        ProxyError::Other(message) if message.contains("timed out") => {
            FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT
        }
        _ => FORWARD_PROXY_FAILURE_SEND_ERROR,
    }
}

fn build_forward_proxy_probe_target(usage_base: &str) -> Result<Url, ProxyError> {
    let mut url = Url::parse(usage_base).map_err(|err| ProxyError::InvalidEndpoint {
        endpoint: usage_base.to_string(),
        source: err,
    })?;
    url.set_path("/usage");
    Ok(url)
}

pub async fn probe_forward_proxy_endpoint(
    client_pool: &ForwardProxyClientPool,
    endpoint: &ForwardProxyEndpoint,
    probe_url: &Url,
    timeout_budget: Duration,
) -> Result<f64, ProxyError> {
    if endpoint.requires_xray() && endpoint.endpoint_url.is_none() {
        return Err(ProxyError::Other("xray_missing".to_string()));
    }
    let client = client_pool
        .client_for(endpoint.endpoint_url.as_ref())
        .await?;
    let started = Instant::now();
    let response = timeout(timeout_budget, client.get(probe_url.clone()).send())
        .await
        .map_err(|_| {
            ProxyError::Other(format!(
                "validation timed out after {}ms",
                timeout_budget.as_millis()
            ))
        })?
        .map_err(ProxyError::Http)?;
    if !is_validation_probe_reachable_status(response.status()) {
        return Err(ProxyError::Other(format!(
            "validation probe returned status {}",
            response.status()
        )));
    }
    Ok(elapsed_ms(started))
}

pub fn failure_kind_from_http_error(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        FORWARD_PROXY_FAILURE_HANDSHAKE_TIMEOUT
    } else if err.is_status() {
        FORWARD_PROXY_FAILURE_UPSTREAM_HTTP_5XX
    } else {
        FORWARD_PROXY_FAILURE_SEND_ERROR
    }
}

async fn wait_for_xray_proxy_ready(
    child: &mut Child,
    local_port: u16,
    ready_timeout: Duration,
) -> Result<(), ProxyError> {
    let deadline = Instant::now() + ready_timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|err| {
            ProxyError::Other(format!("failed to poll xray proxy process status: {err}"))
        })? {
            return Err(ProxyError::Other(format!(
                "xray process exited before ready: {status}"
            )));
        }
        let connect_attempt = timeout(
            Duration::from_millis(250),
            TcpStream::connect(("127.0.0.1", local_port)),
        );
        if connect_attempt
            .await
            .is_ok_and(|connection| connection.is_ok())
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ProxyError::Other(
                "xray local socks endpoint was not ready in time".to_string(),
            ));
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn terminate_child_process(
    child: &mut Child,
    grace_period: Duration,
) -> Result<(), io::Error> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            let result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if result == 0
                && !grace_period.is_zero()
                && timeout(grace_period, child.wait()).await.is_ok()
            {
                return Ok(());
            }
        }
    }
    child.kill().await?;
    let _ = timeout(grace_period, child.wait()).await;
    Ok(())
}

fn pick_unused_local_port() -> Result<u16, ProxyError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|err| {
        ProxyError::Other(format!(
            "failed to bind local socket for port allocation: {err}"
        ))
    })?;
    let port = listener
        .local_addr()
        .map_err(|err| {
            ProxyError::Other(format!(
                "failed to read local address for allocated port: {err}"
            ))
        })?
        .port();
    Ok(port)
}

pub fn stable_hash_u64(raw: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    hasher.finish()
}

fn build_xray_instance_config(local_port: u16, outbound: Value) -> Value {
    json!({
        "log": { "loglevel": "warning" },
        "inbounds": [{
            "tag": "inbound-local-socks",
            "listen": "127.0.0.1",
            "port": local_port,
            "protocol": "socks",
            "settings": { "auth": "noauth", "udp": false }
        }],
        "outbounds": [outbound, { "tag": "direct", "protocol": "freedom" }],
        "routing": {
            "domainStrategy": "AsIs",
            "rules": [{ "type": "field", "inboundTag": ["inbound-local-socks"], "outboundTag": "proxy" }]
        }
    })
}

fn build_xray_outbound_for_endpoint(endpoint: &ForwardProxyEndpoint) -> Result<Value, ProxyError> {
    let raw = endpoint
        .raw_url
        .as_deref()
        .ok_or_else(|| ProxyError::Other("xray endpoint missing share link url".to_string()))?;
    match endpoint.protocol {
        ForwardProxyProtocol::Vmess => build_vmess_xray_outbound(raw),
        ForwardProxyProtocol::Vless => build_vless_xray_outbound(raw),
        ForwardProxyProtocol::Trojan => build_trojan_xray_outbound(raw),
        ForwardProxyProtocol::Shadowsocks => build_shadowsocks_xray_outbound(raw),
        _ => Err(ProxyError::Other(
            "unsupported xray protocol for endpoint".to_string(),
        )),
    }
}

fn build_vmess_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let link = parse_vmess_share_link(raw)?;
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "vmess",
        "settings": {
            "vnext": [{
                "address": link.address,
                "port": link.port,
                "users": [{ "id": link.id, "alterId": link.alter_id, "security": link.security }]
            }]
        }
    });
    if let Some(stream_settings) = build_vmess_stream_settings(&link)
        && let Some(object) = outbound.as_object_mut()
    {
        object.insert("streamSettings".to_string(), stream_settings);
    }
    Ok(outbound)
}

fn build_vmess_stream_settings(link: &VmessShareLink) -> Option<Value> {
    let mut stream = serde_json::Map::new();
    stream.insert("network".to_string(), Value::String(link.network.clone()));
    let mut has_non_default_options = link.network != "tcp";
    let security = link
        .tls_mode
        .as_deref()
        .filter(|value| !value.is_empty() && *value != "none")
        .map(|value| value.to_ascii_lowercase());
    if let Some(security) = security.as_ref() {
        stream.insert("security".to_string(), Value::String(security.clone()));
        has_non_default_options = true;
    }
    match link.network.as_str() {
        "ws" => {
            let mut ws = serde_json::Map::new();
            if let Some(path) = link.path.as_ref().filter(|value| !value.trim().is_empty()) {
                ws.insert("path".to_string(), Value::String(path.clone()));
            }
            if let Some(host) = link.host.as_ref().filter(|value| !value.trim().is_empty()) {
                ws.insert("headers".to_string(), json!({ "Host": host }));
            }
            if !ws.is_empty() {
                stream.insert("wsSettings".to_string(), Value::Object(ws));
                has_non_default_options = true;
            }
        }
        "grpc" => {
            let service_name = link
                .path
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .unwrap_or_default();
            stream.insert(
                "grpcSettings".to_string(),
                json!({ "serviceName": service_name }),
            );
            has_non_default_options = true;
        }
        _ => {}
    }
    if let Some(security) = security
        && security == "tls"
    {
        let mut tls_settings = serde_json::Map::new();
        if let Some(server_name) = link
            .sni
            .as_ref()
            .or(link.host.as_ref())
            .filter(|value| !value.trim().is_empty())
        {
            tls_settings.insert("serverName".to_string(), Value::String(server_name.clone()));
        }
        if let Some(alpn) = link.alpn.as_ref().filter(|items| !items.is_empty()) {
            tls_settings.insert("alpn".to_string(), json!(alpn));
        }
        if let Some(fingerprint) = link
            .fingerprint
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            tls_settings.insert(
                "fingerprint".to_string(),
                Value::String(fingerprint.clone()),
            );
        }
        if !tls_settings.is_empty() {
            stream.insert("tlsSettings".to_string(), Value::Object(tls_settings));
            has_non_default_options = true;
        }
    }
    if has_non_default_options {
        Some(Value::Object(stream))
    } else {
        None
    }
}

fn build_vless_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let url = Url::parse(raw)
        .map_err(|err| ProxyError::Other(format!("invalid vless share link: {err}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProxyError::Other("vless host missing".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("vless port missing".to_string()))?;
    let user_id = url.username();
    if user_id.trim().is_empty() {
        return Err(ProxyError::Other("vless id missing".to_string()));
    }
    let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    let encryption = query
        .get("encryption")
        .cloned()
        .unwrap_or_else(|| "none".to_string());
    let mut user = serde_json::Map::new();
    user.insert("id".to_string(), Value::String(user_id.to_string()));
    user.insert("encryption".to_string(), Value::String(encryption));
    if let Some(flow) = query.get("flow").filter(|value| !value.trim().is_empty()) {
        user.insert("flow".to_string(), Value::String(flow.clone()));
    }
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "vless",
        "settings": { "vnext": [{ "address": host, "port": port, "users": [Value::Object(user)] }] }
    });
    if let Some(stream_settings) = build_stream_settings_from_url(&url, None)
        && let Some(object) = outbound.as_object_mut()
    {
        object.insert("streamSettings".to_string(), stream_settings);
    }
    Ok(outbound)
}

fn build_trojan_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let url = Url::parse(raw)
        .map_err(|err| ProxyError::Other(format!("invalid trojan share link: {err}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| ProxyError::Other("trojan host missing".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ProxyError::Other("trojan port missing".to_string()))?;
    let password = url.username();
    if password.trim().is_empty() {
        return Err(ProxyError::Other("trojan password missing".to_string()));
    }
    let mut outbound = json!({
        "tag": "proxy",
        "protocol": "trojan",
        "settings": { "servers": [{ "address": host, "port": port, "password": password }] }
    });
    if let Some(stream_settings) = build_stream_settings_from_url(&url, Some("tls"))
        && let Some(object) = outbound.as_object_mut()
    {
        object.insert("streamSettings".to_string(), stream_settings);
    }
    Ok(outbound)
}

fn build_shadowsocks_xray_outbound(raw: &str) -> Result<Value, ProxyError> {
    let parsed = parse_shadowsocks_share_link(raw)?;
    Ok(json!({
        "tag": "proxy",
        "protocol": "shadowsocks",
        "settings": { "servers": [{ "address": parsed.host, "port": parsed.port, "method": parsed.method, "password": parsed.password }] }
    }))
}

fn build_stream_settings_from_url(url: &Url, default_security: Option<&str>) -> Option<Value> {
    let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    let network = query
        .get("type")
        .or_else(|| query.get("net"))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "tcp".to_string());
    let security = query
        .get("security")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .or_else(|| default_security.map(str::to_string))
        .unwrap_or_else(|| "none".to_string());
    let host = query
        .get("host")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let path = query
        .get("path")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let service_name = query
        .get("serviceName")
        .or_else(|| query.get("service_name"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| path.clone());
    let mut stream = serde_json::Map::new();
    stream.insert("network".to_string(), Value::String(network.clone()));
    let mut has_non_default_options = network != "tcp";
    if security != "none" {
        stream.insert("security".to_string(), Value::String(security.clone()));
        has_non_default_options = true;
    }
    match network.as_str() {
        "ws" => {
            let mut ws = serde_json::Map::new();
            if let Some(path) = path.as_ref() {
                ws.insert("path".to_string(), Value::String(path.clone()));
            }
            if let Some(host) = host.as_ref() {
                ws.insert("headers".to_string(), json!({ "Host": host }));
            }
            if !ws.is_empty() {
                stream.insert("wsSettings".to_string(), Value::Object(ws));
                has_non_default_options = true;
            }
        }
        "grpc" => {
            stream.insert(
                "grpcSettings".to_string(),
                json!({ "serviceName": service_name.unwrap_or_default() }),
            );
            has_non_default_options = true;
        }
        _ => {}
    }
    if security == "tls" {
        let mut tls_settings = serde_json::Map::new();
        if let Some(server_name) = query
            .get("sni")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| host.clone())
            .or_else(|| url.host_str().map(str::to_string))
        {
            tls_settings.insert("serverName".to_string(), Value::String(server_name));
        }
        if query_flag_true(&query, "allowInsecure") || query_flag_true(&query, "insecure") {
            tls_settings.insert("allowInsecure".to_string(), Value::Bool(true));
        }
        if let Some(fingerprint) = query
            .get("fp")
            .or_else(|| query.get("fingerprint"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            tls_settings.insert("fingerprint".to_string(), Value::String(fingerprint));
        }
        if let Some(alpn) = query
            .get("alpn")
            .map(|value| parse_alpn_csv(value))
            .filter(|items| !items.is_empty())
        {
            tls_settings.insert("alpn".to_string(), json!(alpn));
        }
        if !tls_settings.is_empty() {
            stream.insert("tlsSettings".to_string(), Value::Object(tls_settings));
            has_non_default_options = true;
        }
    } else if security == "reality" {
        let mut reality_settings = serde_json::Map::new();
        if let Some(server_name) = query
            .get("sni")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| host.clone())
            .or_else(|| url.host_str().map(str::to_string))
        {
            reality_settings.insert("serverName".to_string(), Value::String(server_name));
        }
        if let Some(fingerprint) = query
            .get("fp")
            .or_else(|| query.get("fingerprint"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("fingerprint".to_string(), Value::String(fingerprint));
        }
        if let Some(public_key) = query
            .get("pbk")
            .or_else(|| query.get("publicKey"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("publicKey".to_string(), Value::String(public_key));
        }
        if let Some(short_id) = query
            .get("sid")
            .or_else(|| query.get("shortId"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("shortId".to_string(), Value::String(short_id));
        }
        if let Some(spider_x) = query
            .get("spx")
            .or_else(|| query.get("spiderX"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            reality_settings.insert("spiderX".to_string(), Value::String(spider_x));
        }
        if !reality_settings.is_empty() {
            stream.insert(
                "realitySettings".to_string(),
                Value::Object(reality_settings),
            );
            has_non_default_options = true;
        }
    }
    if has_non_default_options {
        Some(Value::Object(stream))
    } else {
        None
    }
}

fn query_flag_true(query: &HashMap<String, String>, key: &str) -> bool {
    query.get(key).is_some_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_probe_url_uses_public_probe_endpoint() {
        let upstream = Url::parse("http://127.0.0.1:30014/mcp").expect("parse upstream");
        let probe = derive_probe_url(&upstream);

        assert_eq!(probe.as_str(), "http://example.com/");
    }

    #[test]
    fn parse_proxy_urls_from_subscription_body_ignores_structured_yaml_configs() {
        let body = r#"
proxies:
  - name: hinet-reality
    type: vless
    server: hinet-ep.707979.xyz
    port: 53842
rule-providers:
  sample:
    type: http
    url: https://example.com/rules.yaml
"#;

        assert!(parse_proxy_urls_from_subscription_body(body).is_empty());
        assert!(subscription_body_uses_unsupported_structure(body));
    }

    #[test]
    fn build_vless_xray_outbound_preserves_reality_settings() {
        let outbound = build_vless_xray_outbound("vless://0688fa59-e971-4278-8c03-4b35821a71dc@hklb-ep.707979.xyz:53842?encryption=none&security=reality&type=tcp&sni=public.sn.files.1drv.com&fp=chrome&pbk=6cJN5zHglyIywI_ZnsC7xW6lD1IO9gkHSvw6uvULCWQ&sid=61446ca92a46cdc7&flow=xtls-rprx-vision#Ivan-hkl-vless-vision").expect("build outbound");
        let stream = outbound
            .get("streamSettings")
            .and_then(Value::as_object)
            .expect("stream settings");
        assert_eq!(
            stream.get("security").and_then(Value::as_str),
            Some("reality")
        );

        let reality = stream
            .get("realitySettings")
            .and_then(Value::as_object)
            .expect("reality settings");
        assert_eq!(
            reality.get("serverName").and_then(Value::as_str),
            Some("public.sn.files.1drv.com")
        );
        assert_eq!(
            reality.get("fingerprint").and_then(Value::as_str),
            Some("chrome")
        );
        assert_eq!(
            reality.get("publicKey").and_then(Value::as_str),
            Some("6cJN5zHglyIywI_ZnsC7xW6lD1IO9gkHSvw6uvULCWQ")
        );
        assert_eq!(
            reality.get("shortId").and_then(Value::as_str),
            Some("61446ca92a46cdc7")
        );
    }

    #[test]
    fn parse_vless_forward_proxy_decodes_percent_encoded_display_name_once() {
        let parsed = parse_vless_forward_proxy(
            "vless://0688fa59-e971-4278-8c03-4b35821a71dc@example.com:443?encryption=none#%E9%A6%99%E6%B8%AF%20%F0%9F%87%AD%F0%9F%87%B0",
        )
        .expect("parse vless");

        assert_eq!(parsed.display_name, "香港 🇭🇰");
    }

    #[test]
    fn parse_trojan_forward_proxy_falls_back_when_fragment_decodes_to_blank() {
        let parsed =
            parse_trojan_forward_proxy("trojan://secret@example.com:8443?security=tls#%20%20")
                .expect("parse trojan");

        assert_eq!(parsed.display_name, "example.com:8443");
    }

    #[test]
    fn parse_vless_forward_proxy_keeps_lossy_fragment_for_invalid_percent_encoding() {
        let parsed = parse_vless_forward_proxy(
            "vless://0688fa59-e971-4278-8c03-4b35821a71dc@example.com:443?encryption=none#broken%ZZname",
        )
        .expect("parse vless");

        assert_eq!(parsed.display_name, "broken%ZZname");
    }

    #[test]
    fn endpoint_host_prefers_share_link_host_for_xray_routes() {
        let endpoint = ForwardProxyEndpoint {
            key: "vless://example".to_string(),
            source: FORWARD_PROXY_SOURCE_MANUAL.to_string(),
            display_name: "example".to_string(),
            protocol: ForwardProxyProtocol::Vless,
            endpoint_url: Some(
                Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route"),
            ),
            raw_url: Some(
                "vless://0688fa59-e971-4278-8c03-4b35821a71dc@1.1.1.1:443?encryption=none#hk"
                    .to_string(),
            ),
            manual_present: true,
            subscription_sources: BTreeSet::new(),
        };

        assert_eq!(endpoint_host(&endpoint).as_deref(), Some("1.1.1.1"));
    }

    #[test]
    fn endpoint_host_keeps_local_listener_for_non_xray_routes() {
        let endpoint = ForwardProxyEndpoint {
            key: "http://127.0.0.1:8080".to_string(),
            source: FORWARD_PROXY_SOURCE_MANUAL.to_string(),
            display_name: "local".to_string(),
            protocol: ForwardProxyProtocol::Http,
            endpoint_url: Some(Url::parse("http://127.0.0.1:8080").expect("parse http url")),
            raw_url: Some("http://example.com:8080".to_string()),
            manual_present: true,
            subscription_sources: BTreeSet::new(),
        };

        assert_eq!(endpoint_host(&endpoint).as_deref(), Some("127.0.0.1"));
    }

    #[test]
    fn subscription_refresh_preserves_overlapping_manual_and_subscription_sources() {
        let subscription_url = "https://subscription.example.com/feed".to_string();
        let endpoint_url = "http://198.51.100.8:8080".to_string();
        let settings = ForwardProxySettings {
            proxy_urls: vec![endpoint_url.clone()],
            subscription_urls: vec![subscription_url.clone()],
            subscription_update_interval_secs: 3600,
            insert_direct: false,
        };
        let mut manager = ForwardProxyManager::new(settings.clone(), Vec::new());
        let fetched = HashMap::from([(subscription_url.clone(), vec![endpoint_url.clone()])]);

        manager.apply_subscription_refresh(&fetched);

        let endpoint = manager
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key == endpoint_url)
            .expect("overlapping endpoint present");
        assert!(endpoint.manual_present);
        assert_eq!(
            endpoint.subscription_sources,
            BTreeSet::from([subscription_url.clone()])
        );
        assert_eq!(endpoint.source, FORWARD_PROXY_SOURCE_MANUAL);

        manager.apply_incremental_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                ..settings
            },
            &HashMap::new(),
        );

        let endpoint = manager
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key == endpoint_url)
            .expect("subscription-backed endpoint should remain after manual removal");
        assert!(!endpoint.manual_present);
        assert_eq!(
            endpoint.subscription_sources,
            BTreeSet::from([subscription_url])
        );
        assert_eq!(endpoint.source, FORWARD_PROXY_SOURCE_SUBSCRIPTION);
    }

    #[test]
    fn incremental_subscription_save_updates_refresh_timestamp() {
        let subscription_url = "https://subscription.example.com/feed".to_string();
        let endpoint_url = "http://198.51.100.8:8080".to_string();
        let mut manager = ForwardProxyManager::new(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
            },
            Vec::new(),
        );

        manager.apply_incremental_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: vec![subscription_url.clone()],
                subscription_update_interval_secs: 3600,
                insert_direct: false,
            },
            &HashMap::from([(subscription_url, vec![endpoint_url])]),
        );

        assert!(manager.last_subscription_refresh_at.is_some());
        assert!(!manager.should_refresh_subscriptions());
    }
}

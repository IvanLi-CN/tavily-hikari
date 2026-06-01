use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use ring::hmac;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaMode {
    Single,
    ActiveStandby,
}

impl HaMode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "active_standby" | "active-standby" | "ha" => Self::ActiveStandby,
            _ => Self::Single,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaNodeRole {
    FullMaster,
    ProvisionalMaster,
    Standby,
    Recovery,
}

impl HaNodeRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FullMaster => "full_master",
            Self::ProvisionalMaster => "provisional_master",
            Self::Standby => "standby",
            Self::Recovery => "recovery",
        }
    }

    pub fn allows_basic_business(self) -> bool {
        matches!(self, Self::FullMaster | Self::ProvisionalMaster)
    }

    pub fn allows_full_writes(self) -> bool {
        matches!(self, Self::FullMaster)
    }

    pub fn is_degraded(self) -> bool {
        !matches!(self, Self::FullMaster)
    }
}

#[derive(Clone, Debug)]
pub struct HaConfig {
    pub mode: HaMode,
    pub node_id: String,
    pub database_path: Option<String>,
    pub node_public_scheme: Option<String>,
    pub node_public_host: Option<String>,
    pub node_public_port: Option<u16>,
    pub edgeone_zone_id: Option<String>,
    pub edgeone_domain: Option<String>,
    pub edgeone_expected_origin_scheme: Option<String>,
    pub edgeone_expected_origin_host: Option<String>,
    pub edgeone_expected_origin_port: Option<u16>,
    pub edgeone_secret_id: Option<String>,
    pub edgeone_secret_key: Option<String>,
    pub edgeone_api_endpoint: String,
    pub sync_peer_url: Option<String>,
    pub internal_token: Option<String>,
    pub sync_interval_secs: u64,
}

impl Default for HaConfig {
    fn default() -> Self {
        Self {
            mode: HaMode::Single,
            node_id: "single".to_string(),
            database_path: None,
            node_public_scheme: None,
            node_public_host: None,
            node_public_port: None,
            edgeone_zone_id: None,
            edgeone_domain: None,
            edgeone_expected_origin_scheme: None,
            edgeone_expected_origin_host: None,
            edgeone_expected_origin_port: None,
            edgeone_secret_id: None,
            edgeone_secret_key: None,
            edgeone_api_endpoint: "https://teo.intl.tencentcloudapi.com".to_string(),
            sync_peer_url: None,
            internal_token: None,
            sync_interval_secs: 15,
        }
    }
}

impl HaConfig {
    pub fn active_standby_ready(&self) -> bool {
        self.mode == HaMode::ActiveStandby
            && self.configured_node_origin().ok().flatten().is_some()
            && self
                .edgeone_zone_id
                .as_deref()
                .is_some_and(|v| !v.is_empty())
            && self
                .edgeone_domain
                .as_deref()
                .is_some_and(|v| !v.is_empty())
            && self
                .edgeone_secret_id
                .as_deref()
                .is_some_and(|v| !v.is_empty())
            && self
                .edgeone_secret_key
                .as_deref()
                .is_some_and(|v| !v.is_empty())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaStatusView {
    pub mode: HaMode,
    pub node_id: String,
    pub node_public_origin: Option<String>,
    pub role: HaNodeRole,
    pub degraded: bool,
    pub allows_basic_business: bool,
    pub allows_full_writes: bool,
    pub edgeone_domain: Option<String>,
    pub edgeone_origin: Option<String>,
    pub edgeone_expected_origin: Option<String>,
    pub edgeone_api_configured: bool,
    pub last_edgeone_check_at: Option<i64>,
    pub last_sync_at: Option<i64>,
    pub sync_lag_seconds: Option<i64>,
    pub recovery_status: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaSnapshotManifest {
    pub source_node_id: String,
    pub generated_at: i64,
    pub wal_checkpoint: bool,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaRecoveryImportResult {
    pub batch_id: String,
    pub source_node_id: String,
    pub imported: bool,
    pub event_count: i64,
    pub checksum: String,
    pub message: String,
    pub status: HaStatusView,
}

#[derive(Clone, Debug)]
pub struct HaFailoverOperationRecord {
    pub operation_id: String,
    pub operation_kind: String,
    pub target_node_id: Option<String>,
    pub from_origin: Option<String>,
    pub to_origin: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EdgeOneAuditEntry {
    pub action: String,
    pub request_json: Option<String>,
    pub response_json: Option<String>,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug)]
struct HaRuntimeState {
    role: HaNodeRole,
    edgeone_origin: Option<String>,
    last_edgeone_check_at: Option<i64>,
    last_sync_at: Option<i64>,
    recovery_status: Option<String>,
    message: Option<String>,
}

#[derive(Clone)]
pub struct HaRuntime {
    config: Arc<HaConfig>,
    state: Arc<RwLock<HaRuntimeState>>,
    edgeone: EdgeOneClient,
}

impl HaRuntime {
    pub fn new(config: HaConfig) -> Self {
        let initial_role = if config.mode == HaMode::Single {
            HaNodeRole::FullMaster
        } else {
            HaNodeRole::Standby
        };
        let edgeone = EdgeOneClient::new(config.clone());
        Self {
            config: Arc::new(config),
            state: Arc::new(RwLock::new(HaRuntimeState {
                role: initial_role,
                edgeone_origin: None,
                last_edgeone_check_at: None,
                last_sync_at: None,
                recovery_status: None,
                message: None,
            })),
            edgeone,
        }
    }

    pub async fn refresh_startup_role(&self) -> Result<(), String> {
        if self.config.mode == HaMode::Single {
            return Ok(());
        }
        match self.edgeone.describe_current_origin().await {
            Ok(origin) => {
                let now = Utc::now().timestamp();
                let role = if self.is_self_origin(origin.as_deref()) {
                    HaNodeRole::FullMaster
                } else {
                    HaNodeRole::Standby
                };
                let mut state = self.state.write().await;
                state.role = role;
                state.edgeone_origin = origin;
                state.last_edgeone_check_at = Some(now);
                state.message = None;
                Ok(())
            }
            Err(err) => {
                let mut state = self.state.write().await;
                state.message = Some(format!("EdgeOne startup role check failed: {err}"));
                Err(err)
            }
        }
    }

    pub fn edgeone_authority_enabled(&self) -> bool {
        self.config.mode != HaMode::Single && self.config.active_standby_ready()
    }

    pub async fn refresh_authoritative_role(&self) -> Result<HaStatusView, String> {
        if !self.edgeone_authority_enabled() {
            return Ok(self.status().await);
        }

        let origin = match self.edgeone.describe_current_origin().await {
            Ok(origin) => origin,
            Err(err) => {
                let mut state = self.state.write().await;
                state.message = Some(format!("EdgeOne authority refresh failed: {err}"));
                return Err(err);
            }
        };
        let self_is_origin = self.is_self_origin(origin.as_deref());
        let now = Utc::now().timestamp();
        let mut state = self.state.write().await;
        state.edgeone_origin = origin.clone();
        state.last_edgeone_check_at = Some(now);
        match (self_is_origin, state.role) {
            (true, HaNodeRole::Standby | HaNodeRole::Recovery) => {
                state.role = HaNodeRole::ProvisionalMaster;
                state.recovery_status = None;
                state.message =
                    Some("EdgeOne origin now points to this node; finalize required".to_string());
            }
            (true, HaNodeRole::FullMaster) => {
                state.recovery_status = None;
                state.message = None;
            }
            (true, HaNodeRole::ProvisionalMaster) => {}
            (false, HaNodeRole::FullMaster | HaNodeRole::ProvisionalMaster) => {
                state.role = HaNodeRole::Recovery;
                let detail = origin
                    .as_deref()
                    .map(|origin| {
                        format!("EdgeOne origin moved to {origin}; recovery import required")
                    })
                    .unwrap_or_else(|| {
                        "EdgeOne origin no longer points to this node; recovery import required"
                            .to_string()
                    });
                state.recovery_status = Some(detail.clone());
                state.message = Some(detail);
            }
            (false, HaNodeRole::Standby) => {
                state.message = None;
            }
            (false, HaNodeRole::Recovery) => {}
        }
        drop(state);
        Ok(self.status().await)
    }

    pub async fn status(&self) -> HaStatusView {
        let state = self.state.read().await;
        let sync_lag_seconds = state
            .last_sync_at
            .map(|last| Utc::now().timestamp().saturating_sub(last));
        HaStatusView {
            mode: self.config.mode,
            node_id: self.config.node_id.clone(),
            node_public_origin: self
                .config
                .configured_node_origin()
                .ok()
                .flatten()
                .map(|origin| origin.authority())
                .or_else(|| self.config.node_public_host.clone()),
            role: state.role,
            degraded: state.role.is_degraded(),
            allows_basic_business: state.role.allows_basic_business(),
            allows_full_writes: state.role.allows_full_writes(),
            edgeone_domain: self.config.edgeone_domain.clone(),
            edgeone_origin: state.edgeone_origin.clone(),
            edgeone_expected_origin: self.config.canonical_expected_origin(),
            edgeone_api_configured: self.config.active_standby_ready(),
            last_edgeone_check_at: state.last_edgeone_check_at,
            last_sync_at: state.last_sync_at,
            sync_lag_seconds,
            recovery_status: state.recovery_status.clone(),
            message: state.message.clone(),
        }
    }

    pub fn database_path(&self) -> Option<PathBuf> {
        self.config
            .database_path
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    pub fn sync_peer_url(&self) -> Option<String> {
        self.config
            .sync_peer_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_end_matches('/').to_string())
    }

    pub fn internal_token(&self) -> Option<String> {
        self.config
            .internal_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }

    pub fn internal_token_matches(&self, candidate: Option<&str>) -> bool {
        match (self.config.internal_token.as_deref(), candidate) {
            (Some(expected), Some(candidate)) => {
                !expected.trim().is_empty() && expected.trim() == candidate.trim()
            }
            _ => false,
        }
    }

    pub fn sync_interval_secs(&self) -> u64 {
        self.config.sync_interval_secs.clamp(5, 15)
    }

    pub async fn role(&self) -> HaNodeRole {
        self.state.read().await.role
    }

    pub async fn allows_full_writes(&self) -> bool {
        self.role().await.allows_full_writes()
    }

    pub async fn block_full_write_reason(&self) -> Option<String> {
        let status = self.status().await;
        (!status.allows_full_writes).then(|| {
            format!(
                "HA role {:?} is degraded; this operation requires full_master",
                status.role
            )
        })
    }

    pub async fn promote_self_to_provisional(&self, force: bool) -> Result<HaStatusView, String> {
        self.promote_self_to_provisional_with_audit(force)
            .await
            .map(|(status, _audit)| status)
    }

    pub async fn promote_self_to_provisional_with_audit(
        &self,
        force: bool,
    ) -> Result<(HaStatusView, Vec<EdgeOneAuditEntry>), String> {
        if self.config.mode == HaMode::Single {
            return Ok((self.status().await, Vec::new()));
        }
        let mut audit = Vec::new();
        let target_origin = self
            .config
            .configured_node_origin()?
            .ok_or_else(|| "NODE_PUBLIC_HOST is required for HA promote".to_string())?;
        if !force && self.role().await != HaNodeRole::Standby {
            return Err("promote requires standby role unless force is set".to_string());
        }
        if !force {
            let (current, entry) = self.edgeone.describe_current_origin_with_audit().await?;
            audit.push(entry);
            if self.is_self_origin(current.as_deref()) {
                let mut state = self.state.write().await;
                state.edgeone_origin = current;
                state.last_edgeone_check_at = Some(Utc::now().timestamp());
            } else if let Some(expected) = self.config.configured_expected_origin()?
                && !current
                    .as_deref()
                    .and_then(|current| PublicOrigin::parse(current, expected.scheme).ok())
                    .is_some_and(|current| current.equivalent_to(&expected))
            {
                return Err(format!(
                    "EdgeOne origin is {:?}, expected {}; refusing promote without force",
                    current,
                    expected.authority()
                ));
            } else if self.config.configured_expected_origin()?.is_none() {
                return Err(format!(
                    "EdgeOne origin is {:?}, expected origin is not configured; refusing promote without force",
                    current
                ));
            }
        }
        let entry = self
            .edgeone
            .modify_origin_with_audit(&target_origin)
            .await?;
        audit.push(entry);
        let mut state = self.state.write().await;
        state.role = HaNodeRole::ProvisionalMaster;
        state.edgeone_origin = Some(target_origin.authority());
        state.last_edgeone_check_at = Some(Utc::now().timestamp());
        state.message = Some("promoted by EdgeOne origin switch; finalize required".to_string());
        drop(state);
        Ok((self.status().await, audit))
    }

    pub async fn finalize_failover(&self) -> Result<HaStatusView, String> {
        let mut state = self.state.write().await;
        if state.role != HaNodeRole::ProvisionalMaster {
            return Err("finalize requires provisional_master role".to_string());
        }
        state.role = HaNodeRole::FullMaster;
        state.message = Some("failover finalized by administrator".to_string());
        drop(state);
        Ok(self.status().await)
    }

    pub async fn enter_recovery(&self, message: String) -> HaStatusView {
        let mut state = self.state.write().await;
        state.role = HaNodeRole::Recovery;
        state.recovery_status = Some(message);
        drop(state);
        self.status().await
    }

    fn is_self_origin(&self, origin: Option<&str>) -> bool {
        match (origin, self.config.configured_node_origin().ok().flatten()) {
            (Some(current), Some(self_origin)) => PublicOrigin::parse(current, self_origin.scheme)
                .ok()
                .is_some_and(|current| current.equivalent_to(&self_origin)),
            _ => false,
        }
    }
}

pub fn sha256_hex_bytes(value: &[u8]) -> String {
    hex_sha256(value)
}

#[derive(Clone)]
struct EdgeOneClient {
    config: HaConfig,
    http: reqwest::Client,
}

impl EdgeOneClient {
    fn new(config: HaConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    async fn describe_current_origin(&self) -> Result<Option<String>, String> {
        self.describe_current_origin_with_audit()
            .await
            .map(|(origin, _audit)| origin)
    }

    async fn describe_current_origin_with_audit(
        &self,
    ) -> Result<(Option<String>, EdgeOneAuditEntry), String> {
        if !self.config.active_standby_ready() {
            return Ok((
                None,
                EdgeOneAuditEntry {
                    action: "DescribeAccelerationDomains".to_string(),
                    request_json: None,
                    response_json: None,
                    status: "skipped".to_string(),
                    message: Some("EdgeOne HA configuration is incomplete".to_string()),
                },
            ));
        }
        let zone_id = self.config.edgeone_zone_id.as_deref().unwrap_or_default();
        let domain = self.config.edgeone_domain.as_deref().unwrap_or_default();
        let payload = json!({
            "ZoneId": zone_id,
            "Filters": [
                { "Name": "domain-name", "Values": [domain] }
            ]
        });
        let (value, audit) = self
            .call_with_audit("DescribeAccelerationDomains", payload)
            .await?;
        let origin_detail = value.pointer("/Response/AccelerationDomains/0/OriginDetail");
        Ok((
            origin_detail
                .and_then(|detail| {
                    origin_detail_to_public_origin(detail, self.config.configured_origin_scheme())
                })
                .map(|origin| origin.authority()),
            audit,
        ))
    }

    async fn modify_origin_with_audit(
        &self,
        target_origin: &PublicOrigin,
    ) -> Result<EdgeOneAuditEntry, String> {
        if !self.config.active_standby_ready() {
            return Err("EdgeOne credentials and domain configuration are required".to_string());
        }
        let mut payload = json!({
            "ZoneId": self.config.edgeone_zone_id.as_deref().unwrap_or_default(),
            "DomainName": self.config.edgeone_domain.as_deref().unwrap_or_default(),
            "OriginProtocol": target_origin.scheme.edgeone_value(),
            "OriginInfo": {
                "OriginType": "ip_domain",
                "Origin": target_origin.host,
                "BackupOrigin": ""
            }
        });
        match target_origin.scheme {
            OriginScheme::Http => payload["HttpOriginPort"] = json!(target_origin.port),
            OriginScheme::Https => payload["HttpsOriginPort"] = json!(target_origin.port),
            OriginScheme::Follow => {
                payload["HttpOriginPort"] = json!(target_origin.port);
                payload["HttpsOriginPort"] = json!(target_origin.port);
            }
        }
        let (_value, audit) = self
            .call_with_audit("ModifyAccelerationDomain", payload)
            .await?;
        Ok(audit)
    }

    async fn call_with_audit(
        &self,
        action: &str,
        payload: Value,
    ) -> Result<(Value, EdgeOneAuditEntry), String> {
        let endpoint = self.config.edgeone_api_endpoint.trim();
        let host = endpoint
            .strip_prefix("https://")
            .or_else(|| endpoint.strip_prefix("http://"))
            .unwrap_or(endpoint)
            .trim_end_matches('/');
        let body = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
        let timestamp = Utc::now().timestamp();
        let auth = tc3_authorization(
            self.config.edgeone_secret_id.as_deref().unwrap_or_default(),
            self.config
                .edgeone_secret_key
                .as_deref()
                .unwrap_or_default(),
            "teo",
            host,
            action,
            timestamp,
            &body,
        )?;
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
        headers.insert(
            "Host",
            HeaderValue::from_str(host).map_err(|err| err.to_string())?,
        );
        headers.insert(
            "X-TC-Action",
            HeaderValue::from_str(action).map_err(|err| err.to_string())?,
        );
        headers.insert("X-TC-Version", HeaderValue::from_static("2022-09-01"));
        headers.insert(
            "X-TC-Timestamp",
            HeaderValue::from_str(&timestamp.to_string()).map_err(|err| err.to_string())?,
        );
        headers.insert(
            "Authorization",
            HeaderValue::from_str(&auth).map_err(|err| err.to_string())?,
        );
        let response = self
            .http
            .post(endpoint)
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|err| err.to_string())?;
        let status = response.status();
        let text = response.text().await.map_err(|err| err.to_string())?;
        if !status.is_success() {
            return Err(format!("EdgeOne {action} failed with {status}: {text}"));
        }
        let value: Value = serde_json::from_str(&text).map_err(|err| err.to_string())?;
        if let Some(error) = value.pointer("/Response/Error") {
            return Err(format!("EdgeOne {action} error: {error}"));
        }
        let audit = EdgeOneAuditEntry {
            action: action.to_string(),
            request_json: Some(serde_json::to_string(&payload).map_err(|err| err.to_string())?),
            response_json: Some(text),
            status: "success".to_string(),
            message: None,
        };
        Ok((value, audit))
    }
}

fn origin_detail_to_public_origin(
    origin_detail: &Value,
    default_scheme: OriginScheme,
) -> Option<PublicOrigin> {
    let origin = origin_detail.get("Origin")?.as_str()?.trim();
    if origin.is_empty() {
        return None;
    }
    let scheme = origin_detail
        .get("OriginProtocol")
        .and_then(Value::as_str)
        .and_then(OriginScheme::parse)
        .unwrap_or(default_scheme);
    let port = match scheme {
        OriginScheme::Http => origin_detail.get("HttpOriginPort").and_then(Value::as_i64),
        OriginScheme::Https => origin_detail.get("HttpsOriginPort").and_then(Value::as_i64),
        OriginScheme::Follow => origin_detail
            .get("HttpsOriginPort")
            .or_else(|| origin_detail.get("HttpOriginPort"))
            .and_then(Value::as_i64),
    }
    .and_then(|port| u16::try_from(port).ok())
    .filter(|port| *port > 0)
    .unwrap_or_else(|| scheme.default_port());
    Some(PublicOrigin {
        scheme,
        host: origin.to_string(),
        port,
    })
}

fn tc3_authorization(
    secret_id: &str,
    secret_key: &str,
    service: &str,
    host: &str,
    _action: &str,
    timestamp: i64,
    payload: &str,
) -> Result<String, String> {
    if secret_id.is_empty() || secret_key.is_empty() {
        return Err("EdgeOne secret id/key are required".to_string());
    }
    let date = chrono::DateTime::from_timestamp(timestamp, 0)
        .ok_or_else(|| "invalid timestamp".to_string())?
        .format("%Y-%m-%d")
        .to_string();
    let hashed_payload = hex_sha256(payload.as_bytes());
    let canonical_request = format!(
        "POST\n/\n\ncontent-type:application/json; charset=utf-8\nhost:{host}\n\ncontent-type;host\n{hashed_payload}"
    );
    let credential_scope = format!("{date}/{service}/tc3_request");
    let hashed_canonical_request = hex_sha256(canonical_request.as_bytes());
    let string_to_sign =
        format!("TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{hashed_canonical_request}");
    let secret_date = hmac_sha256(format!("TC3{secret_key}").as_bytes(), date.as_bytes());
    let secret_service = hmac_sha256(&secret_date, service.as_bytes());
    let secret_signing = hmac_sha256(&secret_service, b"tc3_request");
    let signature = encode_hex(&hmac_sha256(&secret_signing, string_to_sign.as_bytes()));
    Ok(format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{credential_scope}, SignedHeaders=content-type;host, Signature={signature}"
    ))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, data).as_ref().to_vec()
}

fn hex_sha256(data: &[u8]) -> String {
    encode_hex(&Sha256::digest(data))
}

fn encode_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OriginScheme {
    Http,
    Https,
    Follow,
}

impl OriginScheme {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "http" => Some(Self::Http),
            "https" => Some(Self::Https),
            "follow" | "match" | "request" => Some(Self::Follow),
            _ => None,
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::Http => 80,
            Self::Https | Self::Follow => 443,
        }
    }

    fn edgeone_value(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Https => "HTTPS",
            Self::Follow => "FOLLOW",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PublicOrigin {
    scheme: OriginScheme,
    host: String,
    port: u16,
}

impl PublicOrigin {
    fn parse(raw: &str, default_scheme: OriginScheme) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err("origin is empty".to_string());
        }
        let (scheme, rest) = if let Some((scheme_raw, rest)) = trimmed.split_once("://") {
            (
                OriginScheme::parse(scheme_raw)
                    .ok_or_else(|| format!("invalid origin scheme: {scheme_raw}"))?,
                rest,
            )
        } else {
            (default_scheme, trimmed)
        };
        let (host, port) = split_origin_host_port(rest, scheme)?;
        Ok(Self { scheme, host, port })
    }

    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn equivalent_to(&self, other: &Self) -> bool {
        self.scheme == other.scheme
            && self.host.eq_ignore_ascii_case(&other.host)
            && self.port == other.port
    }
}

fn split_origin_host_port(origin: &str, scheme: OriginScheme) -> Result<(String, u16), String> {
    let trimmed = origin.trim();
    let Some((host, port_raw)) = trimmed.rsplit_once(':') else {
        return Ok((trimmed.to_string(), scheme.default_port()));
    };
    if host.is_empty() || port_raw.is_empty() || host.contains(']') {
        return Ok((trimmed.to_string(), scheme.default_port()));
    }
    let port = port_raw
        .parse::<u16>()
        .map_err(|_| format!("invalid origin port: {origin}"))?;
    if port == 0 {
        return Err(format!("origin port out of range: {origin}"));
    }
    Ok((host.to_string(), port))
}

impl HaConfig {
    fn configured_origin_scheme(&self) -> OriginScheme {
        self.node_public_scheme
            .as_deref()
            .and_then(OriginScheme::parse)
            .unwrap_or(OriginScheme::Https)
    }

    fn configured_node_origin(&self) -> Result<Option<PublicOrigin>, String> {
        let scheme = self.configured_origin_scheme();
        Ok(self
            .node_public_host
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|host| PublicOrigin {
                scheme,
                host: host.to_string(),
                port: self
                    .node_public_port
                    .unwrap_or_else(|| scheme.default_port()),
            }))
    }

    fn configured_expected_origin(&self) -> Result<Option<PublicOrigin>, String> {
        let scheme = self
            .edgeone_expected_origin_scheme
            .as_deref()
            .and_then(OriginScheme::parse)
            .unwrap_or_else(|| self.configured_origin_scheme());
        Ok(self
            .edgeone_expected_origin_host
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|host| PublicOrigin {
                scheme,
                host: host.to_string(),
                port: self
                    .edgeone_expected_origin_port
                    .unwrap_or_else(|| scheme.default_port()),
            }))
    }

    fn canonical_expected_origin(&self) -> Option<String> {
        self.configured_expected_origin()
            .ok()
            .flatten()
            .map(|origin| origin.authority())
            .or_else(|| self.edgeone_expected_origin_host.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn single_mode_starts_full_master() {
        let runtime = HaRuntime::new(HaConfig::default());
        let status = runtime.status().await;
        assert_eq!(status.role, HaNodeRole::FullMaster);
        assert!(status.allows_basic_business);
        assert!(status.allows_full_writes);
    }

    #[tokio::test]
    async fn finalize_requires_provisional_master() {
        let runtime = HaRuntime::new(HaConfig {
            mode: HaMode::ActiveStandby,
            node_id: "n1".to_string(),
            node_public_host: Some("127.0.0.1".to_string()),
            node_public_port: Some(58087),
            ..HaConfig::default()
        });
        let err = runtime
            .finalize_failover()
            .await
            .expect_err("standby cannot be finalized");
        assert!(err.contains("provisional_master"));
    }

    #[test]
    fn tc3_authorization_contains_action_scope() {
        let auth = tc3_authorization(
            "sid",
            "skey",
            "teo",
            "teo.intl.tencentcloudapi.com",
            "DescribeAccelerationDomains",
            1_700_000_000,
            "{}",
        )
        .expect("sign");
        assert!(auth.contains("Credential=sid/2023-11-14/teo/tc3_request"));
        assert!(auth.contains("SignedHeaders=content-type;host"));
    }

    #[test]
    fn split_origin_host_port_moves_port_out_of_origin() {
        assert_eq!(
            split_origin_host_port("203.0.113.10:58087", OriginScheme::Https).expect("split"),
            ("203.0.113.10".to_string(), 58087)
        );
        assert_eq!(
            split_origin_host_port("origin.example.com", OriginScheme::Https).expect("split"),
            ("origin.example.com".to_string(), 443)
        );
    }

    #[test]
    fn origin_detail_to_authority_restores_port() {
        let detail = json!({
            "Origin": "203.0.113.10",
            "HttpOriginPort": 58087,
            "HttpsOriginPort": 58087
        });
        assert_eq!(
            origin_detail_to_public_origin(&detail, OriginScheme::Https)
                .map(|origin| origin.authority())
                .as_deref(),
            Some("203.0.113.10:58087")
        );
    }

    #[test]
    fn origin_detail_defaults_https_port_when_edgeone_omits_port() {
        let detail = json!({
            "Origin": "gz.ivanli.cc",
            "OriginProtocol": "HTTPS"
        });
        assert_eq!(
            origin_detail_to_public_origin(&detail, OriginScheme::Https)
                .map(|origin| origin.authority())
                .as_deref(),
            Some("gz.ivanli.cc:443")
        );
    }

    #[test]
    fn structured_node_origin_requires_explicit_host_port() {
        let config = HaConfig {
            node_public_scheme: Some("https".to_string()),
            node_public_host: Some("gz.ivanli.cc".to_string()),
            node_public_port: Some(443),
            ..HaConfig::default()
        };
        assert_eq!(
            config
                .configured_node_origin()
                .expect("origin parse")
                .expect("origin")
                .authority(),
            "gz.ivanli.cc:443"
        );
    }

    #[test]
    fn public_origin_equivalence_requires_same_scheme() {
        let https_origin =
            PublicOrigin::parse("gz.ivanli.cc:443", OriginScheme::Https).expect("https origin");
        let http_origin =
            PublicOrigin::parse("gz.ivanli.cc:443", OriginScheme::Http).expect("http origin");

        assert!(!https_origin.equivalent_to(&http_origin));
    }
}

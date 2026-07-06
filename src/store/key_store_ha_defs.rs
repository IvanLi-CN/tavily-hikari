#[derive(Clone, Debug)]
pub struct HaBaselineExport {
    pub channel: HaSyncChannel,
    pub ndjson: String,
    pub high_watermark: i64,
    pub row_count: usize,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HaEventRecord {
    pub channel: HaSyncChannel,
    pub seq: i64,
    pub kind: String,
    pub resource: String,
    pub resource_id: String,
    pub op: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
    pub checksum: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HaApplyResult {
    pub channel: HaSyncChannel,
    pub high_watermark: i64,
    pub row_count: usize,
    pub payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HaBaselineApplyMode {
    Replace,
    Upsert,
}

#[derive(Debug)]
pub struct HaBaselineApplySession {
    channel: HaSyncChannel,
    conn: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    high_watermark: i64,
    row_count: usize,
    payload_bytes: usize,
    saw_start: bool,
    saw_end: bool,
}

#[derive(Debug)]
pub struct HaBaselineReadSession {
    channel: HaSyncChannel,
    conn: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    generated_at: i64,
}

#[derive(Debug)]
pub struct HaEventsReadSession {
    channel: HaSyncChannel,
    conn: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    generated_at: i64,
}

#[derive(Debug)]
pub struct HaEventsApplySession {
    channel: HaSyncChannel,
    conn: sqlx::pool::PoolConnection<sqlx::Sqlite>,
    high_watermark: i64,
    row_count: usize,
    payload_bytes: usize,
    saw_start: bool,
    saw_end: bool,
}

const HA_SCHEMA_VERSION: i64 = 2;
const HA_CONTROL_OUTBOX_RETENTION_SECS: i64 = 72 * 60 * 60;
const HA_CHANNEL_EXPORT_RETENTION_SECS: i64 = 92 * 24 * 60 * 60;
const HA_CONTROL_PLANE_EVENT_RETENTION_SECS: i64 = 7 * 24 * 60 * 60;

const HA_CONTROL_BASELINE_TABLES: &[&str] = &[
    "admin_password_settings",
    "admin_passkey_challenges",
    "admin_passkey_credentials",
    "admin_passkey_reset_tokens",
    "admin_passkey_sessions",
    "announcements",
    "account_entitlements",
    "api_key_low_quota_depletions",
    "api_key_maintenance_records",
    "api_key_quarantines",
    "api_keys",
    "auth_tokens",
    "forward_proxy_settings",
    "linuxdo_credit_recharge_entitlements",
    "linuxdo_credit_recharge_orders",
    "meta",
    "oauth_accounts",
    "token_api_key_bindings",
    "user_api_key_bindings",
    "user_tag_bindings",
    "user_tags",
    "user_token_bindings",
    "users",
];

const HA_CONTROL_EVENT_TABLES: &[&str] = &[
    "admin_password_settings",
    "admin_passkey_challenges",
    "admin_passkey_credentials",
    "admin_passkey_reset_tokens",
    "admin_passkey_sessions",
    "announcements",
    "account_entitlements",
    "api_key_low_quota_depletions",
    "api_key_maintenance_records",
    "api_key_quarantines",
    "api_keys",
    "auth_tokens",
    "forward_proxy_settings",
    "linuxdo_credit_recharge_entitlements",
    "linuxdo_credit_recharge_orders",
    "meta",
    "oauth_accounts",
    "token_api_key_bindings",
    "user_api_key_bindings",
    "user_tag_bindings",
    "user_tags",
    "user_token_bindings",
    "users",
];

const HA_BILLING_BASELINE_TABLES: &[&str] = &["billing_ledger"];

const HA_RUNTIME_BASELINE_TABLES: &[&str] = &[
    "account_monthly_quota",
    "account_quota_limits",
    "account_usage_buckets",
    "auth_token_quota",
    "forward_proxy_key_affinity",
    "forward_proxy_node_overrides",
    "http_project_api_key_affinity",
    "mcp_sessions",
    "research_requests",
    "token_primary_api_key_affinity",
    "token_usage_buckets",
    "user_primary_api_key_affinity",
];

const HA_RUNTIME_EVENT_TABLES: &[&str] = &[
    "account_monthly_quota",
    "account_quota_limits",
    "account_usage_buckets",
    "auth_token_quota",
    "forward_proxy_key_affinity",
    "forward_proxy_node_overrides",
    "http_project_api_key_affinity",
    "mcp_sessions",
    "research_requests",
    "token_primary_api_key_affinity",
    "token_usage_buckets",
    "user_primary_api_key_affinity",
];

const HA_META_KEYS: &[&str] = &[
    "allow_registration_v1",
    "admin_totp_enabled_at_v1",
    "admin_totp_secret_ciphertext_v1",
    "admin_totp_secret_nonce_v1",
    "api_rebalance_enabled_v1",
    "api_rebalance_percent_v1",
    "global_ip_limit_v1",
    "ha_full_master_node_id_v1",
    "mcp_session_affinity_key_count_v1",
    "rebalance_mcp_enabled_v1",
    "rebalance_mcp_session_percent_v1",
    "recharge_feature_enabled_v1",
    "recharge_user_enabled_v1",
    "request_rate_limit_v1",
    "trusted_client_ip_headers_v1",
    "trusted_proxy_cidrs_v1",
    "user_blocked_key_base_limit_v1",
];

fn ha_baseline_tables(channel: HaSyncChannel) -> &'static [&'static str] {
    match channel {
        HaSyncChannel::Control => HA_CONTROL_BASELINE_TABLES,
        HaSyncChannel::Billing => HA_BILLING_BASELINE_TABLES,
        HaSyncChannel::Runtime => HA_RUNTIME_BASELINE_TABLES,
    }
}

fn ha_resource_allowed_for_channel(channel: HaSyncChannel, resource: &str) -> bool {
    if ha_baseline_tables(channel).contains(&resource) {
        return true;
    }
    if channel == HaSyncChannel::Control && resource == "meta" {
        return true;
    }
    false
}

fn ha_channel_event_table(channel: HaSyncChannel) -> &'static str {
    match channel {
        HaSyncChannel::Control => "ha_outbox",
        HaSyncChannel::Billing => "ha_billing_outbox",
        HaSyncChannel::Runtime => "ha_runtime_outbox",
    }
}

fn ha_channel_sequence_name(channel: HaSyncChannel) -> &'static str {
    ha_channel_event_table(channel)
}

fn ha_channel_event_tables(channel: HaSyncChannel) -> &'static [&'static str] {
    match channel {
        HaSyncChannel::Control => HA_CONTROL_EVENT_TABLES,
        HaSyncChannel::Billing => HA_BILLING_BASELINE_TABLES,
        HaSyncChannel::Runtime => HA_RUNTIME_EVENT_TABLES,
    }
}

fn ha_channel_retention_secs(channel: HaSyncChannel) -> i64 {
    match channel {
        HaSyncChannel::Control => HA_CONTROL_OUTBOX_RETENTION_SECS,
        HaSyncChannel::Billing | HaSyncChannel::Runtime => HA_CHANNEL_EXPORT_RETENTION_SECS,
    }
}

fn ha_channel_outbox_trigger_prefixes(channel: HaSyncChannel) -> Vec<String> {
    let mut prefixes = vec![format!("trg_ha_{}_", channel.as_str())];
    if channel == HaSyncChannel::Control {
        prefixes.push("trg_ha_outbox_".to_string());
    }
    prefixes
}

fn ha_channel_allowed_resources(channel: HaSyncChannel) -> &'static [&'static str] {
    ha_baseline_tables(channel)
}

fn ha_channel_allowed_resources_sql(channel: HaSyncChannel) -> String {
    ha_channel_event_tables(channel)
        .iter()
        .map(|resource| quote_sqlite_string(resource))
        .collect::<Vec<_>>()
        .join(", ")
}

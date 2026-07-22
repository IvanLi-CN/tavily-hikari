use crate::models::{UpstreamPrivacyGate, UpstreamReconciliationAdjustment};
use crate::upstream_privacy::UpstreamProjectIdMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamReconciliationRetryBuckets {
    pub upstream_429: i64,
    pub local_usage_rate_limit: i64,
    pub other: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamKeyActivityPoint {
    pub key_id_hint: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamPrivacyStatus {
    pub phase: String,
    pub configured_project_id_mode: UpstreamProjectIdMode,
    pub effective_project_id_mode: UpstreamProjectIdMode,
    pub fixed_project_id_configured: bool,
    pub configured_mcp_user_agent: String,
    pub effective_mcp_user_agent: Option<String>,
    pub upstream_precise_reconciliation_enabled: bool,
    pub http_allowed_headers: Vec<String>,
    pub control_mcp_allowed_headers: Vec<String>,
    pub gates: Vec<UpstreamPrivacyGate>,
    pub completed_gates: i64,
    pub total_gates: i64,
    pub active_upstream_mcp_sessions: i64,
    pub current_period_code: String,
    pub current_period_ends_at: i64,
    pub next_epoch_at: Option<i64>,
    pub pending_research: i64,
    pub queued_settlements: i64,
    pub degraded_settlements: i64,
    pub last_reconciliation_run_at: Option<i64>,
    pub last_shadow_adjustment_at: Option<i64>,
    pub last_reconciliation_enqueue_error_at: Option<i64>,
    pub retry_buckets: UpstreamReconciliationRetryBuckets,
    pub current_period_bound_users_by_key: Vec<UpstreamKeyActivityPoint>,
    pub current_period_pending_project_ids_by_key: Vec<UpstreamKeyActivityPoint>,
    pub recent_adjustments: Vec<UpstreamReconciliationAdjustment>,
    pub generated_at: i64,
}

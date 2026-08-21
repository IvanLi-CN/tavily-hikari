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
pub struct DailyReconciliationProgress {
    pub observed_accounts: i64,
    pub accounts_with_settled_period: i64,
    pub fully_terminal_accounts: i64,
    pub observed_periods: i64,
    pub settled_periods: i64,
    pub degraded_periods: i64,
    pub pending_periods: i64,
    pub research_total: i64,
    pub research_terminal: i64,
    pub research_pending: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DailyReconciliationKeyProgress {
    pub key_id_hint: String,
    pub terminal_research: i64,
    pub pending_research: i64,
    pub pending_project_ids: i64,
    pub cooldown_until: Option<i64>,
    pub cooldown_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationObservation {
    pub observed_at: Option<i64>,
    pub coverage: String,
    pub queue_estimate: Option<i64>,
    pub has_eligible: bool,
    pub oldest_candidate_age_secs: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationLocalBackoff {
    pub pressure_streak: i64,
    pub level: i64,
    pub available_at: Option<i64>,
    pub last_recovered_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReconciliationRunObservation {
    pub mode: String,
    pub projection_state: String,
    pub projection_scanned_rows: i64,
    pub projection_batch_size: i64,
    pub projection_transaction_p95_ms: i64,
    pub cursor_advanced: bool,
    pub hydrate_ms: i64,
    pub first_remote_ms: Option<i64>,
    pub remote_ms: i64,
    pub finalization_ms: i64,
    pub research_ms: i64,
    pub settled: i64,
    pub no_adjustment: i64,
    pub observed: i64,
    pub upstream_429: i64,
    pub transport_failure: i64,
    pub semantic_failure: i64,
    pub local_pressure: i64,
    pub last_transport_kind: Option<String>,
    #[serde(default)]
    pub last_transport_kind_at: Option<i64>,
    #[serde(default)]
    pub last_retryable_outcome: Option<String>,
    pub continuation_reason: Option<String>,
    pub next_retry_at: Option<i64>,
    pub observed_at: Option<i64>,
}

/// A completed, fixed-duration observation of current-period Research. A
/// window is healthy only when it observes terminal progress without pending
/// Research growing; upstream 429 remains a separate retry diagnostic.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReconciliationResearchProgressWindow {
    pub window_started_at: Option<i64>,
    pub window_ended_at: Option<i64>,
    pub window_seconds: i64,
    pub terminal_delta: i64,
    pub pending_delta: i64,
    pub terminal_rate_positive: bool,
    pub pending_non_growing: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ReconciliationControllerStatus {
    pub mode: String,
    pub activation_period_code: Option<String>,
    pub activation_period_start: Option<i64>,
    pub legacy_active: bool,
    pub paused_reason: Option<String>,
    pub transitioned_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct DashboardAlertProjectionStatus {
    pub coverage: String,
    pub observed_at: Option<i64>,
    pub stale_reason: Option<String>,
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
    /// Current-day pending Research. It remains distinct from the bounded
    /// settlement queue observation below.
    pub pending_research: Option<i64>,
    /// A bounded queue estimate. `None` means this status snapshot has not
    /// observed a bounded estimate and must not be rendered as zero.
    pub queued_settlements: Option<i64>,
    pub degraded_settlements: i64,
    /// `true` when the bounded degraded settlement observation has more rows.
    pub degraded_settlements_capped: bool,
    pub last_reconciliation_run_at: Option<i64>,
    pub last_shadow_adjustment_at: Option<i64>,
    pub last_reconciliation_enqueue_error_at: Option<i64>,
    pub last_research_sweep_at: Option<i64>,
    pub last_research_terminal_at: Option<i64>,
    pub reconciliation_pressure_streak: i64,
    pub reconciliation_backoff_level: i64,
    pub reconciliation_backoff_until: Option<i64>,
    pub reconciliation_last_duration_ms: Option<i64>,
    pub reconciliation_last_attempted: i64,
    pub reconciliation_last_settled: i64,
    #[serde(default)]
    pub reconciliation_last_no_adjustment: i64,
    pub reconciliation_last_upstream_429: i64,
    pub reconciliation_last_budget_exhausted: bool,
    pub reconciliation_observation: ReconciliationObservation,
    pub reconciliation_local_backoff: ReconciliationLocalBackoff,
    #[serde(default)]
    pub reconciliation_run_observation: ReconciliationRunObservation,
    #[serde(default)]
    pub reconciliation_research_progress_window: ReconciliationResearchProgressWindow,
    #[serde(default)]
    pub reconciliation_controller: ReconciliationControllerStatus,
    #[serde(default)]
    pub dashboard_alert_projection: DashboardAlertProjectionStatus,
    pub retry_buckets: UpstreamReconciliationRetryBuckets,
    pub current_period_bound_users_by_key: Vec<UpstreamKeyActivityPoint>,
    pub current_period_pending_project_ids_by_key: Vec<UpstreamKeyActivityPoint>,
    pub daily_reconciliation_progress: DailyReconciliationProgress,
    pub daily_reconciliation_by_key: Vec<DailyReconciliationKeyProgress>,
    pub recent_adjustments: Vec<UpstreamReconciliationAdjustment>,
    pub generated_at: i64,
    #[serde(default)]
    pub coverage: String,
    #[serde(default)]
    pub observed_at: Option<i64>,
    #[serde(default)]
    pub stale_reason: Option<String>,
}

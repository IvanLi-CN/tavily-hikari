use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AdminMcpSessionBindingFilterStatus {
    Active,
    Revoked,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdminMcpSessionBindingsQuery {
    pub status: AdminMcpSessionBindingFilterStatus,
    pub created_from: Option<i64>,
    pub created_to: Option<i64>,
    pub updated_from: Option<i64>,
    pub updated_to: Option<i64>,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdminMcpSessionBindingListItem {
    pub proxy_session_id: String,
    pub auth_token_id: Option<String>,
    pub user_id: Option<String>,
    pub upstream_key_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
    pub status: String,
    pub revoked_at: Option<i64>,
    pub revoke_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdminMcpSessionBindingsPage {
    pub items: Vec<AdminMcpSessionBindingListItem>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub active_matching_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdminMcpSessionBindingsRevokeResult {
    pub revoked_count: i64,
}

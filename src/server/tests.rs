#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::extract::{DefaultBodyLimit, Form, Json, Query, State};
    use axum::http::{HeaderMap, Method, Uri};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{any, delete, get, patch, post};
    use bytes::Bytes;
    use nanoid::nanoid;
    use reqwest::Client;
    use sha2::{Digest, Sha256};
    use sqlx::Connection;
    use sqlx::Row;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::future::pending;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tavily_hikari::{
        DEFAULT_UPSTREAM, ForwardProxySettings, effective_auth_token_log_retention_days,
        effective_request_logs_retention_days, effective_token_hourly_limit,
        request_rate_limit, request_rate_limit_window_minutes, request_rate_limit_window_secs,
    };
    use tokio::net::TcpListener;
    use tokio::sync::Notify;

    mod admin_logs_and_summary;
    mod admin_token_filters_and_maintenance;
    mod admin_users_and_tokens;
    mod alerts_and_ha;
    mod alerts_and_ha_event_exports;
    mod alerts_and_ha_serving_modes;
    mod alerts_and_ha_startup_roles;
    mod api_keys_and_registration;
    mod core_support_and_parsing;
    mod linuxdo_oauth_and_admin_keys;
    mod log_catalog_and_dashboard_sse;
    mod mcp_billing_and_sessions;
    mod mcp_rebalance_and_follow_up;
    mod research_result_and_mcp_subpath;
    mod system_settings_and_forward_proxy;
    mod tavily_http_search;
    mod token_log_details;
    mod upstream_support_and_manual_jobs;
}

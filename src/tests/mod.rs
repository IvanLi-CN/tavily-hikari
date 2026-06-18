use crate::analysis::*;
use crate::models::*;
use crate::store::*;
use crate::tavily_proxy::*;
use crate::*;

use axum::{
    Json, Router,
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{Local, TimeZone, Utc};
use nanoid::nanoid;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Connection, Row, SqlitePool};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;

mod account_quota_and_billing;
mod dashboard_month_series;
mod jobs_and_request_log_retention;
mod linuxdo_credit_recharge;
mod maintenance_and_mcp_affinity;
mod observability_and_lifecycle;
mod proxy_affinity_and_summary;
mod proxy_affinity_runtime_geo;
mod request_kind_and_core;
mod request_rollup;
mod support;
mod usage_series_and_backfills;
mod user_business_calls_1h;
mod user_tokens_and_pending_billing;

use support::*;

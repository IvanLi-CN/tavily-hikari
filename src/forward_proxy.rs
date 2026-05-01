#![allow(dead_code)]

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    io,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use base64::Engine;
use chrono::{TimeZone, Utc};
use reqwest::{Client, Proxy, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, QueryBuilder, Row, Sqlite, SqlitePool};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::{Child, Command},
    sync::{Mutex, RwLock},
    time::{sleep, timeout},
};

use crate::{ProxyError, build_path_prefixed_url, store::KeyStore};

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
const FORWARD_PROXY_WINDOW_STATS_CACHE_TTL_SECS: u64 = 5;
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
const XRAY_PROXY_READY_TIMEOUT_MS: u64 = 5_000;

include!("forward_proxy/settings_and_endpoints.rs");
include!("forward_proxy/manager.rs");
include!("forward_proxy/relay_and_xray.rs");
include!("forward_proxy/storage.rs");
include!("forward_proxy/responses_and_validation.rs");
include!("forward_proxy/tests.rs");

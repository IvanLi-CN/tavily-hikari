use crate::analysis::*;
use crate::models::*;
use crate::store::*;
use crate::tavily_proxy::*;
use crate::*;

use axum::{
    Json, Router,
    http::StatusCode,
    response::IntoResponse,
    routing::{any, get, post},
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::net::TcpListener;

fn env_lock() -> Arc<tokio::sync::Mutex<()>> {
    static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

fn dead_request_kind_migration_owner_pid() -> u32 {
    for candidate in [999_999_u32, 888_888, 777_777, 666_666] {
        if !request_kind_canonical_migration_owner_pid_is_live(candidate) {
            return candidate;
        }
    }
    panic!("unable to find a dead pid candidate for request-kind migration tests");
}

async fn spawn_api_key_geo_mock_server() -> SocketAddr {
    let app = Router::new().route(
        "/geo",
        post(|Json(ips): Json<Vec<String>>| async move {
            let entries = ips
                .into_iter()
                .map(|ip| match ip.as_str() {
                    "18.183.246.69" => serde_json::json!({
                        "ip": ip,
                        "country": "JP",
                        "city": "Tokyo",
                        "subdivision": "13"
                    }),
                    "1.1.1.1" => serde_json::json!({
                        "ip": ip,
                        "country": "HK",
                        "city": null,
                        "subdivision": null
                    }),
                    "1.0.0.1" => serde_json::json!({
                        "ip": ip,
                        "country": "HK",
                        "city": null,
                        "subdivision": null
                    }),
                    "8.8.8.8" => serde_json::json!({
                        "ip": ip,
                        "country": "US",
                        "city": null,
                        "subdivision": null
                    }),
                    _ => serde_json::json!({
                        "ip": ip,
                        "country": null,
                        "city": null,
                        "subdivision": null
                    }),
                })
                .collect::<Vec<_>>();
            Json(entries)
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn spawn_fake_forward_proxy_with_body(body: String) -> SocketAddr {
    let app = Router::new().fallback(any(move || {
        let body = body.clone();
        async move { (StatusCode::OK, body) }
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

#[test]
fn parse_hhmm_validates_clock_time() {
    assert_eq!(parse_hhmm("07:00"), Some((7, 0)));
    assert_eq!(parse_hhmm("23:59"), Some((23, 59)));
    assert_eq!(parse_hhmm("7:00"), None);
    assert_eq!(parse_hhmm("24:00"), None);
    assert_eq!(parse_hhmm("00:60"), None);
    assert_eq!(parse_hhmm(""), None);
    assert_eq!(parse_hhmm("07:00:00"), None);
}

#[test]
fn parse_explicit_today_window_accepts_rfc3339_midnight_pair() {
    let expected_start = chrono::DateTime::parse_from_rfc3339("2026-04-03T00:00:00+08:00")
        .expect("parse start")
        .with_timezone(&Utc)
        .timestamp();
    let expected_end = chrono::DateTime::parse_from_rfc3339("2026-04-04T00:00:00+08:00")
        .expect("parse end")
        .with_timezone(&Utc)
        .timestamp();
    let window = parse_explicit_today_window(
        Some("2026-04-03T00:00:00+08:00"),
        Some("2026-04-04T00:00:00+08:00"),
    )
    .expect("parse explicit today window")
    .expect("window present");

    assert_eq!(
        window,
        TimeRangeUtc {
            start: expected_start,
            end: expected_end,
        }
    );
}

#[test]
fn parse_explicit_today_window_requires_complete_pair() {
    let err = parse_explicit_today_window(Some("2026-04-03T00:00:00+08:00"), None)
        .expect_err("missing end should fail");
    assert!(err.contains("today_start and today_end must be provided together"));
}

#[test]
fn parse_explicit_today_window_requires_offset() {
    let err = parse_explicit_today_window(Some("2026-04-03T00:00:00"), Some("2026-04-04T00:00:00"))
        .expect_err("offset-less timestamps should fail");
    assert!(err.contains("today_start must be a valid ISO8601 datetime with offset"));
}

#[test]
fn parse_explicit_today_window_rejects_non_midnight_boundaries() {
    let err = parse_explicit_today_window(
        Some("2026-04-03T01:00:00+08:00"),
        Some("2026-04-04T00:00:00+08:00"),
    )
    .expect_err("non-midnight start should fail");
    assert!(err.contains("must align to local midnight"));
}

#[test]
fn parse_explicit_today_window_rejects_non_daily_ranges() {
    let err = parse_explicit_today_window(
        Some("2026-04-03T00:00:00+08:00"),
        Some("2026-04-05T00:00:00+08:00"),
    )
    .expect_err("two-day range should fail");
    assert!(err.contains("must describe exactly one natural-day window"));

    let err = parse_explicit_today_window(
        Some("2026-04-03T00:00:00+08:00"),
        Some("2026-04-03T00:00:00+08:00"),
    )
    .expect_err("zero-length range should fail");
    assert!(err.contains("must be later than today_start"));

    let err = parse_explicit_today_window(
        Some("2026-04-03T00:00:00+14:00"),
        Some("2026-04-04T00:00:00-12:00"),
    )
    .expect_err("mixed-offset multi-day utc span should fail");
    assert!(err.contains("must describe exactly one natural-day window"));
}

#[test]
fn parse_forward_proxy_trace_response_normalizes_ipv6_addresses() {
    let parsed = parse_forward_proxy_trace_response(
        "ip=2602:FEDA:F30F:DD6A:782D:DE80:6148:5EE2\nloc=US\ncolo=SJC\n",
    )
    .expect("trace response should parse");
    assert_eq!(
        parsed,
        (
            "2602:feda:f30f:dd6a:782d:de80:6148:5ee2".to_string(),
            "US / SJC".to_string(),
        )
    );
}

#[test]
fn extract_usage_credits_from_json_bytes_finds_nested_usage_and_rounds_up() {
    let body = br#"{"result":{"structuredContent":{"usage":{"credits":1.2}}}}"#;
    assert_eq!(extract_usage_credits_from_json_bytes(body), Some(2));
}

#[test]
fn map_forward_proxy_validation_error_code_distinguishes_invalid_subscriptions() {
    assert_eq!(
        map_forward_proxy_validation_error_code(&ProxyError::Other(
            "subscription contains no supported proxy entries".to_string(),
        )),
        "subscription_invalid"
    );
    assert_eq!(
        map_forward_proxy_validation_error_code(&ProxyError::Other(
            "subscription resolved zero proxy entries".to_string(),
        )),
        "subscription_invalid"
    );
}

#[test]
fn extract_usage_credits_from_json_bytes_parses_string_float_and_rounds_up() {
    let body = br#"{"usage":{"credits":"1.2"}}"#;
    assert_eq!(extract_usage_credits_from_json_bytes(body), Some(2));
}

#[test]
fn extract_usage_credits_from_json_bytes_supports_total_credits_exact() {
    let body = br#"{"usage":{"total_credits_exact":0.2}}"#;
    assert_eq!(extract_usage_credits_from_json_bytes(body), Some(1));
}

#[test]
fn extract_usage_credits_total_from_json_bytes_sums_total_credits_exact() {
    let body =
        br#"[{"usage":{"total_credits_exact":0.2}},{"usage":{"total_credits_exact":"1.2"}}]"#;
    assert_eq!(extract_usage_credits_total_from_json_bytes(body), Some(3));
}

#[test]
fn extract_usage_credits_total_from_json_bytes_sums_across_arrays() {
    let body = br#"[{"result":{"structuredContent":{"usage":{"credits":1}}}},{"result":{"structuredContent":{"usage":{"credits":2.1}}}}]"#;
    assert_eq!(extract_usage_credits_total_from_json_bytes(body), Some(4));
}

#[test]
fn extract_usage_credits_from_json_bytes_parses_sse_and_returns_max() {
    let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":2}}}}\n\n";
    assert_eq!(extract_usage_credits_from_json_bytes(body), Some(2));
}

#[test]
fn extract_usage_credits_total_from_json_bytes_parses_sse_and_sums_by_id() {
    // Duplicate id=1 message should not double count.
    let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":2}}}}\n\n";
    assert_eq!(extract_usage_credits_total_from_json_bytes(body), Some(3));
}

#[test]
fn extract_mcp_usage_credits_by_id_from_bytes_tracks_max_per_id() {
    let body = br#"
    [
      {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"usage":{"credits":1}}}},
      {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"usage":{"credits":2}}}},
      {"jsonrpc":"2.0","id":"abc","result":{"structuredContent":{"usage":{"credits":"3"}}}},
      {"jsonrpc":"2.0","id":null,"result":{"structuredContent":{"usage":{"credits":99}}}},
      {"jsonrpc":"2.0","id":2,"result":{"structuredContent":{"status":200}}}
    ]
    "#;

    let credits = extract_mcp_usage_credits_by_id_from_bytes(body);

    let id1 = serde_json::json!(1).to_string();
    let id_abc = serde_json::json!("abc").to_string();
    let id2 = serde_json::json!(2).to_string();

    assert_eq!(credits.get(&id1), Some(&2));
    assert_eq!(credits.get(&id_abc), Some(&3));
    assert_eq!(
        credits.get(&id2),
        None,
        "missing usage should not create a map entry"
    );
    assert!(
        !credits.values().any(|v| *v == 99),
        "null ids should be ignored"
    );
}

#[test]
fn extract_mcp_usage_credits_by_id_from_bytes_parses_sse() {
    let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":2}}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"structuredContent\":{\"usage\":{\"credits\":1}}}}\n\n";

    let credits = extract_mcp_usage_credits_by_id_from_bytes(body);

    let id1 = serde_json::json!(1).to_string();
    let id2 = serde_json::json!(2).to_string();
    assert_eq!(credits.get(&id1), Some(&2));
    assert_eq!(credits.get(&id2), Some(&1));
}

#[test]
fn extract_mcp_has_error_by_id_from_bytes_marks_error_and_quota_exhausted() {
    let body = br#"
    [
      {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"status":200}}},
      {"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"oops"}},
      {"jsonrpc":"2.0","id":3,"result":{"structuredContent":{"status":432}}}
    ]
    "#;

    let flags = extract_mcp_has_error_by_id_from_bytes(body);
    let id1 = serde_json::json!(1).to_string();
    let id2 = serde_json::json!(2).to_string();
    let id3 = serde_json::json!(3).to_string();

    assert_eq!(flags.get(&id1), Some(&false));
    assert_eq!(flags.get(&id2), Some(&true));
    assert_eq!(flags.get(&id3), Some(&true));
}

#[test]
fn extract_mcp_has_error_by_id_from_bytes_or_accumulates_across_sse() {
    let body = b"data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"structuredContent\":{\"status\":200}}}\n\n\
data: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000,\"message\":\"oops\"}}\n\n";

    let flags = extract_mcp_has_error_by_id_from_bytes(body);
    let id1 = serde_json::json!(1).to_string();
    assert_eq!(flags.get(&id1), Some(&true));
}

#[test]
fn analyze_mcp_attempt_marks_mixed_success_and_error_as_error() {
    let body = br#"[
      {"jsonrpc":"2.0","id":1,"result":{"structuredContent":{"status":200}}},
      {"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"oops"}}
    ]"#;

    let analysis = analyze_mcp_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn classify_token_request_kind_maps_http_routes_and_unknown_paths() {
    assert_eq!(
        classify_token_request_kind("/api/tavily/search", None),
        TokenRequestKind::new("api:search", "API | search", None)
    );
    assert_eq!(
        classify_token_request_kind("/api/tavily/research/req_123", None),
        TokenRequestKind::new("api:research-result", "API | research result", None)
    );
    assert_eq!(
        classify_token_request_kind("/api/custom/raw", None),
        TokenRequestKind::new(
            "api:unknown-path",
            "API | unknown path",
            Some("/api/custom/raw".to_string())
        )
    );
    assert_eq!(
        classify_token_request_kind("/mcp/sse", None),
        TokenRequestKind::new(
            "mcp:unsupported-path",
            "MCP | unsupported path",
            Some("/mcp/sse".to_string())
        )
    );
}

#[test]
fn classify_token_request_kind_maps_mcp_control_plane_and_canonical_unknowns() {
    let search_body = br#"{
      "jsonrpc": "2.0",
      "id": 1,
      "method": "tools/call",
      "params": {
        "name": "tavily-search"
      }
    }"#;
    assert_eq!(
        classify_token_request_kind("/mcp", Some(search_body)),
        TokenRequestKind::new("mcp:search", "MCP | search", None)
    );

    let tool_body = br#"{
      "jsonrpc": "2.0",
      "id": 2,
      "method": "tools/call",
      "params": {
        "name": "Acme Lookup"
      }
    }"#;
    assert_eq!(
        classify_token_request_kind("/mcp", Some(tool_body)),
        TokenRequestKind::new(
            "mcp:third-party-tool",
            "MCP | third-party tool",
            Some("Acme Lookup".to_string())
        )
    );

    let tool_variant_body = br#"{
      "jsonrpc": "2.0",
      "id": 3,
      "method": "tools/call",
      "params": {
        "name": "  acme_lookup  "
      }
    }"#;
    assert_eq!(
        classify_token_request_kind("/mcp", Some(tool_variant_body)),
        TokenRequestKind::new(
            "mcp:third-party-tool",
            "MCP | third-party tool",
            Some("acme_lookup".to_string())
        )
    );

    let init_body = br#"{
      "jsonrpc": "2.0",
      "id": 4,
      "method": "initialize"
    }"#;
    assert_eq!(
        classify_token_request_kind("/mcp", Some(init_body)),
        TokenRequestKind::new("mcp:initialize", "MCP | initialize", None)
    );
}

#[test]
fn classify_token_request_kind_maps_mcp_mixed_batch_to_batch_with_detail() {
    let mixed_batch = br#"[
      {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "tavily-search" }
      },
      {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "tavily-extract" }
      }
    ]"#;
    assert_eq!(
        classify_token_request_kind("/mcp", Some(mixed_batch)),
        TokenRequestKind::new(
            "mcp:batch",
            "MCP | batch",
            Some("search, extract".to_string())
        )
    );

    let same_batch = br#"[
      {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": "tavily-search" }
      },
      {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": { "name": "tavily_search" }
      }
    ]"#;
    assert_eq!(
        classify_token_request_kind("/mcp", Some(same_batch)),
        TokenRequestKind::new("mcp:search", "MCP | search", None)
    );
}

#[test]
fn request_value_bucket_classifies_known_kinds_and_batch_precedence() {
    assert_eq!(
        request_value_bucket_for_request_log("api:search", None),
        RequestValueBucket::Valuable
    );
    assert_eq!(
        request_value_bucket_for_request_log("api:research-result", None),
        RequestValueBucket::Valuable
    );
    assert_eq!(
        request_value_bucket_for_request_log("mcp:initialize", None),
        RequestValueBucket::Other
    );
    assert_eq!(
        request_value_bucket_for_request_log("api:unknown-path", None),
        RequestValueBucket::Unknown
    );

    let other_batch = br#"[
      {"jsonrpc":"2.0","id":1,"method":"initialize"},
      {"jsonrpc":"2.0","id":2,"method":"tools/list"}
    ]"#;
    assert_eq!(
        request_value_bucket_for_request_log("mcp:batch", Some(other_batch)),
        RequestValueBucket::Other
    );

    let valuable_batch = br#"[
      {"jsonrpc":"2.0","id":1,"method":"initialize"},
      {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"tavily-search"}}
    ]"#;
    assert_eq!(
        request_value_bucket_for_request_log("mcp:batch", Some(valuable_batch)),
        RequestValueBucket::Valuable
    );

    let unknown_batch = br#"[
      {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"tavily-search"}},
      {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"Acme Lookup"}}
    ]"#;
    assert_eq!(
        request_value_bucket_for_request_log("mcp:batch", Some(unknown_batch)),
        RequestValueBucket::Unknown
    );
}

#[test]
fn token_request_kind_option_groups_match_protocol_and_billing_contract() {
    assert_eq!(token_request_kind_protocol_group("api:search"), "api");
    assert_eq!(token_request_kind_protocol_group("mcp:search"), "mcp");

    assert_eq!(token_request_kind_billing_group("api:search"), "billable");
    assert_eq!(
        token_request_kind_billing_group("api:research-result"),
        "non_billable"
    );
    assert_eq!(token_request_kind_billing_group("mcp:search"), "billable");
    assert_eq!(
        token_request_kind_billing_group("mcp:tools/list"),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group("mcp:third-party-tool"),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group("mcp:unsupported-path"),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group("mcp:unknown-payload"),
        "non_billable"
    );
    assert_eq!(token_request_kind_billing_group("mcp:batch"), "billable");
    assert_eq!(
        token_request_kind_billing_group_for_token_log("mcp:unknown-payload", false),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group_for_token_log("mcp:unsupported-path", false),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group_for_request(
            "/mcp",
            Some(
                br#"[{"jsonrpc":"2.0","method":"initialize"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
            ),
        ),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group_for_request(
            "/mcp",
            Some(
                br#"[{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":"search","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"mixed batch"}}}]"#,
            ),
        ),
        "billable"
    );
    assert_eq!(
        token_request_kind_billing_group_for_request_log(
            "mcp:batch",
            Some(
                br#"[{"jsonrpc":"2.0","method":"initialize"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
            ),
        ),
        "non_billable"
    );

    assert_eq!(
        token_request_kind_option_billing_group("mcp:batch", false, true),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_option_billing_group("mcp:batch", true, true),
        "billable"
    );
    assert_eq!(
        token_request_kind_option_billing_group("api:search", false, true),
        "billable"
    );
    assert_eq!(
        canonical_request_kind_key_for_filter("mcp:raw:/mcp/sse"),
        "mcp:unsupported-path"
    );
    assert_eq!(
        canonical_request_kind_key_for_filter("mcp:cancel"),
        "mcp:unknown-method"
    );
    assert_eq!(
        canonical_request_kind_key_for_filter("api:custom"),
        "api:unknown-path"
    );
}

#[test]
fn operational_class_maps_control_plane_and_failure_kinds() {
    assert_eq!(
        normalize_operational_class_filter(Some("neutral")),
        Some(OPERATIONAL_CLASS_NEUTRAL)
    );
    assert_eq!(
        operational_class_for_request_kind("mcp:notifications/initialized", OUTCOME_UNKNOWN, None),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        operational_class_for_request_kind("mcp:search", OUTCOME_SUCCESS, None),
        OPERATIONAL_CLASS_SUCCESS
    );
    assert_eq!(
        operational_class_for_token_log("mcp:batch", OUTCOME_SUCCESS, None, false),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        operational_class_for_token_log("mcp:unknown-payload", OUTCOME_UNKNOWN, None, false),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        operational_class_for_token_log("mcp:unsupported-path", OUTCOME_SUCCESS, None, false),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        operational_class_for_request_path(
            "/mcp",
            Some(
                br#"[{"jsonrpc":"2.0","method":"initialize"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#
            ),
            OUTCOME_UNKNOWN,
            None,
        ),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        operational_class_for_request_log(
            "mcp:search",
            Some(br#"not-json"#),
            OUTCOME_SUCCESS,
            None
        ),
        OPERATIONAL_CLASS_SUCCESS
    );
    assert_eq!(
        operational_class_for_request_log(
            "mcp:notifications/initialized",
            Some(br#"not-json"#),
            OUTCOME_SUCCESS,
            None,
        ),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        operational_class_for_request_kind(
            "mcp:search",
            OUTCOME_ERROR,
            Some(FAILURE_KIND_MCP_ACCEPT_406),
        ),
        OPERATIONAL_CLASS_CLIENT_ERROR
    );
    assert_eq!(
        operational_class_for_request_kind(
            "mcp:extract",
            OUTCOME_ERROR,
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
        ),
        OPERATIONAL_CLASS_UPSTREAM_ERROR
    );
    assert_eq!(
        operational_class_for_request_kind("api:search", OUTCOME_ERROR, Some(FAILURE_KIND_OTHER),),
        OPERATIONAL_CLASS_SYSTEM_ERROR
    );
    assert_eq!(
        operational_class_for_request_kind("api:search", OUTCOME_QUOTA_EXHAUSTED, None),
        OPERATIONAL_CLASS_QUOTA_EXHAUSTED
    );
    assert_eq!(
        display_result_status_for_request_kind("mcp:session-delete-unsupported", OUTCOME_ERROR),
        OPERATIONAL_CLASS_NEUTRAL
    );
    assert_eq!(
        display_result_status_for_request_kind("mcp:unknown-payload", OUTCOME_ERROR),
        OUTCOME_ERROR
    );
}

#[test]
fn request_logs_env_settings_enforce_minimums_and_defaults() {
    let lock = env_lock();
    let _guard = lock.blocking_lock();
    let prev_days = std::env::var("REQUEST_LOGS_RETENTION_DAYS").ok();
    let prev_at = std::env::var("REQUEST_LOGS_GC_AT").ok();

    unsafe {
        std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "3");
    }
    assert_eq!(effective_request_logs_retention_days(), 32);

    unsafe {
        std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "40");
    }
    assert_eq!(effective_request_logs_retention_days(), 40);

    unsafe {
        std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "not-a-number");
        std::env::set_var("REQUEST_LOGS_GC_AT", "23:30");
    }
    assert_eq!(effective_request_logs_retention_days(), 32);
    assert_eq!(effective_request_logs_gc_at(), (23, 30));

    unsafe {
        std::env::set_var("REQUEST_LOGS_GC_AT", "7:00");
    }
    assert_eq!(effective_request_logs_gc_at(), (7, 0));

    unsafe {
        if let Some(v) = prev_days {
            std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", v);
        } else {
            std::env::remove_var("REQUEST_LOGS_RETENTION_DAYS");
        }
        if let Some(v) = prev_at {
            std::env::set_var("REQUEST_LOGS_GC_AT", v);
        } else {
            std::env::remove_var("REQUEST_LOGS_GC_AT");
        }
    }
}

#[test]
fn sanitize_headers_removes_blocked_and_keeps_allowed() {
    let upstream = Url::parse("https://mcp.tavily.com/mcp").unwrap();
    let origin = origin_from_url(&upstream);

    let mut headers = HeaderMap::new();
    headers.insert("X-Forwarded-For", HeaderValue::from_static("1.2.3.4"));
    headers.insert("Accept", HeaderValue::from_static("application/json"));

    let sanitized = sanitize_headers_inner(&headers, &upstream, &origin);
    assert!(!sanitized.headers.contains_key("X-Forwarded-For"));
    assert_eq!(
        sanitized.headers.get("Accept").unwrap(),
        &HeaderValue::from_static("application/json")
    );
    assert!(sanitized.dropped.contains(&"x-forwarded-for".to_string()));
    assert!(sanitized.forwarded.contains(&"accept".to_string()));
}

#[test]
fn sanitize_headers_rewrites_origin_and_referer() {
    let upstream = Url::parse("https://mcp.tavily.com:443/mcp").unwrap();
    let origin = origin_from_url(&upstream);

    let mut headers = HeaderMap::new();
    headers.insert("Origin", HeaderValue::from_static("https://proxy.local"));
    headers.insert(
        "Referer",
        HeaderValue::from_static("https://proxy.local/mcp/endpoint"),
    );

    let sanitized = sanitize_headers_inner(&headers, &upstream, &origin);
    assert_eq!(
        sanitized.headers.get("Origin").unwrap(),
        &HeaderValue::from_str(&origin).unwrap()
    );
    assert!(
        sanitized
            .headers
            .get("Referer")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(&origin)
    );
    assert!(sanitized.forwarded.contains(&"origin".to_string()));
    assert!(sanitized.forwarded.contains(&"referer".to_string()));
}

#[test]
fn sanitize_headers_keeps_mcp_session_recovery_headers() {
    let upstream = Url::parse("https://mcp.tavily.com/mcp").unwrap();
    let origin = origin_from_url(&upstream);

    let mut headers = HeaderMap::new();
    headers.insert("Mcp-Session-Id", HeaderValue::from_static("session-123"));
    headers.insert(
        "Mcp-Protocol-Version",
        HeaderValue::from_static("2025-03-26"),
    );
    headers.insert("Last-Event-Id", HeaderValue::from_static("resume-42"));
    headers.insert("X-Forwarded-For", HeaderValue::from_static("1.2.3.4"));
    headers.insert("X-Real-Ip", HeaderValue::from_static("1.2.3.4"));

    let sanitized = sanitize_headers_inner(&headers, &upstream, &origin);
    assert_eq!(
        sanitized.headers.get("mcp-session-id").unwrap(),
        &HeaderValue::from_static("session-123")
    );
    assert_eq!(
        sanitized.headers.get("mcp-protocol-version").unwrap(),
        &HeaderValue::from_static("2025-03-26")
    );
    assert_eq!(
        sanitized.headers.get("last-event-id").unwrap(),
        &HeaderValue::from_static("resume-42")
    );
    assert!(!sanitized.headers.contains_key("x-forwarded-for"));
    assert!(!sanitized.headers.contains_key("x-real-ip"));
    assert!(sanitized.forwarded.contains(&"mcp-session-id".to_string()));
    assert!(
        sanitized
            .forwarded
            .contains(&"mcp-protocol-version".to_string())
    );
    assert!(sanitized.forwarded.contains(&"last-event-id".to_string()));
    assert!(sanitized.dropped.contains(&"x-forwarded-for".to_string()));
    assert!(sanitized.dropped.contains(&"x-real-ip".to_string()));
}

#[test]
fn sanitize_mcp_headers_drops_fingerprint_headers_and_sets_proxy_user_agent() {
    let mut headers = HeaderMap::new();
    headers.insert("Accept", HeaderValue::from_static("application/json"));
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("zh-CN,zh;q=0.9"),
    );
    headers.insert("Origin", HeaderValue::from_static("https://proxy.local"));
    headers.insert(
        "Referer",
        HeaderValue::from_static("https://proxy.local/somewhere"),
    );
    headers.insert("Sec-CH-UA", HeaderValue::from_static("\"Chromium\""));
    headers.insert("User-Agent", HeaderValue::from_static("Mozilla/5.0"));
    headers.insert(
        "Mcp-Session-Id",
        HeaderValue::from_static("opaque-client-session"),
    );
    headers.insert(
        "Mcp-Protocol-Version",
        HeaderValue::from_static("2025-03-26"),
    );

    let sanitized = sanitize_mcp_headers_inner(&headers);

    assert_eq!(
        sanitized.headers.get("accept").unwrap(),
        &HeaderValue::from_static("application/json")
    );
    assert_eq!(
        sanitized.headers.get("mcp-session-id").unwrap(),
        &HeaderValue::from_static("opaque-client-session")
    );
    assert_eq!(
        sanitized.headers.get("mcp-protocol-version").unwrap(),
        &HeaderValue::from_static("2025-03-26")
    );
    assert_eq!(
        sanitized.headers.get("user-agent").unwrap(),
        &HeaderValue::from_static(MCP_PROXY_USER_AGENT)
    );
    assert!(!sanitized.headers.contains_key("accept-language"));
    assert!(!sanitized.headers.contains_key("origin"));
    assert!(!sanitized.headers.contains_key("referer"));
    assert!(!sanitized.headers.contains_key("sec-ch-ua"));
    assert!(sanitized.forwarded.contains(&"user-agent".to_string()));
    assert!(sanitized.dropped.contains(&"accept-language".to_string()));
    assert!(sanitized.dropped.contains(&"origin".to_string()));
    assert!(sanitized.dropped.contains(&"referer".to_string()));
    assert!(sanitized.dropped.contains(&"sec-ch-ua".to_string()));
}

fn temp_db_path(prefix: &str) -> PathBuf {
    let file = format!("{}-{}.db", prefix, nanoid!(8));
    std::env::temp_dir().join(file)
}

#[tokio::test]
async fn successful_request_logs_do_not_backfill_failure_kind() {
    let db_path = temp_db_path("request-log-success-failure-kind");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-request-log-success".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch key id");

    proxy
        .key_store
        .log_attempt(AttemptLog {
            key_id: Some(&key_id),
            auth_token_id: None,
            method: &Method::POST,
            path: "/mcp",
            query: None,
            status: Some(StatusCode::OK),
            tavily_status_code: Some(200),
            error: None,
            request_body: br#"{"jsonrpc":"2.0","id":"success-log","method":"tools/call","params":{"name":"tavily_search","arguments":{"query":"ok"}}}"#,
            response_body: br#"{"jsonrpc":"2.0","id":"success-log","result":{"content":[{"type":"text","text":"ok"}]}}"#,
            outcome: OUTCOME_SUCCESS,
            failure_kind: None,
            key_effect_code: KEY_EFFECT_NONE,
            key_effect_summary: None,
            binding_effect_code: KEY_EFFECT_NONE,
            binding_effect_summary: None,
            selection_effect_code: KEY_EFFECT_NONE,
            selection_effect_summary: None,
            gateway_mode: None,
            experiment_variant: None,
            proxy_session_id: None,
            routing_subject_hash: None,
            upstream_operation: None,
            fallback_reason: None,
            forwarded_headers: &[],
            dropped_headers: &[],
        })
        .await
        .expect("log success attempt");

    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT result_status, failure_kind FROM request_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch request log row");
    assert_eq!(row.0, OUTCOME_SUCCESS);
    assert_eq!(row.1, None);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_tokens_share_persistent_primary_key_affinity_after_restart() {
    let db_path = temp_db_path("user-token-primary-affinity");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "ok": true }
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let upstream = format!("http://{addr}/mcp");
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-user-affinity-a".to_string(),
            "tvly-user-affinity-b".to_string(),
        ],
        &upstream,
        &db_str,
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "user-primary-affinity".to_string(),
            username: Some("user-affinity".to_string()),
            name: Some("User Affinity".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let primary_token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:primary-affinity"))
        .await
        .expect("bind primary token");
    let secondary_seed = proxy
        .create_access_token(Some("linuxdo:secondary-affinity"))
        .await
        .expect("create secondary token");
    let secondary_token = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:secondary-affinity"),
            Some(&secondary_seed.id),
        )
        .await
        .expect("bind secondary token");

    let request = |token_id: &str| ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
        auth_token_id: Some(token_id.to_string()),
        prefer_mcp_session_affinity: false,
        pinned_api_key_id: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
    };

    let first = proxy
        .proxy_request(request(&primary_token.id))
        .await
        .expect("first request succeeds");
    assert!(first.status.is_success());

    let pool = open_sqlite_pool(&db_str, false, false)
        .await
        .expect("open sqlite pool");
    let user_primary: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM user_primary_api_key_affinity
           WHERE user_id = ?
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&pool)
    .await
    .expect("user primary affinity");
    let secondary_primary: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM token_primary_api_key_affinity
           WHERE token_id = ?
           LIMIT 1"#,
    )
    .bind(&secondary_token.id)
    .fetch_one(&pool)
    .await
    .expect("secondary token primary affinity");
    assert_eq!(
        secondary_primary, user_primary,
        "all user tokens should mirror the user's primary key after the first bind"
    );

    drop(proxy);

    let proxy_after_restart = TavilyProxy::with_endpoint(
        vec![
            "tvly-user-affinity-a".to_string(),
            "tvly-user-affinity-b".to_string(),
        ],
        &upstream,
        &db_str,
    )
    .await
    .expect("proxy recreated");

    let second = proxy_after_restart
        .proxy_request(request(&secondary_token.id))
        .await
        .expect("second request succeeds");
    assert!(second.status.is_success());

    let api_key_ids: Vec<String> = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM request_logs
           WHERE auth_token_id IN (?, ?)
             AND path = '/mcp'
             AND api_key_id IS NOT NULL
           ORDER BY id ASC"#,
    )
    .bind(&primary_token.id)
    .bind(&secondary_token.id)
    .fetch_all(&pool)
    .await
    .expect("request log api key ids");
    assert_eq!(api_key_ids.len(), 2);
    assert_eq!(api_key_ids[0], api_key_ids[1]);
    assert_eq!(api_key_ids[0], user_primary);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn token_primary_rebind_falls_back_to_exhausted_key_when_no_other_active_keys_exist() {
    let db_path = temp_db_path("token-primary-rebind-exhausted-fallback");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "ok": true }
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let upstream = format!("http://{addr}/mcp");
    let first_key = "tvly-token-rebind-exhausted-a".to_string();
    let second_key = "tvly-token-rebind-exhausted-b".to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec![first_key.clone(), second_key.clone()],
        &upstream,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("token-primary-rebind-exhausted"))
        .await
        .expect("create token");

    let request = || ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
        auth_token_id: Some(token.id.clone()),
        prefer_mcp_session_affinity: false,
        pinned_api_key_id: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
    };

    let first = proxy
        .proxy_request(request())
        .await
        .expect("initial request succeeds");
    assert!(first.status.is_success());

    let pool = open_sqlite_pool(&db_str, false, false)
        .await
        .expect("open sqlite pool");
    let old_key_id: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM token_primary_api_key_affinity
           WHERE token_id = ?
           LIMIT 1"#,
    )
    .bind(&token.id)
    .fetch_one(&pool)
    .await
    .expect("old primary key");

    let all_keys: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT id, api_key
           FROM api_keys
           ORDER BY id ASC"#,
    )
    .fetch_all(&pool)
    .await
    .expect("all keys");
    let (fallback_key_id, fallback_key_secret) = all_keys
        .into_iter()
        .find(|(id, _)| id != &old_key_id)
        .expect("fallback key");

    proxy
        .mark_key_quota_exhausted_by_secret(&fallback_key_secret)
        .await
        .expect("mark fallback key exhausted");
    proxy
        .disable_key_by_id(&old_key_id)
        .await
        .expect("disable old primary key");

    let rebound = proxy
        .proxy_request(request())
        .await
        .expect("request should fall back to exhausted key");
    assert!(rebound.status.is_success());
    assert_eq!(
        rebound.api_key_id.as_deref(),
        Some(fallback_key_id.as_str())
    );

    let rebound_key_id: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM token_primary_api_key_affinity
           WHERE token_id = ?
           LIMIT 1"#,
    )
    .bind(&token.id)
    .fetch_one(&pool)
    .await
    .expect("rebound primary key");
    assert_eq!(rebound_key_id, fallback_key_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn token_primary_rebind_prefers_active_key_over_existing_exhausted_primary() {
    let db_path = temp_db_path("token-primary-rebind-active-over-exhausted");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": { "ok": true }
            }))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let upstream = format!("http://{addr}/mcp");
    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-token-rebind-active-a".to_string(),
            "tvly-token-rebind-active-b".to_string(),
        ],
        &upstream,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("token-primary-rebind-active"))
        .await
        .expect("create token");

    let request = || ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#),
        auth_token_id: Some(token.id.clone()),
        prefer_mcp_session_affinity: false,
        pinned_api_key_id: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
    };

    let first = proxy
        .proxy_request(request())
        .await
        .expect("initial request succeeds");
    assert!(first.status.is_success());

    let pool = open_sqlite_pool(&db_str, false, false)
        .await
        .expect("open sqlite pool");
    let old_key_id: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM token_primary_api_key_affinity
           WHERE token_id = ?
           LIMIT 1"#,
    )
    .bind(&token.id)
    .fetch_one(&pool)
    .await
    .expect("old primary key");

    let all_keys: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT id, api_key
           FROM api_keys
           ORDER BY id ASC"#,
    )
    .fetch_all(&pool)
    .await
    .expect("all keys");
    let (old_key_secret, rebound_key_id) = all_keys
        .iter()
        .fold((None, None), |(old_secret, rebound), (id, secret)| {
            (
                old_secret.or_else(|| (id == &old_key_id).then_some(secret.clone())),
                rebound.or_else(|| (id != &old_key_id).then_some(id.clone())),
            )
        });
    let old_key_secret = old_key_secret.expect("old primary key secret");
    let rebound_key_id = rebound_key_id.expect("active fallback key");

    proxy
        .mark_key_quota_exhausted_by_secret(&old_key_secret)
        .await
        .expect("mark old primary exhausted");

    let rebound = proxy
        .proxy_request(request())
        .await
        .expect("request should rebind to active key");
    assert!(rebound.status.is_success());
    assert_eq!(rebound.api_key_id.as_deref(), Some(rebound_key_id.as_str()));

    let rebound_primary_key_id: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM token_primary_api_key_affinity
           WHERE token_id = ?
           LIMIT 1"#,
    )
    .bind(&token.id)
    .fetch_one(&pool)
    .await
    .expect("rebound primary key");
    assert_eq!(rebound_primary_key_id, rebound_key_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn token_log_filters_and_options_use_backfilled_request_kind_columns() {
    let db_path = temp_db_path("token-log-request-kind-backfill");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-startup-backfill".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let token = proxy
        .create_access_token(Some("request-kind-backfill"))
        .await
        .expect("token created");

    let stale_kind = TokenRequestKind::new("mcp:raw:/mcp", "MCP | /mcp", None);
    proxy
        .record_token_attempt_with_kind(
            &token.id,
            &Method::POST,
            "/mcp/sse",
            None,
            Some(200),
            Some(200),
            false,
            OUTCOME_SUCCESS,
            None,
            &stale_kind,
        )
        .await
        .expect("record stale request kind row");

    drop(proxy);

    let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");

    let filters = vec!["mcp:raw:/mcp/sse".to_string()];
    let page = repaired
        .token_logs_page(
            &token.id, 1, 20, 0, None, &filters, None, None, None, None, None, None,
        )
        .await
        .expect("query filtered token logs");
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].request_kind_key, "mcp:unsupported-path");
    assert_eq!(page.items[0].request_kind_label, "MCP | unsupported path");
    assert_eq!(
        page.items[0].request_kind_detail.as_deref(),
        Some("/mcp/sse")
    );

    let options = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query request kind options");
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].key, "mcp:unsupported-path");
    assert_eq!(options[0].label, "MCP | unsupported path");
    assert_eq!(options[0].protocol_group, "mcp");
    assert_eq!(options[0].billing_group, "non_billable");
    assert_eq!(options[0].count, 1);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, request_kind_key,
            request_kind_label, result_status, error_message, created_at, counts_business_quota
        ) VALUES (?, 'POST', '/mcp', NULL, 202, NULL, NULL, NULL, 'unknown', NULL, ?, 0)
        "#,
    )
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 1)
    .execute(&repaired.key_store.pool)
    .await
    .expect("insert legacy neutral control-plane row");

    let neutral_page = repaired
        .token_logs_page(
            &token.id,
            1,
            20,
            0,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
            Some("neutral"),
        )
        .await
        .expect("query neutral token logs");
    assert_eq!(neutral_page.total, 2);
    assert_eq!(neutral_page.items.len(), 2);
    let neutral_kinds = neutral_page
        .items
        .iter()
        .map(|item| item.request_kind_key.as_str())
        .collect::<Vec<_>>();
    assert!(neutral_kinds.contains(&"mcp:unknown-payload"));
    assert!(neutral_kinds.contains(&"mcp:unsupported-path"));
    let unsupported_path_log = neutral_page
        .items
        .iter()
        .find(|item| item.request_kind_key == "mcp:unsupported-path")
        .expect("neutral unsupported-path log");
    assert_eq!(
        unsupported_path_log.request_kind_detail.as_deref(),
        Some("/mcp/sse")
    );
    let unknown_payload_log = neutral_page
        .items
        .iter()
        .find(|item| item.request_kind_key == "mcp:unknown-payload")
        .expect("neutral unknown-payload log");
    assert_eq!(
        unknown_payload_log.request_kind_label,
        "MCP | unknown payload"
    );

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, request_kind_key,
            request_kind_label, result_status, error_message, created_at, counts_business_quota
        ) VALUES (?, 'POST', '/mcp', NULL, 200, 200, 'mcp:tool:acme-lookup', 'MCP | acme_lookup', 'success', NULL, ?, 0)
        "#,
    )
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .execute(&repaired.key_store.pool)
    .await
    .expect("insert mismatched duplicate option row");

    let canonicalized_options = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query canonicalized request kind options");
    let third_party_option = canonicalized_options
        .iter()
        .find(|option| option.key == "mcp:third-party-tool")
        .expect("third-party tool option exists");
    assert_eq!(third_party_option.label, "MCP | third-party tool");
    assert_eq!(third_party_option.protocol_group, "mcp");
    assert_eq!(third_party_option.billing_group, "non_billable");
    assert_eq!(third_party_option.count, 1);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, request_kind_key,
            request_kind_label, result_status, error_message, created_at, counts_business_quota
        ) VALUES (?, 'POST', '/mcp', NULL, 429, NULL, 'mcp:raw:/mcp', 'MCP | /mcp', 'quota_exhausted', NULL, ?, 0)
        "#,
    )
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 1)
    .execute(&repaired.key_store.pool)
    .await
    .expect("insert failed billable raw root option row");

    let canonicalized_with_failed_billable_raw = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query request kind options with failed raw root billable row");
    let unknown_payload_option = canonicalized_with_failed_billable_raw
        .iter()
        .find(|option| option.key == "mcp:unknown-payload")
        .expect("unknown payload option exists");
    assert_eq!(unknown_payload_option.billing_group, "non_billable");
    assert_eq!(unknown_payload_option.count, 2);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, request_kind_key,
            request_kind_label, result_status, error_message, created_at, counts_business_quota
        ) VALUES (?, 'POST', '/api/tavily/search', NULL, 429, NULL, 'api:search', 'API | search', 'quota_exhausted', NULL, ?, 0)
        "#,
    )
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 2)
    .execute(&repaired.key_store.pool)
    .await
    .expect("insert failed api search option row");

    let canonicalized_with_failed_search = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query request kind options with failed api search row");
    let api_search_option = canonicalized_with_failed_search
        .iter()
        .find(|option| option.key == "api:search")
        .expect("api search option exists");
    assert_eq!(api_search_option.billing_group, "billable");
    assert_eq!(api_search_option.count, 1);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, request_kind_key,
            request_kind_label, result_status, error_message, created_at, counts_business_quota
        ) VALUES (?, 'POST', '/mcp', NULL, 200, 200, 'mcp:batch', 'MCP | batch', 'success', NULL, ?, 0)
        "#,
    )
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 3)
    .execute(&repaired.key_store.pool)
    .await
    .expect("insert non-billable mcp batch option row");

    let options_with_non_billable_batch = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query request kind options with non-billable mcp batch row");
    let batch_option = options_with_non_billable_batch
        .iter()
        .find(|option| option.key == "mcp:batch")
        .expect("mcp batch option exists");
    assert_eq!(batch_option.billing_group, "non_billable");
    assert_eq!(batch_option.count, 1);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, request_kind_key,
            request_kind_label, result_status, error_message, created_at, counts_business_quota
        ) VALUES (?, 'POST', '/mcp', NULL, 200, 200, 'mcp:batch', 'MCP | batch', 'success', NULL, ?, 1)
        "#,
    )
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 4)
    .execute(&repaired.key_store.pool)
    .await
    .expect("insert billable mcp batch option row");

    let options_with_mixed_batch = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query request kind options with mixed mcp batch rows");
    let mixed_batch_option = options_with_mixed_batch
        .iter()
        .find(|option| option.key == "mcp:batch")
        .expect("mixed mcp batch option exists");
    assert_eq!(mixed_batch_option.billing_group, "billable");
    assert_eq!(mixed_batch_option.count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn startup_blocks_on_request_kind_database_migration() {
    let db_path = temp_db_path("request-kind-database-migration");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("startup-request-kind-backfill"))
        .await
        .expect("token created");
    let request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/search', 404, 404, 'Not Found', 'error',
            'mcp:raw:/mcp/search', 'MCP | /mcp/search',
            X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(Option::<String>::None)
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert legacy request log before migration");

    let token_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            api_key_id,
            method,
            path,
            http_status,
            mcp_status,
            request_kind_key,
            request_kind_label,
            result_status,
            key_effect_code,
            created_at,
            counts_business_quota,
            billing_state
        ) VALUES (
            ?, ?, 'POST', '/mcp', 200, 200,
            'mcp:tool:acme-startup', 'MCP | acme-startup',
            'success', 'none', ?, 0, 'none'
        )
        RETURNING id
        "#,
    )
    .bind(&token.id)
    .bind(Option::<String>::None)
    .bind(Utc::now().timestamp() + 1)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert legacy token log before migration");

    sqlx::query("DELETE FROM meta WHERE key IN (?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration markers");

    drop(proxy);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");

    let request_row = sqlx::query(
        r#"
        SELECT
            request_kind_key,
            request_kind_label,
            request_kind_detail
        FROM request_logs
        WHERE id = ?
        "#,
    )
    .bind(request_log_id)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("request row after database migration");
    assert_eq!(
        request_row
            .try_get::<String, _>("request_kind_key")
            .unwrap(),
        "mcp:unsupported-path"
    );
    assert_eq!(
        request_row
            .try_get::<String, _>("request_kind_label")
            .unwrap(),
        "MCP | unsupported path"
    );
    assert_eq!(
        request_row
            .try_get::<Option<String>, _>("request_kind_detail")
            .unwrap()
            .as_deref(),
        Some("/mcp/search")
    );

    let token_row = sqlx::query(
        r#"
        SELECT
            request_kind_key,
            request_kind_label,
            request_kind_detail
        FROM auth_token_logs
        WHERE id = ?
        "#,
    )
    .bind(token_log_id)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("token row after database migration");
    assert_eq!(
        token_row.try_get::<String, _>("request_kind_key").unwrap(),
        "mcp:third-party-tool"
    );
    assert_eq!(
        token_row
            .try_get::<String, _>("request_kind_label")
            .unwrap(),
        "MCP | third-party tool"
    );
    assert_eq!(
        token_row
            .try_get::<Option<String>, _>("request_kind_detail")
            .unwrap()
            .as_deref(),
        Some("acme-startup")
    );
    assert!(
        !reopened
            .key_store
            .request_logs_column_exists("legacy_request_kind_key")
            .await
            .expect("legacy request_kind column removed"),
        "request_logs should drop legacy request-kind columns during startup migration"
    );
    assert!(
        !reopened
            .key_store
            .table_column_exists("auth_token_logs", "legacy_request_kind_key")
            .await
            .expect("token legacy request_kind column removed"),
        "auth_token_logs should drop legacy request-kind columns during startup migration"
    );

    let request_cursor: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("request cursor after database migration");
    let token_cursor: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("token cursor after database migration");
    assert!(request_cursor >= request_log_id);
    assert!(token_cursor >= token_log_id);

    let migration_done_at: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("request-kind migration done marker");
    assert!(migration_done_at > 0);
    let request_upper_bound: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("request-kind request log upper bound");
    let token_upper_bound: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("request-kind token log upper bound");
    assert!(request_upper_bound >= request_log_id);
    assert!(token_upper_bound >= token_log_id);
    let migration_state: String =
        sqlx::query_scalar("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("request-kind migration state marker");
    assert_eq!(migration_state, format!("done:{migration_done_at}"));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn startup_reruns_request_kind_migration_after_request_log_self_heal() {
    let db_path = temp_db_path("request-kind-migration-reset-after-request-log-self-heal");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("request-kind-self-heal-reset"))
        .await
        .expect("token created");

    let request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/search', 404, 404, 'Not Found', 'error',
            'mcp:raw:/mcp/search', 'MCP | /mcp/search',
            X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(Option::<String>::None)
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert legacy request log before self-heal");

    proxy.key_store.pool.close().await;
    drop(proxy);

    let options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let mut conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("connect rebuild pool");

    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut conn)
        .await
        .expect("disable foreign keys");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut conn)
        .await
        .expect("begin request_logs rebuild");
    sqlx::query(
        r#"
        CREATE TABLE request_logs_self_heal (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            auth_token_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            query TEXT,
            status_code INTEGER,
            tavily_status_code INTEGER,
            error_message TEXT,
            result_status TEXT NOT NULL DEFAULT 'unknown',
            business_credits INTEGER,
            failure_kind TEXT,
            key_effect_code TEXT NOT NULL DEFAULT 'none',
            key_effect_summary TEXT,
            request_body BLOB,
            response_body BLOB,
            forwarded_headers TEXT,
            dropped_headers TEXT,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create request_logs self-heal table");
    sqlx::query(
        r#"
        INSERT INTO request_logs_self_heal (
            id,
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            business_credits,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        )
        SELECT
            id,
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            business_credits,
            failure_kind,
            key_effect_code,
            key_effect_summary,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        FROM request_logs
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("copy request logs without request-kind columns");
    sqlx::query("DROP TABLE request_logs")
        .execute(&mut conn)
        .await
        .expect("drop request_logs");
    sqlx::query("ALTER TABLE request_logs_self_heal RENAME TO request_logs")
        .execute(&mut conn)
        .await
        .expect("rename request_logs self-heal table");
    sqlx::query("COMMIT")
        .execute(&mut conn)
        .await
        .expect("commit request_logs rebuild");
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut conn)
        .await
        .expect("re-enable foreign keys");
    drop(conn);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");

    let request_row = sqlx::query(
        r#"
        SELECT request_kind_key, request_kind_label, request_kind_detail
        FROM request_logs
        WHERE id = ?
        "#,
    )
    .bind(request_log_id)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("request row after self-heal migration");
    assert_eq!(
        request_row
            .try_get::<String, _>("request_kind_key")
            .unwrap(),
        "mcp:unsupported-path"
    );
    assert_eq!(
        request_row
            .try_get::<String, _>("request_kind_label")
            .unwrap(),
        "MCP | unsupported path"
    );
    assert_eq!(
        request_row
            .try_get::<Option<String>, _>("request_kind_detail")
            .unwrap()
            .as_deref(),
        Some("/mcp/search")
    );

    assert!(
        !reopened
            .key_store
            .request_logs_column_exists("legacy_request_kind_key")
            .await
            .expect("legacy request_kind column check"),
        "request_logs migration should drop legacy request-kind columns after rebuild"
    );

    let request_cursor: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("request cursor after self-heal migration");
    assert!(
        request_cursor >= request_log_id,
        "request-kind migration should rerun after self-heal"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_database_migration_state_blocks_reentry() {
    let db_path = temp_db_path("request-kind-migration-state");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    sqlx::query("DELETE FROM meta WHERE key IN (?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration meta");

    let first_claim = proxy
        .key_store
        .try_claim_request_kind_canonical_migration_v1(100)
        .await
        .expect("first migration claim");
    assert_eq!(first_claim, RequestKindCanonicalMigrationClaim::Claimed);

    let running_state: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ? LIMIT 1")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("running state recorded");
    assert_eq!(running_state, format!("running:100:{}", std::process::id()));
    let request_upper_bound: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("request-kind request log upper bound");
    let token_upper_bound: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("request-kind token log upper bound");
    assert_eq!(request_upper_bound, 0);
    assert_eq!(token_upper_bound, 0);

    let second_claim = proxy
        .key_store
        .try_claim_request_kind_canonical_migration_v1(101)
        .await
        .expect("second migration claim");
    assert_eq!(
        second_claim,
        RequestKindCanonicalMigrationClaim::RunningElsewhere(100)
    );

    let reclaimed_claim = proxy
        .key_store
        .try_claim_request_kind_canonical_migration_v1(1000)
        .await
        .expect("reclaimed migration claim");
    assert_eq!(reclaimed_claim, RequestKindCanonicalMigrationClaim::Claimed);

    proxy
        .key_store
        .finish_request_kind_canonical_migration_v1(RequestKindCanonicalMigrationState::Done(102))
        .await
        .expect("finish migration");

    let third_claim = proxy
        .key_store
        .try_claim_request_kind_canonical_migration_v1(103)
        .await
        .expect("third migration claim");
    assert_eq!(
        third_claim,
        RequestKindCanonicalMigrationClaim::AlreadyDone(102)
    );

    let done_state: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ? LIMIT 1")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("done state recorded");
    assert_eq!(done_state, "done:102");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_database_migration_claim_reads_state_without_write_lock() {
    let db_path = temp_db_path("request-kind-migration-read-without-write-lock");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    sqlx::query("DELETE FROM meta WHERE key IN (?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration meta");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
    .bind(format!("running:100:{}", std::process::id()))
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed running state");

    let mut conn = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("acquire write-lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *conn)
        .await
        .expect("begin immediate");

    let claim = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        proxy
            .key_store
            .try_claim_request_kind_canonical_migration_v1(101),
    )
    .await
    .expect("claim should not block on unrelated write lock")
    .expect("claim result");
    assert_eq!(
        claim,
        RequestKindCanonicalMigrationClaim::RunningElsewhere(100)
    );

    sqlx::query("COMMIT")
        .execute(&mut *conn)
        .await
        .expect("commit immediate transaction");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_database_migration_reclaims_dead_running_owner_immediately() {
    let db_path = temp_db_path("request-kind-migration-reclaim-dead-owner");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    sqlx::query("DELETE FROM meta WHERE key IN (?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration meta");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
    .bind(format!(
        "running:100:{}",
        dead_request_kind_migration_owner_pid()
    ))
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed dead running state");

    let claim = proxy
        .key_store
        .try_claim_request_kind_canonical_migration_v1(101)
        .await
        .expect("claim should reclaim dead owner");
    assert_eq!(claim, RequestKindCanonicalMigrationClaim::Claimed);

    let running_state: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ? LIMIT 1")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("running state recorded");
    assert_eq!(running_state, format!("running:101:{}", std::process::id()));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_database_migration_retries_after_transient_write_lock() {
    use sqlx::Connection;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

    let db_path = temp_db_path("request-kind-migration-retry-after-busy");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration markers");
    proxy.key_store.pool.close().await;
    drop(proxy);

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_millis(1));
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options.clone())
        .await
        .expect("busy-test pool");
    let store = KeyStore {
        pool,
        token_binding_cache: RwLock::new(std::collections::HashMap::new()),
        account_quota_resolution_cache: RwLock::new(std::collections::HashMap::new()),
        request_logs_catalog_cache: RwLock::new(std::collections::HashMap::new()),
        admin_heavy_read_semaphore: Semaphore::new(ADMIN_HEAVY_READ_CONCURRENCY),
        #[cfg(test)]
        forced_pending_claim_miss_log_ids: Mutex::new(std::collections::HashSet::new()),
        forced_quota_subject_lock_loss_subjects: std::sync::Mutex::new(
            std::collections::HashSet::new(),
        ),
    };

    let mut lock_conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("connect write lock holder");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock_conn)
        .await
        .expect("hold write lock");

    let release_lock = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        sqlx::query("COMMIT")
            .execute(&mut lock_conn)
            .await
            .expect("release write lock");
    });

    store
        .ensure_request_kind_canonical_migration_v1()
        .await
        .expect("migration should retry after transient busy");
    release_lock.await.expect("join lock release");

    let migration_done_at = store
        .get_meta_i64(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .await
        .expect("migration done marker");
    assert!(migration_done_at.is_some());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_backfill_batch_retries_after_transient_write_lock() {
    use sqlx::Connection;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};

    let db_path = temp_db_path("request-kind-backfill-batch-retry-after-busy");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("request-kind-batch-busy"))
        .await
        .expect("token created");

    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration markers");

    let request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/search', 404, 404, 'Not Found', 'error',
            'mcp:raw:/mcp/search', 'MCP | /mcp/search',
            X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(Option::<String>::None)
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert bounded request log");

    let first_claim = proxy
        .key_store
        .try_claim_request_kind_canonical_migration_v1(100)
        .await
        .expect("first migration claim");
    assert_eq!(first_claim, RequestKindCanonicalMigrationClaim::Claimed);

    let request_upper_bound: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("request-kind request log upper bound");
    let token_upper_bound: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("request-kind token log upper bound");
    assert_eq!(request_upper_bound, request_log_id);
    assert_eq!(token_upper_bound, 0);

    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_millis(1));
    let mut lock_conn = sqlx::SqliteConnection::connect_with(&options)
        .await
        .expect("connect write lock holder");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut lock_conn)
        .await
        .expect("hold write lock");

    let release_lock = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        sqlx::query("COMMIT")
            .execute(&mut lock_conn)
            .await
            .expect("release write lock");
    });

    let report = run_request_kind_canonical_backfill_with_pool(
        &proxy.key_store.pool,
        128,
        false,
        Some(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE),
        Some(RequestKindCanonicalBackfillUpperBounds {
            request_logs: request_upper_bound,
            auth_token_logs: token_upper_bound,
        }),
    )
    .await
    .expect("backfill should retry after transient busy");
    release_lock.await.expect("join lock release");

    assert_eq!(report.request_logs.cursor_after, request_log_id);
    let canonical_request_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM request_logs WHERE id = ?")
            .bind(request_log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("request log canonicalized");
    assert_eq!(canonical_request_kind, "mcp:unsupported-path");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_kind_database_migration_uses_persisted_upper_bounds() {
    let db_path = temp_db_path("request-kind-migration-upper-bounds");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("request-kind-upper-bounds"))
        .await
        .expect("token created");

    sqlx::query("DELETE FROM meta WHERE key IN (?, ?, ?, ?)")
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1)
        .bind(META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear request-kind migration markers");

    let request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/search', 404, 404, 'Not Found', 'error',
            'mcp:raw:/mcp/search', 'MCP | /mcp/search',
            X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(Option::<String>::None)
    .bind(&token.id)
    .bind(Utc::now().timestamp())
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert bounded request log");

    let token_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            api_key_id,
            method,
            path,
            http_status,
            mcp_status,
            request_kind_key,
            request_kind_label,
            result_status,
            key_effect_code,
            created_at,
            counts_business_quota,
            billing_state
        ) VALUES (
            ?, ?, 'POST', '/mcp', 200, 200,
            'mcp:tool:acme-target', 'MCP | acme-target',
            'success', 'none', ?, 0, 'none'
        )
        RETURNING id
        "#,
    )
    .bind(&token.id)
    .bind(Option::<String>::None)
    .bind(Utc::now().timestamp() + 1)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert bounded token log");

    let late_request_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            status_code,
            tavily_status_code,
            error_message,
            result_status,
            request_kind_key,
            request_kind_label,
            request_body,
            response_body,
            forwarded_headers,
            dropped_headers,
            visibility,
            created_at
        ) VALUES (
            ?, ?, 'POST', '/mcp/late', 404, 404, 'Not Found', 'error',
            'mcp:raw:/mcp/late', 'MCP | /mcp/late',
            X'7B7D', X'4E6F7420466F756E64', '[]', '[]', 'visible', ?
        )
        RETURNING id
        "#,
    )
    .bind(Option::<String>::None)
    .bind(&token.id)
    .bind(Utc::now().timestamp() + 2)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert late request log");

    let late_token_log_id: i64 = sqlx::query_scalar(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            api_key_id,
            method,
            path,
            http_status,
            mcp_status,
            request_kind_key,
            request_kind_label,
            result_status,
            key_effect_code,
            created_at,
            counts_business_quota,
            billing_state
        ) VALUES (
            ?, ?, 'POST', '/mcp', 200, 200,
            'mcp:tool:acme-late', 'MCP | acme-late',
            'success', 'none', ?, 0, 'none'
        )
        RETURNING id
        "#,
    )
    .bind(&token.id)
    .bind(Option::<String>::None)
    .bind(Utc::now().timestamp() + 3)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("insert late token log");

    let report = run_request_kind_canonical_backfill_with_pool(
        &proxy.key_store.pool,
        128,
        false,
        Some(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE),
        Some(RequestKindCanonicalBackfillUpperBounds {
            request_logs: request_log_id,
            auth_token_logs: token_log_id,
        }),
    )
    .await
    .expect("run bounded request-kind backfill");

    assert_eq!(report.request_logs.cursor_after, request_log_id);
    assert_eq!(report.auth_token_logs.cursor_after, token_log_id);

    let canonical_request_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM request_logs WHERE id = ?")
            .bind(request_log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("bounded request log canonicalized");
    let late_request_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM request_logs WHERE id = ?")
            .bind(late_request_log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("late request log canonicalized by write-path trigger");
    assert_eq!(canonical_request_kind, "mcp:unsupported-path");
    assert_eq!(late_request_kind, "mcp:unsupported-path");

    let canonical_token_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM auth_token_logs WHERE id = ?")
            .bind(token_log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("bounded token log canonicalized");
    let late_token_kind: String =
        sqlx::query_scalar("SELECT request_kind_key FROM auth_token_logs WHERE id = ?")
            .bind(late_token_log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("late token log untouched");
    assert_eq!(canonical_token_kind, "mcp:third-party-tool");
    assert_eq!(late_token_kind, "mcp:tool:acme-late");

    let _ = std::fs::remove_file(db_path);
}

use crate::analysis::*;
use crate::models::*;
use crate::store::*;
use crate::tavily_proxy::*;
use crate::*;

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{any, get, post},
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::net::TcpListener;

fn env_lock() -> Arc<tokio::sync::Mutex<()>> {
    static LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
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
fn classify_token_request_kind_maps_http_routes_and_raw_paths() {
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
        TokenRequestKind::new("api:raw:/api/custom/raw", "API | /api/custom/raw", None)
    );
    assert_eq!(
        classify_token_request_kind("/mcp/sse", None),
        TokenRequestKind::new("mcp:raw:/mcp/sse", "MCP | /mcp/sse", None)
    );
}

#[test]
fn classify_token_request_kind_maps_mcp_control_plane_and_tools() {
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
        TokenRequestKind::new("mcp:tool:acme-lookup", "MCP | Acme Lookup", None)
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
        TokenRequestKind::new("mcp:tool:acme-lookup", "MCP | acme_lookup", None)
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
        token_request_kind_billing_group("mcp:tool:acme-lookup"),
        "non_billable"
    );
    assert_eq!(
        token_request_kind_billing_group("mcp:tool:tavily-graph"),
        "billable"
    );
    assert_eq!(
        token_request_kind_billing_group("mcp:raw:/mcp/sse"),
        "billable"
    );
    assert_eq!(token_request_kind_billing_group("mcp:raw:/mcp"), "billable");
    assert_eq!(token_request_kind_billing_group("mcp:batch"), "billable");

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
    assert_eq!(effective_request_logs_retention_days(), 7);

    unsafe {
        std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "10");
    }
    assert_eq!(effective_request_logs_retention_days(), 10);

    unsafe {
        std::env::set_var("REQUEST_LOGS_RETENTION_DAYS", "not-a-number");
        std::env::set_var("REQUEST_LOGS_GC_AT", "23:30");
    }
    assert_eq!(effective_request_logs_retention_days(), 7);
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

fn temp_db_path(prefix: &str) -> PathBuf {
    let file = format!("{}-{}.db", prefix, nanoid!(8));
    std::env::temp_dir().join(file)
}

#[tokio::test]
async fn token_log_filters_and_options_use_backfilled_request_kind_columns() {
    let db_path = temp_db_path("token-log-request-kind-backfill");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
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
    let (logs, total) = repaired
        .token_logs_page(&token.id, 1, 20, 0, None, &filters)
        .await
        .expect("query filtered token logs");
    assert_eq!(total, 1);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].request_kind_key, "mcp:raw:/mcp/sse");
    assert_eq!(logs[0].request_kind_label, "MCP | /mcp/sse");

    let options = repaired
        .token_log_request_kind_options(&token.id, 0, None)
        .await
        .expect("query request kind options");
    assert_eq!(options.len(), 1);
    assert_eq!(options[0].key, "mcp:raw:/mcp/sse");
    assert_eq!(options[0].label, "MCP | /mcp/sse");
    assert_eq!(options[0].protocol_group, "mcp");
    assert_eq!(options[0].billing_group, "billable");

    sqlx::query(
        r#"
        UPDATE auth_token_logs
        SET request_kind_key = 'mcp:tool:acme-lookup',
            request_kind_label = 'MCP | Acme Lookup'
        WHERE token_id = ?
        "#,
    )
    .bind(&token.id)
    .execute(&repaired.key_store.pool)
    .await
    .expect("stamp stored request kind");
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
    assert_eq!(canonicalized_options.len(), 1);
    assert_eq!(canonicalized_options[0].key, "mcp:tool:acme-lookup");
    assert_eq!(canonicalized_options[0].label, "MCP | Acme Lookup");
    assert_eq!(canonicalized_options[0].protocol_group, "mcp");
    assert_eq!(canonicalized_options[0].billing_group, "non_billable");

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
    let raw_root_option = canonicalized_with_failed_billable_raw
        .iter()
        .find(|option| option.key == "mcp:raw:/mcp")
        .expect("raw root option exists");
    assert_eq!(raw_root_option.billing_group, "billable");

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

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_status_keeps_tx_clean_after_insert_failure() {
    let db_path = temp_db_path("api-key-upsert-clean-tx-after-failure");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open db pool");

    sqlx::query(
        r#"
        CREATE TRIGGER fail_insert_api_key
        BEFORE INSERT ON api_keys
        WHEN NEW.api_key = 'tvly-force-fail'
        BEGIN
            SELECT RAISE(ABORT, 'boom');
        END;
        "#,
    )
    .execute(&pool)
    .await
    .expect("create fail trigger");

    let first_err = proxy
        .add_or_undelete_key_with_status_in_group("tvly-force-fail", Some("team-a"))
        .await
        .expect_err("first key should fail due to trigger");
    assert!(
        first_err.to_string().contains("boom"),
        "error should include trigger message"
    );

    let (second_id, second_status) = proxy
        .add_or_undelete_key_with_status_in_group("tvly-after-failure", Some("team-a"))
        .await
        .expect("second key insert should not be polluted by previous failure");
    assert_eq!(second_status, ApiKeyUpsertStatus::Created);
    assert!(!second_id.is_empty(), "second key id must be present");

    let inserted_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE api_key = 'tvly-after-failure'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count inserted keys");
    assert_eq!(
        inserted_count, 1,
        "follow-up insert must succeed even after previous tx failure"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_status_refreshes_existing_registration_metadata_only() {
    let db_path = temp_db_path("api-key-upsert-refresh-registration");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-existing",
            Some("old"),
            Some("8.8.8.8"),
            Some("US"),
        )
        .await
        .expect("existing key created");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let (same_key_id, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-existing",
            Some("new"),
            Some("8.8.4.4"),
            Some("US Westfield (MA)"),
        )
        .await
        .expect("existing key refreshed");
    assert_eq!(same_key_id, key_id);
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch refreshed key");
    assert_eq!(row.0.as_deref(), Some("old"));
    assert_eq!(row.1.as_deref(), Some("8.8.4.4"));
    assert_eq!(row.2.as_deref(), Some("US Westfield (MA)"));

    proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-existing",
            None,
            Some("2606:4700:4700::1111"),
            None,
        )
        .await
        .expect("existing key refreshed to empty region");

    let refreshed_row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch refreshed key after region clear");
    assert_eq!(refreshed_row.0.as_deref(), Some("old"));
    assert_eq!(refreshed_row.1.as_deref(), Some("2606:4700:4700::1111"));
    assert!(
        refreshed_row.2.is_none(),
        "region should clear when the new registration ip has no resolved region"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_for_registration_prefers_exact_ip_then_region() {
    let db_path = temp_db_path("proxy-affinity-registration-selection");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                    "http://8.8.8.8:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (exact, exact_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:exact",
            &geo_origin,
            Some("18.183.246.69"),
            Some("JP Tokyo (13)"),
            None,
        )
        .await
        .expect("exact proxy affinity");
    assert_eq!(
        exact.primary_proxy_key.as_deref(),
        Some("http://18.183.246.69:8080"),
        "exact IP match should win before region matching"
    );
    assert_eq!(
        exact_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp),
        "exact IP selections should expose registration_ip match kind"
    );

    let (region, region_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:region",
            &geo_origin,
            Some("103.232.214.107"),
            Some("HK"),
            None,
        )
        .await
        .expect("region proxy affinity");
    assert_eq!(
        region.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080"),
        "same-region proxy should win when no exact IP node exists"
    );
    assert_eq!(
        region_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::SameRegion),
        "same-region selections should expose same_region match kind"
    );

    let (fallback, fallback_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:fallback",
            &geo_origin,
            Some("103.232.214.107"),
            Some("ZZ"),
            None,
        )
        .await
        .expect("fallback proxy affinity");
    assert!(
        fallback.primary_proxy_key.is_some(),
        "selection should still fall back to a selectable proxy node"
    );
    assert_eq!(
        fallback_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other),
        "fallback selections should expose other match kind"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_persists_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-persist");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (record, _preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:persist-runtime-geo",
            &geo_origin,
            Some("8.8.8.8"),
            Some("HK"),
            None,
        )
        .await
        .expect("registration-aware affinity");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load persisted runtime geo metadata");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode persisted resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode persisted resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_persists_forward_proxy_runtime_geo_metadata_from_trace_exit_ip() {
    let db_path = temp_db_path("proxy-runtime-geo-persist-trace-exit");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let fake_proxy_addr =
        spawn_fake_forward_proxy_with_body("ip=1.1.1.1\nloc=US\ncolo=LAX\n".to_string()).await;
    let proxy_url = format!("http://{fake_proxy_addr}");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:persist-runtime-geo-trace-exit",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("registration-aware affinity from trace exit ip");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some(proxy_url.as_str())
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load persisted trace-driven runtime geo metadata");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode persisted trace resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode persisted trace resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_persists_forward_proxy_runtime_geo_metadata_for_xray_route() {
    let db_path = temp_db_path("proxy-runtime-geo-persist-xray");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let raw_proxy_url =
        "vless://0688fa59-e971-4278-8c03-4b35821a71dc@1.1.1.1:443?encryption=none#hk";
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![raw_proxy_url.to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let endpoint = manager
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.raw_url.as_deref() == Some(raw_proxy_url))
            .expect("xray endpoint");
        let endpoint_key = endpoint.key.clone();
        let route_url = Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route");
        endpoint.endpoint_url = Some(route_url.clone());
        let runtime = manager
            .runtime
            .get_mut(&endpoint_key)
            .expect("xray runtime state");
        runtime.endpoint_url = Some(route_url.to_string());
        runtime.available = true;
        runtime.last_error = None;
    }

    let (record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:persist-runtime-geo-xray",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("registration-aware affinity for xray route");
    let primary = record.primary_proxy_key.expect("primary proxy key");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&primary)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load persisted runtime geo metadata for xray route");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode persisted resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode persisted resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_reuses_persisted_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-reuse");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
        proxy_urls: vec![
            "http://18.183.246.69:8080".to_string(),
            "http://1.1.1.1:8080".to_string(),
        ],
        subscription_urls: Vec::new(),
        subscription_update_interval_secs: 3600,
        insert_direct: false,

        egress_socks5_enabled: false,
        egress_socks5_url: String::new(),
    }
    .normalized();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
    }

    proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:seed-runtime-geo",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("seed persisted runtime geo metadata");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }

    let (record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:reuse-runtime-geo",
            "http://127.0.0.1:9/geo",
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should reuse persisted runtime geo metadata");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn update_forward_proxy_settings_rejects_invalid_egress_socks5_url() {
    let db_path = temp_db_path("invalid-egress-socks5-url");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let result = proxy
        .update_forward_proxy_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: true,
                egress_socks5_enabled: true,
                egress_socks5_url: "socks5h://user:pass@127".to_string(),
            },
            true,
        )
        .await;

    match result {
        Err(ProxyError::Other(message)) => {
            assert!(
                message.contains("valid socks5:// or socks5h:// URL"),
                "unexpected validation error: {message}",
            );
        }
        other => panic!("expected invalid egress socks5 URL to be rejected, got {other:?}"),
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_refreshes_incomplete_persisted_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-incomplete");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
        proxy_urls: vec![
            "http://18.183.246.69:8080".to_string(),
            "http://1.1.1.1:8080".to_string(),
        ],
        subscription_urls: Vec::new(),
        subscription_update_interval_secs: 3600,
        insert_direct: false,

        egress_socks5_enabled: false,
        egress_socks5_url: String::new(),
    }
    .normalized();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
    }

    proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:seed-runtime-geo-incomplete",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("seed persisted runtime geo metadata");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_regions_json = '[]', geo_refreshed_at = 0 WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .execute(&proxy.key_store.pool)
    .await
    .expect("clear persisted runtime regions");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }

    let (_record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:refresh-runtime-geo-incomplete",
            &geo_origin,
            None,
            Some("HK"),
            None,
        )
        .await
        .expect("selection should refresh incomplete persisted runtime geo metadata");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::SameRegion)
    );

    let row: String = sqlx::query_scalar(
        "SELECT resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load refreshed runtime region metadata");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row).expect("decode refreshed resolved regions");
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_refreshes_legacy_host_based_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-legacy-host");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
        proxy_urls: vec!["http://1.1.1.1:8080".to_string()],
        subscription_urls: Vec::new(),
        subscription_update_interval_secs: 3600,
        insert_direct: false,

        egress_socks5_enabled: false,
        egress_socks5_url: String::new(),
    }
    .normalized();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
    }

    proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:seed-runtime-geo-legacy",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("seed persisted runtime geo metadata");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ips_json = '[\"1.1.1.1\"]', resolved_regions_json = '[\"HK\"]', resolved_ip_source = '' WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy host-based runtime geo metadata");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }
    reloaded
        .set_forward_proxy_trace_override_for_test(
            "http://1.1.1.1:8080",
            "1.0.0.1",
            "TEST / 1.0.0.1",
        )
        .await;

    let (record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:refresh-runtime-geo-legacy",
            &geo_origin,
            Some("1.0.0.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should refresh legacy host-based runtime geo metadata");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String, String) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind("http://1.1.1.1:8080")
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load refreshed legacy runtime geo metadata");
    assert_eq!(row.0, "trace".to_string());
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.1).expect("decode refreshed legacy resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.2).expect("decode refreshed legacy resolved regions");
    assert_eq!(resolved_ips, vec!["1.0.0.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_does_not_match_legacy_host_based_runtime_geo_when_trace_fails() {
    let db_path = temp_db_path("proxy-runtime-geo-ignore-legacy-host");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://127.0.0.1:1".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let settings = ForwardProxySettings {
        proxy_urls: vec![proxy_url.clone()],
        subscription_urls: Vec::new(),
        subscription_update_interval_secs: 3600,
        insert_direct: false,

        egress_socks5_enabled: false,
        egress_socks5_url: String::new(),
    }
    .normalized();
    let endpoint_key = {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(settings.clone());
        manager
            .endpoints
            .iter()
            .find(|endpoint| {
                endpoint.endpoint_url.as_ref().map(Url::to_string) == Some(proxy_url.clone())
                    || endpoint.key == proxy_url
            })
            .map(|endpoint| endpoint.key.clone())
            .unwrap_or_else(|| proxy_url.clone())
    };
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&endpoint_key)
            .cloned()
            .expect("persisted runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist initial runtime state");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ips_json = '[\"1.1.1.1\"]', resolved_regions_json = '[\"HK\"]', resolved_ip_source = '' WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy host-based runtime geo metadata");

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(settings);
    }

    let (record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:ignore-runtime-geo-legacy",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should ignore legacy host-based runtime geo metadata");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some(endpoint_key.as_str())
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other)
    );

    let row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load stale runtime source");
    assert_eq!(row.0, "negative");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.1).expect("decode negative resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.2).expect("decode negative resolved regions");
    assert!(resolved_ips.is_empty());
    assert!(resolved_regions.is_empty());
    assert!(
        row.3 > 0,
        "trace failures should persist a negative geo placeholder timestamp"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_reuses_negative_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-reuse-negative");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://127.0.0.1:1".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (_first_record, first_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:negative-cache-first",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("first selection should persist negative placeholder");
    assert_eq!(
        first_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other)
    );

    let first_row: (String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load first negative runtime row");
    assert_eq!(first_row.0, "negative");
    assert!(first_row.1 > 0);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let (_second_record, second_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:negative-cache-second",
            "http://127.0.0.1:9/geo",
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("second selection should reuse negative placeholder");
    assert_eq!(
        second_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::Other)
    );

    let second_row: (String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load second negative runtime row");
    assert_eq!(second_row.0, "negative");
    assert_eq!(
        second_row.1, first_row.1,
        "negative GEO placeholders should be reused without retracing on each request"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_retries_stale_negative_forward_proxy_runtime_geo_metadata() {
    let db_path = temp_db_path("proxy-runtime-geo-retry-stale-negative");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "negative".to_string();
        runtime.resolved_ips = Vec::new();
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at =
            Utc::now().timestamp() - (FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS + 1);
    }
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&proxy_url)
            .cloned()
            .expect("persisted runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist stale negative runtime state");
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.1.1.1", "TEST / 1.1.1.1")
        .await;

    let (_record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:retry-stale-negative-cache",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should retry stale negative placeholders");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String, String) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed runtime row");
    assert_eq!(row.0, "trace");
    assert_eq!(row.1, "[\"1.1.1.1\"]");
    assert_eq!(row.2, "[\"HK\"]");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_marks_trace_without_region_as_retriable_trace_cache() {
    let db_path = temp_db_path("proxy-runtime-geo-trace-without-region");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "8.8.8.8", "TEST / 8.8.8.8")
        .await;

    let (_first_record, first_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:trace-without-region-first",
            "http://127.0.0.1:9/geo",
            Some("8.8.8.8"),
            Some("HK"),
            None,
        )
        .await
        .expect("first selection should persist retriable trace cache when GEO lookup is empty");
    assert_eq!(
        first_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let first_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load retriable trace cache row");
    assert_eq!(first_row.0, "trace");
    assert_eq!(first_row.1, "[\"8.8.8.8\"]");
    assert_eq!(first_row.2, "[]");
    assert!(first_row.3 > 0);

    proxy
        .forward_proxy_trace_overrides
        .lock()
        .await
        .remove(&proxy_url);
    tokio::time::sleep(Duration::from_secs(1)).await;

    let (_second_record, second_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:trace-without-region-second",
            "http://127.0.0.1:9/geo",
            Some("8.8.8.8"),
            Some("HK"),
            None,
        )
        .await
        .expect("second selection should reuse cached trace IPs when GEO lookup stays empty");
    assert_eq!(
        second_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let second_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("reload retriable trace cache row");
    assert_eq!(second_row.0, "trace");
    assert_eq!(second_row.1, "[\"8.8.8.8\"]");
    assert_eq!(second_row.2, "[]");
    assert_eq!(
        second_row.3, first_row.3,
        "region lookup retries should reuse cached trace IPs without rerunning trace"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn persist_forward_proxy_geo_candidates_preserves_runtime_health_columns() {
    let db_path = temp_db_path("proxy-runtime-geo-preserve-health");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://1.1.1.1:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    proxy
        .update_forward_proxy_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            },
            false,
        )
        .await
        .expect("proxy settings updated");

    let endpoint = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .endpoints
            .iter()
            .find(|endpoint| endpoint.key == proxy_url)
            .cloned()
            .expect("forward proxy endpoint")
    };

    sqlx::query(
        "UPDATE forward_proxy_runtime SET weight = 9.25, success_ema = 0.11, latency_ema_ms = 321.0, consecutive_failures = 7, is_penalized = 1 WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed updated runtime health metrics");

    proxy
        .persist_forward_proxy_geo_candidates(&[ForwardProxyGeoCandidate {
            endpoint,
            host_ips: vec!["1.1.1.1".to_string()],
            regions: vec!["HK".to_string()],
            source: ForwardProxyGeoSource::Trace,
            geo_refreshed_at: Utc::now().timestamp(),
        }])
        .await
        .expect("persist geo metadata only");

    let row: (f64, f64, Option<f64>, i64, i64, String, String, String) = sqlx::query_as(
        "SELECT weight, success_ema, latency_ema_ms, consecutive_failures, is_penalized, resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load runtime row after geo-only update");
    assert_eq!(row.0, 9.25);
    assert_eq!(row.1, 0.11);
    assert_eq!(row.2, Some(321.0));
    assert_eq!(row.3, 7);
    assert_eq!(row.4, 1);
    assert_eq!(row.5, "trace");
    assert_eq!(row.6, "[\"1.1.1.1\"]");
    assert_eq!(row.7, "[\"HK\"]");

    let manager = proxy.forward_proxy.lock().await;
    let runtime = manager
        .runtime(&proxy_url)
        .expect("runtime state should still exist");
    assert!(runtime.weight.is_finite());
    assert_eq!(runtime.resolved_ip_source, "trace");
    assert_eq!(runtime.resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(runtime.resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_keeps_in_memory_match_when_runtime_geo_persist_fails() {
    let db_path = temp_db_path("proxy-runtime-geo-persist-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.0.0.1", "TEST / 1.0.0.1")
        .await;

    sqlx::query("DROP TABLE forward_proxy_runtime")
        .execute(&proxy.key_store.pool)
        .await
        .expect("drop runtime table to force persist failure");

    let (record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:runtime-geo-persist-fallback",
            &geo_origin,
            Some("1.0.0.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should still succeed when runtime geo persistence fails");
    assert_eq!(
        record.primary_proxy_key.as_deref(),
        Some(proxy_url.as_str())
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    proxy
        .forward_proxy_trace_overrides
        .lock()
        .await
        .remove(&proxy_url);

    let (_cached_record, cached_preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:runtime-geo-persist-fallback-reuse",
            "http://127.0.0.1:9/geo",
            Some("1.0.0.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should retain in-memory GEO cache after persist failure");
    assert_eq!(
        cached_preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn record_forward_proxy_attempt_preserves_geo_metadata_written_by_other_tasks() {
    let db_path = temp_db_path("proxy-runtime-geo-preserve-on-health-write");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://1.1.1.1:8080".to_string();
    let refreshed_at = Utc::now().timestamp();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    proxy
        .update_forward_proxy_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            },
            false,
        )
        .await
        .expect("proxy settings updated");

    sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ip_source = 'trace', resolved_ips_json = '[\"1.1.1.1\"]', resolved_regions_json = '[\"HK\"]', geo_refreshed_at = ? WHERE proxy_key = ?",
    )
    .bind(refreshed_at)
    .bind(&proxy_url)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed fresher GEO metadata in store");

    proxy
        .record_forward_proxy_attempt_inner(&proxy_url, true, Some(12.0), None, false)
        .await
        .expect("record attempt should not clobber stored GEO metadata");

    let row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load runtime row after health-only persist");
    assert_eq!(row.0, "trace");
    assert_eq!(row.1, "[\"1.1.1.1\"]");
    assert_eq!(row.2, "[\"HK\"]");
    assert_eq!(row.3, refreshed_at);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn force_refresh_replaces_stale_trace_with_negative_placeholder() {
    let db_path = temp_db_path("proxy-runtime-geo-force-refresh-negative");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.0.0.1", "TEST / 1.0.0.1")
        .await;
    proxy
        .refresh_forward_proxy_geo_metadata(&geo_origin, true)
        .await
        .expect("first force refresh should persist trace metadata");

    let first_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load initial trace runtime row");
    assert_eq!(first_row.0, "trace");
    assert_eq!(first_row.1, "[\"1.0.0.1\"]");
    assert_eq!(first_row.2, "[\"HK\"]");
    assert!(first_row.3 > 0);

    proxy
        .forward_proxy_trace_overrides
        .lock()
        .await
        .remove(&proxy_url);
    tokio::time::sleep(Duration::from_secs(1)).await;

    proxy
        .refresh_forward_proxy_geo_metadata(&geo_origin, true)
        .await
        .expect("second force refresh should replace stale trace with negative placeholder");

    let second_row: (String, String, String, i64) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json, geo_refreshed_at FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed negative runtime row");
    assert_eq!(second_row.0, "negative");
    assert_eq!(second_row.1, "[]");
    assert_eq!(second_row.2, "[]");
    assert!(
        second_row.3 > first_row.3,
        "force refresh failures should replace stale trace data with a fresh negative placeholder timestamp"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_retraces_non_global_trace_cache_without_regions() {
    let db_path = temp_db_path("proxy-runtime-geo-retrace-loopback-trace");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let bad_geo_origin = "http://127.0.0.1:9/geo";
    let refreshed_at = Utc::now().timestamp();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["127.0.0.1".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at = refreshed_at;
    }
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&proxy_url)
            .cloned()
            .expect("persisted runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist seeded runtime state");
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "8.8.8.8", "TEST / 8.8.8.8")
        .await;

    let (_record, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:retrace-loopback-trace-cache",
            bad_geo_origin,
            Some("8.8.8.8"),
            Some("US"),
            None,
        )
        .await
        .expect("selection should retrace non-global cached trace IPs");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String, String) = sqlx::query_as(
        "SELECT resolved_ip_source, resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&proxy_url)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed runtime row");
    assert_eq!(row.0, "trace");
    assert_eq!(row.1, "[\"8.8.8.8\"]");
    assert_eq!(row.2, "[]");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_tracks_remaining_ttl() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-ttl");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }
    proxy
        .set_forward_proxy_trace_override_for_test(&proxy_url, "1.1.1.1", "TEST / 1.1.1.1")
        .await;
    proxy
        .refresh_forward_proxy_geo_metadata(&geo_origin, true)
        .await
        .expect("seed fresh GEO runtime metadata");

    {
        let mut manager = proxy.forward_proxy.lock().await;
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.geo_refreshed_at = Utc::now().timestamp() - (max_age_secs - 5);
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert!(
        (0..=5).contains(&wait_secs),
        "scheduler should wait only the remaining TTL before the first 24h GEO refresh, got {wait_secs}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_backs_off_recent_incomplete_trace_cache() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-incomplete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["8.8.8.8".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at = Utc::now().timestamp();
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert!(
        (1..=FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS).contains(&wait_secs),
        "recent incomplete trace metadata should back off briefly instead of hot-looping, got {wait_secs}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_retries_stale_incomplete_trace_cache_immediately() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-stale-incomplete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["8.8.8.8".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at =
            Utc::now().timestamp() - (FORWARD_PROXY_GEO_NEGATIVE_RETRY_COOLDOWN_SECS + 1);
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert_eq!(
        wait_secs, 0,
        "stale incomplete trace metadata should be retried immediately once the cooldown expires"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn forward_proxy_geo_refresh_wait_secs_does_not_back_off_non_global_trace_cache() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-wait-non-global");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy_url = "http://proxy.invalid:8080".to_string();
    let max_age_secs = 24 * 3600;

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![proxy_url.clone()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let runtime = manager
            .runtime
            .get_mut(&proxy_url)
            .expect("runtime state should exist for proxy");
        runtime.available = true;
        runtime.last_error = None;
        runtime.resolved_ip_source = "trace".to_string();
        runtime.resolved_ips = vec!["127.0.0.1".to_string()];
        runtime.resolved_regions = Vec::new();
        runtime.geo_refreshed_at = Utc::now().timestamp();
    }

    let wait_secs = proxy
        .forward_proxy_geo_refresh_wait_secs(max_age_secs)
        .await;
    assert_eq!(
        wait_secs, 0,
        "non-global incomplete trace metadata should be retried immediately instead of entering the cooldown"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_refreshes_stale_loopback_runtime_geo_metadata_for_xray_route() {
    let db_path = temp_db_path("proxy-runtime-geo-refresh-loopback-xray");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let raw_proxy_url =
        "vless://0688fa59-e971-4278-8c03-4b35821a71dc@1.1.1.1:443?encryption=none#hk";
    let endpoint_key = {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![raw_proxy_url.to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let endpoint = manager
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.raw_url.as_deref() == Some(raw_proxy_url))
            .expect("xray endpoint");
        let endpoint_key = endpoint.key.clone();
        let route_url = Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route");
        endpoint.endpoint_url = Some(route_url.clone());
        let runtime = manager
            .runtime
            .get_mut(&endpoint_key)
            .expect("xray runtime state");
        runtime.endpoint_url = Some(route_url.to_string());
        runtime.available = true;
        runtime.last_error = None;
        endpoint_key
    };
    let persisted_runtime = {
        let manager = proxy.forward_proxy.lock().await;
        manager
            .runtime
            .get(&endpoint_key)
            .cloned()
            .expect("persisted xray runtime state")
    };
    forward_proxy::persist_forward_proxy_runtime_state(&proxy.key_store.pool, &persisted_runtime)
        .await
        .expect("persist initial xray runtime state");

    let updated = sqlx::query(
        "UPDATE forward_proxy_runtime SET resolved_ips_json = '[\"127.0.0.1\"]', resolved_regions_json = '[]' WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed stale loopback runtime geo metadata");
    assert_eq!(
        updated.rows_affected(),
        1,
        "should seed an existing runtime row"
    );

    let reloaded = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reloaded");
    {
        let mut manager = reloaded.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![raw_proxy_url.to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        let endpoint = manager
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.raw_url.as_deref() == Some(raw_proxy_url))
            .expect("reloaded xray endpoint");
        let route_url = Url::parse("socks5h://127.0.0.1:41000").expect("parse local xray route");
        endpoint.endpoint_url = Some(route_url.clone());
        let runtime = manager
            .runtime
            .get_mut(&endpoint_key)
            .expect("reloaded xray runtime state");
        runtime.endpoint_url = Some(route_url.to_string());
        runtime.available = true;
        runtime.last_error = None;
    }

    let (_record, preview) = reloaded
        .select_proxy_affinity_preview_for_registration_with_hint(
            "subject:refresh-runtime-geo-loopback-xray",
            &geo_origin,
            Some("1.1.1.1"),
            Some("HK"),
            None,
        )
        .await
        .expect("selection should refresh stale loopback runtime geo metadata");
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::RegistrationIp)
    );

    let row: (String, String) = sqlx::query_as(
        "SELECT resolved_ips_json, resolved_regions_json FROM forward_proxy_runtime WHERE proxy_key = ?",
    )
    .bind(&endpoint_key)
    .fetch_one(&reloaded.key_store.pool)
    .await
    .expect("load refreshed xray runtime geo metadata");
    let resolved_ips: Vec<String> =
        serde_json::from_str(&row.0).expect("decode refreshed xray resolved ips");
    let resolved_regions: Vec<String> =
        serde_json::from_str(&row.1).expect("decode refreshed xray resolved regions");
    assert_eq!(resolved_ips, vec!["1.1.1.1".to_string()]);
    assert_eq!(resolved_regions, vec!["HK".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_registration_proxy_affinity_persists_and_refreshes() {
    let db_path = temp_db_path("proxy-affinity-registration-persist");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-affinity",
            Some("alpha"),
            Some("18.183.246.69"),
            Some("JP Tokyo (13)"),
            &geo_origin,
        )
        .await
        .expect("key created with proxy affinity");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let created_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load created affinity");
    assert_eq!(
        created_affinity.0.as_deref(),
        Some("http://18.183.246.69:8080")
    );

    let (same_key_id, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-affinity",
            Some("beta"),
            Some("1.1.1.1"),
            Some("HK"),
            &geo_origin,
        )
        .await
        .expect("key refreshed with new proxy affinity");
    assert_eq!(same_key_id, key_id);
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let row: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT group_name, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed key");
    assert_eq!(row.0.as_deref(), Some("alpha"));
    assert_eq!(row.1.as_deref(), Some("1.1.1.1"));
    assert_eq!(row.2.as_deref(), Some("HK"));

    let refreshed_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed affinity");
    assert_eq!(refreshed_affinity.0.as_deref(), Some("http://1.1.1.1:8080"));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_registration_proxy_affinity_hint_keeps_validation_fallback() {
    let db_path = temp_db_path("proxy-affinity-registration-hint");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                    "http://8.8.8.8:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, _) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hinted-fallback",
            None,
            Some("9.9.9.9"),
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hinted proxy affinity");

    let created_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load hinted affinity");
    assert_eq!(
        created_affinity.0.as_deref(),
        Some("http://1.1.1.1:8080"),
        "fallback imports should preserve the proxy chosen during validation"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn key_sticky_nodes_preview_is_read_only_but_uses_effective_assignment() {
    let db_path = temp_db_path("sticky-nodes-preview-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let mut proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    proxy.api_key_geo_origin = geo_origin.clone();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,
                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, _) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration(
            "tvly-sticky-preview",
            None,
            Some("18.183.246.69"),
            Some("JP Tokyo (13)"),
        )
        .await
        .expect("key created without persisted proxy affinity");

    let sticky_nodes = proxy
        .key_sticky_nodes(&key_id)
        .await
        .expect("load sticky node preview");
    assert_eq!(sticky_nodes.nodes.len(), 2);
    assert_eq!(sticky_nodes.nodes[0].role, "primary");
    assert_eq!(
        sticky_nodes.nodes[0].node.key, "http://18.183.246.69:8080",
        "preview should reflect the same effective primary node the request path would pick"
    );

    let persisted_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM forward_proxy_key_affinity WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count affinity rows");
    assert_eq!(
        persisted_count, 0,
        "admin sticky-node preview must not persist or mutate forward proxy affinity"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_persists_across_upsert_paths() {
    let db_path = temp_db_path("proxy-affinity-hint-only-upsert");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-only",
            Some("alpha"),
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hint-only affinity");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let created_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load created hint-only affinity");
    assert_eq!(created_affinity.0.as_deref(), Some("http://1.1.1.1:8080"));

    let (same_key_id, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-only",
            Some("beta"),
            None,
            None,
            &geo_origin,
            Some("http://18.183.246.69:8080"),
        )
        .await
        .expect("key refreshed with hint-only affinity");
    assert_eq!(same_key_id, key_id);
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let refreshed_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load refreshed hint-only affinity");
    assert_eq!(
        refreshed_affinity.0.as_deref(),
        Some("http://18.183.246.69:8080")
    );

    proxy
        .soft_delete_key_by_id(&key_id)
        .await
        .expect("soft delete key before undelete");

    let (_, undeleted_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-only",
            Some("gamma"),
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key undeleted with hint-only affinity");
    assert_eq!(undeleted_status, ApiKeyUpsertStatus::Undeleted);

    let undeleted_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load undeleted hint-only affinity");
    assert_eq!(undeleted_affinity.0.as_deref(), Some("http://1.1.1.1:8080"));

    let row: (Option<i64>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT deleted_at, registration_ip, registration_region FROM api_keys WHERE id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load undeleted key");
    assert!(row.0.is_none(), "undelete should clear deleted_at");
    assert!(
        row.1.is_none() && row.2.is_none(),
        "hint-only imports should not fabricate registration metadata"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_stale_hint_only_proxy_affinity_does_not_persist_fallback() {
    let db_path = temp_db_path("proxy-affinity-stale-hint-only");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-stale-hint-only",
            None,
            None,
            None,
            &geo_origin,
            Some("http://9.9.9.9:8080"),
        )
        .await
        .expect("key created without persisting stale hint");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row");
    assert!(
        affinity_row.0.is_none() && affinity_row.1.is_none(),
        "stale hint-only imports must not silently bind a fallback node"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for stale hint-only key");
    assert!(
        !plan.is_empty(),
        "keys without durable affinity should still get a runtime fallback plan"
    );

    let affinity_row_after_plan: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row after building stale hint plan");
    assert!(
        affinity_row_after_plan.0.is_none() && affinity_row_after_plan.1.is_none(),
        "runtime fallback planning must not backfill durable affinity for stale hints"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_keeps_selected_node_when_temporarily_unavailable()
 {
    let db_path = temp_db_path("proxy-affinity-hint-only-unavailable");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        manager
            .runtime
            .get_mut("http://1.1.1.1:8080")
            .expect("runtime for selected node")
            .available = false;
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-unavailable",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with unavailable hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query persisted unavailable hint affinity");
    assert_eq!(affinity_row.0.as_deref(), Some("http://1.1.1.1:8080"));

    let reconciled = proxy
        .reconcile_proxy_affinity_record(&key_id)
        .await
        .expect("reconcile unavailable hint affinity");
    assert_eq!(
        reconciled.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080"),
        "temporary outages should not discard a caller-pinned hint-only primary"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for unavailable hint-only key");
    assert!(
        plan.iter()
            .all(|candidate| candidate.key != "http://1.1.1.1:8080"),
        "temporarily unavailable pinned nodes should stay durable but not be retried until healthy"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_does_not_route_through_zero_weight_primary()
 {
    let db_path = temp_db_path("proxy-affinity-hint-only-zero-weight");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
        manager
            .runtime
            .get_mut("http://1.1.1.1:8080")
            .expect("runtime for selected node")
            .weight = 0.0;
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-zero-weight",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with zero-weight hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for zero-weight hint-only key");
    assert!(
        plan.iter()
            .all(|candidate| candidate.key != "http://1.1.1.1:8080"),
        "zero-weight pinned nodes should stay durable but not bypass routing weight gates"
    );

    let reconciled = proxy
        .reconcile_proxy_affinity_record(&key_id)
        .await
        .expect("reconcile zero-weight hint affinity");
    assert_eq!(
        reconciled.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080"),
        "runtime weight changes should not erase the stored hint-only primary"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_rebuilds_when_primary_disappears() {
    let db_path = temp_db_path("proxy-affinity-hint-only-rebuilds");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-rebuilds",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec!["http://18.183.246.69:8080".to_string()],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let reconciled = proxy
        .reconcile_proxy_affinity_record(&key_id)
        .await
        .expect("reconcile hint-only affinity after primary removal");
    assert_eq!(
        reconciled.primary_proxy_key.as_deref(),
        Some("http://18.183.246.69:8080"),
        "when a hinted primary disappears entirely, the key should heal onto a remaining candidate"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_refresh_invalidates_cached_record() {
    let db_path = temp_db_path("proxy-affinity-hint-cache-refresh");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-cache-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with hint-only affinity");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let warmed = proxy
        .load_proxy_affinity_record(&key_id)
        .await
        .expect("warm affinity cache");
    assert_eq!(
        warmed.primary_proxy_key.as_deref(),
        Some("http://1.1.1.1:8080")
    );

    let (_, refreshed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-cache-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://18.183.246.69:8080"),
        )
        .await
        .expect("refresh hint-only affinity");
    assert_eq!(refreshed_status, ApiKeyUpsertStatus::Existed);

    let refreshed = proxy
        .load_proxy_affinity_record(&key_id)
        .await
        .expect("reload affinity after refresh");
    assert_eq!(
        refreshed.primary_proxy_key.as_deref(),
        Some("http://18.183.246.69:8080"),
        "re-importing a hinted key should evict stale cache entries before the next request"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_direct_affinity_does_not_persist() {
    let db_path = temp_db_path("proxy-affinity-hint-only-direct");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-direct",
            None,
            None,
            None,
            &geo_origin,
            Some(forward_proxy::FORWARD_PROXY_DIRECT_KEY),
        )
        .await
        .expect("key created without persisting direct hint");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query direct hint affinity row");
    assert!(
        affinity_row.0.is_none() && affinity_row.1.is_none(),
        "direct validation results must not become durable affinity records"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for direct hint-only key");
    assert!(
        !plan.is_empty(),
        "direct-only validation results should still allow runtime fallback selection"
    );

    let affinity_row_after_plan: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query direct hint affinity row after plan build");
    assert!(
        affinity_row_after_plan.0.is_none() && affinity_row_after_plan.1.is_none(),
        "runtime fallback planning must not convert direct hints into durable affinity"
    );

    let marker_updated_at_before: i64 =
        sqlx::query_scalar("SELECT updated_at FROM forward_proxy_key_affinity WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("query marker timestamp before repeat plan build");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let _ = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("rebuild direct-hint runtime plan");
    let marker_updated_at_after: i64 =
        sqlx::query_scalar("SELECT updated_at FROM forward_proxy_key_affinity WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("query marker timestamp after repeat plan build");
    assert_eq!(
        marker_updated_at_after, marker_updated_at_before,
        "explicit empty markers should not churn the database on every runtime plan build"
    );

    proxy
        .promote_proxy_affinity_secondary(&key_id, "http://18.183.246.69:8080")
        .await
        .expect("learn durable affinity after direct hint");
    let learned_affinity: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query learned affinity after first successful routed request");
    assert_eq!(
        learned_affinity.0.as_deref(),
        Some("http://18.183.246.69:8080"),
        "empty affinity markers should be replaceable once a real proxy success is observed"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_direct_affinity_keeps_direct_runtime_fallback() {
    let db_path = temp_db_path("proxy-affinity-hint-only-direct-fallback");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: Vec::new(),
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: true,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-direct-fallback",
            None,
            None,
            None,
            &geo_origin,
            Some(forward_proxy::FORWARD_PROXY_DIRECT_KEY),
        )
        .await
        .expect("key created with direct-only hint");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for direct-only deployment");
    assert_eq!(
        plan.len(),
        1,
        "direct-only deployments should keep one direct fallback"
    );
    assert_eq!(plan[0].key, forward_proxy::FORWARD_PROXY_DIRECT_KEY);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_plain_key_without_registration_metadata_still_synthesizes_affinity() {
    let db_path = temp_db_path("proxy-affinity-plain-key-synthesizes");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, status) = proxy
        .add_or_undelete_key_with_status("tvly-plain-no-registration")
        .await
        .expect("plain key created");
    assert_eq!(status, ApiKeyUpsertStatus::Created);

    let before_plan: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query affinity before runtime reconciliation");
    assert!(
        before_plan.is_none(),
        "plain keys should start without an explicit affinity marker"
    );

    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build attempt plan for plain key");
    assert!(
        !plan.is_empty(),
        "plain keys should still get a ranked runtime attempt plan"
    );

    let synthesized: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query synthesized affinity after runtime reconciliation");
    assert!(
        synthesized.0.is_some(),
        "plain keys must still materialize a durable primary affinity"
    );

    if let Some(secondary) = synthesized.1.clone() {
        proxy
            .promote_proxy_affinity_secondary(&key_id, &secondary)
            .await
            .expect("promote synthesized secondary");
        let promoted: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
        )
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("query promoted affinity");
        assert_eq!(
            promoted.0.as_deref(),
            Some(secondary.as_str()),
            "plain keys should keep the existing self-healing promotion behavior"
        );
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_stale_hint_only_proxy_affinity_clears_existing_affinity() {
    let db_path = temp_db_path("proxy-affinity-stale-hint-clears-existing");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-stale-hint-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("key created with valid hint");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let (_, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-stale-hint-refresh",
            None,
            None,
            None,
            &geo_origin,
            Some("http://9.9.9.9:8080"),
        )
        .await
        .expect("existing key refreshed with stale hint");
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row after stale refresh");
    assert!(
        affinity_row.0.is_none() && affinity_row.1.is_none(),
        "stale hint-only refresh should clear the old affinity instead of keeping it"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn add_or_undelete_key_with_hint_only_proxy_affinity_preserves_existing_registration_affinity()
 {
    let db_path = temp_db_path("proxy-affinity-hint-preserves-registration");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, created_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-hint-preserves-registration",
            None,
            Some("1.1.1.1"),
            Some("US Westfield (MA)"),
            &geo_origin,
        )
        .await
        .expect("key created with registration affinity");
    assert_eq!(created_status, ApiKeyUpsertStatus::Created);

    let (_, existed_status) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity_hint(
            "tvly-hint-preserves-registration",
            None,
            None,
            None,
            &geo_origin,
            Some("http://18.183.246.69:8080"),
        )
        .await
        .expect("existing registration key refreshed with hint-only payload");
    assert_eq!(existed_status, ApiKeyUpsertStatus::Existed);

    let affinity_row: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT primary_proxy_key, secondary_proxy_key FROM forward_proxy_key_affinity WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("query affinity row after hint-only refresh");
    assert_eq!(
        affinity_row.0.as_deref(),
        Some("http://1.1.1.1:8080"),
        "hint-only refresh must not override durable registration-based affinity"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn select_proxy_affinity_for_same_region_uses_ranked_match_within_region_candidates() {
    let db_path = temp_db_path("proxy-affinity-region-ranked-match");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://1.1.1.1:8080".to_string(),
                    "http://1.0.0.1:8080".to_string(),
                    "http://18.183.246.69:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: false,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (subject, expected_primary) = {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.ensure_non_zero_weight();
        (0..256usize)
            .filter_map(|index| {
                let subject = format!("subject:ranked-region:{index}");
                let ranked =
                    manager.rank_candidates_for_subject(&subject, &HashSet::new(), false, 3);
                let first_hk = ranked.into_iter().find(|endpoint| {
                    matches!(
                        endpoint.key.as_str(),
                        "http://1.1.1.1:8080" | "http://1.0.0.1:8080"
                    )
                })?;
                (first_hk.key == "http://1.0.0.1:8080").then_some((subject, first_hk.key))
            })
            .next()
            .expect("find subject whose ranked HK candidate is not the first configured node")
    };

    let (affinity, preview) = proxy
        .select_proxy_affinity_preview_for_registration_with_hint(
            &subject,
            &geo_origin,
            Some("103.232.214.107"),
            Some("HK"),
            Some("http://1.1.1.1:8080"),
        )
        .await
        .expect("same-region proxy affinity");
    assert_eq!(
        affinity.primary_proxy_key.as_deref(),
        Some(expected_primary.as_str()),
        "same-region selection should stay inside the region-matched set and follow ranked order"
    );
    assert_eq!(
        preview.as_ref().map(|item| item.match_kind),
        Some(AssignedProxyMatchKind::SameRegion),
        "same-region ranked picks should still report same_region"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn build_proxy_attempt_plan_prefers_same_region_candidates_before_other_regions() {
    let db_path = temp_db_path("proxy-attempt-plan-region-order");
    let db_str = db_path.to_string_lossy().to_string();
    let geo_addr = spawn_api_key_geo_mock_server().await;
    let geo_origin = format!("http://{geo_addr}/geo");

    let mut proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    proxy.api_key_geo_origin = geo_origin.clone();
    {
        let mut manager = proxy.forward_proxy.lock().await;
        manager.apply_settings(
            ForwardProxySettings {
                proxy_urls: vec![
                    "http://18.183.246.69:8080".to_string(),
                    "http://1.1.1.1:8080".to_string(),
                    "http://1.0.0.1:8080".to_string(),
                    "http://8.8.8.8:8080".to_string(),
                ],
                subscription_urls: Vec::new(),
                subscription_update_interval_secs: 3600,
                insert_direct: true,

                egress_socks5_enabled: false,
                egress_socks5_url: String::new(),
            }
            .normalized(),
        );
    }

    let (key_id, _) = proxy
        .add_or_undelete_key_with_status_in_group_and_registration_proxy_affinity(
            "tvly-region-plan",
            None,
            Some("1.1.1.1"),
            Some("HK"),
            &geo_origin,
        )
        .await
        .expect("key created with region-aware proxy affinity");
    let plan = proxy
        .build_proxy_attempt_plan(&key_id)
        .await
        .expect("build proxy attempt plan");
    let plan_keys = plan.into_iter().map(|item| item.key).collect::<Vec<_>>();

    assert_eq!(
        plan_keys.first().map(String::as_str),
        Some("http://1.1.1.1:8080")
    );
    let same_region_pos = plan_keys
        .iter()
        .position(|key| key == "http://1.0.0.1:8080")
        .expect("same-region backup present in plan");
    let other_region_positions = ["http://18.183.246.69:8080", "http://8.8.8.8:8080"]
        .into_iter()
        .filter_map(|key| plan_keys.iter().position(|item| item == key))
        .collect::<Vec<_>>();
    assert!(
        other_region_positions
            .iter()
            .all(|position| same_region_pos < *position),
        "same-region backup should be attempted before other-region candidates"
    );
    assert!(
        !plan_keys
            .iter()
            .any(|key| key == forward_proxy::FORWARD_PROXY_DIRECT_KEY),
        "direct should only be considered after all proxy candidates fail"
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn analyze_http_attempt_treats_2xx_as_success() {
    let body = br#"{"query":"test","results":[]}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_SUCCESS);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn analyze_http_attempt_uses_structured_status_and_marks_quota_exhausted() {
    let body = br#"{"status":432,"error":"quota_exhausted"}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.tavily_status_code, Some(432));
}

#[test]
fn analyze_http_attempt_treats_http_errors_as_error() {
    let body = br#"{"error":"upstream failed"}"#;
    let analysis = analyze_http_attempt(StatusCode::INTERNAL_SERVER_ERROR, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(500));
}

#[test]
fn analyze_http_attempt_treats_failed_status_string_as_error() {
    let body = br#"{"status":"failed"}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn analyze_http_attempt_treats_pending_status_string_as_success() {
    let body = br#"{"status":"pending"}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_SUCCESS);
    assert_eq!(analysis.key_health_action, KeyHealthAction::None);
    assert_eq!(analysis.tavily_status_code, Some(200));
}

#[test]
fn analyze_http_attempt_prioritizes_structured_status_code_for_quota_exhausted() {
    let body = br#"{"status":432,"detail":{"status":"failed"}}"#;
    let analysis = analyze_http_attempt(StatusCode::OK, body);
    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.tavily_status_code, Some(432));
}

#[test]
fn analyze_http_attempt_marks_401_deactivated_as_quarantine() {
    let body =
        br#"{"detail":{"error":"The account associated with this API key has been deactivated."}}"#;
    let analysis = analyze_http_attempt(StatusCode::UNAUTHORIZED, body);
    assert_eq!(analysis.status, OUTCOME_ERROR);
    match analysis.key_health_action {
        KeyHealthAction::Quarantine(decision) => {
            assert_eq!(decision.reason_code, "account_deactivated");
            assert!(decision.reason_summary.contains("HTTP 401"));
        }
        other => panic!("expected quarantine action, got {other:?}"),
    }
    assert_eq!(analysis.tavily_status_code, Some(401));
}

#[test]
fn extract_research_request_id_accepts_snake_and_camel_case() {
    let snake = br#"{"request_id":"req-snake"}"#;
    let camel = br#"{"requestId":"req-camel"}"#;
    assert_eq!(
        extract_research_request_id(snake).as_deref(),
        Some("req-snake")
    );
    assert_eq!(
        extract_research_request_id(camel).as_deref(),
        Some("req-camel")
    );
}

#[test]
fn extract_research_request_id_from_path_decodes_segment() {
    assert_eq!(
        extract_research_request_id_from_path("/research/req%2Fabc").as_deref(),
        Some("req/abc")
    );
}

#[test]
fn redact_api_key_bytes_removes_api_key_value() {
    let input = br#"{"api_key":"th-ABCD-secret","nested":{"api_key":"tvly-secret"}}"#;
    let redacted = redact_api_key_bytes(input);
    let text = String::from_utf8_lossy(&redacted);
    assert!(
        !text.contains("th-ABCD-secret") && !text.contains("tvly-secret"),
        "redacted payload should not contain original secrets"
    );
    assert!(
        text.contains("\"api_key\":\"***redacted***\""),
        "api_key fields should be replaced with placeholder"
    );
}

#[tokio::test]
async fn proxy_http_search_marks_key_exhausted_on_quota_status() {
    let db_path = temp_db_path("http-search-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-quota-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    // Mock Tavily HTTP /search that always returns structured status 432.
    let app = Router::new().route(
        "/search",
        post(|| async {
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": 432,
                    "error": "quota_exhausted",
                })),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);

    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test" });

    let (_resp, analysis) = proxy
        .proxy_http_search(
            &usage_base,
            Some("tok1"),
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
        )
        .await
        .expect("proxy search succeeded");

    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.tavily_status_code, Some(432));

    // Verify that the key is marked exhausted in the database.
    let store = proxy.key_store.clone();
    let (status,): (String,) = sqlx::query_as("SELECT status FROM api_keys WHERE api_key = ?")
        .bind(expected_api_key)
        .fetch_one(&store.pool)
        .await
        .expect("key row exists");
    assert_eq!(status, STATUS_EXHAUSTED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_http_json_endpoint_injects_bearer_auth_when_enabled() {
    let db_path = temp_db_path("http-json-bearer-enabled");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-bearer-enabled-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let app = Router::new().route(
        "/search",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.to_string();
                async move {
                    let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(api_key, expected_api_key);

                    let authorization = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    let expected_auth = format!("Bearer {}", expected_api_key);
                    assert_eq!(
                        authorization, expected_auth,
                        "upstream authorization should use Tavily key"
                    );
                    assert!(
                        !authorization.starts_with("Bearer th-"),
                        "upstream authorization must not be Hikari token"
                    );

                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);
    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test" });

    let _ = proxy
        .proxy_http_json_endpoint(
            &usage_base,
            "/search",
            Some("tok1"),
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
            true,
        )
        .await
        .expect("proxy request succeeds");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_http_json_endpoint_quarantines_key_on_401_deactivated() {
    let db_path = temp_db_path("http-json-quarantine-401");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-quarantine-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let app = Router::new().route(
        "/search",
        post(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "detail": {
                        "error": "The account associated with this API key has been deactivated."
                    }
                })),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);
    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test" });

    let (_resp, analysis) = proxy
        .proxy_http_search(
            &usage_base,
            Some("tok1"),
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
        )
        .await
        .expect("proxy search succeeded");

    assert_eq!(analysis.status, OUTCOME_ERROR);
    match analysis.key_health_action {
        KeyHealthAction::Quarantine(ref decision) => {
            assert_eq!(decision.reason_code, "account_deactivated");
        }
        ref other => panic!("expected quarantine action, got {other:?}"),
    }

    let store = proxy.key_store.clone();
    let (status,): (String,) = sqlx::query_as("SELECT status FROM api_keys WHERE api_key = ?")
        .bind(expected_api_key)
        .fetch_one(&store.pool)
        .await
        .expect("key row exists");
    assert_eq!(status, STATUS_ACTIVE);

    let quarantine_row = sqlx::query(
        r#"SELECT source, reason_code, cleared_at FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&store.pool)
    .await
    .expect("quarantine row exists");
    let source: String = quarantine_row.try_get("source").expect("source");
    let reason_code: String = quarantine_row.try_get("reason_code").expect("reason_code");
    let cleared_at: Option<i64> = quarantine_row.try_get("cleared_at").expect("cleared_at");
    assert_eq!(source, "/api/tavily/search");
    assert_eq!(reason_code, "account_deactivated");
    assert!(cleared_at.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_request_quarantines_key_on_mcp_unauthorized() {
    let db_path = temp_db_path("mcp-quarantine-401");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "message": "Unauthorized: invalid api key"
                    },
                    "id": 1
                })),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let expected_api_key = "tvly-mcp-quarantine-key";
    let upstream = format!("http://{addr}/mcp");
    let proxy = TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
        .await
        .expect("proxy created");

    let request = ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
        auth_token_id: Some("tok1".to_string()),
    };

    let response = proxy.proxy_request(request).await.expect("proxy response");
    assert_eq!(response.status, StatusCode::UNAUTHORIZED);

    let store = proxy.key_store.clone();
    let quarantine_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_request_quarantines_key_on_mcp_error_body_without_http_status() {
    let db_path = temp_db_path("mcp-quarantine-jsonrpc-error");
    let db_str = db_path.to_string_lossy().to_string();

    let app = Router::new().route(
        "/mcp",
        post(|| async {
            Json(serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "message": "Unauthorized: invalid api key"
                },
                "id": 1
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

    let expected_api_key = "tvly-mcp-jsonrpc-error-key";
    let upstream = format!("http://{addr}/mcp");
    let proxy = TavilyProxy::with_endpoint(vec![expected_api_key.to_string()], &upstream, &db_str)
        .await
        .expect("proxy created");

    let request = ProxyRequest {
        method: Method::POST,
        path: "/mcp".to_string(),
        query: None,
        headers: HeaderMap::new(),
        body: Bytes::from_static(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#),
        auth_token_id: Some("tok1".to_string()),
    };

    let response = proxy.proxy_request(request).await.expect("proxy response");
    assert_eq!(response.status, StatusCode::OK);

    let quarantine_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_result_keeps_affinity_when_original_key_is_quarantined() {
    let db_path = temp_db_path("research-affinity-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-research-affinity-a".to_string(),
            "tvly-research-affinity-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (affinity_key_id, _other_key_id) =
        rows.into_iter()
            .fold((None, None), |mut acc, (id, secret)| {
                if secret == "tvly-research-affinity-a" {
                    acc.0 = Some(id);
                } else if secret == "tvly-research-affinity-b" {
                    acc.1 = Some(id);
                }
                acc
            });
    let affinity_key_id = affinity_key_id.expect("affinity key exists");
    let request_id = "req-affinity-quarantine";

    proxy
        .record_research_request_affinity(request_id, &affinity_key_id, "tok1")
        .await
        .expect("record research affinity");
    proxy
        .key_store
        .quarantine_key_by_id(
            &affinity_key_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine affinity key");

    let err = proxy
        .acquire_key_for_research_request(Some("tok1"), Some(request_id))
        .await
        .expect_err("result retrieval should not fall back to a different key");
    assert!(matches!(err, ProxyError::NoAvailableKeys));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn classify_quarantine_reason_ignores_generic_unauthorized_errors() {
    let unauthorized = classify_quarantine_reason(Some(401), br#"{"error":"unauthorized"}"#);
    assert!(unauthorized.is_none());

    let forbidden = classify_quarantine_reason(Some(403), br#"{"error":"forbidden"}"#);
    assert!(forbidden.is_none());

    let invalid_payload_key =
        classify_quarantine_reason(None, br#"{"error":"invalid key \"depth\""}"#);
    assert!(invalid_payload_key.is_none());
}

#[tokio::test]
async fn quarantined_keys_are_excluded_until_admin_clears_them() {
    let db_path = temp_db_path("quarantine-acquire");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-quarantine-a".to_string(),
            "tvly-quarantine-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (first_id, _first_secret) = rows
        .into_iter()
        .find(|(_, secret)| secret == "tvly-quarantine-a")
        .expect("first key exists");

    assert!(
        proxy
            .key_store
            .try_acquire_specific_key(&first_id)
            .await
            .expect("acquire specific before quarantine")
            .is_some()
    );

    proxy
        .key_store
        .quarantine_key_by_id(
            &first_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine key");

    assert!(
        proxy
            .key_store
            .try_acquire_specific_key(&first_id)
            .await
            .expect("acquire specific after quarantine")
            .is_none()
    );

    let summary = proxy.summary().await.expect("summary");
    assert_eq!(summary.active_keys, 1);
    assert_eq!(summary.quarantined_keys, 1);

    proxy
        .clear_key_quarantine_by_id(&first_id)
        .await
        .expect("clear quarantine");

    assert!(
        proxy
            .key_store
            .try_acquire_specific_key(&first_id)
            .await
            .expect("acquire specific after clear")
            .is_some()
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn quarantine_key_by_id_is_safe_under_concurrent_calls() {
    let db_path = temp_db_path("quarantine-concurrent");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-quarantine-race".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seeded key id");
    let store = proxy.key_store.clone();

    let first = {
        let store = store.clone();
        let key_id = key_id.clone();
        async move {
            store
                .quarantine_key_by_id(
                    &key_id,
                    "/api/tavily/search",
                    "account_deactivated",
                    "Tavily account deactivated (HTTP 401)",
                    "first detail",
                )
                .await
        }
    };
    let second = {
        let store = store.clone();
        let key_id = key_id.clone();
        async move {
            store
                .quarantine_key_by_id(
                    &key_id,
                    "/api/tavily/search",
                    "account_deactivated",
                    "Tavily account deactivated (HTTP 401)",
                    "second detail",
                )
                .await
        }
    };

    let (first_result, second_result) = tokio::join!(first, second);
    first_result.expect("first quarantine succeeds");
    second_result.expect("second quarantine succeeds");

    let quarantine_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn quarantine_key_by_id_preserves_original_created_at() {
    let db_path = temp_db_path("quarantine-created-at");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-quarantine-created-at".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seeded key id");

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "first detail",
        )
        .await
        .expect("first quarantine");

    let first_created_at: i64 = sqlx::query_scalar(
        "SELECT created_at FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("first created_at");

    tokio::time::sleep(Duration::from_secs(1)).await;

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "second detail",
        )
        .await
        .expect("second quarantine");

    let second_created_at: i64 = sqlx::query_scalar(
        "SELECT created_at FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("second created_at");
    assert_eq!(second_created_at, first_created_at);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn list_keys_pending_quota_sync_skips_quarantined_keys() {
    let db_path = temp_db_path("quota-sync-skip-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-quota-sync-a".to_string(),
            "tvly-quota-sync-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (quarantined_id, active_id) =
        rows.into_iter()
            .fold((None, None), |mut acc, (id, secret)| {
                if secret == "tvly-quota-sync-a" {
                    acc.0 = Some(id);
                } else if secret == "tvly-quota-sync-b" {
                    acc.1 = Some(id);
                }
                acc
            });
    let quarantined_id = quarantined_id.expect("quarantined key exists");
    let active_id = active_id.expect("active key exists");

    proxy
        .key_store
        .quarantine_key_by_id(
            &quarantined_id,
            "/api/tavily/usage",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine key");

    let pending = proxy
        .list_keys_pending_quota_sync(24 * 60 * 60)
        .await
        .expect("list pending keys");
    assert_eq!(pending, vec![active_id]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_quota_totals_exclude_quarantined_keys() {
    let db_path = temp_db_path("summary-quota-excludes-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-summary-quota-a".to_string(),
            "tvly-summary-quota-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id, api_key FROM api_keys ORDER BY api_key ASC",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch keys");
    let (quarantined_id, active_id) =
        rows.into_iter()
            .fold((None, None), |mut acc, (id, secret)| {
                if secret == "tvly-summary-quota-a" {
                    acc.0 = Some(id);
                } else if secret == "tvly-summary-quota-b" {
                    acc.1 = Some(id);
                }
                acc
            });
    let quarantined_id = quarantined_id.expect("quarantined key exists");
    let active_id = active_id.expect("active key exists");

    proxy
        .key_store
        .update_quota_for_key(&quarantined_id, 100, 80, Utc::now().timestamp())
        .await
        .expect("update quarantined key quota");
    proxy
        .key_store
        .update_quota_for_key(&active_id, 50, 40, Utc::now().timestamp())
        .await
        .expect("update active key quota");
    proxy
        .key_store
        .quarantine_key_by_id(
            &quarantined_id,
            "/api/tavily/search",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("quarantine key");

    let summary = proxy.summary().await.expect("summary");
    assert_eq!(summary.total_quota_limit, 50);
    assert_eq!(summary.total_quota_remaining, 40);

    let _ = std::fs::remove_file(db_path);
}

async fn insert_summary_window_bucket(
    proxy: &TavilyProxy,
    key_id: &str,
    bucket_start: i64,
    total_requests: i64,
    success_count: i64,
    error_count: i64,
    quota_exhausted_count: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO api_key_usage_buckets (
            api_key_id,
            bucket_start,
            bucket_secs,
            total_requests,
            success_count,
            error_count,
            quota_exhausted_count,
            updated_at
        ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(key_id)
    .bind(bucket_start)
    .bind(total_requests)
    .bind(success_count)
    .bind(error_count)
    .bind(quota_exhausted_count)
    .bind(bucket_start + 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert summary window bucket");
}

async fn insert_summary_window_logs(
    proxy: &TavilyProxy,
    key_id: &str,
    created_at: i64,
    outcome: &str,
    count: usize,
) {
    for offset in 0..count {
        sqlx::query(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (?, NULL, 'GET', '/v1/search', NULL, 200, 200, NULL, ?, NULL, NULL, '[]', '[]', ?)
            "#,
        )
        .bind(key_id)
        .bind(outcome)
        .bind(created_at + offset as i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert summary window log");
    }
}

#[tokio::test]
async fn summary_windows_split_today_yesterday_and_month() {
    let db_path = temp_db_path("summary-windows-split");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-a".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);
    let yesterday_start = previous_local_day_start_utc_ts(now);
    let yesterday_same_time = previous_local_same_time_utc_ts(now);
    let month_start = start_of_local_month_utc_ts(now);
    let previous_month_start = start_of_local_month_utc_ts(now - chrono::Duration::days(32));

    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_SUCCESS, 9).await;
    insert_summary_window_logs(&proxy, &key_id, today_start + 3600, OUTCOME_ERROR, 2).await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        today_start + 7200,
        OUTCOME_QUOTA_EXHAUSTED,
        1,
    )
    .await;
    insert_summary_window_logs(&proxy, &key_id, yesterday_start + 60, OUTCOME_SUCCESS, 5).await;
    insert_summary_window_logs(&proxy, &key_id, yesterday_start + 3600, OUTCOME_ERROR, 1).await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        yesterday_start + 7200,
        OUTCOME_QUOTA_EXHAUSTED,
        1,
    )
    .await;
    insert_summary_window_logs(
        &proxy,
        &key_id,
        yesterday_same_time + 60,
        OUTCOME_SUCCESS,
        3,
    )
    .await;

    insert_summary_window_bucket(&proxy, &key_id, today_start, 12, 9, 2, 1).await;
    insert_summary_window_bucket(&proxy, &key_id, yesterday_start, 10, 8, 1, 1).await;
    let mut expected_month = SummaryWindowMetrics {
        total_requests: 12,
        success_count: 9,
        error_count: 2,
        quota_exhausted_count: 1,
        new_keys: 1,
        new_quarantines: 0,
    };
    if yesterday_start >= month_start {
        expected_month.total_requests += 10;
        expected_month.success_count += 8;
        expected_month.error_count += 1;
        expected_month.quota_exhausted_count += 1;
    }
    if month_start < yesterday_start {
        insert_summary_window_bucket(&proxy, &key_id, month_start, 3, 2, 1, 0).await;
        expected_month.total_requests += 3;
        expected_month.success_count += 2;
        expected_month.error_count += 1;
    }
    insert_summary_window_bucket(&proxy, &key_id, previous_month_start, 99, 80, 10, 9).await;

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");

    assert_eq!(
        summary.today,
        SummaryWindowMetrics {
            total_requests: 12,
            success_count: 9,
            error_count: 2,
            quota_exhausted_count: 1,
            ..SummaryWindowMetrics::default()
        }
    );
    assert_eq!(
        summary.yesterday,
        SummaryWindowMetrics {
            total_requests: 7,
            success_count: 5,
            error_count: 1,
            quota_exhausted_count: 1,
            ..SummaryWindowMetrics::default()
        }
    );
    assert_eq!(summary.month, expected_month);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn summary_windows_return_zero_for_empty_yesterday_bucket() {
    let db_path = temp_db_path("summary-windows-empty-yesterday");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-b".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;

    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);
    insert_summary_window_logs(&proxy, &key_id, today_start + 60, OUTCOME_SUCCESS, 4).await;
    insert_summary_window_logs(&proxy, &key_id, today_start + 3600, OUTCOME_ERROR, 1).await;
    insert_summary_window_bucket(&proxy, &key_id, today_start, 5, 4, 1, 0).await;

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");
    assert_eq!(summary.yesterday, SummaryWindowMetrics::default());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_usage_probe_401_quarantines_key() {
    let db_path = temp_db_path("research-usage-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-research-quarantine-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let app = Router::new().route(
        "/usage",
        get(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "invalid api key",
                })),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);
    let headers = HeaderMap::new();
    let options = serde_json::json!({ "query": "test research" });

    let err = proxy
        .proxy_http_research_with_usage_diff(
            &usage_base,
            Some("tok1"),
            &Method::POST,
            "/api/tavily/research",
            options,
            &headers,
            false,
        )
        .await
        .expect_err("research should fail when usage probe is unauthorized");

    match err {
        ProxyError::UsageHttp { status, .. } => {
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        other => panic!("expected usage http error, got {other:?}"),
    }

    let quarantine_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM api_key_quarantines
           WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?) AND cleared_at IS NULL"#,
    )
    .bind(expected_api_key)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn sync_key_quota_quarantines_usage_auth_failures() {
    let db_path = temp_db_path("sync-usage-quarantine");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-sync-quarantine".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("seeded key id");

    let app = Router::new().route(
        "/usage",
        get(|| async {
            (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "detail": {
                        "error": "The account associated with this API key has been deactivated."
                    }
                })),
            )
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{addr}");
    let err = proxy
        .sync_key_quota(&key_id, &usage_base)
        .await
        .expect_err("sync should fail");
    match err {
        ProxyError::UsageHttp { status, .. } => assert_eq!(status, StatusCode::UNAUTHORIZED),
        other => panic!("expected usage http error, got {other:?}"),
    }

    let quarantine_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantine rows");
    assert_eq!(quarantine_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn proxy_http_json_endpoint_does_not_inject_bearer_auth_when_disabled() {
    let db_path = temp_db_path("http-json-bearer-disabled");
    let db_str = db_path.to_string_lossy().to_string();

    let expected_api_key = "tvly-http-bearer-disabled-key";
    let proxy = TavilyProxy::with_endpoint(
        vec![expected_api_key.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let app = Router::new().route(
        "/search",
        post({
            move |headers: HeaderMap, Json(body): Json<Value>| {
                let expected_api_key = expected_api_key.to_string();
                async move {
                    let api_key = body.get("api_key").and_then(|v| v.as_str()).unwrap_or("");
                    assert_eq!(api_key, expected_api_key);
                    assert!(
                        headers.get(axum::http::header::AUTHORIZATION).is_none(),
                        "upstream authorization should be absent when injection is disabled"
                    );
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "status": 200,
                            "results": [],
                        })),
                    )
                }
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });

    let usage_base = format!("http://{}", addr);
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        HeaderValue::from_static("Bearer th-client-token"),
    );
    let options = serde_json::json!({ "query": "test" });

    let _ = proxy
        .proxy_http_json_endpoint(
            &usage_base,
            "/search",
            Some("tok1"),
            &Method::POST,
            "/api/tavily/search",
            options,
            &headers,
            false,
        )
        .await
        .expect("proxy request succeeds");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn quota_blocks_after_hourly_limit() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("quota-test");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy.create_access_token(None).await.expect("token");

    let hourly_limit = effective_token_hourly_limit();

    for _ in 0..hourly_limit {
        let verdict = proxy
            .check_token_quota(&token.id)
            .await
            .expect("quota check ok");
        assert!(verdict.allowed, "should be allowed within limit");
    }

    let verdict = proxy
        .check_token_quota(&token.id)
        .await
        .expect("quota check ok");
    assert!(!verdict.allowed, "expected hourly limit to block");
    assert_eq!(verdict.exceeded_window, Some(QuotaWindow::Hour));

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn quota_window_name_reports_exhausted_when_at_limit() {
    let verdict = TokenQuotaVerdict::new(2, 2, 0, 10, 0, 100);
    assert!(verdict.allowed, "at-limit is not considered exceeded");
    assert_eq!(verdict.window_name(), Some("hour"));
    assert_eq!(verdict.state_key(), "hour");
}

#[tokio::test]
async fn hourly_any_request_limit_blocks_after_threshold() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("any-limit-test");
    let db_str = db_path.to_string_lossy().to_string();

    // Force hourly raw request limit to a small number for this test.
    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "2");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("any-limit"))
        .await
        .expect("create token");

    let hourly_limit = effective_token_hourly_request_limit();

    for _ in 0..hourly_limit {
        let verdict = proxy
            .check_token_hourly_requests(&token.id)
            .await
            .expect("hourly-any check ok");
        assert!(verdict.allowed, "should be allowed within hourly-any limit");
    }

    let verdict = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("hourly-any check ok");
    assert!(
        !verdict.allowed,
        "expected hourly-any limit to block additional requests"
    );
    assert_eq!(verdict.hourly_limit, hourly_limit);

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn delete_access_token_soft_deletes_and_hides_from_list() {
    let db_path = temp_db_path("token-delete");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let token = proxy
        .create_access_token(Some("soft-delete-test"))
        .await
        .expect("create token");

    // Sanity check: token is visible before delete.
    let tokens_before = proxy
        .list_access_tokens()
        .await
        .expect("list tokens before delete");
    assert!(
        tokens_before.iter().any(|t| t.id == token.id),
        "token should appear in list before delete"
    );

    // Inspect raw row to confirm it's enabled and not deleted.
    let store = proxy.key_store.clone();
    let (enabled_before, deleted_at_before): (i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(&token.id)
            .fetch_one(&store.pool)
            .await
            .expect("token row exists before delete");
    assert_eq!(enabled_before, 1);
    assert!(
        deleted_at_before.is_none(),
        "deleted_at should be NULL before delete"
    );

    // Perform delete via public API (soft delete).
    proxy
        .delete_access_token(&token.id)
        .await
        .expect("delete token");

    // Row still exists but marked disabled and soft-deleted.
    let (enabled_after, deleted_at_after): (i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(&token.id)
            .fetch_one(&store.pool)
            .await
            .expect("token row exists after delete");
    assert_eq!(enabled_after, 0, "token should be disabled after delete");
    assert!(
        deleted_at_after.is_some(),
        "deleted_at should be set after delete"
    );

    // Token is no longer returned from management listing.
    let tokens_after = proxy
        .list_access_tokens()
        .await
        .expect("list tokens after delete");
    assert!(
        tokens_after.iter().all(|t| t.id != token.id),
        "soft-deleted token should not appear in list"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_counts_only_billable_logs() {
    let db_path = temp_db_path("rollup-billable");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-billable"))
        .await
        .expect("create token");

    let store = proxy.key_store.clone();
    let base_ts = 1_700_000_000i64;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(base_ts)
    .execute(&store.pool)
    .await
    .expect("insert billable log");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 0, ?)
        "#,
    )
    .bind(&token.id)
    .bind(base_ts + 10)
    .execute(&store.pool)
    .await
    .expect("insert nonbillable log");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");

    let (success, system, external, quota): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT success_count, system_failure_count, external_failure_count, quota_exhausted_count FROM token_usage_stats WHERE token_id = ?",
    )
    .bind(&token.id)
    .fetch_one(&store.pool)
    .await
    .expect("stats row after first rollup");
    assert_eq!(success, 1, "should count only billable logs");
    assert_eq!(
        system + external + quota,
        0,
        "no failure counts expected in this test"
    );

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(base_ts + 20)
    .execute(&store.pool)
    .await
    .expect("insert second billable log");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");

    let (success_after,): (i64,) =
        sqlx::query_as("SELECT success_count FROM token_usage_stats WHERE token_id = ?")
            .bind(&token.id)
            .fetch_one(&store.pool)
            .await
            .expect("stats row after second rollup");
    assert_eq!(
        success_after, 2,
        "bucket should grow by billable increments"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_is_idempotent_without_new_logs() {
    let db_path = temp_db_path("rollup-idempotent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-idempotent"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let ts = 1_700_001_000i64;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(ts)
    .execute(&store.pool)
    .await
    .expect("insert billable log");

    let first = proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");
    assert!(first.0 > 0, "first rollup should process at least one row");

    let after_first = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after first rollup");
    assert_eq!(after_first.total_requests, 1);
    assert_eq!(after_first.success_count, 1);

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");
    assert_eq!(second.0, 0, "second rollup should be a no-op");
    assert!(second.1.is_none(), "second rollup should return no max ts");

    let after_second = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after second rollup");
    assert_eq!(after_second.total_requests, 1);
    assert_eq!(after_second.success_count, 1);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_processes_same_second_log_once() {
    let db_path = temp_db_path("rollup-same-second");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-same-second"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let ts = 1_700_002_000i64;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(ts)
    .execute(&store.pool)
    .await
    .expect("insert first log");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(ts)
    .execute(&store.pool)
    .await
    .expect("insert second log with same second");

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");
    assert!(second.0 > 0, "second rollup should process the new row");

    let after_second = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after second rollup");
    assert_eq!(after_second.total_requests, 2);
    assert_eq!(after_second.success_count, 2);

    let third = proxy
        .rollup_token_usage_stats()
        .await
        .expect("third rollup");
    assert_eq!(third.0, 0, "third rollup should be a no-op");

    let after_third = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after third rollup");
    assert_eq!(after_third.total_requests, 2);
    assert_eq!(after_third.success_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_migrates_legacy_timestamp_cursor() {
    let db_path = temp_db_path("rollup-legacy-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-cursor"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let base_ts = 1_700_003_000i64;

    for offset in [0_i64, 10, 20] {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(base_ts + offset)
        .execute(&store.pool)
        .await
        .expect("insert log");
    }

    // Simulate pre-v2 state with only the legacy timestamp cursor present.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .execute(&store.pool)
        .await
        .expect("delete v2 cursor");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
    .bind((base_ts + 10).to_string())
    .execute(&store.pool)
    .await
    .expect("set legacy cursor");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("rollup with migrated cursor");

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after migrated rollup");
    assert_eq!(
        summary.total_requests, 2,
        "migration should include boundary-second rows to avoid undercount on legacy_ts"
    );
    assert_eq!(summary.success_count, 2);

    let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("max log id")
    .expect("max log id should exist");
    let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .fetch_one(&store.pool)
        .await
        .expect("v2 cursor exists");
    let cursor_v2 = cursor_v2_raw
        .parse::<i64>()
        .expect("v2 cursor should be numeric");
    assert_eq!(cursor_v2, expected_last_id);

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup after migration");
    assert_eq!(second.0, 0, "should not reprocess previous logs");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_migration_handles_out_of_order_timestamps() {
    let db_path = temp_db_path("rollup-legacy-cursor-out-of-order");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-cursor-out-of-order"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let legacy_ts = 1_700_020_000i64;

    // Insert a newer log first, then an older-timestamp log second to create id/timestamp disorder.
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(legacy_ts + 100)
    .execute(&store.pool)
    .await
    .expect("insert newer log first");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(legacy_ts - 100)
    .execute(&store.pool)
    .await
    .expect("insert older log second");

    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .execute(&store.pool)
        .await
        .expect("delete v2 cursor");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
    .bind(legacy_ts.to_string())
    .execute(&store.pool)
    .await
    .expect("set legacy cursor");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("rollup with out-of-order migration");

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after migration");
    assert_eq!(
        summary.total_requests, 1,
        "migration should include all logs newer than legacy_ts even when id/timestamp are out of order"
    );
    assert_eq!(summary.success_count, 1);

    let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("max log id")
    .expect("max log id should exist");
    let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .fetch_one(&store.pool)
        .await
        .expect("v2 cursor exists");
    let cursor_v2 = cursor_v2_raw
        .parse::<i64>()
        .expect("v2 cursor should be numeric");
    assert_eq!(cursor_v2, expected_last_id);

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup after migration");
    assert_eq!(second.0, 0, "second rollup should be a no-op");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_migration_includes_same_second_boundary_logs() {
    let db_path = temp_db_path("rollup-legacy-cursor-same-second");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-cursor-same-second"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let legacy_ts = 1_700_030_000i64;

    for _ in 0..2 {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
            ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
            "#,
        )
        .bind(&token.id)
        .bind(legacy_ts)
        .execute(&store.pool)
        .await
        .expect("insert same-second log");
    }

    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .execute(&store.pool)
        .await
        .expect("delete v2 cursor");
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
    .bind(legacy_ts.to_string())
    .execute(&store.pool)
    .await
    .expect("set legacy cursor");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("rollup with same-second migration boundary");

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after migration");
    assert_eq!(
        summary.total_requests, 2,
        "migration must not miss logs at the same second as legacy_ts"
    );
    assert_eq!(summary.success_count, 2);

    let expected_last_id = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT MAX(id) FROM auth_token_logs WHERE counts_business_quota = 1",
    )
    .fetch_one(&store.pool)
    .await
    .expect("max log id")
    .expect("max log id should exist");
    let cursor_v2_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
        .fetch_one(&store.pool)
        .await
        .expect("v2 cursor exists");
    let cursor_v2 = cursor_v2_raw
        .parse::<i64>()
        .expect("v2 cursor should be numeric");
    assert_eq!(cursor_v2, expected_last_id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn rollup_token_usage_stats_keeps_legacy_timestamp_cursor_monotonic() {
    let db_path = temp_db_path("rollup-legacy-ts-monotonic");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("rollup-legacy-ts-monotonic"))
        .await
        .expect("create token");
    let store = proxy.key_store.clone();
    let newer_ts = 1_700_010_000i64;
    let older_ts = newer_ts - 3_600;

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(newer_ts)
    .execute(&store.pool)
    .await
    .expect("insert newer log first");

    proxy
        .rollup_token_usage_stats()
        .await
        .expect("first rollup");

    let first_legacy_ts_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .fetch_one(&store.pool)
        .await
        .expect("legacy cursor exists after first rollup");
    let first_legacy_ts = first_legacy_ts_raw
        .parse::<i64>()
        .expect("legacy ts should be numeric");
    assert_eq!(first_legacy_ts, newer_ts);

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, counts_business_quota, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1, ?)
        "#,
    )
    .bind(&token.id)
    .bind(older_ts)
    .execute(&store.pool)
    .await
    .expect("insert older log with newer id");

    let second = proxy
        .rollup_token_usage_stats()
        .await
        .expect("second rollup");
    assert_eq!(
        second.1,
        Some(newer_ts),
        "reported last_rollup_ts should stay aligned with the clamped legacy cursor"
    );

    let second_legacy_ts_raw: String = sqlx::query_scalar("SELECT value FROM meta WHERE key = ?")
        .bind(META_KEY_TOKEN_USAGE_ROLLUP_TS)
        .fetch_one(&store.pool)
        .await
        .expect("legacy cursor exists after second rollup");
    let second_legacy_ts = second_legacy_ts_raw
        .parse::<i64>()
        .expect("legacy ts should be numeric");
    assert_eq!(
        second_legacy_ts, newer_ts,
        "legacy ts must not regress when processed logs have older timestamps"
    );

    let summary = proxy
        .token_summary_since(&token.id, 0, None)
        .await
        .expect("summary after second rollup");
    assert_eq!(summary.total_requests, 2);
    assert_eq!(summary.success_count, 2);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn heal_orphan_auth_tokens_from_logs_creates_soft_deleted_token() {
    let db_path = temp_db_path("heal-orphan");
    let db_str = db_path.to_string_lossy().to_string();

    // Initialize schema.
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    // Insert an auth_token_logs entry for a token id that does not exist in auth_tokens.
    let orphan_token_id = "ZZZZ";
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1234567890)
        "#,
    )
    .bind(orphan_token_id)
    .execute(&store.pool)
    .await
    .expect("insert orphan log");

    // Clear healer meta key so that we can invoke the healer path again for this test.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_HEAL_ORPHAN_TOKENS_V1)
        .execute(&store.pool)
        .await
        .expect("delete meta gate");

    // Run healer directly.
    store
        .heal_orphan_auth_tokens_from_logs()
        .await
        .expect("heal orphan tokens");

    // Verify that a soft-deleted auth_tokens row was created for the orphan id.
    let (enabled, total_requests, deleted_at): (i64, i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, total_requests, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(orphan_token_id)
            .fetch_one(&store.pool)
            .await
            .expect("restored token row");

    assert_eq!(enabled, 0, "restored token should be disabled");
    assert_eq!(
        total_requests, 1,
        "restored token should count orphan log entries"
    );
    assert!(
        deleted_at.is_some(),
        "restored token should be marked soft-deleted"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_is_single_use() {
    let db_path = temp_db_path("oauth-state-single-use");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state("linuxdo", Some("/"), 120)
        .await
        .expect("create oauth state");
    let first = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state first");
    let second = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state second");

    assert_eq!(first, Some(Some("/".to_string())));
    assert_eq!(second, None, "oauth state must be single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_binding_hash_must_match() {
    let db_path = temp_db_path("oauth-state-binding-hash");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding("linuxdo", Some("/"), 120, Some("nonce-hash-a"))
        .await
        .expect("create oauth state");

    let wrong_hash = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-b"))
        .await
        .expect("consume oauth state with wrong hash");
    assert_eq!(wrong_hash, None, "wrong hash must not consume oauth state");

    let matched = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state with matching hash");
    assert_eq!(matched, Some(Some("/".to_string())));

    let reused = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state reused");
    assert_eq!(reused, None, "oauth state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_payload_carries_bind_token_id() {
    let db_path = temp_db_path("oauth-state-bind-token-id");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding_and_token(
            "linuxdo",
            Some("/console"),
            120,
            Some("nonce-hash-a"),
            Some("a1b2"),
        )
        .await
        .expect("create oauth state");

    let payload = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state")
        .expect("payload exists");

    assert_eq!(payload.redirect_to.as_deref(), Some("/console"));
    assert_eq!(payload.bind_token_id.as_deref(), Some("a1b2"));

    let consumed_again = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state second");
    assert!(consumed_again.is_none(), "state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_reuses_existing_binding() {
    let db_path = temp_db_path("user-token-binding-reuse");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "alice-uid".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alice");

    let first = proxy
        .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
        .await
        .expect("bind alice first");
    let second = proxy
        .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
        .await
        .expect("bind alice second");

    assert_eq!(
        first.id, second.id,
        "same user should reuse one token binding"
    );
    assert_eq!(first.token, second.token);

    let bob = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "bob-uid".to_string(),
            username: Some("bob".to_string()),
            name: Some("Bob".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert bob");
    let bob_token = proxy
        .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob"))
        .await
        .expect("bind bob");

    assert_ne!(
        first.id, bob_token.id,
        "different users must not share the same token binding"
    );

    let store = proxy.key_store.clone();
    let (alice_bindings,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
            .bind(&alice.user_id)
            .fetch_one(&store.pool)
            .await
            .expect("count alice bindings");
    assert_eq!(
        alice_bindings, 1,
        "alice should have exactly one binding row"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_with_preferred_keeps_existing_binding_and_adds_preferred() {
    let db_path = temp_db_path("user-token-binding-preferred-rebind");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-rebind-user".to_string(),
            username: Some("preferred_rebind".to_string()),
            name: Some("Preferred Rebind".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let original = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:preferred_rebind"))
        .await
        .expect("ensure initial binding");
    let mistaken = proxy
        .create_access_token(Some("linuxdo:mistaken"))
        .await
        .expect("create mistaken token");

    let store = proxy.key_store.clone();
    sqlx::query("UPDATE user_token_bindings SET token_id = ?, updated_at = ? WHERE user_id = ?")
        .bind(&mistaken.id)
        .bind(Utc::now().timestamp() - 30)
        .bind(&user.user_id)
        .execute(&store.pool)
        .await
        .expect("simulate mistaken binding");

    let rebound = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:preferred_rebind"),
            Some(&original.id),
        )
        .await
        .expect("rebind preferred token");

    assert_eq!(
        rebound.id, original.id,
        "preferred token should be bound to the user"
    );

    let (binding_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
            .bind(&user.user_id)
            .fetch_one(&store.pool)
            .await
            .expect("count user bindings");
    assert_eq!(
        binding_count, 2,
        "preferred binding should be added without removing existing token"
    );

    let preferred_owner = sqlx::query_scalar::<_, Option<String>>(
        "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
    )
    .bind(&original.id)
    .fetch_optional(&store.pool)
    .await
    .expect("query preferred owner")
    .flatten();
    assert_eq!(
        preferred_owner.as_deref(),
        Some(user.user_id.as_str()),
        "preferred token should belong to the user"
    );

    let mistaken_owner = sqlx::query_scalar::<_, Option<String>>(
        "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
    )
    .bind(&mistaken.id)
    .fetch_optional(&store.pool)
    .await
    .expect("query mistaken token owner")
    .flatten();
    assert_eq!(
        mistaken_owner.as_deref(),
        Some(user.user_id.as_str()),
        "existing token must stay bound to the same user"
    );

    let primary = proxy
        .get_user_token(&user.user_id)
        .await
        .expect("query primary user token");
    match primary {
        UserTokenLookup::Found(secret) => assert_eq!(
            secret.id, original.id,
            "latest preferred binding should be selected as primary token"
        ),
        other => panic!("expected found user token, got {other:?}"),
    }

    let (enabled, deleted_at): (i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ? LIMIT 1")
            .bind(&mistaken.id)
            .fetch_one(&store.pool)
            .await
            .expect("query mistaken token state");
    assert_eq!(enabled, 1, "mistaken token should remain active");
    assert!(
        deleted_at.is_none(),
        "mistaken token should not be soft-deleted"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_with_preferred_falls_back_when_preferred_owned_by_other_user() {
    let db_path = temp_db_path("user-token-binding-preferred-conflict");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-conflict-alice".to_string(),
            username: Some("alice_conflict".to_string()),
            name: Some("Alice Conflict".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alice");
    let bob = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-conflict-bob".to_string(),
            username: Some("bob_conflict".to_string()),
            name: Some("Bob Conflict".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert bob");
    let bob_token = proxy
        .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob_conflict"))
        .await
        .expect("ensure bob token");

    let alice_result = proxy
        .ensure_user_token_binding_with_preferred(
            &alice.user_id,
            Some("linuxdo:alice_conflict"),
            Some(&bob_token.id),
        )
        .await
        .expect("fallback binding for alice");

    assert_ne!(
        alice_result.id, bob_token.id,
        "preferred token owned by other user must not be rebound"
    );

    let store = proxy.key_store.clone();
    let (owner,): (String,) =
        sqlx::query_as("SELECT user_id FROM user_token_bindings WHERE token_id = ?")
            .bind(&bob_token.id)
            .fetch_one(&store.pool)
            .await
            .expect("query bob token owner");
    assert_eq!(
        owner, bob.user_id,
        "conflicting token owner must remain unchanged"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_with_preferred_falls_back_when_preferred_unavailable() {
    let db_path = temp_db_path("user-token-binding-preferred-unavailable");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-unavailable-user".to_string(),
            username: Some("preferred_unavailable".to_string()),
            name: Some("Preferred Unavailable".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let original = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:preferred_unavailable"))
        .await
        .expect("ensure original binding");
    let disabled = proxy
        .create_access_token(Some("linuxdo:disabled_preferred"))
        .await
        .expect("create disabled preferred token");
    proxy
        .set_access_token_enabled(&disabled.id, false)
        .await
        .expect("disable preferred token");

    let fallback_disabled = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:preferred_unavailable"),
            Some(&disabled.id),
        )
        .await
        .expect("fallback when preferred disabled");
    assert_eq!(
        fallback_disabled.id, original.id,
        "disabled preferred token should be ignored"
    );

    let deleted = proxy
        .create_access_token(Some("linuxdo:deleted_preferred"))
        .await
        .expect("create deleted preferred token");
    proxy
        .delete_access_token(&deleted.id)
        .await
        .expect("soft delete preferred token");

    let fallback_deleted = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:preferred_unavailable"),
            Some(&deleted.id),
        )
        .await
        .expect("fallback when preferred deleted");
    assert_eq!(
        fallback_deleted.id, original.id,
        "soft-deleted preferred token should be ignored"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn force_user_relogin_migration_revokes_existing_sessions_once() {
    let db_path = temp_db_path("force-user-relogin-v1");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "force-relogin-user".to_string(),
            username: Some("force_relogin".to_string()),
            name: Some("Force Relogin".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let session = proxy
        .create_user_session(&user, 3600)
        .await
        .expect("create session");

    let store = proxy.key_store.clone();
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_FORCE_USER_RELOGIN_V1)
        .execute(&store.pool)
        .await
        .expect("delete relogin migration meta key");
    drop(proxy);

    let _proxy_after_restart =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy restarted");

    let revoked_at = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT revoked_at FROM user_sessions WHERE token = ? LIMIT 1",
    )
    .bind(&session.token)
    .fetch_optional(&store.pool)
    .await
    .expect("query session after restart")
    .flatten();
    assert!(
        revoked_at.is_some(),
        "existing sessions must be revoked by one-time relogin migration"
    );

    let relogin_migration_mark =
        sqlx::query_scalar::<_, Option<String>>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_FORCE_USER_RELOGIN_V1)
            .fetch_optional(&store.pool)
            .await
            .expect("query relogin migration mark")
            .flatten();
    assert!(
        relogin_migration_mark.is_some(),
        "relogin migration must record one-time completion mark"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_token_bindings_migration_supports_multi_binding_without_backfill() {
    let db_path = temp_db_path("user-token-bindings-multi-binding-migration");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-binding-user".to_string(),
            username: Some("legacy_binding_user".to_string()),
            name: Some("Legacy Binding User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert legacy user");
    let legacy = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_binding_user"))
        .await
        .expect("create legacy binding");
    drop(proxy);

    let options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open db pool");

    let legacy_row = sqlx::query_as::<_, (String, String, i64, i64)>(
        "SELECT user_id, token_id, created_at, updated_at FROM user_token_bindings WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_one(&pool)
    .await
    .expect("read legacy binding row");
    sqlx::query("DROP TABLE user_token_bindings")
        .execute(&pool)
        .await
        .expect("drop user_token_bindings");
    sqlx::query(
        r#"
        CREATE TABLE user_token_bindings (
            user_id TEXT PRIMARY KEY,
            token_id TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id),
            FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("recreate legacy user_token_bindings");
    sqlx::query(
        "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&legacy_row.0)
    .bind(&legacy_row.1)
    .bind(legacy_row.2)
    .bind(legacy_row.3)
    .execute(&pool)
    .await
    .expect("insert legacy binding row");
    drop(pool);

    let proxy_after_restart =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy restarted");
    let preferred = proxy_after_restart
        .create_access_token(Some("linuxdo:preferred_after_migration"))
        .await
        .expect("create preferred token");
    proxy_after_restart
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:legacy_binding_user"),
            Some(&preferred.id),
        )
        .await
        .expect("bind preferred token after migration");

    let store = proxy_after_restart.key_store.clone();
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
            .bind(&user.user_id)
            .fetch_one(&store.pool)
            .await
            .expect("count user bindings after migration");
    assert_eq!(
        count, 2,
        "migrated schema should allow multiple token bindings per user"
    );

    let owners = sqlx::query_as::<_, (String, String)>(
        "SELECT token_id, user_id FROM user_token_bindings WHERE user_id = ? ORDER BY token_id ASC",
    )
    .bind(&user.user_id)
    .fetch_all(&store.pool)
    .await
    .expect("query owners after migration");
    assert!(
        owners
            .iter()
            .any(|(token_id, owner)| token_id == &legacy.id && owner == &user.user_id),
        "legacy binding should be preserved"
    );
    assert!(
        owners
            .iter()
            .any(|(token_id, owner)| token_id == &preferred.id && owner == &user.user_id),
        "preferred binding should be added"
    );

    let _ = std::fs::remove_file(db_path);
}
#[tokio::test]
async fn get_user_token_returns_unavailable_after_soft_delete() {
    let db_path = temp_db_path("user-token-unavailable");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "charlie-uid".to_string(),
            username: Some("charlie".to_string()),
            name: Some("Charlie".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert charlie");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:charlie"))
        .await
        .expect("bind charlie");

    let before = proxy
        .get_user_token(&user.user_id)
        .await
        .expect("lookup user token before delete");
    assert!(
        matches!(before, UserTokenLookup::Found(_)),
        "token should be available before delete"
    );

    proxy
        .delete_access_token(&token.id)
        .await
        .expect("soft delete token");

    let after = proxy
        .get_user_token(&user.user_id)
        .await
        .expect("lookup user token after delete");
    assert!(
        matches!(after, UserTokenLookup::Unavailable),
        "soft-deleted binding should report unavailable"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn get_user_token_secret_returns_none_when_token_disabled() {
    let db_path = temp_db_path("user-token-secret-disabled");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "disabled-secret-user".to_string(),
            username: Some("disabled_secret_user".to_string()),
            name: Some("Disabled Secret User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:disabled_secret_user"))
        .await
        .expect("bind token");

    let before = proxy
        .get_user_token_secret(&user.user_id, &token.id)
        .await
        .expect("secret before disable");
    assert!(before.is_some(), "enabled token should expose secret");

    proxy
        .set_access_token_enabled(&token.id, false)
        .await
        .expect("disable token");

    let after = proxy
        .get_user_token_secret(&user.user_id, &token.id)
        .await
        .expect("secret after disable");
    assert!(after.is_none(), "disabled token should not expose secret");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_for_previous_subject_stays_pending_after_token_binding_changes_subject() {
    let db_path = temp_db_path("pending-billing-subject-flip");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-subject-flip"))
        .await
        .expect("create token");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("simulated pending charge"),
            3,
            None,
        )
        .await
        .expect("record pending billing attempt");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-subject-user".to_string(),
            username: Some("pending_billing_subject".to_string()),
            name: Some("Pending Billing Subject".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:pending_billing_subject"),
            Some(&token.id),
        )
        .await
        .expect("bind existing token to user");

    let _guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("reconcile pending billing after subject flip");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(token_minute_sum, 3);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_for_previous_account_subject_stays_pending_after_token_becomes_unbound() {
    let db_path = temp_db_path("pending-billing-account-to-token-subject-flip");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-account-user".to_string(),
            username: Some("pending_billing_account".to_string()),
            name: Some("Pending Billing Account".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:pending_billing_account"))
        .await
        .expect("bind token");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("simulated pending charge"),
            4,
            None,
        )
        .await
        .expect("record pending billing attempt");

    sqlx::query("DELETE FROM user_token_bindings WHERE token_id = ?")
        .bind(&token.id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("unbind token");
    proxy.key_store.cache_token_binding(&token.id, None).await;

    let _guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("reconcile pending billing after unbind");

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(account_minute_sum, 4);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn locked_billing_subject_keeps_original_precheck_after_binding_change() {
    let db_path = temp_db_path("locked-billing-subject-precheck");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("locked-billing-subject-precheck"))
        .await
        .expect("create token");
    proxy
        .charge_token_quota(&token.id, 1)
        .await
        .expect("seed token quota before binding change");

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing");
    assert_eq!(guard.billing_subject(), format!("token:{}", token.id));

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "locked-billing-subject-precheck-user".to_string(),
            username: Some("locked_billing_precheck".to_string()),
            name: Some("Locked Billing Precheck".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:locked_billing_precheck"),
            Some(&token.id),
        )
        .await
        .expect("bind existing token to user");

    let locked_verdict = proxy
        .peek_token_quota_for_subject(guard.billing_subject())
        .await
        .expect("peek locked subject quota");
    assert_eq!(locked_verdict.hourly_used, 1);

    let current_verdict = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek current token quota");
    assert_eq!(current_verdict.hourly_used, 0);

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_attempt_for_subject_charges_original_subject_after_binding_change() {
    let db_path = temp_db_path("pending-billing-for-subject");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-for-subject"))
        .await
        .expect("create token");

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-for-subject-user".to_string(),
            username: Some("pending_billing_subject_charge".to_string()),
            name: Some("Pending Billing Subject Charge".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:pending_billing_subject_charge"),
            Some(&token.id),
        )
        .await
        .expect("bind existing token to user");

    let log_id = proxy
        .record_pending_billing_attempt_for_subject(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("subject pinned to original token"),
            2,
            guard.billing_subject(),
            None,
        )
        .await
        .expect("record pending billing attempt with pinned subject");
    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle pending billing attempt");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(token_minute_sum, 2);

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(account_minute_sum, 0);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn lock_token_billing_uses_fresh_binding_after_external_rebind() {
    let db_path = temp_db_path("lock-token-billing-fresh-binding");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");
    let token = proxy_a
        .create_access_token(Some("fresh-binding-rebind"))
        .await
        .expect("create token");

    // Warm proxy_a's cache with the old unbound subject first.
    let initial = proxy_a
        .peek_token_quota(&token.id)
        .await
        .expect("peek unbound quota");
    assert_eq!(initial.hourly_used, 0);

    let user = proxy_b
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "fresh-binding-user".to_string(),
            username: Some("fresh_binding_user".to_string()),
            name: Some("Fresh Binding User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy_b
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:fresh_binding_user"),
            Some(&token.id),
        )
        .await
        .expect("bind token on proxy b");

    let guard = proxy_a
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing after external rebind");
    assert_eq!(guard.billing_subject(), format!("account:{}", user.user_id));

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_replay_does_not_backfill_previous_month_into_current_token_quota() {
    let db_path = temp_db_path("pending-billing-token-old-month");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-token-old-month"))
        .await
        .expect("create token");

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let previous_month_ts = current_month_start - 60;

    sqlx::query(
        "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(7_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed current token month");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("previous month token charge"),
            3,
            None,
        )
        .await
        .expect("record pending token billing");
    sqlx::query("UPDATE auth_token_logs SET created_at = ? WHERE id = ?")
        .bind(previous_month_ts)
        .bind(log_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("rewrite token log timestamp");

    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle previous month token billing");

    let token_month: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token monthly quota");
    assert_eq!(token_month, (current_month_start, 7));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_replay_does_not_backfill_previous_month_into_current_account_quota() {
    let db_path = temp_db_path("pending-billing-account-old-month");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-account-old-month-user".to_string(),
            username: Some("pending_billing_account_old_month".to_string()),
            name: Some("Pending Billing Account Old Month".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(
            &user.user_id,
            Some("linuxdo:pending_billing_account_old_month"),
        )
        .await
        .expect("bind token");

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let previous_month_ts = current_month_start - 60;

    sqlx::query(
        "INSERT INTO account_monthly_quota (user_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(current_month_start)
    .bind(11_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed current account month");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("previous month account charge"),
            4,
            None,
        )
        .await
        .expect("record pending account billing");
    sqlx::query("UPDATE auth_token_logs SET created_at = ? WHERE id = ?")
        .bind(previous_month_ts)
        .bind(log_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("rewrite account log timestamp");

    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle previous month account billing");

    let account_month: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account monthly quota");
    assert_eq!(account_month, (current_month_start, 11));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn settle_pending_billing_attempt_is_idempotent_across_instances() {
    let db_path = temp_db_path("pending-billing-idempotent-settle");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");
    let token = proxy_a
        .create_access_token(Some("pending-billing-idempotent-settle"))
        .await
        .expect("create token");

    let log_id = proxy_a
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("concurrent settle"),
            5,
            None,
        )
        .await
        .expect("record pending billing attempt");

    let settle_a = tokio::spawn(async move {
        proxy_a
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle on proxy a");
    });
    let proxy_b_settle = proxy_b.clone();
    let settle_b = tokio::spawn(async move {
        proxy_b_settle
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle on proxy b");
    });

    tokio::try_join!(settle_a, settle_b).expect("join settle tasks");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_b.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(token_minute_sum, 5);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy_b.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_claim_miss_is_retry_later_until_next_replay() {
    let db_path = temp_db_path("pending-billing-claim-miss-retry");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-claim-miss-retry"))
        .await
        .expect("create token");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("forced claim miss"),
            3,
            None,
        )
        .await
        .expect("record pending billing attempt");

    proxy.force_pending_billing_claim_miss_once(log_id).await;

    let outcome = proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("forced claim miss should surface retry-later outcome");
    assert_eq!(outcome, PendingBillingSettleOutcome::RetryLater);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read pending billing state");
    assert_eq!(billing_state, BILLING_STATE_PENDING);

    let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
    assert_eq!(verdict.hourly_used, 0);

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("next billing lock should replay the pending charge before precheck");
    drop(guard);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read charged billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let verdict = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek quota after replay");
    assert_eq!(verdict.hourly_used, 3);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn token_billing_lock_serializes_across_proxy_instances() {
    let db_path = temp_db_path("billing-lock-cross-instance");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");
    let token = proxy_a
        .create_access_token(Some("billing-lock-cross-instance"))
        .await
        .expect("create token");

    let guard = proxy_a
        .lock_token_billing(&token.id)
        .await
        .expect("acquire first billing lock");

    let token_id = token.id.clone();
    let waiter = tokio::spawn(async move {
        let _guard = proxy_b
            .lock_token_billing(&token_id)
            .await
            .expect("acquire second billing lock");
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !waiter.is_finished(),
        "second proxy instance should wait for the shared billing lock"
    );

    drop(guard);
    tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("second proxy acquires after release")
        .expect("waiter joins");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_usage_lock_serializes_across_proxy_instances() {
    let db_path = temp_db_path("research-usage-cross-instance-lock");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");

    let guard = proxy_a
        .lock_research_key_usage("shared-upstream-key")
        .await
        .expect("acquire first research lock");

    let waiter = tokio::spawn(async move {
        let _guard = proxy_b
            .lock_research_key_usage("shared-upstream-key")
            .await
            .expect("acquire second research lock");
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !waiter.is_finished(),
        "second proxy instance should wait for the shared research lock"
    );

    drop(guard);
    tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("second proxy acquires after release")
        .expect("waiter joins");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn bound_token_quota_checks_use_account_counters() {
    let db_path = temp_db_path("bound-token-account-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "quota-user".to_string(),
            username: Some("quota_user".to_string()),
            name: Some("Quota User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:quota_user"))
        .await
        .expect("bind token");

    proxy
        .charge_token_quota(&token.id, 2)
        .await
        .expect("charge business quota credits");

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(
        account_minute_sum, 2,
        "account buckets should count charged credits"
    );

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(
        token_minute_sum, 0,
        "bound token should no longer mutate token-level buckets"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn business_quota_credits_cutover_preserves_existing_counters_once() {
    let db_path = temp_db_path("business-quota-credits-cutover");
    let db_str = db_path.to_string_lossy().to_string();

    // First start: create schema + seed token/user rows for FK constraints.
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let unbound_token = proxy
        .create_access_token(Some("cutover-unbound-token"))
        .await
        .expect("create token");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "cutover-user".to_string(),
            username: Some("cutover".to_string()),
            name: Some("Cutover User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let bound_token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:cutover"))
        .await
        .expect("bind token");

    // Simulate an older DB (pre-cutover) by clearing the cutover meta key and writing
    // legacy request-count counters into the buckets/quota tables. The migration should
    // preserve them so deploys do not silently reset active customer quotas.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset cutover meta");

    let now = Utc::now();
    let now_ts = now.timestamp();
    let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
    let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
    let month_start = start_of_month(now).timestamp();

    // Token-scoped legacy counters.
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(minute_bucket)
    .bind(GRANULARITY_MINUTE)
    .bind(9_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token minute bucket");
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(hour_bucket)
    .bind(GRANULARITY_HOUR)
    .bind(11_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token hour bucket");
    // Ensure the request limiter bucket is not affected by the cutover reset.
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(minute_bucket)
    .bind(GRANULARITY_REQUEST_MINUTE)
    .bind(5_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token request_minute bucket");
    sqlx::query(
        "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(month_start)
    .bind(13_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token monthly quota");

    // Account-scoped legacy counters (e.g. from old backfill).
    sqlx::query(
        "INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(minute_bucket)
    .bind(GRANULARITY_MINUTE)
    .bind(7_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account minute bucket");
    sqlx::query(
        "INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(hour_bucket)
    .bind(GRANULARITY_HOUR)
    .bind(8_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account hour bucket");
    sqlx::query(
        "INSERT INTO account_monthly_quota (user_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(month_start)
    .bind(14_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account monthly quota");

    drop(proxy);

    // Second start: cutover migration should preserve legacy counters exactly once.
    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy restarted");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(
        token_minute_sum, 9,
        "cutover should preserve token minute buckets"
    );

    let token_hour_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read token hour buckets");
    assert_eq!(
        token_hour_sum, 11,
        "cutover should preserve token hour buckets"
    );

    let token_request_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_REQUEST_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read token request_minute buckets");
    assert_eq!(
        token_request_minute_sum, 5,
        "cutover must not clear raw request limiter buckets"
    );

    let token_monthly_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM auth_token_quota WHERE token_id = ?",
    )
    .bind(&unbound_token.id)
    .fetch_optional(&proxy_after.key_store.pool)
    .await
    .expect("read token monthly quota")
    .unwrap_or(0);
    assert_eq!(
        token_monthly_count, 13,
        "cutover should preserve token monthly quota"
    );

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(
        account_minute_sum, 7,
        "cutover should preserve account minute buckets"
    );

    let account_hour_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account hour buckets");
    assert_eq!(
        account_hour_sum, 8,
        "cutover should preserve account hour buckets"
    );

    let account_monthly_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_optional(&proxy_after.key_store.pool)
    .await
    .expect("read account monthly quota")
    .unwrap_or(0);
    assert_eq!(
        account_monthly_count, 14,
        "cutover should preserve account monthly quota"
    );

    // Third start: cutover meta key exists, so preserved counters should remain untouched.
    sqlx::query(
        "UPDATE token_usage_buckets SET count = ? WHERE token_id = ? AND bucket_start = ? AND granularity = ?",
    )
    .bind(12_i64)
    .bind(&unbound_token.id)
    .bind(minute_bucket)
    .bind(GRANULARITY_MINUTE)
    .execute(&proxy_after.key_store.pool)
    .await
    .expect("update post-cutover token bucket");
    drop(proxy_after);

    let proxy_third = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy restarted again");

    let token_minute_sum_after: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_third.key_store.pool)
    .await
    .expect("read token minute buckets after third start");
    assert_eq!(
        token_minute_sum_after, 12,
        "cutover migration must not rerun after meta is set"
    );

    // Silence unused warning for the bound token variable; it exists only for FK seeding.
    assert!(!bound_token.id.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_quota_backfill_is_idempotent() {
    let db_path = temp_db_path("account-backfill-idempotent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "backfill-user".to_string(),
            username: Some("backfill".to_string()),
            name: Some("Backfill User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:backfill"))
        .await
        .expect("bind token");

    let month_start = start_of_month(Utc::now()).timestamp();
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&token.id)
    .bind(month_start)
    .bind(GRANULARITY_MINUTE)
    .bind(3_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token minute bucket");
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&token.id)
    .bind(month_start)
    .bind(GRANULARITY_HOUR)
    .bind(5_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token hour bucket");
    sqlx::query(
        "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)\n             ON CONFLICT(token_id) DO UPDATE SET month_start = excluded.month_start, month_count = excluded.month_count",
    )
    .bind(&token.id)
    .bind(month_start)
    .bind(7_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token monthly quota");

    sqlx::query("DELETE FROM account_usage_buckets")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear account buckets");
    sqlx::query("DELETE FROM account_monthly_quota")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear account monthly");
    sqlx::query("DELETE FROM account_quota_limits")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear account limits");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset backfill meta");

    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened for first backfill");

    let first_account_minute: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account minute after first backfill");
    let first_account_hour: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account hour after first backfill");
    let first_month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account month after first backfill");

    assert_eq!(first_account_minute, 3);
    assert_eq!(first_account_hour, 5);
    assert_eq!(first_month_count, 7);

    drop(proxy_after);

    let proxy_again = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened for idempotent check");
    let second_account_minute: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_again.key_store.pool)
    .await
    .expect("read account minute after second init");
    let second_account_hour: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_again.key_store.pool)
    .await
    .expect("read account hour after second init");
    let second_month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_again.key_store.pool)
    .await
    .expect("read account month after second init");

    assert_eq!(second_account_minute, first_account_minute);
    assert_eq!(second_account_hour, first_account_hour);
    assert_eq!(second_month_count, first_month_count);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_quota_limits_sync_with_env_defaults_on_restart() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-sync");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "limit-sync-user".to_string(),
            username: Some("limit_sync_user".to_string()),
            name: Some("Limit Sync User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:limit_sync_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id)
        .await
        .expect("seed account quota row");

    let seeded_limits: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read seeded limits");
    assert_eq!(seeded_limits, (0, 0, 0, 0));

    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 11,
               hourly_limit = 12,
               daily_limit = 13,
               monthly_limit = 14,
               inherits_defaults = 1
           WHERE user_id = ?"#,
    )
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy default-following row");

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let second_limits: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read second limits");
    assert_eq!(second_limits, (21, 22, 23, 24));

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn legacy_current_default_account_quota_limits_keep_following_defaults_after_reclassification()
 {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-legacy-current-default");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-current-default-user".to_string(),
            username: Some("legacy_current_default_user".to_string()),
            name: Some("Legacy Current Default User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_current_default_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id)
        .await
        .expect("seed account quota row");
    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 11,
               hourly_limit = 12,
               daily_limit = 13,
               monthly_limit = 14,
               inherits_defaults = 1
           WHERE user_id = ?"#,
    )
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy current default row");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear inherits defaults backfill marker");

    drop(proxy);

    let proxy_after_backfill =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened for backfill");
    let first_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after_backfill.key_store.pool)
    .await
    .expect("read reclassified default limits");
    assert_eq!(first_limits, (11, 12, 13, 14, 1));

    drop(proxy_after_backfill);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after_sync =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened for sync");
    let second_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after_sync.key_store.pool)
    .await
    .expect("read synced default limits");
    assert_eq!(second_limits, (21, 22, 23, 24, 1));

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn shared_legacy_noncurrent_tuple_is_left_custom_during_reclassification() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-legacy-shared-noncurrent");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let alpha = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-shared-alpha".to_string(),
            username: Some("legacy_shared_alpha".to_string()),
            name: Some("Legacy Shared Alpha".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alpha");
    let beta = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-shared-beta".to_string(),
            username: Some("legacy_shared_beta".to_string()),
            name: Some("Legacy Shared Beta".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert beta");
    let custom_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-shared-custom".to_string(),
            username: Some("legacy_shared_custom".to_string()),
            name: Some("Legacy Shared Custom".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(3),
            raw_payload_json: None,
        })
        .await
        .expect("upsert custom user");
    for user in [&alpha, &beta, &custom_user] {
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_shared"))
            .await
            .expect("bind token");
        proxy
            .user_dashboard_summary(&user.user_id)
            .await
            .expect("seed account quota row");
    }
    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 11,
               hourly_limit = 12,
               daily_limit = 13,
               monthly_limit = 14,
               inherits_defaults = 1,
               updated_at = created_at + 5
           WHERE user_id IN (?, ?)"#,
    )
    .bind(&alpha.user_id)
    .bind(&beta.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate shared non-current tuple rows");
    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 101,
               hourly_limit = 102,
               daily_limit = 103,
               monthly_limit = 104,
               inherits_defaults = 1,
               updated_at = created_at
           WHERE user_id = ?"#,
    )
    .bind(&custom_user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate legacy custom row");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear inherits defaults backfill marker");

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    for user_id in [&alpha.user_id, &beta.user_id] {
        let limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read shared tuple limits");
        assert_eq!(limits, (11, 12, 13, 14, 0));
    }
    let custom_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&custom_user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read shared custom limits");
    assert_eq!(custom_limits, (101, 102, 103, 104, 0));

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[test]
fn build_account_quota_resolution_clamps_negative_tag_totals_to_zero() {
    let base = AccountQuotaLimits {
        hourly_any_limit: 10,
        hourly_limit: 20,
        daily_limit: 30,
        monthly_limit: 40,
        inherits_defaults: false,
    };
    let resolution = build_account_quota_resolution(
        base.clone(),
        vec![UserTagBindingRecord {
            source: USER_TAG_SOURCE_MANUAL.to_string(),
            tag: UserTagRecord {
                id: "custom-tag".to_string(),
                name: "custom_tag".to_string(),
                display_name: "Custom Tag".to_string(),
                icon: Some("sparkles".to_string()),
                system_key: None,
                effect_kind: USER_TAG_EFFECT_QUOTA_DELTA.to_string(),
                hourly_any_delta: -100,
                hourly_delta: -200,
                daily_delta: -300,
                monthly_delta: -400,
                user_count: 1,
            },
        }],
    );

    assert_eq!(resolution.base.hourly_any_limit, 10);
    assert_eq!(resolution.effective.hourly_any_limit, 0);
    assert_eq!(resolution.effective.hourly_limit, 0);
    assert_eq!(resolution.effective.daily_limit, 0);
    assert_eq!(resolution.effective.monthly_limit, 0);
    assert_eq!(resolution.breakdown.len(), 3);
    let effective_row = resolution
        .breakdown
        .iter()
        .find(|entry| entry.kind == "effective")
        .expect("effective row present");
    assert_eq!(effective_row.effect_kind, "effective");
    assert_eq!(effective_row.hourly_any_delta, 0);
    assert_eq!(effective_row.hourly_delta, 0);
    assert_eq!(effective_row.daily_delta, 0);
    assert_eq!(effective_row.monthly_delta, 0);
}

#[tokio::test]
async fn new_account_without_tags_defaults_to_zero_base_and_effective_quota() {
    let db_path = temp_db_path("new-account-zero-base");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "new-zero-base-user".to_string(),
            username: Some("new_zero_base_user".to_string()),
            name: Some("New Zero Base User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let resolution = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolve account quota");

    assert_eq!(resolution.base.hourly_any_limit, 0);
    assert_eq!(resolution.base.hourly_limit, 0);
    assert_eq!(resolution.base.daily_limit, 0);
    assert_eq!(resolution.base.monthly_limit, 0);
    assert!(!resolution.base.inherits_defaults);
    assert_eq!(resolution.effective.hourly_any_limit, 0);
    assert_eq!(resolution.effective.hourly_limit, 0);
    assert_eq!(resolution.effective.daily_limit, 0);
    assert_eq!(resolution.effective.monthly_limit, 0);

    let persisted: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read persisted zero base");
    assert_eq!(persisted, (0, 0, 0, 0, 0));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn new_linuxdo_account_effective_quota_comes_only_from_tags() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("new-linuxdo-tag-only-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "new-linuxdo-tag-only-user".to_string(),
            username: Some("new_linuxdo_tag_only_user".to_string()),
            name: Some("New LinuxDo Tag Only User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let resolution = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolve account quota");
    let tag_only_limits = AccountQuotaLimits::legacy_defaults();

    assert_eq!(resolution.base.hourly_any_limit, 0);
    assert_eq!(resolution.base.hourly_limit, 0);
    assert_eq!(resolution.base.daily_limit, 0);
    assert_eq!(resolution.base.monthly_limit, 0);
    assert_eq!(
        resolution.effective.hourly_any_limit,
        tag_only_limits.hourly_any_limit
    );
    assert_eq!(
        resolution.effective.hourly_limit,
        tag_only_limits.hourly_limit
    );
    assert_eq!(
        resolution.effective.daily_limit,
        tag_only_limits.daily_limit
    );
    assert_eq!(
        resolution.effective.monthly_limit,
        tag_only_limits.monthly_limit
    );
    assert!(
        resolution
            .tags
            .iter()
            .any(|binding| { binding.tag.system_key.as_deref() == Some("linuxdo_l2") })
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn historical_account_without_quota_row_keeps_legacy_defaults_on_first_resolution() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("historical-account-missing-quota-row");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "historical-missing-quota-row".to_string(),
            username: Some("historical_missing_quota_row".to_string()),
            name: Some("Historical Missing Quota Row".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    sqlx::query("DELETE FROM account_quota_limits WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete quota row");
    sqlx::query("UPDATE users SET created_at = ?, updated_at = ? WHERE id = ?")
        .bind(100_i64)
        .bind(100_i64)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("mark user historical");
    proxy
        .key_store
        .set_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1, 200)
        .await
        .expect("set zero-base cutover after user creation");

    let resolution = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolve historical account quota");
    let expected = AccountQuotaLimits::legacy_defaults();

    assert_eq!(resolution.base.hourly_any_limit, expected.hourly_any_limit);
    assert_eq!(resolution.base.hourly_limit, expected.hourly_limit);
    assert_eq!(resolution.base.daily_limit, expected.daily_limit);
    assert_eq!(resolution.base.monthly_limit, expected.monthly_limit);
    assert!(resolution.base.inherits_defaults);

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn manual_account_quota_matching_legacy_defaults_stays_custom_on_restart() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("manual-account-quota-matching-legacy-defaults");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "manual-legacy-default-tuple".to_string(),
            username: Some("manual_legacy_default_tuple".to_string()),
            name: Some("Manual Legacy Default Tuple".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let updated = proxy
        .key_store
        .update_account_quota_limits(&user.user_id, 11, 12, 13, 14)
        .await
        .expect("update account quota");
    assert!(updated);

    let first_row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read first row");
    assert_eq!(first_row, (11, 12, 13, 14, 0));

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let second_row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read second row");
    assert_eq!(second_row, (11, 12, 13, 14, 0));

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn legacy_default_following_account_keeps_inherits_defaults_on_noop_save() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("legacy-default-following-noop-save");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "legacy-default-following-noop-save".to_string(),
            username: Some("legacy_default_following_noop_save".to_string()),
            name: Some("Legacy Default Following Noop Save".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 11,
               hourly_limit = 12,
               daily_limit = 13,
               monthly_limit = 14,
               inherits_defaults = 1
           WHERE user_id = ?"#,
    )
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy default-following row");

    let updated = proxy
        .key_store
        .update_account_quota_limits(&user.user_id, 11, 12, 13, 14)
        .await
        .expect("update account quota");
    assert!(updated);

    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read row after noop save");
    assert_eq!(row, (11, 12, 13, 14, 1));

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_quota_resolution_cache_invalidates_on_binding_and_tag_updates() {
    let db_path = temp_db_path("account-quota-resolution-cache");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "quota-cache-user".to_string(),
            username: Some("quota_cache_user".to_string()),
            name: Some("Quota Cache User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let defaults = AccountQuotaLimits::zero_base();

    let initial = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("initial resolution");
    assert_eq!(
        initial.effective.hourly_any_limit,
        defaults.hourly_any_limit
    );
    assert_eq!(initial.effective.hourly_limit, defaults.hourly_limit);

    let tag = proxy
        .create_user_tag(
            "quota_cache_boost",
            "Quota Cache Boost",
            Some("sparkles"),
            USER_TAG_EFFECT_QUOTA_DELTA,
            7,
            8,
            9,
            10,
        )
        .await
        .expect("create custom tag");
    proxy
        .bind_user_tag_to_user(&user.user_id, &tag.id)
        .await
        .expect("bind user tag");

    let after_bind = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolution after bind");
    assert_eq!(
        after_bind.effective.hourly_any_limit,
        defaults.hourly_any_limit + 7
    );
    assert_eq!(after_bind.effective.hourly_limit, defaults.hourly_limit + 8);

    proxy
        .update_user_tag(
            &tag.id,
            "quota_cache_boost",
            "Quota Cache Boost",
            Some("sparkles"),
            USER_TAG_EFFECT_QUOTA_DELTA,
            11,
            12,
            13,
            14,
        )
        .await
        .expect("update user tag")
        .expect("updated user tag");

    let after_update = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolution after update");
    assert_eq!(
        after_update.effective.hourly_any_limit,
        defaults.hourly_any_limit + 11
    );
    assert_eq!(
        after_update.effective.hourly_limit,
        defaults.hourly_limit + 12
    );
    assert_eq!(
        after_update.effective.daily_limit,
        defaults.daily_limit + 13
    );
    assert_eq!(
        after_update.effective.monthly_limit,
        defaults.monthly_limit + 14
    );

    proxy
        .unbind_user_tag_from_user(&user.user_id, &tag.id)
        .await
        .expect("unbind user tag");
    let after_unbind = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolution after unbind");
    assert_eq!(
        after_unbind.effective.hourly_any_limit,
        defaults.hourly_any_limit
    );
    assert_eq!(after_unbind.effective.hourly_limit, defaults.hourly_limit);
    assert_eq!(after_unbind.effective.daily_limit, defaults.daily_limit);
    assert_eq!(after_unbind.effective.monthly_limit, defaults.monthly_limit);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_system_tag_defaults_backfill_repairs_legacy_zero_seed() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tag-defaults");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    sqlx::query(
        r#"UPDATE user_tags
           SET hourly_any_delta = 0,
               hourly_delta = 0,
               daily_delta = 0,
               monthly_delta = 0
           WHERE system_key LIKE 'linuxdo_l%'"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("zero system tag deltas");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear linuxdo defaults migration marker");
    drop(proxy);

    let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy recreated");
    let defaults = linuxdo_system_tag_default_deltas();
    let seeded_rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT hourly_any_delta, hourly_delta, daily_delta, monthly_delta FROM user_tags WHERE system_key LIKE 'linuxdo_l%' ORDER BY system_key",
    )
    .fetch_all(&repaired.key_store.pool)
    .await
    .expect("read repaired seeded tag rows");
    assert_eq!(seeded_rows.len(), 5);
    assert!(
        seeded_rows
            .iter()
            .all(|row| *row == (defaults.0, defaults.1, defaults.2, defaults.3))
    );
}

#[tokio::test]
async fn linuxdo_system_tag_defaults_backfill_repairs_partial_legacy_zero_seed() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tag-defaults-partial");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    sqlx::query(
        r#"UPDATE user_tags
           SET hourly_any_delta = 0,
               hourly_delta = 0,
               daily_delta = 0,
               monthly_delta = 0
           WHERE system_key IN ('linuxdo_l1', 'linuxdo_l3')"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("zero partial system tag deltas");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear linuxdo defaults migration marker");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear linuxdo defaults tuple marker");
    drop(proxy);

    let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy recreated");
    let defaults = linuxdo_system_tag_default_deltas();
    let seeded_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&repaired.key_store.pool)
    .await
    .expect("read repaired seeded tag rows");
    assert_eq!(seeded_rows.len(), 5);
    assert!(
        seeded_rows
            .iter()
            .all(|(_, hourly_any, hourly, daily, monthly)| {
                (*hourly_any, *hourly, *daily, *monthly)
                    == (defaults.0, defaults.1, defaults.2, defaults.3)
            })
    );
}

#[tokio::test]
async fn linuxdo_system_tag_defaults_follow_env_changes_without_overwriting_customized_system_tags()
{
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tag-default-sync");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let initial_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read initial linuxdo system tag rows");
    assert_eq!(initial_rows.len(), 5);
    assert!(
        initial_rows
            .iter()
            .all(|(_, hourly_any, hourly, daily, monthly)| {
                (*hourly_any, *hourly, *daily, *monthly) == (11, 12, 13, 14)
            })
    );
    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after_default_change =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened after default change");
    let synced_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&proxy_after_default_change.key_store.pool)
    .await
    .expect("read synced linuxdo system tag rows");
    assert!(
        synced_rows
            .iter()
            .all(|(_, hourly_any, hourly, daily, monthly)| {
                (*hourly_any, *hourly, *daily, *monthly) == (21, 22, 23, 24)
            })
    );

    proxy_after_default_change
        .update_user_tag(
            "linuxdo_l2",
            "linuxdo_l2",
            "L2",
            Some("linuxdo"),
            USER_TAG_EFFECT_QUOTA_DELTA,
            101,
            102,
            103,
            104,
        )
        .await
        .expect("update system tag")
        .expect("system tag present");
    drop(proxy_after_default_change);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "31");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "32");
        std::env::set_var("TOKEN_DAILY_LIMIT", "33");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "34");
    }

    let proxy_after_customization =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened after system tag customization");
    let final_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&proxy_after_customization.key_store.pool)
    .await
    .expect("read final linuxdo system tag rows");
    assert_eq!(final_rows.len(), 5);
    for (system_key, hourly_any, hourly, daily, monthly) in final_rows {
        if system_key == "linuxdo_l2" {
            assert_eq!((hourly_any, hourly, daily, monthly), (101, 102, 103, 104));
        } else {
            assert_eq!((hourly_any, hourly, daily, monthly), (31, 32, 33, 34));
        }
    }

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_system_tags_seed_backfill_and_trust_level_sync() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tags");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let seeded_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_tags WHERE system_key LIKE 'linuxdo_l%'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count seeded tags");
    assert_eq!(seeded_count, 5);

    let defaults = linuxdo_system_tag_default_deltas();
    let seeded_rows = sqlx::query_as::<_, (String, String, Option<String>, i64, i64, i64, i64)>(
        "SELECT display_name, name, icon, hourly_any_delta, hourly_delta, daily_delta, monthly_delta FROM user_tags WHERE system_key LIKE 'linuxdo_l%' ORDER BY system_key",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read seeded tag rows");
    assert_eq!(seeded_rows.len(), 5);
    assert_eq!(
        seeded_rows[0],
        (
            "L0".to_string(),
            "linuxdo_l0".to_string(),
            Some("linuxdo".to_string()),
            defaults.0,
            defaults.1,
            defaults.2,
            defaults.3,
        )
    );
    assert_eq!(
        seeded_rows[4],
        (
            "L4".to_string(),
            "linuxdo_l4".to_string(),
            Some("linuxdo".to_string()),
            defaults.0,
            defaults.1,
            defaults.2,
            defaults.3,
        )
    );

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-system-user".to_string(),
            username: Some("linuxdo_system_user".to_string()),
            name: Some("LinuxDo System User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(3),
            raw_payload_json: None,
        })
        .await
        .expect("upsert linuxdo user");

    let first_key: String = sqlx::query_scalar(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read first linuxdo binding");
    assert_eq!(first_key, "linuxdo_l3");

    sqlx::query("DELETE FROM user_tag_bindings WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete bindings to simulate historical gap");
    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let restored_key: String = sqlx::query_scalar(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read restored linuxdo binding");
    assert_eq!(restored_key, "linuxdo_l3");

    proxy_after
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-system-user".to_string(),
            username: Some("linuxdo_system_user".to_string()),
            name: Some("LinuxDo System User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("update linuxdo trust level");
    let sync_keys = sqlx::query_scalar::<_, String>(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           ORDER BY t.system_key"#,
    )
    .bind(&user.user_id)
    .fetch_all(&proxy_after.key_store.pool)
    .await
    .expect("read synced linuxdo bindings");
    assert_eq!(sync_keys, vec!["linuxdo_l1".to_string()]);

    proxy_after
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-system-user".to_string(),
            username: Some("linuxdo_system_user".to_string()),
            name: Some("LinuxDo System User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("update linuxdo trust level to none");
    let retained_keys = sqlx::query_scalar::<_, String>(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           ORDER BY t.system_key"#,
    )
    .bind(&user.user_id)
    .fetch_all(&proxy_after.key_store.pool)
    .await
    .expect("read retained linuxdo bindings");
    assert_eq!(retained_keys, vec!["linuxdo_l1".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_oauth_upsert_skips_missing_tags_for_new_accounts_and_recovers_after_reseed() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-sync-best-effort");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    sqlx::query("DELETE FROM user_tags WHERE system_key LIKE 'linuxdo_l%'")
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete linuxdo system tags");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-best-effort-user".to_string(),
            username: Some("linuxdo_best_effort_user".to_string()),
            name: Some("LinuxDo Best Effort User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("new linuxdo account should still succeed without system tags");

    let oauth_row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM oauth_accounts WHERE provider = 'linuxdo' AND provider_user_id = ?",
    )
    .bind("linuxdo-best-effort-user")
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count oauth rows");
    assert_eq!(oauth_row_count, 1);
    let user_row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ?")
        .bind("linuxdo_best_effort_user")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count user rows");
    assert_eq!(user_row_count, 1);

    let binding_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count linuxdo bindings after skipped sync");
    assert_eq!(binding_count, 0);

    proxy
        .key_store
        .seed_linuxdo_system_tags()
        .await
        .expect("reseed linuxdo system tags");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-best-effort-user".to_string(),
            username: Some("linuxdo_best_effort_user".to_string()),
            name: Some("LinuxDo Best Effort User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("oauth upsert should attach system tag after reseeding tags");

    let restored_key: String = sqlx::query_scalar(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read restored linuxdo binding");
    assert_eq!(restored_key, "linuxdo_l2");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn legacy_custom_account_quota_limits_with_initial_timestamps_are_reclassified_before_default_resync()
 {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-legacy-custom");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-custom-user".to_string(),
            username: Some("legacy_custom_user".to_string()),
            name: Some("Legacy Custom User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_custom_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id)
        .await
        .expect("seed account quota row");
    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 101,
               hourly_limit = 102,
               daily_limit = 103,
               monthly_limit = 104,
               inherits_defaults = 1,
               updated_at = created_at
           WHERE user_id = ?"#,
    )
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate legacy custom quota row with initial timestamps");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear inherits defaults backfill marker");

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read persisted legacy custom limits");
    assert_eq!(limits, (101, 102, 103, 104, 0));

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn custom_account_quota_limits_survive_default_resync() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-custom-persist");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "limit-custom-user".to_string(),
            username: Some("limit_custom_user".to_string()),
            name: Some("Limit Custom User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:limit_custom_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id)
        .await
        .expect("seed account quota row");
    let updated = proxy
        .update_account_quota_limits(&user.user_id, 101, 102, 103, 104)
        .await
        .expect("update custom base quota");
    assert!(updated);

    let first_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read custom limits");
    assert_eq!(first_limits, (101, 102, 103, 104, 0));

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let second_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read persisted custom limits");
    assert_eq!(second_limits, (101, 102, 103, 104, 0));

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn block_all_user_tag_zeroes_effective_quota_and_blocks_account_usage() {
    let db_path = temp_db_path("user-tag-block-all");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "block-all-user".to_string(),
            username: Some("block_all_user".to_string()),
            name: Some("Block All User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:block_all_user"))
        .await
        .expect("bind token");
    let tag = proxy
        .create_user_tag(
            "blocked_all",
            "Blocked All",
            Some("ban"),
            USER_TAG_EFFECT_BLOCK_ALL,
            0,
            0,
            0,
            0,
        )
        .await
        .expect("create block all tag");
    let bound = proxy
        .bind_user_tag_to_user(&user.user_id, &tag.id)
        .await
        .expect("bind block all tag");
    assert!(bound);

    let details = proxy
        .get_admin_user_quota_details(&user.user_id)
        .await
        .expect("quota details")
        .expect("quota details present");
    assert_eq!(details.effective.hourly_any_limit, 0);
    assert_eq!(details.effective.hourly_limit, 0);
    assert_eq!(details.effective.daily_limit, 0);
    assert_eq!(details.effective.monthly_limit, 0);
    assert!(
        details
            .breakdown
            .iter()
            .any(|entry| entry.effect_kind == USER_TAG_EFFECT_BLOCK_ALL)
    );

    let hourly_any_verdict = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("hourly-any verdict");
    assert!(!hourly_any_verdict.allowed);
    assert_eq!(hourly_any_verdict.hourly_limit, 0);

    let quota_verdict = proxy
        .check_token_quota(&token.id)
        .await
        .expect("business quota verdict");
    assert!(!quota_verdict.allowed);
    assert_eq!(quota_verdict.hourly_limit, 0);
    assert_eq!(quota_verdict.daily_limit, 0);
    assert_eq!(quota_verdict.monthly_limit, 0);

    let request_usage: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_REQUEST_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read raw request usage");
    assert_eq!(request_usage, 0);
    let hourly_usage: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read hourly business usage");
    assert_eq!(hourly_usage, 0);
    let daily_usage: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read daily business usage");
    assert_eq!(daily_usage, 0);
    let monthly_usage = sqlx::query_scalar::<_, i64>(
        "SELECT month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("read monthly business usage")
    .unwrap_or(0);
    assert_eq!(monthly_usage, 0);

    let unbound = proxy
        .unbind_user_tag_from_user(&user.user_id, &tag.id)
        .await
        .expect("unbind block all tag");
    assert!(unbound);

    let hourly_any_after_unbind = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("hourly-any verdict after unbind");
    assert!(hourly_any_after_unbind.allowed);

    let quota_after_unbind = proxy
        .check_token_quota(&token.id)
        .await
        .expect("business quota verdict after unbind");
    assert!(quota_after_unbind.allowed);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn list_recent_jobs_paginated_includes_key_group() {
    let db_path = temp_db_path("jobs-list-key-group");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let grouped_key_id = proxy
        .add_or_undelete_key_in_group("tvly-jobs-grouped", Some("ops"))
        .await
        .expect("create grouped key");
    let ungrouped_key_id = proxy
        .add_or_undelete_key_in_group("tvly-jobs-ungrouped", None)
        .await
        .expect("create ungrouped key");

    let grouped_job_id = proxy
        .scheduled_job_start("quota_sync", Some(&grouped_key_id), 1)
        .await
        .expect("start grouped job");
    proxy
        .scheduled_job_finish(grouped_job_id, "error", Some("usage_http 401"))
        .await
        .expect("finish grouped job");

    let ungrouped_job_id = proxy
        .scheduled_job_start("quota_sync", Some(&ungrouped_key_id), 1)
        .await
        .expect("start ungrouped job");
    proxy
        .scheduled_job_finish(ungrouped_job_id, "success", Some("limit=100 remaining=99"))
        .await
        .expect("finish ungrouped job");

    let cleanup_job_id = proxy
        .scheduled_job_start("auth_token_logs_gc", None, 1)
        .await
        .expect("start cleanup job");
    proxy
        .scheduled_job_finish(cleanup_job_id, "success", Some("pruned=10"))
        .await
        .expect("finish cleanup job");

    let geo_job_id = proxy
        .scheduled_job_start("forward_proxy_geo_refresh", None, 1)
        .await
        .expect("start geo job");
    proxy
        .scheduled_job_finish(geo_job_id, "success", Some("refreshed_candidates=4"))
        .await
        .expect("finish geo job");

    let (items, total) = proxy
        .list_recent_jobs_paginated("all", 1, 10)
        .await
        .expect("list jobs");

    assert_eq!(total, 4);

    let grouped_job = items
        .iter()
        .find(|item| item.key_id.as_deref() == Some(grouped_key_id.as_str()))
        .expect("grouped job present");
    assert_eq!(grouped_job.key_group.as_deref(), Some("ops"));

    let ungrouped_job = items
        .iter()
        .find(|item| item.key_id.as_deref() == Some(ungrouped_key_id.as_str()))
        .expect("ungrouped job present");
    assert_eq!(ungrouped_job.key_group, None);

    let cleanup_job = items
        .iter()
        .find(|item| item.job_type == "auth_token_logs_gc")
        .expect("cleanup job present");
    assert_eq!(cleanup_job.key_group, None);

    let geo_job = items
        .iter()
        .find(|item| item.job_type == "forward_proxy_geo_refresh")
        .expect("geo job present");
    assert_eq!(geo_job.key_id, None);
    assert_eq!(geo_job.key_group, None);

    let (geo_items, geo_total) = proxy
        .list_recent_jobs_paginated("geo", 1, 10)
        .await
        .expect("list geo jobs");
    assert_eq!(geo_total, 1);
    assert_eq!(geo_items.len(), 1);
    assert_eq!(geo_items[0].job_type, "forward_proxy_geo_refresh");

    let _ = std::fs::remove_file(db_path);
}

async fn seed_charged_business_attempt(proxy: &TavilyProxy, token_id: &str, credits: i64) {
    let log_id = proxy
        .record_pending_billing_attempt(
            token_id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("seed charged business attempt"),
            credits,
            None,
        )
        .await
        .expect("record pending billing attempt");
    let outcome = proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle pending billing attempt");
    assert_eq!(outcome, PendingBillingSettleOutcome::Charged);
}

async fn current_month_charged_stats(pool: &SqlitePool) -> (i64, i64) {
    let now = Utc::now();
    let month_start = start_of_month(now).timestamp();
    sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            COUNT(*) AS charged_rows,
            COALESCE(SUM(business_credits), 0) AS charged_credits
        FROM auth_token_logs
        WHERE billing_state = ?
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
        "#,
    )
    .bind(BILLING_STATE_CHARGED)
    .bind(month_start)
    .fetch_one(pool)
    .await
    .expect("read current month charged stats")
}

async fn account_business_window_sums(pool: &SqlitePool, user_id: &str) -> (i64, i64) {
    let minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(pool)
    .await
    .expect("read account minute usage");
    let hour_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(pool)
    .await
    .expect("read account hour usage");
    (minute_sum, hour_sum)
}

#[tokio::test]
async fn billing_ledger_audit_detects_bound_token_month_residue_and_rebase_preserves_hour_day() {
    let db_path = temp_db_path("monthly-quota-rebase-bound-token");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "monthly-rebase-bound-user".to_string(),
            username: Some("monthly_bound".to_string()),
            name: Some("Monthly Bound".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:monthly_bound"))
        .await
        .expect("bind token");

    for _ in 0..5 {
        seed_charged_business_attempt(&proxy, &token.id, 1).await;
    }

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let baseline_verdict = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek bound quota");
    assert_eq!(baseline_verdict.hourly_used, 5);
    assert_eq!(baseline_verdict.daily_used, 5);
    assert_eq!(baseline_verdict.monthly_used, 5);

    let baseline_window_sums =
        account_business_window_sums(&proxy.key_store.pool, &user.user_id).await;
    let baseline_charged_stats = current_month_charged_stats(&proxy.key_store.pool).await;

    sqlx::query(
        r#"
        INSERT INTO account_monthly_quota (user_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&user.user_id)
    .bind(current_month_start)
    .bind(1_350_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt account month quota");
    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(token_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(1_959_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt token month quota");

    let audit_before = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, Utc::now())
        .await
        .expect("audit before rebase");
    assert_eq!(audit_before.summary.hour_only_mismatches, 0);
    assert_eq!(audit_before.summary.day_only_mismatches, 0);
    assert_eq!(audit_before.summary.month_only_mismatches, 2);
    assert_eq!(audit_before.summary.mixed_mismatches, 0);

    let token_subject = format!("token:{}", token.id);
    let token_entry = audit_before
        .entries
        .iter()
        .find(|entry| entry.billing_subject == token_subject)
        .expect("bound token entry present");
    assert_eq!(token_entry.hour.diff_credits, 0);
    assert_eq!(token_entry.day.diff_credits, 0);
    assert_eq!(token_entry.month.ledger_credits, 0);
    assert_eq!(token_entry.month.quota_credits, 1_959);

    let account_subject = format!("account:{}", user.user_id);
    let account_entry = audit_before
        .entries
        .iter()
        .find(|entry| entry.billing_subject == account_subject)
        .expect("bound account entry present");
    assert_eq!(account_entry.hour.diff_credits, 0);
    assert_eq!(account_entry.day.diff_credits, 0);
    assert_eq!(account_entry.month.ledger_credits, 5);
    assert_eq!(account_entry.month.quota_credits, 1_350);
    assert_eq!(account_entry.month.diff_credits, 1_345);

    let rebase_report = rebase_current_month_business_quota_with_pool(
        &proxy.key_store.pool,
        Utc::now(),
        META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
        true,
    )
    .await
    .expect("rebase current month");
    assert_eq!(rebase_report.current_month_charged_rows, 5);
    assert_eq!(rebase_report.current_month_charged_credits, 5);
    assert_eq!(rebase_report.rebased_subject_count, 1);
    assert_eq!(rebase_report.rebased_account_subjects, 1);
    assert_eq!(rebase_report.rebased_token_subjects, 0);
    assert!(rebase_report.cleared_token_rows >= 1);
    assert!(rebase_report.cleared_account_rows >= 1);

    let audit_after = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, Utc::now())
        .await
        .expect("audit after rebase");
    assert_eq!(audit_after.summary.mismatched_subjects, 0);

    let token_month_row: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read bound token monthly row");
    assert_eq!(token_month_row, (current_month_start, 0));

    let account_month_row: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account monthly row");
    assert_eq!(account_month_row, (current_month_start, 5));

    let charged_stats_after = current_month_charged_stats(&proxy.key_store.pool).await;
    assert_eq!(charged_stats_after, baseline_charged_stats);

    let post_window_sums = account_business_window_sums(&proxy.key_store.pool, &user.user_id).await;
    assert_eq!(post_window_sums, baseline_window_sums);

    let verdict_after = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek bound quota after");
    assert_eq!(verdict_after.hourly_used, 5);
    assert_eq!(verdict_after.daily_used, 5);
    assert_eq!(verdict_after.monthly_used, 5);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn monthly_quota_rebase_startup_gate_runs_once_and_manual_rebase_remains_idempotent() {
    let db_path = temp_db_path("monthly-quota-rebase-startup-gate");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("monthly-rebase-unbound"))
        .await
        .expect("create unbound token");

    for _ in 0..3 {
        seed_charged_business_attempt(&proxy, &token.id, 2).await;
    }

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let charged_stats_before = current_month_charged_stats(&proxy.key_store.pool).await;
    assert_eq!(charged_stats_before, (3, 6));

    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(token_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(17_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt token month quota");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset monthly rebase meta");

    let audit_before = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, Utc::now())
        .await
        .expect("audit before startup rebase");
    assert_eq!(audit_before.summary.month_only_mismatches, 1);

    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");

    let audit_after_startup =
        audit_business_quota_ledger_with_pool(&proxy_after.key_store.pool, Utc::now())
            .await
            .expect("audit after startup rebase");
    assert_eq!(audit_after_startup.summary.mismatched_subjects, 0);

    let token_month_count_after_startup: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy_after.key_store.pool)
            .await
            .expect("read token month after startup rebase");
    assert_eq!(token_month_count_after_startup, 6);

    let meta_value_after_startup: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
            .fetch_one(&proxy_after.key_store.pool)
            .await
            .expect("read startup rebase meta");
    assert_eq!(meta_value_after_startup, current_month_start);

    sqlx::query("UPDATE auth_token_quota SET month_count = ? WHERE token_id = ?")
        .bind(9_i64)
        .bind(&token.id)
        .execute(&proxy_after.key_store.pool)
        .await
        .expect("corrupt token month after startup rebase");
    drop(proxy_after);

    let proxy_third = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened third time");
    let token_month_count_after_third_start: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy_third.key_store.pool)
            .await
            .expect("read token month after third start");
    assert_eq!(
        token_month_count_after_third_start, 9,
        "startup gate should not rerun once current-month meta is already set"
    );

    let audit_after_third_start =
        audit_business_quota_ledger_with_pool(&proxy_third.key_store.pool, Utc::now())
            .await
            .expect("audit after third start");
    assert_eq!(audit_after_third_start.summary.month_only_mismatches, 1);

    let manual_rebase_report = rebase_current_month_business_quota_with_pool(
        &proxy_third.key_store.pool,
        Utc::now(),
        META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
        true,
    )
    .await
    .expect("manual rebase after startup gate");
    assert_eq!(
        manual_rebase_report.previous_rebase_month_start,
        Some(current_month_start)
    );
    assert!(!manual_rebase_report.meta_updated);
    assert_eq!(manual_rebase_report.rebased_subject_count, 1);
    assert_eq!(manual_rebase_report.rebased_token_subjects, 1);
    assert_eq!(manual_rebase_report.rebased_account_subjects, 0);

    let token_month_count_after_manual: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy_third.key_store.pool)
            .await
            .expect("read token month after manual rebase");
    assert_eq!(token_month_count_after_manual, 6);

    let audit_after_manual =
        audit_business_quota_ledger_with_pool(&proxy_third.key_store.pool, Utc::now())
            .await
            .expect("audit after manual rebase");
    assert_eq!(audit_after_manual.summary.mismatched_subjects, 0);

    let charged_stats_after_manual = current_month_charged_stats(&proxy_third.key_store.pool).await;
    assert_eq!(charged_stats_after_manual, charged_stats_before);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn begin_immediate_sqlite_connection_takes_write_lock_up_front() {
    use sqlx::Connection;

    let db_path = temp_db_path("monthly-quota-rebase-begin-immediate");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let mut immediate_conn = begin_immediate_sqlite_connection(&proxy.key_store.pool)
        .await
        .expect("begin immediate transaction");

    let writer_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(20));
    let mut competing_writer = sqlx::SqliteConnection::connect_with(&writer_options)
        .await
        .expect("connect competing writer");

    let write_err = sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?)")
        .bind(format!("begin-immediate-lock-{}", nanoid!(6)))
        .bind("1")
        .execute(&mut competing_writer)
        .await
        .expect_err("write should wait on the immediate transaction lock");
    assert!(is_transient_sqlite_write_error(&ProxyError::Database(
        write_err
    )));

    sqlx::query("ROLLBACK")
        .execute(&mut *immediate_conn)
        .await
        .expect("rollback immediate transaction");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[cfg(unix)]
#[tokio::test]
async fn billing_ledger_audit_reads_read_only_database_copy() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("billing-ledger-audit-read-only-{}", nanoid!(8)));
    fs::create_dir_all(&dir).expect("create temp audit dir");
    let db_path = dir.join("audit.db");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("audit-read-only-copy"))
        .await
        .expect("create token");
    seed_charged_business_attempt(&proxy, &token.id, 2).await;
    drop(proxy);

    let original_dir_mode = fs::metadata(&dir)
        .expect("read dir metadata")
        .permissions()
        .mode();
    let original_db_mode = fs::metadata(&db_path)
        .expect("read db metadata")
        .permissions()
        .mode();

    fs::set_permissions(&db_path, fs::Permissions::from_mode(0o444)).expect("make db read-only");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("make dir read-only");

    let audit_result = audit_business_quota_ledger(&db_str, Utc::now()).await;

    fs::set_permissions(&dir, fs::Permissions::from_mode(original_dir_mode))
        .expect("restore dir permissions");
    fs::set_permissions(&db_path, fs::Permissions::from_mode(original_db_mode))
        .expect("restore db permissions");

    let audit = audit_result.expect("audit read-only database");
    assert_eq!(audit.summary.current_month_charged_rows, 1);
    assert_eq!(audit.summary.current_month_charged_credits, 2);
    assert_eq!(audit.summary.mismatched_subjects, 0);

    let _ = fs::remove_file(&db_path);
    let _ = fs::remove_file(dir.join("audit.db-shm"));
    let _ = fs::remove_file(dir.join("audit.db-wal"));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn startup_monthly_rebase_skips_legacy_charged_rows_without_billing_subject() {
    let db_path = temp_db_path("startup-monthly-rebase-legacy-billing-subject-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("legacy-billing-subject-gap"))
        .await
        .expect("create token");

    seed_charged_business_attempt(&proxy, &token.id, 3).await;

    let month_count_before_restart: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read token month count before restart");
    assert_eq!(month_count_before_restart, 3);

    sqlx::query(
        r#"
        UPDATE auth_token_logs
        SET billing_subject = NULL
        WHERE token_id = ?
          AND billing_state = ?
          AND COALESCE(business_credits, 0) > 0
        "#,
    )
    .bind(&token.id)
    .bind(BILLING_STATE_CHARGED)
    .execute(&proxy.key_store.pool)
    .await
    .expect("clear billing subject on legacy charged row");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset monthly rebase meta");
    drop(proxy);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened despite legacy billing_subject gap");

    let month_count_after_restart: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("read token month count after restart");
    assert_eq!(month_count_after_restart, month_count_before_restart);

    let rebase_meta: Option<i64> =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
            .fetch_optional(&reopened.key_store.pool)
            .await
            .expect("read monthly rebase meta after skipped startup");
    assert_eq!(rebase_meta, None);

    let audit_err = audit_business_quota_ledger_with_pool(&reopened.key_store.pool, Utc::now())
        .await
        .expect_err("audit should still surface legacy billing_subject gap");
    assert!(is_invalid_current_month_billing_subject_error(&audit_err));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn manual_key_maintenance_actions_append_audit_records() {
    let db_path = temp_db_path("maintenance-manual");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-manual".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, api_key): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/mcp",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("seed quarantine");

    proxy
        .clear_key_quarantine_by_id_with_actor(
            &key_id,
            MaintenanceActor {
                auth_token_id: None,
                actor_user_id: Some("user-1".to_string()),
                actor_display_name: Some("Admin One".to_string()),
            },
        )
        .await
        .expect("clear quarantine with audit");
    proxy
        .mark_key_quota_exhausted_by_secret_with_actor(
            &api_key,
            MaintenanceActor {
                auth_token_id: None,
                actor_user_id: Some("user-1".to_string()),
                actor_display_name: Some("Admin One".to_string()),
            },
        )
        .await
        .expect("mark exhausted with audit");

    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64)>(
        r#"
        SELECT operation_code, actor_user_id, actor_display_name, quarantine_before, quarantine_after
        FROM api_key_maintenance_records
        WHERE key_id = ?
        ORDER BY operation_code ASC
        "#,
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch maintenance rows");

    assert_eq!(rows.len(), 2);
    let clear_row = rows
        .iter()
        .find(|row| row.0 == MAINTENANCE_OP_MANUAL_CLEAR_QUARANTINE)
        .expect("clear quarantine row");
    assert_eq!(clear_row.1.as_deref(), Some("user-1"));
    assert_eq!(clear_row.2.as_deref(), Some("Admin One"));
    assert_eq!(clear_row.3, 1);
    assert_eq!(clear_row.4, 0);
    assert!(
        rows.iter()
            .any(|row| row.0 == MAINTENANCE_OP_MANUAL_MARK_EXHAUSTED)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn auto_key_health_actions_append_audit_records() {
    let db_path = temp_db_path("maintenance-auto");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-auto".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    let quarantine_effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_ERROR,
                tavily_status_code: Some(401),
                key_health_action: KeyHealthAction::Quarantine(QuarantineDecision {
                    reason_code: "account_deactivated".to_string(),
                    reason_summary: "Tavily account deactivated (HTTP 401)".to_string(),
                    reason_detail: "deactivated".to_string(),
                }),
                failure_kind: Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401.to_string()),
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("auto quarantine");
    assert_eq!(quarantine_effect.code, KEY_EFFECT_QUARANTINED);

    proxy
        .key_store
        .mark_quota_exhausted(&secret)
        .await
        .expect("seed exhausted");
    let restore_effect = proxy
        .reconcile_key_health(
            &lease,
            "/api/tavily/search",
            &AttemptAnalysis {
                status: OUTCOME_SUCCESS,
                tavily_status_code: Some(200),
                key_health_action: KeyHealthAction::None,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("auto restore");
    assert_eq!(restore_effect.code, KEY_EFFECT_RESTORED_ACTIVE);

    let ops = sqlx::query_scalar::<_, String>(
        r#"
        SELECT operation_code
        FROM api_key_maintenance_records
        WHERE key_id = ?
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch operation codes");

    assert!(ops.contains(&MAINTENANCE_OP_AUTO_QUARANTINE.to_string()));
    assert!(ops.contains(&MAINTENANCE_OP_AUTO_RESTORE_ACTIVE.to_string()));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconcile_key_health_reports_none_when_state_already_changed() {
    let db_path = temp_db_path("maintenance-repeat-noop");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-repeat-noop".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    proxy
        .key_store
        .mark_quota_exhausted(&secret)
        .await
        .expect("seed exhausted");
    let exhausted_effect = proxy
        .reconcile_key_health(
            &lease,
            "/api/tavily/search",
            &AttemptAnalysis {
                status: OUTCOME_QUOTA_EXHAUSTED,
                tavily_status_code: Some(432),
                key_health_action: KeyHealthAction::MarkExhausted,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("repeat exhausted");
    assert_eq!(exhausted_effect.code, KEY_EFFECT_NONE);

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/mcp",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("seed quarantine");
    let quarantine_effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_ERROR,
                tavily_status_code: Some(401),
                key_health_action: KeyHealthAction::Quarantine(QuarantineDecision {
                    reason_code: "account_deactivated".to_string(),
                    reason_summary: "Tavily account deactivated (HTTP 401)".to_string(),
                    reason_detail: "deactivated".to_string(),
                }),
                failure_kind: Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401.to_string()),
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("repeat quarantine");
    assert_eq!(quarantine_effect.code, KEY_EFFECT_NONE);

    let maintenance_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_key_maintenance_records WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count maintenance records");
    assert_eq!(maintenance_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconcile_key_health_does_not_restore_exhausted_key_on_error() {
    let db_path = temp_db_path("maintenance-no-restore-on-error");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-no-restore".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    proxy
        .key_store
        .mark_quota_exhausted(&secret)
        .await
        .expect("seed exhausted");

    let effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_ERROR,
                tavily_status_code: Some(429),
                key_health_action: KeyHealthAction::None,
                failure_kind: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429.to_string()),
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("error should not restore");
    assert_eq!(effect.code, KEY_EFFECT_NONE);

    let status: String = sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ? LIMIT 1")
        .bind(&key_id)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read key status");
    assert_eq!(status, STATUS_EXHAUSTED);

    let maintenance_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_key_maintenance_records WHERE key_id = ?")
            .bind(&key_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count maintenance records");
    assert_eq!(maintenance_count, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn usage_error_quarantine_appends_audit_record() {
    let db_path = temp_db_path("maintenance-usage-quarantine");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-usage-quarantine".to_string()],
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
        .maybe_quarantine_usage_error(
            &key_id,
            "/api/tavily/usage",
            &ProxyError::UsageHttp {
                status: StatusCode::UNAUTHORIZED,
                body: "The account associated with this API key has been deactivated.".to_string(),
            },
        )
        .await
        .expect("usage quarantine");

    let op_codes = sqlx::query_scalar::<_, String>(
        "SELECT operation_code FROM api_key_maintenance_records WHERE key_id = ?",
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch maintenance operations");
    assert_eq!(op_codes, vec![MAINTENANCE_OP_AUTO_QUARANTINE.to_string()]);

    let _ = std::fs::remove_file(db_path);
}

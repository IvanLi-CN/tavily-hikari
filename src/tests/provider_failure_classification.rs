//! Plan todo 3 — per-provider failure classification, request-kind taxonomy,
//! and the generalized exhaustion gate. Pure unit coverage (no DB): the
//! provider x status classifier matrix, the three QA scenarios (SerpApi
//! dual-429 discrimination raw + SSE, Brave 422 VALIDATION loc guard), the M7
//! tool-name collision guard, the M8 Tavily-only depletion gate, the M9
//! static-whitelist surfaces, and Tavily byte-stability pins.

use super::*;
use crate::providers::ProviderId;
use axum::http::{HeaderName, HeaderValue};

fn classify(provider: ProviderId, status: u16, body: &[u8]) -> ProviderFailureSignal {
    classify_provider_failure(
        provider,
        &ProviderFailureEvidence {
            status,
            headers: None,
            body,
        },
    )
}

fn classify_with_headers(
    provider: ProviderId,
    status: u16,
    headers: &HeaderMap,
    body: &[u8],
) -> ProviderFailureSignal {
    classify_provider_failure(
        provider,
        &ProviderFailureEvidence {
            status,
            headers: Some(headers),
            body,
        },
    )
}

fn headers_with(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        let name = HeaderName::from_bytes(name.as_bytes()).expect("header name");
        map.insert(name, HeaderValue::from_str(value).expect("header value"));
    }
    map
}

fn tools_call_body(tool: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": {} }
    }))
    .expect("tools/call body serializes")
}

fn mcp_kind_for(provider: ProviderId, tool: &str) -> String {
    classify_provider_mcp_request_kind(
        provider,
        provider_mcp_ingress_path(provider),
        Some(&tools_call_body(tool)),
    )
    .key
}

// ---------------------------------------------------------------------------
// QA scenario (happy): SerpApi 429 with the run-out body classifies Exhausted.
// ---------------------------------------------------------------------------

#[test]
fn serpapi_429_run_out_body_classifies_exhausted() {
    // Given the canonical SerpApi monthly-exhaustion body (draft contract)
    let body = br#"{"error":"Your account has run out of searches."}"#;

    // When SerpApi answers 429
    let signal = classify(ProviderId::Serpapi, 429, body);

    // Then the key-health signal is Exhausted (drives MarkExhausted)
    assert_eq!(signal, ProviderFailureSignal::Exhausted);
}

// ---------------------------------------------------------------------------
// QA scenario (failure A): the hourly-throughput 429 stays RateLimited.
// ---------------------------------------------------------------------------

#[test]
fn serpapi_429_hourly_throughput_classifies_rate_limited_not_exhausted() {
    // Given the mock_serpapi hourly-throughput message (NOT the run-out body)
    let body = br#"{"error":"Your account has exceeded its hourly throughput limit."}"#;

    // When SerpApi answers 429
    let signal = classify(ProviderId::Serpapi, 429, body);

    // Then it is a transient rate limit, never an exhaustion mark
    assert_eq!(
        signal,
        ProviderFailureSignal::RateLimited {
            retry_after_secs: None
        }
    );
    assert_ne!(signal, ProviderFailureSignal::Exhausted);
}

#[test]
fn serpapi_429_sse_wrapped_run_out_classifies_exhausted() {
    // Given the passthrough SSE envelope (mock_serpapi `data: {...}` frame)
    let body = b"data: {\"error\":\"Your account has run out of searches.\"}\n\n";

    // When SerpApi MCP passthrough answers 429 inside an event stream
    let signal = classify(ProviderId::Serpapi, 429, body);

    // Then the enveloped run-out payload still exhausts (amendment M7 / C5)
    assert_eq!(signal, ProviderFailureSignal::Exhausted);
}

#[test]
fn serpapi_429_sse_wrapped_jsonrpc_error_run_out_classifies_exhausted() {
    // Given a JSON-RPC error envelope wrapped in an SSE frame
    let body = b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32000,\"message\":\"Your account has run out of searches.\"}}\n\n";

    // When SerpApi MCP passthrough answers 429
    let signal = classify(ProviderId::Serpapi, 429, body);

    // Then the nested error message still exhausts
    assert_eq!(signal, ProviderFailureSignal::Exhausted);
}

#[test]
fn serpapi_429_sse_wrapped_hourly_throughput_classifies_rate_limited() {
    // Given the hourly-throughput message inside an SSE frame
    let body = b"data: {\"error\":\"Your account has exceeded its hourly throughput limit.\"}\n\n";

    // When SerpApi answers 429
    let signal = classify(ProviderId::Serpapi, 429, body);

    // Then the envelope changes nothing: transient backoff only (C5)
    assert_eq!(
        signal,
        ProviderFailureSignal::RateLimited {
            retry_after_secs: None
        }
    );
}

// ---------------------------------------------------------------------------
// QA scenario (failure B): Brave 422 VALIDATION on a query loc stays ClientError.
// ---------------------------------------------------------------------------

#[test]
fn brave_422_validation_query_loc_classifies_client_error_not_auth_failed() {
    // Given a VALID subscription token and a VALIDATION error about the query
    let body = br#"{"error":{"code":"VALIDATION","detail":"Request validation failed","meta":{"errors":[{"loc":["query","q"],"msg":"field required"}]},"status":422},"type":"ErrorResponse"}"#;

    // When Brave answers 422
    let signal = classify(ProviderId::Brave, 422, body);

    // Then the key is NOT penalized as an auth failure
    assert_eq!(signal, ProviderFailureSignal::ClientError);
    assert_ne!(signal, ProviderFailureSignal::AuthFailed);
}

#[test]
fn brave_422_validation_header_loc_classifies_auth_failed() {
    // Given the missing-token probe shape (loc points at the auth header)
    let body = br#"{"error":{"code":"VALIDATION","detail":"Request validation failed","meta":{"errors":[{"loc":["header","x-subscription-token"],"msg":"Field required"}]},"status":422},"type":"ErrorResponse"}"#;

    // When Brave answers 422
    let signal = classify(ProviderId::Brave, 422, body);

    // Then only the subscription-token loc promotes VALIDATION to AuthFailed
    assert_eq!(signal, ProviderFailureSignal::AuthFailed);
}

// ---------------------------------------------------------------------------
// Classifier matrix: every provider x contract status (plan todo 3 acceptance).
// ---------------------------------------------------------------------------

#[test]
fn serpapi_failure_matrix_covers_contract_statuses() {
    // Given the SerpApi contract (draft: api-status-and-error-codes)
    let cases: &[(u16, &[u8], ProviderFailureSignal)] = &[
        (
            200,
            br#"{"search_metadata":{"id":"s1","status":"Success"}}"#,
            ProviderFailureSignal::Ok,
        ),
        (
            400,
            br#"{"error":"Missing query `q` parameter."}"#,
            ProviderFailureSignal::ClientError,
        ),
        (
            401,
            br#"{"error":"Invalid API key. Please pass api_key as a query param."}"#,
            ProviderFailureSignal::AuthFailed,
        ),
        (402, b"{}", ProviderFailureSignal::ClientError),
        (
            403,
            br#"{"error":"You don't have permission to use this API."}"#,
            ProviderFailureSignal::AuthFailed,
        ),
        (
            410,
            br#"{"error":"Search archive entry is gone."}"#,
            ProviderFailureSignal::ClientError,
        ),
        // SerpApi reports invalid keys via 401, never 422 — an unexpected 422
        // (token-invalid or validation shaped) stays a non-penalizing client error.
        (
            422,
            br#"{"error":"Invalid API key"}"#,
            ProviderFailureSignal::ClientError,
        ),
        (
            422,
            br#"{"error":"Validation failed"}"#,
            ProviderFailureSignal::ClientError,
        ),
        (
            503,
            br#"{"error":"Search failed."}"#,
            ProviderFailureSignal::ServerTransient,
        ),
    ];

    for (status, body, expected) in cases {
        // When each contract status arrives, then the signal matches the matrix
        // (429 exhaust-vs-rate discrimination is pinned by the QA tests above).
        assert_eq!(
            classify(ProviderId::Serpapi, *status, body),
            *expected,
            "serpapi status {status}"
        );
    }
}

#[test]
fn exa_failure_matrix_covers_contract_statuses() {
    // Given the Exa error envelope {requestId,error,tag} (draft contract)
    let cases: &[(u16, &[u8], ProviderFailureSignal)] = &[
        (
            200,
            br#"{"requestId":"r1","results":[]}"#,
            ProviderFailureSignal::Ok,
        ),
        (
            400,
            br#"{"requestId":"r1","error":"Bad request","tag":"BAD_REQUEST"}"#,
            ProviderFailureSignal::ClientError,
        ),
        (
            401,
            br#"{"requestId":"r1","error":"Invalid API key","tag":"INVALID_API_KEY"}"#,
            ProviderFailureSignal::AuthFailed,
        ),
        (
            402,
            br#"{"requestId":"r1","error":"No more credits","tag":"NO_MORE_CREDITS"}"#,
            ProviderFailureSignal::Exhausted,
        ),
        (
            402,
            br#"{"requestId":"r1","error":"Key budget hit","tag":"API_KEY_BUDGET_EXCEEDED"}"#,
            ProviderFailureSignal::Exhausted,
        ),
        (
            402,
            br#"{"requestId":"r1","error":"Team budget hit","tag":"TEAM_BUDGET_EXCEEDED"}"#,
            ProviderFailureSignal::Exhausted,
        ),
        (
            402,
            br#"{"requestId":"r1","error":"Payment required","tag":"OTHER"}"#,
            ProviderFailureSignal::ClientError,
        ),
        (402, b"{}", ProviderFailureSignal::ClientError),
        (
            403,
            br#"{"requestId":"r1","error":"Feature disabled","tag":"FEATURE_DISABLED"}"#,
            ProviderFailureSignal::ClientError,
        ),
        (410, b"{}", ProviderFailureSignal::ClientError),
        (
            422,
            br#"{"requestId":"r1","error":"Validation failed","tag":"VALIDATION_ERROR"}"#,
            ProviderFailureSignal::ClientError,
        ),
        (
            503,
            br#"{"requestId":"r1","error":"Overloaded","tag":"SERVICE_OVERLOADED"}"#,
            ProviderFailureSignal::ServerTransient,
        ),
    ];

    for (status, body, expected) in cases {
        // When/Then: 403 carries NO key penalty; untagged 402 never exhausts
        // (429 Retry-After parsing is pinned by its dedicated test).
        assert_eq!(
            classify(ProviderId::Exa, *status, body),
            *expected,
            "exa status {status}"
        );
    }
}

#[test]
fn exa_429_parses_retry_after_header() {
    // Given the documented Exa rate-limit shape (429 + Retry-After seconds)
    let body = br#"{"requestId":"r1","error":"Rate limited","tag":"RATE_LIMIT_EXCEEDED"}"#;

    // When Retry-After is present, then the hint rides the signal
    let with_header = headers_with(&[("retry-after", "2")]);
    assert_eq!(
        classify_with_headers(ProviderId::Exa, 429, &with_header, body),
        ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(2)
        }
    );

    // When Retry-After is absent or unparsable, then the hint stays None
    assert_eq!(
        classify(ProviderId::Exa, 429, body),
        ProviderFailureSignal::RateLimited {
            retry_after_secs: None
        }
    );
    let garbage = headers_with(&[("retry-after", "soon")]);
    assert_eq!(
        classify_with_headers(ProviderId::Exa, 429, &garbage, body),
        ProviderFailureSignal::RateLimited {
            retry_after_secs: None
        }
    );
}

#[test]
fn brave_failure_matrix_covers_contract_statuses() {
    // Given the Brave error envelope {error:{code,detail,meta,status}} and the
    // live-probed 422-not-401 invalid-token fact (draft contract)
    let cases: &[(u16, &[u8], ProviderFailureSignal)] = &[
        (200, br#"{"type":"search","query":{}}"#, ProviderFailureSignal::Ok),
        (400, b"{}", ProviderFailureSignal::ClientError),
        (401, b"{}", ProviderFailureSignal::AuthFailed),
        (
            402,
            br#"{"error":{"code":"QUOTA_LIMITED","detail":"Quota limited","meta":null,"status":402},"type":"ErrorResponse"}"#,
            ProviderFailureSignal::Exhausted,
        ),
        (402, b"{}", ProviderFailureSignal::Exhausted),
        (403, b"{}", ProviderFailureSignal::ClientError),
        (410, b"{}", ProviderFailureSignal::ClientError),
        (
            422,
            br#"{"error":{"code":"SUBSCRIPTION_TOKEN_INVALID","detail":"Subscription token is not valid.","meta":null,"status":422},"type":"ErrorResponse"}"#,
            ProviderFailureSignal::AuthFailed,
        ),
        (
            429,
            br#"{"error":{"code":"RATE_LIMITED","detail":"Rate limited","meta":null,"status":429},"type":"ErrorResponse"}"#,
            ProviderFailureSignal::RateLimited { retry_after_secs: None },
        ),
        (
            429,
            b"{}",
            ProviderFailureSignal::RateLimited { retry_after_secs: None },
        ),
        (503, b"{}", ProviderFailureSignal::ServerTransient),
    ];

    for (status, body, expected) in cases {
        // When/Then (422 VALIDATION loc discrimination is pinned by the QA
        // failure-B test above).
        assert_eq!(
            classify(ProviderId::Brave, *status, body),
            *expected,
            "brave status {status}"
        );
    }
}

#[test]
fn brave_402_captures_rate_limit_reset_window_hint() {
    // Given a quota-limited response advertising the month-window reset
    let body = br#"{"error":{"code":"QUOTA_LIMITED","detail":"Quota limited","meta":null,"status":402},"type":"ErrorResponse"}"#;
    let headers = headers_with(&[("x-ratelimit-reset", "1735689600")]);

    // When the exhausted signal is classified
    let signal = classify_with_headers(ProviderId::Brave, 402, &headers, body);

    // Then the signal exhausts and the reset hint is captured beside it
    // (plan-fixed enum shape; todo 14 passive harvest consumes the helper)
    assert_eq!(signal, ProviderFailureSignal::Exhausted);
    assert_eq!(
        brave_rate_limit_reset_secs(Some(&headers)),
        Some(1_735_689_600)
    );
    assert_eq!(brave_rate_limit_reset_secs(None), None);
    let garbage = headers_with(&[("x-ratelimit-reset", "next-month")]);
    assert_eq!(brave_rate_limit_reset_secs(Some(&garbage)), None);
}

#[test]
fn brave_429_honors_retry_after_when_present() {
    // Given a rate-limited response that also carries Retry-After
    let body =
        br#"{"error":{"code":"RATE_LIMITED","meta":null,"status":429},"type":"ErrorResponse"}"#;
    let headers = headers_with(&[("retry-after", "1")]);

    // When/Then the hint rides the RateLimited signal
    assert_eq!(
        classify_with_headers(ProviderId::Brave, 429, &headers, body),
        ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(1)
        }
    );
}

#[test]
fn tavily_failure_matrix_mirrors_live_semantics() {
    // Given the Tavily status vocabulary (432 is THE exhaustion status)
    let cases: &[(u16, ProviderFailureSignal)] = &[
        (200, ProviderFailureSignal::Ok),
        (400, ProviderFailureSignal::ClientError),
        (401, ProviderFailureSignal::AuthFailed),
        (402, ProviderFailureSignal::ClientError),
        (403, ProviderFailureSignal::AuthFailed),
        (410, ProviderFailureSignal::ClientError),
        (422, ProviderFailureSignal::ClientError),
        (
            429,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None,
            },
        ),
        (432, ProviderFailureSignal::Exhausted),
        (503, ProviderFailureSignal::ServerTransient),
    ];

    for (status, expected) in cases {
        // When/Then the mirrored enum never rewrites live Tavily behavior
        assert_eq!(
            classify(ProviderId::Tavily, *status, b"{}"),
            *expected,
            "tavily status {status}"
        );
    }
}

// ---------------------------------------------------------------------------
// Tavily byte-stability pins for the LIVE pipeline outputs (todo 3 MUST NOT).
// ---------------------------------------------------------------------------

#[test]
fn tavily_live_pipeline_outputs_stay_exact() {
    // Given the live 432 HTTP attempt, When analyzed, Then quota-exhausted
    // outcome + MarkExhausted + the marked-exhausted key effect (unchanged)
    let analysis = analyze_http_attempt(StatusCode::from_u16(432).expect("432 status"), b"{}");
    assert_eq!(analysis.status, OUTCOME_QUOTA_EXHAUSTED);
    assert_eq!(analysis.tavily_status_code, Some(432));
    assert_eq!(analysis.key_health_action, KeyHealthAction::MarkExhausted);
    assert_eq!(analysis.key_effect.code, KEY_EFFECT_MARKED_EXHAUSTED);

    // Given a Tavily 429, Then the failure kind stays upstream_rate_limited_429
    assert_eq!(
        classify_failure_kind("/mcp", Some(429), Some(429), None, b"{}").as_deref(),
        Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
    );

    // Given an unrecognized Tavily 403, Then the kind stays upstream_unknown_403
    assert_eq!(
        classify_failure_kind(
            "/mcp",
            Some(200),
            Some(403),
            None,
            br#"{"error":"Forbidden"}"#
        )
        .as_deref(),
        Some(FAILURE_KIND_UPSTREAM_UNKNOWN_403)
    );
}

#[test]
fn provider_signals_map_into_house_failure_kind_taxonomy() {
    // Given/When/Then: the bridge reuses EXISTING lib.rs failure-kind constants
    assert_eq!(
        ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(2)
        }
        .failure_kind(),
        Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
    );
    assert_eq!(
        ProviderFailureSignal::AuthFailed.failure_kind(),
        Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401)
    );
    assert_eq!(
        ProviderFailureSignal::ServerTransient.failure_kind(),
        Some(FAILURE_KIND_UPSTREAM_GATEWAY_5XX)
    );
    assert_eq!(
        ProviderFailureSignal::ClientError.failure_kind(),
        Some(FAILURE_KIND_OTHER)
    );
    assert_eq!(ProviderFailureSignal::Exhausted.failure_kind(), None);
    assert_eq!(ProviderFailureSignal::Ok.failure_kind(), None);
}

#[test]
fn provider_failure_kinds_feed_operational_class() {
    // Given bridged failure kinds on new provider request kinds, Then the
    // operational class surfaces upstream errors and exhaustion unchanged
    assert_eq!(
        operational_class_for_request_kind(
            "api:exa/search",
            OUTCOME_ERROR,
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        ),
        OPERATIONAL_CLASS_UPSTREAM_ERROR
    );
    assert_eq!(
        operational_class_for_request_kind(
            "api:brave/web",
            OUTCOME_ERROR,
            Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401)
        ),
        OPERATIONAL_CLASS_UPSTREAM_ERROR
    );
    assert_eq!(
        operational_class_for_request_kind("mcp:serpapi/search", OUTCOME_QUOTA_EXHAUSTED, None),
        OPERATIONAL_CLASS_QUOTA_EXHAUSTED
    );
}

// ---------------------------------------------------------------------------
// M8: the depletion gate stays Tavily-432-only.
// ---------------------------------------------------------------------------

#[test]
fn low_quota_depletion_gate_stays_tavily_432_only() {
    // Given/When/Then: only Tavily + 432 records low-quota depletion; every
    // new-provider exhaustion status is a NEGATIVE case (amendment M8)
    assert!(low_quota_depletion_applies(ProviderId::Tavily, Some(432)));
    assert!(!low_quota_depletion_applies(ProviderId::Tavily, Some(429)));
    assert!(!low_quota_depletion_applies(ProviderId::Tavily, None));
    assert!(!low_quota_depletion_applies(ProviderId::Serpapi, Some(432)));
    assert!(!low_quota_depletion_applies(ProviderId::Serpapi, Some(429)));
    assert!(!low_quota_depletion_applies(ProviderId::Exa, Some(402)));
    assert!(!low_quota_depletion_applies(ProviderId::Brave, Some(402)));
}

#[test]
fn brave_rate_limited_backoff_is_second_scale() {
    // Given Brave's second-scale w=1 window, Then its RateLimited backoff is
    // short (default 2s, clamped 1..=30) while other providers keep the
    // minute-scale MCP-init ladder (Retry-After first, 30..=300 clamp, 60s)
    let brave = |signal| {
        TavilyProxy::effective_provider_rate_limited_backoff_secs(ProviderId::Brave, signal, 0)
    };
    assert_eq!(
        brave(ProviderFailureSignal::RateLimited {
            retry_after_secs: None
        }),
        Some(2)
    );
    assert_eq!(
        brave(ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(0)
        }),
        Some(1)
    );
    assert_eq!(
        brave(ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(9_999)
        }),
        Some(30)
    );
    assert_eq!(brave(ProviderFailureSignal::Exhausted), None);

    let exa = |signal| {
        TavilyProxy::effective_provider_rate_limited_backoff_secs(ProviderId::Exa, signal, 0)
    };
    assert_eq!(
        exa(ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(2)
        }),
        Some(30)
    );
    assert_eq!(
        exa(ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(120)
        }),
        Some(120)
    );
    assert_eq!(
        exa(ProviderFailureSignal::RateLimited {
            retry_after_secs: None
        }),
        Some(60)
    );

    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Serpapi,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(45)
            },
            0
        ),
        Some(45)
    );
}

// ---------------------------------------------------------------------------
// M7: provider-context tool normalization + collision guard (C5).
// ---------------------------------------------------------------------------

#[test]
fn serpapi_tools_never_alias_to_tavily_mcp_kinds() {
    // Given a SerpApi passthrough tools/call, Then it bills under the
    // serpapi-prefixed kind — NEVER mcp:search, NEVER mcp:third-party-tool
    assert_eq!(
        mcp_kind_for(ProviderId::Serpapi, "search"),
        "mcp:serpapi/search"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Serpapi, "search_table"),
        "mcp:serpapi/search_table"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Serpapi, "search_dashboard"),
        "mcp:serpapi/search_dashboard"
    );

    // Given the SAME tool name under Tavily provider context, Then the bare
    // `search` stays a third-party tool (Tavily byte-stable) — collision guard
    assert_eq!(
        mcp_kind_for(ProviderId::Tavily, "search"),
        "mcp:third-party-tool"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Tavily, "tavily-search"),
        "mcp:search"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Serpapi, "tavily-search"),
        "mcp:third-party-tool"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Exa, "brave_web_search"),
        "mcp:third-party-tool"
    );
}

#[test]
fn exa_and_brave_facade_tools_map_to_enumerated_kinds() {
    // Given the local-facade tool catalogs (M7 enumerated kind keys)
    assert_eq!(
        mcp_kind_for(ProviderId::Exa, "web_search_exa"),
        "mcp:exa/web_search"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Exa, "web_fetch_exa"),
        "mcp:exa/web_fetch"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Exa, "exa_answer"),
        "mcp:exa/answer"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Brave, "brave_web_search"),
        "mcp:brave/web_search"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Brave, "brave_news_search"),
        "mcp:brave/news_search"
    );
    assert_eq!(
        mcp_kind_for(ProviderId::Brave, "brave_llm_context"),
        "mcp:brave/llm_context"
    );

    // Given case/whitespace noise, Then normalization is case-insensitive
    assert_eq!(
        mcp_kind_for(ProviderId::Serpapi, "  SEARCH_Table "),
        "mcp:serpapi/search_table"
    );
}

#[test]
fn provider_mcp_control_plane_kinds_stay_shared_and_non_billable() {
    // Given non-tools/call methods on a provider ingress, Then the shared
    // canonical control-plane kinds apply (initialize/tools-list non-billable)
    let initialize = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
    }))
    .expect("initialize body");
    let kind =
        classify_provider_mcp_request_kind(ProviderId::Serpapi, "/serpapi/mcp", Some(&initialize));
    assert_eq!(kind.key, "mcp:initialize");
    assert_eq!(token_request_kind_billing_group(&kind.key), "non_billable");

    let resources = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0", "id": 2, "method": "resources/list", "params": {}
    }))
    .expect("resources body");
    let kind =
        classify_provider_mcp_request_kind(ProviderId::Serpapi, "/serpapi/mcp", Some(&resources));
    assert_eq!(kind.key, "mcp:resources/list");
    assert_eq!(token_request_kind_billing_group(&kind.key), "non_billable");
}

#[test]
fn serpapi_mcp_batch_mirrors_tavily_batch_shape() {
    // Given a homogeneous SerpApi batch, Then the single kind wins
    let body = serde_json::to_vec(&serde_json::json!([
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search"}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "search"}}
    ]))
    .expect("batch body");
    let kind = classify_provider_mcp_request_kind(ProviderId::Serpapi, "/serpapi/mcp", Some(&body));
    assert_eq!(kind.key, "mcp:serpapi/search");

    // Given a mixed batch, Then the shared batch kind carries label details
    let mixed = serde_json::to_vec(&serde_json::json!([
        {"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search"}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "search_table"}}
    ]))
    .expect("mixed batch body");
    let kind =
        classify_provider_mcp_request_kind(ProviderId::Serpapi, "/serpapi/mcp", Some(&mixed));
    assert_eq!(kind.key, "mcp:batch");
    assert_eq!(
        kind.detail.as_deref(),
        Some("serpapi/search, serpapi/search_table")
    );
}

// ---------------------------------------------------------------------------
// M9: STATIC whitelist surfaces (Rust catalog + SQL literals).
// ---------------------------------------------------------------------------

#[test]
fn provider_kinds_cover_full_static_whitelist_surface() {
    // Given the additive catalog, Then every key is canonical, labeled,
    // billable, Valuable-bucketed, filter-stable, and protocol-grouped
    for (key, label) in PROVIDER_REQUEST_KINDS {
        assert!(is_canonical_request_kind_key(key), "canonical {key}");
        assert_eq!(
            canonical_request_kind_label(key),
            Some(*label),
            "label {key}"
        );
        assert_eq!(
            token_request_kind_from_canonical_key(key, None)
                .expect("canonical triple")
                .label,
            *label
        );
        assert_eq!(canonical_request_kind_key_for_filter(key), *key);
        assert_eq!(
            token_request_kind_billing_group(key),
            "billable",
            "billable {key}"
        );
        assert_eq!(
            request_value_bucket_for_request_log(key, None),
            RequestValueBucket::Valuable,
            "valuable {key}"
        );
        let expected_group = if key.starts_with("mcp:") {
            "mcp"
        } else {
            "api"
        };
        assert_eq!(token_request_kind_protocol_group(key), expected_group);
    }

    // Given the non-billable MCP SQL surface, Then no billable provider kind
    // leaks into it (initialize/ping/tools-list/resources stay non-billable)
    let non_billable_sql = token_request_kind_non_billable_mcp_sql("k");
    for (key, _) in PROVIDER_REQUEST_KINDS
        .iter()
        .filter(|(k, _)| k.starts_with("mcp:"))
    {
        assert!(!non_billable_sql.contains(&format!("'{key}'")), "{key}");
    }
    assert!(non_billable_sql.contains("'mcp:initialize'"));
}

#[test]
fn sql_literal_surfaces_carry_provider_kinds() {
    // Given the stored-predicate surface, Then every provider key is whitelisted
    let predicate_sql = canonical_request_kind_stored_predicate_sql("k");
    for (key, _) in PROVIDER_REQUEST_KINDS {
        assert!(predicate_sql.contains(&format!("'{key}'")), "pred {key}");
    }

    // Given the canonical label SQL, Then every provider key rides the single
    // IN-guarded humanization row (per-key rows would explode the composed SQL)
    let label_sql = canonical_request_kind_label_sql("k");
    assert!(label_sql.contains("UPPER(SUBSTR("));
    for (key, _) in PROVIDER_REQUEST_KINDS {
        assert!(label_sql.contains(&format!("'{key}'")), "label sql {key}");
    }

    // Given the stored-kind SQL, Then provider keys pass through the IN list
    let stored_kind_sql = token_log_request_kind_key_sql("p", "k");
    for (key, _) in PROVIDER_REQUEST_KINDS {
        assert!(stored_kind_sql.contains(&format!("'{key}'")), "kind {key}");
    }

    // Given the value-bucket SQL, Then provider kinds land in the valuable IN list
    let bucket_sql = request_value_bucket_sql("k", "b");
    for (key, _) in PROVIDER_REQUEST_KINDS {
        assert!(bucket_sql.contains(&format!("'{key}'")), "bucket {key}");
    }
}

#[test]
fn classify_token_request_kind_dispatches_provider_paths() {
    // Given provider REST facade paths, Then the canonical API kinds apply
    for (path, key, label) in [
        (
            "/api/serpapi/search",
            "api:serpapi/search",
            "API | serpapi/search",
        ),
        ("/api/exa/search", "api:exa/search", "API | exa/search"),
        (
            "/api/exa/contents",
            "api:exa/contents",
            "API | exa/contents",
        ),
        ("/api/exa/answer", "api:exa/answer", "API | exa/answer"),
        ("/api/brave/web/search", "api:brave/web", "API | brave/web"),
        (
            "/api/brave/news/search",
            "api:brave/news",
            "API | brave/news",
        ),
        (
            "/api/brave/llm/context",
            "api:brave/llm-context",
            "API | brave/llm-context",
        ),
    ] {
        let kind = classify_token_request_kind(path, None);
        assert_eq!(kind.key, key, "path {path}");
        assert_eq!(kind.label, label);
    }

    // Given provider MCP ingress paths, Then provider-context tool kinds apply
    let kind = classify_token_request_kind("/serpapi/mcp", Some(&tools_call_body("search_table")));
    assert_eq!(kind.key, "mcp:serpapi/search_table");
    assert_eq!(
        classify_token_request_kind("/exa/mcp", Some(&tools_call_body("web_fetch_exa"))).key,
        "mcp:exa/web_fetch"
    );
    assert_eq!(
        classify_token_request_kind("/brave/mcp", Some(&tools_call_body("brave_llm_context"))).key,
        "mcp:brave/llm_context"
    );

    // Given a provider MCP subpath, Then it stays the shared unsupported-path
    // kind with the original path preserved in detail (lossless history)
    let kind = classify_token_request_kind("/serpapi/mcp/sub", None);
    assert_eq!(kind.key, "mcp:unsupported-path");
    assert_eq!(kind.detail.as_deref(), Some("/serpapi/mcp/sub"));

    // Given an unknown provider REST path, Then the unknown-path kind applies
    assert_eq!(
        classify_token_request_kind("/api/serpapi/nope", None).key,
        "api:unknown-path"
    );

    // Given Tavily paths, Then classification stays byte-identical
    assert_eq!(
        classify_token_request_kind("/api/tavily/search", None).key,
        "api:search"
    );
    assert_eq!(
        classify_token_request_kind("/mcp", Some(&tools_call_body("tavily-search"))).key,
        "mcp:search"
    );
    assert_eq!(
        classify_token_request_kind("/mcp", Some(&tools_call_body("search"))).key,
        "mcp:third-party-tool"
    );
}

#[tokio::test]
async fn provider_sql_surfaces_execute_against_sqlite() {
    // Given every generated SQL surface with provider kinds embedded, When
    // executed against SQLite, Then the statements parse and evaluate to the
    // canonical outputs (guards the alert-CTE composition, not just literals)
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory pool");
    let label: String = sqlx::query_scalar(&format!(
        "SELECT {} AS v",
        canonical_request_kind_label_sql("'mcp:exa/answer'")
    ))
    .fetch_one(&pool)
    .await
    .expect("label sql executes");
    assert_eq!(label, "MCP | exa/answer");
    // Executed-SQL pin: the humanization row renders EXACTLY the catalog labels
    for (key, expected) in PROVIDER_REQUEST_KINDS {
        let rendered: String = sqlx::query_scalar(&format!(
            "SELECT {} AS v",
            canonical_request_kind_label_sql(&format!("'{key}'"))
        ))
        .fetch_one(&pool)
        .await
        .expect("per-key label sql executes");
        assert_eq!(&rendered, expected, "sql label {key}");
    }
    let stored: String = sqlx::query_scalar(&format!(
        "SELECT {} AS v",
        token_log_request_kind_key_sql("'/api/exa/search'", "'api:exa/search'")
    ))
    .fetch_one(&pool)
    .await
    .expect("stored-kind sql executes");
    assert_eq!(stored, "api:exa/search");
    let bucket: String = sqlx::query_scalar(&format!(
        "SELECT {} AS v",
        request_value_bucket_sql("'mcp:brave/web_search'", "NULL")
    ))
    .fetch_one(&pool)
    .await
    .expect("bucket sql executes");
    assert_eq!(bucket, "valuable");
    let pred: i64 = sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM (SELECT 1) WHERE {}",
        canonical_request_kind_stored_predicate_sql("'api:brave/web'")
    ))
    .fetch_one(&pool)
    .await
    .expect("predicate sql executes");
    assert_eq!(pred, 1);
    let passthrough: String = sqlx::query_scalar(&format!(
        "SELECT {} AS v",
        request_log_request_kind_key_sql("'/api/brave/news/search'", "NULL", "'api:brave/news'")
    ))
    .fetch_one(&pool)
    .await
    .expect("request-log sql executes");
    assert_eq!(passthrough, "api:brave/news");
}

#[test]
fn provider_mcp_ingress_paths_are_pinned() {
    // Given/When/Then: the ingress table the routing todos (11/15-17) mount
    assert_eq!(provider_mcp_ingress_path(ProviderId::Tavily), "/mcp");
    assert_eq!(
        provider_mcp_ingress_path(ProviderId::Serpapi),
        "/serpapi/mcp"
    );
    assert_eq!(provider_mcp_ingress_path(ProviderId::Exa), "/exa/mcp");
    assert_eq!(provider_mcp_ingress_path(ProviderId::Brave), "/brave/mcp");
}

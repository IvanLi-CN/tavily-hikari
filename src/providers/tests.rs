use std::str::FromStr;

use crate::providers::{
    BRAVE, EXA, KeyAuthInjection, McpMode, ProviderId, RecoveryPolicy, SERPAPI, TAVILY,
    UnknownProviderId,
};

/// Display, the SQLTEXT helper `as_str`, and FromStr must agree on one stable
/// lowercase key per provider (the DB/wire spelling).
#[test]
fn provider_id_display_from_str_roundtrip_all_four() {
    // Given: every provider and its canonical lowercase key.
    let cases = [
        (ProviderId::Tavily, "tavily"),
        (ProviderId::Serpapi, "serpapi"),
        (ProviderId::Exa, "exa"),
        (ProviderId::Brave, "brave"),
    ];
    for (id, key) in cases {
        // When/Then: Display and as_str emit the stable key; FromStr recovers
        // the same variant.
        assert_eq!(id.to_string(), key);
        assert_eq!(id.as_str(), key);
        assert_eq!(ProviderId::from_str(key), Ok(id));
    }
}

/// Failure path: an unknown provider string yields a TYPED error carrying the
/// input — never a panic.
#[test]
fn provider_id_from_str_unknown_returns_typed_error() {
    let err = ProviderId::from_str("openai").expect_err("openai must not be a provider");
    assert_eq!(
        err,
        UnknownProviderId {
            input: "openai".to_owned()
        }
    );
    assert!(err.to_string().contains("openai"));
}

#[test]
fn provider_id_serde_uses_lowercase_renames() {
    let cases = [
        (ProviderId::Tavily, "tavily"),
        (ProviderId::Serpapi, "serpapi"),
        (ProviderId::Exa, "exa"),
        (ProviderId::Brave, "brave"),
    ];
    for (id, key) in cases {
        let json = serde_json::to_string(&id).expect("provider id serializes");
        assert_eq!(json, format!("\"{key}\""));
        let parsed = serde_json::from_str::<ProviderId>(&json).expect("rename deserializes");
        assert_eq!(parsed, id);
    }
}

/// Negative serde test: unknown strings are rejected at the boundary.
#[test]
fn provider_id_serde_rejects_unknown_variant() {
    let err = serde_json::from_str::<ProviderId>("\"openai\"")
        .expect_err("unknown provider string must be rejected");
    assert!(format!("{err}").contains("openai"));
}

#[test]
fn tavily_spec_mirrors_current_constants_metadata_only() {
    // The registry is METADATA ONLY: mcp_upstream_default must equal the live
    // crate constant so the two can never drift.
    assert_eq!(TAVILY.id, ProviderId::Tavily);
    assert_eq!(TAVILY.rest_base_default, "https://api.tavily.com");
    assert_eq!(TAVILY.mcp_upstream_default, Some(crate::DEFAULT_UPSTREAM));
    // Tavily keeps its existing dual-mode (passthrough/rebalance) machinery;
    // RebalanceLegacy marks "do not touch" for later provider-aware ingress.
    assert_eq!(TAVILY.mcp_mode, McpMode::RebalanceLegacy);
    assert_eq!(
        TAVILY.upstream_key_injection,
        KeyAuthInjection::QueryParam {
            name: "tavilyApiKey"
        }
    );
    assert_eq!(TAVILY.client_auth_query_slot, Some("tavilyApiKey"));
    // Tavily-Api-Key is an UPSTREAM-only header: the Tavily request path stays
    // byte-stable, so there is no client-facing header slot.
    assert_eq!(TAVILY.client_auth_header_slot, None);
    const { assert!(TAVILY.supports_quota_sync) };
    assert_eq!(TAVILY.recovery, RecoveryPolicy::UtcMonth);
    // DOCUMENTED CHOICE: the plan leaves Tavily credits unspecified ("metadata
    // only"), so the safest literal `tavily_search = 1` is encoded and every
    // other op is 0 here. REAL Tavily billing semantics stay owned by the
    // existing machinery in handlers/tavily.rs and are NOT rewired.
    assert_eq!((TAVILY.credits)("search"), 1);
    assert_eq!((TAVILY.credits)("extract"), 0);
    assert_eq!((TAVILY.credits)("crawl"), 0);
    assert_eq!((TAVILY.credits)("map"), 0);
    assert_eq!((TAVILY.credits)("research"), 0);
    assert_eq!((TAVILY.credits)("usage"), 0);
}

#[test]
fn serpapi_spec_defaults_and_credits() {
    assert_eq!(SERPAPI.id, ProviderId::Serpapi);
    assert_eq!(SERPAPI.rest_base_default, "https://serpapi.com");
    assert_eq!(
        SERPAPI.mcp_upstream_default,
        Some("https://mcp.serpapi.com/mcp")
    );
    assert_eq!(SERPAPI.mcp_mode, McpMode::Passthrough);
    assert_eq!(SERPAPI.upstream_key_injection, KeyAuthInjection::Bearer);
    assert_eq!(SERPAPI.client_auth_query_slot, Some("api_key"));
    assert_eq!(SERPAPI.client_auth_header_slot, None);
    const { assert!(SERPAPI.supports_quota_sync) };
    assert_eq!(SERPAPI.recovery, RecoveryPolicy::RenewalDate);
    // Only successful searches are billable; /account stays free and uncounted.
    assert_eq!((SERPAPI.credits)("search"), 1);
    assert_eq!((SERPAPI.credits)("account"), 0);
}

#[test]
fn exa_spec_defaults_and_credits() {
    assert_eq!(EXA.id, ProviderId::Exa);
    assert_eq!(EXA.rest_base_default, "https://api.exa.ai");
    // Hosted MCP passthrough is deferred (ADR 0005) — local facade only.
    assert_eq!(EXA.mcp_upstream_default, None);
    assert_eq!(EXA.mcp_mode, McpMode::LocalFacade);
    assert_eq!(
        EXA.upstream_key_injection,
        KeyAuthInjection::Header { name: "x-api-key" }
    );
    assert_eq!(EXA.client_auth_query_slot, None);
    assert_eq!(EXA.client_auth_header_slot, Some("x-api-key"));
    // No live remaining-balance API exists (ADR 0005) — local counting only.
    const { assert!(!EXA.supports_quota_sync) };
    assert_eq!(EXA.recovery, RecoveryPolicy::SuccessOnly);
    assert_eq!((EXA.credits)("search"), 1);
    assert_eq!((EXA.credits)("contents"), 1);
    assert_eq!((EXA.credits)("answer"), 2);
    assert_eq!((EXA.credits)("unknown"), 0);
}

#[test]
fn brave_spec_defaults_and_credits() {
    assert_eq!(BRAVE.id, ProviderId::Brave);
    assert_eq!(BRAVE.rest_base_default, "https://api.search.brave.com");
    // Brave hosts no MCP server (ADR 0005) — local facade only.
    assert_eq!(BRAVE.mcp_upstream_default, None);
    assert_eq!(BRAVE.mcp_mode, McpMode::LocalFacade);
    assert_eq!(
        BRAVE.upstream_key_injection,
        KeyAuthInjection::Header {
            name: "X-Subscription-Token"
        }
    );
    assert_eq!(BRAVE.client_auth_query_slot, None);
    assert_eq!(BRAVE.client_auth_header_slot, Some("X-Subscription-Token"));
    // Quota sync is supported PASSIVELY via X-RateLimit-* header harvest
    // (zero extra requests); recovery follows the month reset window.
    const { assert!(BRAVE.supports_quota_sync) };
    assert_eq!(BRAVE.recovery, RecoveryPolicy::RateLimitResetWindow);
    assert_eq!((BRAVE.credits)("web"), 1);
    assert_eq!((BRAVE.credits)("news"), 1);
    assert_eq!((BRAVE.credits)("llm_context"), 2);
    assert_eq!((BRAVE.credits)("images"), 0);
}

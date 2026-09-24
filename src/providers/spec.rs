use super::id::ProviderId;

/// How a pooled upstream key is attached to outbound provider requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAuthInjection {
    /// Query-parameter injection carrying the parameter name (SerpApi `api_key`;
    /// Tavily's MCP forward uses `tavilyApiKey`).
    QueryParam { name: &'static str },
    /// Header injection carrying the header name (Exa, Brave).
    Header { name: &'static str },
    /// `Authorization: Bearer <key>` injection (hosted SerpApi MCP).
    Bearer,
}

/// How the provider's MCP surface is served by Hikari.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpMode {
    /// Byte-passthrough to a provider-hosted MCP server (SerpApi).
    Passthrough,
    /// Locally built MCP facade translating tools/call to provider REST
    /// (Exa, Brave — ADR 0005).
    LocalFacade,
    /// Tavily's existing passthrough/rebalance dual-mode machinery. Recorded
    /// for completeness; the registry MUST NOT rewire it (byte-stable rule).
    RebalanceLegacy,
}

/// How an `exhausted` key returns to the active pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPolicy {
    /// Exhaustion clears at UTC month rollover (Tavily today).
    UtcMonth,
    /// Exhaustion clears at/after the per-key renewal date (SerpApi
    /// `plan_renewal_date` from the /account sync).
    RenewalDate,
    /// Exhaustion clears only on the next successful call or manual restore
    /// (Exa — prepaid credits, no calendar reset).
    SuccessOnly,
    /// Exhaustion clears per the month window advertised by `X-RateLimit-Reset`
    /// (Brave passive harvest).
    RateLimitResetWindow,
}

/// Metadata-only description of one provider (ADR 0005).
///
/// Recording a value here never rewires an existing flow: the Tavily entry
/// mirrors constants the live request path already hardcodes, and the new
/// providers bind to it in later todos (classification, header policy, config,
/// routing).
///
/// Intentionally no `PartialEq`/`Eq` derive: the `credits` field is a function
/// pointer, and comparing function pointers is unreliable across codegen units
/// (rustc `unpredictable_function_pointer_comparisons`). Compare fields.
#[derive(Debug, Clone, Copy)]
pub struct ProviderSpec {
    pub id: ProviderId,
    pub rest_base_default: &'static str,
    pub mcp_upstream_default: Option<&'static str>,
    pub mcp_mode: McpMode,
    pub upstream_key_injection: KeyAuthInjection,
    pub client_auth_query_slot: Option<&'static str>,
    pub client_auth_header_slot: Option<&'static str>,
    pub credits: fn(op: &str) -> u32,
    pub supports_quota_sync: bool,
    pub recovery: RecoveryPolicy,
}

/// DOCUMENTED CHOICE: plan todo 2 leaves Tavily's credit table unspecified
/// ("metadata only"), so the safest literal is encoded — `search = 1`, every
/// other op unbilled here. REAL Tavily billing semantics stay owned by the
/// existing machinery (handlers/tavily.rs) and are NOT rewired by this table.
fn tavily_credits(op: &str) -> u32 {
    match op {
        "search" => 1,
        _ => 0,
    }
}

/// SerpApi: only successful searches are billable; /account is free.
fn serpapi_credits(op: &str) -> u32 {
    match op {
        "search" => 1,
        _ => 0,
    }
}

/// Exa per-op credits (ADR 0005): search 1, contents 1, answer 2.
fn exa_credits(op: &str) -> u32 {
    match op {
        "search" | "contents" => 1,
        "answer" => 2,
        _ => 0,
    }
}

/// Brave per-op credits (ADR 0005): web 1, news 1, llm_context 2.
fn brave_credits(op: &str) -> u32 {
    match op {
        "web" | "news" => 1,
        "llm_context" => 2,
        _ => 0,
    }
}

/// Tavily — mirrors today's constants as METADATA ONLY.
pub const TAVILY: ProviderSpec = ProviderSpec {
    id: ProviderId::Tavily,
    rest_base_default: "https://api.tavily.com",
    // Must equal crate::DEFAULT_UPSTREAM (lib.rs); asserted in tests so the
    // registry and the live constant can never drift apart.
    mcp_upstream_default: Some("https://mcp.tavily.com/mcp"),
    mcp_mode: McpMode::RebalanceLegacy,
    upstream_key_injection: KeyAuthInjection::QueryParam {
        name: "tavilyApiKey",
    },
    client_auth_query_slot: Some("tavilyApiKey"),
    // Tavily-Api-Key is an UPSTREAM-only header: the Tavily request path stays
    // byte-stable, so no client-facing header slot is exposed here.
    client_auth_header_slot: None,
    credits: tavily_credits,
    supports_quota_sync: true,
    recovery: RecoveryPolicy::UtcMonth,
};

/// SerpApi — hosted MCP passthrough with Bearer injection (ADR 0005).
pub const SERPAPI: ProviderSpec = ProviderSpec {
    id: ProviderId::Serpapi,
    rest_base_default: "https://serpapi.com",
    mcp_upstream_default: Some("https://mcp.serpapi.com/mcp"),
    mcp_mode: McpMode::Passthrough,
    upstream_key_injection: KeyAuthInjection::Bearer,
    // Client-facing facade slot: the Hikari th- token may ride the native
    // `api_key` query parameter; upstream key injection is Bearer-based.
    client_auth_query_slot: Some("api_key"),
    client_auth_header_slot: None,
    credits: serpapi_credits,
    supports_quota_sync: true,
    recovery: RecoveryPolicy::RenewalDate,
};

/// Exa — local MCP facade only; hosted passthrough deferred (ADR 0005).
pub const EXA: ProviderSpec = ProviderSpec {
    id: ProviderId::Exa,
    rest_base_default: "https://api.exa.ai",
    mcp_upstream_default: None,
    mcp_mode: McpMode::LocalFacade,
    upstream_key_injection: KeyAuthInjection::Header { name: "x-api-key" },
    client_auth_query_slot: None,
    client_auth_header_slot: Some("x-api-key"),
    credits: exa_credits,
    // No live remaining-balance API exists — local counting only.
    supports_quota_sync: false,
    recovery: RecoveryPolicy::SuccessOnly,
};

/// Brave — local MCP facade (Brave hosts no MCP server, ADR 0005).
pub const BRAVE: ProviderSpec = ProviderSpec {
    id: ProviderId::Brave,
    rest_base_default: "https://api.search.brave.com",
    mcp_upstream_default: None,
    mcp_mode: McpMode::LocalFacade,
    upstream_key_injection: KeyAuthInjection::Header {
        name: "X-Subscription-Token",
    },
    client_auth_query_slot: None,
    client_auth_header_slot: Some("X-Subscription-Token"),
    credits: brave_credits,
    // Passive harvest of X-RateLimit-* month-window headers from every
    // response — zero extra requests.
    supports_quota_sync: true,
    recovery: RecoveryPolicy::RateLimitResetWindow,
};

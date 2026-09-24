# ADR 0005: Multi-Provider Upstream Abstraction

## Status

Accepted

## Context

Tavily Hikari multiplexes Tavily keys behind one MCP ingress and one native HTTP facade. The
product will now stand three more search providers beside Tavily: SerpApi, Exa, and Brave Search.
Their contracts differ in the exact places the current code assumes Tavily, so a single hard-coded
provider can no longer describe ingress, key injection, health, quota, and billing.

SerpApi authenticates REST calls with the `api_key` query parameter, not a header. `GET /account`
is documented free and uncounted and returns `plan_searches_left`, `total_searches_left`,
`searches_per_month`, and `plan_renewal_date`, which mirrors the existing Tavily usage sync. Its
`429` is dual-semantic: monthly exhaustion arrives as `"Your account has run out of searches."`,
while a throughput `429` uses a different message and is transient. SerpApi hosts an MCP server at
`mcp.serpapi.com` that accepts `Authorization: Bearer <key>`, and its `search`, `search_table`, and
`search_dashboard` tool names collide with Tavily's canonical classifier names. Its terms
restrict resale and multi-account patterns (sections 2 and 6).

Exa authenticates with `x-api-key` (or Bearer) against `api.exa.ai`. Exhaustion is a `402` whose
tag is `NO_MORE_CREDITS`, `API_KEY_BUDGET_EXCEEDED`, or `TEAM_BUDGET_EXCEEDED`. Exa exposes no live
remaining-balance API: only a separate team-management usage history host, gated per team, with no
"credits left" read. Its hosted MCP at `mcp.exa.ai` is Cloudflare-fronted, and a direct POST was
observed as `"Sorry, you have been blocked"`; its server also reports analytics to a third party
(`api.agnost.ai`). That analytics flow conflicts with this product's high-anonymity posture. Exa's
license is non-transferable and non-sublicensable per authorized user.

Brave authenticates with the `X-Subscription-Token` header against `api.search.brave.com/res/v1`.
An invalid key returns HTTP `422` with `SUBSCRIPTION_TOKEN_INVALID`, not `401` (live-probed);
quota exhaustion returns `402` with `QUOTA_LIMITED`. Brave publishes `X-RateLimit-*` headers that
cover both a second window and the month window (`w=2592000`), so the monthly remaining amount can
be read from any response without a usage endpoint. Brave hosts no MCP server; its official MCP is
self-hosted only. Its terms prohibit retaining search-result data and limit key sharing to
affiliates and contractors who need to know (section 3(b)(v)/(xii), section 4(a)).

All three providers' terms restrict reselling, key pooling, and multi-account rotation while this
product resells metered access through tokens and Linux.do credit. The differences above are real
seams, not cosmetic naming: URL building, key injection, the exhaustion gate, the tool namespace,
and the usage-sync source. The decision below records how those seams are absorbed without moving
Tavily behavior. Substantive terms are recorded in `CONTEXT.md` under Multi-Provider Upstream Terms.

## Decision

- Add one additive registry of `ProviderSpec` entries keyed by `ProviderId` (`Tavily`, `Serpapi`,
  `Exa`, `Brave`). Each spec carries the REST base default, the hosted MCP upstream and MCP mode,
  the upstream key-injection style, the client auth slots, the per-operation credit table, the
  quota-sync capability, and the recovery policy. The Tavily spec records today's constants as
  metadata only; it never rewires the existing Tavily request path, allowlists, or defaults.
- Route by path prefix. `/serpapi/mcp` is a passthrough to the hosted SerpApi MCP with Bearer
  injection and session-id translation. `/exa/mcp` and `/brave/mcp` are local MCP facades. Native
  HTTP facades are `/api/serpapi/search`, `/api/exa/{search,contents,answer}`, and
  `/api/brave/res/v1/{web/search,news/search,llm/context}`. Every facade accepts the Hikari `th-`
  token in the provider-native auth slot and as `Authorization: Bearer`.
- Keep one shared credit wallet per token and add a per-provider credit table for billed
  operations: serpapi search `1`; exa search `1`, contents `1`, answer `2`; brave web `1`, news
  `1`, llm_context `2`. Initialize, ping, tools/list, and resources remain non-billable.
- Scope the key pool by provider. Acquire, affinity, transient backoff, quarantine, and health
  transitions never lease a key across providers; a request only ever uses a key for the provider
  it addresses.
- Classify exhaustion per provider instead of assuming the Tavily `432`. SerpApi `401`/`403` are
  auth failures, a `429` carrying `"run out of searches"` is exhausted with renewal-date recovery,
  and any other `429` is an hour-scoped transient backoff. Exa `401` is an auth failure, a `402`
  tagged `NO_MORE_CREDITS`, `API_KEY_BUDGET_EXCEEDED`, or `TEAM_BUDGET_EXCEEDED` is exhausted with
  success-restore or manual recovery, and `429` honors `Retry-After`. Brave `422`
  `SUBSCRIPTION_TOKEN_INVALID` is an auth failure, `402 QUOTA_LIMITED` is exhausted on the month
  window, and `429` is a short backoff. Low-quota depletion stays Tavily-only.
- Keep segmented reconciliation, the `accessToken` HMAC pseudonym, and `X-Project-ID` injection
  Tavily-only. No third-party upstream ever receives the pseudonym, and reconciliation and
  research-drain jobs stay guarded to Tavily keys.
- Split the migration. One main-ledger migration `v45` owns the `provider` columns for `api_keys`,
  `auth_token_logs`, and `mcp_sessions`. A separate observability-sidecar ALTER path adds
  `request_logs.provider` idempotently. The two mechanisms are never conflated.
- Sync quota from what each provider offers. SerpApi uses an active `/account` adapter inside the
  existing sync lanes. Brave uses a passive header harvest. Exa uses none, because no live balance
  read exists.

## Consequences

- Exa hosted-MCP passthrough is deferred. The local facade is the only Exa MCP surface for now:
  the hosted server's Cloudflare obstruction was observed directly, and its third-party analytics
  conflict with the high-anonymity posture. The deferral is recorded here so a later decision
  reopens it explicitly.
- Brave's data-retention prohibition is an operator-policy note only. The existing body-retention
  profiles are the lever; no per-provider retention code is added.
- All three providers need an operator legal note on resale and pooling: SerpApi sections 2 and 6,
  Exa's non-transferable license, and Brave section 3(b)(v)/(xii) plus section 4(a). Pooling one
  organization's own keys within purchased quota is the documented boundary.
- Tavily stays byte-stable. The golden Tavily suites, the three existing header allowlists, and the
  existing env defaults must pass unchanged except for the authorized schema-inventory vector.
- New providers are proven against stub upstreams only. The existing "never hit production
  endpoints" rule extends to SerpApi, Exa, and Brave.
- Provider logic lives in ProviderSpec and classifier tables, not scattered conditionals, and new
  files stay inside the repository source-line budgets.

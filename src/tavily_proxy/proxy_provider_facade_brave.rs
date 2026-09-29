//! Brave REST facade execution layer (plan todo 14; amendments M2/M8/M11/M17/M22).
//!
//! Layered over the generic todo-11 seam (`proxy_provider_rest_facade`):
//! * upstream-response classification via the committed todo-3 brave
//!   classifier, projected into the shared `AttemptAnalysis` contract so the
//!   EXISTING `reconcile_key_health` machinery applies the acceptance health
//!   actions (422 SUBSCRIPTION_TOKEN_INVALID / VALIDATION-flagged-token
//!   auth-fail → quarantine row, 402 QUOTA_LIMITED → exhausted, 429
//!   RATE_LIMITED → transient backoff on the shared HTTP scope sized by the
//!   committed second-scale Brave ladder);
//! * request_logs persistence (provider stamp, legacy `tavily_status_code`
//!   column carries the upstream status, bodies per retention profile with
//!   `redact_api_key_bytes` secret redaction — the pooled X-Subscription-Token
//!   and th- tokens are never persisted);
//! * M11 PASSIVE QUOTA HARVEST: every response (success AND error) has its
//!   X-RateLimit-* month window parsed by POSITIONAL pairing and, only on a
//!   material change, persisted through the exact M11 seam
//!   (`KeyStore::record_quota_sync_sample`) with the distinct source
//!   `brave_passive`. The harvest adds ZERO outbound requests (it reads the
//!   response headers the upstream call already buffered) and never fails the
//!   request (best-effort, contention-skipped, never holds the response —
//!   CONTEXT.md foreground-work discipline);
//! * a deterministic bin-side outcome projection (`brave_facade_outcome`) so
//!   the server boundary can drive reserve→settle billing with the credit
//!   tiers (web=1, news=1, llm_context=2) from the committed todo-2
//!   ProviderSpec table without re-implementing the classifier.
//!
//! Boundary notes: RateLimitResetWindow recovery of exhausted Brave keys is
//! implemented by T9's provider-scoped `reset_monthly` (todo 9) and the health
//! signals/alerts wiring depth stays todo 18 — this layer only emits the
//! signals and applies the reconcile actions, exactly as SerpApi/Exa do. The
//! harvested X-RateLimit-Reset month window is parsed to exercise positional
//! pairing but is NOT re-wired into recovery here (todo-18 owns that seam).
//!
//! SEAM LIMITATION (flagged, not silently widened): M11 wants a month-window
//! Limit of `0` persisted as `quota_limit NULL` (NEVER 0). The named
//! `record_quota_sync_sample` seam binds `limit` as `i64` and the
//! `api_key_quota_sync_samples.quota_limit` column is `NOT NULL`, so NULL is
//! not expressible through it without a store change (FORBIDDEN without
//! STOP+report). The harvest therefore maps `0 → unlimited → skip-write`,
//! which never persists `0` and leaves a fresh key's already-NULL
//! `quota_limit` intact — honoring the "NEVER 0" invariant for every reachable
//! case (Brave plan limits and mock_brave are always `> 0`).

use super::*;

/// M11 passive-harvest source stamp (DISTINCT value; never reused by the
/// Tavily/SerpApi sync jobs so the sample table stays auditable per source).
const BRAVE_PASSIVE_QUOTA_SYNC_SOURCE: &str = "brave_passive";

/// Brave month window in seconds (`w=2592000` = 30 days) — the window that
/// carries the plan's monthly request budget; the `w=1` window is the
/// per-second rate limit and is never harvested as quota.
const BRAVE_MONTH_WINDOW_SECS: i64 = 2_592_000;

/// Deterministic projection of one Brave facade response onto the token-log
/// vocabulary (result status + failure kind). Exported for the bin boundary
/// where the classifier internals are crate-private.
#[derive(Debug, Clone, Copy)]
pub struct BraveFacadeOutcome {
    /// `success` / `quota_exhausted` / `error` (shared OUTCOME_* vocabulary).
    pub result_status: &'static str,
    pub failure_kind: Option<&'static str>,
}

/// Classify a buffered Brave upstream response (todo-3 classifier) without
/// header evidence; the 422-code/402/429 split never depends on headers for
/// the outcome vocabulary, only Retry-After sizing (handled lib-side where the
/// response headers are live).
pub fn brave_facade_outcome(status: StatusCode, body: &[u8]) -> BraveFacadeOutcome {
    let signal = classify_provider_failure(
        providers::ProviderId::Brave,
        &ProviderFailureEvidence {
            status: status.as_u16(),
            headers: None,
            body,
        },
    );
    outcome_from_signal(signal)
}

fn outcome_from_signal(signal: ProviderFailureSignal) -> BraveFacadeOutcome {
    let result_status = match signal {
        ProviderFailureSignal::Ok => OUTCOME_SUCCESS,
        ProviderFailureSignal::Exhausted => OUTCOME_QUOTA_EXHAUSTED,
        ProviderFailureSignal::AuthFailed
        | ProviderFailureSignal::RateLimited { .. }
        | ProviderFailureSignal::ClientError
        | ProviderFailureSignal::ServerTransient => OUTCOME_ERROR,
    };
    BraveFacadeOutcome {
        result_status,
        failure_kind: signal.failure_kind(),
    }
}

fn brave_key_health_action(
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> KeyHealthAction {
    match signal {
        // 402 QUOTA_LIMITED exhausts the key. Brave recovery is
        // RateLimitResetWindow (todo-2 registry): the exhausted→active flip is
        // OWNED by T9's provider-scoped `reset_monthly` — NOT duplicated here.
        ProviderFailureSignal::Exhausted => KeyHealthAction::MarkExhausted,
        // 422 SUBSCRIPTION_TOKEN_INVALID (and VALIDATION whose meta.errors loc
        // is the x-subscription-token header) auth-fails into the EXISTING
        // quarantine machinery (mirrors SerpApi/Exa invalid-key behavior);
        // provider-tagged alert payloads and recovery policy land with todo 18.
        // M8: quarantine/exhaustion for new providers never writes
        // api_key_low_quota_depletions.
        ProviderFailureSignal::AuthFailed => KeyHealthAction::Quarantine(QuarantineDecision {
            reason_code: BLOCKED_KEY_REASON_INVALID_API_KEY.to_string(),
            reason_summary: format!(
                "Brave rejected the pooled subscription token as invalid (HTTP {})",
                status.as_u16()
            ),
            reason_detail: truncate_text(
                String::from_utf8_lossy(body).trim(),
                QUARANTINE_REASON_DETAIL_MAX_LEN,
            ),
        }),
        ProviderFailureSignal::Ok
        | ProviderFailureSignal::RateLimited { .. }
        | ProviderFailureSignal::ClientError
        | ProviderFailureSignal::ServerTransient => KeyHealthAction::None,
    }
}

fn brave_attempt_analysis(
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> AttemptAnalysis {
    let outcome = outcome_from_signal(signal);
    AttemptAnalysis {
        status: outcome.result_status,
        // The legacy column carries the upstream HTTP status verbatim for
        // provider rows (plan todo 14 logging contract).
        tavily_status_code: Some(i64::from(status.as_u16())),
        key_health_action: brave_key_health_action(status, body, signal),
        failure_kind: outcome.failure_kind.map(str::to_string),
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

/// One parsed Brave month window (M11). `limit == None` means the month window
/// advertised `0` (unlimited) which is NEVER persisted as `0` (see module
/// docs); `reset` is the X-RateLimit-Reset month-window value, parsed to prove
/// positional pairing but owned by todo-18 recovery (not re-wired here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BraveMonthWindow {
    limit: Option<i64>,
    remaining: i64,
    reset: Option<i64>,
}

/// Split a comma-delimited positional X-RateLimit-* list into trimmed tokens.
fn split_positional_list(raw: Option<&str>) -> Vec<String> {
    match raw {
        Some(value) => value
            .split(',')
            .map(|part| part.trim().to_string())
            .collect(),
        None => Vec::new(),
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// Locate the POSITIONAL index of the month window (`w=2592000`) inside the
/// X-RateLimit-Policy grammar (`1;w=1, 1000;w=2592000`). The index pairs the
/// X-RateLimit-Limit/Remaining/Reset lists position-for-position. A `w=0` or
/// an unparseable window never matches the month window and is skipped.
fn brave_month_window_index(policy: &str) -> Option<usize> {
    for (index, spec) in policy.split(',').enumerate() {
        for part in spec.split(';') {
            let part = part.trim();
            if let Some(window) = part.strip_prefix("w=")
                && window.trim().parse::<i64>().ok() == Some(BRAVE_MONTH_WINDOW_SECS)
            {
                return Some(index);
            }
        }
    }
    None
}

/// Parse the Brave month window from response headers (M11). Grammar is
/// POSITIONAL: the policy defines window order, and Limit/Remaining/Reset are
/// comma lists paired by that order. Returns None when the policy is absent,
/// has no `w=2592000` month window, a paired list is missing/too short, or the
/// month tokens are unparseable — every edge silently yields None (no harvest).
fn parse_brave_rate_limit_month_window(headers: &HeaderMap) -> Option<BraveMonthWindow> {
    let policy = header_str(headers, "x-ratelimit-policy")?;
    let month_index = brave_month_window_index(policy)?;

    let limits = split_positional_list(header_str(headers, "x-ratelimit-limit"));
    let remainings = split_positional_list(header_str(headers, "x-ratelimit-remaining"));
    let resets = split_positional_list(header_str(headers, "x-ratelimit-reset"));

    // Remaining is mandatory and positional; a short/missing list yields None.
    let remaining = remainings.get(month_index)?.trim().parse::<i64>().ok()?;
    // Limit is mandatory and positional; `0` means unlimited (None), never 0.
    let limit_raw = limits.get(month_index)?.trim().parse::<i64>().ok()?;
    let limit = match limit_raw {
        0 => None,
        positive => Some(positive),
    };
    // Reset is optional (todo-18 owns recovery); positional when present.
    let reset = resets
        .get(month_index)
        .and_then(|value| value.trim().parse::<i64>().ok());

    Some(BraveMonthWindow {
        limit,
        remaining,
        reset,
    })
}

/// The material-change decision (M11: persist ONLY on exhaustion transition,
/// threshold crossing, or reset-window change). `Skip` for: no month window,
/// an unlimited (`0`) month Limit (never writable as NULL through the i64
/// seam, and 0 is NEVER persisted), or a non-material repeat identical to the
/// stored quota. Pure/side-effect-free for exhaustive unit testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BraveHarvestDecision {
    Skip,
    Write { limit: i64, remaining: i64 },
}

fn decide_brave_harvest(
    window: Option<BraveMonthWindow>,
    stored_limit: Option<i64>,
    stored_remaining: Option<i64>,
) -> BraveHarvestDecision {
    let Some(window) = window else {
        return BraveHarvestDecision::Skip;
    };
    let Some(limit) = window.limit else {
        // Month Limit `0` = unlimited: NEVER write 0; leave the (already NULL)
        // quota_limit intact. See module docs for the exact seam limitation.
        return BraveHarvestDecision::Skip;
    };
    if stored_limit == Some(limit) && stored_remaining == Some(window.remaining) {
        // Non-material repeat (bounded-write proof): identical quota, no row.
        return BraveHarvestDecision::Skip;
    }
    BraveHarvestDecision::Write {
        limit,
        remaining: window.remaining,
    }
}

impl TavilyProxy {
    /// Post-response bookkeeping for the Brave REST facade (todo-14 mirror of
    /// `record_exa_rest_facade_attempt` + M11 passive harvest): reconcile,
    /// second-scale transient backoff arming, request_logs persistence,
    /// backoff/log linking, and the best-effort month-window quota harvest.
    /// The SerpApi/Exa record paths stay byte-identical behind their own
    /// gates; Tavily REST never reaches this seam.
    ///
    /// `response_headers` are the live upstream response headers (already
    /// buffered by the executor for the verbatim passthrough), so the harvest
    /// adds ZERO extra outbound requests.
    pub(crate) async fn record_brave_rest_facade_attempt(
        &self,
        ctx: &ProviderFacadeAttemptContext<'_>,
        response_headers: &HeaderMap,
    ) -> Result<ProviderFacadeAttemptRecord, ProxyError> {
        if ctx.request.provider != providers::ProviderId::Brave {
            return Ok(ProviderFacadeAttemptRecord::boundary_default());
        }

        let analysis = brave_attempt_analysis(ctx.status, ctx.body_bytes, ctx.signal);
        let mut key_effect = self
            .reconcile_key_health(
                ctx.lease,
                ctx.request.path.as_str(),
                &analysis,
                ctx.request.auth_token_id.as_deref(),
            )
            .await?;
        if key_effect.code == KEY_EFFECT_NONE && analysis.status == OUTCOME_SUCCESS {
            key_effect = self
                .clear_transient_backoffs_after_success(
                    &ctx.lease.id,
                    ctx.request.path.as_str(),
                    ctx.request.auth_token_id.as_deref(),
                )
                .await?;
        }

        // 429 RATE_LIMITED (todo-3 RateLimited — Brave has no exhaustion-flavored
        // 429): arm the shared HTTP backoff scope that provider-scoped
        // acquisition avoids. Sizing uses the committed todo-3 SECOND-scale
        // Brave ladder (default 2s, 1..=30 clamp), distinct from Exa's
        // minute-scale floor and SerpApi's hour-scale envelope; todo 18 keeps
        // the second-scale sizing on the shared golden-pinned scope (D1).
        let armed_backoff = match ctx.signal {
            ProviderFailureSignal::RateLimited { .. } => {
                let now = self.backend_time.now_ts();
                match Self::effective_provider_rate_limited_backoff_secs(
                    ctx.request.provider,
                    ctx.signal,
                    now,
                ) {
                    Some(retry_after_secs) => self
                        .key_store
                        .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                            key_id: &ctx.lease.id,
                            scope: HTTP_GLOBAL_BACKOFF_SCOPE,
                            cooldown_until: now + retry_after_secs,
                            retry_after_secs,
                            reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
                            source_request_log_id: None,
                            now,
                        })
                        .await?
                        .is_some(),
                    // Unreachable for RateLimited; keep the match exhaustive
                    // without an escape hatch.
                    None => false,
                }
            }
            ProviderFailureSignal::Ok
            | ProviderFailureSignal::Exhausted
            | ProviderFailureSignal::AuthFailed
            | ProviderFailureSignal::ClientError
            | ProviderFailureSignal::ServerTransient => false,
        };
        if armed_backoff && key_effect.code == KEY_EFFECT_NONE {
            key_effect = Self::transient_backoff_set_effect();
        }

        // Secrets redacted before persistence (M17): the pooled key rides the
        // X-Subscription-Token header only and the client slot was stripped at
        // the boundary, so the persisted bodies/query never carry th- tokens or
        // provider keys; the redaction pass stays as the defense-in-depth
        // backstop. GET bodies are empty, so request_body is trivially clean.
        let redacted_request_body = redact_api_key_bytes(&ctx.request.body);
        let redacted_response_body = redact_api_key_bytes(ctx.body_bytes);
        let request_log_id = self
            .key_store
            .log_attempt(AttemptLog {
                key_id: Some(&ctx.lease.id),
                auth_token_id: ctx.request.auth_token_id.as_deref(),
                method: &ctx.request.method,
                // Brave rows key on the CANONICAL kind path (todo-3 static
                // table entry), not the full registered `/res/v1` mirror: the
                // store derives request_kind_key (and the value-bucket /
                // retention bucketing) from this column via
                // classify_token_request_kind, and only the canonical path
                // maps to api:brave/{web,news,llm-context}. The client-facing
                // full path stays on auth_token_logs (bin boundary) and in
                // runtime diagnostics. SerpApi/Exa pass their registered path
                // unchanged because the two coincide there.
                path: ctx.request.kind_path,
                query: ctx.request.query.as_deref(),
                status: Some(ctx.status),
                tavily_status_code: analysis.tavily_status_code,
                error: None,
                request_body: &redacted_request_body,
                response_body: &redacted_response_body,
                outcome: analysis.status,
                failure_kind: analysis.failure_kind.as_deref(),
                key_effect_code: key_effect.code.as_str(),
                key_effect_summary: key_effect.summary.as_deref(),
                binding_effect_code: KEY_EFFECT_NONE,
                binding_effect_summary: None,
                selection_effect_code: KEY_EFFECT_NONE,
                selection_effect_summary: None,
                gateway_mode: None,
                experiment_variant: None,
                provider: Some(ctx.request.provider.as_str()),
                proxy_session_id: None,
                routing_subject_hash: None,
                upstream_operation: Some(ctx.request.upstream_operation),
                fallback_reason: None,
                forwarded_headers: &ctx.sanitized_headers.forwarded,
                dropped_headers: &ctx.sanitized_headers.dropped,
                client_ip: ctx.request.client_ip.as_ref(),
            })
            .await?;
        self.link_transient_backoff_clear_request_log(&key_effect, &ctx.lease.id, request_log_id)
            .await?;
        if armed_backoff {
            self.key_store
                .set_api_key_transient_backoff_request_log_id(
                    &ctx.lease.id,
                    HTTP_GLOBAL_BACKOFF_SCOPE,
                    request_log_id,
                    self.backend_time.now_ts(),
                )
                .await?;
        }

        // M11 passive quota harvest: best-effort, bounded, material-only, and
        // NEVER fails the request. Runs after request_logs so the audit row is
        // durable first; a fast no-op when the response carries no month window
        // (e.g. every mock_brave error response), so it never extends the
        // failure-path latency and adds zero outbound requests.
        self.harvest_brave_passive_quota(&ctx.lease.id, response_headers)
            .await;

        tracing::debug!(
            component = "provider_facade",
            event = "provider_facade_attempt_recorded",
            provider = %ctx.request.provider,
            operation = ctx.request.upstream_operation,
            status = ctx.status.as_u16(),
            result_status = analysis.status,
            failure_kind = ?analysis.failure_kind.as_deref(),
            key_effect = %key_effect.code,
            api_key_id = %ctx.lease.id,
            request_log_id,
            "provider facade attempt recorded"
        );

        Ok(ProviderFacadeAttemptRecord {
            request_log_id: Some(request_log_id),
            key_effect,
        })
    }

    /// M11 passive quota harvest. Parses the month window from the LIVE
    /// response headers (zero extra requests) and, only on a material change,
    /// persists `quota_limit` / `quota_remaining` / `quota_synced_at` through
    /// the exact M11 seam (`KeyStore::record_quota_sync_sample`) with source
    /// `brave_passive`. Best-effort: a missing month window returns before any
    /// DB read (bounding failure-path latency), and any read/write error is
    /// logged at debug and swallowed so the harvest NEVER fails the request or
    /// holds the response (CONTEXT.md foreground-work discipline).
    async fn harvest_brave_passive_quota(&self, key_id: &str, response_headers: &HeaderMap) {
        let window = parse_brave_rate_limit_month_window(response_headers);
        if window.is_none() {
            // No month window advertised (success on news/llm, or ANY error
            // response): no DB read, no write. Keeps the failure path fast.
            return;
        }

        // Bounded foreground read of the stored quota to gate the material
        // change. On error treat as absent (best-effort) — a redundant write is
        // idempotent-valued and harmless; contention never blocks the response.
        let stored = sqlx::query_as::<_, (Option<i64>, Option<i64>)>(
            "SELECT quota_limit, quota_remaining FROM api_keys WHERE id = ? LIMIT 1",
        )
        .bind(key_id)
        .fetch_optional(&self.key_store.pool)
        .await
        .ok()
        .flatten()
        .unwrap_or((None, None));

        match decide_brave_harvest(window, stored.0, stored.1) {
            BraveHarvestDecision::Skip => {}
            BraveHarvestDecision::Write { limit, remaining } => {
                let now = self.backend_time.now_ts();
                if let Err(err) = self
                    .key_store
                    .record_quota_sync_sample(
                        key_id,
                        limit,
                        remaining,
                        now,
                        BRAVE_PASSIVE_QUOTA_SYNC_SOURCE,
                    )
                    .await
                {
                    tracing::debug!(
                        component = "provider_facade",
                        event = "brave_passive_quota_harvest_deferred",
                        api_key_id = %key_id,
                        err = %err,
                        "brave passive quota harvest skipped (best-effort, contention-safe)"
                    );
                }
            }
        }
    }

    /// Transport-failure audit row for the Brave REST facade (mirrors the Exa
    /// transport-error row; the T11 boundary applies to Tavily only now that
    /// SerpApi/Exa/Brave each own their record path). No response headers exist
    /// on a transport failure, so there is nothing to harvest.
    pub(crate) async fn log_brave_rest_facade_transport_error(
        &self,
        request: &ProviderRestFacadeRequest,
        lease: &ApiKeyLease,
        sanitized_headers: &SanitizedHeaders,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        if request.provider != providers::ProviderId::Brave {
            return Ok(());
        }
        let redacted_request_body = redact_api_key_bytes(&request.body);
        let redacted_empty: Vec<u8> = Vec::new();
        let error_message = err.to_string();
        self.key_store
            .log_attempt(AttemptLog {
                key_id: Some(&lease.id),
                auth_token_id: request.auth_token_id.as_deref(),
                method: &request.method,
                // Canonical kind path — same request_logs keying rule as the
                // success record path (store derives request_kind from it).
                path: request.kind_path,
                query: request.query.as_deref(),
                status: None,
                tavily_status_code: None,
                error: Some(&error_message),
                request_body: &redacted_request_body,
                response_body: &redacted_empty,
                outcome: OUTCOME_ERROR,
                failure_kind: None,
                key_effect_code: KEY_EFFECT_NONE,
                key_effect_summary: None,
                binding_effect_code: KEY_EFFECT_NONE,
                binding_effect_summary: None,
                selection_effect_code: KEY_EFFECT_NONE,
                selection_effect_summary: None,
                gateway_mode: None,
                experiment_variant: None,
                provider: Some(request.provider.as_str()),
                proxy_session_id: None,
                routing_subject_hash: None,
                upstream_operation: Some(request.upstream_operation),
                fallback_reason: None,
                forwarded_headers: &sanitized_headers.forwarded,
                dropped_headers: &sanitized_headers.dropped,
                client_ip: request.client_ip.as_ref(),
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod brave_facade_translation_tests {
    use super::*;
    use reqwest::header::HeaderName;

    fn headers_with(entries: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in entries {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("valid header name"),
                HeaderValue::from_str(value).expect("valid header value"),
            );
        }
        map
    }

    /// Canonical mock_brave web/search success headers: policy `1;w=1,
    /// 1000;w=2592000` pairs positionally with Limit `50, 1000`, Remaining
    /// `1, 999`, Reset `1, 2592000` — month window is index 1.
    fn web_search_headers(remaining: u64) -> HeaderMap {
        headers_with(&[
            ("x-ratelimit-policy", "1;w=1, 1000;w=2592000"),
            ("x-ratelimit-limit", "50, 1000"),
            (
                "x-ratelimit-remaining",
                &format!("1, {remaining}"),
            ),
            ("x-ratelimit-reset", "1, 2592000"),
        ])
    }

    // ---- outcome / classification vocabulary ----

    #[test]
    fn brave_outcome_projects_classifier_signals_onto_token_log_vocabulary() {
        let ok = brave_facade_outcome(StatusCode::OK, br#"{"type":"search","web":{"results":[]}}"#);
        assert_eq!(ok.result_status, OUTCOME_SUCCESS);
        assert_eq!(ok.failure_kind, None);

        // 402 QUOTA_LIMITED → exhausted (no failure kind).
        let quota = brave_facade_outcome(
            StatusCode::PAYMENT_REQUIRED,
            br#"{"error":{"code":"QUOTA_LIMITED"}}"#,
        );
        assert_eq!(quota.result_status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(quota.failure_kind, None);

        // 422 SUBSCRIPTION_TOKEN_INVALID → auth-fail error vocabulary.
        let auth = brave_facade_outcome(
            StatusCode::UNPROCESSABLE_ENTITY,
            br#"{"error":{"code":"SUBSCRIPTION_TOKEN_INVALID"}}"#,
        );
        assert_eq!(auth.result_status, OUTCOME_ERROR);
        assert_eq!(
            auth.failure_kind,
            Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401)
        );

        // 429 RATE_LIMITED → error rate-limited vocabulary.
        let throttled = brave_facade_outcome(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"code":"RATE_LIMITED"}}"#,
        );
        assert_eq!(throttled.result_status, OUTCOME_ERROR);
        assert_eq!(
            throttled.failure_kind,
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        );

        // 422 VALIDATION WITHOUT the token loc → ClientError (no key penalty).
        let validation = brave_facade_outcome(
            StatusCode::UNPROCESSABLE_ENTITY,
            br#"{"error":{"code":"VALIDATION","meta":{"errors":[{"loc":["query","q"]}]}}}"#,
        );
        assert_eq!(validation.result_status, OUTCOME_ERROR);
        assert_eq!(validation.failure_kind, Some(FAILURE_KIND_OTHER));
    }

    #[test]
    fn brave_health_actions_split_auth_exhaust_rate_and_validation() {
        // 402 QUOTA_LIMITED → MarkExhausted (recovery owned by T9 reset_monthly).
        let quota = brave_attempt_analysis(
            StatusCode::PAYMENT_REQUIRED,
            br#"{"error":{"code":"QUOTA_LIMITED"}}"#,
            ProviderFailureSignal::Exhausted,
        );
        assert!(matches!(
            quota.key_health_action,
            KeyHealthAction::MarkExhausted
        ));
        assert_eq!(quota.status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(quota.tavily_status_code, Some(402));

        // 422 SUBSCRIPTION_TOKEN_INVALID → Quarantine via existing machinery.
        let auth = brave_attempt_analysis(
            StatusCode::UNPROCESSABLE_ENTITY,
            br#"{"error":{"code":"SUBSCRIPTION_TOKEN_INVALID"}}"#,
            ProviderFailureSignal::AuthFailed,
        );
        let KeyHealthAction::Quarantine(decision) = auth.key_health_action else {
            panic!("brave auth-fail must quarantine via the existing machinery");
        };
        assert_eq!(decision.reason_code, BLOCKED_KEY_REASON_INVALID_API_KEY);
        assert!(decision.reason_summary.contains("HTTP 422"));

        // 422 VALIDATION (non-token loc) → ClientError passthrough, NO penalty.
        let validation = brave_attempt_analysis(
            StatusCode::UNPROCESSABLE_ENTITY,
            br#"{"error":{"code":"VALIDATION","meta":{"errors":[{"loc":["query","q"]}]}}}"#,
            ProviderFailureSignal::ClientError,
        );
        assert!(matches!(
            validation.key_health_action,
            KeyHealthAction::None
        ));
        assert_eq!(validation.status, OUTCOME_ERROR);

        // 429 RATE_LIMITED → None health action (backoff armed separately).
        let throttled = brave_attempt_analysis(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"code":"RATE_LIMITED"}}"#,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None,
            },
        );
        assert!(matches!(throttled.key_health_action, KeyHealthAction::None));
        assert_eq!(throttled.tavily_status_code, Some(429));
    }

    #[test]
    fn brave_429_backoff_is_second_scale_not_minute_scale() {
        // No Retry-After (mock_brave 429 shape) → Brave second-scale default 2s,
        // distinct from Exa/SerpApi's 30s minute-scale floor.
        let signal = ProviderFailureSignal::RateLimited {
            retry_after_secs: None,
        };
        assert_eq!(
            TavilyProxy::effective_provider_rate_limited_backoff_secs(
                providers::ProviderId::Brave,
                signal,
                0
            ),
            Some(2)
        );
        // A huge Retry-After clamps to the 30s second-scale ceiling.
        let big = ProviderFailureSignal::RateLimited {
            retry_after_secs: Some(600),
        };
        assert_eq!(
            TavilyProxy::effective_provider_rate_limited_backoff_secs(
                providers::ProviderId::Brave,
                big,
                0
            ),
            Some(30)
        );
    }

    // ---- M11 month-window header grammar (exhaustive edge cases) ----

    #[test]
    fn parse_positional_month_window_pairs_limit_remaining_reset() {
        let window =
            parse_brave_rate_limit_month_window(&web_search_headers(999)).expect("month window");
        assert_eq!(
            window,
            BraveMonthWindow {
                limit: Some(1000),
                remaining: 999,
                reset: Some(2592000),
            }
        );
    }

    #[test]
    fn parse_returns_none_when_policy_absent_or_has_no_month_window() {
        // No policy header at all.
        assert!(
            parse_brave_rate_limit_month_window(&headers_with(&[
                ("x-ratelimit-limit", "50, 1000"),
                ("x-ratelimit-remaining", "1, 999"),
            ]))
            .is_none()
        );
        // Policy with only the per-second window (w=1), no month window.
        assert!(
            parse_brave_rate_limit_month_window(&headers_with(&[
                ("x-ratelimit-policy", "1;w=1"),
                ("x-ratelimit-limit", "50"),
                ("x-ratelimit-remaining", "1"),
            ]))
            .is_none()
        );
    }

    #[test]
    fn parse_returns_none_on_short_or_missing_paired_lists() {
        // Month window is index 1 but the Remaining list has only one element.
        assert!(
            parse_brave_rate_limit_month_window(&headers_with(&[
                ("x-ratelimit-policy", "1;w=1, 1000;w=2592000"),
                ("x-ratelimit-limit", "50, 1000"),
                ("x-ratelimit-remaining", "1"),
            ]))
            .is_none()
        );
        // Remaining header missing entirely.
        assert!(
            parse_brave_rate_limit_month_window(&headers_with(&[
                ("x-ratelimit-policy", "1;w=1, 1000;w=2592000"),
                ("x-ratelimit-limit", "50, 1000"),
            ]))
            .is_none()
        );
    }

    #[test]
    fn parse_treats_w_zero_and_unparseable_windows_as_non_month() {
        // A `w=0` window is never the month window; the real month window is
        // still found at its positional index.
        let window = parse_brave_rate_limit_month_window(&headers_with(&[
            ("x-ratelimit-policy", "1;w=0, 1000;w=2592000"),
            ("x-ratelimit-limit", "50, 1000"),
            ("x-ratelimit-remaining", "1, 998"),
        ]))
        .expect("month window at index 1");
        assert_eq!(window.remaining, 998);
        assert_eq!(window.limit, Some(1000));
    }

    #[test]
    fn parse_handles_multiple_month_windows_and_extra_params() {
        // First `w=2592000` wins; extra `;`-params on a window are tolerated.
        let window = parse_brave_rate_limit_month_window(&headers_with(&[
            ("x-ratelimit-policy", "1;w=1;k=x, 500;w=2592000, 1000;w=2592000"),
            ("x-ratelimit-limit", "50, 500, 1000"),
            ("x-ratelimit-remaining", "1, 499, 998"),
        ]))
        .expect("first month window (index 1)");
        assert_eq!(window.limit, Some(500));
        assert_eq!(window.remaining, 499);
    }

    #[test]
    fn parse_returns_none_on_unparseable_month_tokens() {
        // Month Remaining token is garbage → parse yields None (no harvest).
        assert!(
            parse_brave_rate_limit_month_window(&headers_with(&[
                ("x-ratelimit-policy", "1;w=1, 1000;w=2592000"),
                ("x-ratelimit-limit", "50, 1000"),
                ("x-ratelimit-remaining", "1, not-a-number"),
            ]))
            .is_none()
        );
    }

    #[test]
    fn parse_maps_zero_limit_to_unlimited_none() {
        // Month Limit `0` → unlimited → None (NEVER persisted as 0, per M11).
        let window = parse_brave_rate_limit_month_window(&headers_with(&[
            ("x-ratelimit-policy", "0;w=2592000"),
            ("x-ratelimit-limit", "0"),
            ("x-ratelimit-remaining", "999"),
        ]))
        .expect("month window present");
        assert_eq!(window.limit, None);
        assert_eq!(window.remaining, 999);
    }

    #[test]
    fn parse_is_status_independent_harvestable_on_error_headers() {
        // Error-path harvest: the parser reads only headers, so a synthetic 402
        // / 429 response carrying month headers parses identically to a 200.
        // (mock_brave emits NO headers on errors → the live error path is a
        // clean no-op; this proves the parse itself is status-agnostic.)
        let window = parse_brave_rate_limit_month_window(&headers_with(&[
            ("x-ratelimit-policy", "1;w=1, 1000;w=2592000"),
            ("x-ratelimit-limit", "50, 1000"),
            ("x-ratelimit-remaining", "1, 0"),
            ("x-ratelimit-reset", "1, 86400"),
        ]))
        .expect("month window on a hypothetical error response");
        assert_eq!(window.remaining, 0);
        assert_eq!(window.reset, Some(86400));
    }

    // ---- M11 material-change decision (bounded-write proof) ----

    #[test]
    fn decide_writes_on_first_sample_and_on_material_change() {
        let window = Some(BraveMonthWindow {
            limit: Some(1000),
            remaining: 999,
            reset: Some(2592000),
        });
        // Fresh key (NULL/NULL) → material.
        assert_eq!(
            decide_brave_harvest(window, None, None),
            BraveHarvestDecision::Write {
                limit: 1000,
                remaining: 999
            }
        );
        // Remaining decremented (threshold crossing / depletion) → material.
        assert_eq!(
            decide_brave_harvest(window, Some(1000), Some(1000)),
            BraveHarvestDecision::Write {
                limit: 1000,
                remaining: 999
            }
        );
        // Limit changed (reset-window/plan change) → material.
        assert_eq!(
            decide_brave_harvest(window, Some(500), Some(999)),
            BraveHarvestDecision::Write {
                limit: 1000,
                remaining: 999
            }
        );
    }

    #[test]
    fn decide_skips_non_material_repeat_unlimited_and_absent() {
        // Identical stored quota → non-material repeat (bounded-write proof).
        let window = Some(BraveMonthWindow {
            limit: Some(1000),
            remaining: 998,
            reset: Some(2592000),
        });
        assert_eq!(
            decide_brave_harvest(window, Some(1000), Some(998)),
            BraveHarvestDecision::Skip
        );
        // Unlimited (limit None) → never write 0, skip.
        let unlimited = Some(BraveMonthWindow {
            limit: None,
            remaining: 998,
            reset: None,
        });
        assert_eq!(
            decide_brave_harvest(unlimited, None, None),
            BraveHarvestDecision::Skip
        );
        // No month window parsed → skip.
        assert_eq!(
            decide_brave_harvest(None, Some(1000), Some(998)),
            BraveHarvestDecision::Skip
        );
    }
}

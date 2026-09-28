//! SerpApi REST facade execution layer (plan todo 12; amendments M2/M8/M17/M22).
//!
//! Layered over the generic todo-11 seam (`proxy_provider_rest_facade`):
//! * upstream-response classification via the committed todo-3 serpapi
//!   classifier, projected into the shared `AttemptAnalysis` contract so the
//!   EXISTING `reconcile_key_health` machinery applies the acceptance health
//!   actions (401/403 auth-fail → quarantine row, 429 run-out → exhausted,
//!   hourly-flavor 429 → transient backoff on the shared HTTP scope);
//! * request_logs persistence (provider stamp, legacy `tavily_status_code`
//!   column carries the upstream status, bodies per retention profile with
//!   `redact_api_key_bytes` secret redaction — pooled key and th- tokens are
//!   never persisted);
//! * a deterministic bin-side outcome projection (`serpapi_facade_outcome`)
//!   so the server boundary can drive reserve→settle billing and token-log
//!   rows without re-implementing the classifier.
//!
//! Boundary notes: alerts with provider fields, per-provider backoff scope
//! strings, and renewal-date recovery remain todo 18; M22 forbids response
//! inspection, so business success is 2xx and credits come from the committed
//! todo-2 ProviderSpec table at the bin boundary. Exa/Brave keep their
//! documented T11 boundary (todos 13/14 flip the provider gate below).

use super::*;

/// Deterministic projection of one SerpApi facade response onto the token-log
/// vocabulary (result status + failure kind). Exported for the bin boundary
/// where the classifier internals are crate-private.
#[derive(Debug, Clone, Copy)]
pub struct SerpapiFacadeOutcome {
    /// `success` / `quota_exhausted` / `error` (shared OUTCOME_* vocabulary).
    pub result_status: &'static str,
    pub failure_kind: Option<&'static str>,
}

/// Classify a buffered SerpApi upstream response (todo-3 classifier) without
/// header evidence; the RateLimited/Ok split never depends on headers for the
/// outcome vocabulary, only Retry-After sizing (handled lib-side).
pub fn serpapi_facade_outcome(status: StatusCode, body: &[u8]) -> SerpapiFacadeOutcome {
    let signal = classify_provider_failure(
        providers::ProviderId::Serpapi,
        &ProviderFailureEvidence {
            status: status.as_u16(),
            headers: None,
            body,
        },
    );
    outcome_from_signal(signal)
}

fn outcome_from_signal(signal: ProviderFailureSignal) -> SerpapiFacadeOutcome {
    let result_status = match signal {
        ProviderFailureSignal::Ok => OUTCOME_SUCCESS,
        ProviderFailureSignal::Exhausted => OUTCOME_QUOTA_EXHAUSTED,
        ProviderFailureSignal::AuthFailed
        | ProviderFailureSignal::RateLimited { .. }
        | ProviderFailureSignal::ClientError
        | ProviderFailureSignal::ServerTransient => OUTCOME_ERROR,
    };
    SerpapiFacadeOutcome {
        result_status,
        failure_kind: signal.failure_kind(),
    }
}

fn serpapi_key_health_action(
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> KeyHealthAction {
    match signal {
        ProviderFailureSignal::Exhausted => KeyHealthAction::MarkExhausted,
        // Auth-fail feeds the EXISTING quarantine machinery (mirrors the
        // Tavily invalid-key behavior); provider-tagged alert payloads and
        // recovery policy land with todo 18. M8: quarantine/exhaustion for
        // new providers never writes api_key_low_quota_depletions.
        ProviderFailureSignal::AuthFailed => KeyHealthAction::Quarantine(QuarantineDecision {
            reason_code: BLOCKED_KEY_REASON_INVALID_API_KEY.to_string(),
            reason_summary: format!(
                "SerpApi rejected the pooled API key as invalid (HTTP {})",
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

fn serpapi_attempt_analysis(
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> AttemptAnalysis {
    let outcome = outcome_from_signal(signal);
    AttemptAnalysis {
        status: outcome.result_status,
        // The legacy column carries the upstream HTTP status verbatim for
        // provider rows (plan todo 12 logging contract).
        tavily_status_code: Some(i64::from(status.as_u16())),
        key_health_action: serpapi_key_health_action(status, body, signal),
        failure_kind: outcome.failure_kind.map(str::to_string),
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

/// Bookkeeping bundle produced by [`TavilyProxy::record_provider_rest_facade_attempt`].
pub(crate) struct ProviderFacadeAttemptRecord {
    pub(crate) request_log_id: Option<i64>,
    pub(crate) key_effect: KeyEffect,
}

impl ProviderFacadeAttemptRecord {
    /// Documented T11 boundary values (no row, no effect) — what Exa/Brave
    /// keep until todos 13/14 extend the gate.
    pub(crate) fn boundary_default() -> Self {
        Self {
            request_log_id: None,
            key_effect: KeyEffect::none(),
        }
    }
}

/// Everything the post-response bookkeeping needs; grouped so the executor
/// signature stays readable and the todo 13/14 extension is mechanical.
pub(crate) struct ProviderFacadeAttemptContext<'a> {
    pub(crate) request: &'a ProviderRestFacadeRequest,
    pub(crate) lease: &'a ApiKeyLease,
    pub(crate) sanitized_headers: &'a SanitizedHeaders,
    pub(crate) status: StatusCode,
    pub(crate) body_bytes: &'a [u8],
    pub(crate) signal: ProviderFailureSignal,
}

impl TavilyProxy {
    /// Post-response bookkeeping for provider REST facades.
    ///
    /// Todo-12 gate: ONLY the SerpApi surface records request_logs, applies
    /// `reconcile_key_health`, and arms transient backoffs; Exa/Brave keep the
    /// documented T11 boundary (todos 13/14 own their extensions and flip
    /// this gate). Tavily REST never reaches this seam (legacy executors).
    pub(crate) async fn record_provider_rest_facade_attempt(
        &self,
        ctx: &ProviderFacadeAttemptContext<'_>,
    ) -> Result<ProviderFacadeAttemptRecord, ProxyError> {
        if ctx.request.provider != providers::ProviderId::Serpapi {
            return Ok(ProviderFacadeAttemptRecord::boundary_default());
        }

        let analysis = serpapi_attempt_analysis(ctx.status, ctx.body_bytes, ctx.signal);
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

        // Hourly-flavor 429 (todo-3 RateLimited — NEVER exhaustion): arm the
        // shared HTTP backoff scope that provider-scoped acquisition avoids.
        // Sizing uses the committed todo-3 helper; SerpApi hour-scale scope
        // strings land with todo 18.
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
        // upstream URL only and client slots were stripped at the boundary, so
        // these bodies/queries never carry th- tokens or provider keys.
        let redacted_request_body = redact_api_key_bytes(&ctx.request.body);
        let redacted_response_body = redact_api_key_bytes(ctx.body_bytes);
        let request_log_id = self
            .key_store
            .log_attempt(AttemptLog {
                key_id: Some(&ctx.lease.id),
                auth_token_id: ctx.request.auth_token_id.as_deref(),
                method: &ctx.request.method,
                path: ctx.request.path.as_str(),
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

    /// Transport-failure audit row for provider REST facades (mirrors the
    /// Tavily executors' Err-arm `log_attempt`); same todo-12 provider gate.
    pub(crate) async fn log_provider_rest_facade_transport_error(
        &self,
        request: &ProviderRestFacadeRequest,
        lease: &ApiKeyLease,
        sanitized_headers: &SanitizedHeaders,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        if request.provider != providers::ProviderId::Serpapi {
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
                path: request.path.as_str(),
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
mod serpapi_facade_translation_tests {
    use super::*;

    fn evidence(status: u16, body: &'static [u8]) -> ProviderFailureEvidence<'static> {
        ProviderFailureEvidence {
            status,
            headers: None,
            body,
        }
    }

    #[test]
    fn serpapi_outcome_projects_classifier_signals_onto_token_log_vocabulary() {
        let ok = serpapi_facade_outcome(StatusCode::OK, br#"{"organic_results":[]}"#);
        assert_eq!(ok.result_status, OUTCOME_SUCCESS);
        assert_eq!(ok.failure_kind, None);

        let run_out = serpapi_facade_outcome(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":"Your account has run out of searches."}"#,
        );
        assert_eq!(run_out.result_status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(run_out.failure_kind, None);

        let hourly = serpapi_facade_outcome(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":"Your account has exceeded its hourly throughput limit."}"#,
        );
        assert_eq!(hourly.result_status, OUTCOME_ERROR);
        assert_eq!(
            hourly.failure_kind,
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        );

        let auth = serpapi_facade_outcome(
            StatusCode::UNAUTHORIZED,
            br#"{"error":"Invalid API key. Your API key should be here: https://serpapi.com/manage-api-key"}"#,
        );
        assert_eq!(auth.result_status, OUTCOME_ERROR);
        assert_eq!(
            auth.failure_kind,
            Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401)
        );

        // SSE-wrapped run-out (mock_serpapi emits BOTH flavors) exhausts too.
        let sse_run_out_signal = classify_provider_failure(
            providers::ProviderId::Serpapi,
            &evidence(
                429,
                b"data: {\"error\":\"Your account has run out of searches.\"}\n\n",
            ),
        );
        assert_eq!(sse_run_out_signal, ProviderFailureSignal::Exhausted);
    }

    #[test]
    fn serpapi_health_actions_split_dual_429_and_auth_fail() {
        let exhausted = serpapi_attempt_analysis(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":"Your account has run out of searches."}"#,
            ProviderFailureSignal::Exhausted,
        );
        assert!(matches!(
            exhausted.key_health_action,
            KeyHealthAction::MarkExhausted
        ));
        assert_eq!(exhausted.status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(exhausted.tavily_status_code, Some(429));

        let hourly = serpapi_attempt_analysis(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":"Your account has exceeded its hourly throughput limit."}"#,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None,
            },
        );
        assert!(matches!(hourly.key_health_action, KeyHealthAction::None));
        assert_eq!(hourly.status, OUTCOME_ERROR);

        let auth = serpapi_attempt_analysis(
            StatusCode::UNAUTHORIZED,
            br#"{"error":"Invalid API key."}"#,
            ProviderFailureSignal::AuthFailed,
        );
        let KeyHealthAction::Quarantine(decision) = auth.key_health_action else {
            panic!("auth-fail must quarantine via the existing machinery");
        };
        assert_eq!(decision.reason_code, BLOCKED_KEY_REASON_INVALID_API_KEY);
        assert!(decision.reason_summary.contains("HTTP 401"));
    }
}

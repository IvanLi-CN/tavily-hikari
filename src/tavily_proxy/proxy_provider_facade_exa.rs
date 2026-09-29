//! Exa REST facade execution layer (plan todo 13; amendments M2/M8/M17/M22).
//!
//! Layered over the generic todo-11 seam (`proxy_provider_rest_facade`):
//! * upstream-response classification via the committed todo-3 exa
//!   classifier, projected into the shared `AttemptAnalysis` contract so the
//!   EXISTING `reconcile_key_health` machinery applies the acceptance health
//!   actions (401 auth-fail → quarantine row, 402 budget tags → exhausted,
//!   429 → transient backoff on the shared HTTP scope sized from the parsed
//!   Retry-After);
//! * request_logs persistence (provider stamp, legacy `tavily_status_code`
//!   column carries the upstream status, bodies per retention profile with
//!   `redact_api_key_bytes` secret redaction — pooled key and th- tokens are
//!   never persisted; the bin-side boundary additionally strips client
//!   `api_key`-like body fields BEFORE this seam sees them);
//! * a deterministic bin-side outcome projection (`exa_facade_outcome`) so
//!   the server boundary can drive reserve→settle billing with the credit
//!   tiers (search=1, contents=1, answer=2) from the committed todo-2
//!   ProviderSpec table without re-implementing the classifier.
//!
//! Boundary notes: todo 18 landed the remaining depth — alert projections
//! inherit the provider field, the SuccessOnly calendar-free recovery
//! (402-exhausted keys restore on the next successful call via the shared
//! `reconcile_key_health` success arm) is matrix-pinned, and the Retry-After
//! minute-ladder sizing on the shared golden-pinned backoff scope is final
//! (M12: per-key backoff only, no new global limiter). M22 forbids response
//! inspection, so business success is 2xx. The typed stream-400 rejection is
//! a bin-boundary rule (never reaches this seam).

use super::*;

/// Deterministic projection of one Exa facade response onto the token-log
/// vocabulary (result status + failure kind). Exported for the bin boundary
/// where the classifier internals are crate-private.
#[derive(Debug, Clone, Copy)]
pub struct ExaFacadeOutcome {
    /// `success` / `quota_exhausted` / `error` (shared OUTCOME_* vocabulary).
    pub result_status: &'static str,
    pub failure_kind: Option<&'static str>,
}

/// Classify a buffered Exa upstream response (todo-3 classifier) without
/// header evidence; the 402-budget-tag/429 split never depends on headers for
/// the outcome vocabulary, only Retry-After sizing (handled lib-side where the
/// response headers are live).
pub fn exa_facade_outcome(status: StatusCode, body: &[u8]) -> ExaFacadeOutcome {
    let signal = classify_provider_failure(
        providers::ProviderId::Exa,
        &ProviderFailureEvidence {
            status: status.as_u16(),
            headers: None,
            body,
        },
    );
    outcome_from_signal(signal)
}

fn outcome_from_signal(signal: ProviderFailureSignal) -> ExaFacadeOutcome {
    let result_status = match signal {
        ProviderFailureSignal::Ok => OUTCOME_SUCCESS,
        ProviderFailureSignal::Exhausted => OUTCOME_QUOTA_EXHAUSTED,
        ProviderFailureSignal::AuthFailed
        | ProviderFailureSignal::RateLimited { .. }
        | ProviderFailureSignal::ClientError
        | ProviderFailureSignal::ServerTransient => OUTCOME_ERROR,
    };
    ExaFacadeOutcome {
        result_status,
        failure_kind: signal.failure_kind(),
    }
}

fn exa_key_health_action(
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> KeyHealthAction {
    match signal {
        // 402 budget tags (NO_MORE_CREDITS / API_KEY_BUDGET_EXCEEDED /
        // TEAM_BUDGET_EXCEEDED per the todo-3 classifier) exhaust the key.
        // Exa recovery is SuccessOnly (todo-2 registry): the flip back to
        // active belongs to the next successful call under todo 18's health
        // wiring — no calendar reset is armed here.
        ProviderFailureSignal::Exhausted => KeyHealthAction::MarkExhausted,
        // Auth-fail feeds the EXISTING quarantine machinery (mirrors the
        // SerpApi/Tavily invalid-key behavior); provider-tagged alert
        // payloads and recovery policy land with todo 18. M8:
        // quarantine/exhaustion for new providers never writes
        // api_key_low_quota_depletions.
        ProviderFailureSignal::AuthFailed => KeyHealthAction::Quarantine(QuarantineDecision {
            reason_code: BLOCKED_KEY_REASON_INVALID_API_KEY.to_string(),
            reason_summary: format!(
                "Exa rejected the pooled API key as invalid (HTTP {})",
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

fn exa_attempt_analysis(
    status: StatusCode,
    body: &[u8],
    signal: ProviderFailureSignal,
) -> AttemptAnalysis {
    let outcome = outcome_from_signal(signal);
    AttemptAnalysis {
        status: outcome.result_status,
        // The legacy column carries the upstream HTTP status verbatim for
        // provider rows (plan todo 13 logging contract).
        tavily_status_code: Some(i64::from(status.as_u16())),
        key_health_action: exa_key_health_action(status, body, signal),
        failure_kind: outcome.failure_kind.map(str::to_string),
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

impl TavilyProxy {
    /// Post-response bookkeeping for the Exa REST facade (todo-13 mirror of
    /// `record_provider_rest_facade_attempt`): reconcile, transient backoff
    /// arming, request_logs persistence, and backoff/log linking. The SerpApi
    /// record path stays byte-identical behind its own gate; Tavily REST never
    /// reaches this seam and Brave keeps the T11 boundary until todo 14.
    pub(crate) async fn record_exa_rest_facade_attempt(
        &self,
        ctx: &ProviderFacadeAttemptContext<'_>,
    ) -> Result<ProviderFacadeAttemptRecord, ProxyError> {
        if ctx.request.provider != providers::ProviderId::Exa {
            return Ok(ProviderFacadeAttemptRecord::boundary_default());
        }

        let analysis = exa_attempt_analysis(ctx.status, ctx.body_bytes, ctx.signal);
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

        // 429 (todo-3 RateLimited — Exa has no exhaustion-flavored 429): arm
        // the shared HTTP backoff scope that provider-scoped acquisition
        // avoids. Sizing parses the upstream Retry-After through the
        // committed todo-3 helper (minute-scale ladder clamp); todo 18 KEEPS
        // this ladder and the shared scope (goldens pin Retry-After 2 → 30s
        // on scope http_global; M12 per-key backoff only — evidence D1/D2).
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
        // x-api-key header only, the client header slot was stripped at the
        // boundary, and the bin-side body prep removed client api_key-like
        // fields, so the persisted request body is already slot-free; the
        // redaction pass stays as the defense-in-depth backstop.
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

    /// Transport-failure audit row for the Exa REST facade (mirrors the
    /// SerpApi transport-error row; the T11 boundary applies to the other
    /// non-Exa providers).
    pub(crate) async fn log_exa_rest_facade_transport_error(
        &self,
        request: &ProviderRestFacadeRequest,
        lease: &ApiKeyLease,
        sanitized_headers: &SanitizedHeaders,
        err: &ProxyError,
    ) -> Result<(), ProxyError> {
        if request.provider != providers::ProviderId::Exa {
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
mod exa_facade_translation_tests {
    use super::*;
    use reqwest::header::RETRY_AFTER;

    #[test]
    fn exa_outcome_projects_classifier_signals_onto_token_log_vocabulary() {
        let ok = exa_facade_outcome(StatusCode::OK, br#"{"requestId":"mock","results":[]}"#);
        assert_eq!(ok.result_status, OUTCOME_SUCCESS);
        assert_eq!(ok.failure_kind, None);

        // 402 exhaustion tags (todo-3 classifier vocabulary).
        for tag in [
            "NO_MORE_CREDITS",
            "API_KEY_BUDGET_EXCEEDED",
            "TEAM_BUDGET_EXCEEDED",
        ] {
            let body = format!(r#"{{"requestId":"mock","error":"out","tag":"{tag}"}}"#);
            let exhausted = exa_facade_outcome(StatusCode::PAYMENT_REQUIRED, body.as_bytes());
            assert_eq!(exhausted.result_status, OUTCOME_QUOTA_EXHAUSTED, "{tag}");
            assert_eq!(exhausted.failure_kind, None, "{tag}");
        }

        // A 402 WITHOUT a budget tag is a plain client error, never exhaust.
        let other = exa_facade_outcome(
            StatusCode::PAYMENT_REQUIRED,
            br#"{"requestId":"mock","error":"weird","tag":"SOMETHING_ELSE"}"#,
        );
        assert_eq!(other.result_status, OUTCOME_ERROR);
        assert_eq!(other.failure_kind, Some(FAILURE_KIND_OTHER));

        let auth = exa_facade_outcome(
            StatusCode::UNAUTHORIZED,
            br#"{"requestId":"mock","error":"Invalid API key","tag":"INVALID_API_KEY"}"#,
        );
        assert_eq!(auth.result_status, OUTCOME_ERROR);
        assert_eq!(
            auth.failure_kind,
            Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401)
        );

        let throttled = exa_facade_outcome(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"requestId":"mock","error":"Rate limit exceeded","tag":"RATE_LIMIT_EXCEEDED"}"#,
        );
        assert_eq!(throttled.result_status, OUTCOME_ERROR);
        assert_eq!(
            throttled.failure_kind,
            Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429)
        );
    }

    #[test]
    fn exa_health_actions_split_exhaustion_auth_and_rate_limits() {
        let exhausted = exa_attempt_analysis(
            StatusCode::PAYMENT_REQUIRED,
            br#"{"tag":"NO_MORE_CREDITS"}"#,
            ProviderFailureSignal::Exhausted,
        );
        assert!(matches!(
            exhausted.key_health_action,
            KeyHealthAction::MarkExhausted
        ));
        assert_eq!(exhausted.status, OUTCOME_QUOTA_EXHAUSTED);
        assert_eq!(exhausted.tavily_status_code, Some(402));

        let auth = exa_attempt_analysis(
            StatusCode::UNAUTHORIZED,
            br#"{"error":"Invalid API key","tag":"INVALID_API_KEY"}"#,
            ProviderFailureSignal::AuthFailed,
        );
        let KeyHealthAction::Quarantine(decision) = auth.key_health_action else {
            panic!("auth-fail must quarantine via the existing machinery");
        };
        assert_eq!(decision.reason_code, BLOCKED_KEY_REASON_INVALID_API_KEY);
        assert!(decision.reason_summary.contains("HTTP 401"));

        let throttled = exa_attempt_analysis(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"tag":"RATE_LIMIT_EXCEEDED"}"#,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(2),
            },
        );
        assert!(matches!(throttled.key_health_action, KeyHealthAction::None));
        assert_eq!(throttled.status, OUTCOME_ERROR);
        assert_eq!(throttled.tavily_status_code, Some(429));
    }

    #[test]
    fn exa_429_retry_after_header_parses_into_the_backoff_signal() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("2"));
        let signal = classify_provider_failure(
            providers::ProviderId::Exa,
            &ProviderFailureEvidence {
                status: 429,
                headers: Some(&headers),
                body: br#"{"tag":"RATE_LIMIT_EXCEEDED"}"#,
            },
        );
        assert_eq!(
            signal,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(2)
            }
        );
        // The parsed header sizes the armed backoff through the committed
        // minute-scale ladder (2s clamps to the 30s floor).
        assert_eq!(
            TavilyProxy::effective_provider_rate_limited_backoff_secs(
                providers::ProviderId::Exa,
                signal,
                0
            ),
            Some(30)
        );
    }
}

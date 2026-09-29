//! Plan todo 18 — provider × signal health matrix (DB-level pins) + alert
//! provider-field flow + M8 negative assertion + FU-A full-path alias pins.
//!
//! Coverage map (acceptance, plan L231):
//! - Exhausted  → `mark_quota_exhausted` + `api_key_maintenance_records`
//!   audit row + `api_key_exhausted` alert carrying `provider` (existing type
//!   key; NO new alert kinds, M18).
//! - AuthFailed → the existing quarantine machinery (`api_key_quarantines`
//!   row mirroring Tavily auth-fail behavior) + `auto_quarantine` maintenance
//!   row + `upstream_key_blocked` alert carrying `provider`.
//! - RateLimited → per-provider backoff SIZING on the shared `http_global`
//!   scope (the scope/reason columns the committed goldens pin): Brave
//!   second-scale, SerpApi hour-scale, Exa Retry-After via the existing
//!   minute-ladder clamp; arm→success-clear cycle per provider.
//! - Success on any provider path → `restore_active_status` +
//!   `auto_restore_active` maintenance row (Exa SuccessOnly recovery
//!   completes here; SerpApi renewal-date + Brave month-window calendar
//!   recovery stays pinned by `provider_pool_scoping::reset_monthly_recovers_
//!   each_provider_by_policy` — referenced, not reimplemented).
//! - M8 (binding): ZERO `api_key_low_quota_depletions` rows for
//!   serpapi/exa/brave after exhaustion+harvest+restore flows even with
//!   `quota_remaining <= threshold` seeded; Tavily-432 positive control
//!   proves the assertion can fire. 432 gate untouched.
//! - Alert wire shape: new-provider events carry `provider`; Tavily/legacy
//!   events serialize WITHOUT the `provider` key (byte-stable wire JSON) and
//!   the projection payload_json round-trips both shapes (raw CTE +
//!   projected read paths).
//! - FU-A: the three ADDITIVE brave full-path aliases classify the registered
//!   native paths onto the canonical kinds/labels/buckets.

use super::*;

use crate::backend_time::{BackendTime, ManualBackendTime};
use crate::providers::ProviderId;

fn utc_ts(rfc3339: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .expect("valid rfc3339 fixture timestamp")
        .with_timezone(&chrono::Utc)
}

async fn seed_provider_key(proxy: &TavilyProxy, provider: ProviderId, secret: &str) -> String {
    proxy
        .key_store
        .sync_keys(provider, &[secret.to_string()])
        .await
        .expect("seed provider key");
    sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = ?")
        .bind(secret)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("key id lookup")
}

async fn key_status(proxy: &TavilyProxy, secret: &str) -> String {
    sqlx::query_scalar("SELECT status FROM api_keys WHERE api_key = ?")
        .bind(secret)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("key status lookup")
}

async fn maintenance_rows(proxy: &TavilyProxy, key_id: &str, operation_code: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_maintenance_records WHERE key_id = ? AND operation_code = ?",
    )
    .bind(key_id)
    .bind(operation_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("maintenance count")
}

async fn active_quarantine_reason(proxy: &TavilyProxy, key_id: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT reason_code FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("quarantine lookup")
}

async fn depletion_rows(proxy: &TavilyProxy) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM api_key_low_quota_depletions")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("depletion count")
}

async fn active_backoff(proxy: &TavilyProxy, key_id: &str) -> Option<(String, String, i64)> {
    sqlx::query(
        "SELECT scope, reason_code, retry_after_secs FROM api_key_transient_backoffs \
         WHERE key_id = ? AND cooldown_until > ? \
         ORDER BY cooldown_until DESC LIMIT 1",
    )
    .bind(key_id)
    .bind(proxy.backend_time.now_ts())
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("backoff lookup")
    .map(|row| {
        (
            row.get::<Option<String>, _>("scope").unwrap_or_default(),
            row.get::<Option<String>, _>("reason_code")
                .unwrap_or_default(),
            row.get::<Option<i64>, _>("retry_after_secs").unwrap_or(-1),
        )
    })
}

fn exhausted_analysis(upstream_status: i64) -> AttemptAnalysis {
    AttemptAnalysis {
        status: OUTCOME_QUOTA_EXHAUSTED,
        tavily_status_code: Some(upstream_status),
        key_health_action: KeyHealthAction::MarkExhausted,
        failure_kind: None,
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

fn auth_failed_analysis(upstream_status: i64) -> AttemptAnalysis {
    AttemptAnalysis {
        status: OUTCOME_ERROR,
        tavily_status_code: Some(upstream_status),
        key_health_action: KeyHealthAction::Quarantine(QuarantineDecision {
            reason_code: BLOCKED_KEY_REASON_INVALID_API_KEY.to_string(),
            reason_summary: "matrix auth-fail".to_string(),
            reason_detail: "matrix".to_string(),
        }),
        failure_kind: Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401.to_string()),
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

fn success_analysis() -> AttemptAnalysis {
    AttemptAnalysis {
        status: OUTCOME_SUCCESS,
        tavily_status_code: Some(200),
        key_health_action: KeyHealthAction::None,
        failure_kind: None,
        key_effect: KeyEffect::none(),
        api_key_id: None,
    }
}

async fn spawn_matrix_proxy(prefix: &str) -> (TavilyProxy, ManualBackendTime) {
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, clock) = BackendTime::manual(utc_ts("2026-09-15T12:00:00Z"));
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("matrix proxy created");
    (proxy, clock)
}

// ---------------------------------------------------------------------------
// RateLimited: per-provider sizing matrix (shared http_global scope).
// ---------------------------------------------------------------------------

/// SerpApi hour-scale sizing (D6 "hour scope"): no Retry-After → the full
/// hour default; parsed Retry-After honored from the minute-ladder floor up
/// to the hour ceiling (the committed T3 pin 45→45 stays byte-stable).
#[test]
fn serpapi_rate_limited_backoff_is_hour_scale() {
    // Given the todo-3 signal with no Retry-After (mock/live SerpApi hourly
    // 429s carry none), When sized for SerpApi, Then the hour default arms.
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Serpapi,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None
            },
            utc_ts("2026-09-15T12:00:00Z").timestamp(),
        ),
        Some(3600),
        "hour-scale default"
    );
    // A parsed Retry-After inside the envelope is honored (T3 pin: 45→45)...
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Serpapi,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(120)
            },
            0,
        ),
        Some(120)
    );
    // ...and clamped on both ends: the minute-ladder floor below, the hour
    // ceiling above.
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Serpapi,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(2)
            },
            0,
        ),
        Some(30)
    );
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Serpapi,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(7200)
            },
            0,
        ),
        Some(3600)
    );
}

/// Brave second-scale + Exa minute-ladder pins: committed T13/T14 behavior
/// stays byte-identical (goldens pin brave=2s and exa Retry-After 2→30).
#[test]
fn brave_second_scale_and_exa_minute_ladder_pins_unchanged() {
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Brave,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None
            },
            0,
        ),
        Some(2),
        "brave second-scale default"
    );
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Brave,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(120)
            },
            0,
        ),
        Some(30),
        "brave clamps to the 30s second-scale ceiling"
    );
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Exa,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(2)
            },
            0,
        ),
        Some(30),
        "exa Retry-After through the existing minute-ladder floor (golden)"
    );
    assert_eq!(
        TavilyProxy::effective_provider_rate_limited_backoff_secs(
            ProviderId::Exa,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: Some(120)
            },
            0,
        ),
        Some(120),
        "exa Retry-After honored inside the ladder"
    );
}

/// Arm→success semantics per provider on the shared scope: the 429 row shape
/// the goldens pin (scope http_global, reason upstream_rate_limited_429,
/// per-provider sizing) expires by cooldown and is NOT cleared by the
/// success path (existing Tavily semantics: `clear_transient_backoffs_after_
/// success` clears only 403-blocked rows, which the second phase pins).
#[tokio::test]
async fn rate_limited_arm_and_success_clear_cycle_per_provider() {
    let (proxy, _clock) = spawn_matrix_proxy("health-matrix-backoff-cycle").await;
    for provider in [ProviderId::Serpapi, ProviderId::Exa, ProviderId::Brave] {
        let secret = format!("mtx-rl-{}", provider.as_str());
        let key_id = seed_provider_key(&proxy, provider, &secret).await;
        let now = proxy.backend_time.now_ts();
        let sized = TavilyProxy::effective_provider_rate_limited_backoff_secs(
            provider,
            ProviderFailureSignal::RateLimited {
                retry_after_secs: None,
            },
            now,
        )
        .expect("rate-limited sizing");
        proxy
            .key_store
            .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                key_id: &key_id,
                scope: HTTP_GLOBAL_BACKOFF_SCOPE,
                cooldown_until: now + sized,
                retry_after_secs: sized,
                reason_code: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
                source_request_log_id: None,
                now,
            })
            .await
            .expect("arm backoff");
        let (scope, reason, retry_after) = active_backoff(&proxy, &key_id)
            .await
            .expect("armed row present");
        assert_eq!(scope, HTTP_GLOBAL_BACKOFF_SCOPE);
        assert_eq!(reason, FAILURE_KIND_UPSTREAM_RATE_LIMITED_429);
        assert_eq!(retry_after, sized);
        // The key STAYS active under a transient 429 (never exhausted).
        assert_eq!(key_status(&proxy, &secret).await, "active");

        // Success on the same key leaves the 429 row to its cooldown expiry
        // (house semantics, provider-agnostic) — the clear lane targets the
        // 403-blocked rows pinned in the second phase below.
        let lease = proxy
            .key_store
            .acquire_key_for_provider(provider)
            .await
            .expect("provider lease");
        assert_eq!(lease.id, key_id);
        let effect = proxy
            .clear_transient_backoffs_after_success(&lease.id, "/matrix", None)
            .await
            .expect("success clear pass over a 429 row");
        assert_eq!(effect.code.as_str(), KEY_EFFECT_NONE);
        assert!(
            active_backoff(&proxy, &key_id).await.is_some(),
            "429 backoffs expire by cooldown, not by success clearing"
        );

        // Phase 2 — the 403-blocked flavor IS cleared by the success path
        // (same lane the six provider record paths call), with its audit row.
        proxy
            .key_store
            .arm_api_key_transient_backoff(ApiKeyTransientBackoffArm {
                key_id: &key_id,
                scope: HTTP_GLOBAL_BACKOFF_SCOPE,
                cooldown_until: now + sized,
                retry_after_secs: sized,
                reason_code: Some(FAILURE_KIND_UPSTREAM_UNKNOWN_403),
                source_request_log_id: None,
                now,
            })
            .await
            .expect("arm 403 backoff");
        let effect = proxy
            .clear_transient_backoffs_after_success(&lease.id, "/matrix", None)
            .await
            .expect("success clear");
        assert_eq!(effect.code.as_str(), KEY_EFFECT_TRANSIENT_BACKOFF_CLEARED);
        assert!(active_backoff(&proxy, &key_id).await.is_none());
        assert_eq!(
            maintenance_rows(&proxy, &key_id, MAINTENANCE_OP_AUTO_CLEAR_TRANSIENT_BACKOFF).await,
            1
        );
    }
}

// ---------------------------------------------------------------------------
// Exhausted / AuthFailed / Success per provider (reconcile DB contract).
// ---------------------------------------------------------------------------

/// Exhausted per provider: status flip + audit maintenance row + effect;
/// M8 — zero depletion rows even with quota_remaining seeded at 0.
#[tokio::test]
async fn exhausted_signal_marks_exhausted_with_maintenance_row_per_provider() {
    let (proxy, _clock) = spawn_matrix_proxy("health-matrix-exhausted").await;
    // Provider exhaustion statuses from the committed classifier vocabulary:
    // serpapi run-out = 429, exa budget tags = 402, brave QUOTA_LIMITED = 402.
    for (provider, upstream_status) in [
        (ProviderId::Serpapi, 429i64),
        (ProviderId::Exa, 402),
        (ProviderId::Brave, 402),
    ] {
        let secret = format!("mtx-ex-{}", provider.as_str());
        let key_id = seed_provider_key(&proxy, provider, &secret).await;
        // M8 adversarial seed: remaining AT ZERO (under any threshold) so a
        // wrongly-generalized depletion gate WOULD write a row.
        sqlx::query("UPDATE api_keys SET quota_remaining = 0, quota_limit = 100 WHERE id = ?")
            .bind(&key_id)
            .execute(&proxy.key_store.pool)
            .await
            .expect("seed zero remaining");
        let lease = proxy
            .key_store
            .acquire_key_for_provider(provider)
            .await
            .expect("lease");
        let effect = proxy
            .reconcile_key_health(
                &lease,
                "/matrix/exhausted",
                &exhausted_analysis(upstream_status),
                None,
            )
            .await
            .expect("reconcile exhausted");
        assert_eq!(
            effect.code.as_str(),
            KEY_EFFECT_MARKED_EXHAUSTED,
            "{provider}"
        );
        assert_eq!(key_status(&proxy, &secret).await, "exhausted", "{provider}");
        assert_eq!(
            maintenance_rows(&proxy, &key_id, MAINTENANCE_OP_AUTO_MARK_EXHAUSTED).await,
            1,
            "{provider} audit row"
        );
    }
    // M8 binding negative: the new-provider flows above wrote ZERO depletion
    // rows despite the seeded threshold crossing.
    assert_eq!(
        depletion_rows(&proxy).await,
        0,
        "M8: new providers never write api_key_low_quota_depletions"
    );
}

/// Tavily-432 positive control: the untouched 432 gate still records a
/// depletion row for a Tavily key at/below the threshold (proves the M8
/// negative assertions above can actually fire).
#[tokio::test]
async fn tavily_432_positive_control_still_records_depletion() {
    let (proxy, _clock) = spawn_matrix_proxy("health-matrix-tavily-432").await;
    let key_id = seed_provider_key(&proxy, ProviderId::Tavily, "mtx-tvly-432").await;
    sqlx::query("UPDATE api_keys SET quota_remaining = 0 WHERE id = ?")
        .bind(&key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed zero remaining");
    let lease = proxy
        .key_store
        .acquire_key_for_provider(ProviderId::Tavily)
        .await
        .expect("tavily lease");
    let effect = proxy
        .reconcile_key_health(&lease, "/matrix/tavily-432", &exhausted_analysis(432), None)
        .await
        .expect("reconcile tavily 432");
    assert_eq!(effect.code.as_str(), KEY_EFFECT_MARKED_EXHAUSTED);
    assert_eq!(
        depletion_rows(&proxy).await,
        1,
        "Tavily 432 keeps its depletion behavior byte-stable"
    );
}

/// M8 gate unit pin: the generalized depletion predicate refuses every
/// non-Tavily provider even on a literal 432 status.
#[test]
fn low_quota_depletion_gate_refuses_new_providers_even_on_432() {
    for provider in [ProviderId::Serpapi, ProviderId::Exa, ProviderId::Brave] {
        assert!(
            !low_quota_depletion_applies(provider, Some(432)),
            "{provider} must never write depletion rows"
        );
    }
    assert!(low_quota_depletion_applies(ProviderId::Tavily, Some(432)));
    assert!(!low_quota_depletion_applies(ProviderId::Tavily, Some(402)));
}

/// AuthFailed per provider: quarantine row mirroring the Tavily auth-fail
/// shape (reason invalid_api_key, cleared_at NULL) + audit maintenance row.
#[tokio::test]
async fn auth_failed_signal_quarantines_with_maintenance_row_per_provider() {
    let (proxy, _clock) = spawn_matrix_proxy("health-matrix-authfail").await;
    // Provider auth-fail statuses: serpapi 401, exa 401, brave 422
    // SUBSCRIPTION_TOKEN_INVALID (live-probed classifier vocabulary).
    for (provider, upstream_status) in [
        (ProviderId::Serpapi, 401i64),
        (ProviderId::Exa, 401),
        (ProviderId::Brave, 422),
    ] {
        let secret = format!("mtx-af-{}", provider.as_str());
        let key_id = seed_provider_key(&proxy, provider, &secret).await;
        let lease = proxy
            .key_store
            .acquire_key_for_provider(provider)
            .await
            .expect("lease");
        let effect = proxy
            .reconcile_key_health(
                &lease,
                "/matrix/authfail",
                &auth_failed_analysis(upstream_status),
                None,
            )
            .await
            .expect("reconcile auth failure");
        assert_eq!(effect.code.as_str(), KEY_EFFECT_QUARANTINED, "{provider}");
        assert_eq!(
            active_quarantine_reason(&proxy, &key_id).await.as_deref(),
            Some(BLOCKED_KEY_REASON_INVALID_API_KEY),
            "{provider} quarantine row"
        );
        assert_eq!(
            maintenance_rows(&proxy, &key_id, MAINTENANCE_OP_AUTO_QUARANTINE).await,
            1,
            "{provider} audit row"
        );
    }
    assert_eq!(depletion_rows(&proxy).await, 0, "M8 stays negative");
}

/// Success-restore per provider (Exa SuccessOnly recovery completes here):
/// an exhausted key flips back to active with its audit row + effect.
#[tokio::test]
async fn success_restores_exhausted_keys_with_maintenance_row_per_provider() {
    let (proxy, _clock) = spawn_matrix_proxy("health-matrix-restore").await;
    for provider in [ProviderId::Serpapi, ProviderId::Exa, ProviderId::Brave] {
        let secret = format!("mtx-sr-{}", provider.as_str());
        let key_id = seed_provider_key(&proxy, provider, &secret).await;
        assert!(
            proxy
                .key_store
                .mark_quota_exhausted(&secret)
                .await
                .expect("seed exhausted")
        );
        // Tier-2 fallback re-leases the exhausted key (house semantics).
        let lease = proxy
            .key_store
            .acquire_key_for_provider(provider)
            .await
            .expect("exhausted lease");
        assert_eq!(lease.id, key_id);
        let effect = proxy
            .reconcile_key_health(&lease, "/matrix/success", &success_analysis(), None)
            .await
            .expect("reconcile success");
        assert_eq!(
            effect.code.as_str(),
            KEY_EFFECT_RESTORED_ACTIVE,
            "{provider}"
        );
        assert_eq!(key_status(&proxy, &secret).await, "active", "{provider}");
        assert_eq!(
            maintenance_rows(&proxy, &key_id, MAINTENANCE_OP_AUTO_RESTORE_ACTIVE).await,
            1,
            "{provider} audit row"
        );
    }
    assert_eq!(depletion_rows(&proxy).await, 0, "M8 stays negative");
}

// ---------------------------------------------------------------------------
// Alert events: provider field flow (existing type keys, zero new kinds).
// ---------------------------------------------------------------------------

/// Billing-settle-shaped `auth_token_logs` seed for the alert matrix (the
/// facade settle path writes these rows with the provider stamp + failure
/// kind projection).
#[derive(Clone, Copy)]
struct AtlAlertSeed<'a> {
    token_id: &'a str,
    key_id: Option<&'a str>,
    provider: Option<&'a str>,
    result_status: &'a str,
    failure_kind: Option<&'a str>,
    http_status: i64,
    path: &'a str,
    kind_key: Option<&'a str>,
    created_at: i64,
}

async fn seed_alert_atl_row(proxy: &TavilyProxy, seed: AtlAlertSeed<'_>) {
    sqlx::query(
        "INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status,
            request_kind_key, request_kind_label, result_status, failure_kind,
            key_effect_code, binding_effect_code, selection_effect_code,
            provider, api_key_id, counts_business_quota, created_at
         ) VALUES (?, 'GET', ?, NULL, ?, NULL, ?, ?, ?, ?, 'none', 'none', 'none', ?, ?, 1, ?)",
    )
    .bind(seed.token_id)
    .bind(seed.path)
    .bind(seed.http_status)
    .bind(seed.kind_key)
    .bind(seed.kind_key.map(|key| format!("label {key}")))
    .bind(seed.result_status)
    .bind(seed.failure_kind)
    .bind(seed.provider)
    .bind(seed.key_id)
    .bind(seed.created_at)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed atl alert row");
}

/// Wire-shape provider extraction: reads the SERIALIZED event so the probe
/// pins the exact API/payload surface (a missing field is `None`, and legacy
/// rows must never gain the key).
fn wire_provider(event: &AlertEventRecord) -> Option<String> {
    serde_json::to_value(event)
        .expect("serialize alert event")
        .get("provider")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// The full alert matrix: real reconcile-written maintenance rows (serpapi
/// quarantine, exa exhaustion) + facade-shaped atl rows (brave 429, tavily
/// 429/432 controls) → alert events keyed by EXISTING type constants, each
/// new-provider event carrying its provider and every Tavily/legacy event
/// serializing WITHOUT the provider key (byte-stable wire JSON). Both read
/// paths are covered: raw CTE before the projection catches up and the
/// projected read model after.
#[tokio::test]
async fn alert_events_carry_provider_for_new_providers_and_keep_tavily_wire_shape() {
    let (proxy, _clock) = spawn_matrix_proxy("health-matrix-alerts").await;
    let now = proxy.backend_time.now_ts();
    sqlx::query(
        "INSERT INTO auth_tokens (id, secret, created_at) VALUES ('hmx-token', 'hmx-secret', ?)",
    )
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token");

    // SerpApi auth-fail quarantine via the real reconcile path.
    let serpapi_key_id = seed_provider_key(&proxy, ProviderId::Serpapi, "mtx-al-srp").await;
    let serpapi_lease = proxy
        .key_store
        .acquire_key_for_provider(ProviderId::Serpapi)
        .await
        .expect("serpapi lease");
    proxy
        .reconcile_key_health(
            &serpapi_lease,
            "/matrix/alerts/serpapi",
            &auth_failed_analysis(401),
            Some("hmx-token"),
        )
        .await
        .expect("serpapi quarantine");

    // Exa exhaustion via the real reconcile path.
    let exa_key_id = seed_provider_key(&proxy, ProviderId::Exa, "mtx-al-exa").await;
    let exa_lease = proxy
        .key_store
        .acquire_key_for_provider(ProviderId::Exa)
        .await
        .expect("exa lease");
    proxy
        .reconcile_key_health(
            &exa_lease,
            "/matrix/alerts/exa",
            &exhausted_analysis(402),
            Some("hmx-token"),
        )
        .await
        .expect("exa exhaustion");

    // Brave 429 + Tavily controls as billing-settle-shaped atl rows.
    let brave_key_id = seed_provider_key(&proxy, ProviderId::Brave, "mtx-al-brv").await;
    seed_alert_atl_row(
        &proxy,
        AtlAlertSeed {
            token_id: "hmx-token",
            key_id: Some(&brave_key_id),
            provider: Some("brave"),
            result_status: OUTCOME_ERROR,
            failure_kind: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            http_status: 429,
            path: "/api/brave/res/v1/web/search",
            kind_key: Some("api:brave/web"),
            created_at: now,
        },
    )
    .await;
    let tavily_key_id = seed_provider_key(&proxy, ProviderId::Tavily, "mtx-al-tvly").await;
    seed_alert_atl_row(
        &proxy,
        AtlAlertSeed {
            token_id: "hmx-token",
            key_id: Some(&tavily_key_id),
            provider: None,
            result_status: OUTCOME_ERROR,
            failure_kind: Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429),
            http_status: 429,
            path: "/api/tavily/search",
            kind_key: Some("api:search"),
            created_at: now - 1,
        },
    )
    .await;
    seed_alert_atl_row(
        &proxy,
        AtlAlertSeed {
            token_id: "hmx-token",
            key_id: Some(&tavily_key_id),
            provider: None,
            result_status: OUTCOME_QUOTA_EXHAUSTED,
            failure_kind: None,
            http_status: 432,
            path: "/api/tavily/search",
            kind_key: Some("api:search"),
            created_at: now - 2,
        },
    )
    .await;

    // READ PATH 1 — raw CTE (fresh source rows are ahead of the projection
    // watermark, so the page serves from the live branch).
    let raw_page = proxy
        .key_store
        .fetch_alert_events_page(None, None, None, None, None, None, &[], 1, 100)
        .await
        .expect("raw alert page");
    let summarize = |items: &[AlertEventRecord]| -> Vec<(String, Option<String>, String)> {
        let mut rows = items
            .iter()
            .map(|event| {
                (
                    event.alert_type.clone(),
                    wire_provider(event),
                    event.subject_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    let raw_rows = summarize(&raw_page.items);
    assert!(
        raw_rows.contains(&(
            ALERT_TYPE_UPSTREAM_KEY_BLOCKED.to_string(),
            Some("serpapi".to_string()),
            serpapi_key_id.clone()
        )),
        "serpapi quarantine alert carries provider (raw): {raw_rows:?}"
    );
    assert!(
        raw_rows.contains(&(
            ALERT_TYPE_API_KEY_EXHAUSTED.to_string(),
            Some("exa".to_string()),
            exa_key_id.clone()
        )),
        "exa exhaustion alert carries provider (raw): {raw_rows:?}"
    );
    assert!(
        raw_rows.contains(&(
            ALERT_TYPE_UPSTREAM_RATE_LIMITED_429.to_string(),
            Some("brave".to_string()),
            brave_key_id.clone()
        )),
        "brave 429 alert carries provider (raw): {raw_rows:?}"
    );
    // Tavily controls: existing types, NO provider on the record or the wire.
    let tavily_429 = raw_page
        .items
        .iter()
        .find(|event| {
            event.alert_type == ALERT_TYPE_UPSTREAM_RATE_LIMITED_429
                && event.subject_id == tavily_key_id
        })
        .expect("tavily 429 alert");
    assert!(wire_provider(tavily_429).is_none());
    let tavily_wire = serde_json::to_value(tavily_429).expect("serialize tavily event");
    assert!(
        tavily_wire.get("provider").is_none(),
        "legacy Tavily wire JSON must not gain a provider key (byte-stable)"
    );
    let tavily_432 = raw_page
        .items
        .iter()
        .find(|event| event.alert_type == ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432)
        .expect("tavily 432 alert (branch stays Tavily-scoped)");
    assert!(wire_provider(tavily_432).is_none());
    assert!(
        raw_page
            .items
            .iter()
            .any(|event| wire_provider(event).as_deref() == Some("serpapi")),
        "serpapi event exposes provider on the wire"
    );

    // Projection write side: advance until the seeded events land, then pin
    // payload_json provider round-trip + absence for legacy rows.
    let mut projected_events = 0i64;
    for _ in 0..48 {
        proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("advance projection slice");
        projected_events = sqlx::query_scalar(
            "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events",
        )
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count projected events");
        if projected_events >= 5 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(
        projected_events >= 5,
        "projection converged with the seeded events"
    );
    let payloads: Vec<String> = sqlx::query_scalar(
        "SELECT payload_json FROM observability.dashboard_alert_projection_events",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("payload dump");
    assert!(
        payloads
            .iter()
            .any(|payload| payload.contains("\"provider\":\"serpapi\"")),
        "serpapi provider persisted in the projection payload"
    );
    assert!(
        payloads
            .iter()
            .any(|payload| payload.contains("\"provider\":\"exa\"")),
        "exa provider persisted in the projection payload"
    );
    assert!(
        payloads
            .iter()
            .any(|payload| payload.contains("\"provider\":\"brave\"")),
        "brave provider persisted in the projection payload"
    );
    assert!(
        payloads.iter().any(|payload| {
            payload.contains(ALERT_TYPE_UPSTREAM_USAGE_LIMIT_432) && !payload.contains("provider")
        }),
        "legacy tavily payload keeps its byte shape (no provider key)"
    );

    // READ PATH 2 — projected read model after convergence.
    let projected_page = proxy
        .key_store
        .fetch_alert_events_page(None, None, None, None, None, None, &[], 1, 100)
        .await
        .expect("projected alert page");
    let projected_rows = summarize(&projected_page.items);
    assert_eq!(
        projected_rows, raw_rows,
        "raw and projected read paths expose the identical provider flow"
    );
}

// ---------------------------------------------------------------------------
// FU-A: brave registered full-path alias kinds (additive).
// ---------------------------------------------------------------------------

/// The three ADDITIVE full-path aliases classify the registered native brave
/// paths onto the canonical kinds/labels, and the canonical short paths stay
/// unchanged (Tavily + siblings byte-stable).
#[test]
fn brave_full_path_aliases_classify_registered_kinds() {
    for (full_path, canonical_path, kind, label) in [
        (
            "/api/brave/res/v1/web/search",
            "/api/brave/web/search",
            "api:brave/web",
            "API | brave/web",
        ),
        (
            "/api/brave/res/v1/news/search",
            "/api/brave/news/search",
            "api:brave/news",
            "API | brave/news",
        ),
        (
            "/api/brave/res/v1/llm/context",
            "/api/brave/llm/context",
            "api:brave/llm-context",
            "API | brave/llm-context",
        ),
    ] {
        let full = classify_token_request_kind(full_path, None);
        assert_eq!(full.key, kind, "full path {full_path}");
        assert_eq!(full.label, label);
        let canonical = classify_token_request_kind(canonical_path, None);
        assert_eq!(canonical.key, kind, "canonical path stays {canonical_path}");
        assert_eq!(canonical.label, label);
    }
}

/// SQL-mirror pin: provider rows pass through the stored-key whitelist for
/// BOTH brave path shapes (the trigger/CTE SQL never derives provider kinds
/// from paths, so the aliases need no SQL-side sibling).
#[tokio::test]
async fn brave_full_path_sql_kind_whitelist_pins() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("memory pool");
    for path in ["'/api/brave/res/v1/web/search'", "'/api/brave/web/search'"] {
        let stored: String = sqlx::query_scalar(&format!(
            "SELECT {} AS v",
            token_log_request_kind_key_sql(path, "'api:brave/web'")
        ))
        .fetch_one(&pool)
        .await
        .expect("stored-kind sql executes");
        assert_eq!(stored, "api:brave/web", "path {path}");
        let request_log: String = sqlx::query_scalar(&format!(
            "SELECT {} AS v",
            request_log_request_kind_key_sql(path, "NULL", "'api:brave/web'")
        ))
        .fetch_one(&pool)
        .await
        .expect("request-log kind sql executes");
        assert_eq!(request_log, "api:brave/web", "path {path}");
    }
}

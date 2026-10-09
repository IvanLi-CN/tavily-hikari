//! Quota-sync provider adapters + M4/M8 hard-guard matrix (plan todo 19).
//!
//! Coverage map (task gate 1):
//! - SerpApi /account adapter: pure field mapping (all four key fields),
//!   live fetch+persist through the injectable BackendTime clock, account
//!   status NOTE-without-disable (+ dedupe), renewal-driven recovery via
//!   the EXISTING T9 RenewalDate policy with the T18-parity audit row, and
//!   the pre-renewal stay-exhausted complement.
//! - M4 C4a: Tavily-/usage and SerpApi-/account fetches contend for the
//!   SINGLE remote-attempt lease — exactly one outbound in flight, the
//!   second deferred until the first completes, both eventually succeed.
//! - M8 negative: zero api_key_low_quota_depletions rows for serpapi sync,
//!   with a Tavily positive control proving the assertion can fire.
//! - Zero-leak guard seams: pending-list provider separation (cold+hot),
//!   reconciliation key enumeration excludes KNOWN non-tavily keys while
//!   legacy dangling-key rows stay included (compat pin), research drain
//!   candidate + wake-calc guards, Tavily probe-secret guard.
//! - FU-B (T18 handoff #1) DECISION PIN: keep the documented brave Limit-0
//!   SKIP-WRITE. The seam fact that blocks a NULL-capable variant without
//!   a migration is pinned here via PRAGMA (api_key_quota_sync_samples.
//!   quota_limit NOT NULL); the parser/decision unit pins
//!   (parse_maps_zero_limit_to_unlimited_none +
//!   decide_skips_non_material_repeat_unlimited_and_absent) live in the
//!   FORBIDDEN brave facade module and stay green untouched. A Limit-0
//!   brave key therefore PRESERVES its fresh-key NULL quota_limit (M11
//!   "NEVER 0"), and the call-site + column both sit outside T19
//!   ownership — an additive NULL seam variant is mechanically
//!   unreachable here, so the documented skip stands.

use super::*;

use crate::backend_time::BackendTime;
use crate::providers::ProviderId;
use axum::extract::Query;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

fn utc_ts(rfc3339: &str) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .expect("valid rfc3339 fixture timestamp")
        .with_timezone(&chrono::Utc)
}

/// Shared observability for the in-process upstream mocks: an in-flight
/// gauge (C4a single-outbound proof), an ordered enter/exit event log, and
/// per-slot secret observations (zero-leak proofs).
#[derive(Default)]
struct EgressObservation {
    inflight: AtomicUsize,
    peak_inflight: AtomicUsize,
    events: StdMutex<Vec<String>>,
    usage_requests: AtomicUsize,
    account_requests: AtomicUsize,
    exa_requests: AtomicUsize,
    brave_requests: AtomicUsize,
    seen_usage_bearers: StdMutex<Vec<String>>,
    last_account_api_key: StdMutex<Option<String>>,
}

impl EgressObservation {
    fn enter(&self, name: &str) {
        let now = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak_inflight.fetch_max(now, Ordering::SeqCst);
        self.events
            .lock()
            .expect("events lock")
            .push(format!("enter:{name}"));
    }

    fn exit(&self, name: &str) {
        self.inflight.fetch_sub(1, Ordering::SeqCst);
        self.events
            .lock()
            .expect("events lock")
            .push(format!("exit:{name}"));
    }
}

fn account_payload(
    account_status: &str,
    searches_per_month: i64,
    total_searches_left: i64,
    plan_renewal_date: &str,
) -> serde_json::Value {
    json!({
        "account_status": account_status,
        "plan_id": "bigdata",
        "searches_per_month": searches_per_month,
        "total_searches_left": total_searches_left,
        "plan_searches_left": total_searches_left,
        "plan_renewal_date": plan_renewal_date,
        "this_hour_searches": 1,
        "account_rate_limit_per_hour": 6000
    })
}

/// In-process mock upstream serving the Tavily `/usage` lane, the SerpApi
/// `/account` lane, and inert exa/brave counters (scheduler-never-polls
/// proof). The account status/renewal fields are mutable so one spawn can
/// serve several scenarios.
struct MockUpstreams {
    base: String,
}

async fn spawn_adapter_mock_upstreams(
    obs: Arc<EgressObservation>,
    account_body: serde_json::Value,
    hold_millis: u64,
) -> MockUpstreams {
    let account_body = Arc::new(StdMutex::new(account_body));
    let mut app = Router::new();
    {
        let obs = obs.clone();
        app = app.route(
            "/usage",
            get(move |headers: axum::http::HeaderMap| {
                let obs = obs.clone();
                async move {
                    obs.usage_requests.fetch_add(1, Ordering::SeqCst);
                    if let Some(value) = headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|header| header.to_str().ok())
                    {
                        obs.seen_usage_bearers
                            .lock()
                            .expect("bearer lock")
                            .push(value.to_string());
                    }
                    obs.enter("tavily_usage");
                    tokio::time::sleep(Duration::from_millis(hold_millis)).await;
                    obs.exit("tavily_usage");
                    Json(json!({ "key": { "limit": 100, "usage": 10 } }))
                }
            }),
        );
    }
    {
        let obs = obs.clone();
        let body = account_body.clone();
        app = app.route(
            "/account",
            get(
                move |Query(query): Query<std::collections::HashMap<String, String>>| {
                    let obs = obs.clone();
                    let body = body.clone();
                    async move {
                        obs.account_requests.fetch_add(1, Ordering::SeqCst);
                        *obs.last_account_api_key.lock().expect("account key lock") =
                            query.get("api_key").cloned();
                        obs.enter("serpapi_account");
                        tokio::time::sleep(Duration::from_millis(hold_millis)).await;
                        obs.exit("serpapi_account");
                        Json(body.lock().expect("account body lock").clone())
                    }
                },
            ),
        );
    }
    {
        let obs = obs.clone();
        app = app.route(
            "/exa-probe",
            get(move || {
                let obs = obs.clone();
                async move {
                    obs.exa_requests.fetch_add(1, Ordering::SeqCst);
                    StatusCode::from_u16(200).expect("valid status")
                }
            }),
        );
    }
    {
        let obs = obs.clone();
        app = app.route(
            "/brave-probe",
            get(move || {
                let obs = obs.clone();
                async move {
                    obs.brave_requests.fetch_add(1, Ordering::SeqCst);
                    StatusCode::from_u16(200).expect("valid status")
                }
            }),
        );
    }
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
    let addr = listener.local_addr().expect("mock addr");
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .expect("mock upstreams run");
    });
    MockUpstreams {
        base: format!("http://{addr}"),
    }
}

fn adapter_bootstrap(serpapi_rest: &str, exa_rest: &str, brave_rest: &str) -> ProviderBootstrap {
    ProviderBootstrap {
        serpapi_upstream: serpapi_rest.to_string(),
        exa_upstream: exa_rest.to_string(),
        brave_upstream: brave_rest.to_string(),
        ..ProviderBootstrap::default()
    }
}

async fn build_adapter_proxy(
    db_stem: &str,
    mocks: &MockUpstreams,
    now: chrono::DateTime<chrono::Utc>,
) -> (TavilyProxy, crate::ManualBackendTime) {
    let db_path = temp_db_path(db_stem);
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, clock) = BackendTime::manual(now);
    let bootstrap = adapter_bootstrap(
        &mocks.base,
        &format!("{}/exa-probe", mocks.base),
        &format!("{}/brave-probe", mocks.base),
    );
    let proxy = TavilyProxy::with_providers_and_time_in_ha_mode(
        Vec::<String>::new(),
        "http://127.0.0.1:9",
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        bootstrap,
        backend_time,
        HaMode::Single,
    )
    .await
    .expect("create adapter proxy");
    (proxy, clock)
}

async fn key_id_for(pool: &sqlx::SqlitePool, secret: &str) -> String {
    sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = ?")
        .bind(secret)
        .fetch_one(pool)
        .await
        .expect("key id lookup")
}

async fn fetch_key_row(
    pool: &sqlx::SqlitePool,
    key_id: &str,
) -> (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
    sqlx::query_as(
        "SELECT status, quota_limit, quota_remaining, quota_synced_at, quota_renews_at \
         FROM api_keys WHERE id = ?",
    )
    .bind(key_id)
    .fetch_one(pool)
    .await
    .expect("read api_keys row")
}

async fn maintenance_rows(
    pool: &sqlx::SqlitePool,
    key_id: &str,
) -> Vec<(
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
)> {
    sqlx::query_as(
        "SELECT operation_code, reason_code, reason_detail, status_before, status_after \
         FROM api_key_maintenance_records WHERE key_id = ? ORDER BY created_at ASC, rowid ASC",
    )
    .bind(key_id)
    .fetch_all(pool)
    .await
    .expect("read maintenance rows")
}

async fn depletion_row_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM api_key_low_quota_depletions")
        .fetch_one(pool)
        .await
        .expect("count depletion rows")
}

async fn seed_reconciliation_usage_row(
    pool: &sqlx::SqlitePool,
    token_id: &str,
    key_id: &str,
    period_code: &str,
    period_end: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, period_start, period_end,
            request_count, first_used_at, last_used_at, updated_at, settlement_mode
        ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
        "#,
    )
    .bind(token_id)
    .bind(key_id)
    .bind(period_code)
    .bind(format!("project-{token_id}"))
    .bind(format!("token:{token_id}"))
    .bind(period_end - 3_000)
    .bind(period_end)
    .bind(period_end - 3_100)
    .bind(period_end)
    .bind(period_end)
    .execute(pool)
    .await
    .expect("seed reconciliation usage row");
}

async fn seed_research_row(
    pool: &sqlx::SqlitePool,
    request_id: &str,
    token_id: &str,
    key_id: &str,
    period_code: &str,
    now: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_research (
            request_id, token_id, key_id, period_code, created_at, terminal_at, next_poll_at,
            updated_at
        ) VALUES (?, ?, ?, ?, ?, NULL, ?, ?)
        "#,
    )
    .bind(request_id)
    .bind(token_id)
    .bind(key_id)
    .bind(period_code)
    .bind(now - 900)
    .bind(now - 10)
    .bind(now - 900)
    .execute(pool)
    .await
    .expect("seed research row");
}

async fn enable_reconciliation_shadow_gate(proxy: &TavilyProxy) {
    let mut settings = proxy.get_system_settings().await.expect("load settings");
    settings.upstream_project_id_mode = UpstreamProjectIdMode::AccessToken;
    settings.api_rebalance_enabled = true;
    settings.api_rebalance_percent = 100;
    settings.rebalance_mcp_enabled = true;
    settings.rebalance_mcp_session_percent = 100;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("enable reconciliation shadow gate");
}

// =====================================================================================
// Pure field mapping (T10-parity contract; TDD red = functions did not exist)
// =====================================================================================

/// Given a full /account payload, When mapped, Then all four quota fields
/// follow the plan-L235 contract and plan_renewal_date lands on UTC
/// midnight.
#[test]
fn parse_maps_all_four_serpapi_account_fields() {
    let payload = account_payload("Active", 30_000, 12_345, "2026-10-23");
    let snapshot =
        parse_serpapi_account_snapshot(&payload, 1_700_000_000).expect("payload maps cleanly");
    let expected_renews_at = chrono::NaiveDate::from_ymd_opt(2026, 10, 23)
        .expect("valid fixture date")
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_utc()
        .timestamp();
    assert_eq!(snapshot.quota_limit, 30_000);
    assert_eq!(snapshot.quota_remaining, 12_345);
    assert_eq!(snapshot.quota_renews_at, Some(expected_renews_at));
    assert_eq!(snapshot.quota_synced_at, 1_700_000_000);
    assert_eq!(snapshot.account_status, "Active");
}

/// Given a payload without the plan fields, When mapped, Then a typed
/// QuotaDataMissing surfaces (never a silent zero-quota write).
#[test]
fn parse_rejects_payload_without_plan_fields() {
    let payload = json!({ "account_status": "Active", "plan_id": "bigdata" });
    let err = parse_serpapi_account_snapshot(&payload, 1_700_000_000)
        .expect_err("missing plan fields must be a typed error");
    assert!(
        matches!(err, ProxyError::QuotaDataMissing { .. }),
        "typed quota-data-missing error, got {err:?}"
    );
}

/// Given a payload with an unparsable plan_renewal_date, When mapped, Then
/// quota_renews_at degrades to None (the persistence seam COALESCE-keeps
/// the previously stored renewal) instead of failing the sync.
#[test]
fn parse_unparsable_plan_renewal_date_yields_none_renews() {
    let payload = account_payload("Active", 30_000, 17, "23/10/2026");
    let snapshot =
        parse_serpapi_account_snapshot(&payload, 1_700_000_000).expect("bad date degrades, no err");
    assert_eq!(snapshot.quota_renews_at, None);
    assert_eq!(snapshot.quota_remaining, 17);
}

/// Given remaining == 0 (run-out), When mapped, Then the snapshot persists
/// — a deliberate divergence from the Tavily both-zero rejection: an
/// exhausted SerpApi plan is a VALID sync result that must keep its
/// quota_renews_at recovery anchor.
#[test]
fn parse_accepts_zero_remaining_run_out_state() {
    let payload = account_payload("Active", 5_000, 0, "2026-10-23");
    let snapshot =
        parse_serpapi_account_snapshot(&payload, 1_700_000_000).expect("run-out is valid");
    assert_eq!(snapshot.quota_remaining, 0);
    assert_eq!(snapshot.quota_limit, 5_000);
}

// =====================================================================================
// Live adapter flows (injectable BackendTime clock per ADR 2026-06-15)
// =====================================================================================

/// Acceptance (plan L238): a serpapi quota_sync run against the mock
/// /account updates ALL FOUR key fields; the secret travels in the native
/// api_key query slot; the Tavily /usage lane is never touched.
#[tokio::test]
async fn serpapi_account_sync_updates_all_four_key_fields() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 30_000, 29_999, "2026-10-23"),
        0,
    )
    .await;
    let now = utc_ts("2026-09-30T06:00:00Z");
    let (proxy, _clock) = build_adapter_proxy("serpapi-sync-four-fields", &mocks, now).await;
    let secret = "serp-four-fields-a";
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &[secret.to_string()])
        .await
        .expect("seed serpapi key");
    let key_id = key_id_for(&proxy.key_store.pool, secret).await;

    let admission = Arc::new(RemoteAttemptAdmissionController::default());
    let snapshot = proxy
        .fetch_serpapi_account_for_sync(secret, &key_id, admission, false)
        .await
        .expect("serpapi /account fetch succeeds");
    let effects = proxy
        .record_serpapi_account_sync(&key_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("serpapi snapshot persists");

    let expected_renews_at = chrono::NaiveDate::from_ymd_opt(2026, 10, 23)
        .expect("valid fixture date")
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight")
        .and_utc()
        .timestamp();
    let (status, limit, remaining, synced_at, renews_at) =
        fetch_key_row(&proxy.key_store.pool, &key_id).await;
    assert_eq!(status, "active", "a healthy sync never changes status");
    assert_eq!(limit, Some(30_000));
    assert_eq!(remaining, Some(29_999));
    assert_eq!(synced_at, Some(now.timestamp()), "quota_synced_at = now");
    assert_eq!(renews_at, Some(expected_renews_at));
    assert!(!effects.renewed_to_active);
    assert!(!effects.account_note_recorded);
    assert_eq!(
        obs.last_account_api_key
            .lock()
            .expect("key lock")
            .as_deref(),
        Some(secret),
        "the secret travels in the native api_key query slot"
    );
    assert_eq!(obs.usage_requests.load(Ordering::SeqCst), 0);
    assert_eq!(obs.account_requests.load(Ordering::SeqCst), 1);
    assert_eq!(depletion_row_count(&proxy.key_store.pool).await, 0);
}

/// Failure scenario (plan L239): account_status "Suspended" produces a
/// maintenance-record NOTE and the key status stays UNCHANGED (never
/// auto-disabled). Repeated syncs with the same status do not stack rows;
/// a NEW status records a fresh note.
#[tokio::test]
async fn serpapi_account_status_suspended_records_note_without_disabling() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Suspended", 30_000, 500, "2026-10-23"),
        0,
    )
    .await;
    let (proxy, _clock) = build_adapter_proxy(
        "serpapi-suspended-note",
        &mocks,
        utc_ts("2026-09-30T06:00:00Z"),
    )
    .await;
    let secret = "serp-suspended-a";
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &[secret.to_string()])
        .await
        .expect("seed serpapi key");
    let key_id = key_id_for(&proxy.key_store.pool, secret).await;
    let snapshot = SerpapiAccountSnapshot {
        quota_limit: 30_000,
        quota_remaining: 500,
        quota_renews_at: Some(1_792_742_400),
        quota_synced_at: utc_ts("2026-09-30T06:00:00Z").timestamp(),
        account_status: "Suspended".to_string(),
    };

    let effects = proxy
        .record_serpapi_account_sync(&key_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("suspended snapshot persists");
    assert!(effects.account_note_recorded);
    let (status, _, _, _, _) = fetch_key_row(&proxy.key_store.pool, &key_id).await;
    assert_eq!(status, "active", "NOTE only — the key is NOT disabled");
    let rows = maintenance_rows(&proxy.key_store.pool, &key_id).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "serpapi_account_status_note");
    assert_eq!(rows[0].1.as_deref(), Some("account_status_not_active"));
    assert_eq!(
        rows[0].2.as_deref(),
        Some("account_status=Suspended source=quota_sync/serpapi")
    );
    assert_eq!(rows[0].3.as_deref(), Some("active"));
    assert_eq!(rows[0].4.as_deref(), Some("active"));
    let quarantines: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_quarantines WHERE key_id = ? AND cleared_at IS NULL",
    )
    .bind(&key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count quarantines");
    assert_eq!(quarantines, 0, "a non-Active status never quarantines");

    // Same status again => deduped (no stacked rows on the 24h cadence).
    let effects = proxy
        .record_serpapi_account_sync(&key_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("repeat sync persists");
    assert!(!effects.account_note_recorded);
    assert_eq!(
        maintenance_rows(&proxy.key_store.pool, &key_id).await.len(),
        1,
        "dedupe keeps one row per status"
    );

    // A NEW status records a fresh note.
    let degraded = SerpapiAccountSnapshot {
        account_status: "Degraded".to_string(),
        ..snapshot.clone()
    };
    let effects = proxy
        .record_serpapi_account_sync(&key_id, &degraded, "quota_sync/serpapi")
        .await
        .expect("status-change sync persists");
    assert!(effects.account_note_recorded);
    let rows = maintenance_rows(&proxy.key_store.pool, &key_id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].0, "serpapi_account_status_note");
    assert_eq!(
        rows[1].2.as_deref(),
        Some("account_status=Degraded source=quota_sync/serpapi")
    );
}

/// Acceptance (plan L235/L238): renewal-driven recovery — once
/// quota_renews_at passes, the EXISTING T9 reset_monthly(RenewalDate)
/// policy flips exhausted -> active during the sync, and the flip writes
/// the T18-parity auto_restore_active audit row (handoff #3).
#[tokio::test]
async fn serpapi_renewal_recovery_flips_exhausted_key_with_audit_row() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 30_000, 30_000, "2026-03-10"),
        0,
    )
    .await;
    let (proxy, _clock) = build_adapter_proxy(
        "serpapi-renewal-flip",
        &mocks,
        utc_ts("2026-03-15T12:00:00Z"),
    )
    .await;
    let secret = "serp-renewal-a";
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &[secret.to_string()])
        .await
        .expect("seed serpapi key");
    let key_id = key_id_for(&proxy.key_store.pool, secret).await;
    assert!(
        proxy
            .key_store
            .mark_quota_exhausted(secret)
            .await
            .expect("exhaust key"),
        "key starts exhausted"
    );

    let snapshot = SerpapiAccountSnapshot {
        quota_limit: 30_000,
        quota_remaining: 30_000,
        quota_renews_at: Some(utc_ts("2026-03-10T00:00:00Z").timestamp()),
        quota_synced_at: utc_ts("2026-03-15T12:00:00Z").timestamp(),
        account_status: "Active".to_string(),
    };
    let effects = proxy
        .record_serpapi_account_sync(&key_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("renewal sync persists");

    assert!(effects.renewed_to_active, "renewal date passed => flip");
    let (status, _, remaining, _, renews_at) = fetch_key_row(&proxy.key_store.pool, &key_id).await;
    assert_eq!(status, "active");
    assert_eq!(remaining, Some(30_000));
    assert_eq!(renews_at, Some(utc_ts("2026-03-10T00:00:00Z").timestamp()));
    let rows = maintenance_rows(&proxy.key_store.pool, &key_id).await;
    let restore = rows
        .iter()
        .find(|row| row.0 == "auto_restore_active")
        .expect("auto_restore_active audit row exists");
    assert_eq!(restore.1.as_deref(), Some("serpapi_renewal_recovery"));
    assert_eq!(restore.3.as_deref(), Some("exhausted"));
    assert_eq!(restore.4.as_deref(), Some("active"));
}

/// Complement of the renewal flip: strictly BEFORE quota_renews_at the
/// exhausted key STAYS exhausted (RenewalDate policy honored; no calendar
/// leaks from the Tavily UtcMonth policy).
#[tokio::test]
async fn serpapi_sync_before_renewal_date_keeps_key_exhausted() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 30_000, 0, "2026-03-10"),
        0,
    )
    .await;
    let (proxy, _clock) = build_adapter_proxy(
        "serpapi-pre-renewal-stay",
        &mocks,
        utc_ts("2026-03-01T12:00:00Z"),
    )
    .await;
    let secret = "serp-pre-renewal-a";
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &[secret.to_string()])
        .await
        .expect("seed serpapi key");
    let key_id = key_id_for(&proxy.key_store.pool, secret).await;
    assert!(
        proxy
            .key_store
            .mark_quota_exhausted(secret)
            .await
            .expect("exhaust key"),
        "key starts exhausted"
    );

    let snapshot = SerpapiAccountSnapshot {
        quota_limit: 30_000,
        quota_remaining: 0,
        quota_renews_at: Some(utc_ts("2026-03-10T00:00:00Z").timestamp()),
        quota_synced_at: utc_ts("2026-03-01T12:00:00Z").timestamp(),
        account_status: "Active".to_string(),
    };
    let effects = proxy
        .record_serpapi_account_sync(&key_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("pre-renewal sync persists");

    assert!(!effects.renewed_to_active);
    let (status, _, _, _, _) = fetch_key_row(&proxy.key_store.pool, &key_id).await;
    assert_eq!(status, "exhausted", "no flip before the renewal date");
    assert!(
        maintenance_rows(&proxy.key_store.pool, &key_id)
            .await
            .iter()
            .all(|row| row.0 != "auto_restore_active"),
        "no restore audit row without a flip"
    );
}

/// M8 (binding): the SerpApi response feeds ONLY quota fields — zero
/// api_key_low_quota_depletions writes even at remaining == 0, with a
/// Tavily positive control proving the assertion can fire.
#[tokio::test]
async fn m8_serpapi_sync_writes_zero_low_quota_depletions() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 5_000, 0, "2026-10-23"),
        0,
    )
    .await;
    let (proxy, _clock) = build_adapter_proxy(
        "m8-serpapi-zero-depletion",
        &mocks,
        utc_ts("2026-09-30T06:00:00Z"),
    )
    .await;
    let serpapi_secret = "serp-m8-zero";
    let tavily_secret = "tvly-m8-control";
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &[serpapi_secret.to_string()])
        .await
        .expect("seed serpapi key");
    proxy
        .key_store
        .sync_keys(ProviderId::Tavily, &[tavily_secret.to_string()])
        .await
        .expect("seed tavily key");
    let serpapi_key_id = key_id_for(&proxy.key_store.pool, serpapi_secret).await;
    let tavily_key_id = key_id_for(&proxy.key_store.pool, tavily_secret).await;

    let snapshot = SerpapiAccountSnapshot {
        quota_limit: 5_000,
        quota_remaining: 0,
        quota_renews_at: Some(utc_ts("2026-10-23T00:00:00Z").timestamp()),
        quota_synced_at: utc_ts("2026-09-30T06:00:00Z").timestamp(),
        account_status: "Active".to_string(),
    };
    proxy
        .record_serpapi_account_sync(&serpapi_key_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("serpapi run-out snapshot persists");
    assert_eq!(
        depletion_row_count(&proxy.key_store.pool).await,
        0,
        "M8: serpapi sync NEVER writes api_key_low_quota_depletions"
    );

    // Positive control: the Tavily seam CAN write a depletion row, so the
    // zero assertion above is a real detector, not a vacuous pass.
    sqlx::query("UPDATE api_keys SET quota_remaining = 5 WHERE id = ?")
        .bind(&tavily_key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed low tavily remaining");
    assert!(
        proxy
            .key_store
            .record_low_quota_depletion_if_needed(&tavily_key_id, 15)
            .await
            .expect("tavily depletion seam runs"),
        "positive control fires"
    );
    assert_eq!(depletion_row_count(&proxy.key_store.pool).await, 1);
    let tavily_provider_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_low_quota_depletions d \
         JOIN api_keys k ON k.id = d.key_id WHERE k.provider <> 'tavily'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count non-tavily depletion rows");
    assert_eq!(tavily_provider_rows, 0);
}

/// FU-B decision pin (T18 handoff #1): the M11 seam
/// record_quota_sync_sample binds limit as i64 and
/// api_key_quota_sync_samples.quota_limit is NOT NULL — NULL is
/// inexpressible through the seam without a migration (migrations are
/// FORBIDDEN in todo 19) and the brave Limit-0 call site lives in the
/// FORBIDDEN facade module. This PRAGMA pin locks the schema fact that
/// makes "keep the documented skip" the only ownership-legal semantics:
/// a brave month-window Limit 0 SKIP-WRITE preserves the fresh key's NULL
/// quota_limit (M11 "NEVER 0") instead of persisting a wrong 0.
#[tokio::test]
async fn fu_b_quota_sample_seam_cannot_express_null_limit() {
    let db_path = temp_db_path("fu-b-sample-seam-notnull");
    let db_str = db_path.to_string_lossy().to_string();
    let (backend_time, _clock) = BackendTime::manual(utc_ts("2026-09-30T06:00:00Z"));
    let store = KeyStore::new_with_time(&db_str, backend_time)
        .await
        .expect("store");
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT name, \"notnull\" FROM pragma_table_info('api_key_quota_sync_samples')",
    )
    .fetch_all(&store.pool)
    .await
    .expect("read samples table info");
    let quota_limit_notnull = rows
        .iter()
        .find(|(name, _)| name == "quota_limit")
        .expect("quota_limit column exists")
        .1;
    assert_eq!(
        quota_limit_notnull, 1,
        "quota_limit stays NOT NULL — the skip-write is the only NULL-honoring semantics \
         without a forbidden migration; brave Limit-0 keys keep NULL quota_limit (M11)"
    );
}

// =====================================================================================
// Zero-leak guard seams (cycle-1 RED set: these fail until the M4 guards land)
// =====================================================================================

/// M4 FIRST filter: the Tavily cold/hot pending selections must enumerate
/// ONLY provider='tavily' rows once non-tavily keys exist, while the
/// provider-scoped seams enumerate exactly their own provider.
#[tokio::test]
async fn pending_quota_sync_lists_separate_providers() {
    let db_path = temp_db_path("pending-list-provider-separation");
    let db_str = db_path.to_string_lossy().to_string();
    let now = utc_ts("2026-09-30T06:00:00Z");
    let (backend_time, _clock) = BackendTime::manual(now);
    let store = KeyStore::new_with_time(&db_str, backend_time)
        .await
        .expect("store");
    store
        .sync_keys(ProviderId::Tavily, &["tvly-pending-a".to_string()])
        .await
        .expect("seed tavily key");
    store
        .sync_keys(ProviderId::Serpapi, &["serp-pending-a".to_string()])
        .await
        .expect("seed serpapi key");
    store
        .sync_keys(ProviderId::Exa, &["exa-pending-a".to_string()])
        .await
        .expect("seed exa key");
    store
        .sync_keys(ProviderId::Brave, &["brv-pending-a".to_string()])
        .await
        .expect("seed brave key");

    let tavily_id = key_id_for(&store.pool, "tvly-pending-a").await;
    let serpapi_id = key_id_for(&store.pool, "serp-pending-a").await;

    // Cold lists: fresh keys (quota_synced_at NULL) are pending everywhere.
    let tavily_cold = store
        .list_keys_pending_quota_sync(24 * 3600)
        .await
        .expect("tavily cold list");
    assert_eq!(
        tavily_cold,
        vec![tavily_id.clone()],
        "M4: the Tavily cold lane enumerates ONLY tavily keys"
    );
    let serpapi_cold = store
        .list_keys_pending_provider_quota_sync(ProviderId::Serpapi, 24 * 3600)
        .await
        .expect("serpapi cold list");
    assert_eq!(serpapi_cold, vec![serpapi_id.clone()]);
    let exa_cold = store
        .list_keys_pending_provider_quota_sync(ProviderId::Exa, 24 * 3600)
        .await
        .expect("exa cold list");
    let exa_id = key_id_for(&store.pool, "exa-pending-a").await;
    assert_eq!(
        exa_cold,
        vec![exa_id],
        "generic seam enumerates exactly its own provider — the exa/brave \
         never-polled guarantee is lane composition (no exa/brave lane or \
         job type exists; pinned by the bin registry test [&str; 9])"
    );

    // Hot lists: mark every key recently used, then assert the same
    // separation on the hot thresholds.
    sqlx::query("UPDATE api_keys SET last_used_at = ?")
        .bind(now.timestamp() - 600)
        .execute(&store.pool)
        .await
        .expect("mark keys recently used");
    let tavily_hot = store
        .list_keys_pending_hot_quota_sync(2 * 3600, 15 * 60)
        .await
        .expect("tavily hot list");
    assert_eq!(
        tavily_hot,
        vec![tavily_id],
        "M4: the Tavily hot lane enumerates ONLY tavily keys"
    );
    let serpapi_hot = store
        .list_keys_pending_hot_provider_quota_sync(ProviderId::Serpapi, 2 * 3600, 15 * 60)
        .await
        .expect("serpapi hot list");
    assert_eq!(serpapi_hot, vec![serpapi_id]);
}

/// Zero-leak: the reconciliation key enumeration that feeds
/// fetch_upstream_project_usage (Bearer secret -> TAVILY /usage) excludes
/// KNOWN non-tavily keys, while legacy dangling-key usage rows stay
/// included (compat pin for the NOT-EXISTS guard shape — dozens of golden
/// fixtures seed usage rows without api_keys rows).
#[tokio::test]
async fn reconciliation_key_enumeration_excludes_non_tavily_keys() {
    let db_path = temp_db_path("reconciliation-key-guard");
    let db_str = db_path.to_string_lossy().to_string();
    let now = utc_ts("2026-09-30T06:00:00Z");
    let (backend_time, _clock) = BackendTime::manual(now);
    let store = KeyStore::new_with_time(&db_str, backend_time)
        .await
        .expect("store");
    store
        .sync_keys(ProviderId::Tavily, &["tvly-recon-a".to_string()])
        .await
        .expect("seed tavily key");
    store
        .sync_keys(ProviderId::Serpapi, &["serp-recon-a".to_string()])
        .await
        .expect("seed serpapi key");
    let tavily_id = key_id_for(&store.pool, "tvly-recon-a").await;
    let serpapi_id = key_id_for(&store.pool, "serp-recon-a").await;
    let token = "recon-guard-token";
    let period = "2026-09-30/S1";
    seed_reconciliation_usage_row(
        &store.pool,
        token,
        &tavily_id,
        period,
        now.timestamp() - 900,
    )
    .await;
    seed_reconciliation_usage_row(
        &store.pool,
        token,
        &serpapi_id,
        period,
        now.timestamp() - 900,
    )
    .await;
    seed_reconciliation_usage_row(
        &store.pool,
        token,
        "ghost-legacy-key-without-row",
        period,
        now.timestamp() - 900,
    )
    .await;

    let batch = store
        .reconciliation_key_ids_batch(&[(token.to_string(), period.to_string())])
        .await
        .expect("enumerate reconciliation keys");
    let mut key_ids = batch
        .get(&(token.to_string(), period.to_string()))
        .expect("candidate keys present")
        .clone();
    key_ids.sort();
    let mut expected = vec![tavily_id, "ghost-legacy-key-without-row".to_string()];
    expected.sort();
    assert_eq!(
        key_ids, expected,
        "non-tavily keys never enter the reconciliation engine; dangling legacy rows stay included"
    );
    assert!(
        !key_ids.contains(&serpapi_id),
        "zero-leak: the serpapi secret can never reach TAVILY_USAGE_BASE /usage"
    );
}

/// Zero-leak: research-drain candidate selection (r.key_id drives the
/// Tavily /research poll with that key's Bearer secret) excludes known
/// non-tavily keys while tavily rows keep polling.
#[tokio::test]
async fn research_drain_candidates_exclude_non_tavily_keys() {
    let db_path = temp_db_path("research-drain-key-guard");
    let db_str = db_path.to_string_lossy().to_string();
    let now = utc_ts("2026-09-30T06:00:00Z");
    let (backend_time, _clock) = BackendTime::manual(now);
    let store = KeyStore::new_with_time(&db_str, backend_time)
        .await
        .expect("store");
    store
        .sync_keys(ProviderId::Tavily, &["tvly-research-a".to_string()])
        .await
        .expect("seed tavily key");
    store
        .sync_keys(ProviderId::Serpapi, &["serp-research-a".to_string()])
        .await
        .expect("seed serpapi key");
    let tavily_id = key_id_for(&store.pool, "tvly-research-a").await;
    let serpapi_id = key_id_for(&store.pool, "serp-research-a").await;
    let ts = now.timestamp();
    for (token, key_id, request_id) in [
        ("research-guard-token-tvly", tavily_id.as_str(), "req-tvly"),
        ("research-guard-token-serp", serpapi_id.as_str(), "req-serp"),
    ] {
        seed_reconciliation_usage_row(&store.pool, token, key_id, "2026-09-30/S1", ts - 900).await;
        seed_research_row(&store.pool, request_id, token, key_id, "2026-09-30/S1", ts).await;
    }

    let page = store
        .next_upstream_reconciliation_research_candidates(10)
        .await
        .expect("research candidates page");
    let request_ids: Vec<String> = page
        .candidates
        .iter()
        .map(|candidate| candidate.request_id.clone())
        .collect();
    assert_eq!(
        request_ids,
        vec!["req-tvly".to_string()],
        "zero-leak: only the tavily-keyed research row is pollable"
    );
    assert!(
        page.candidates
            .iter()
            .all(|candidate| candidate.key_id == tavily_id),
        "candidate key ids carry only the tavily key"
    );
}

/// Zero-leak (wake-calc leg): a research row owned by a non-tavily key
/// must NOT make the drain representative available — otherwise the guard
/// at candidate level would leave a permanently no-op waking job.
#[tokio::test]
async fn research_drain_availability_ignores_non_tavily_keys() {
    let db_path = temp_db_path("research-drain-availability-guard");
    let db_str = db_path.to_string_lossy().to_string();
    let now = utc_ts("2026-09-30T06:00:00Z");
    let (backend_time, _clock) = BackendTime::manual(now);
    let proxy = TavilyProxy::with_options_and_time(
        Vec::<String>::new(),
        "http://127.0.0.1:9",
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("create proxy");
    enable_reconciliation_shadow_gate(&proxy).await;
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &["serp-availability-a".to_string()])
        .await
        .expect("seed serpapi key");
    let serpapi_id = key_id_for(&proxy.key_store.pool, "serp-availability-a").await;
    let ts = now.timestamp();
    seed_reconciliation_usage_row(
        &proxy.key_store.pool,
        "availability-guard-token",
        &serpapi_id,
        "2026-09-30/S1",
        ts - 900,
    )
    .await;
    seed_research_row(
        &proxy.key_store.pool,
        "req-availability-serp",
        "availability-guard-token",
        &serpapi_id,
        "2026-09-30/S1",
        ts,
    )
    .await;

    assert_eq!(
        proxy
            .key_store
            .upstream_reconciliation_research_drain_available_at()
            .await
            .expect("read drain availability"),
        None,
        "zero-leak: a non-tavily research row never wakes the drain job"
    );
}

/// M4 C4a (mandatory): Tavily-/usage and SerpApi-/account jobs contend for
/// the SINGLE remote-attempt lease — exactly one outbound in flight, the
/// second deferred until the first completes, both eventually succeed.
#[tokio::test]
async fn c4a_single_remote_lease_serializes_tavily_and_serpapi_fetches() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 30_000, 42, "2026-10-23"),
        200,
    )
    .await;
    let (proxy, _clock) = build_adapter_proxy(
        "c4a-lease-contention",
        &mocks,
        utc_ts("2026-09-30T06:00:00Z"),
    )
    .await;
    proxy
        .key_store
        .sync_keys(ProviderId::Tavily, &["tvly-lease-a".to_string()])
        .await
        .expect("seed tavily key");
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &["serp-lease-a".to_string()])
        .await
        .expect("seed serpapi key");
    let tavily_id = key_id_for(&proxy.key_store.pool, "tvly-lease-a").await;
    let serpapi_id = key_id_for(&proxy.key_store.pool, "serp-lease-a").await;

    // ONE shared admission controller = the single remote-attempt lease the
    // scheduler hands both job families (remote_attempt_admission_for_state).
    let admission = Arc::new(RemoteAttemptAdmissionController::default());
    let tavily_base = mocks.base.clone();
    let proxy_tavily = proxy.clone();
    let admission_tavily = admission.clone();
    let tavily_task = tokio::spawn(async move {
        proxy_tavily
            .fetch_usage_quota_for_sync_secret_with_admission(
                "tvly-lease-a",
                &tavily_base,
                &tavily_id,
                admission_tavily,
                false,
            )
            .await
    });
    let proxy_serpapi = proxy.clone();
    let admission_serpapi = admission.clone();
    let serpapi_task = tokio::spawn(async move {
        proxy_serpapi
            .fetch_serpapi_account_for_sync("serp-lease-a", &serpapi_id, admission_serpapi, false)
            .await
    });
    let (tavily_result, serpapi_result) = tokio::join!(tavily_task, serpapi_task);
    let (limit, remaining) = tavily_result
        .expect("tavily task joins")
        .expect("tavily /usage fetch succeeds despite contention");
    assert_eq!((limit, remaining), (100, 90));
    let snapshot = serpapi_result
        .expect("serpapi task joins")
        .expect("serpapi /account fetch succeeds despite contention");
    assert_eq!(snapshot.quota_remaining, 42);

    assert_eq!(
        obs.peak_inflight.load(Ordering::SeqCst),
        1,
        "exactly one outbound request in flight at any instant"
    );
    assert_eq!(obs.usage_requests.load(Ordering::SeqCst), 1);
    assert_eq!(obs.account_requests.load(Ordering::SeqCst), 1);
    let events = obs.events.lock().expect("events lock").clone();
    assert_eq!(events.len(), 4, "{events:?}");
    let first_exit = events
        .iter()
        .position(|event| event.starts_with("exit:"))
        .expect("an exit event exists");
    let second_enter = events
        .iter()
        .rposition(|event| event.starts_with("enter:"))
        .expect("a second enter event exists");
    assert!(
        first_exit < second_enter,
        "the second outbound is DEFERRED until the first completes: {events:?}"
    );
}

/// Zero-leak (probe leg): probe_api_key_quota refuses a STORED non-tavily
/// secret before any egress, while unstored candidate secrets (the
/// registration flow) keep probing through — byte-stable Tavily behavior.
#[tokio::test]
async fn tavily_probe_guard_rejects_stored_foreign_secret() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 30_000, 42, "2026-10-23"),
        0,
    )
    .await;
    let (proxy, _clock) =
        build_adapter_proxy("tavily-probe-guard", &mocks, utc_ts("2026-09-30T06:00:00Z")).await;
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &["serp-probe-guard".to_string()])
        .await
        .expect("seed serpapi key");

    let err = proxy
        .probe_api_key_quota("serp-probe-guard", &mocks.base)
        .await
        .expect_err("stored serpapi secret must be rejected by the tavily probe");
    let message = err.to_string();
    assert!(
        message.contains("provider_guard_rejected"),
        "typed guard rejection, got {message}"
    );
    assert_eq!(
        obs.usage_requests.load(Ordering::SeqCst),
        0,
        "zero-leak: no /usage request ever leaves for the foreign secret"
    );

    // Registration compatibility: an UNSTORED secret still probes through.
    let (limit, remaining) = proxy
        .probe_api_key_quota("unstored-candidate-secret", &mocks.base)
        .await
        .expect("unstored candidate secret probes (registration flow)");
    assert_eq!((limit, remaining), (100, 90));
    assert_eq!(obs.usage_requests.load(Ordering::SeqCst), 1);
}

/// Scheduler-never-polls confirmation (plan L235): after running the
/// serpapi adapter end to end, the exa/brave observation endpoints show
/// ZERO polling — no lane, no job type, no incidental fetch touches them.
#[tokio::test]
async fn exa_and_brave_endpoints_are_never_polled_by_quota_sync() {
    let obs = Arc::new(EgressObservation::default());
    let mocks = spawn_adapter_mock_upstreams(
        obs.clone(),
        account_payload("Active", 30_000, 7, "2026-10-23"),
        0,
    )
    .await;
    let (proxy, _clock) = build_adapter_proxy(
        "exa-brave-never-polled",
        &mocks,
        utc_ts("2026-09-30T06:00:00Z"),
    )
    .await;
    proxy
        .key_store
        .sync_keys(ProviderId::Exa, &["exa-never-polled".to_string()])
        .await
        .expect("seed exa key");
    proxy
        .key_store
        .sync_keys(ProviderId::Brave, &["brv-never-polled".to_string()])
        .await
        .expect("seed brave key");
    proxy
        .key_store
        .sync_keys(ProviderId::Serpapi, &["serp-never-polled".to_string()])
        .await
        .expect("seed serpapi key");
    let serpapi_id = key_id_for(&proxy.key_store.pool, "serp-never-polled").await;

    // NOTE: the provider-scoped list seam is GENERIC (it would enumerate
    // exa/brave rows if called) — the "never polled" guarantee is that NO
    // lane or dispatch arm exists for exa/brave: the pending-selection
    // wrapper is only ever invoked with Serpapi (scheduler child), and the
    // bin registry pin asserts REMOTE_IO_SCHEDULED_JOB_TYPES holds exactly
    // the 9 known types (no exa/brave job type). What IS asserted here: a
    // full serpapi sync cycle against a DB seeded with exa+brave keys
    // produces ZERO requests on their endpoints.
    assert_eq!(
        proxy
            .list_keys_pending_provider_quota_sync(ProviderId::Exa, 24 * 3600)
            .await
            .expect("exa list")
            .len(),
        1,
        "generic seam enumerates exactly its own provider (no cross-provider leakage)"
    );
    let admission = Arc::new(RemoteAttemptAdmissionController::default());
    let snapshot = proxy
        .fetch_serpapi_account_for_sync("serp-never-polled", &serpapi_id, admission, false)
        .await
        .expect("serpapi fetch");
    proxy
        .record_serpapi_account_sync(&serpapi_id, &snapshot, "quota_sync/serpapi")
        .await
        .expect("serpapi record");

    assert_eq!(obs.exa_requests.load(Ordering::SeqCst), 0);
    assert_eq!(obs.brave_requests.load(Ordering::SeqCst), 0);
    assert_eq!(obs.account_requests.load(Ordering::SeqCst), 1);
}

//! Plan todo 19 bin coverage: `quota_sync/serpapi` dispatch through the
//! real scheduled-job machinery and the M4 zero-leak hard guards at every
//! Tavily egress entry point the bin can drive in-process:
//! scheduler hot/cold lane pending lists, the quota_sync dispatch arm
//! (gate-level provider guard), the admin manual sync-usage HTTP surface
//! (T10 dispatch, re-asserted for zero /usage leakage), and the exa/brave
//! never-polled counters. Reconciliation-engine and probe-secret legs are
//! pinned in the lib matrix (tests::provider_quota_sync_adapters) against
//! the same seams the bin drives here.

use super::core_support_and_parsing::*;
use super::*;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicUsize;

const TAVILY_SECRET: &str = "tvly-dispatch-guard-a";
const SERPAPI_SECRET: &str = "serp-dispatch-guard-a";
const EXA_SECRET: &str = "exa-dispatch-guard-a";
const BRAVE_SECRET: &str = "brv-dispatch-guard-a";

#[derive(Default)]
struct DispatchEgress {
    usage_requests: AtomicUsize,
    usage_bearers: StdMutex<Vec<String>>,
    account_requests: AtomicUsize,
    last_account_api_key: StdMutex<Option<String>>,
    // Shared-Arc counters handed to the inert exa/brave mocks so the
    // never-polled assertions read the SAME atomics the mocks bump.
    exa_counter: Arc<AtomicUsize>,
    brave_counter: Arc<AtomicUsize>,
}

impl DispatchEgress {
    fn new_arc() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

async fn bind_dispatch_ephemeral(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

/// Tavily /usage mock recording EVERY Authorization bearer it receives —
/// the zero-leak assertion surface (M4 C4a: ZERO non-tavily keys, ever).
async fn spawn_usage_mock(obs: Arc<DispatchEgress>) -> SocketAddr {
    bind_dispatch_ephemeral(
        Router::new()
            .route(
                "/usage",
                get(move |headers: HeaderMap| {
                    let obs = obs.clone();
                    async move {
                        obs.usage_requests.fetch_add(1, Ordering::SeqCst);
                        if let Some(value) = headers
                            .get(axum::http::header::AUTHORIZATION)
                            .and_then(|header| header.to_str().ok())
                        {
                            obs.usage_bearers
                                .lock()
                                .expect("bearer lock")
                                .push(value.to_string());
                        }
                        Json(json!({ "key": { "limit": 100, "usage": 10 } }))
                    }
                }),
            )
            .with_state(()),
    )
    .await
}

/// SerpApi /account mock (free/uncounted probe shape, T13/T6 contract).
async fn spawn_account_mock(obs: Arc<DispatchEgress>) -> SocketAddr {
    bind_dispatch_ephemeral(
        Router::new()
            .route(
                "/account",
                get(move |Query(query): Query<HashMap<String, String>>| {
                    let obs = obs.clone();
                    async move {
                        obs.account_requests.fetch_add(1, Ordering::SeqCst);
                        *obs.last_account_api_key.lock().expect("account key lock") =
                            query.get("api_key").cloned();
                        Json(json!({
                            "account_status": "Active",
                            "plan_id": "bigdata",
                            "searches_per_month": 30_000_i64,
                            "total_searches_left": 29_000_i64,
                            "plan_searches_left": 29_000_i64,
                            "plan_renewal_date": "2026-10-23",
                            "this_hour_searches": 1,
                            "account_rate_limit_per_hour": 6000
                        }))
                    }
                }),
            )
            .with_state(()),
    )
    .await
}

/// Inert exa/brave counters — any polling of these providers by quota-sync
/// machinery would show up here (plan L235: Exa/Brave get NO active
/// polling).
async fn spawn_counter_mock(counter: Arc<AtomicUsize>) -> SocketAddr {
    bind_dispatch_ephemeral(
        Router::new()
            .fallback(any(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }))
            .with_state(()),
    )
    .await
}

struct DispatchHarness {
    state: Arc<AppState>,
    admin_addr: SocketAddr,
    client: Client,
    pool: sqlx::SqlitePool,
    obs: Arc<DispatchEgress>,
    db_path: std::path::PathBuf,
}

impl Drop for DispatchHarness {
    fn drop(&mut self) {
        if let Some(dir) = self.db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

async fn spawn_dispatch_harness(prefix: &str) -> DispatchHarness {
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    let obs = DispatchEgress::new_arc();
    let usage_addr = spawn_usage_mock(obs.clone()).await;
    let account_addr = spawn_account_mock(obs.clone()).await;
    let exa_addr = spawn_counter_mock(obs.exa_counter.clone()).await;
    let brave_addr = spawn_counter_mock(obs.brave_counter.clone()).await;
    let providers = tavily_hikari::ProviderBootstrap {
        serpapi_upstream: format!("http://{account_addr}"),
        serpapi_mcp_upstream: format!("http://{account_addr}/mcp"),
        exa_upstream: format!("http://{exa_addr}"),
        brave_upstream: format!("http://{brave_addr}"),
        serpapi_keys: vec![SERPAPI_SECRET.to_string()],
        exa_keys: vec![EXA_SECRET.to_string()],
        brave_keys: vec![BRAVE_SECRET.to_string()],
    };
    let proxy = TavilyProxy::with_providers_in_ha_mode(
        vec![TAVILY_SECRET.to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        providers,
        tavily_hikari::HaMode::Single,
    )
    .await
    .expect("proxy created");
    let usage_base = format!("http://{usage_addr}");
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: true,
        usage_base: usage_base.clone(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });
    let app = Router::new()
        .route("/api/keys", get(list_keys))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .with_state(state.clone());
    let admin_addr = bind_dispatch_ephemeral(app).await;
    let pool = connect_sqlite_test_pool(&db_str).await;
    DispatchHarness {
        state,
        admin_addr,
        client: Client::new(),
        pool,
        obs,
        db_path,
    }
}

impl DispatchHarness {
    async fn key_id(&self, secret: &str) -> String {
        sqlx::query_scalar("SELECT id FROM api_keys WHERE api_key = ?")
            .bind(secret)
            .fetch_one(&self.pool)
            .await
            .expect("key id lookup")
    }

    /// Enqueue + claim + dispatch one job through the REAL scheduler entry
    /// (run_manual_claimed_job), returning (succeeded, job row).
    async fn dispatch_job(&self, job_type: &str, key_id: &str) -> (bool, String, Option<String>) {
        let queued = self
            .state
            .proxy
            .scheduled_job_enqueue(job_type, "manual", Some(key_id), 1)
            .await
            .expect("enqueue job");
        let claim = self
            .state
            .proxy
            .scheduled_job_mark_running(queued.job_id)
            .await
            .expect("claim job")
            .expect("job becomes running");
        let succeeded = run_manual_claimed_job(
            self.state.clone(),
            job_type.to_string(),
            Some(key_id.to_string()),
            ClaimedScheduledJob {
                job_id: claim.id,
                claim_generation: claim.claim_generation,
                _job_execution_gate: None,
            },
            None,
            true,
        )
        .await;
        let (status, message): (String, Option<String>) =
            sqlx::query_as("SELECT status, message FROM scheduled_jobs WHERE id = ?")
                .bind(claim.id)
                .fetch_one(&self.pool)
                .await
                .expect("read job row");
        (succeeded, status, message)
    }

    async fn key_row(
        &self,
        key_id: &str,
    ) -> (String, Option<i64>, Option<i64>, Option<i64>, Option<i64>) {
        sqlx::query_as(
            "SELECT status, quota_limit, quota_remaining, quota_synced_at, quota_renews_at \
             FROM api_keys WHERE id = ?",
        )
        .bind(key_id)
        .fetch_one(&self.pool)
        .await
        .expect("read key row")
    }

    async fn post_status(&self, path: &str) -> u16 {
        self.client
            .post(format!("http://{}{}", self.admin_addr, path))
            .send()
            .await
            .expect("request completes")
            .status()
            .as_u16()
    }
}

/// Acceptance (plan L238): a `quota_sync/serpapi` job run against the mock
/// /account endpoint updates ALL FOUR key fields through the real dispatch
/// arm, with the lease/gate geometry of the Tavily family; the Tavily
/// /usage mock is never touched.
#[tokio::test]
async fn serpapi_quota_sync_job_updates_all_four_fields_via_dispatch() {
    let harness = spawn_dispatch_harness("serpapi-dispatch-four-fields").await;
    let key_id = harness.key_id(SERPAPI_SECRET).await;

    let (succeeded, status, message) = harness
        .dispatch_job(QUOTA_SYNC_SERPAPI_JOB_TYPE, &key_id)
        .await;
    assert!(succeeded, "job succeeds: {status} {message:?}");
    assert_eq!(status, "success");
    let message = message.expect("success message");
    assert!(
        message.contains("limit=30000") && message.contains("remaining=29000"),
        "job message carries the fetched quota: {message}"
    );

    let (key_status, limit, remaining, synced_at, renews_at) = harness.key_row(&key_id).await;
    assert_eq!(key_status, "active");
    assert_eq!(limit, Some(30_000));
    assert_eq!(remaining, Some(29_000));
    assert!(
        synced_at.is_some_and(|value| value > 0),
        "quota_synced_at stamped: {synced_at:?}"
    );
    let expected_renews_at = NaiveDate::parse_from_str("2026-10-23", "%Y-%m-%d")
        .expect("fixture date parses")
        .and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        .and_utc()
        .timestamp();
    assert_eq!(renews_at, Some(expected_renews_at));
    assert_eq!(harness.obs.account_requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        harness
            .obs
            .last_account_api_key
            .lock()
            .expect("key lock")
            .as_deref(),
        Some(SERPAPI_SECRET)
    );
    assert_eq!(
        harness.obs.usage_requests.load(Ordering::SeqCst),
        0,
        "zero-leak: the serpapi lane never touches the Tavily /usage mock"
    );
}

/// M4 zero-leak centerpiece: the Tavily quota_sync lane (manual job shape
/// of hot/cold) FAILS CLOSED on every stored non-tavily key — typed
/// provider_guard_rejected job error, and the /usage mock NEVER receives a
/// non-tavily bearer — while the tavily key keeps its byte-stable success.
#[tokio::test]
async fn tavily_quota_sync_lane_fails_closed_on_foreign_keys() {
    let harness = spawn_dispatch_harness("tavily-lane-guard-sweep").await;

    for secret in [SERPAPI_SECRET, EXA_SECRET, BRAVE_SECRET] {
        let key_id = harness.key_id(secret).await;
        let (succeeded, status, message) = harness.dispatch_job("quota_sync", &key_id).await;
        assert!(
            !succeeded,
            "the Tavily lane must refuse foreign key {secret}"
        );
        assert_eq!(status, "error");
        let message = message.expect("guard message");
        assert!(
            message.contains("provider_guard_rejected"),
            "typed guard rejection for {secret}, got: {message}"
        );
    }
    assert_eq!(
        harness.obs.usage_requests.load(Ordering::SeqCst),
        0,
        "zero-leak: no /usage request left for any foreign key"
    );
    assert!(
        harness
            .obs
            .usage_bearers
            .lock()
            .expect("bearer lock")
            .is_empty()
    );

    // Positive control: the tavily key still syncs byte-stably.
    let tavily_id = harness.key_id(TAVILY_SECRET).await;
    let (succeeded, _, message) = harness.dispatch_job("quota_sync", &tavily_id).await;
    assert!(succeeded, "tavily lane unchanged: {message:?}");
    let bearers = harness
        .obs
        .usage_bearers
        .lock()
        .expect("bearer lock")
        .clone();
    assert_eq!(
        bearers,
        vec![format!("Bearer {TAVILY_SECRET}")],
        "the /usage mock received ONLY the tavily key"
    );
    let (status, limit, remaining, _, _) = harness.key_row(&tavily_id).await;
    assert_eq!(status, "active");
    assert_eq!((limit, remaining), (Some(100), Some(90)));
}

/// Symmetric guard: the serpapi /account lane refuses the tavily (and
/// exa/brave) keys before any egress — a Tavily secret must never be sent
/// to a third-party REST base either.
#[tokio::test]
async fn serpapi_quota_sync_lane_fails_closed_on_foreign_keys() {
    let harness = spawn_dispatch_harness("serpapi-lane-guard-sweep").await;

    for secret in [TAVILY_SECRET, EXA_SECRET, BRAVE_SECRET] {
        let key_id = harness.key_id(secret).await;
        let (succeeded, status, message) = harness
            .dispatch_job(QUOTA_SYNC_SERPAPI_JOB_TYPE, &key_id)
            .await;
        assert!(
            !succeeded,
            "the serpapi lane must refuse foreign key {secret}"
        );
        assert_eq!(status, "error");
        let message = message.expect("guard message");
        assert!(
            message.contains("provider_guard_rejected"),
            "typed guard rejection for {secret}, got: {message}"
        );
    }
    assert_eq!(
        harness.obs.account_requests.load(Ordering::SeqCst),
        0,
        "zero-leak: no /account request left for any foreign key"
    );
}

/// M4 FIRST filter at the scheduler surface: the hot/cold pending selection
/// functions the lanes call enumerate ONLY tavily keys through the legacy
/// Tavily wrappers, and exactly their own provider through the scoped
/// seams — so a non-tavily key can never even be enqueued onto a Tavily
/// lane.
#[tokio::test]
async fn lane_pending_selections_enumerate_only_own_provider() {
    let harness = spawn_dispatch_harness("lane-pending-selection").await;
    let tavily_id = harness.key_id(TAVILY_SECRET).await;
    let serpapi_id = harness.key_id(SERPAPI_SECRET).await;

    // Hot lane precondition: every key looks recently used.
    let now = Utc::now().timestamp();
    sqlx::query("UPDATE api_keys SET last_used_at = ?")
        .bind(now - 600)
        .execute(&harness.pool)
        .await
        .expect("mark keys recently used");

    let cold = harness
        .state
        .proxy
        .list_keys_pending_quota_sync(24 * 3600)
        .await
        .expect("cold list");
    assert_eq!(
        cold,
        vec![tavily_id.clone()],
        "M4: the Tavily cold lane enumerates ONLY tavily keys"
    );
    let hot = harness
        .state
        .proxy
        .list_keys_pending_hot_quota_sync(2 * 3600, 15 * 60)
        .await
        .expect("hot list");
    assert_eq!(
        hot,
        vec![tavily_id],
        "M4: the Tavily hot lane enumerates ONLY tavily keys"
    );
    let serpapi_cold = harness
        .state
        .proxy
        .list_keys_pending_provider_quota_sync(
            tavily_hikari::providers::ProviderId::Serpapi,
            24 * 3600,
        )
        .await
        .expect("serpapi cold list");
    assert_eq!(serpapi_cold, vec![serpapi_id.clone()]);
    let serpapi_hot = harness
        .state
        .proxy
        .list_keys_pending_hot_provider_quota_sync(
            tavily_hikari::providers::ProviderId::Serpapi,
            2 * 3600,
            15 * 60,
        )
        .await
        .expect("serpapi hot list");
    assert_eq!(serpapi_hot, vec![serpapi_id]);
}

/// Admin manual sync-usage entry point (admin_auth.rs:348 path) re-asserted
/// for zero-leak: foreign keys take their T10 provider arms (serpapi live
/// snapshot via /account, brave passive snapshot, exa 409) and the Tavily
/// /usage mock receives NOTHING; exa/brave upstream counters stay at zero
/// (no polling of any kind).
#[tokio::test]
async fn admin_manual_sync_usage_keeps_tavily_usage_free_of_foreign_keys() {
    let harness = spawn_dispatch_harness("admin-sync-usage-sweep").await;

    let serpapi_id = harness.key_id(SERPAPI_SECRET).await;
    let exa_id = harness.key_id(EXA_SECRET).await;
    let brave_id = harness.key_id(BRAVE_SECRET).await;

    let serpapi_status = harness
        .post_status(&format!("/api/keys/{serpapi_id}/sync-usage"))
        .await;
    assert_eq!(
        serpapi_status, 200,
        "serpapi manual sync returns the live snapshot (T10 arm)"
    );
    let exa_status = harness
        .post_status(&format!("/api/keys/{exa_id}/sync-usage"))
        .await;
    assert_eq!(exa_status, 409, "exa stays typed-unsupported (T10 arm)");
    let brave_status = harness
        .post_status(&format!("/api/keys/{brave_id}/sync-usage"))
        .await;
    assert_eq!(
        brave_status, 200,
        "brave manual sync returns the passive snapshot (T10 arm)"
    );

    assert_eq!(
        harness.obs.usage_requests.load(Ordering::SeqCst),
        0,
        "zero-leak: manual sync of foreign keys never touches Tavily /usage"
    );
    assert_eq!(harness.obs.account_requests.load(Ordering::SeqCst), 1);
    assert_eq!(harness.obs.exa_counter.load(Ordering::SeqCst), 0);
    assert_eq!(harness.obs.brave_counter.load(Ordering::SeqCst), 0);
}

/// Registry pin (M4): `quota_sync/serpapi` is the ONE new entry in the
/// fixed REMOTE_IO_SCHEDULED_JOB_TYPES array ([&str; 8] -> [&str; 9]) and
/// rides the shared single remote-attempt lease while keeping the
/// durable-write DB execution gate discipline of the Tavily family.
#[test]
fn serpapi_job_type_registered_as_remote_io_with_db_gate() {
    assert_eq!(REMOTE_IO_SCHEDULED_JOB_TYPES.len(), 9);
    assert!(REMOTE_IO_SCHEDULED_JOB_TYPES.contains(&QUOTA_SYNC_SERPAPI_JOB_TYPE));
    assert_eq!(QUOTA_SYNC_SERPAPI_JOB_TYPE, "quota_sync/serpapi");
    assert!(scheduled_job_uses_remote_io(QUOTA_SYNC_SERPAPI_JOB_TYPE));
    assert!(scheduled_job_uses_db_execution_gate(
        QUOTA_SYNC_SERPAPI_JOB_TYPE
    ));
    // No exa/brave job type exists anywhere in the registry (never polled).
    assert!(
        !REMOTE_IO_SCHEDULED_JOB_TYPES
            .iter()
            .any(|job_type| job_type.contains("exa") || job_type.contains("brave")),
        "exa/brave have NO scheduler lane or job type"
    );
}

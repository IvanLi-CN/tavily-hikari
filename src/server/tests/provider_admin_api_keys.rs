//! Plan todo 10 bin coverage: provider-aware admin key CRUD/filter/detail,
//! zero-cost validate probes (M13) proven by mock billable counters,
//! per-provider sync-usage semantics, additive-DTO compat pins, and the M22
//! typed cross-provider duplicate rejection at the admin API.

use super::core_support_and_parsing::temp_db_path;
use super::*;
use std::sync::atomic::AtomicUsize;
use tavily_hikari::providers::ProviderId;

const SERP_VALID_KEY: &str = "serp-valid-key";
const SERP_BAD_KEY: &str = "serp-bad-key";
const EXA_VALID_KEY: &str = "exa-valid-key";
const EXA_BAD_KEY: &str = "exa-bad-key";
const BRAVE_VALID_KEY: &str = "brave-valid-key";
const BRAVE_BAD_KEY: &str = "brave-bad-key";

/// Per-provider mock counters. `billable` only increments on a response shape
/// that the real provider would charge for; the M13 acceptance is that every
/// validate/sync probe in this module leaves it at zero.
#[derive(Debug, Default)]
struct ProviderCounters {
    billable: AtomicUsize,
    probe_hits: AtomicUsize,
}

#[derive(Clone)]
struct MockCounters(Arc<ProviderCounters>);

impl ProviderCounters {
    fn new_arc() -> Arc<ProviderCounters> {
        Arc::new(ProviderCounters::default())
    }

    fn billable(&self) -> usize {
        self.billable.load(Ordering::SeqCst)
    }

    fn probe_hits(&self) -> usize {
        self.probe_hits.load(Ordering::SeqCst)
    }
}

async fn bind_ephemeral(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

// ---------- mock SerpApi: free /account + billable /search ----------

async fn serpapi_account(
    State(counters): State<MockCounters>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    counters.0.probe_hits.fetch_add(1, Ordering::SeqCst);
    let key = query.get("api_key").map(String::as_str).unwrap_or_default();
    if key == SERP_VALID_KEY {
        (
            StatusCode::OK,
            Json(json!({
                "account_status": "Active",
                "plan_id": "bigdata",
                "searches_per_month": 5000_i64,
                "total_searches_left": 4321_i64,
                "plan_searches_left": 4321_i64,
                "plan_renewal_date": "2026-10-23",
                "this_hour_searches": 7,
                "account_rate_limit_per_hour": 6000
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid API key." })),
        )
            .into_response()
    }
}

async fn serpapi_search_billable(State(counters): State<MockCounters>) -> Response {
    counters.0.billable.fetch_add(1, Ordering::SeqCst);
    (StatusCode::OK, Json(json!({ "organic_results": [] }))).into_response()
}

async fn spawn_serpapi_mock(counters: Arc<ProviderCounters>) -> SocketAddr {
    bind_ephemeral(
        Router::new()
            .route("/account", get(serpapi_account))
            .route("/search", get(serpapi_search_billable))
            .with_state(MockCounters(counters)),
    )
    .await
}

// ---------- mock Exa: unbilled /contents validation probe ----------

async fn exa_contents(State(counters): State<MockCounters>, headers: HeaderMap) -> Response {
    counters.0.probe_hits.fetch_add(1, Ordering::SeqCst);
    let key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if key == EXA_VALID_KEY {
        // A valid key authenticates; the empty body then fails request
        // validation. Exa only bills on 2xx, so this error is unbilled.
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({ "error": "body must contain either `ids` or `urls`" })),
        )
            .into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid api key" })),
        )
            .into_response()
    }
}

async fn exa_search_billable(State(counters): State<MockCounters>) -> Response {
    counters.0.billable.fetch_add(1, Ordering::SeqCst);
    (StatusCode::OK, Json(json!({ "results": [] }))).into_response()
}

async fn spawn_exa_mock(counters: Arc<ProviderCounters>) -> SocketAddr {
    bind_ephemeral(
        Router::new()
            .route("/contents", post(exa_contents))
            .route("/search", post(exa_search_billable))
            .with_state(MockCounters(counters)),
    )
    .await
}

// ---------- mock Brave: 422 discrimination (auth precedes routing) ----------

fn brave_error_envelope(code: &str, detail: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(json!({ "error": { "code": code, "detail": detail } })),
    )
        .into_response()
}

async fn brave_web_search(
    State(counters): State<MockCounters>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    counters.0.probe_hits.fetch_add(1, Ordering::SeqCst);
    let token = headers
        .get("x-subscription-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if token.is_empty() {
        return brave_error_envelope("VALIDATION", "Unable to validate request parameter(s)");
    }
    if token != BRAVE_VALID_KEY {
        return brave_error_envelope(
            "SUBSCRIPTION_TOKEN_INVALID",
            "The provided subscription token is invalid.",
        );
    }
    let has_query = uri
        .query()
        .map(|query| {
            form_urlencoded::parse(query.as_bytes())
                .any(|(key, value)| key == "q" && !value.trim().is_empty())
        })
        .unwrap_or(false);
    if !has_query {
        // Auth succeeded; the missing `q` fails validation. Only successful
        // requests count for Brave, so this probe is unbilled.
        return brave_error_envelope("VALIDATION", "Unable to validate request parameter(s)");
    }
    counters.0.billable.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        Json(json!({ "type": "search", "web": { "results": [] } })),
    )
        .into_response()
}

async fn spawn_brave_mock(counters: Arc<ProviderCounters>) -> SocketAddr {
    bind_ephemeral(
        Router::new()
            .route("/res/v1/web/search", get(brave_web_search))
            .with_state(MockCounters(counters)),
    )
    .await
}

// ---------- admin harness ----------

struct ProviderAdminHarness {
    addr: SocketAddr,
    client: Client,
    serpapi_counters: Arc<ProviderCounters>,
    exa_counters: Arc<ProviderCounters>,
    brave_counters: Arc<ProviderCounters>,
    db_path: PathBuf,
}

impl Drop for ProviderAdminHarness {
    fn drop(&mut self) {
        if let Some(dir) = self.db_path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

impl ProviderAdminHarness {
    async fn post_json(&self, path: &str, payload: Value) -> (u16, Value) {
        let response = self
            .client
            .post(format!("http://{}{}", self.addr, path))
            .json(&payload)
            .send()
            .await
            .expect("request completes");
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let parsed = serde_json::from_str(&body).unwrap_or(Value::String(body));
        (status, parsed)
    }

    async fn get_json(&self, path: &str) -> (u16, Value) {
        let response = self
            .client
            .get(format!("http://{}{}", self.addr, path))
            .send()
            .await
            .expect("request completes");
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let parsed = serde_json::from_str(&body).unwrap_or(Value::String(body));
        (status, parsed)
    }

    async fn create_key(&self, payload: Value) -> (u16, Value) {
        self.post_json("/api/keys", payload).await
    }
}

async fn spawn_provider_admin_harness(prefix: &str) -> ProviderAdminHarness {
    let db_path = temp_db_path(prefix);
    let db_str = db_path.to_string_lossy().to_string();
    let serpapi_counters = ProviderCounters::new_arc();
    let exa_counters = ProviderCounters::new_arc();
    let brave_counters = ProviderCounters::new_arc();
    let serpapi_addr = spawn_serpapi_mock(serpapi_counters.clone()).await;
    let exa_addr = spawn_exa_mock(exa_counters.clone()).await;
    let brave_addr = spawn_brave_mock(brave_counters.clone()).await;
    let providers = tavily_hikari::ProviderBootstrap {
        serpapi_upstream: format!("http://{serpapi_addr}"),
        serpapi_mcp_upstream: format!("http://{serpapi_addr}/mcp"),
        exa_upstream: format!("http://{exa_addr}"),
        brave_upstream: format!("http://{brave_addr}"),
        serpapi_keys: Vec::new(),
        exa_keys: Vec::new(),
        brave_keys: Vec::new(),
    };
    let proxy = TavilyProxy::with_providers_in_ha_mode(
        Vec::<String>::new(),
        &format!("http://{serpapi_addr}/mcp"),
        &db_str,
        tavily_hikari::TavilyProxyOptions::from_database_path(&db_str),
        providers,
        tavily_hikari::HaMode::Single,
    )
    .await
    .expect("proxy created");
    let state = Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: true,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin: true,
        // Any accidental Tavily validate/sync traffic lands on an unrouted
        // mock path (404) instead of a real endpoint.
        usage_base: format!("http://{brave_addr}/__unused_tavily_usage"),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    });
    let app = Router::new()
        .route("/api/keys", get(list_keys).post(create_api_key))
        .route("/api/keys/validate", post(post_validate_api_keys))
        .route("/api/keys/:id", get(get_api_key_detail))
        .route("/api/keys/:id/sync-usage", post(post_sync_key_usage))
        .with_state(state);
    let addr = bind_ephemeral(app).await;
    ProviderAdminHarness {
        addr,
        client: Client::new(),
        serpapi_counters,
        exa_counters,
        brave_counters,
        db_path,
    }
}

fn find_result<'a>(body: &'a Value, api_key: &str) -> &'a Value {
    body["results"]
        .as_array()
        .expect("results array")
        .iter()
        .find(|item| item["api_key"] == json!(api_key))
        .unwrap_or_else(|| panic!("result missing for {api_key}"))
}

fn renewal_epoch(date: &str) -> i64 {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .expect("renewal date parses")
        .and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        .and_utc()
        .timestamp()
}

#[tokio::test]
async fn create_with_provider_stamps_and_rejects_cross_provider_duplicate() {
    // Given a fresh admin surface
    let harness = spawn_provider_admin_harness("provider-admin-create").await;

    // When a brave key is created with an explicit provider
    let (status, body) = harness
        .create_key(json!({ "api_key": "k-brave-shared", "provider": "brave" }))
        .await;
    // Then it is created and stamped
    assert_eq!(status, 201, "create brave key: {body}");
    let brave_id = body["id"].as_str().expect("id").to_string();

    // When the SAME api_key is re-submitted for a different provider
    let (status, body) = harness
        .create_key(json!({ "api_key": "k-brave-shared", "provider": "serpapi" }))
        .await;
    // Then M22 keeps the global UNIQUE(api_key): typed rejection, no re-stamp
    assert_eq!(status, 409, "cross-provider duplicate: {body}");
    assert_eq!(body["error"], json!("provider_conflict"));
    assert!(
        body["detail"].as_str().unwrap_or_default().contains("brave"),
        "detail names the owning provider: {body}"
    );

    // And the legacy default (tavily) also conflicts against the brave row
    let (status, body) = harness.create_key(json!({ "api_key": "k-brave-shared" })).await;
    assert_eq!(status, 409, "tavily duplicate of brave key: {body}");
    assert_eq!(body["error"], json!("provider_conflict"));

    // And an unknown provider string is rejected at the boundary
    let (status, _) = harness
        .create_key(json!({ "api_key": "k-unknown-provider", "provider": "google" }))
        .await;
    assert!(status == 400 || status == 422, "unknown provider: {status}");

    // When a legacy client posts without provider
    let (status, body) = harness.create_key(json!({ "api_key": "k-legacy" })).await;
    // Then it lands as tavily (byte-identical legacy behavior)
    assert_eq!(status, 201, "legacy create: {body}");
    let legacy_id = body["id"].as_str().expect("id").to_string();

    let (status, body) = harness.get_json("/api/keys?per_page=100").await;
    assert_eq!(status, 200);
    let items = body["items"].as_array().expect("items");
    for (id, expected) in [
        (brave_id.as_str(), "brave"),
        (legacy_id.as_str(), "tavily"),
    ] {
        let item = items
            .iter()
            .find(|item| item["id"] == json!(id))
            .unwrap_or_else(|| panic!("item {id} listed"));
        assert_eq!(item["provider"], json!(expected), "stamped provider");
    }
}

#[tokio::test]
async fn list_filter_and_detail_expose_provider_and_quota_renews_at() {
    // Given one key per provider
    let harness = spawn_provider_admin_harness("provider-admin-list").await;
    let mut ids = HashMap::new();
    for (api_key, provider) in [
        ("k-tavily", "tavily"),
        ("k-brave", "brave"),
        ("k-exa", "exa"),
        ("k-serpapi", "serpapi"),
    ] {
        let (status, body) = harness
            .create_key(json!({ "api_key": api_key, "provider": provider }))
            .await;
        assert_eq!(status, 201, "create {api_key}: {body}");
        ids.insert(
            api_key.to_string(),
            body["id"].as_str().expect("id").to_string(),
        );
    }

    // When listing without a provider filter
    let (status, body) = harness.get_json("/api/keys?per_page=100").await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], json!(4), "unfiltered total");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 4);
    // Then every item carries provider, and the additive DTO surface is
    // exactly the legacy key set plus `provider` (quota_renews_at stays
    // absent while NULL).
    let mut expected_keys: Vec<&str> = vec![
        "id",
        "status",
        "group",
        "registration_ip",
        "registration_region",
        "status_changed_at",
        "last_used_at",
        "deleted_at",
        "quota_limit",
        "quota_remaining",
        "quota_synced_at",
        "total_requests",
        "success_count",
        "error_count",
        "quota_exhausted_count",
        "quarantine",
        "transient_backoff",
        "provider",
    ];
    expected_keys.sort_unstable();
    for item in items {
        let mut keys: Vec<&str> = item
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, expected_keys, "additive item shape");
        let api_key = ids
            .iter()
            .find(|(_, id)| Some(id.as_str()) == item["id"].as_str())
            .map(|(api_key, _)| api_key.as_str())
            .expect("id maps back");
        let expected_provider = api_key.trim_start_matches("k-");
        assert_eq!(item["provider"], json!(expected_provider));
    }

    // When filtering by provider
    let (status, body) = harness.get_json("/api/keys?provider=brave").await;
    assert_eq!(status, 200);
    assert_eq!(body["total"], json!(1), "brave-only total");
    assert_eq!(body["items"][0]["id"], json!(ids["k-brave"]));
    assert_eq!(body["items"][0]["provider"], json!("brave"));

    let (_, body) = harness
        .get_json("/api/keys?provider=brave&provider=exa")
        .await;
    assert_eq!(body["total"], json!(2), "multi-provider total");

    let (_, body) = harness.get_json("/api/keys?provider=bogus").await;
    assert_eq!(body["total"], json!(0), "unknown provider matches nothing");

    // When reading key detail
    let (status, body) = harness
        .get_json(&format!("/api/keys/{}", ids["k-brave"]))
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["provider"], json!("brave"), "detail provider");
    assert!(
        body.get("quota_renews_at").is_none() || body["quota_renews_at"].is_null(),
        "detail quota_renews_at absent while NULL: {body}"
    );
}

#[tokio::test]
async fn validate_matrix_probes_are_zero_cost_for_every_provider() {
    // Given a fresh admin surface with per-provider mocks
    let harness = spawn_provider_admin_harness("provider-admin-validate").await;

    // When validating a valid+invalid key for every new provider
    let (status, body) = harness
        .post_json(
            "/api/keys/validate",
            json!({
                "items": [
                    { "api_key": SERP_VALID_KEY, "provider": "serpapi" },
                    { "api_key": SERP_BAD_KEY, "provider": "serpapi" },
                    { "api_key": EXA_VALID_KEY, "provider": "exa" },
                    { "api_key": EXA_BAD_KEY, "provider": "exa" },
                    { "api_key": BRAVE_VALID_KEY, "provider": "brave" },
                    { "api_key": BRAVE_BAD_KEY, "provider": "brave" }
                ]
            }),
        )
        .await;
    assert_eq!(status, 200, "validate response: {body}");

    // Then the summary buckets the zero-cost probe matrix
    assert_eq!(body["summary"]["input_lines"], json!(6));
    assert_eq!(body["summary"]["unique_in_input"], json!(6));
    assert_eq!(body["summary"]["ok"], json!(3), "summary: {body}");
    assert_eq!(body["summary"]["invalid"], json!(3), "summary: {body}");
    assert_eq!(body["summary"]["error"], json!(0), "summary: {body}");

    // SerpApi: /account 200 surfaces the plan fields; 401 is invalid.
    let ok = find_result(&body, SERP_VALID_KEY);
    assert_eq!(ok["status"], json!("ok"));
    assert_eq!(ok["provider"], json!("serpapi"));
    assert_eq!(ok["quota_limit"], json!(5000));
    assert_eq!(ok["quota_remaining"], json!(4321));
    let detail = ok["detail"].as_str().expect("plan detail");
    for needle in [
        "plan_searches_left=4321",
        "total_searches_left=4321",
        "searches_per_month=5000",
        "plan_renewal_date=2026-10-23",
        "account_status=Active",
    ] {
        assert!(detail.contains(needle), "detail {needle}: {detail}");
    }
    let bad = find_result(&body, SERP_BAD_KEY);
    assert_eq!(bad["status"], json!("unauthorized"));
    assert_eq!(bad["provider"], json!("serpapi"));

    // Exa: 422 on the empty /contents body means the key authenticated
    // (unbilled validation error); 401 is invalid.
    let ok = find_result(&body, EXA_VALID_KEY);
    assert_eq!(ok["status"], json!("ok"));
    assert_eq!(ok["provider"], json!("exa"));
    assert_eq!(ok["reason"], json!("UNBILLED_VALIDATION_ERROR"));
    let bad = find_result(&body, EXA_BAD_KEY);
    assert_eq!(bad["status"], json!("unauthorized"));
    assert_eq!(bad["provider"], json!("exa"));

    // Brave: 422 VALIDATION (missing q, auth already passed) is valid;
    // 422 SUBSCRIPTION_TOKEN_INVALID is invalid.
    let ok = find_result(&body, BRAVE_VALID_KEY);
    assert_eq!(ok["status"], json!("ok"));
    assert_eq!(ok["provider"], json!("brave"));
    assert_eq!(ok["reason"], json!("VALIDATION"));
    let bad = find_result(&body, BRAVE_BAD_KEY);
    assert_eq!(bad["status"], json!("invalid"));
    assert_eq!(bad["provider"], json!("brave"));
    assert_eq!(bad["reason"], json!("SUBSCRIPTION_TOKEN_INVALID"));

    // Then every billable counter is untouched (M13: validation never hits a
    // billable endpoint) and only the free probe endpoints were called.
    assert_eq!(harness.serpapi_counters.billable(), 0, "serpapi unbilled");
    assert_eq!(harness.serpapi_counters.probe_hits(), 2, "account probes");
    assert_eq!(harness.exa_counters.billable(), 0, "exa unbilled");
    assert_eq!(harness.exa_counters.probe_hits(), 2, "contents probes");
    assert_eq!(harness.brave_counters.billable(), 0, "brave unbilled");
    assert_eq!(harness.brave_counters.probe_hits(), 2, "brave probes");

    // When the top-level provider form is used (plain `api_keys` list)
    let (status, body) = harness
        .post_json(
            "/api/keys/validate",
            json!({ "api_keys": [BRAVE_VALID_KEY], "provider": "brave" }),
        )
        .await;
    // Then it behaves identically to the per-item form
    assert_eq!(status, 200);
    let ok = find_result(&body, BRAVE_VALID_KEY);
    assert_eq!(ok["status"], json!("ok"));
    assert_eq!(ok["provider"], json!("brave"));
    assert_eq!(harness.brave_counters.billable(), 0, "still unbilled");
}

#[tokio::test]
async fn sync_usage_is_live_for_serpapi_passive_for_brave_and_409_for_exa() {
    // Given one stored key per new provider
    let harness = spawn_provider_admin_harness("provider-admin-sync").await;
    let mut ids = HashMap::new();
    for (api_key, provider) in [
        (SERP_VALID_KEY, "serpapi"),
        (SERP_BAD_KEY, "serpapi"),
        (BRAVE_VALID_KEY, "brave"),
        (EXA_VALID_KEY, "exa"),
    ] {
        let (status, body) = harness
            .create_key(json!({ "api_key": api_key, "provider": provider }))
            .await;
        assert_eq!(status, 201, "create {api_key}: {body}");
        ids.insert(
            api_key.to_string(),
            body["id"].as_str().expect("id").to_string(),
        );
    }

    // When syncing the valid serpapi key
    let (status, body) = harness
        .post_json(
            &format!("/api/keys/{}/sync-usage", ids[SERP_VALID_KEY]),
            json!({}),
        )
        .await;
    // Then the live /account fetch updates all four quota fields
    assert_eq!(status, 200, "serpapi sync: {body}");
    assert_eq!(body["source"], json!("live"));
    assert_eq!(body["provider"], json!("serpapi"));
    assert_eq!(body["quota_limit"], json!(5000));
    assert_eq!(body["quota_remaining"], json!(4321));
    assert_eq!(body["quota_renews_at"], json!(renewal_epoch("2026-10-23")));
    let synced_at = body["quota_synced_at"].as_i64().expect("synced_at");
    assert!(synced_at > 0, "quota_synced_at stamped");
    // And the key detail reflects the persisted values
    let (status, body) = harness
        .get_json(&format!("/api/keys/{}", ids[SERP_VALID_KEY]))
        .await;
    assert_eq!(status, 200);
    assert_eq!(body["quota_limit"], json!(5000));
    assert_eq!(body["quota_remaining"], json!(4321));
    assert_eq!(body["quota_renews_at"], json!(renewal_epoch("2026-10-23")));
    assert_eq!(body["provider"], json!("serpapi"));
    // And the probe stayed on the free /account endpoint
    assert_eq!(harness.serpapi_counters.billable(), 0, "serpapi unbilled");

    // When syncing a serpapi key whose /account rejects it
    let (status, body) = harness
        .post_json(
            &format!("/api/keys/{}/sync-usage", ids[SERP_BAD_KEY]),
            json!({}),
        )
        .await;
    // Then the typed usage-http error surfaces with the upstream status
    assert_eq!(status, 401, "bad serpapi sync: {body}");
    assert_eq!(body["error"], json!("usage_http"));

    // When syncing the brave key
    let (status, body) = harness
        .post_json(
            &format!("/api/keys/{}/sync-usage", ids[BRAVE_VALID_KEY]),
            json!({}),
        )
        .await;
    // Then it returns the last passive-harvest snapshot with zero live calls
    assert_eq!(status, 200, "brave sync: {body}");
    assert_eq!(body["source"], json!("passive"));
    assert_eq!(body["provider"], json!("brave"));
    assert_eq!(harness.brave_counters.probe_hits(), 0, "no brave call");
    assert_eq!(harness.brave_counters.billable(), 0, "brave unbilled");

    // When syncing the exa key
    let (status, body) = harness
        .post_json(
            &format!("/api/keys/{}/sync-usage", ids[EXA_VALID_KEY]),
            json!({}),
        )
        .await;
    // Then it is a typed 409 (no live balance API exists)
    assert_eq!(status, 409, "exa sync: {body}");
    assert_eq!(body["error"], json!("sync_unsupported"));
    assert_eq!(body["provider"], json!("exa"));
    assert_eq!(harness.exa_counters.probe_hits(), 0, "no exa call");
}

#[test]
fn tavily_dto_and_validate_result_shapes_stay_additive() {
    // Given a legacy-shaped tavily metrics fixture
    let metrics = ApiKeyMetrics {
        id: "key-id-1".to_string(),
        provider: ProviderId::Tavily,
        status: "active".to_string(),
        group_name: None,
        registration_ip: None,
        registration_region: None,
        status_changed_at: Some(1_700_000_000),
        last_used_at: Some(1_700_000_001),
        deleted_at: None,
        quota_limit: Some(5000),
        quota_remaining: Some(4000),
        quota_synced_at: Some(1_700_000_002),
        quota_renews_at: None,
        total_requests: 10,
        success_count: 9,
        error_count: 1,
        quota_exhausted_count: 0,
        quarantine: None,
        transient_backoff: None,
    };

    // When rendering the LIST view
    let view = ApiKeyView::from_list(metrics.clone());
    // Then the serialized JSON is the legacy key set, order, and values with
    // exactly one additive key (`provider`); `quota_renews_at` stays absent
    // while NULL.
    assert_eq!(
        serde_json::to_string(&view).expect("serialize"),
        concat!(
            r#"{"id":"key-id-1","provider":"tavily","status":"active","group":null,"#,
            r#""registration_ip":null,"registration_region":null,"status_changed_at":1700000000,"#,
            r#""last_used_at":1700000001,"deleted_at":null,"quota_limit":5000,"quota_remaining":4000,"#,
            r#""quota_synced_at":1700000002,"total_requests":10,"success_count":9,"error_count":1,"#,
            r#""quota_exhausted_count":0,"quarantine":null,"transient_backoff":null}"#
        )
    );

    // When rendering the DETAIL view for a serpapi row with a renewal date
    let mut detail_metrics = metrics.clone();
    detail_metrics.provider = ProviderId::Serpapi;
    detail_metrics.quota_renews_at = Some(1_792_742_400);
    let detail_json: Value =
        serde_json::from_str(&serde_json::to_string(&ApiKeyView::from_detail(detail_metrics)).expect("serialize"))
            .expect("parse");
    assert_eq!(detail_json["provider"], json!("serpapi"));
    assert_eq!(detail_json["quota_renews_at"], json!(1_792_742_400));

    // Given a legacy tavily validate result
    let legacy_result = ValidateKeyResult {
        api_key: "tvly-key".to_string(),
        status: "ok".to_string(),
        provider: None,
        reason: None,
        registration_ip: None,
        registration_region: None,
        assigned_proxy_key: None,
        assigned_proxy_label: None,
        assigned_proxy_match_kind: None,
        quota_limit: Some(5000),
        quota_remaining: Some(4000),
        detail: None,
    };
    // Then the new optional fields never appear in its JSON (byte-identical
    // legacy shape for tavily-only fixtures)
    assert_eq!(
        serde_json::to_string(&legacy_result).expect("serialize"),
        r#"{"api_key":"tvly-key","status":"ok","quota_limit":5000,"quota_remaining":4000}"#
    );

    // And legacy create payloads parse without provider while unknown
    // providers are rejected at the serde boundary.
    let legacy: CreateKeyRequest =
        serde_json::from_str(r#"{"api_key":"k"}"#).expect("legacy body parses");
    assert_eq!(legacy.provider, None);
    let brave: CreateKeyRequest =
        serde_json::from_str(r#"{"api_key":"k","provider":"brave"}"#).expect("provider parses");
    assert_eq!(brave.provider, Some(ProviderId::Brave));
    assert!(
        serde_json::from_str::<CreateKeyRequest>(r#"{"api_key":"k","provider":"google"}"#)
            .is_err(),
        "unknown provider rejected"
    );
}

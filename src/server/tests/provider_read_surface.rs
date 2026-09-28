//! Plan todo 20 bin coverage: the additive provider dimension across the
//! read surfaces — /api/summary(+windows) provider breakdowns, /api/logs
//! item provider + provider facets + `?provider=` pagination (legacy NULL
//! rows coalesce to `tavily`), user/public token-log provider derivation,
//! request-kind catalog registration of every todo-3 kind with M9 billing
//! groups, and the M18 boundary (rollup-backed legacy totals untouched).

use super::core_support_and_parsing::{connect_sqlite_test_pool, temp_db_path};
use super::linuxdo_oauth_and_admin_keys::find_cookie_pair;
use super::upstream_support_and_manual_jobs::spawn_builtin_keys_admin_server;
use super::*;

/// The STATIC todo-3 whitelist (src/analysis.rs PROVIDER_REQUEST_KINDS):
/// kind key -> owning provider. Copied verbatim so this bin test pins the
/// server-side registration end to end.
const PROVIDER_KIND_FIXTURES: &[(&str, &str)] = &[
    ("api:serpapi/search", "serpapi"),
    ("api:exa/search", "exa"),
    ("api:exa/contents", "exa"),
    ("api:exa/answer", "exa"),
    ("api:brave/web", "brave"),
    ("api:brave/news", "brave"),
    ("api:brave/llm-context", "brave"),
    ("mcp:serpapi/search", "serpapi"),
    ("mcp:serpapi/search_table", "serpapi"),
    ("mcp:serpapi/search_dashboard", "serpapi"),
    ("mcp:brave/web_search", "brave"),
    ("mcp:brave/news_search", "brave"),
    ("mcp:brave/llm_context", "brave"),
    ("mcp:exa/web_search", "exa"),
    ("mcp:exa/web_fetch", "exa"),
    ("mcp:exa/answer", "exa"),
];

fn cleanup_db_dir(db_path: &std::path::Path) {
    if let Some(dir) = db_path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

async fn bind_read_surface_ephemeral(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    addr
}

fn read_surface_state(proxy: TavilyProxy, dev_open_admin: bool) -> Arc<AppState> {
    Arc::new(AppState {
        proxy,
        static_dir: None,
        forward_auth: ForwardAuthConfig::new(None, None, None, None),
        forward_auth_enabled: false,
        builtin_admin: BuiltinAdminAuth::new(false, None, None),
        admin_passkey: AdminPasskeyOptions::disabled(),
        linuxdo_oauth: LinuxDoOAuthOptions::disabled(),
        linuxdo_credit: LinuxDoCreditOptions::disabled(),
        ha: tavily_hikari::HaRuntime::new(tavily_hikari::HaConfig::default()),
        dev_open_admin,
        usage_base: "http://127.0.0.1:58088".to_string(),
        api_key_ip_geo_origin: "https://api.country.is".to_string(),
        dashboard_overview_cache: new_dashboard_overview_cache(),
        remote_attempt_admission: new_remote_attempt_admission(),
    })
}

async fn login_admin(client: &Client, addr: SocketAddr, password: &str) -> String {
    let login_resp = client
        .post(format!("http://{}/api/admin/login", addr))
        .json(&serde_json::json!({ "password": password }))
        .send()
        .await
        .expect("admin login");
    assert_eq!(login_resp.status(), reqwest::StatusCode::OK);
    find_cookie_pair(login_resp.headers(), BUILTIN_ADMIN_COOKIE_NAME).expect("admin session cookie")
}

async fn admin_get_json(client: &Client, addr: SocketAddr, cookie: &str, path: &str) -> Value {
    let response = client
        .get(format!("http://{addr}{path}"))
        .header(reqwest::header::COOKIE, cookie)
        .send()
        .await
        .expect("admin request completes");
    assert_eq!(response.status(), reqwest::StatusCode::OK, "GET {path}");
    let body = response.text().await.unwrap_or_default();
    serde_json::from_str(&body).unwrap_or_else(|_| panic!("json for {path}: {body}"))
}

/// Seeds one request_logs row per (provider, kind, status) triple. `provider`
/// stays NULL for the legacy row so readers must coalesce it to `tavily`.
async fn seed_mixed_provider_request_logs(pool: &sqlx::SqlitePool, key_id: &str, created_at: i64) {
    for (index, (provider, kind, result_status)) in [
        (Some("brave"), "api:brave/web", "success"),
        (Some("brave"), "api:brave/news", "success"),
        (Some("exa"), "api:exa/search", "success"),
        (Some("serpapi"), "api:serpapi/search", "error"),
        (None, "api:search", "success"),
        (Some("tavily"), "api:search", "success"),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query(
            r#"
            INSERT INTO observability.request_logs (
                api_key_id, auth_token_id, method, path, query,
                status_code, tavily_status_code, error_message, result_status,
                request_kind_key, request_kind_label, provider,
                request_body, response_body, forwarded_headers, dropped_headers,
                created_at
            ) VALUES (?, NULL, 'POST', ?, ?, 200, 200, NULL, ?, ?, 'seeded kind', ?, NULL, NULL, '[]', '[]', ?)
            "#,
        )
        .bind(key_id)
        .bind(format!("/{kind}"))
        .bind(format!("q=read-surface-{index}"))
        .bind(result_status)
        .bind(kind)
        .bind(provider)
        .bind(created_at - index as i64)
        .execute(pool)
        .await
        .expect("insert mixed-provider request log");
    }
}

// ---------- gate (a): additive DTO compat pins ----------

#[test]
fn dto_provider_additions_keep_legacy_payloads_byte_identical() {
    // Given a legacy-shaped (zeroed) summary, the exact-string pin proves
    // the additive `providers` field never appears while None.
    let summary_view = SummaryView::from(tavily_hikari::ProxySummary {
        total_requests: 0,
        success_count: 0,
        error_count: 0,
        quota_exhausted_count: 0,
        active_keys: 0,
        exhausted_keys: 0,
        quarantined_keys: 0,
        temporary_isolated_keys: 0,
        last_activity: None,
        total_quota_limit: 0,
        total_quota_remaining: 0,
    });
    assert_eq!(
        serde_json::to_string(&summary_view).expect("serialize"),
        concat!(
            r#"{"total_requests":0,"success_count":0,"error_count":0,"quota_exhausted_count":0,"#,
            r#""active_keys":0,"exhausted_keys":0,"quarantined_keys":0,"temporary_isolated_keys":0,"#,
            r#""last_activity":null,"total_quota_limit":0,"total_quota_remaining":0}"#
        )
    );

    // Given the default summary-window metrics, the same holds per bucket.
    let window_view = SummaryWindowView::from(tavily_hikari::SummaryWindowMetrics::default());
    assert_eq!(
        serde_json::to_string(&window_view).expect("serialize"),
        concat!(
            r#"{"total_requests":0,"success_count":0,"error_count":0,"quota_exhausted_count":0,"#,
            r#""valuable_success_count":0,"valuable_failure_count":0,"other_success_count":0,"#,
            r#""other_failure_count":0,"unknown_count":0,"upstream_exhausted_key_count":0,"#,
            r#""new_keys":0,"new_quarantines":0,"quota_charge":{"local_estimated_credits":0,"#,
            r#""upstream_actual_credits":0,"sampled_key_count":0,"stale_key_count":0,"#,
            r#""latest_sync_at":null}}"#
        )
    );

    // Given a legacy-shaped facets view, `providers: None` is skipped so the
    // six legacy keys stay byte-identical.
    let facets = RequestLogFacetsView {
        results: Vec::new(),
        key_effects: Vec::new(),
        binding_effects: Vec::new(),
        selection_effects: Vec::new(),
        tokens: Vec::new(),
        keys: Vec::new(),
        providers: None,
    };
    assert_eq!(
        serde_json::to_string(&facets).expect("serialize"),
        r#"{"results":[],"keyEffects":[],"bindingEffects":[],"selectionEffects":[],"tokens":[],"keys":[]}"#
    );

    // Given a tavily request-log record, every legacy field serializes
    // unchanged and exactly one additive key (`provider`) appears last.
    let record = RequestLogRecord {
        id: 41,
        key_id: Some("k4".to_string()),
        auth_token_id: Some("tok".to_string()),
        method: "POST".to_string(),
        path: "/search".to_string(),
        query: None,
        status_code: None,
        tavily_status_code: None,
        error_message: None,
        business_credits: None,
        request_kind_key: "api:search".to_string(),
        request_kind_label: "API | search".to_string(),
        request_kind_detail: None,
        request_kind_protocol_group: "api".to_string(),
        request_kind_billing_group: "billable".to_string(),
        result_status: "success".to_string(),
        failure_kind: None,
        key_effect_code: "none".to_string(),
        key_effect_summary: None,
        binding_effect_code: "none".to_string(),
        binding_effect_summary: None,
        selection_effect_code: "none".to_string(),
        selection_effect_summary: None,
        gateway_mode: None,
        experiment_variant: None,
        provider: "tavily".to_string(),
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
        operational_class: "success".to_string(),
        request_body: Vec::new(),
        response_body: Vec::new(),
        request_body_bytes: None,
        response_body_bytes: None,
        request_body_sha256: None,
        response_body_sha256: None,
        body_cleaned_reason: None,
        body_cleaned_at: None,
        created_at: 1_700_000_050,
        forwarded_headers: Vec::new(),
        dropped_headers: Vec::new(),
        remote_addr: None,
        client_ip: None,
        client_ip_source: None,
        client_ip_trusted: false,
        ip_headers: Vec::new(),
    };
    assert_eq!(
        serde_json::to_string(&RequestLogView::from_summary_record(record)).expect("serialize"),
        concat!(
            r#"{"id":41,"key_id":"k4","auth_token_id":"tok","method":"POST","path":"/search","#,
            r#""query":null,"http_status":null,"mcp_status":null,"business_credits":null,"#,
            r#""request_kind_key":"api:search","request_kind_label":"API | search","#,
            r#""request_kind_detail":null,"result_status":"success","created_at":1700000050,"#,
            r#""error_message":null,"failure_kind":null,"key_effect_code":"none","#,
            r#""key_effect_summary":null,"binding_effect_code":"none","binding_effect_summary":null,"#,
            r#""selection_effect_code":"none","selection_effect_summary":null,"gateway_mode":null,"#,
            r#""experiment_variant":null,"proxy_session_id":null,"routing_subject_hash":null,"#,
            r#""upstream_operation":null,"fallback_reason":null,"request_body":null,"#,
            r#""response_body":null,"request_body_bytes":null,"response_body_bytes":null,"#,
            r#""request_body_sha256":null,"response_body_sha256":null,"body_cleaned_reason":null,"#,
            r#""body_cleaned_at":null,"forwarded_headers":[],"dropped_headers":[],"#,
            r#""remote_addr":null,"client_ip":null,"client_ip_source":null,"#,
            r#""client_ip_trusted":false,"ip_headers":[],"operationalClass":"success","#,
            r#""requestKindProtocolGroup":"api","requestKindBillingGroup":"billable","#,
            r#""provider":"tavily"}"#
        )
    );

    // Given a legacy token-log record, the public view gains exactly the
    // additive trailing `provider` key.
    let public_view = PublicTokenLogView::from_record(
        token_log_record_fixture("api:search", "API | search", 1_700_000_060),
        UiLanguage::En,
    );
    assert_eq!(
        serde_json::to_string(&public_view).expect("serialize"),
        concat!(
            r#"{"id":7,"method":"POST","path":"/search","query":null,"httpStatus":200,"#,
            r#""mcpStatus":null,"resultStatus":"success","errorMessage":null,"#,
            r#""createdAt":1700000060,"provider":"tavily"}"#
        )
    );
}

fn token_log_record_fixture(kind_key: &str, kind_label: &str, created_at: i64) -> TokenLogRecord {
    TokenLogRecord {
        id: 7,
        key_id: Some("k1".to_string()),
        method: "POST".to_string(),
        path: "/search".to_string(),
        query: None,
        http_status: Some(200),
        mcp_status: None,
        business_credits: None,
        request_kind_key: kind_key.to_string(),
        request_kind_label: kind_label.to_string(),
        request_kind_detail: None,
        counts_business_quota: false,
        result_status: "success".to_string(),
        error_message: None,
        failure_kind: None,
        key_effect_code: "none".to_string(),
        key_effect_summary: None,
        binding_effect_code: "none".to_string(),
        binding_effect_summary: None,
        selection_effect_code: "none".to_string(),
        selection_effect_summary: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: None,
        fallback_reason: None,
        created_at,
    }
}

// ---------- gate (d) unit half + M9 static coverage ----------

#[test]
fn token_log_provider_derivation_covers_static_provider_kinds() {
    // Every static whitelist kind derives its owning provider.
    for (kind, provider) in PROVIDER_KIND_FIXTURES {
        assert_eq!(
            token_log_provider_from_request_kind(kind),
            *provider,
            "kind {kind} derivation"
        );
    }
    // Non-provider kinds (and junk) stay tavily: the whitelist is STATIC
    // (M9), so nothing else may claim a provider segment.
    for kind in [
        "api:search",
        "mcp:search",
        "mcp:batch",
        "mcp:tools/list",
        "mcp:resources/x",
        "mcp:third-party-tool",
        "api:unknown-path",
        "",
        "junk",
    ] {
        assert_eq!(
            token_log_provider_from_request_kind(kind),
            "tavily",
            "kind {kind:?} stays tavily"
        );
    }

    // The user-console view inherits the same derivation.
    let user_view = UserTokenLogView::from_record(
        token_log_record_fixture(
            "mcp:brave/web_search",
            "MCP | brave/web_search",
            1_700_000_061,
        ),
        UiLanguage::En,
    );
    assert_eq!(user_view.provider, "brave");
}

// ---------- gates (b)+(f): /api/logs items, facets, filtered pagination ----------

#[tokio::test]
async fn admin_logs_gain_provider_items_facets_and_filtered_pagination() {
    // Given a DB seeded with mixed-provider request logs plus one legacy
    // NULL-provider row (created inside today's window)
    let db_path = temp_db_path("read-surface-logs");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-read-surface-logs".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let windows = proxy.summary_windows().await.expect("summary windows");
    let created_at = windows.today_start + (windows.today_end - windows.today_start) / 2;
    let pool = connect_sqlite_test_pool(&db_str).await;
    seed_mixed_provider_request_logs(&pool, &key_id, created_at).await;
    drop(pool);
    proxy
        .rebuild_request_log_catalog_for_test()
        .await
        .expect("materialize catalog rollups");

    let admin_password = "read-surface-logs-password";
    let admin_addr = spawn_builtin_keys_admin_server(proxy.clone(), admin_password).await;
    let client = Client::new();
    let cookie = login_admin(&client, admin_addr, admin_password).await;

    // When reading the unfiltered page
    let body = admin_get_json(&client, admin_addr, &cookie, "/api/logs?page=1&per_page=20").await;
    // Then every item carries provider and the NULL-legacy row coalesces to
    // tavily, while all six rows paginate as before
    assert_eq!(body["total"], json!(6), "unfiltered total: {body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 6);
    let provider_of = |query: &str| -> String {
        items
            .iter()
            .find(|item| item["query"] == json!(query))
            .unwrap_or_else(|| panic!("item for {query}"))["provider"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(provider_of("q=read-surface-0"), "brave");
    assert_eq!(provider_of("q=read-surface-1"), "brave");
    assert_eq!(provider_of("q=read-surface-2"), "exa");
    assert_eq!(provider_of("q=read-surface-3"), "serpapi");
    assert_eq!(provider_of("q=read-surface-4"), "tavily", "NULL coalesce");
    assert_eq!(provider_of("q=read-surface-5"), "tavily");

    // And the facets gain the provider list with request_logs counts
    let providers = body.pointer("/facets/providers").and_then(|v| v.as_array());
    let providers = providers.unwrap_or_else(|| panic!("providers facet: {body}"));
    let facet_counts: std::collections::HashMap<String, i64> = providers
        .iter()
        .map(|option| {
            (
                option["value"].as_str().unwrap_or_default().to_string(),
                option["count"].as_i64().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        facet_counts.get("brave").copied(),
        Some(2),
        "{facet_counts:?}"
    );
    assert_eq!(
        facet_counts.get("exa").copied(),
        Some(1),
        "{facet_counts:?}"
    );
    assert_eq!(
        facet_counts.get("serpapi").copied(),
        Some(1),
        "{facet_counts:?}"
    );
    assert_eq!(
        facet_counts.get("tavily").copied(),
        Some(2),
        "{facet_counts:?}"
    );

    // When filtering by provider=brave
    let body = admin_get_json(&client, admin_addr, &cookie, "/api/logs?provider=brave").await;
    // Then only brave rows come back and the total follows the filter
    assert_eq!(body["total"], json!(2), "brave total: {body}");
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert!(
        items.iter().all(|item| item["provider"] == json!("brave")),
        "only brave items: {body}"
    );

    // When filtering by provider=tavily, the NULL-legacy row is included
    let body = admin_get_json(&client, admin_addr, &cookie, "/api/logs?provider=tavily").await;
    assert_eq!(body["total"], json!(2), "tavily total incl NULL: {body}");

    // When paginating a filtered result set one row at a time
    let page_one = admin_get_json(
        &client,
        admin_addr,
        &cookie,
        "/api/logs?provider=tavily&per_page=1&page=1",
    )
    .await;
    let page_two = admin_get_json(
        &client,
        admin_addr,
        &cookie,
        "/api/logs?provider=tavily&per_page=1&page=2",
    )
    .await;
    // Then the filtered total is stable across pages and the pages are
    // disjoint (COUNT mirrors the items predicate)
    assert_eq!(page_one["total"], json!(2), "page1 total: {page_one}");
    assert_eq!(page_two["total"], json!(2), "page2 total: {page_two}");
    assert_eq!(page_one["items"].as_array().expect("items").len(), 1);
    assert_eq!(page_two["items"].as_array().expect("items").len(), 1);
    assert_ne!(
        page_one["items"][0]["id"], page_two["items"][0]["id"],
        "distinct rows across filtered pages"
    );

    // When combining provider with an existing result filter
    let body = admin_get_json(
        &client,
        admin_addr,
        &cookie,
        "/api/logs?provider=brave&result=error",
    )
    .await;
    assert_eq!(body["total"], json!(0), "brave has no errors: {body}");
    assert_eq!(body["items"].as_array().expect("items").len(), 0);
    let body = admin_get_json(
        &client,
        admin_addr,
        &cookie,
        "/api/logs?provider=serpapi&result=error",
    )
    .await;
    assert_eq!(body["total"], json!(1), "serpapi error row: {body}");

    cleanup_db_dir(&db_path);
}

#[tokio::test]
async fn provider_filter_on_tavily_only_db_returns_empty_page_shape() {
    // Given a tavily-only DB
    let db_path = temp_db_path("read-surface-empty-filter");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-read-surface-empty".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let admin_password = "read-surface-empty-password";
    let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
    let client = Client::new();
    let cookie = login_admin(&client, admin_addr, admin_password).await;

    // When filtering provider=exa (no such rows exist)
    let response = client
        .get(format!("http://{admin_addr}/api/logs?provider=exa"))
        .header(reqwest::header::COOKIE, cookie.clone())
        .send()
        .await
        .expect("request completes");
    // Then it is a well-formed empty page, not an error
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json body");
    assert_eq!(body["total"], json!(0), "empty total: {body}");
    assert_eq!(body["items"], json!([]), "empty items: {body}");
    assert_eq!(body["page"], json!(1));
    assert_eq!(body["perPage"], json!(20));
    for pointer in ["/requestKindOptions", "/facets/results", "/facets/keys"] {
        assert!(
            body.pointer(pointer).is_some(),
            "page shape keeps {pointer}: {body}"
        );
    }

    cleanup_db_dir(&db_path);
}

// ---------- gate (c): summary + windows provider breakdowns ----------

#[tokio::test]
async fn summary_and_windows_carry_provider_breakdowns_alongside_legacy_totals() {
    // Given keys for all four providers, per-key daily usage buckets (the
    // source of the legacy summary totals), mixed-provider request logs from
    // today, and one rollup minute bucket pinning the legacy window path
    let db_path = temp_db_path("read-surface-summary");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-read-surface-summary".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let windows = proxy.summary_windows().await.expect("summary windows");
    let created_at = windows.today_start + (windows.today_end - windows.today_start) / 2;
    let pool = connect_sqlite_test_pool(&db_str).await;

    for (id, api_key, provider) in [
        ("read-surface-brave-key", "k-read-brave", "brave"),
        ("read-surface-exa-key", "k-read-exa", "exa"),
        ("read-surface-serp-key", "k-read-serpapi", "serpapi"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO api_keys (id, api_key, status, provider, created_at)
            VALUES (?, ?, 'active', ?, ?)
            "#,
        )
        .bind(id)
        .bind(api_key)
        .bind(provider)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert provider key");
    }
    // (key_id, total, success, error, quota_exhausted)
    for (bound_key_id, total, success, error, exhausted) in [
        (key_id.clone(), 2_i64, 2_i64, 0_i64, 0_i64),
        ("read-surface-brave-key".to_string(), 10, 10, 0, 0),
        ("read-surface-exa-key".to_string(), 3, 2, 1, 0),
        ("read-surface-serp-key".to_string(), 1, 0, 1, 0),
    ] {
        sqlx::query(
            r#"
            INSERT INTO observability.api_key_usage_buckets (
                api_key_id, bucket_start, bucket_secs,
                total_requests, success_count, error_count, quota_exhausted_count, updated_at
            ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&bound_key_id)
        .bind(windows.today_start)
        .bind(total)
        .bind(success)
        .bind(error)
        .bind(exhausted)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert usage bucket");
    }
    seed_mixed_provider_request_logs(&pool, &key_id, created_at).await;
    let minute_bucket = created_at.div_euclid(60) * 60;
    sqlx::query(
        r#"
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start, bucket_secs, total_requests, success_count, error_count,
            quota_exhausted_count, valuable_success_count, valuable_failure_count,
            valuable_failure_429_count, other_success_count, other_failure_count,
            unknown_count, mcp_non_billable, mcp_billable, api_non_billable,
            api_billable, local_estimated_credits, updated_at
        ) VALUES (?, 60, 2, 2, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 2, 2, ?)
        "#,
    )
    .bind(minute_bucket)
    .bind(minute_bucket + 59)
    .execute(&pool)
    .await
    .expect("insert dashboard minute bucket");
    drop(pool);

    let admin_password = "read-surface-summary-password";
    let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
    let client = Client::new();
    let cookie = login_admin(&client, admin_addr, admin_password).await;

    // When reading /api/summary
    let body = admin_get_json(&client, admin_addr, &cookie, "/api/summary").await;
    // Then the legacy totals keep reflecting the pre-aggregated buckets ...
    assert_eq!(body["total_requests"], json!(16), "legacy total: {body}");
    assert_eq!(body["success_count"], json!(14), "legacy success: {body}");
    assert_eq!(body["error_count"], json!(2), "legacy error: {body}");
    // ... and the additive breakdown segments the SAME source by key-join
    // (M18), ordered by provider with NULL-free values.
    let providers = body["providers"].as_array();
    let providers = providers.unwrap_or_else(|| panic!("providers breakdown: {body}"));
    let rows: Vec<(String, i64, i64, i64, i64)> = providers
        .iter()
        .map(|row| {
            (
                row["provider"].as_str().unwrap_or_default().to_string(),
                row["total_requests"].as_i64().unwrap_or_default(),
                row["success_count"].as_i64().unwrap_or_default(),
                row["error_count"].as_i64().unwrap_or_default(),
                row["quota_exhausted_count"].as_i64().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("brave".to_string(), 10, 10, 0, 0),
            ("exa".to_string(), 3, 2, 1, 0),
            ("serpapi".to_string(), 1, 0, 1, 0),
            ("tavily".to_string(), 2, 2, 0, 0),
        ],
        "provider breakdown rows"
    );
    assert_eq!(
        rows.iter().map(|row| row.1).sum::<i64>(),
        16,
        "breakdown sums to the legacy total"
    );

    // When reading /api/summary/windows
    let body = admin_get_json(&client, admin_addr, &cookie, "/api/summary/windows").await;
    // Then every window bucket gains the additive provider breakdown read
    // from bounded request_logs scans (today's six seeded rows) ...
    let today_providers = body.pointer("/today/providers").and_then(|v| v.as_array());
    let today_providers = today_providers.unwrap_or_else(|| panic!("today providers: {body}"));
    let today_counts: std::collections::HashMap<String, (i64, i64, i64)> = today_providers
        .iter()
        .map(|row| {
            (
                row["provider"].as_str().unwrap_or_default().to_string(),
                (
                    row["total_requests"].as_i64().unwrap_or_default(),
                    row["success_count"].as_i64().unwrap_or_default(),
                    row["error_count"].as_i64().unwrap_or_default(),
                ),
            )
        })
        .collect();
    assert_eq!(
        today_counts.get("brave").copied(),
        Some((2, 2, 0)),
        "{today_counts:?}"
    );
    assert_eq!(
        today_counts.get("exa").copied(),
        Some((1, 1, 0)),
        "{today_counts:?}"
    );
    assert_eq!(
        today_counts.get("serpapi").copied(),
        Some((1, 0, 1)),
        "{today_counts:?}"
    );
    assert_eq!(
        today_counts.get("tavily").copied(),
        Some((2, 2, 0)),
        "{today_counts:?}"
    );
    // ... the rollup-backed legacy bucket fields stay untouched (M18: the
    // seeded minute bucket flows through the existing kind grouping) ...
    assert_eq!(
        body.pointer("/today/valuable_success_count")
            .and_then(|v| v.as_i64()),
        Some(2),
        "legacy valuable bucket unchanged: {body}"
    );
    assert_eq!(
        body.pointer("/today/total_requests")
            .and_then(|v| v.as_i64()),
        Some(2),
        "legacy today total unchanged: {body}"
    );
    // ... and the month window exposes the same additive breakdown.
    let month_providers = body.pointer("/month/providers").and_then(|v| v.as_array());
    assert!(
        month_providers.is_some_and(|list| list.len() == 4),
        "month providers: {body}"
    );

    cleanup_db_dir(&db_path);
}

// ---------- gate (d) HTTP half: public token logs ----------

#[tokio::test]
async fn public_token_logs_carry_provider_dimension() {
    // Given a real access token with two auth_token_logs rows: one static
    // provider kind and one legacy kind
    let db_path = temp_db_path("read-surface-token-logs");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-read-surface-tokens".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let created = proxy
        .create_access_token(Some("read-surface-token"))
        .await
        .expect("create access token");
    let now = chrono::Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
    for (kind_key, kind_label, path, created_at) in [
        (
            "api:brave/web",
            "API | brave/web",
            "/api/brave/res/v1/web/search",
            now - 2,
        ),
        ("api:search", "API | search", "/api/tavily/search", now - 1),
    ] {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, query, http_status, mcp_status,
                request_kind_key, request_kind_label, request_kind_detail,
                result_status, error_message, key_effect_code, binding_effect_code,
                selection_effect_code, counts_business_quota, created_at
            ) VALUES (?, 'POST', ?, NULL, 200, NULL, ?, ?, NULL, 'success', NULL, 'none', 'none', 'none', 1, ?)
            "#,
        )
        .bind(&created.id)
        .bind(path)
        .bind(kind_key)
        .bind(kind_label)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert auth token log");
    }
    drop(pool);

    let state = read_surface_state(proxy, true);
    let app = Router::new()
        .route("/api/public/logs", get(get_public_logs))
        .with_state(state);
    let addr = bind_read_surface_ephemeral(app).await;

    // When reading the public token logs
    let client = Client::new();
    let response = client
        .get(format!(
            "http://{addr}/api/public/logs?token={}&limit=20",
            created.token
        ))
        .send()
        .await
        .expect("public logs request");
    // Then each item carries the provider derived from the static kind
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json body");
    let items = body
        .as_array()
        .unwrap_or_else(|| panic!("items array: {body}"));
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["provider"], json!("tavily"), "legacy kind: {body}");
    assert_eq!(items[1]["provider"], json!("brave"), "brave kind: {body}");

    cleanup_db_dir(&db_path);
}

// ---------- gate (e): catalog registers every todo-3 kind ----------

#[tokio::test]
async fn catalog_registers_all_static_provider_kinds_with_m9_billing_groups() {
    // Given one request_logs row per static provider kind
    let db_path = temp_db_path("read-surface-catalog");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-read-surface-catalog".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let now = chrono::Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
    for (index, (kind, provider)) in PROVIDER_KIND_FIXTURES.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO observability.request_logs (
                api_key_id, auth_token_id, method, path, query,
                status_code, tavily_status_code, error_message, result_status,
                request_kind_key, request_kind_label, provider,
                request_body, response_body, forwarded_headers, dropped_headers,
                created_at
            ) VALUES (?, NULL, 'POST', ?, NULL, 200, NULL, NULL, 'success', ?, 'seeded kind', ?, NULL, NULL, '[]', '[]', ?)
            "#,
        )
        .bind(&key_id)
        .bind(format!("/seeded/{index}"))
        .bind(kind)
        .bind(provider)
        .bind(now - index as i64)
        .execute(&pool)
        .await
        .expect("insert provider kind log");
    }
    drop(pool);
    proxy
        .rebuild_request_log_catalog_for_test()
        .await
        .expect("materialize catalog rollups");

    let admin_password = "read-surface-catalog-password";
    let admin_addr = spawn_builtin_keys_admin_server(proxy, admin_password).await;
    let client = Client::new();
    let cookie = login_admin(&client, admin_addr, admin_password).await;

    // When reading the logs catalog
    let body = admin_get_json(&client, admin_addr, &cookie, "/api/logs/catalog").await;
    // Then every static provider kind is registered server-side with the M9
    // billing group (facade ops are billable) and its protocol group.
    let options = body["requestKindOptions"]
        .as_array()
        .unwrap_or_else(|| panic!("requestKindOptions: {body}"));
    for (kind, _provider) in PROVIDER_KIND_FIXTURES {
        let option = options
            .iter()
            .find(|option| option["key"] == json!(kind))
            .unwrap_or_else(|| panic!("catalog registers {kind}: {body}"));
        assert_eq!(
            option["billing_group"],
            json!("billable"),
            "M9 billing group for {kind}"
        );
        let expected_protocol = if kind.starts_with("mcp:") {
            "mcp"
        } else {
            "api"
        };
        assert_eq!(
            option["protocol_group"],
            json!(expected_protocol),
            "protocol group for {kind}"
        );
    }

    cleanup_db_dir(&db_path);
}

// ---------- item 4 pin: key metrics stay provider-scoped ----------

#[tokio::test]
async fn key_metrics_stay_provider_scoped_via_key_binding() {
    // Given a tavily key (pooled) and a brave key, each with its own logs
    let db_path = temp_db_path("read-surface-key-metrics");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-read-surface-metrics".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list api key metrics")
        .into_iter()
        .next()
        .expect("seeded key exists")
        .id;
    let now = chrono::Utc::now().timestamp();
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, provider, created_at)
        VALUES ('read-surface-brave-key', 'k-read-metrics-brave', 'active', 'brave', ?)
        "#,
    )
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert brave key");
    // fetch_key_summary_since reads per-key daily usage buckets, so seed one
    // bucket per key: the scoping proof is that each key's metrics only ever
    // see its own rows (and therefore its own provider's traffic).
    for (bound_key_id, total, success) in [
        (key_id.clone(), 3_i64, 3_i64),
        ("read-surface-brave-key".to_string(), 2, 2),
    ] {
        sqlx::query(
            r#"
            INSERT INTO observability.api_key_usage_buckets (
                api_key_id, bucket_start, bucket_secs,
                total_requests, success_count, error_count, quota_exhausted_count, updated_at
            ) VALUES (?, ?, 86400, ?, ?, 0, 0, ?)
            "#,
        )
        .bind(&bound_key_id)
        .bind(now)
        .bind(total)
        .bind(success)
        .bind(now)
        .execute(&pool)
        .await
        .expect("insert scoped usage bucket");
    }
    drop(pool);

    let state = read_surface_state(proxy, true);
    let app = Router::new()
        .route("/api/keys/:id/metrics", get(get_key_metrics))
        .with_state(state);
    let addr = bind_read_surface_ephemeral(app).await;
    let client = Client::new();

    // When reading the tavily key's metrics with an explicit since
    let response = client
        .get(format!(
            "http://{addr}/api/keys/{key_id}/metrics?since={}",
            now - 3600
        ))
        .send()
        .await
        .expect("metrics request");
    // Then only the rows bound to THAT key count (provider scoping is
    // automatic via key->provider) and the payload stays legacy-shaped
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body: Value = response.json().await.expect("json body");
    assert_eq!(body["total_requests"], json!(3), "tavily key only: {body}");
    assert_eq!(body["success_count"], json!(3), "tavily key only: {body}");
    assert!(
        body.get("providers").is_none(),
        "key metrics keep the legacy payload: {body}"
    );

    // And the brave key sees only its own rows
    let response = client
        .get(format!(
            "http://{addr}/api/keys/read-surface-brave-key/metrics?since={}",
            now - 3600
        ))
        .send()
        .await
        .expect("metrics request");
    let body: Value = response.json().await.expect("json body");
    assert_eq!(body["total_requests"], json!(2), "brave key only: {body}");

    cleanup_db_dir(&db_path);
}

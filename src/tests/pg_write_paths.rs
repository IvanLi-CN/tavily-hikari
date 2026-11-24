//! Cross-backend write-path contracts.
//!
//! Every sequence runs on a plain SQLite store first — that run is the oracle — and then on
//! a store with an attached PostgreSQL pool. Both traces must match, except for values the
//! task allows to drift: auto-increment ids, sub-minute timestamp skew, and float tails.
//!
//! Set `TAVILY_TEST_PG_URL` (for example
//! `postgres://user:pass@127.0.0.1:5432/dbname`) to exercise the PostgreSQL branch. When the
//! variable is unset — or blank — each test prints one skip line and passes, so local runs
//! and PostgreSQL-less CI stay green.
//!
//! Row identifiers are prefixed with `nanoid!()` so parallel tests can share one database.
//! Each store applies its own bootstrap/migrations; nothing here writes DDL.

use super::*;

const PG_URL_ENV: &str = "TAVILY_TEST_PG_URL";
const SECS_PER_MINUTE_I64: i64 = 60;

// ---------------------------------------------------------------------------
// Backend harness
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Sqlite,
    Postgres,
}

impl Backend {
    fn of(store: &KeyStore) -> Self {
        if store.pg_pool.is_some() {
            Self::Postgres
        } else {
            Self::Sqlite
        }
    }

    /// SQLite keeps the core relations in the main schema, PostgreSQL in `core`.
    fn core(self, relation: &str) -> String {
        match self {
            Self::Sqlite => relation.to_string(),
            Self::Postgres => format!("core.{relation}"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Bind<'a> {
    Text(&'a str),
    Int(i64),
}

/// Rewrite positional `?` placeholders into PostgreSQL `$n` placeholders.
fn postgres_placeholders(sql: &str) -> String {
    let mut rewritten = String::with_capacity(sql.len() + 8);
    let mut index = 0_usize;
    for character in sql.chars() {
        if character == '?' {
            index += 1;
            rewritten.push('$');
            rewritten.push_str(&index.to_string());
        } else {
            rewritten.push(character);
        }
    }
    rewritten
}

async fn fetch_scalar(store: &KeyStore, backend: Backend, sql: &str, binds: &[Bind<'_>]) -> i64 {
    match backend {
        Backend::Sqlite => {
            let mut query = sqlx::query_scalar::<_, i64>(sql);
            for value in binds {
                query = match value {
                    Bind::Text(text) => query.bind(*text),
                    Bind::Int(number) => query.bind(*number),
                };
            }
            query
                .fetch_one(&store.pool)
                .await
                .unwrap_or_else(|err| panic!("sqlite scalar `{sql}`: {err}"))
        }
        Backend::Postgres => {
            let sql = postgres_placeholders(sql);
            let mut query = sqlx::query_scalar::<_, i64>(&sql);
            for value in binds {
                query = match value {
                    Bind::Text(text) => query.bind(*text),
                    Bind::Int(number) => query.bind(*number),
                };
            }
            query
                .fetch_one(store.pg().expect("postgres pool"))
                .await
                .unwrap_or_else(|err| panic!("postgres scalar `{sql}`: {err}"))
        }
    }
}

async fn fetch_pairs(
    store: &KeyStore,
    backend: Backend,
    sql: &str,
    binds: &[Bind<'_>],
) -> Vec<(String, i64)> {
    match backend {
        Backend::Sqlite => {
            let mut query = sqlx::query_as::<_, (String, i64)>(sql);
            for value in binds {
                query = match value {
                    Bind::Text(text) => query.bind(*text),
                    Bind::Int(number) => query.bind(*number),
                };
            }
            query
                .fetch_all(&store.pool)
                .await
                .unwrap_or_else(|err| panic!("sqlite rows `{sql}`: {err}"))
        }
        Backend::Postgres => {
            let sql = postgres_placeholders(sql);
            let mut query = sqlx::query_as::<_, (String, i64)>(&sql);
            for value in binds {
                query = match value {
                    Bind::Text(text) => query.bind(*text),
                    Bind::Int(number) => query.bind(*number),
                };
            }
            query
                .fetch_all(store.pg().expect("postgres pool"))
                .await
                .unwrap_or_else(|err| panic!("postgres rows `{sql}`: {err}"))
        }
    }
}

async fn execute_statement(
    store: &KeyStore,
    backend: Backend,
    sql: &str,
    binds: &[Bind<'_>],
) -> u64 {
    match backend {
        Backend::Sqlite => {
            let mut query = sqlx::query(sql);
            for value in binds {
                query = match value {
                    Bind::Text(text) => query.bind(*text),
                    Bind::Int(number) => query.bind(*number),
                };
            }
            query
                .execute(&store.pool)
                .await
                .unwrap_or_else(|err| panic!("sqlite execute `{sql}`: {err}"))
                .rows_affected()
        }
        Backend::Postgres => {
            let sql = postgres_placeholders(sql);
            let mut query = sqlx::query(&sql);
            for value in binds {
                query = match value {
                    Bind::Text(text) => query.bind(*text),
                    Bind::Int(number) => query.bind(*number),
                };
            }
            query
                .execute(store.pg().expect("postgres pool"))
                .await
                .unwrap_or_else(|err| panic!("postgres execute `{sql}`: {err}"))
                .rows_affected()
        }
    }
}

struct StorePair {
    sqlite: TavilyProxy,
    postgres: TavilyProxy,
}

impl StorePair {
    /// `None` means the PostgreSQL branch is not configured: the caller returns early.
    async fn open(prefix: &str, now: i64) -> Option<Self> {
        let url = match std::env::var(PG_URL_ENV) {
            Ok(url) if !url.trim().is_empty() => url,
            _ => {
                println!("pg write paths: skipped ({PG_URL_ENV} unset)");
                return None;
            }
        };
        apply_postgres_migrations(&url).await;
        let sqlite = open_proxy(&format!("{prefix}-sqlite"), now, None).await;
        let postgres = open_proxy(&format!("{prefix}-postgres"), now, Some(url)).await;
        Some(Self { sqlite, postgres })
    }
}

/// Production applies the embedded migrations during the PostgreSQL startup gate. This harness
/// opens the pool directly, so it runs them once per database before any statement executes.
async fn apply_postgres_migrations(url: &str) {
    let pool = sqlx::PgPool::connect(url)
        .await
        .unwrap_or_else(|err| panic!("connect postgres for migrations: {err}"));
    crate::store::pg::PgStore::run_migrations(&pool)
        .await
        .unwrap_or_else(|err| panic!("apply postgres migrations: {err}"));
    pool.close().await;
}

/// Build a store on a throwaway SQLite file. Passing `database_url` attaches the PostgreSQL
/// pool through the same startup path production uses (`attach_postgres`).
async fn open_proxy(prefix: &str, now: i64, database_url: Option<String>) -> TavilyProxy {
    let db_path = temp_db_path(prefix);
    let database_path = db_path.to_string_lossy().to_string();
    let (backend_time, _manual) = BackendTime::manual_from_ts(now);
    let options = TavilyProxyOptions {
        database_url,
        ..TavilyProxyOptions::from_database_path(&database_path)
    };
    TavilyProxy::with_options_and_time(
        // An empty key set keeps `sync_keys` from soft-deleting rows other tests own in a
        // shared PostgreSQL database; every test seeds only the keys it needs.
        Vec::<String>::new(),
        DEFAULT_UPSTREAM,
        &database_path,
        options,
        backend_time,
    )
    .await
    .unwrap_or_else(|err| panic!("open {prefix} store: {err}"))
}

/// Deterministic-but-unique manual clock. The timestamp is minute aligned plus 30s, derived
/// from a nanoid so rollup buckets stay private to one test even when several shards share
/// one PostgreSQL database.
fn unique_epoch() -> i64 {
    let tag = nanoid!(12);
    let hash = tag
        .bytes()
        .fold(0_i64, |acc, byte| (acc * 31 + i64::from(byte)) % 90_000_000);
    1_500_000_000 + (hash / SECS_PER_MINUTE_I64) * SECS_PER_MINUTE_I64 + 30
}

fn minute_bucket(timestamp: i64) -> i64 {
    timestamp - timestamp.rem_euclid(SECS_PER_MINUTE_I64)
}

// ---------------------------------------------------------------------------
// 1. Pending-billing settlement (reserve -> settle -> duplicate settle -> refund)
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct QuotaTrace {
    balance_after_reserve: i64,
    pending_rows_after_reserve: i64,
    credit_sum_after_reserve: i64,
    settle_outcome: String,
    balance_after_settle: i64,
    charged_rows_after_settle: i64,
    duplicate_settle_outcome: String,
    balance_after_duplicate_settle: i64,
    charged_rows_after_duplicate_settle: i64,
    credit_sum_after_duplicate_settle: i64,
    refund_settle_outcome: String,
    balance_after_refund: i64,
    charged_rows_after_refund: i64,
    credit_sum_after_refund: i64,
}

async fn account_monthly_balance(store: &KeyStore, backend: Backend, user_id: &str) -> i64 {
    let relation = backend.core("account_monthly_quota");
    fetch_scalar(
        store,
        backend,
        &format!(
            "SELECT CAST(COALESCE((SELECT month_count FROM {relation} WHERE user_id = ?), 0) AS BIGINT)"
        ),
        &[Bind::Text(user_id)],
    )
    .await
}

async fn ledger_state_rows(store: &KeyStore, backend: Backend, token_id: &str, state: &str) -> i64 {
    let relation = backend.core("billing_ledger");
    fetch_scalar(
        store,
        backend,
        &format!(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM {relation} WHERE token_id = ? AND billing_state = ?"
        ),
        &[Bind::Text(token_id), Bind::Text(state)],
    )
    .await
}

async fn token_log_credit_sum(store: &KeyStore, backend: Backend, token_id: &str) -> i64 {
    let relation = backend.core("auth_token_logs");
    fetch_scalar(
        store,
        backend,
        &format!(
            "SELECT CAST(COALESCE(SUM(business_credits), 0) AS BIGINT) FROM {relation} WHERE token_id = ?"
        ),
        &[Bind::Text(token_id)],
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn reserve_pending_billing(
    proxy: &TavilyProxy,
    token_id: &str,
    subject: &str,
    credits: i64,
    counts_business_quota: bool,
) -> i64 {
    proxy
        .record_pending_billing_attempt_for_subject(
            token_id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(200),
            None,
            counts_business_quota,
            OUTCOME_SUCCESS,
            None,
            credits,
            subject,
            None,
        )
        .await
        .unwrap_or_else(|err| panic!("reserve pending billing: {err}"))
}

async fn run_quota_sequence(proxy: &TavilyProxy, tag: &str) -> QuotaTrace {
    let store = proxy.key_store.as_ref();
    let backend = Backend::of(store);

    let user_id = store
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "pg-write-paths".to_string(),
            provider_user_id: format!("{tag}-quota-user"),
            username: Some(format!("{tag}-quota-user")),
            name: Some("Quota user".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("create quota user")
        .user_id;
    let token = store
        .create_access_token(Some("pg write paths quota"))
        .await
        .expect("create quota token");
    let subject = format!("account:{user_id}");

    // reserve: the durable ledger row exists while the account balance is untouched.
    let reserve_log = reserve_pending_billing(proxy, &token.id, &subject, 4, true).await;
    let balance_after_reserve = account_monthly_balance(store, backend, &user_id).await;
    let pending_rows_after_reserve = ledger_state_rows(store, backend, &token.id, "pending").await;
    let credit_sum_after_reserve = token_log_credit_sum(store, backend, &token.id).await;

    // settle: the reservation becomes a charge exactly once.
    let settle_outcome = format!(
        "{:?}",
        proxy
            .settle_pending_billing_attempt(reserve_log)
            .await
            .expect("settle pending billing")
    );
    let balance_after_settle = account_monthly_balance(store, backend, &user_id).await;
    let charged_rows_after_settle = ledger_state_rows(store, backend, &token.id, "charged").await;

    // duplicate settle must not charge twice.
    let duplicate_settle_outcome = format!(
        "{:?}",
        proxy
            .settle_pending_billing_attempt(reserve_log)
            .await
            .expect("re-settle pending billing")
    );
    let balance_after_duplicate_settle = account_monthly_balance(store, backend, &user_id).await;
    let charged_rows_after_duplicate_settle =
        ledger_state_rows(store, backend, &token.id, "charged").await;
    let credit_sum_after_duplicate_settle = token_log_credit_sum(store, backend, &token.id).await;

    // "refund" substitute: the store has no negative-credit path, so the closest existing
    // contract is the zero-credit settlement branch. It marks the reservation terminal
    // without moving the balance — the same observable a full refund leaves behind.
    let refund_log = reserve_pending_billing(proxy, &token.id, &subject, 0, true).await;
    let refund_settle_outcome = format!(
        "{:?}",
        proxy
            .settle_pending_billing_attempt(refund_log)
            .await
            .expect("settle zero-credit pending billing")
    );
    let balance_after_refund = account_monthly_balance(store, backend, &user_id).await;
    let charged_rows_after_refund = ledger_state_rows(store, backend, &token.id, "charged").await;
    let credit_sum_after_refund = token_log_credit_sum(store, backend, &token.id).await;

    QuotaTrace {
        balance_after_reserve,
        pending_rows_after_reserve,
        credit_sum_after_reserve,
        settle_outcome,
        balance_after_settle,
        charged_rows_after_settle,
        duplicate_settle_outcome,
        balance_after_duplicate_settle,
        charged_rows_after_duplicate_settle,
        credit_sum_after_duplicate_settle,
        refund_settle_outcome,
        balance_after_refund,
        charged_rows_after_refund,
        credit_sum_after_refund,
    }
}

#[tokio::test]
async fn quota_settlement_matches_across_backends() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-write-quota-settlement", now).await else {
        return;
    };
    let tag = nanoid!(10);

    let sqlite = run_quota_sequence(&pair.sqlite, &tag).await;
    let postgres = run_quota_sequence(&pair.postgres, &tag).await;

    assert_eq!(
        sqlite, postgres,
        "quota settlement diverged between SQLite and PostgreSQL"
    );
    // Invariants that must hold on either backend, not just match.
    assert_eq!(sqlite.balance_after_reserve, 0);
    assert_eq!(sqlite.pending_rows_after_reserve, 1);
    assert_eq!(sqlite.credit_sum_after_reserve, 4);
    assert_eq!(sqlite.settle_outcome, "Charged");
    assert_eq!(sqlite.balance_after_settle, 4);
    assert_eq!(sqlite.duplicate_settle_outcome, "AlreadySettled");
    assert_eq!(
        sqlite.balance_after_duplicate_settle, sqlite.balance_after_settle,
        "a duplicate settle must not charge the account twice"
    );
    assert_eq!(
        sqlite.charged_rows_after_duplicate_settle,
        sqlite.charged_rows_after_settle
    );
    assert_eq!(sqlite.refund_settle_outcome, "Charged");
    assert_eq!(sqlite.balance_after_refund, sqlite.balance_after_settle);
    assert_eq!(sqlite.credit_sum_after_refund, 4);
}

// ---------------------------------------------------------------------------
// 2. Request log flush -> rollup deltas
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct RequestLogTrace {
    detail_count: i64,
    visibility_counts: Vec<(String, i64)>,
    key_bucket_total: i64,
    key_bucket_success: i64,
    key_bucket_error: i64,
    dashboard_bucket_total: i64,
    dashboard_bucket_success: i64,
    dashboard_bucket_error: i64,
}

fn attempt_log_entry<'a>(
    key_id: &'a str,
    token_id: &'a str,
    outcome: &'a str,
    status: StatusCode,
    headers: &'a [String],
) -> AttemptLog<'a> {
    AttemptLog {
        key_id: Some(key_id),
        auth_token_id: Some(token_id),
        method: &Method::POST,
        path: "/api/tavily/search",
        query: Some("q=pg-write-paths"),
        status: Some(status),
        tavily_status_code: Some(i64::from(status.as_u16())),
        error: None,
        request_body: b"{\"query\":\"pg-write-paths\"}",
        response_body: b"{\"results\":[]}",
        outcome,
        failure_kind: None,
        key_effect_code: KEY_EFFECT_NONE,
        key_effect_summary: None,
        binding_effect_code: KEY_EFFECT_NONE,
        binding_effect_summary: None,
        selection_effect_code: KEY_EFFECT_NONE,
        selection_effect_summary: None,
        gateway_mode: None,
        experiment_variant: None,
        proxy_session_id: None,
        routing_subject_hash: None,
        upstream_operation: Some("http_search"),
        fallback_reason: None,
        forwarded_headers: headers,
        dropped_headers: headers,
        client_ip: None,
    }
}

async fn run_request_log_flush_sequence(
    proxy: &TavilyProxy,
    tag: &str,
    now: i64,
) -> RequestLogTrace {
    let store = proxy.key_store.as_ref();
    let backend = Backend::of(store);

    let key = format!("{tag}-request-log-key");
    let (key_id, _) = store
        .add_or_undelete_key_with_status(&key)
        .await
        .expect("create request log key");
    let token = store
        .create_access_token(Some("pg write paths request logs"))
        .await
        .expect("create request log token");

    let headers: Vec<String> = Vec::new();
    let request_log_ids = [
        store
            .log_attempt(attempt_log_entry(
                &key_id,
                &token.id,
                OUTCOME_SUCCESS,
                StatusCode::OK,
                &headers,
            ))
            .await
            .expect("log successful attempt"),
        store
            .log_attempt(attempt_log_entry(
                &key_id,
                &token.id,
                OUTCOME_SUCCESS,
                StatusCode::OK,
                &headers,
            ))
            .await
            .expect("log successful attempt"),
        store
            .log_attempt(attempt_log_entry(
                &key_id,
                &token.id,
                OUTCOME_SUCCESS,
                StatusCode::OK,
                &headers,
            ))
            .await
            .expect("log successful attempt"),
        store
            .log_attempt(attempt_log_entry(
                &key_id,
                &token.id,
                OUTCOME_ERROR,
                StatusCode::INTERNAL_SERVER_ERROR,
                &headers,
            ))
            .await
            .expect("log failed attempt"),
    ];

    // One hidden row proves visibility grouping is compared, not assumed.
    execute_statement(
        store,
        backend,
        "UPDATE observability.request_logs SET visibility = ? WHERE id = ?",
        &[Bind::Text("hidden"), Bind::Int(request_log_ids[0])],
    )
    .await;

    store
        .flush_request_stats_writes()
        .await
        .expect("flush request stats writes");

    let detail_count = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COUNT(*) AS BIGINT) FROM observability.request_logs WHERE api_key_id = ?",
        &[Bind::Text(&key_id)],
    )
    .await;
    let visibility_counts = fetch_pairs(
        store,
        backend,
        "SELECT visibility, CAST(COUNT(*) AS BIGINT) FROM observability.request_logs \
         WHERE api_key_id = ? GROUP BY visibility ORDER BY visibility",
        &[Bind::Text(&key_id)],
    )
    .await;
    let key_bucket_total = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COALESCE(SUM(total_requests), 0) AS BIGINT) FROM observability.api_key_usage_buckets \
         WHERE api_key_id = ?",
        &[Bind::Text(&key_id)],
    )
    .await;
    let key_bucket_success = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COALESCE(SUM(success_count), 0) AS BIGINT) FROM observability.api_key_usage_buckets \
         WHERE api_key_id = ?",
        &[Bind::Text(&key_id)],
    )
    .await;
    let key_bucket_error = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COALESCE(SUM(error_count), 0) AS BIGINT) FROM observability.api_key_usage_buckets \
         WHERE api_key_id = ?",
        &[Bind::Text(&key_id)],
    )
    .await;
    let bucket_start = minute_bucket(now);
    let dashboard_bucket_total = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COALESCE(SUM(total_requests), 0) AS BIGINT) FROM observability.dashboard_request_rollup_buckets \
         WHERE bucket_start = ? AND bucket_secs = ?",
        &[Bind::Int(bucket_start), Bind::Int(SECS_PER_MINUTE_I64)],
    )
    .await;
    let dashboard_bucket_success = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COALESCE(SUM(success_count), 0) AS BIGINT) FROM observability.dashboard_request_rollup_buckets \
         WHERE bucket_start = ? AND bucket_secs = ?",
        &[Bind::Int(bucket_start), Bind::Int(SECS_PER_MINUTE_I64)],
    )
    .await;
    let dashboard_bucket_error = fetch_scalar(
        store,
        backend,
        "SELECT CAST(COALESCE(SUM(error_count), 0) AS BIGINT) FROM observability.dashboard_request_rollup_buckets \
         WHERE bucket_start = ? AND bucket_secs = ?",
        &[Bind::Int(bucket_start), Bind::Int(SECS_PER_MINUTE_I64)],
    )
    .await;

    RequestLogTrace {
        detail_count,
        visibility_counts,
        key_bucket_total,
        key_bucket_success,
        key_bucket_error,
        dashboard_bucket_total,
        dashboard_bucket_success,
        dashboard_bucket_error,
    }
}

#[tokio::test]
async fn request_log_flush_matches_across_backends() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-write-request-log-flush", now).await else {
        return;
    };
    let tag = nanoid!(10);

    let sqlite = run_request_log_flush_sequence(&pair.sqlite, &tag, now).await;
    let postgres = run_request_log_flush_sequence(&pair.postgres, &tag, now).await;

    assert_eq!(
        sqlite, postgres,
        "request log flush diverged between SQLite and PostgreSQL"
    );
    assert_eq!(sqlite.detail_count, 4);
    assert_eq!(
        sqlite.visibility_counts,
        vec![("hidden".to_string(), 1), ("visible".to_string(), 3)]
    );
    // Bucket conservation: the flushed rollup covers every detail row exactly once.
    assert_eq!(sqlite.key_bucket_total, sqlite.detail_count);
    assert_eq!(sqlite.key_bucket_success, 3);
    assert_eq!(sqlite.key_bucket_error, 1);
    assert_eq!(sqlite.dashboard_bucket_total, sqlite.detail_count);
    assert_eq!(sqlite.dashboard_bucket_success, 3);
    assert_eq!(sqlite.dashboard_bucket_error, 1);
}

// ---------------------------------------------------------------------------
// 3. Alert projection rebuild idempotency
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct AlertTrace {
    event_total: i64,
    group_total: i64,
    group_counts: Vec<(String, i64)>,
    group_event_total: i64,
}

async fn seed_alert_events(proxy: &TavilyProxy, token_id: &str, now: i64) {
    // The alert projection is not ported to PostgreSQL yet: it reads the SQLite source lane
    // even when a pool is attached. Both store configurations therefore get the same
    // SQLite-only seed, which is exactly the contract this case pins down.
    let rows: [(&str, Option<&str>, i64, Option<&str>); 4] = [
        (
            "quota_exhausted",
            None,
            0,
            Some("user request rate limit exceeded on rolling 5m window"),
        ),
        (
            "quota_exhausted",
            None,
            0,
            Some("user request rate limit exceeded on rolling 5m window"),
        ),
        (
            "quota_exhausted",
            None,
            1,
            Some("account monthly credits exhausted"),
        ),
        (
            OUTCOME_ERROR,
            Some("upstream_rate_limited_429"),
            1,
            Some("upstream returned 429"),
        ),
    ];
    for (index, (result_status, failure_kind, counts_business_quota, error_message)) in
        rows.iter().enumerate()
    {
        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id, method, path, request_kind_key, request_kind_label, request_kind_detail,
                result_status, error_message, failure_kind, key_effect_code, binding_effect_code,
                selection_effect_code, counts_business_quota, created_at
            ) VALUES (?, 'POST', '/mcp', 'mcp_call', 'MCP call', 'MCP call', ?, ?, ?, 'none', 'none', 'none', ?, ?)
            "#,
        )
        .bind(token_id)
        .bind(*result_status)
        .bind(*error_message)
        .bind(*failure_kind)
        .bind(*counts_business_quota)
        .bind(now - 300 + index as i64)
        .execute(&proxy.key_store.pool)
        .await
        .expect("seed alert source row");
    }
}

/// Admission-gated reads answer `Deferred` under contention; tests must retry, not fail.
fn is_deferred_admission(error: &ProxyError) -> bool {
    matches!(error, ProxyError::Deferred { .. })
}

async fn alert_snapshot(proxy: &TavilyProxy, token_id: &str) -> AlertTrace {
    // The alert read model is admission-gated; a parallel test can hold the read budget.
    // A deferred read is a retry contract, not a failure.
    let mut attempts = 0_u32;
    let events = loop {
        attempts += 1;
        match proxy
            .key_store
            .fetch_alert_events_page(None, None, None, None, Some(token_id), None, &[], 1, 100)
            .await
        {
            Ok(events) => break events,
            Err(err) if is_deferred_admission(&err) && attempts < 1200 => {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(err) => panic!("read alert events: {err}"),
        }
    };
    attempts = 0;
    let groups = loop {
        attempts += 1;
        match proxy
            .key_store
            .fetch_alert_groups_page(None, None, None, None, Some(token_id), None, &[], 1, 100)
            .await
        {
            Ok(groups) => break groups,
            Err(err) if is_deferred_admission(&err) && attempts < 1200 => {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(err) => panic!("read alert groups: {err}"),
        }
    };
    let mut group_counts = groups
        .items
        .iter()
        .map(|group| (group.alert_type.clone(), group.count))
        .collect::<Vec<_>>();
    group_counts.sort();
    AlertTrace {
        event_total: events.total,
        group_total: groups.total,
        group_event_total: group_counts.iter().map(|(_, count)| *count).sum(),
        group_counts,
    }
}

async fn advance_alert_projection_until(
    proxy: &TavilyProxy,
    token_id: &str,
    expected_events: i64,
) -> AlertTrace {
    let mut last = None;
    for _ in 0..240 {
        let _ = proxy
            .advance_dashboard_alert_projection_slice()
            .await
            .expect("advance alert projection slice");
        let trace = alert_snapshot(proxy, token_id).await;
        let mut status_attempts = 0_u32;
        let status = loop {
            status_attempts += 1;
            match proxy.key_store.alert_projection_status().await {
                Ok(status) => break status,
                Err(err) if is_deferred_admission(&err) && status_attempts < 1200 => {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(err) => panic!("read alert projection coverage: {err}"),
            }
        };
        // Waiting for coverage is required: before the projection catches up the read path
        // falls back to the raw source CTE, which would mask a broken projection rebuild.
        if status.coverage == "ok" && trace.event_total == expected_events {
            return trace;
        }
        last = Some(trace);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("alert projection did not converge: last snapshot {last:?}");
}

async fn reset_alert_projection(proxy: &TavilyProxy) {
    for statement in [
        "DELETE FROM observability.dashboard_alert_projection_events",
        "DELETE FROM observability.dashboard_alert_projection_recent_summaries",
        "UPDATE observability.dashboard_alert_projection_state SET cursor_occurred_at = 0, \
         cursor_row_sort_id = '', fence_occurred_at = NULL, fence_row_sort_id = NULL, \
         phase = 'catching_up', stale_reason = NULL",
        "UPDATE observability.dashboard_alert_projection_history_state SET cursor_occurred_at = 0, \
         cursor_row_sort_id = '', fence_occurred_at = NULL, fence_row_sort_id = NULL, \
         phase = 'catching_up'",
    ] {
        sqlx::query(statement)
            .execute(&proxy.key_store.pool)
            .await
            .unwrap_or_else(|err| panic!("reset alert projection `{statement}`: {err}"));
    }
}

async fn run_alert_rebuild_sequence(proxy: &TavilyProxy, now: i64) -> (AlertTrace, AlertTrace) {
    let store = proxy.key_store.as_ref();
    let token = store
        .create_access_token(Some("pg write paths alerts"))
        .await
        .expect("create alert source token");
    seed_alert_events(proxy, &token.id, now).await;

    let first = advance_alert_projection_until(proxy, &token.id, 4).await;
    reset_alert_projection(proxy).await;
    let second = advance_alert_projection_until(proxy, &token.id, 4).await;
    (first, second)
}

#[tokio::test]
async fn alert_projection_rebuild_is_idempotent() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-write-alert-projection", now).await else {
        return;
    };

    let (sqlite_first, sqlite_second) = run_alert_rebuild_sequence(&pair.sqlite, now).await;
    let (postgres_first, postgres_second) = run_alert_rebuild_sequence(&pair.postgres, now).await;

    assert_eq!(
        sqlite_first, sqlite_second,
        "rebuilding the alert projection twice must reproduce the first result"
    );
    assert_eq!(
        sqlite_first, postgres_first,
        "alert projection diverged between SQLite and PostgreSQL stores"
    );
    assert_eq!(
        postgres_first, postgres_second,
        "rebuilding the alert projection twice must reproduce the first result on PostgreSQL"
    );
    assert_eq!(sqlite_first.event_total, 4);
    // Group counts aggregate the deduplicated events; conservation holds on both backends.
    assert_eq!(sqlite_first.group_event_total, sqlite_first.event_total);
    assert!(sqlite_first.group_total > 0);
}

// ---------------------------------------------------------------------------
// 4. MCP session lifecycle
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct SessionView {
    protocol_version: Option<String>,
    last_event_id: Option<String>,
    /// Seconds relative to the manual clock, matching the tolerated skew rule.
    expires_in: i64,
    updated_in: i64,
    revoked_at_present: bool,
    revoke_reason: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct McpSessionTrace {
    created: Option<SessionView>,
    after_touch: Option<SessionView>,
    after_second_touch: Option<SessionView>,
    after_revoke: Option<SessionView>,
    latest_active_for_token: Option<SessionView>,
}

fn session_view(binding: &McpSessionBinding, now: i64) -> SessionView {
    SessionView {
        protocol_version: binding.protocol_version.clone(),
        last_event_id: binding.last_event_id.clone(),
        expires_in: binding.expires_at - now,
        updated_in: binding.updated_at - now,
        revoked_at_present: binding.revoked_at.is_some(),
        revoke_reason: binding.revoke_reason.clone(),
    }
}

async fn read_session(proxy: &TavilyProxy, session_id: &str, now: i64) -> Option<SessionView> {
    proxy
        .get_active_mcp_session(session_id)
        .await
        .expect("read active mcp session")
        .map(|binding| session_view(&binding, now))
}

async fn run_mcp_session_sequence(proxy: &TavilyProxy, tag: &str, now: i64) -> McpSessionTrace {
    let store = proxy.key_store.as_ref();
    let key = format!("{tag}-mcp-session-key");
    let (key_id, _) = store
        .add_or_undelete_key_with_status(&key)
        .await
        .expect("create mcp session key");

    let session_id = proxy
        .create_mcp_session(
            &format!("{tag}-upstream-session"),
            &key_id,
            None,
            None,
            Some("2025-03-26"),
            None,
        )
        .await
        .expect("create mcp session");
    let created = read_session(proxy, &session_id, now).await;

    store
        .touch_mcp_session(
            &session_id,
            Some("2025-06-18"),
            Some("event-42"),
            now + 5,
            now + 7200,
        )
        .await
        .expect("touch mcp session");
    let after_touch = read_session(proxy, &session_id, now).await;

    // A touch without new identity fields must keep the stored values (COALESCE semantics).
    store
        .touch_mcp_session(&session_id, None, None, now + 10, now + 9000)
        .await
        .expect("touch mcp session without protocol fields");
    let after_second_touch = read_session(proxy, &session_id, now).await;

    store
        .revoke_mcp_session(&session_id, "pg write path test revoke")
        .await
        .expect("revoke mcp session");
    let after_revoke = read_session(proxy, &session_id, now).await;
    let latest_active_for_token = proxy
        .latest_active_mcp_session_for_token("no-such-token")
        .await
        .expect("read latest session for unbound token")
        .map(|binding| session_view(&binding, now));

    McpSessionTrace {
        created,
        after_touch,
        after_second_touch,
        after_revoke,
        latest_active_for_token,
    }
}

#[tokio::test]
async fn mcp_session_lifecycle_matches_across_backends() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-write-mcp-session", now).await else {
        return;
    };
    let tag = nanoid!(10);

    let sqlite = run_mcp_session_sequence(&pair.sqlite, &tag, now).await;
    let postgres = run_mcp_session_sequence(&pair.postgres, &tag, now).await;

    assert_eq!(
        sqlite, postgres,
        "mcp session lifecycle diverged between SQLite and PostgreSQL"
    );
    let created = sqlite
        .created
        .as_ref()
        .expect("session visible after create");
    assert_eq!(created.protocol_version.as_deref(), Some("2025-03-26"));
    assert_eq!(created.last_event_id, None);
    let touched = sqlite
        .after_touch
        .as_ref()
        .expect("session visible after touch");
    assert_eq!(touched.protocol_version.as_deref(), Some("2025-06-18"));
    assert_eq!(touched.last_event_id.as_deref(), Some("event-42"));
    assert_eq!(touched.expires_in, 7200);
    let touched_again = sqlite
        .after_second_touch
        .as_ref()
        .expect("session visible after second touch");
    assert_eq!(
        touched_again.protocol_version, touched.protocol_version,
        "a touch without protocol fields must not clear them"
    );
    assert_eq!(touched_again.last_event_id, touched.last_event_id);
    assert_eq!(touched_again.expires_in, 9000);
    assert_eq!(
        sqlite.after_revoke, None,
        "a revoked session must no longer be resolvable"
    );
    assert_eq!(sqlite.latest_active_for_token, None);
}

// ---------------------------------------------------------------------------
// 5. API key lifecycle
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct KeyStateView {
    status: Option<String>,
    quarantined: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct ApiKeyTrace {
    create_outcome: String,
    created: KeyStateView,
    quarantine_outcome: bool,
    quarantined: KeyStateView,
    second_quarantine_outcome: bool,
    second_quarantined: KeyStateView,
    clear_outcome: bool,
    restored: KeyStateView,
    restore_active_outcome: bool,
    deleted: KeyStateView,
    quarantine_rows: i64,
    uncleared_quarantine_rows: i64,
    deleted_at_present: bool,
}

async fn key_state(store: &KeyStore, key_id: &str) -> KeyStateView {
    let snapshot = store
        .fetch_key_state_snapshot(key_id)
        .await
        .expect("read key state snapshot");
    KeyStateView {
        status: snapshot.status,
        quarantined: snapshot.quarantined,
    }
}

async fn run_api_key_sequence(proxy: &TavilyProxy, tag: &str) -> ApiKeyTrace {
    let store = proxy.key_store.as_ref();
    let backend = Backend::of(store);
    let key = format!("{tag}-api-key");

    let (key_id, create_outcome) = store
        .add_or_undelete_key_with_status(&key)
        .await
        .expect("create api key");
    let created = key_state(store, &key_id).await;

    let quarantine_outcome = store
        .quarantine_key_by_id(
            &key_id,
            "pg-write-paths",
            "manual_review",
            "quarantined by pg write path test",
            "test detail",
        )
        .await
        .expect("quarantine api key");
    let quarantined = key_state(store, &key_id).await;

    let second_quarantine_outcome = store
        .quarantine_key_by_id(
            &key_id,
            "pg-write-paths",
            "manual_review_updated",
            "quarantine updated",
            "test detail",
        )
        .await
        .expect("re-quarantine api key");
    let second_quarantined = key_state(store, &key_id).await;

    let clear_outcome = store
        .clear_key_quarantine_by_id(&key_id)
        .await
        .expect("clear api key quarantine");
    let restored = key_state(store, &key_id).await;
    let restore_active_outcome = store
        .restore_active_status(&key)
        .await
        .expect("restore active status");

    store
        .soft_delete_key_by_id(&key_id)
        .await
        .expect("soft delete api key");
    let deleted = key_state(store, &key_id).await;

    let quarantines = backend.core("api_key_quarantines");
    let quarantine_rows = fetch_scalar(
        store,
        backend,
        &format!("SELECT CAST(COUNT(*) AS BIGINT) FROM {quarantines} WHERE key_id = ?"),
        &[Bind::Text(&key_id)],
    )
    .await;
    let uncleared_quarantine_rows = fetch_scalar(
        store,
        backend,
        &format!(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM {quarantines} WHERE key_id = ? AND cleared_at IS NULL"
        ),
        &[Bind::Text(&key_id)],
    )
    .await;
    let keys = backend.core("api_keys");
    let deleted_at_present = fetch_scalar(
        store,
        backend,
        &format!(
            "SELECT CAST(COUNT(*) AS BIGINT) FROM {keys} WHERE id = ? AND deleted_at IS NOT NULL"
        ),
        &[Bind::Text(&key_id)],
    )
    .await
        == 1;

    ApiKeyTrace {
        create_outcome: format!("{create_outcome:?}"),
        created,
        quarantine_outcome,
        quarantined,
        second_quarantine_outcome,
        second_quarantined,
        clear_outcome,
        restored,
        restore_active_outcome,
        deleted,
        quarantine_rows,
        uncleared_quarantine_rows,
        deleted_at_present,
    }
}

#[tokio::test]
async fn api_key_lifecycle_matches_across_backends() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-write-api-key", now).await else {
        return;
    };
    let tag = nanoid!(10);

    let sqlite = run_api_key_sequence(&pair.sqlite, &tag).await;
    let postgres = run_api_key_sequence(&pair.postgres, &tag).await;

    assert_eq!(
        sqlite, postgres,
        "api key lifecycle diverged between SQLite and PostgreSQL"
    );
    assert_eq!(sqlite.create_outcome, "Created");
    assert_eq!(
        sqlite.created,
        KeyStateView {
            status: Some("active".to_string()),
            quarantined: false,
        }
    );
    assert!(sqlite.quarantine_outcome);
    assert_eq!(
        sqlite.quarantined,
        KeyStateView {
            status: Some("active".to_string()),
            quarantined: true,
        }
    );
    assert!(
        !sqlite.second_quarantine_outcome,
        "an uncleared quarantine must be updated, not duplicated"
    );
    assert_eq!(sqlite.second_quarantined, sqlite.quarantined);
    assert!(sqlite.clear_outcome);
    assert_eq!(
        sqlite.restored,
        KeyStateView {
            status: Some("active".to_string()),
            quarantined: false,
        }
    );
    assert!(
        !sqlite.restore_active_outcome,
        "an already-active key is not an exhausted key to restore"
    );
    assert_eq!(
        sqlite.deleted,
        KeyStateView {
            status: None,
            quarantined: false,
        }
    );
    assert_eq!(sqlite.quarantine_rows, 1);
    assert_eq!(sqlite.uncleared_quarantine_rows, 0);
    assert!(sqlite.deleted_at_present);
}

// ---------------------------------------------------------------------------
// 2x. Canonical alert page (converted reader)
// ---------------------------------------------------------------------------

fn canonical_alert_payload(source_id: &str, occurred_at: i64) -> String {
    format!(
        r#"{{"source_kind":"auth_token_log","source_id":"{source_id}","row_sort_id":"{source_id}",
        "alert_type":"upstream_rate_limited_429","occurred_at":{occurred_at},
        "token_id":null,"key_id":null,"request_log_id":null,"method":null,"path":null,"query":null,
        "request_kind_key":null,"request_kind_label":null,"request_kind_detail":null,
        "result_status":null,"failure_kind":null,"error_message":null,"counts_business_quota":null,
        "user_id":null,"user_display_name":null,"user_username":null,"reason_code":null,
        "reason_summary":null,"reason_detail":null,"job_id":null,"job_type":null,
        "job_trigger_source":null,"job_status":null,"job_attempt":null,"job_message":null,
        "job_queued_at":null,"job_started_at":null,"job_finished_at":null}}"#
    )
}

/// The canonical page reader must produce the same page from the PostgreSQL snapshot as the
/// SQLite read session: same total, same ordering, same event identity.
#[tokio::test]
async fn canonical_alert_page_matches_across_backends() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-alert-canonical-page", now).await else {
        return;
    };
    let build_generation = now;
    let tag = nanoid!(8);
    for index in 0..3_i64 {
        let source_id = format!("atl:{tag}-{index}");
        let occurred_at = now - 30 - index;
        let payload = canonical_alert_payload(&source_id, occurred_at);
        let binds = [
            Bind::Int(build_generation),
            Bind::Text("auth_token_log"),
            Bind::Text(&source_id),
            Bind::Int(occurred_at),
            Bind::Text(&source_id),
            Bind::Text("partition"),
            Bind::Text(&payload),
        ];
        for backend in [Backend::Sqlite, Backend::Postgres] {
            let store = match backend {
                Backend::Sqlite => &pair.sqlite.key_store,
                Backend::Postgres => &pair.postgres.key_store,
            };
            execute_statement(
                store,
                backend,
                "INSERT INTO observability.admin_alert_canonical_group_events \
                 (build_generation, source_kind, source_id, occurred_at, row_sort_id, \
                  partition_key, payload_json) VALUES (?, ?, ?, ?, ?, ?, ?)",
                &binds,
            )
            .await;
        }
    }

    let sqlite_page = pair
        .sqlite
        .key_store
        .fetch_admin_alert_events_page_for_canonical_snapshot(build_generation, 1, 20)
        .await
        .expect("sqlite canonical page");
    let postgres_page = pair
        .postgres
        .key_store
        .fetch_admin_alert_events_page_for_canonical_snapshot(build_generation, 1, 20)
        .await
        .expect("postgres canonical page");

    assert_eq!(sqlite_page.total, 3);
    assert_eq!(postgres_page.total, sqlite_page.total);
    let sqlite_ids: Vec<&str> = sqlite_page
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let postgres_ids: Vec<&str> = postgres_page
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(postgres_ids, sqlite_ids);
    assert_eq!(postgres_page.items.len(), 3);
}

/// The filtered projection page must build the same page from PostgreSQL: same total, ordering,
/// and event identity, with the JSONB payload rewrite in place of `json_extract`.
#[tokio::test]
async fn filtered_alert_page_matches_across_backends() {
    let now = unique_epoch();
    let Some(pair) = StorePair::open("pg-alert-filtered-page", now).await else {
        return;
    };
    let tag = nanoid!(8);
    for index in 0..3_i64 {
        let source_id = format!("atl:{tag}-{index}");
        let occurred_at = now - 60 - index;
        let payload = canonical_alert_payload(&source_id, occurred_at);
        let binds = [
            Bind::Text("auth_token_log"),
            Bind::Text(&source_id),
            Bind::Int(occurred_at),
            Bind::Text(&source_id),
            Bind::Text(&payload),
            Bind::Int(now),
        ];
        for backend in [Backend::Sqlite, Backend::Postgres] {
            let store = match backend {
                Backend::Sqlite => &pair.sqlite.key_store,
                Backend::Postgres => &pair.postgres.key_store,
            };
            execute_statement(
                store,
                backend,
                "INSERT INTO observability.dashboard_alert_projection_events \
                 (source_kind, source_id, occurred_at, row_sort_id, payload_json, projected_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                &binds,
            )
            .await;
        }
    }

    let request_kinds: Vec<String> = Vec::new();
    let sqlite_page = pair
        .sqlite
        .key_store
        .fetch_admin_alert_events_page_for_operation(
            None,
            Some(now - 600),
            None,
            None,
            None,
            None,
            &request_kinds,
            1,
            20,
            SqliteOperation::AdminAlertsRead,
        )
        .await
        .expect("sqlite filtered page");
    let postgres_page = pair
        .postgres
        .key_store
        .fetch_admin_alert_events_page_for_operation(
            None,
            Some(now - 600),
            None,
            None,
            None,
            None,
            &request_kinds,
            1,
            20,
            SqliteOperation::AdminAlertsRead,
        )
        .await
        .expect("postgres filtered page");

    assert_eq!(sqlite_page.total, 3);
    assert_eq!(postgres_page.total, sqlite_page.total);
    let sqlite_ids: Vec<&str> = sqlite_page
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    let postgres_ids: Vec<&str> = postgres_page
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(postgres_ids, sqlite_ids);
}

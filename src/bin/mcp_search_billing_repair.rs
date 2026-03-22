use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
    path::Path,
    time::Duration,
};

use chrono::{TimeZone, Utc};
use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use serde_json::Value;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tavily_hikari::{
    DEFAULT_UPSTREAM, PendingBillingSettleOutcome, TavilyProxy, rebase_current_month_business_quota,
};

const BILLING_STATE_NONE: &str = "none";
const BILLING_STATE_PENDING: &str = "pending";
const MAX_LOG_SKEW_SECS: i64 = 30;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Repair historical MCP search logs that missed downstream billing"
)]
struct Cli {
    /// SQLite database path to inspect.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Inclusive UTC unix timestamp lower bound.
    #[arg(long)]
    from_ts: i64,

    /// Inclusive UTC unix timestamp upper bound.
    #[arg(long)]
    to_ts: i64,

    /// Optional token_id filter.
    #[arg(long)]
    token_id: Option<String>,

    /// Only report candidate rows without writing changes.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Clone)]
struct SearchAuthLog {
    id: i64,
    token_id: String,
    created_at: i64,
    business_credits: Option<i64>,
    billing_state: String,
}

#[derive(Debug, Clone)]
struct SearchRequestLog {
    id: i64,
    token_id: String,
    created_at: i64,
    credits: i64,
}

#[derive(Debug, Clone)]
struct RepairCandidate {
    auth_log_id: i64,
    request_log_id: i64,
    token_id: String,
    created_at: i64,
    credits: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RepairSkippedToken {
    token_id: String,
    auth_log_count: usize,
    request_log_count: usize,
    reason: String,
}

#[derive(Debug, Serialize)]
struct RepairReport {
    dry_run: bool,
    from_ts: i64,
    to_ts: i64,
    token_id: Option<String>,
    candidate_count: usize,
    affected_token_count: usize,
    total_credits: i64,
    repaired_log_ids: Vec<i64>,
    skipped_tokens: Vec<RepairSkippedToken>,
    monthly_rebase: Option<Value>,
}

fn expected_search_credits_from_request_body(bytes: &[u8]) -> Option<i64> {
    let payload: Value = serde_json::from_slice(bytes).ok()?;
    if payload.get("method").and_then(|value| value.as_str()) != Some("tools/call") {
        return None;
    }

    let params = payload.get("params")?;
    let tool_name = params
        .get("name")
        .and_then(|value| value.as_str())?
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-");
    if tool_name != "tavily-search" {
        return None;
    }

    let search_depth = params
        .get("arguments")
        .and_then(|arguments| arguments.get("search_depth"))
        .and_then(|value| value.as_str())
        .unwrap_or("");

    Some(if search_depth.eq_ignore_ascii_case("advanced") {
        2
    } else {
        1
    })
}

async fn connect_sqlite_pool(db_path: &str) -> Result<sqlx::SqlitePool, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await
}

async fn load_search_auth_logs(
    pool: &sqlx::SqlitePool,
    from_ts: i64,
    to_ts: i64,
    token_id: Option<&str>,
) -> Result<Vec<SearchAuthLog>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, token_id, created_at, business_credits, COALESCE(billing_state, 'none') AS billing_state
         FROM auth_token_logs
         WHERE request_kind_key = 'mcp:search'
           AND result_status = 'success'
           AND http_status = 200
           AND created_at >= ?
           AND created_at <= ?
           AND (? IS NULL OR token_id = ?)
         ORDER BY token_id ASC, created_at ASC, id ASC",
    )
    .bind(from_ts)
    .bind(to_ts)
    .bind(token_id)
    .bind(token_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(SearchAuthLog {
                id: row.try_get("id")?,
                token_id: row.try_get("token_id")?,
                created_at: row.try_get("created_at")?,
                business_credits: row.try_get("business_credits")?,
                billing_state: row.try_get("billing_state")?,
            })
        })
        .collect()
}

async fn load_search_request_logs(
    pool: &sqlx::SqlitePool,
    from_ts: i64,
    to_ts: i64,
    token_id: Option<&str>,
) -> Result<Vec<SearchRequestLog>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, auth_token_id, created_at, request_body
         FROM request_logs
         WHERE result_status = 'success'
           AND status_code = 200
           AND path LIKE '/mcp%'
           AND auth_token_id IS NOT NULL
           AND created_at >= ?
           AND created_at <= ?
           AND (? IS NULL OR auth_token_id = ?)
         ORDER BY auth_token_id ASC, created_at ASC, id ASC",
    )
    .bind(from_ts)
    .bind(to_ts)
    .bind(token_id)
    .bind(token_id)
    .fetch_all(pool)
    .await?;

    let mut request_logs = Vec::new();
    for row in rows {
        let request_body = row.try_get::<Vec<u8>, _>("request_body")?;
        let Some(credits) = expected_search_credits_from_request_body(&request_body) else {
            continue;
        };
        request_logs.push(SearchRequestLog {
            id: row.try_get("id")?,
            token_id: row.try_get("auth_token_id")?,
            created_at: row.try_get("created_at")?,
            credits,
        });
    }

    Ok(request_logs)
}

fn build_candidates(
    auth_logs: Vec<SearchAuthLog>,
    request_logs: Vec<SearchRequestLog>,
) -> (Vec<RepairCandidate>, Vec<RepairSkippedToken>) {
    let mut auth_by_token: BTreeMap<String, Vec<SearchAuthLog>> = BTreeMap::new();
    for log in auth_logs {
        auth_by_token
            .entry(log.token_id.clone())
            .or_default()
            .push(log);
    }

    let mut request_by_token: BTreeMap<String, Vec<SearchRequestLog>> = BTreeMap::new();
    for log in request_logs {
        request_by_token
            .entry(log.token_id.clone())
            .or_default()
            .push(log);
    }

    let token_ids = auth_by_token
        .keys()
        .chain(request_by_token.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut candidates = Vec::new();
    let mut skipped_tokens = Vec::new();

    for token_id in token_ids {
        let mut auth_entries = auth_by_token.remove(&token_id).unwrap_or_default();
        let mut request_entries = request_by_token.remove(&token_id).unwrap_or_default();
        auth_entries.sort_by_key(|entry| (entry.created_at, entry.id));
        request_entries.sort_by_key(|entry| (entry.created_at, entry.id));

        if auth_entries.len() != request_entries.len() {
            skipped_tokens.push(RepairSkippedToken {
                token_id,
                auth_log_count: auth_entries.len(),
                request_log_count: request_entries.len(),
                reason: "search auth/request log counts differ".to_string(),
            });
            continue;
        }

        let mut token_candidates = Vec::new();
        let mut token_skip_reason = None;

        for (auth_log, request_log) in auth_entries.iter().zip(request_entries.iter()) {
            let skew = (auth_log.created_at - request_log.created_at).abs();
            if skew > MAX_LOG_SKEW_SECS {
                token_skip_reason = Some(format!(
                    "paired log skew too large for auth_log_id={} request_log_id={} ({skew}s)",
                    auth_log.id, request_log.id
                ));
                break;
            }

            if auth_log.business_credits.is_none() && auth_log.billing_state == BILLING_STATE_NONE {
                token_candidates.push(RepairCandidate {
                    auth_log_id: auth_log.id,
                    request_log_id: request_log.id,
                    token_id: token_id.clone(),
                    created_at: auth_log.created_at,
                    credits: request_log.credits,
                });
            }
        }

        if let Some(reason) = token_skip_reason {
            skipped_tokens.push(RepairSkippedToken {
                token_id,
                auth_log_count: auth_entries.len(),
                request_log_count: request_entries.len(),
                reason,
            });
            continue;
        }

        candidates.extend(token_candidates);
    }

    (candidates, skipped_tokens)
}

async fn load_candidates(
    pool: &sqlx::SqlitePool,
    from_ts: i64,
    to_ts: i64,
    token_id: Option<&str>,
) -> Result<(Vec<RepairCandidate>, Vec<RepairSkippedToken>), sqlx::Error> {
    let auth_logs = load_search_auth_logs(pool, from_ts, to_ts, token_id).await?;
    let request_logs = load_search_request_logs(pool, from_ts, to_ts, token_id).await?;
    Ok(build_candidates(auth_logs, request_logs))
}

async fn settle_repaired_log(
    proxy: &TavilyProxy,
    auth_log_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    for attempt in 0..2 {
        match proxy.settle_pending_billing_attempt(auth_log_id).await? {
            PendingBillingSettleOutcome::Charged | PendingBillingSettleOutcome::AlreadySettled => {
                return Ok(());
            }
            PendingBillingSettleOutcome::RetryLater if attempt == 0 => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            PendingBillingSettleOutcome::RetryLater => {
                let message = format!(
                    "pending billing claim miss for auth_token_logs.id={auth_log_id} during repair",
                );
                let _ = proxy
                    .annotate_pending_billing_attempt(auth_log_id, &message)
                    .await;
                return Err(io::Error::other(message).into());
            }
        }
    }

    unreachable!("pending billing settle loop should always return");
}

async fn repair_candidates(
    proxy: &TavilyProxy,
    pool: &sqlx::SqlitePool,
    candidates: &[RepairCandidate],
) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let mut repaired_log_ids = Vec::new();

    for candidate in candidates {
        let billing_guard = proxy.lock_token_billing(&candidate.token_id).await?;
        billing_guard.ensure_live()?;

        let updated = sqlx::query(
            "UPDATE auth_token_logs
             SET business_credits = ?,
                 billing_state = ?,
                 billing_subject = ?
             WHERE id = ?
               AND business_credits IS NULL
               AND COALESCE(billing_state, 'none') = 'none'",
        )
        .bind(candidate.credits)
        .bind(BILLING_STATE_PENDING)
        .bind(billing_guard.billing_subject())
        .bind(candidate.auth_log_id)
        .execute(pool)
        .await?;

        if updated.rows_affected() != 1 {
            continue;
        }

        if let Err(err) = billing_guard.ensure_live() {
            let message = format!(
                "quota subject lock lost before settling repaired auth_token_logs.id={}: {}",
                candidate.auth_log_id, err
            );
            let _ = proxy
                .annotate_pending_billing_attempt(candidate.auth_log_id, &message)
                .await;
            return Err(io::Error::other(message).into());
        }

        settle_repaired_log(proxy, candidate.auth_log_id).await?;
        repaired_log_ids.push(candidate.auth_log_id);
    }

    Ok(repaired_log_ids)
}

fn build_report(
    cli: &Cli,
    candidates: &[RepairCandidate],
    repaired_log_ids: Vec<i64>,
    skipped_tokens: Vec<RepairSkippedToken>,
    monthly_rebase: Option<Value>,
) -> RepairReport {
    let affected_tokens = candidates
        .iter()
        .map(|candidate| candidate.token_id.clone())
        .collect::<BTreeSet<_>>();
    let _request_log_ids = candidates
        .iter()
        .map(|candidate| candidate.request_log_id)
        .collect::<BTreeSet<_>>();
    let _latest_candidate_at = candidates
        .iter()
        .map(|candidate| candidate.created_at)
        .max();

    RepairReport {
        dry_run: cli.dry_run,
        from_ts: cli.from_ts,
        to_ts: cli.to_ts,
        token_id: cli.token_id.clone(),
        candidate_count: candidates.len(),
        affected_token_count: affected_tokens.len(),
        total_credits: candidates.iter().map(|candidate| candidate.credits).sum(),
        repaired_log_ids,
        skipped_tokens,
        monthly_rebase,
    }
}

fn write_report(mut writer: impl Write, report: &RepairReport) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut writer, report)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let cli = Cli::parse();
    if cli.from_ts > cli.to_ts {
        return Err("--from-ts must be less than or equal to --to-ts".into());
    }

    let db_path = Path::new(&cli.db_path);
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let pool = connect_sqlite_pool(&cli.db_path).await?;
    let (candidates, skipped_tokens) =
        load_candidates(&pool, cli.from_ts, cli.to_ts, cli.token_id.as_deref()).await?;

    let (repaired_log_ids, monthly_rebase) = if cli.dry_run {
        (Vec::new(), None)
    } else {
        let proxy =
            TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &cli.db_path)
                .await?;
        let repaired_log_ids = repair_candidates(&proxy, &pool, &candidates).await?;
        let monthly_rebase = if repaired_log_ids.is_empty() {
            None
        } else {
            let rebase_at = Utc
                .timestamp_opt(cli.to_ts, 0)
                .single()
                .unwrap_or_else(Utc::now);
            Some(serde_json::to_value(
                rebase_current_month_business_quota(&cli.db_path, rebase_at).await?,
            )?)
        };
        (repaired_log_ids, monthly_rebase)
    };

    let report = build_report(
        &cli,
        &candidates,
        repaired_log_ids,
        skipped_tokens,
        monthly_rebase,
    );
    write_report(io::stdout().lock(), &report)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        RepairCandidate, RepairSkippedToken, build_candidates, connect_sqlite_pool,
        expected_search_credits_from_request_body, load_candidates, repair_candidates,
    };
    use chrono::{Datelike, TimeZone, Utc};
    use nanoid::nanoid;
    use serde_json::{Value, json};
    use sqlx::Row;
    use tavily_hikari::{DEFAULT_UPSTREAM, TavilyProxy};

    fn temp_db_path(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{prefix}-{}.db", nanoid!(8)))
    }

    async fn init_proxy_and_pool(prefix: &str) -> (TavilyProxy, sqlx::SqlitePool, String) {
        let db_path = temp_db_path(prefix);
        let db_str = db_path.to_string_lossy().to_string();
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let pool = connect_sqlite_pool(&db_str).await.expect("sqlite pool");
        (proxy, pool, db_str)
    }

    async fn insert_token(pool: &sqlx::SqlitePool, token_id: &str, created_at: i64) {
        sqlx::query(
            "INSERT INTO auth_tokens (id, secret, enabled, total_requests, created_at, deleted_at)
             VALUES (?, ?, 1, 0, ?, NULL)",
        )
        .bind(token_id)
        .bind(format!("th-{token_id}-secret"))
        .bind(created_at)
        .execute(pool)
        .await
        .expect("insert token");
    }

    async fn insert_api_key(pool: &sqlx::SqlitePool, api_key_id: &str) {
        sqlx::query(
            "INSERT INTO api_keys (id, api_key, status, created_at, last_used_at)
             VALUES (?, ?, 'active', 0, 0)",
        )
        .bind(api_key_id)
        .bind(format!("tvly-{api_key_id}-secret"))
        .execute(pool)
        .await
        .expect("insert api key");
    }

    async fn insert_auth_search_log(
        pool: &sqlx::SqlitePool,
        token_id: &str,
        created_at: i64,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                http_status,
                request_kind_key,
                request_kind_label,
                result_status,
                counts_business_quota,
                created_at
             ) VALUES (?, 'POST', '/mcp', 200, 'mcp:search', 'MCP | search', 'success', 1, ?)
             RETURNING id",
        )
        .bind(token_id)
        .bind(created_at)
        .fetch_one(pool)
        .await
        .expect("insert auth search log")
    }

    async fn insert_request_search_log(
        pool: &sqlx::SqlitePool,
        api_key_id: &str,
        token_id: &str,
        created_at: i64,
        body: Value,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                status_code,
                result_status,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
             ) VALUES (?, ?, 'POST', '/mcp', 200, 'success', ?, X'', '[]', '[]', ?)
             RETURNING id",
        )
        .bind(api_key_id)
        .bind(token_id)
        .bind(serde_json::to_vec(&body).expect("serialize request body"))
        .bind(created_at)
        .fetch_one(pool)
        .await
        .expect("insert request search log")
    }

    #[test]
    fn parses_underscore_search_body() {
        let body = serde_json::to_vec(&json!({
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "smoke",
                    "search_depth": "advanced"
                }
            }
        }))
        .expect("serialize body");

        assert_eq!(expected_search_credits_from_request_body(&body), Some(2));
    }

    #[test]
    fn parses_hyphenated_search_body() {
        let body = serde_json::to_vec(&json!({
            "method": "tools/call",
            "params": {
                "name": "tavily-search",
                "arguments": {
                    "query": "smoke"
                }
            }
        }))
        .expect("serialize body");

        assert_eq!(expected_search_credits_from_request_body(&body), Some(1));
    }

    #[test]
    fn ignores_non_search_tool_calls() {
        let body = serde_json::to_vec(&json!({
            "method": "tools/call",
            "params": {
                "name": "tavily_extract",
                "arguments": {
                    "urls": ["https://example.com"]
                }
            }
        }))
        .expect("serialize body");

        assert_eq!(expected_search_credits_from_request_body(&body), None);
    }

    #[test]
    fn build_candidates_skips_tokens_with_mismatched_log_counts() {
        let (candidates, skipped) = build_candidates(
            vec![
                super::SearchAuthLog {
                    id: 1,
                    token_id: "tok-a".to_string(),
                    created_at: 100,
                    business_credits: None,
                    billing_state: "none".to_string(),
                },
                super::SearchAuthLog {
                    id: 2,
                    token_id: "tok-a".to_string(),
                    created_at: 110,
                    business_credits: None,
                    billing_state: "none".to_string(),
                },
            ],
            vec![super::SearchRequestLog {
                id: 8,
                token_id: "tok-a".to_string(),
                created_at: 100,
                credits: 2,
            }],
        );

        assert!(candidates.is_empty());
        assert_eq!(
            skipped,
            vec![RepairSkippedToken {
                token_id: "tok-a".to_string(),
                auth_log_count: 2,
                request_log_count: 1,
                reason: "search auth/request log counts differ".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn load_candidates_pairs_auth_and_request_logs_by_token_order() {
        let (_proxy, pool, db_str) = init_proxy_and_pool("mcp-search-repair-load").await;
        let created_at = Utc::now().timestamp();
        insert_api_key(&pool, "key-1").await;
        insert_token(&pool, "tok-load", created_at).await;

        insert_auth_search_log(&pool, "tok-load", created_at).await;
        let request_log_id = insert_request_search_log(
            &pool,
            "key-1",
            "tok-load",
            created_at,
            json!({
                "method": "tools/call",
                "params": {
                    "name": "tavily_search",
                    "arguments": {
                        "query": "repair load",
                        "search_depth": "advanced"
                    }
                }
            }),
        )
        .await;

        let (candidates, skipped) =
            load_candidates(&pool, created_at - 1, created_at + 1, Some("tok-load"))
                .await
                .expect("load repair candidates");

        assert!(skipped.is_empty());
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| (
                    candidate.token_id.clone(),
                    candidate.request_log_id,
                    candidate.credits
                ))
                .collect::<Vec<_>>(),
            vec![("tok-load".to_string(), request_log_id, 2)]
        );

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn repair_candidates_settles_existing_auth_logs_and_is_idempotent() {
        let (proxy, pool, db_str) = init_proxy_and_pool("mcp-search-repair-settle").await;
        let created_at = Utc::now().timestamp();
        insert_token(&pool, "tok-repair", created_at).await;

        let auth_log_id = insert_auth_search_log(&pool, "tok-repair", created_at).await;
        let candidates = vec![RepairCandidate {
            auth_log_id,
            request_log_id: 77,
            token_id: "tok-repair".to_string(),
            created_at,
            credits: 2,
        }];

        let repaired = repair_candidates(&proxy, &pool, &candidates)
            .await
            .expect("repair candidates");
        assert_eq!(repaired, vec![auth_log_id]);

        let charged_row = sqlx::query(
            "SELECT business_credits, billing_state, billing_subject
             FROM auth_token_logs
             WHERE id = ?",
        )
        .bind(auth_log_id)
        .fetch_one(&pool)
        .await
        .expect("fetch charged auth log");
        assert_eq!(
            charged_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("business credits"),
            Some(2)
        );
        assert_eq!(
            charged_row
                .try_get::<String, _>("billing_state")
                .expect("billing state"),
            "charged"
        );
        assert_eq!(
            charged_row
                .try_get::<Option<String>, _>("billing_subject")
                .expect("billing subject")
                .as_deref(),
            Some("token:tok-repair")
        );

        let now = Utc::now();
        let month_start = Utc
            .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .single()
            .expect("month start")
            .timestamp();

        let token_month_count: i64 = sqlx::query_scalar(
            "SELECT COALESCE(month_count, 0) FROM auth_token_quota WHERE token_id = ? AND month_start = ?",
        )
        .bind("tok-repair")
        .bind(month_start)
        .fetch_one(&pool)
        .await
        .expect("read token month count");
        assert_eq!(token_month_count, 2);

        let token_minute_sum: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
        )
        .bind("tok-repair")
        .bind("minute")
        .fetch_one(&pool)
        .await
        .expect("read token minute buckets");
        assert_eq!(token_minute_sum, 2);

        let repaired_again = repair_candidates(&proxy, &pool, &candidates)
            .await
            .expect("repair candidates rerun");
        assert!(repaired_again.is_empty());
        let _ = std::fs::remove_file(db_str);
    }
}

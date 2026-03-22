use std::{
    collections::{BTreeSet, HashMap, HashSet},
    io::{self, Write},
    path::Path,
    time::Duration,
};

use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use serde_json::Value;
use sqlx::{
    Row,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tavily_hikari::{
    DEFAULT_UPSTREAM, REQUEST_LOG_VISIBILITY_SUPPRESSED_RETRY_SHADOW,
    REQUEST_LOG_VISIBILITY_VISIBLE, TavilyProxy,
};

const MAX_RETRY_GAP_SECS: i64 = 10;
const FAILURE_KIND_TOOL_ARGUMENT_VALIDATION: &str = "tool_argument_validation";

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Suppress historical MCP retry-shadow request logs caused by include_usage rejection"
)]
struct Cli {
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    #[arg(long)]
    from_ts: i64,

    #[arg(long)]
    to_ts: i64,

    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Debug, Clone)]
struct RequestLogRow {
    id: i64,
    api_key_id: String,
    auth_token_id: String,
    method: String,
    path: String,
    query: Option<String>,
    created_at: i64,
    request_body: Vec<u8>,
    response_body: Vec<u8>,
}

#[derive(Debug, Clone)]
struct RetryRepairCandidate {
    shadow_log_id: i64,
    final_log_id: i64,
    token_id: String,
    api_key_id: String,
}

#[derive(Debug, Serialize)]
struct RetryRepairReport {
    dry_run: bool,
    from_ts: i64,
    to_ts: i64,
    candidate_count: usize,
    affected_token_count: usize,
    affected_key_count: usize,
    suppressed_log_ids: Vec<i64>,
    final_log_ids: Vec<i64>,
    rebuilt_api_key_usage_buckets: bool,
}

fn response_mentions_include_usage_rejection(body: &[u8]) -> bool {
    let normalized = String::from_utf8_lossy(body).to_ascii_lowercase();
    normalized.contains("include_usage") && normalized.contains("unexpected keyword argument")
}

fn normalize_request_body_without_include_usage(bytes: &[u8]) -> Option<(Value, bool)> {
    fn scrub(value: &mut Value, removed: &mut bool) {
        match value {
            Value::Object(map) => {
                let method = map
                    .get("method")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                if method == "tools/call"
                    && let Some(Value::Object(params)) = map.get_mut("params")
                    && let Some(Value::Object(arguments)) = params.get_mut("arguments")
                    && arguments.remove("include_usage").is_some()
                {
                    *removed = true;
                }

                for nested in map.values_mut() {
                    scrub(nested, removed);
                }
            }
            Value::Array(items) => {
                for nested in items.iter_mut() {
                    scrub(nested, removed);
                }
            }
            _ => {}
        }
    }

    let mut value = serde_json::from_slice::<Value>(bytes).ok()?;
    let mut removed = false;
    scrub(&mut value, &mut removed);
    Some((value, removed))
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

async fn load_shadow_logs(
    pool: &sqlx::SqlitePool,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<RequestLogRow>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            created_at,
            request_body,
            response_body
        FROM request_logs
        WHERE visibility = ?
          AND result_status = 'error'
          AND failure_kind = ?
          AND auth_token_id IS NOT NULL
          AND path LIKE '/mcp%'
          AND created_at >= ?
          AND created_at <= ?
        ORDER BY auth_token_id ASC, created_at ASC, id ASC
        "#,
    )
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(FAILURE_KIND_TOOL_ARGUMENT_VALIDATION)
    .bind(from_ts)
    .bind(to_ts)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(RequestLogRow {
                id: row.try_get("id")?,
                api_key_id: row.try_get("api_key_id")?,
                auth_token_id: row.try_get("auth_token_id")?,
                method: row.try_get("method")?,
                path: row.try_get("path")?,
                query: row.try_get("query")?,
                created_at: row.try_get("created_at")?,
                request_body: row
                    .try_get::<Option<Vec<u8>>, _>("request_body")?
                    .unwrap_or_default(),
                response_body: row
                    .try_get::<Option<Vec<u8>>, _>("response_body")?
                    .unwrap_or_default(),
            })
        })
        .collect()
}

async fn load_potential_final_logs(
    pool: &sqlx::SqlitePool,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<RequestLogRow>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            created_at,
            request_body,
            response_body
        FROM request_logs
        WHERE visibility = ?
          AND auth_token_id IS NOT NULL
          AND path LIKE '/mcp%'
          AND created_at >= ?
          AND created_at <= ?
        ORDER BY auth_token_id ASC, created_at ASC, id ASC
        "#,
    )
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .bind(from_ts)
    .bind(to_ts.saturating_add(MAX_RETRY_GAP_SECS))
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(RequestLogRow {
                id: row.try_get("id")?,
                api_key_id: row.try_get("api_key_id")?,
                auth_token_id: row.try_get("auth_token_id")?,
                method: row.try_get("method")?,
                path: row.try_get("path")?,
                query: row.try_get("query")?,
                created_at: row.try_get("created_at")?,
                request_body: row
                    .try_get::<Option<Vec<u8>>, _>("request_body")?
                    .unwrap_or_default(),
                response_body: row
                    .try_get::<Option<Vec<u8>>, _>("response_body")?
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn build_candidates(
    shadows: Vec<RequestLogRow>,
    finals: Vec<RequestLogRow>,
) -> Vec<RetryRepairCandidate> {
    let mut finals_by_key: HashMap<(String, String, String, Option<String>), Vec<RequestLogRow>> =
        HashMap::new();
    for row in finals {
        finals_by_key
            .entry((
                row.auth_token_id.clone(),
                row.method.clone(),
                row.path.clone(),
                row.query.clone(),
            ))
            .or_default()
            .push(row);
    }

    for rows in finals_by_key.values_mut() {
        rows.sort_by_key(|row| (row.created_at, row.id));
    }

    let mut consumed_final_ids = HashSet::new();
    let mut candidates = Vec::new();

    for shadow in shadows {
        if !response_mentions_include_usage_rejection(&shadow.response_body) {
            continue;
        }

        let Some((normalized_shadow_body, removed_include_usage)) =
            normalize_request_body_without_include_usage(&shadow.request_body)
        else {
            continue;
        };
        if !removed_include_usage {
            continue;
        }

        let key = (
            shadow.auth_token_id.clone(),
            shadow.method.clone(),
            shadow.path.clone(),
            shadow.query.clone(),
        );
        let Some(final_rows) = finals_by_key.get(&key) else {
            continue;
        };

        let Some(final_row) = final_rows.iter().find(|candidate| {
            if candidate.id == shadow.id || consumed_final_ids.contains(&candidate.id) {
                return false;
            }
            if candidate.created_at < shadow.created_at {
                return false;
            }
            if candidate.created_at.saturating_sub(shadow.created_at) > MAX_RETRY_GAP_SECS {
                return false;
            }
            if candidate.id < shadow.id && candidate.created_at == shadow.created_at {
                return false;
            }

            let Some((normalized_candidate_body, _)) =
                normalize_request_body_without_include_usage(&candidate.request_body)
            else {
                return false;
            };
            normalized_candidate_body == normalized_shadow_body
        }) else {
            continue;
        };

        consumed_final_ids.insert(final_row.id);
        candidates.push(RetryRepairCandidate {
            shadow_log_id: shadow.id,
            final_log_id: final_row.id,
            token_id: shadow.auth_token_id,
            api_key_id: shadow.api_key_id,
        });
    }

    candidates
}

async fn suppress_retry_shadows(
    pool: &sqlx::SqlitePool,
    candidates: &[RetryRepairCandidate],
) -> Result<Vec<i64>, sqlx::Error> {
    let mut suppressed = Vec::new();
    for candidate in candidates {
        let updated = sqlx::query(
            "UPDATE request_logs
             SET visibility = ?
             WHERE id = ?
               AND visibility = ?",
        )
        .bind(REQUEST_LOG_VISIBILITY_SUPPRESSED_RETRY_SHADOW)
        .bind(candidate.shadow_log_id)
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .execute(pool)
        .await?;

        if updated.rows_affected() == 1 {
            suppressed.push(candidate.shadow_log_id);
        }
    }

    Ok(suppressed)
}

fn build_report(
    cli: &Cli,
    candidates: &[RetryRepairCandidate],
    suppressed_log_ids: Vec<i64>,
    rebuilt_api_key_usage_buckets: bool,
) -> RetryRepairReport {
    let affected_tokens = candidates
        .iter()
        .map(|candidate| candidate.token_id.clone())
        .collect::<BTreeSet<_>>();
    let affected_keys = candidates
        .iter()
        .map(|candidate| candidate.api_key_id.clone())
        .collect::<BTreeSet<_>>();
    let mut final_log_ids = candidates
        .iter()
        .map(|candidate| candidate.final_log_id)
        .collect::<Vec<_>>();
    final_log_ids.sort_unstable();
    final_log_ids.dedup();

    RetryRepairReport {
        dry_run: cli.dry_run,
        from_ts: cli.from_ts,
        to_ts: cli.to_ts,
        candidate_count: candidates.len(),
        affected_token_count: affected_tokens.len(),
        affected_key_count: affected_keys.len(),
        suppressed_log_ids,
        final_log_ids,
        rebuilt_api_key_usage_buckets,
    }
}

fn write_report(mut writer: impl Write, report: &RetryRepairReport) -> io::Result<()> {
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
    let shadows = load_shadow_logs(&pool, cli.from_ts, cli.to_ts).await?;
    let finals = load_potential_final_logs(&pool, cli.from_ts, cli.to_ts).await?;
    let candidates = build_candidates(shadows, finals);

    let (suppressed_log_ids, rebuilt_api_key_usage_buckets) = if cli.dry_run {
        (Vec::new(), false)
    } else {
        let suppressed_log_ids = suppress_retry_shadows(&pool, &candidates).await?;
        let rebuilt = if suppressed_log_ids.is_empty() {
            false
        } else {
            let proxy =
                TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &cli.db_path)
                    .await?;
            proxy.rebuild_api_key_usage_buckets().await?;
            true
        };
        (suppressed_log_ids, rebuilt)
    };

    let report = build_report(
        &cli,
        &candidates,
        suppressed_log_ids,
        rebuilt_api_key_usage_buckets,
    );
    write_report(io::stdout().lock(), &report)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        RetryRepairCandidate, build_candidates, connect_sqlite_pool,
        normalize_request_body_without_include_usage, suppress_retry_shadows,
    };
    use chrono::Utc;
    use nanoid::nanoid;
    use serde_json::{Value, json};
    use sqlx::Row;
    use tavily_hikari::{
        DEFAULT_UPSTREAM, REQUEST_LOG_VISIBILITY_SUPPRESSED_RETRY_SHADOW, TavilyProxy,
    };

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

    struct RequestLogSeed<'a> {
        id: i64,
        api_key_id: &'a str,
        token_id: &'a str,
        created_at: i64,
        result_status: &'a str,
        failure_kind: Option<&'a str>,
        request_body: Value,
        response_body: Value,
        visibility: &'a str,
    }

    async fn insert_request_log(pool: &sqlx::SqlitePool, seed: RequestLogSeed<'_>) {
        sqlx::query(
            "INSERT INTO request_logs (
                id,
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                failure_kind,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                visibility,
                created_at
             ) VALUES (?, ?, ?, 'POST', '/mcp', NULL, 200, NULL, NULL, ?, ?, ?, ?, '[]', '[]', ?, ?)",
        )
        .bind(seed.id)
        .bind(seed.api_key_id)
        .bind(seed.token_id)
        .bind(seed.result_status)
        .bind(seed.failure_kind)
        .bind(serde_json::to_vec(&seed.request_body).expect("serialize request body"))
        .bind(serde_json::to_vec(&seed.response_body).expect("serialize response body"))
        .bind(seed.visibility)
        .bind(seed.created_at)
        .execute(pool)
        .await
        .expect("insert request log");
    }

    #[test]
    fn normalizer_strips_include_usage_from_tools_call() {
        let payload = serde_json::to_vec(&json!({
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "smoke",
                    "include_usage": true
                }
            }
        }))
        .expect("serialize payload");

        let (normalized, removed) =
            normalize_request_body_without_include_usage(&payload).expect("normalized");
        assert!(removed);
        assert_eq!(normalized["params"]["arguments"].get("include_usage"), None);
    }

    #[tokio::test]
    async fn build_candidates_matches_retry_shadow_to_final_attempt() {
        let created_at = Utc::now().timestamp();
        let shadow_request = json!({
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "retry shadow",
                    "search_depth": "advanced",
                    "include_usage": true
                }
            }
        });
        let final_request = json!({
            "method": "tools/call",
            "params": {
                "name": "tavily_search",
                "arguments": {
                    "query": "retry shadow",
                    "search_depth": "advanced"
                }
            }
        });

        let candidates = build_candidates(
            vec![super::RequestLogRow {
                id: 10,
                api_key_id: "key-1".to_string(),
                auth_token_id: "tok-1".to_string(),
                method: "POST".to_string(),
                path: "/mcp".to_string(),
                query: None,
                created_at,
                request_body: serde_json::to_vec(&shadow_request).expect("shadow request body"),
                response_body: serde_json::to_vec(&json!({
                    "error": {
                        "message": "Unexpected keyword argument: include_usage"
                    }
                }))
                .expect("shadow response body"),
            }],
            vec![super::RequestLogRow {
                id: 11,
                api_key_id: "key-1".to_string(),
                auth_token_id: "tok-1".to_string(),
                method: "POST".to_string(),
                path: "/mcp".to_string(),
                query: None,
                created_at: created_at + 1,
                request_body: serde_json::to_vec(&final_request).expect("final request body"),
                response_body: serde_json::to_vec(&json!({
                    "result": {
                        "structuredContent": {
                            "status": 200
                        }
                    }
                }))
                .expect("final response body"),
            }],
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].shadow_log_id, 10);
        assert_eq!(candidates[0].final_log_id, 11);
    }

    #[tokio::test]
    async fn suppress_retry_shadows_marks_rows_and_rebuilds_buckets() {
        let (proxy, pool, db_str) = init_proxy_and_pool("mcp-request-log-repair").await;
        let created_at = Utc::now().timestamp();
        insert_api_key(&pool, "key-1").await;

        insert_request_log(
            &pool,
            RequestLogSeed {
                id: 10,
                api_key_id: "key-1",
                token_id: "tok-1",
                created_at,
                result_status: "error",
                failure_kind: Some("tool_argument_validation"),
                request_body: json!({
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_search",
                        "arguments": {
                            "query": "retry shadow",
                            "include_usage": true
                        }
                    }
                }),
                response_body: json!({
                    "error": {
                        "message": "Unexpected keyword argument: include_usage"
                    }
                }),
                visibility: "visible",
            },
        )
        .await;
        insert_request_log(
            &pool,
            RequestLogSeed {
                id: 11,
                api_key_id: "key-1",
                token_id: "tok-1",
                created_at: created_at + 1,
                result_status: "success",
                failure_kind: None,
                request_body: json!({
                    "method": "tools/call",
                    "params": {
                        "name": "tavily_search",
                        "arguments": {
                            "query": "retry shadow"
                        }
                    }
                }),
                response_body: json!({
                    "result": {
                        "structuredContent": {
                            "status": 200
                        }
                    }
                }),
                visibility: "visible",
            },
        )
        .await;

        let suppressed = suppress_retry_shadows(
            &pool,
            &[RetryRepairCandidate {
                shadow_log_id: 10,
                final_log_id: 11,
                token_id: "tok-1".to_string(),
                api_key_id: "key-1".to_string(),
            }],
        )
        .await
        .expect("suppress retry shadows");
        assert_eq!(suppressed, vec![10]);

        proxy
            .rebuild_api_key_usage_buckets()
            .await
            .expect("rebuild api key usage buckets");

        let visibility: String =
            sqlx::query_scalar("SELECT visibility FROM request_logs WHERE id = 10")
                .fetch_one(&pool)
                .await
                .expect("read updated visibility");
        assert_eq!(visibility, REQUEST_LOG_VISIBILITY_SUPPRESSED_RETRY_SHADOW);

        let row = sqlx::query(
            "SELECT total_requests, success_count, error_count
             FROM api_key_usage_buckets
             WHERE api_key_id = ? AND bucket_secs = 86400",
        )
        .bind("key-1")
        .fetch_one(&pool)
        .await
        .expect("fetch rebuilt bucket");
        assert_eq!(row.try_get::<i64, _>("total_requests").expect("total"), 1);
        assert_eq!(row.try_get::<i64, _>("success_count").expect("success"), 1);
        assert_eq!(row.try_get::<i64, _>("error_count").expect("error"), 0);

        let _ = std::fs::remove_file(db_str);
    }
}

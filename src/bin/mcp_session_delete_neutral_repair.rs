use std::{
    collections::BTreeSet,
    io::{self, Write},
    path::Path,
    time::Duration,
};

use chrono::{Datelike, TimeZone, Utc};
use clap::Parser;
use dotenvy::dotenv;
use serde::Serialize;
use serde_json::Value;
use sqlx::{
    Connection, Row, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use tavily_hikari::{MonthlyQuotaRebaseReport, ProxyError};

const FAILURE_KIND_MCP_METHOD_405: &str = "mcp_method_405";
const REQUEST_KIND_KEY: &str = "mcp:session-delete-unsupported";
const REQUEST_KIND_LABEL: &str = "MCP | session delete unsupported";
const BILLING_STATE_NONE: &str = "none";
const SESSION_DELETE_MESSAGE: &str = "Session termination not supported";
const META_KEY_TOKEN_USAGE_ROLLUP_TS: &str = "token_usage_rollup_last_ts";
const META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2: &str = "token_usage_rollup_last_log_id_v2";
const META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1: &str = "business_quota_monthly_rebase_v1";
const TOKEN_USAGE_STATS_BUCKET_SECS: i64 = 3600;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Neutralize historical DELETE /mcp 405 session-teardown logs and rebuild derived usage/quota data"
)]
struct Cli {
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    #[arg(long, default_value_t = false, conflicts_with = "apply")]
    dry_run: bool,

    #[arg(long, default_value_t = false)]
    apply: bool,
}

#[derive(Debug, Clone)]
struct RequestLogCandidate {
    id: i64,
    created_at: i64,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    business_credits: Option<i64>,
}

#[derive(Debug, Clone)]
struct AuthTokenLogCandidate {
    id: i64,
    token_id: String,
    created_at: i64,
    request_kind_key: Option<String>,
    request_kind_label: Option<String>,
    request_kind_detail: Option<String>,
    counts_business_quota: bool,
    business_credits: Option<i64>,
    billing_state: String,
    billing_subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RepairMonthSummary {
    month_start: i64,
    month_iso: String,
}

#[derive(Debug, Serialize)]
struct RepairReport {
    dry_run: bool,
    apply: bool,
    matched_request_rows: usize,
    matched_auth_token_rows: usize,
    request_rows_needing_update: usize,
    auth_token_rows_needing_update: usize,
    affected_token_count: usize,
    affected_tokens: Vec<String>,
    touched_months: Vec<RepairMonthSummary>,
    request_log_ids: Vec<i64>,
    auth_token_log_ids: Vec<i64>,
    request_rows_updated: usize,
    auth_token_rows_updated: usize,
    token_usage_stats_rows_rebuilt: i64,
    monthly_rebase: Option<Value>,
}

#[derive(Debug)]
struct RepairExecutionSummary {
    request_rows_updated: usize,
    auth_token_rows_updated: usize,
    token_usage_stats_rows_rebuilt: i64,
    monthly_rebase: Option<Value>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MonthlyRebaseEntry {
    month_start: i64,
    month_iso: String,
    report: MonthlyQuotaRebaseReport,
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

async fn connect_immediate_sqlite_connection(
    db_path: &str,
) -> Result<SqliteConnection, sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut connection)
        .await?;
    Ok(connection)
}

async fn read_meta_i64(
    connection: &mut SqliteConnection,
    key: &str,
) -> Result<Option<i64>, ProxyError> {
    let value = sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
        .bind(key)
        .fetch_optional(&mut *connection)
        .await
        .map_err(ProxyError::Database)?;
    Ok(value.and_then(|raw| raw.parse::<i64>().ok()))
}

async fn write_meta_i64(
    connection: &mut SqliteConnection,
    key: &str,
    value: i64,
) -> Result<(), ProxyError> {
    sqlx::query(
        r#"
        INSERT INTO meta (key, value)
        VALUES (?, ?)
        ON CONFLICT(key) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(key)
    .bind(value.to_string())
    .execute(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;
    Ok(())
}

fn repair_month_start(ts: i64) -> i64 {
    let dt = Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now);
    Utc.with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
        .single()
        .expect("valid month start")
        .timestamp()
}

fn repair_month_summary(month_start: i64) -> RepairMonthSummary {
    let month_iso = Utc
        .timestamp_opt(month_start, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .format("%Y-%m")
        .to_string();
    RepairMonthSummary {
        month_start,
        month_iso,
    }
}

fn request_log_needs_update(candidate: &RequestLogCandidate) -> bool {
    candidate.request_kind_key.as_deref() != Some(REQUEST_KIND_KEY)
        || candidate.request_kind_label.as_deref() != Some(REQUEST_KIND_LABEL)
        || candidate.request_kind_detail.is_some()
        || candidate.business_credits.is_some()
}

fn auth_token_log_needs_update(candidate: &AuthTokenLogCandidate) -> bool {
    candidate.request_kind_key.as_deref() != Some(REQUEST_KIND_KEY)
        || candidate.request_kind_label.as_deref() != Some(REQUEST_KIND_LABEL)
        || candidate.request_kind_detail.is_some()
        || candidate.counts_business_quota
        || candidate.business_credits.is_some()
        || candidate.billing_state != BILLING_STATE_NONE
        || candidate.billing_subject.is_some()
}

async fn load_request_log_candidates(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<RequestLogCandidate>, sqlx::Error> {
    let message_like = format!("%{}%", SESSION_DELETE_MESSAGE.to_ascii_lowercase());
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            created_at,
            request_kind_key,
            request_kind_label,
            request_kind_detail,
            business_credits
        FROM request_logs
        WHERE method = 'DELETE'
          AND path = '/mcp'
          AND status_code = 405
          AND tavily_status_code = 405
          AND failure_kind = ?
          AND (
                LOWER(CAST(COALESCE(response_body, X'') AS TEXT)) LIKE ?
                OR LOWER(COALESCE(error_message, '')) LIKE ?
          )
        ORDER BY id ASC
        "#,
    )
    .bind(FAILURE_KIND_MCP_METHOD_405)
    .bind(&message_like)
    .bind(&message_like)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(RequestLogCandidate {
                id: row.try_get("id")?,
                created_at: row.try_get("created_at")?,
                request_kind_key: row.try_get("request_kind_key")?,
                request_kind_label: row.try_get("request_kind_label")?,
                request_kind_detail: row.try_get("request_kind_detail")?,
                business_credits: row.try_get("business_credits")?,
            })
        })
        .collect()
}

async fn load_auth_token_log_candidates(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<AuthTokenLogCandidate>, sqlx::Error> {
    let message_like = format!("%{}%", SESSION_DELETE_MESSAGE.to_ascii_lowercase());
    let rows = sqlx::query(
        r#"
        SELECT
            atl.id,
            atl.token_id,
            atl.created_at,
            atl.request_kind_key,
            atl.request_kind_label,
            atl.request_kind_detail,
            atl.counts_business_quota,
            atl.business_credits,
            COALESCE(atl.billing_state, 'none') AS billing_state,
            atl.billing_subject
        FROM auth_token_logs atl
        LEFT JOIN request_logs rl ON rl.id = atl.request_log_id
        WHERE atl.method = 'DELETE'
          AND atl.path = '/mcp'
          AND COALESCE(atl.http_status, rl.status_code) = 405
          AND COALESCE(atl.mcp_status, rl.tavily_status_code) = 405
          AND (
                COALESCE(atl.failure_kind, rl.failure_kind) = ?
                OR COALESCE(atl.failure_kind, rl.failure_kind) IS NULL
          )
          AND (
                LOWER(COALESCE(atl.error_message, '')) LIKE ?
                OR LOWER(COALESCE(rl.error_message, '')) LIKE ?
                OR LOWER(CAST(COALESCE(rl.response_body, X'') AS TEXT)) LIKE ?
          )
        ORDER BY atl.id ASC
        "#,
    )
    .bind(FAILURE_KIND_MCP_METHOD_405)
    .bind(&message_like)
    .bind(&message_like)
    .bind(&message_like)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(AuthTokenLogCandidate {
                id: row.try_get("id")?,
                token_id: row.try_get("token_id")?,
                created_at: row.try_get("created_at")?,
                request_kind_key: row.try_get("request_kind_key")?,
                request_kind_label: row.try_get("request_kind_label")?,
                request_kind_detail: row.try_get("request_kind_detail")?,
                counts_business_quota: row.try_get::<i64, _>("counts_business_quota")? != 0,
                business_credits: row.try_get("business_credits")?,
                billing_state: row.try_get("billing_state")?,
                billing_subject: row.try_get("billing_subject")?,
            })
        })
        .collect()
}

async fn apply_request_log_updates_on_connection(
    connection: &mut SqliteConnection,
    candidates: &[RequestLogCandidate],
) -> Result<usize, sqlx::Error> {
    let mut updated = 0usize;
    for candidate in candidates
        .iter()
        .filter(|candidate| request_log_needs_update(candidate))
    {
        let result = sqlx::query(
            r#"
            UPDATE request_logs
            SET request_kind_key = ?,
                request_kind_label = ?,
                request_kind_detail = NULL,
                business_credits = NULL
            WHERE id = ?
            "#,
        )
        .bind(REQUEST_KIND_KEY)
        .bind(REQUEST_KIND_LABEL)
        .bind(candidate.id)
        .execute(&mut *connection)
        .await?;
        updated += result.rows_affected() as usize;
    }
    Ok(updated)
}

#[cfg(test)]
async fn apply_request_log_updates(
    pool: &sqlx::SqlitePool,
    candidates: &[RequestLogCandidate],
) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut updated = 0usize;
    for candidate in candidates
        .iter()
        .filter(|candidate| request_log_needs_update(candidate))
    {
        let result = sqlx::query(
            r#"
            UPDATE request_logs
            SET request_kind_key = ?,
                request_kind_label = ?,
                request_kind_detail = NULL,
                business_credits = NULL
            WHERE id = ?
            "#,
        )
        .bind(REQUEST_KIND_KEY)
        .bind(REQUEST_KIND_LABEL)
        .bind(candidate.id)
        .execute(&mut *tx)
        .await?;
        updated += result.rows_affected() as usize;
    }
    tx.commit().await?;
    Ok(updated)
}

async fn apply_auth_token_log_updates_on_connection(
    connection: &mut SqliteConnection,
    candidates: &[AuthTokenLogCandidate],
) -> Result<usize, sqlx::Error> {
    let mut updated = 0usize;
    for candidate in candidates
        .iter()
        .filter(|candidate| auth_token_log_needs_update(candidate))
    {
        let result = sqlx::query(
            r#"
            UPDATE auth_token_logs
            SET request_kind_key = ?,
                request_kind_label = ?,
                request_kind_detail = NULL,
                counts_business_quota = 0,
                business_credits = NULL,
                billing_state = ?,
                billing_subject = NULL
            WHERE id = ?
            "#,
        )
        .bind(REQUEST_KIND_KEY)
        .bind(REQUEST_KIND_LABEL)
        .bind(BILLING_STATE_NONE)
        .bind(candidate.id)
        .execute(&mut *connection)
        .await?;
        updated += result.rows_affected() as usize;
    }
    Ok(updated)
}

#[cfg(test)]
async fn apply_auth_token_log_updates(
    pool: &sqlx::SqlitePool,
    candidates: &[AuthTokenLogCandidate],
) -> Result<usize, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let mut updated = 0usize;
    for candidate in candidates
        .iter()
        .filter(|candidate| auth_token_log_needs_update(candidate))
    {
        let result = sqlx::query(
            r#"
            UPDATE auth_token_logs
            SET request_kind_key = ?,
                request_kind_label = ?,
                request_kind_detail = NULL,
                counts_business_quota = 0,
                business_credits = NULL,
                billing_state = ?,
                billing_subject = NULL
            WHERE id = ?
            "#,
        )
        .bind(REQUEST_KIND_KEY)
        .bind(REQUEST_KIND_LABEL)
        .bind(BILLING_STATE_NONE)
        .bind(candidate.id)
        .execute(&mut *tx)
        .await?;
        updated += result.rows_affected() as usize;
    }
    tx.commit().await?;
    Ok(updated)
}

fn touched_months(
    request_candidates: &[RequestLogCandidate],
    auth_candidates: &[AuthTokenLogCandidate],
) -> Vec<RepairMonthSummary> {
    let mut month_starts = BTreeSet::new();
    for candidate in request_candidates {
        month_starts.insert(repair_month_start(candidate.created_at));
    }
    for candidate in auth_candidates {
        month_starts.insert(repair_month_start(candidate.created_at));
    }
    month_starts
        .into_iter()
        .map(repair_month_summary)
        .collect::<Vec<_>>()
}

fn affected_tokens(auth_candidates: &[AuthTokenLogCandidate]) -> Vec<String> {
    auth_candidates
        .iter()
        .map(|candidate| candidate.token_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn repair_month_end(month_start: i64) -> i64 {
    let current_month_start = repair_month_start(Utc::now().timestamp());
    if month_start == current_month_start {
        return Utc::now().timestamp();
    }

    let dt = Utc
        .timestamp_opt(month_start, 0)
        .single()
        .unwrap_or_else(Utc::now);
    let next_month_start = if dt.month() == 12 {
        Utc.with_ymd_and_hms(dt.year() + 1, 1, 1, 0, 0, 0)
            .single()
            .expect("valid next month start")
    } else {
        Utc.with_ymd_and_hms(dt.year(), dt.month() + 1, 1, 0, 0, 0)
            .single()
            .expect("valid next month start")
    };
    next_month_start.timestamp() - 1
}

async fn rebase_current_month_business_quota_with_connection(
    connection: &mut SqliteConnection,
    now: chrono::DateTime<Utc>,
) -> Result<MonthlyQuotaRebaseReport, ProxyError> {
    let locked_now = Utc::now();
    let generated_at = if locked_now > now {
        locked_now.timestamp()
    } else {
        now.timestamp()
    };
    let target_month_start = repair_month_start(generated_at);
    let previous_rebase_month_start =
        read_meta_i64(connection, META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1).await?;

    let invalid_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM auth_token_logs
        WHERE billing_state = 'charged'
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
          AND (
                billing_subject IS NULL
                OR (
                    billing_subject NOT LIKE 'token:%'
                    AND billing_subject NOT LIKE 'account:%'
                )
          )
        "#,
    )
    .bind(target_month_start)
    .bind(generated_at)
    .fetch_one(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;
    if invalid_count > 0 {
        return Err(ProxyError::QuotaDataMissing {
            reason: format!(
                "found {invalid_count} charged auth_token_logs rows with invalid billing_subject between {target_month_start} and {generated_at}"
            ),
        });
    }

    let (current_month_charged_rows, current_month_charged_credits): (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) AS charged_rows,
            COALESCE(SUM(business_credits), 0) AS charged_credits
        FROM auth_token_logs
        WHERE billing_state = 'charged'
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
        "#,
    )
    .bind(target_month_start)
    .bind(generated_at)
    .fetch_one(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;

    let rebased_subjects = sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT
            billing_subject,
            COALESCE(SUM(business_credits), 0) AS total_credits,
            COUNT(*) AS charged_rows
        FROM auth_token_logs
        WHERE billing_state = 'charged'
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
        GROUP BY billing_subject
        ORDER BY billing_subject ASC
        "#,
    )
    .bind(target_month_start)
    .bind(generated_at)
    .fetch_all(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;

    let cleared_token_rows =
        sqlx::query("UPDATE auth_token_quota SET month_start = ?, month_count = 0")
            .bind(target_month_start)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?
            .rows_affected() as i64;
    let cleared_account_rows =
        sqlx::query("UPDATE account_monthly_quota SET month_start = ?, month_count = 0")
            .bind(target_month_start)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?
            .rows_affected() as i64;

    let mut rebased_token_subjects = 0usize;
    let mut rebased_account_subjects = 0usize;
    for (billing_subject, total_credits, _row_count) in &rebased_subjects {
        if let Some(token_id) = billing_subject.strip_prefix("token:") {
            sqlx::query(
                r#"
                INSERT INTO auth_token_quota (token_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(token_id) DO UPDATE SET
                    month_start = excluded.month_start,
                    month_count = excluded.month_count
                "#,
            )
            .bind(token_id)
            .bind(target_month_start)
            .bind(*total_credits)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?;
            rebased_token_subjects += 1;
            continue;
        }

        if let Some(user_id) = billing_subject.strip_prefix("account:") {
            sqlx::query(
                r#"
                INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    month_start = excluded.month_start,
                    month_count = excluded.month_count
                "#,
            )
            .bind(user_id)
            .bind(target_month_start)
            .bind(*total_credits)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?;
            rebased_account_subjects += 1;
            continue;
        }
    }

    let meta_updated = previous_rebase_month_start != Some(target_month_start);
    write_meta_i64(
        connection,
        META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
        target_month_start,
    )
    .await?;

    Ok(MonthlyQuotaRebaseReport {
        current_month_start: target_month_start,
        previous_rebase_month_start,
        current_month_charged_rows,
        current_month_charged_credits,
        rebased_subject_count: rebased_subjects.len(),
        rebased_token_subjects,
        rebased_account_subjects,
        cleared_token_rows,
        cleared_account_rows,
        meta_updated,
    })
}

async fn rebase_historical_business_quota_month_with_connection(
    connection: &mut SqliteConnection,
    target_month_start: i64,
) -> Result<MonthlyQuotaRebaseReport, ProxyError> {
    let generated_at = repair_month_end(target_month_start);

    let invalid_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM auth_token_logs
        WHERE billing_state = 'charged'
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
          AND (
                billing_subject IS NULL
                OR (
                    billing_subject NOT LIKE 'token:%'
                    AND billing_subject NOT LIKE 'account:%'
                )
          )
        "#,
    )
    .bind(target_month_start)
    .bind(generated_at)
    .fetch_one(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;
    if invalid_count > 0 {
        return Err(ProxyError::QuotaDataMissing {
            reason: format!(
                "found {invalid_count} charged auth_token_logs rows with invalid billing_subject between {target_month_start} and {generated_at}"
            ),
        });
    }

    let (current_month_charged_rows, current_month_charged_credits): (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) AS charged_rows,
            COALESCE(SUM(business_credits), 0) AS charged_credits
        FROM auth_token_logs
        WHERE billing_state = 'charged'
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
        "#,
    )
    .bind(target_month_start)
    .bind(generated_at)
    .fetch_one(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;

    let rebased_subjects = sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT
            billing_subject,
            COALESCE(SUM(business_credits), 0) AS total_credits,
            COUNT(*) AS charged_rows
        FROM auth_token_logs
        WHERE billing_state = 'charged'
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
          AND created_at <= ?
        GROUP BY billing_subject
        ORDER BY billing_subject ASC
        "#,
    )
    .bind(target_month_start)
    .bind(generated_at)
    .fetch_all(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;

    let cleared_token_rows =
        sqlx::query("UPDATE auth_token_quota SET month_count = 0 WHERE month_start = ?")
            .bind(target_month_start)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?
            .rows_affected() as i64;
    let cleared_account_rows =
        sqlx::query("UPDATE account_monthly_quota SET month_count = 0 WHERE month_start = ?")
            .bind(target_month_start)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?
            .rows_affected() as i64;

    let mut rebased_token_subjects = 0usize;
    let mut rebased_account_subjects = 0usize;
    for (billing_subject, total_credits, _row_count) in &rebased_subjects {
        if let Some(token_id) = billing_subject.strip_prefix("token:") {
            sqlx::query(
                r#"
                INSERT INTO auth_token_quota (token_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(token_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start >= auth_token_quota.month_start THEN excluded.month_start
                        ELSE auth_token_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start >= auth_token_quota.month_start THEN excluded.month_count
                        ELSE auth_token_quota.month_count
                    END
                "#,
            )
            .bind(token_id)
            .bind(target_month_start)
            .bind(*total_credits)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?;
            rebased_token_subjects += 1;
            continue;
        }

        if let Some(user_id) = billing_subject.strip_prefix("account:") {
            sqlx::query(
                r#"
                INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                VALUES (?, ?, ?)
                ON CONFLICT(user_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start >= account_monthly_quota.month_start THEN excluded.month_start
                        ELSE account_monthly_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start >= account_monthly_quota.month_start THEN excluded.month_count
                        ELSE account_monthly_quota.month_count
                    END
                "#,
            )
            .bind(user_id)
            .bind(target_month_start)
            .bind(*total_credits)
            .execute(&mut *connection)
            .await
            .map_err(ProxyError::Database)?;
            rebased_account_subjects += 1;
            continue;
        }
    }

    Ok(MonthlyQuotaRebaseReport {
        current_month_start: target_month_start,
        previous_rebase_month_start: None,
        current_month_charged_rows,
        current_month_charged_credits,
        rebased_subject_count: rebased_subjects.len(),
        rebased_token_subjects,
        rebased_account_subjects,
        cleared_token_rows,
        cleared_account_rows,
        meta_updated: false,
    })
}

async fn rebuild_all_token_usage_stats_with_connection(
    connection: &mut SqliteConnection,
) -> Result<i64, ProxyError> {
    let (max_log_id, max_created_at): (Option<i64>, Option<i64>) = sqlx::query_as(
        r#"
        SELECT
            MAX(id) AS max_log_id,
            MAX(created_at) AS max_created_at
        FROM auth_token_logs
        WHERE counts_business_quota = 1
        "#,
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(ProxyError::Database)?;

    let deleted = sqlx::query("DELETE FROM token_usage_stats")
        .execute(&mut *connection)
        .await
        .map_err(ProxyError::Database)?
        .rows_affected() as i64;
    let inserted = sqlx::query(
        r#"
        INSERT INTO token_usage_stats (
            token_id,
            bucket_start,
            bucket_secs,
            success_count,
            system_failure_count,
            external_failure_count,
            quota_exhausted_count
        )
        SELECT
            token_id,
            (created_at / ?) * ? AS bucket_start,
            ? AS bucket_secs,
            SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
            SUM(
                CASE
                    WHEN result_status != 'success'
                         AND result_status != 'quota_exhausted'
                         AND (
                            (http_status BETWEEN 400 AND 599)
                            OR (mcp_status BETWEEN 400 AND 599)
                        ) THEN 1
                    ELSE 0
                END
            ) AS system_failure_count,
            SUM(
                CASE
                    WHEN result_status != 'success'
                         AND result_status != 'quota_exhausted'
                         AND NOT (
                            (http_status BETWEEN 400 AND 599)
                            OR (mcp_status BETWEEN 400 AND 599)
                        ) THEN 1
                    ELSE 0
                END
            ) AS external_failure_count,
            SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
        FROM auth_token_logs
        WHERE counts_business_quota = 1
        GROUP BY token_id, bucket_start
        "#,
    )
    .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
    .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
    .bind(TOKEN_USAGE_STATS_BUCKET_SECS)
    .execute(&mut *connection)
    .await
    .map_err(ProxyError::Database)?
    .rows_affected() as i64;

    write_meta_i64(
        connection,
        META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2,
        max_log_id.unwrap_or(0),
    )
    .await?;
    if let Some(ts) = max_created_at {
        let legacy_ts = read_meta_i64(connection, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?;
        write_meta_i64(
            connection,
            META_KEY_TOKEN_USAGE_ROLLUP_TS,
            legacy_ts.map_or(ts, |old| old.max(ts)),
        )
        .await?;
    }

    Ok(deleted + inserted)
}

async fn execute_apply_repair(
    db_path: &str,
    request_candidates: &[RequestLogCandidate],
    auth_candidates: &[AuthTokenLogCandidate],
) -> Result<RepairExecutionSummary, ProxyError> {
    let mut connection = connect_immediate_sqlite_connection(db_path)
        .await
        .map_err(ProxyError::Database)?;
    let result = async {
        let request_rows_updated =
            apply_request_log_updates_on_connection(&mut connection, request_candidates).await?;
        let auth_token_rows_updated =
            apply_auth_token_log_updates_on_connection(&mut connection, auth_candidates).await?;
        let token_usage_stats_rows_rebuilt = if auth_token_rows_updated == 0 {
            0
        } else {
            rebuild_all_token_usage_stats_with_connection(&mut connection).await?
        };
        let touched_months = touched_months(request_candidates, auth_candidates);
        let monthly_rebase = if auth_token_rows_updated == 0 {
            None
        } else {
            rebase_touched_business_quota_months_with_connection(&mut connection, &touched_months)
                .await?
        };
        Ok(RepairExecutionSummary {
            request_rows_updated,
            auth_token_rows_updated,
            token_usage_stats_rows_rebuilt,
            monthly_rebase,
        })
    }
    .await;

    match result {
        Ok(summary) => {
            sqlx::query("COMMIT")
                .execute(&mut connection)
                .await
                .map_err(ProxyError::Database)?;
            Ok(summary)
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut connection).await;
            Err(err)
        }
    }
}

async fn rebase_touched_business_quota_months_with_connection(
    connection: &mut SqliteConnection,
    touched_months: &[RepairMonthSummary],
) -> Result<Option<Value>, ProxyError> {
    if touched_months.is_empty() {
        return Ok(None);
    }

    let current_month_start = repair_month_start(Utc::now().timestamp());
    let mut entries = Vec::with_capacity(touched_months.len());
    for month in touched_months {
        let report = if month.month_start == current_month_start {
            rebase_current_month_business_quota_with_connection(connection, Utc::now()).await?
        } else {
            rebase_historical_business_quota_month_with_connection(connection, month.month_start)
                .await?
        };
        entries.push(MonthlyRebaseEntry {
            month_start: month.month_start,
            month_iso: month.month_iso.clone(),
            report,
        });
    }

    serde_json::to_value(entries)
        .map(Some)
        .map_err(|err| ProxyError::Other(err.to_string()))
}

#[cfg(test)]
async fn rebase_touched_business_quota_months(
    pool: &sqlx::SqlitePool,
    db_path: &str,
    touched_months: &[RepairMonthSummary],
) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    let _ = pool;
    let mut connection = connect_immediate_sqlite_connection(db_path).await?;
    let result =
        rebase_touched_business_quota_months_with_connection(&mut connection, touched_months).await;
    match result {
        Ok(value) => {
            sqlx::query("COMMIT").execute(&mut connection).await?;
            Ok(value)
        }
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut connection).await;
            Err(Box::new(err))
        }
    }
}

fn build_report(
    dry_run: bool,
    apply: bool,
    request_candidates: &[RequestLogCandidate],
    auth_candidates: &[AuthTokenLogCandidate],
    execution: RepairExecutionSummary,
) -> RepairReport {
    let affected_tokens = affected_tokens(auth_candidates);
    RepairReport {
        dry_run,
        apply,
        matched_request_rows: request_candidates.len(),
        matched_auth_token_rows: auth_candidates.len(),
        request_rows_needing_update: request_candidates
            .iter()
            .filter(|candidate| request_log_needs_update(candidate))
            .count(),
        auth_token_rows_needing_update: auth_candidates
            .iter()
            .filter(|candidate| auth_token_log_needs_update(candidate))
            .count(),
        affected_token_count: affected_tokens.len(),
        affected_tokens,
        touched_months: touched_months(request_candidates, auth_candidates),
        request_log_ids: request_candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect(),
        auth_token_log_ids: auth_candidates
            .iter()
            .map(|candidate| candidate.id)
            .collect(),
        request_rows_updated: execution.request_rows_updated,
        auth_token_rows_updated: execution.auth_token_rows_updated,
        token_usage_stats_rows_rebuilt: execution.token_usage_stats_rows_rebuilt,
        monthly_rebase: execution.monthly_rebase,
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
    let apply = cli.apply;
    let dry_run = cli.dry_run || !apply;

    let db_path = Path::new(&cli.db_path);
    if let Some(parent) = db_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let pool = connect_sqlite_pool(&cli.db_path).await?;
    let request_candidates = load_request_log_candidates(&pool).await?;
    let auth_candidates = load_auth_token_log_candidates(&pool).await?;

    let (
        request_rows_updated,
        auth_token_rows_updated,
        token_usage_stats_rows_rebuilt,
        monthly_rebase,
    ) = if dry_run {
        (0, 0, 0, None)
    } else {
        let execution =
            execute_apply_repair(&cli.db_path, &request_candidates, &auth_candidates).await?;
        (
            execution.request_rows_updated,
            execution.auth_token_rows_updated,
            execution.token_usage_stats_rows_rebuilt,
            execution.monthly_rebase,
        )
    };

    let report = build_report(
        dry_run,
        apply,
        &request_candidates,
        &auth_candidates,
        RepairExecutionSummary {
            request_rows_updated,
            auth_token_rows_updated,
            token_usage_stats_rows_rebuilt,
            monthly_rebase,
        },
    );
    write_report(io::stdout().lock(), &report)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AuthTokenLogCandidate, BILLING_STATE_NONE, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2,
        REQUEST_KIND_KEY, REQUEST_KIND_LABEL, RepairExecutionSummary, apply_auth_token_log_updates,
        apply_request_log_updates, build_report, connect_sqlite_pool, execute_apply_repair,
        load_auth_token_log_candidates, load_request_log_candidates,
        rebase_touched_business_quota_months, repair_month_start, request_log_needs_update,
        touched_months,
    };
    use chrono::{Datelike, TimeZone, Utc};
    use nanoid::nanoid;
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

    async fn seed_session_delete_misclassified_logs(
        pool: &sqlx::SqlitePool,
        token_id: &str,
        created_at: i64,
    ) -> (i64, i64) {
        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (
                NULL,
                ?,
                'DELETE',
                '/mcp',
                NULL,
                405,
                405,
                'Method Not Allowed: Session termination not supported',
                'error',
                'mcp:unknown-payload',
                'MCP | unknown payload',
                '/mcp',
                2,
                'mcp_method_405',
                'none',
                NULL,
                X'7B7D',
                ?,
                '[]',
                '[]',
                ?
            )
            RETURNING id
            "#,
        )
        .bind(token_id)
        .bind(
            br#"{"error":"Method Not Allowed","message":"Method Not Allowed: Session termination not supported"}"#.as_slice(),
        )
        .bind(created_at)
        .fetch_one(pool)
        .await
        .expect("insert request log");

        let auth_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (
                ?,
                'DELETE',
                '/mcp',
                NULL,
                405,
                405,
                'mcp:unknown-payload',
                'MCP | unknown payload',
                '/mcp',
                'error',
                'Method Not Allowed: Session termination not supported',
                'mcp_method_405',
                'none',
                NULL,
                1,
                2,
                ?,
                'charged',
                NULL,
                ?,
                ?
            )
            RETURNING id
            "#,
        )
        .bind(token_id)
        .bind(format!("token:{token_id}"))
        .bind(request_log_id)
        .bind(created_at)
        .fetch_one(pool)
        .await
        .expect("insert auth token log");

        (request_log_id, auth_log_id)
    }

    async fn seed_billable_search_auth_log(
        pool: &sqlx::SqlitePool,
        token_id: &str,
        created_at: i64,
    ) -> i64 {
        sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (
                ?,
                'POST',
                '/search',
                NULL,
                200,
                NULL,
                'api:search',
                'API | search',
                NULL,
                'success',
                NULL,
                NULL,
                'none',
                NULL,
                1,
                1,
                ?,
                'charged',
                NULL,
                NULL,
                ?
            )
            RETURNING id
            "#,
        )
        .bind(token_id)
        .bind(format!("token:{token_id}"))
        .bind(created_at)
        .fetch_one(pool)
        .await
        .expect("insert billable search auth log")
    }

    fn current_month_start(ts: i64) -> i64 {
        let dt = Utc.timestamp_opt(ts, 0).single().expect("valid timestamp");
        Utc.with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
            .single()
            .expect("valid month start")
            .timestamp()
    }

    #[tokio::test]
    async fn dry_run_detects_candidates_without_writing() {
        let (proxy, pool, db_str) = init_proxy_and_pool("session-delete-repair-dry-run").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-dry-run"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();
        let (request_log_id, auth_log_id) =
            seed_session_delete_misclassified_logs(&pool, &token.id, created_at).await;

        let request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load request candidates");
        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");

        assert_eq!(request_candidates.len(), 1);
        assert_eq!(auth_candidates.len(), 1);
        assert!(request_log_needs_update(&request_candidates[0]));
        assert_eq!(auth_candidates[0].id, auth_log_id);

        let request_kind_key: String =
            sqlx::query_scalar("SELECT request_kind_key FROM request_logs WHERE id = ?")
                .bind(request_log_id)
                .fetch_one(&pool)
                .await
                .expect("read request kind");
        assert_eq!(request_kind_key, "mcp:unknown-payload");

        let report = build_report(
            true,
            false,
            &request_candidates,
            &auth_candidates,
            RepairExecutionSummary {
                request_rows_updated: 0,
                auth_token_rows_updated: 0,
                token_usage_stats_rows_rebuilt: 0,
                monthly_rebase: None,
            },
        );
        assert_eq!(report.request_rows_needing_update, 1);
        assert_eq!(report.auth_token_rows_needing_update, 1);
        assert_eq!(report.affected_token_count, 1);
        assert_eq!(report.request_rows_updated, 0);
        assert_eq!(report.auth_token_rows_updated, 0);
        assert_eq!(report.token_usage_stats_rows_rebuilt, 0);
        assert_eq!(report.touched_months.len(), 1);

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn request_candidates_match_error_text_without_response_body() {
        let (proxy, pool, db_str) =
            init_proxy_and_pool("session-delete-repair-error-text-request").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-error-text-request"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();

        let request_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO request_logs (
                api_key_id,
                auth_token_id,
                method,
                path,
                query,
                status_code,
                tavily_status_code,
                error_message,
                result_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                business_credits,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                request_body,
                response_body,
                forwarded_headers,
                dropped_headers,
                created_at
            ) VALUES (
                NULL,
                ?,
                'DELETE',
                '/mcp',
                NULL,
                405,
                405,
                'Method Not Allowed: Session termination not supported',
                'error',
                'mcp:unknown-payload',
                'MCP | unknown payload',
                '/mcp',
                2,
                'mcp_method_405',
                'none',
                NULL,
                X'7B7D',
                X'7B226D657373616765223A226F74686572227D',
                '[]',
                '[]',
                ?
            )
            RETURNING id
            "#,
        )
        .bind(&token.id)
        .bind(created_at)
        .fetch_one(&pool)
        .await
        .expect("insert request log");

        let request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load request candidates");
        assert_eq!(request_candidates.len(), 1);
        assert_eq!(request_candidates[0].id, request_log_id);

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn apply_updates_rows_and_rebuilds_derived_usage() {
        let (proxy, pool, db_str) = init_proxy_and_pool("session-delete-repair-apply").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-apply"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();
        let current_month_start = current_month_start(created_at);
        let (request_log_id, auth_log_id) =
            seed_session_delete_misclassified_logs(&pool, &token.id, created_at).await;

        sqlx::query(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = excluded.month_start,
                month_count = excluded.month_count
            "#,
        )
        .bind(&token.id)
        .bind(current_month_start)
        .bind(2_i64)
        .execute(&pool)
        .await
        .expect("seed month quota");

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("rollup token usage stats");
        let stats_before: Option<(i64,)> =
            sqlx::query_as("SELECT system_failure_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&token.id)
                .fetch_optional(&pool)
                .await
                .expect("read usage stats before repair");
        assert_eq!(stats_before, Some((1,)));

        let request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load request candidates");
        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");
        let execution = execute_apply_repair(&db_str, &request_candidates, &auth_candidates)
            .await
            .expect("apply repair");
        assert_eq!(execution.request_rows_updated, 1);
        assert_eq!(execution.auth_token_rows_updated, 1);
        assert!(execution.token_usage_stats_rows_rebuilt >= 1);
        assert!(execution.monthly_rebase.is_some());

        let request_row = sqlx::query(
            "SELECT request_kind_key, request_kind_label, request_kind_detail, business_credits FROM request_logs WHERE id = ?",
        )
        .bind(request_log_id)
        .fetch_one(&pool)
        .await
        .expect("read repaired request row");
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("request_kind_key")
                .expect("request kind key")
                .as_deref(),
            Some(REQUEST_KIND_KEY)
        );
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("request_kind_label")
                .expect("request kind label")
                .as_deref(),
            Some(REQUEST_KIND_LABEL)
        );
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("request_kind_detail")
                .expect("request kind detail"),
            None
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("request business credits"),
            None
        );

        let auth_row = sqlx::query(
            "SELECT request_kind_key, request_kind_label, request_kind_detail, counts_business_quota, business_credits, billing_state, billing_subject FROM auth_token_logs WHERE id = ?",
        )
        .bind(auth_log_id)
        .fetch_one(&pool)
        .await
        .expect("read repaired auth row");
        assert_eq!(
            auth_row
                .try_get::<Option<String>, _>("request_kind_key")
                .expect("auth request kind key")
                .as_deref(),
            Some(REQUEST_KIND_KEY)
        );
        assert_eq!(
            auth_row
                .try_get::<Option<String>, _>("request_kind_label")
                .expect("auth request kind label")
                .as_deref(),
            Some(REQUEST_KIND_LABEL)
        );
        assert_eq!(
            auth_row
                .try_get::<Option<String>, _>("request_kind_detail")
                .expect("auth request kind detail"),
            None
        );
        assert_eq!(
            auth_row
                .try_get::<i64, _>("counts_business_quota")
                .expect("counts business quota"),
            0
        );
        assert_eq!(
            auth_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("auth business credits"),
            None
        );
        assert_eq!(
            auth_row
                .try_get::<String, _>("billing_state")
                .expect("billing state"),
            BILLING_STATE_NONE
        );
        assert_eq!(
            auth_row
                .try_get::<Option<String>, _>("billing_subject")
                .expect("billing subject"),
            None
        );

        let stats_after: Option<(i64,)> =
            sqlx::query_as("SELECT system_failure_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&token.id)
                .fetch_optional(&pool)
                .await
                .expect("read usage stats after repair");
        assert_eq!(stats_after, None);

        let month_count_after: i64 =
            sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ?")
                .bind(&token.id)
                .fetch_one(&pool)
                .await
                .expect("read month quota after repair");
        assert_eq!(month_count_after, 0);

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn auth_candidates_include_standalone_rows_matched_by_error_text() {
        let (proxy, pool, db_str) =
            init_proxy_and_pool("session-delete-repair-standalone-auth").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-standalone-auth"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();

        let auth_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (
                ?,
                'DELETE',
                '/mcp',
                NULL,
                405,
                405,
                'mcp:unknown-payload',
                'MCP | unknown payload',
                '/mcp',
                'error',
                'Method Not Allowed: Session termination not supported',
                'mcp_method_405',
                'none',
                NULL,
                1,
                2,
                ?,
                'charged',
                NULL,
                NULL,
                ?
            )
            RETURNING id
            "#,
        )
        .bind(&token.id)
        .bind(format!("token:{}", token.id))
        .bind(created_at)
        .fetch_one(&pool)
        .await
        .expect("insert standalone auth token log");

        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");
        assert_eq!(auth_candidates.len(), 1);
        assert_eq!(auth_candidates[0].id, auth_log_id);
        assert_eq!(
            apply_auth_token_log_updates(&pool, &auth_candidates)
                .await
                .expect("apply auth updates"),
            1
        );

        let updated = sqlx::query(
            "SELECT request_kind_key, counts_business_quota, business_credits, billing_state FROM auth_token_logs WHERE id = ?",
        )
        .bind(auth_log_id)
        .fetch_one(&pool)
        .await
        .expect("read updated auth row");
        assert_eq!(
            updated
                .try_get::<Option<String>, _>("request_kind_key")
                .expect("request kind key")
                .as_deref(),
            Some(REQUEST_KIND_KEY)
        );
        assert_eq!(
            updated
                .try_get::<i64, _>("counts_business_quota")
                .expect("counts_business_quota"),
            0
        );
        assert_eq!(
            updated
                .try_get::<Option<i64>, _>("business_credits")
                .expect("business_credits"),
            None
        );
        assert_eq!(
            updated
                .try_get::<String, _>("billing_state")
                .expect("billing_state"),
            BILLING_STATE_NONE
        );

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn auth_candidates_include_rows_when_failure_kind_is_missing() {
        let (proxy, pool, db_str) =
            init_proxy_and_pool("session-delete-repair-null-failure-kind").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-null-failure-kind"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();

        let auth_log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (
                ?,
                'DELETE',
                '/mcp',
                NULL,
                405,
                405,
                'mcp:unknown-payload',
                'MCP | unknown payload',
                '/mcp',
                'error',
                'Method Not Allowed: Session termination not supported',
                NULL,
                'none',
                NULL,
                1,
                2,
                ?,
                'charged',
                NULL,
                NULL,
                ?
            )
            RETURNING id
            "#,
        )
        .bind(&token.id)
        .bind(format!("token:{}", token.id))
        .bind(created_at)
        .fetch_one(&pool)
        .await
        .expect("insert standalone auth token log without failure kind");

        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");
        assert_eq!(auth_candidates.len(), 1);
        assert_eq!(auth_candidates[0].id, auth_log_id);

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn apply_is_idempotent_after_first_repair() {
        let (proxy, pool, db_str) = init_proxy_and_pool("session-delete-repair-idempotent").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-idempotent"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();
        seed_session_delete_misclassified_logs(&pool, &token.id, created_at).await;

        let first_request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load first request candidates");
        let first_auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load first auth candidates");
        let first_execution =
            execute_apply_repair(&db_str, &first_request_candidates, &first_auth_candidates)
                .await
                .expect("first repair apply");
        assert_eq!(first_execution.request_rows_updated, 1);
        assert!(first_execution.auth_token_rows_updated <= 1);
        if first_execution.auth_token_rows_updated > 0 {
            assert!(first_execution.monthly_rebase.is_some());
        }

        let second_request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load second request candidates");
        let second_auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load second auth candidates");
        let second_execution =
            execute_apply_repair(&db_str, &second_request_candidates, &second_auth_candidates)
                .await
                .expect("second repair apply");
        assert_eq!(second_execution.request_rows_updated, 0);
        assert_eq!(second_execution.auth_token_rows_updated, 0);
        assert_eq!(second_execution.token_usage_stats_rows_rebuilt, 0);
        assert_eq!(
            second_request_candidates
                .iter()
                .filter(|candidate| request_log_needs_update(candidate))
                .count(),
            0
        );
        assert_eq!(
            second_auth_candidates
                .iter()
                .filter(|candidate| super::auth_token_log_needs_update(candidate))
                .count(),
            0
        );

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn apply_repair_rolls_back_when_derived_rebuild_fails() {
        let (proxy, pool, db_str) =
            init_proxy_and_pool("session-delete-repair-atomic-rollback").await;
        let token = proxy
            .create_access_token(Some("session-delete-repair-atomic-rollback"))
            .await
            .expect("create token");
        let created_at = Utc::now().timestamp();
        let (request_log_id, auth_log_id) =
            seed_session_delete_misclassified_logs(&pool, &token.id, created_at).await;

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("initial rollup");
        let stats_before_failure: Option<(i64,)> =
            sqlx::query_as("SELECT system_failure_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&token.id)
                .fetch_optional(&pool)
                .await
                .expect("read usage stats before rollback");

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (
                ?,
                'POST',
                '/search',
                NULL,
                200,
                NULL,
                'api:search',
                'API | search',
                NULL,
                'success',
                NULL,
                NULL,
                'none',
                NULL,
                1,
                3,
                'broken-subject',
                'charged',
                NULL,
                NULL,
                ?
            )
            "#,
        )
        .bind(&token.id)
        .bind(created_at)
        .execute(&pool)
        .await
        .expect("insert invalid charged row");

        let request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load request candidates");
        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");
        let err = execute_apply_repair(&db_str, &request_candidates, &auth_candidates)
            .await
            .expect_err("repair should roll back on invalid quota data");
        match err {
            tavily_hikari::ProxyError::QuotaDataMissing { .. } => {}
            other => panic!("unexpected repair error: {other}"),
        }

        let request_row =
            sqlx::query("SELECT request_kind_key, business_credits FROM request_logs WHERE id = ?")
                .bind(request_log_id)
                .fetch_one(&pool)
                .await
                .expect("request row after rollback");
        assert_eq!(
            request_row
                .try_get::<Option<String>, _>("request_kind_key")
                .expect("request kind key")
                .as_deref(),
            Some("mcp:unknown-payload")
        );
        assert_eq!(
            request_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("business credits"),
            Some(2)
        );

        let auth_row = sqlx::query(
            "SELECT request_kind_key, counts_business_quota, business_credits, billing_state FROM auth_token_logs WHERE id = ?",
        )
        .bind(auth_log_id)
        .fetch_one(&pool)
        .await
        .expect("auth row after rollback");
        assert_eq!(
            auth_row
                .try_get::<Option<String>, _>("request_kind_key")
                .expect("request kind key")
                .as_deref(),
            Some("mcp:unknown-payload")
        );
        assert_eq!(
            auth_row
                .try_get::<i64, _>("counts_business_quota")
                .expect("counts business quota"),
            1
        );
        assert_eq!(
            auth_row
                .try_get::<Option<i64>, _>("business_credits")
                .expect("business credits"),
            Some(2)
        );
        assert_eq!(
            auth_row
                .try_get::<String, _>("billing_state")
                .expect("billing state"),
            "charged"
        );

        let stats_after_failure: Option<(i64,)> =
            sqlx::query_as("SELECT system_failure_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&token.id)
                .fetch_optional(&pool)
                .await
                .expect("read usage stats after rollback");
        assert_eq!(stats_after_failure, stats_before_failure);

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn apply_repair_rebuilds_usage_stats_without_tail_double_counting() {
        let (proxy, pool, db_str) =
            init_proxy_and_pool("session-delete-repair-rollup-cursor").await;
        let repaired_token = proxy
            .create_access_token(Some("session-delete-repair-rollup-cursor-primary"))
            .await
            .expect("create repaired token");
        let unaffected_token = proxy
            .create_access_token(Some("session-delete-repair-rollup-cursor-tail"))
            .await
            .expect("create unaffected token");
        let created_at = Utc::now().timestamp();
        seed_session_delete_misclassified_logs(&pool, &repaired_token.id, created_at).await;

        proxy
            .rollup_token_usage_stats()
            .await
            .expect("initial rollup");

        let tail_log_id =
            seed_billable_search_auth_log(&pool, &unaffected_token.id, created_at + 1).await;

        let request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load request candidates");
        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");
        let execution = execute_apply_repair(&db_str, &request_candidates, &auth_candidates)
            .await
            .expect("apply repair");
        assert_eq!(execution.request_rows_updated, 1);
        assert_eq!(execution.auth_token_rows_updated, 1);
        assert!(execution.token_usage_stats_rows_rebuilt >= 1);

        let rollup_cursor: Option<String> =
            sqlx::query_scalar("SELECT value FROM meta WHERE key = ? LIMIT 1")
                .bind(META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2)
                .fetch_optional(&pool)
                .await
                .expect("read rollup cursor");
        assert_eq!(
            rollup_cursor.and_then(|value| value.parse::<i64>().ok()),
            Some(tail_log_id)
        );

        let repaired_stats: Option<(i64,)> =
            sqlx::query_as("SELECT system_failure_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&repaired_token.id)
                .fetch_optional(&pool)
                .await
                .expect("read repaired token stats");
        assert_eq!(repaired_stats, None);

        let tail_stats_before: Option<(i64,)> =
            sqlx::query_as("SELECT success_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&unaffected_token.id)
                .fetch_optional(&pool)
                .await
                .expect("read tail stats before follow-up rollup");
        assert_eq!(tail_stats_before, Some((1,)));

        let rollup = proxy
            .rollup_token_usage_stats()
            .await
            .expect("follow-up rollup");
        assert_eq!(rollup.0, 0);

        let tail_stats_after: Option<(i64,)> =
            sqlx::query_as("SELECT success_count FROM token_usage_stats WHERE token_id = ?")
                .bind(&unaffected_token.id)
                .fetch_optional(&pool)
                .await
                .expect("read tail stats after follow-up rollup");
        assert_eq!(tail_stats_after, Some((1,)));

        let _ = std::fs::remove_file(db_str);
    }

    #[tokio::test]
    async fn historical_month_rebase_preserves_newer_quota_rows() {
        let (proxy, pool, db_str) =
            init_proxy_and_pool("session-delete-repair-historical-rebase").await;
        let old_token = proxy
            .create_access_token(Some("session-delete-repair-old-month"))
            .await
            .expect("create old token");
        let current_token = proxy
            .create_access_token(Some("session-delete-repair-current-month"))
            .await
            .expect("create current token");

        let now = Utc::now();
        let current_start = current_month_start(now.timestamp());
        let previous_month_point = current_start - 1;
        let previous_start = current_month_start(previous_month_point);
        let old_created_at = previous_start + 12 * 60 * 60;
        let current_created_at = now.timestamp();

        seed_session_delete_misclassified_logs(&pool, &old_token.id, old_created_at).await;

        sqlx::query(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = excluded.month_start,
                month_count = excluded.month_count
            "#,
        )
        .bind(&old_token.id)
        .bind(previous_start)
        .bind(2_i64)
        .execute(&pool)
        .await
        .expect("seed previous month quota");

        sqlx::query(
            r#"
            INSERT INTO auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at
            ) VALUES (
                ?,
                'POST',
                '/search',
                NULL,
                200,
                200,
                'tavily:search',
                'Tavily | search',
                NULL,
                'success',
                NULL,
                NULL,
                'none',
                NULL,
                1,
                5,
                ?,
                'charged',
                NULL,
                NULL,
                ?
            )
            "#,
        )
        .bind(&current_token.id)
        .bind(format!("token:{}", current_token.id))
        .bind(current_created_at)
        .execute(&pool)
        .await
        .expect("seed current charged auth log");

        sqlx::query(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = excluded.month_start,
                month_count = excluded.month_count
            "#,
        )
        .bind(&current_token.id)
        .bind(current_start)
        .bind(5_i64)
        .execute(&pool)
        .await
        .expect("seed current month quota");

        let request_candidates = load_request_log_candidates(&pool)
            .await
            .expect("load request candidates");
        let auth_candidates = load_auth_token_log_candidates(&pool)
            .await
            .expect("load auth candidates");
        assert_eq!(request_candidates.len(), 1);
        assert_eq!(auth_candidates.len(), 1);

        assert_eq!(
            apply_request_log_updates(&pool, &request_candidates)
                .await
                .expect("apply request updates"),
            1
        );
        assert_eq!(
            apply_auth_token_log_updates(&pool, &auth_candidates)
                .await
                .expect("apply auth updates"),
            1
        );

        let months = touched_months(&request_candidates, &auth_candidates);
        assert_eq!(months.len(), 1);
        assert_eq!(months[0].month_start, previous_start);
        assert!(
            rebase_touched_business_quota_months(&pool, &db_str, &months)
                .await
                .expect("rebase touched months")
                .is_some()
        );

        let old_month_row: (i64, i64) = sqlx::query_as(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(&old_token.id)
        .fetch_one(&pool)
        .await
        .expect("read previous month quota");
        assert_eq!(old_month_row, (previous_start, 0));

        let current_month_row: (i64, i64) = sqlx::query_as(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(&current_token.id)
        .fetch_one(&pool)
        .await
        .expect("read current month quota");
        assert_eq!(current_month_row, (current_start, 5));

        let _ = std::fs::remove_file(db_str);
    }

    #[test]
    fn touched_months_collects_unique_month_windows() {
        let current = Utc
            .with_ymd_and_hms(2026, 3, 30, 12, 0, 0)
            .single()
            .unwrap();
        let older = Utc
            .with_ymd_and_hms(2026, 2, 10, 12, 0, 0)
            .single()
            .unwrap();
        let months = touched_months(
            &[super::RequestLogCandidate {
                id: 1,
                created_at: current.timestamp(),
                request_kind_key: Some("legacy".to_string()),
                request_kind_label: Some("legacy".to_string()),
                request_kind_detail: Some("/mcp".to_string()),
                business_credits: Some(1),
            }],
            &[AuthTokenLogCandidate {
                id: 2,
                token_id: "tok".to_string(),
                created_at: older.timestamp(),
                request_kind_key: Some("legacy".to_string()),
                request_kind_label: Some("legacy".to_string()),
                request_kind_detail: Some("/mcp".to_string()),
                counts_business_quota: true,
                business_credits: Some(1),
                billing_state: "charged".to_string(),
                billing_subject: Some("token:tok".to_string()),
            }],
        );
        assert_eq!(
            months,
            vec![
                super::repair_month_summary(repair_month_start(older.timestamp())),
                super::repair_month_summary(repair_month_start(current.timestamp())),
            ]
        );
    }
}

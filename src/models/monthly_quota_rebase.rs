use super::*;

pub(crate) async fn rebase_current_month_business_quota_with_pool<F>(
    pool: &SqlitePool,
    now: F,
    meta_key: &str,
    update_meta: bool,
) -> Result<MonthlyQuotaRebaseReport, ProxyError>
where
    F: FnOnce() -> chrono::DateTime<Utc>,
{
    let now = now();
    let mut conn = begin_immediate_sqlite_connection_for_monthly_quota_rebase(pool).await?;
    let windows = monthly_quota_rebase_windows(now);
    let previous_rebase_month_start = get_meta_i64_executor(&mut *conn, meta_key).await?;

    let result = rebase_current_month_business_quota_locked(
        &mut conn,
        windows,
        meta_key,
        update_meta,
        previous_rebase_month_start,
    )
    .await;

    finish_monthly_quota_rebase_transaction(&mut conn, result).await
}

pub(crate) async fn maybe_rebase_current_month_business_quota_with_pool<F>(
    pool: &SqlitePool,
    now: F,
    meta_key: &str,
    update_meta: bool,
) -> Result<Option<MonthlyQuotaRebaseReport>, ProxyError>
where
    F: FnOnce() -> chrono::DateTime<Utc>,
{
    let now = now();
    let mut conn = begin_immediate_sqlite_connection_for_monthly_quota_rebase(pool).await?;
    let windows = monthly_quota_rebase_windows(now);
    let previous_rebase_month_start = get_meta_i64_executor(&mut *conn, meta_key).await?;
    if previous_rebase_month_start == Some(windows.month_window_start) {
        sqlx::query("COMMIT").execute(&mut *conn).await?;
        return Ok(None);
    }

    let result = rebase_current_month_business_quota_locked(
        &mut conn,
        windows,
        meta_key,
        update_meta,
        previous_rebase_month_start,
    )
    .await;

    finish_monthly_quota_rebase_transaction(&mut conn, result)
        .await
        .map(Some)
}

async fn rebase_current_month_business_quota_locked(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    windows: BillingLedgerWindows,
    meta_key: &str,
    update_meta: bool,
    previous_rebase_month_start: Option<i64>,
) -> Result<MonthlyQuotaRebaseReport, ProxyError> {
    ensure_charged_subjects_are_valid(
        conn.as_mut(),
        windows.month_window_start,
        windows.generated_at,
    )
    .await?;

    let (current_month_charged_rows, current_month_charged_credits) =
        fetch_current_month_charged_totals(
            conn.as_mut(),
            windows.month_window_start,
            windows.generated_at,
        )
        .await?;
    let rebased_subjects = fetch_charged_ledger_window(
        conn.as_mut(),
        windows.month_window_start,
        windows.generated_at,
    )
    .await?;

    let cleared_token_rows =
        sqlx::query("UPDATE auth_token_quota SET month_start = ?, month_count = 0")
            .bind(windows.month_window_start)
            .execute(conn.as_mut())
            .await?
            .rows_affected() as i64;
    let cleared_account_rows =
        sqlx::query("UPDATE account_monthly_quota SET month_start = ?, month_count = 0")
            .bind(windows.month_window_start)
            .execute(conn.as_mut())
            .await?
            .rows_affected() as i64;

    let mut rebased_token_subjects = 0_usize;
    let mut rebased_account_subjects = 0_usize;
    for (billing_subject, total_credits, _row_count) in rebased_subjects.iter() {
        match QuotaSubject::from_billing_subject(billing_subject)? {
            QuotaSubject::Token(token_id) => {
                sqlx::query(
                    r#"
                    INSERT INTO auth_token_quota (token_id, month_start, month_count)
                    VALUES (?, ?, ?)
                    ON CONFLICT(token_id) DO UPDATE SET
                        month_start = excluded.month_start,
                        month_count = excluded.month_count
                    "#,
                )
                .bind(&token_id)
                .bind(windows.month_window_start)
                .bind(*total_credits)
                .execute(conn.as_mut())
                .await?;
                rebased_token_subjects += 1;
            }
            QuotaSubject::Account(user_id) => {
                sqlx::query(
                    r#"
                    INSERT INTO account_monthly_quota (user_id, month_start, month_count)
                    VALUES (?, ?, ?)
                    ON CONFLICT(user_id) DO UPDATE SET
                        month_start = excluded.month_start,
                        month_count = excluded.month_count
                    "#,
                )
                .bind(&user_id)
                .bind(windows.month_window_start)
                .bind(*total_credits)
                .execute(conn.as_mut())
                .await?;
                rebased_account_subjects += 1;
            }
        }
    }
    let meta_updated =
        update_meta && previous_rebase_month_start != Some(windows.month_window_start);
    if update_meta {
        set_meta_i64_executor(conn.as_mut(), meta_key, windows.month_window_start).await?;
    }

    Ok(MonthlyQuotaRebaseReport {
        current_month_start: windows.month_window_start,
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

async fn finish_monthly_quota_rebase_transaction(
    conn: &mut sqlx::pool::PoolConnection<Sqlite>,
    result: Result<MonthlyQuotaRebaseReport, ProxyError>,
) -> Result<MonthlyQuotaRebaseReport, ProxyError> {
    let report = match result {
        Ok(report) => report,
        Err(err) => {
            let _ = sqlx::query("ROLLBACK").execute(conn.as_mut()).await;
            return Err(err);
        }
    };

    if let Err(err) = sqlx::query("COMMIT").execute(conn.as_mut()).await {
        let _ = sqlx::query("ROLLBACK").execute(conn.as_mut()).await;
        return Err(ProxyError::Database(err));
    }

    Ok(report)
}

fn monthly_quota_rebase_windows(now: chrono::DateTime<Utc>) -> BillingLedgerWindows {
    let locked_now = BackendTime::system().now_utc();
    BillingLedgerWindows::from_now(if locked_now > now { locked_now } else { now })
}

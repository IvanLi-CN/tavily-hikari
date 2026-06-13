use clap::Parser;
use serde::Serialize;
use sqlx::Row;
use std::path::PathBuf;

#[path = "support/sqlite_sidecar.rs"]
mod sqlite_sidecar;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Read-only request_logs daily growth vs request_logs_gc cleanup summary"
)]
struct Cli {
    /// SQLite database path to inspect.
    #[arg(long, env = "PROXY_DB_PATH", default_value = "data/tavily_proxy.db")]
    db_path: String,

    /// Number of local-calendar days to include, counting today.
    #[arg(long, default_value_t = 7)]
    days: i64,

    /// Emit JSON output.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyStatsRow {
    day: String,
    request_logs: i64,
    rows_with_body: i64,
    response_body_bytes: i64,
    gc_jobs: i64,
    gc_cleaned_bodies: i64,
    gc_deleted_rows: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    days: i64,
    rows: Vec<DailyStatsRow>,
    complete_days: i64,
    avg_rows_with_body_added: f64,
    avg_gc_cleaned_bodies: f64,
    gc_keeps_up_on_average: bool,
}

async fn connect_sqlite_pool(db_path: &str) -> Result<sqlx::SqlitePool, sqlx::Error> {
    sqlite_sidecar::connect_sqlite_pool(db_path, false, true, 1).await
}

async fn fetch_request_log_rows(
    pool: &sqlx::SqlitePool,
    days: i64,
) -> Result<Vec<DailyStatsRow>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        WITH RECURSIVE days(day) AS (
            SELECT date('now', 'localtime', ?)
            UNION ALL
            SELECT date(day, '+1 day')
            FROM days
            WHERE day < date('now', 'localtime')
        ),
        request_daily AS (
            SELECT
                date(datetime(created_at, 'unixepoch', 'localtime')) AS day,
                COUNT(*) AS request_logs,
                SUM(CASE WHEN request_body IS NOT NULL OR response_body IS NOT NULL THEN 1 ELSE 0 END) AS rows_with_body,
                SUM(COALESCE(response_body_bytes, LENGTH(response_body), 0)) AS response_body_bytes
            FROM request_logs
            WHERE created_at >= strftime('%s', date('now', 'localtime', ?))
            GROUP BY 1
        ),
        gc_daily AS (
            SELECT
                date(datetime(started_at, 'unixepoch', 'localtime')) AS day,
                COUNT(*) AS gc_jobs,
                SUM(
                    CAST(
                        COALESCE(
                            NULLIF(
                                substr(
                                    message,
                                    instr(message, 'cleaned_bodies=') + length('cleaned_bodies='),
                                    instr(substr(message, instr(message, 'cleaned_bodies=') + length('cleaned_bodies=')), ' ') - 1
                                ),
                                ''
                            ),
                            '0'
                        ) AS INTEGER
                    )
                ) AS gc_cleaned_bodies,
                SUM(
                    CAST(
                        COALESCE(
                            NULLIF(
                                substr(
                                    message,
                                    instr(message, 'deleted_rows=') + length('deleted_rows='),
                                    instr(substr(message, instr(message, 'deleted_rows=') + length('deleted_rows=')), ' ') - 1
                                ),
                                ''
                            ),
                            '0'
                        ) AS INTEGER
                    )
                ) AS gc_deleted_rows
            FROM scheduled_jobs
            WHERE job_type = 'request_logs_gc'
              AND started_at >= strftime('%s', date('now', 'localtime', ?))
              AND status = 'success'
              AND message LIKE '%cleaned_bodies=%'
            GROUP BY 1
        )
        SELECT
            days.day AS day,
            COALESCE(request_daily.request_logs, 0) AS request_logs,
            COALESCE(request_daily.rows_with_body, 0) AS rows_with_body,
            COALESCE(request_daily.response_body_bytes, 0) AS response_body_bytes,
            COALESCE(gc_daily.gc_jobs, 0) AS gc_jobs,
            COALESCE(gc_daily.gc_cleaned_bodies, 0) AS gc_cleaned_bodies,
            COALESCE(gc_daily.gc_deleted_rows, 0) AS gc_deleted_rows
        FROM days
        LEFT JOIN request_daily ON request_daily.day = days.day
        LEFT JOIN gc_daily ON gc_daily.day = days.day
        ORDER BY days.day ASC
        "#,
    )
    .bind(format!("-{} days", days.saturating_sub(1)))
    .bind(format!("-{} days", days.saturating_sub(1)))
    .bind(format!("-{} days", days.saturating_sub(1)))
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(DailyStatsRow {
                day: row.try_get("day")?,
                request_logs: row.try_get("request_logs")?,
                rows_with_body: row.try_get("rows_with_body")?,
                response_body_bytes: row.try_get("response_body_bytes")?,
                gc_jobs: row.try_get("gc_jobs")?,
                gc_cleaned_bodies: row.try_get("gc_cleaned_bodies")?,
                gc_deleted_rows: row.try_get("gc_deleted_rows")?,
            })
        })
        .collect()
}

fn build_summary(days: i64, rows: Vec<DailyStatsRow>) -> Summary {
    let complete_days = rows.len().saturating_sub(1) as i64;
    let complete = rows.iter().take(complete_days.max(0) as usize);
    let mut body_added_sum = 0i64;
    let mut gc_sum = 0i64;
    let mut counted = 0i64;
    for row in complete {
        body_added_sum += row.rows_with_body;
        gc_sum += row.gc_cleaned_bodies;
        counted += 1;
    }
    let avg_rows_with_body_added = if counted > 0 {
        body_added_sum as f64 / counted as f64
    } else {
        0.0
    };
    let avg_gc_cleaned_bodies = if counted > 0 {
        gc_sum as f64 / counted as f64
    } else {
        0.0
    };
    Summary {
        days,
        rows,
        complete_days: counted,
        avg_rows_with_body_added,
        avg_gc_cleaned_bodies,
        gc_keeps_up_on_average: avg_gc_cleaned_bodies >= avg_rows_with_body_added,
    }
}

fn print_plain(summary: &Summary) {
    println!(
        "day\trequest_logs\trows_with_body\tresponse_body_bytes\tgc_jobs\tgc_cleaned_bodies\tgc_deleted_rows"
    );
    for row in &summary.rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.day,
            row.request_logs,
            row.rows_with_body,
            row.response_body_bytes,
            row.gc_jobs,
            row.gc_cleaned_bodies,
            row.gc_deleted_rows
        );
    }
    println!(
        "summary\tcomplete_days={}\tavg_rows_with_body_added={:.2}\tavg_gc_cleaned_bodies={:.2}\tgc_keeps_up_on_average={}",
        summary.complete_days,
        summary.avg_rows_with_body_added,
        summary.avg_gc_cleaned_bodies,
        summary.gc_keeps_up_on_average
    );
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let db_path = PathBuf::from(&cli.db_path);
    if !db_path.exists() {
        return Err(format!("database does not exist: {}", db_path.display()).into());
    }
    let pool = connect_sqlite_pool(&cli.db_path).await?;
    let rows = fetch_request_log_rows(&pool, cli.days.max(1)).await?;
    let summary = build_summary(cli.days.max(1), rows);
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_plain(&summary);
    }
    Ok(())
}

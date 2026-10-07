use std::path::PathBuf;
use std::process::Command;

use chrono::{Days, Local, TimeZone};
use nanoid::nanoid;
use serde_json::Value;
use tavily_hikari::{DEFAULT_UPSTREAM, TavilyProxy};

#[path = "common/support_binaries.rs"]
mod support_binaries;

fn temp_db_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}-{}.db", std::process::id(), nanoid!(8)))
}

#[tokio::test]
async fn recovery_cli_reports_deferred_json_and_nonzero_status_when_service_is_active() {
    let db_path = temp_db_path("request-statistics-recovery-cli-active");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("create active service");
    let target_day = proxy.backend_time().local_now().date_naive() - Days::new(1);
    let target_day_start = Local
        .from_local_datetime(&target_day.and_hms_opt(0, 0, 0).expect("local midnight"))
        .single()
        .expect("unambiguous local midnight")
        .timestamp();

    let output = Command::new(support_binaries::resolve_support_binary(
        "REQUEST_STATISTICS_RECOVERY_TEST_BIN",
        env!("CARGO_BIN_EXE_request_statistics_recovery_once"),
    ))
    .args([
        "--db-path",
        &db_str,
        "--target-day-start",
        &target_day_start.to_string(),
        "--max-runtime-secs",
        "1",
        "--json",
    ])
    .output()
    .expect("run recovery CLI");
    assert!(
        !output.status.success(),
        "incomplete recovery must fail the CLI"
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse recovery JSON");
    assert_eq!(report["outcome"], "deferred", "report={report}");
    assert!(
        report["error"]
            .as_str()
            .expect("deferred CLI error")
            .contains("exclusive database ownership")
    );
    drop(proxy);
}

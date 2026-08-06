#!/usr/bin/env python3

"""Keep the RequestStatsPipeline boundary explicit in production hot paths."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parent.parent
HOT_PATHS = (
    ROOT / "src/store/key_store_dashboard_rollup_integrity.rs",
    ROOT / "src/store/key_store_dashboard_month_series.rs",
    ROOT / "src/store/key_store_request_logs_summary_windows.rs",
    ROOT / "src/store/key_store_request_stats_flush_and_public_metrics.rs",
)
REQUEST_LOG_PATH = ROOT / "src/store/key_store_request_log_body_retention.rs"
AUTH_TOKEN_PATH = ROOT / "src/store/key_store_users_and_oauth.rs"
ACCOUNT_ROLLUP_PATH = ROOT / "src/store/key_store_account_usage_rollups.rs"
RANKINGS_PATH = ROOT / "src/store/key_store_user_rankings.rs"
SUMMARY_PATH = ROOT / "src/store/key_store_request_logs_and_dashboard.rs"
PIPELINE_PATH = ROOT / "src/store/key_store_request_stats_pipeline.rs"
RUNTIME_PATH = ROOT / "src/store/sqlite_runtime.rs"

FORBIDDEN_HOT_PATH_TOKENS = (
    "RequestStatsCoalescer",
    "request_stats_coalescer",
    "with_primary_pool",
    "with_read_flush_pool",
    "compatibility_primary_pool",
    "compatibility_read_flush_pool",
    "sqlx::Transaction",
    "sqlx::pool::PoolConnection",
    "self.pool",
)
RAW_EXECUTION_SUFFIXES = (
    ".fetch_one(&self.pool)",
    ".fetch_optional(&self.pool)",
    ".fetch_all(&self.pool)",
    ".execute(&self.pool)",
)


def fail(message: str) -> None:
    print(f"request-stats architecture check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def read(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError as exc:
        fail(f"cannot read {path.relative_to(ROOT)}: {exc}")


def function_body(source: str, signature: str) -> str:
    start = source.find(signature)
    if start < 0:
        fail(f"missing function signature {signature!r}")
    next_function = len(source)
    for marker in (
        "\n    pub(crate) async fn ",
        "\n    pub(crate) fn ",
        "\n    pub async fn ",
        "\n    pub fn ",
        "\n    async fn ",
        "\n    fn ",
    ):
        candidate = source.find(marker, start + len(signature))
        if candidate >= 0:
            next_function = min(next_function, candidate)
    return source[start:next_function]


def check_hot_paths() -> None:
    for path in HOT_PATHS:
        source = read(path)
        for token in FORBIDDEN_HOT_PATH_TOKENS:
            if token in source:
                fail(f"{path.relative_to(ROOT)} contains forbidden token {token!r}")


def check_ingestion_boundaries() -> None:
    request_log = function_body(read(REQUEST_LOG_PATH), "pub(crate) async fn log_attempt")
    if "request_stats_primary_fetch_scalar_one!" not in request_log:
        fail("log_attempt does not insert through RequestStatsPipeline")
    if any(suffix in request_log for suffix in RAW_EXECUTION_SUFFIXES):
        fail("log_attempt still executes request-log SQL through the raw KeyStore pool")

    auth_token = function_body(read(AUTH_TOKEN_PATH), "pub(crate) async fn insert_token_log(")
    if "request_stats_primary_execute!" not in auth_token:
        fail("insert_token_log does not insert through RequestStatsPipeline")
    if any(suffix in auth_token for suffix in RAW_EXECUTION_SUFFIXES):
        fail("insert_token_log still executes auth-token SQL through the raw KeyStore pool")

    pending_billing = function_body(
        read(AUTH_TOKEN_PATH), "async fn insert_token_log_pending_billing_once("
    )
    if "begin_primary_transaction" not in pending_billing:
        fail("pending-billing auth-token insert does not begin through RequestStatsPipeline")
    if "self.pool.begin()" in pending_billing:
        fail("pending-billing auth-token insert still begins a raw KeyStore transaction")


def check_auth_log_helper_boundaries() -> None:
    auth_token = read(AUTH_TOKEN_PATH)
    for signature in (
        "async fn resolve_request_log_diagnostic_metadata(",
        "async fn resolve_token_log_request_kind(",
    ):
        body = function_body(auth_token, signature)
        if "request_stats_primary_fetch_optional!" not in body:
            fail(f"{signature} does not read through RequestStatsPipeline")
        if any(suffix in body for suffix in RAW_EXECUTION_SUFFIXES):
            fail(f"{signature} still reads through the raw KeyStore pool")

    token_keys = read(ROOT / "src/store/key_store_keys.rs")
    token_binding = function_body(token_keys, "pub(crate) async fn find_user_id_by_token_fresh(")
    if "request_stats_primary_fetch_optional!" not in token_binding:
        fail("find_user_id_by_token_fresh does not read through RequestStatsPipeline")
    if any(suffix in token_binding for suffix in RAW_EXECUTION_SUFFIXES):
        fail("find_user_id_by_token_fresh still reads through the raw KeyStore pool")


def check_read_boundaries() -> None:
    account_rollup = function_body(
        read(ACCOUNT_ROLLUP_PATH), "pub(crate) async fn fetch_account_usage_rollup_values("
    )
    if "request_stats_primary_fetch_all!" not in account_rollup:
        fail("account usage rollup values do not read through RequestStatsPipeline")
    if any(suffix in account_rollup for suffix in RAW_EXECUTION_SUFFIXES):
        fail("account usage rollup values still read through the raw KeyStore pool")

    rankings = read(RANKINGS_PATH)
    for signature in (
        "async fn fetch_user_unique_ip_ranking_rows(",
        "async fn fetch_user_ranking_rollup_totals(",
        "async fn apply_user_ranking_partial_range(",
        "async fn fetch_user_ranking_identities(",
    ):
        body = function_body(rankings, signature)
        if "request_stats_primary_fetch_all!" not in body:
            fail(f"{signature} does not read through RequestStatsPipeline")
        if any(suffix in body for suffix in RAW_EXECUTION_SUFFIXES):
            fail(f"{signature} still reads through the raw KeyStore pool")

    summary = read(SUMMARY_PATH)
    for signature in (
        "pub(crate) async fn fetch_summary_without_flush_tx(",
        "pub(crate) async fn fetch_summary_without_flush(",
        "pub(crate) async fn fetch_success_breakdown_from_dashboard_rollups(",
    ):
        body = function_body(summary, signature)
        if "sqlx::Transaction" in body or "self.pool.begin()" in body:
            fail(f"{signature} still owns a raw request-stats transaction")
        if any(suffix in body for suffix in RAW_EXECUTION_SUFFIXES):
            fail(f"{signature} still executes request-stat SQL through the raw KeyStore pool")


def check_bounded_backfill() -> None:
    pipeline = read(PIPELINE_PATH)
    dashboard = read(ROOT / "src/store/key_store_dashboard_rollup_integrity.rs")
    runtime = read(RUNTIME_PATH)
    if "pub(crate) const BACKFILL_PAGE_ROWS: i64 = 500;" not in pipeline:
        fail("RequestStatsPipeline does not define the bounded backfill page size")
    if "pub(crate) const MAX_PENDING_KEYS: usize = 100;" not in pipeline:
        fail("RequestStatsPipeline does not define a pending rollup capacity")
    if "reserve_pending_capacity" not in pipeline:
        fail("RequestStatsPipeline does not apply pending rollup backpressure")
    if "created_at > ? OR (created_at = ? AND id > ?)" not in dashboard:
        fail("dashboard backfill is missing the stable created_at/id cursor")
    if "LIMIT ?" not in dashboard:
        fail("dashboard backfill is missing its bound")
    if "emit_perf_sample_log(" not in dashboard or "page_size:" not in dashboard:
        fail("dashboard backfill does not emit per-page performance evidence")
    if "primary_pool: SqlitePool" not in runtime or "read_flush_pool: SqlitePool" not in runtime:
        fail("SqliteRuntime no longer owns both production pools")


def main() -> int:
    check_hot_paths()
    check_ingestion_boundaries()
    check_auth_log_helper_boundaries()
    check_read_boundaries()
    check_bounded_backfill()
    print("request-stats architecture check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

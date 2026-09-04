use tempfile::{TempDir, tempdir};

const DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID: &str = "dashboard-quota-recovery-key";

async fn dashboard_quota_recovery_store() -> (TempDir, std::sync::Arc<KeyStore>, SummaryWindowBounds)
{
    let temp_dir = tempdir().expect("create temp directory");
    let db_path = temp_dir.path().join("dashboard-quota-recovery.db");
    let store = std::sync::Arc::new(
        KeyStore::new_with_time(&db_path.to_string_lossy(), BackendTime::system())
            .await
            .expect("create key store"),
    );
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at, status_changed_at)
        VALUES (?, ?, 'active', 0, 0)
        "#,
    )
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind("tvly-dashboard-quota-recovery-key")
    .execute(&store.pool)
    .await
    .expect("insert quota recovery key");

    let bounds = SummaryWindowBounds {
        today_start: 800,
        today_end: 1_000,
        today_period_end: 1_000,
        yesterday_start: 500,
        yesterday_end: 800,
        month_start: 0,
        month_quota_charge_start: 0,
        month_period_end: 1_000,
        previous_month_start: -1_000,
        previous_month_end: 0,
    };
    (temp_dir, store, bounds)
}

async fn insert_dashboard_quota_recovery_sample(
    store: &KeyStore,
    captured_at: i64,
    quota_remaining: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (
            key_id, quota_limit, quota_remaining, captured_at, source
        ) VALUES (?, 1000, ?, ?, 'test')
        "#,
    )
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(quota_remaining)
    .bind(captured_at)
    .execute(&store.pool)
    .await
    .expect("insert quota sample");
}

async fn seed_dashboard_quota_recovery_samples(store: &KeyStore, sample_count: i64) {
    insert_dashboard_quota_recovery_sample(store, 100, 1_000).await;
    for offset in 0..sample_count {
        insert_dashboard_quota_recovery_sample(store, 500 + offset, 999 - offset).await;
    }
}

#[tokio::test]
async fn dashboard_quota_recovery_discards_a_multi_page_draft_on_native_deadline() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    store
        .sqlite_runtime
        .force_cooperative_query_deadline_after_reads_for_test(2);

    let result = store
        .fetch_dashboard_quota_charge_read_model(bounds, 0)
        .await;

    assert!(
        matches!(
            result,
            Err(ProxyError::Deferred { operation, ref reason })
                if operation == "admin_alerts_read" && reason == "read_budget"
        ),
        "recovery must discard its draft when a bounded source page reaches the native deadline: {result:?}"
    );

    let restarted = store
        .fetch_dashboard_quota_charge_read_model(bounds, 0)
        .await
        .expect("a later recovery starts from a clean bounded stage");
    assert_eq!(restarted.watermark.source_id, 66);
    assert_eq!(restarted.snapshot.month.upstream_actual_credits, 65);
}

#[tokio::test]
async fn dashboard_quota_source_probe_defers_after_future_page_budget() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    insert_dashboard_quota_recovery_sample(&store, 500, 999).await;
    for offset in 0..(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE * DASHBOARD_QUOTA_SOURCE_MAX_PAGES as i64) {
        insert_dashboard_quota_recovery_sample(&store, 1_000 + offset, 998 - offset).await;
    }

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        store.fetch_dashboard_quota_sample_watermark(bounds.today_end),
    )
    .await
    .expect("source probe must have a whole-operation page budget");

    assert!(
        matches!(
            result,
            Err(ProxyError::Deferred { operation, ref reason })
                if operation == "admin_alerts_read" && reason == "read_budget"
        ),
        "future samples spanning multiple keyset pages must defer instead of extending the probe: {result:?}"
    );
}

#[tokio::test]
async fn dashboard_quota_recovery_paginates_an_exact_multi_page_window() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    for offset in 0..33 {
        insert_dashboard_quota_recovery_sample(&store, 1_000 + offset, 934 - offset).await;
    }

    let model = store
        .fetch_dashboard_quota_charge_read_model(bounds, 3)
        .await
        .expect("recover every bounded quota page");

    assert_eq!(model.watermark.source_id, 66);
    assert_eq!(model.snapshot.month.upstream_actual_credits, 65);
    assert_eq!(model.snapshot.month.latest_sync_at, Some(564));
    assert_eq!(model.snapshot.month.sampled_key_count, 1);
    assert_eq!(model.snapshot.month.stale_key_count, 3);
}

#[tokio::test]
async fn dashboard_quota_recovery_defers_when_source_changes_on_every_staged_page() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 1).await;
    let mut next_pause = Some(store.install_dashboard_overview_read_pause().await);
    let recovery_store = std::sync::Arc::clone(&store);
    let recovery = tokio::spawn(async move {
        recovery_store
            .fetch_dashboard_quota_charge_read_model(bounds, 0)
            .await
    });

    for attempt in 0..DASHBOARD_QUOTA_RECOVERY_MAX_ATTEMPTS {
        let pause = next_pause
            .take()
            .expect("each restart stages a page before its source check");
        tokio::time::timeout(Duration::from_secs(2), pause.wait_until_arrived())
            .await
            .expect("recovery staged a page");
        insert_dashboard_quota_recovery_sample(&store, 600 + attempt as i64, 998 - attempt as i64)
            .await;
        pause.release();
        if attempt + 1 < DASHBOARD_QUOTA_RECOVERY_MAX_ATTEMPTS {
            next_pause = Some(store.install_dashboard_overview_read_pause().await);
        }
    }

    let result = tokio::time::timeout(Duration::from_secs(2), recovery)
        .await
        .expect("recovery must stop after a bounded number of source changes")
        .expect("join quota recovery");
    assert!(
        matches!(
            result,
            Err(ProxyError::Deferred { operation, ref reason })
                if operation == "admin_alerts_read" && reason == "read_budget"
        ),
        "recovery must defer rather than restart forever when every staged page changes source: {result:?}"
    );
}

#[tokio::test]
async fn dashboard_quota_recovery_restarts_after_an_out_of_order_backfill() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    let pause = store.install_dashboard_overview_read_pause().await;
    let recovery_store = std::sync::Arc::clone(&store);
    let mut recovery = tokio::spawn(async move {
        recovery_store
            .fetch_dashboard_quota_charge_read_model(bounds, 0)
            .await
    });

    tokio::time::timeout(Duration::from_secs(2), pause.wait_until_arrived())
        .await
        .expect("recovery staged its first page");
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut recovery)
            .await
            .is_err(),
        "a staged model must not finish before its source revision is rechecked"
    );
    insert_dashboard_quota_recovery_sample(&store, 520, 978).await;
    pause.release();

    let model = tokio::time::timeout(Duration::from_secs(2), recovery)
        .await
        .expect("recovery restarts after the source changes")
        .expect("join quota recovery")
        .expect("rebuild from the new source watermark");
    let watermark = store
        .fetch_dashboard_quota_sample_watermark(bounds.today_end)
        .await
        .expect("read final source watermark");
    assert_eq!(model.watermark, watermark);
    assert_eq!(model.snapshot.month.upstream_actual_credits, 65);
}

#[tokio::test]
async fn dashboard_quota_recovery_cancellation_leaves_no_partial_stage_to_reuse() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    let pause = store.install_dashboard_overview_read_pause().await;
    let recovery_store = std::sync::Arc::clone(&store);
    let recovery = tokio::spawn(async move {
        recovery_store
            .fetch_dashboard_quota_charge_read_model(bounds, 0)
            .await
    });

    tokio::time::timeout(Duration::from_secs(2), pause.wait_until_arrived())
        .await
        .expect("recovery staged its first page before cancellation");
    recovery.abort();
    assert!(recovery.await.is_err(), "recovery task was cancelled");
    pause.release();

    let restarted = store
        .fetch_dashboard_quota_charge_read_model(bounds, 0)
        .await
        .expect("a new recovery starts from an empty stage after cancellation");
    assert_eq!(restarted.watermark.source_id, 66);
    assert_eq!(restarted.snapshot.month.upstream_actual_credits, 65);
}

#[tokio::test]
async fn dashboard_quota_recovery_queries_use_existing_indexes() {
    let (_temp_dir, store, _bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 1).await;

    let window_plan = explain_dashboard_quota_recovery_window_query(&store).await;
    assert_dashboard_quota_recovery_index_plan(
        &window_plan,
        "idx_api_key_quota_sync_samples_captured",
        "window keyset",
    );

    let window_cursor_plan = explain_dashboard_quota_recovery_window_cursor_query(&store).await;
    assert_dashboard_quota_recovery_index_plan(
        &window_cursor_plan,
        "idx_api_key_quota_sync_samples_captured",
        "window keyset cursor",
    );

    let baseline_timestamp_plan =
        explain_dashboard_quota_recovery_baseline_timestamp_query(&store).await;
    assert_dashboard_quota_recovery_index_plan(
        &baseline_timestamp_plan,
        "idx_api_key_quota_sync_samples_key_captured",
        "baseline timestamp",
    );

    let baseline_page_plan = explain_dashboard_quota_recovery_baseline_page_query(&store).await;
    assert_dashboard_quota_recovery_index_plan(
        &baseline_page_plan,
        "idx_api_key_quota_sync_samples_key_captured",
        "baseline page",
    );

    let source_id_plan = explain_dashboard_quota_recovery_source_id_query(&store).await;
    assert!(
        source_id_plan.contains("INTEGER PRIMARY KEY"),
        "source revision keyset missed rowid access: {source_id_plan}"
    );
    assert!(
        !source_id_plan.contains("SCAN api_key_quota_sync_samples"),
        "source revision keyset scanned quota samples: {source_id_plan}"
    );

    let source_timestamp_plan =
        explain_dashboard_quota_recovery_source_timestamp_query(&store).await;
    assert_dashboard_quota_recovery_index_plan(
        &source_timestamp_plan,
        "idx_api_key_quota_sync_samples_captured",
        "source timestamp",
    );
}

async fn explain_dashboard_quota_recovery_window_query(store: &KeyStore) -> String {
    sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, key_id, quota_remaining, captured_at
        FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
        WHERE captured_at >= ? AND captured_at < ? AND id <= ?
        ORDER BY captured_at DESC, key_id ASC, id ASC
        LIMIT ?
        "#,
    )
    .bind(500_i64)
    .bind(1_000_i64)
    .bind(2_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery window query")
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join("\n")
}

async fn explain_dashboard_quota_recovery_window_cursor_query(store: &KeyStore) -> String {
    sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, key_id, quota_remaining, captured_at
        FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
        WHERE captured_at >= ?
          AND captured_at < ?
          AND id <= ?
          AND (
            captured_at < ?
            OR (
                captured_at = ?
                AND (key_id > ? OR (key_id = ? AND id > ?))
            )
          )
        ORDER BY captured_at DESC, key_id ASC, id ASC
        LIMIT ?
        "#,
    )
    .bind(500_i64)
    .bind(1_000_i64)
    .bind(2_i64)
    .bind(550_i64)
    .bind(550_i64)
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(1_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery window cursor query")
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join("\n")
}

async fn explain_dashboard_quota_recovery_baseline_timestamp_query(store: &KeyStore) -> String {
    sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT captured_at
        FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_key_captured
        WHERE key_id = ? AND captured_at < ? AND id <= ?
        ORDER BY captured_at DESC
        LIMIT ?
        "#,
    )
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(500_i64)
    .bind(2_i64)
    .bind(1_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery baseline timestamp query")
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join("\n")
}

async fn explain_dashboard_quota_recovery_baseline_page_query(store: &KeyStore) -> String {
    sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, key_id, quota_remaining, captured_at
        FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_key_captured
        WHERE key_id = ? AND captured_at = ? AND id > ? AND id <= ?
        ORDER BY id ASC
        LIMIT ?
        "#,
    )
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(100_i64)
    .bind(0_i64)
    .bind(1_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery baseline page query")
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join("\n")
}

async fn explain_dashboard_quota_recovery_source_id_query(store: &KeyStore) -> String {
    sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, captured_at
        FROM api_key_quota_sync_samples
        WHERE id > 0
        ORDER BY id DESC
        LIMIT ?
        "#,
    )
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery source id query")
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join("\n")
}

async fn explain_dashboard_quota_recovery_source_timestamp_query(store: &KeyStore) -> String {
    sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT captured_at
        FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
        WHERE captured_at < ?
        ORDER BY captured_at DESC, key_id ASC, id ASC
        LIMIT ?
        "#,
    )
    .bind(1_000_i64)
    .bind(1_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery source timestamp query")
    .iter()
    .map(|row| row.get::<String, _>("detail"))
    .collect::<Vec<_>>()
    .join("\n")
}

fn assert_dashboard_quota_recovery_index_plan(plan: &str, index: &str, label: &str) {
    assert!(plan.contains(index), "{label} missed {index}: {plan}");
    assert!(
        !plan.contains("SCAN api_key_quota_sync_samples"),
        "{label} scanned quota samples: {plan}"
    );
    assert!(
        !plan.contains("USE TEMP B-TREE"),
        "{label} sorted outside its keyset index: {plan}"
    );
}

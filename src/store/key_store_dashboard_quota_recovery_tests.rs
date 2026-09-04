use tempfile::{TempDir, tempdir};

const DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID: &str = "dashboard-quota-recovery-key";

async fn dashboard_quota_recovery_store() -> (TempDir, std::sync::Arc<KeyStore>, SummaryWindowBounds) {
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
        insert_dashboard_quota_recovery_sample(store, 500, 999 - offset).await;
    }
}

#[tokio::test]
async fn dashboard_quota_recovery_paginates_exact_keyset_model() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    for offset in 0..33 {
        insert_dashboard_quota_recovery_sample(&store, 1_000 + offset, 934 - offset).await;
    }

    let model = store
        .fetch_dashboard_quota_charge_read_model(bounds, 3)
        .await
        .expect("recover all quota pages");

    assert_eq!(model.watermark.source_id, 66);
    assert_eq!(model.watermark.source_count, 66);
    assert_eq!(
        model.watermark,
        store
            .fetch_dashboard_quota_sample_watermark(bounds.today_end)
            .await
            .expect("read normal quota watermark"),
        "the staged recovery watermark must retain the normal probe's exact source generation"
    );
    assert_eq!(model.snapshot.month.upstream_actual_credits, 65);
    assert_eq!(model.snapshot.month.latest_sync_at, Some(500));
    assert_eq!(model.snapshot.month.sampled_key_count, 1);
    assert_eq!(model.snapshot.month.stale_key_count, 3);
}

#[tokio::test]
async fn dashboard_quota_recovery_discards_staged_model_on_source_change() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    let pause = store.install_dashboard_overview_read_pause().await;
    let recovery_store = std::sync::Arc::clone(&store);
    let mut recovery = tokio::spawn(async move {
        recovery_store
            .fetch_dashboard_quota_charge_read_model(bounds, 0)
            .await
    });

    pause.wait_until_arrived().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut recovery)
            .await
            .is_err(),
        "a staged model must not complete before its source generation is rechecked"
    );
    insert_dashboard_quota_recovery_sample(&store, 565, 934).await;
    pause.release();

    let model = recovery
        .await
        .expect("join quota recovery")
        .expect("restart recovery from the changed source generation");
    let final_watermark = store
        .fetch_dashboard_quota_sample_watermark(bounds.today_end)
        .await
        .expect("read final source watermark");
    assert_eq!(model.watermark, final_watermark);
    assert_eq!(model.snapshot.month.upstream_actual_credits, 66);
}

#[tokio::test]
async fn dashboard_quota_recovery_cancellation_discards_private_model() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    let pause = store.install_dashboard_overview_read_pause().await;
    let recovery_store = std::sync::Arc::clone(&store);
    let recovery = tokio::spawn(async move {
        recovery_store
            .fetch_dashboard_quota_charge_read_model(bounds, 0)
            .await
    });

    pause.wait_until_arrived().await;
    recovery.abort();
    assert!(
        recovery
            .await
            .expect_err("cancelled recovery must not return a staged model")
            .is_cancelled(),
        "the paused recovery task should be cancelled"
    );
    pause.release();

    let restarted = store
        .fetch_dashboard_quota_charge_read_model(bounds, 0)
        .await
        .expect("a later recovery must start from an empty private model");
    assert_eq!(restarted.watermark.source_count, 66);
    assert_eq!(restarted.snapshot.month.upstream_actual_credits, 65);
}

#[tokio::test]
async fn dashboard_quota_recovery_restarts_after_native_page_deadline() {
    let (_temp_dir, store, bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 65).await;
    store
        .sqlite_runtime
        .force_cooperative_query_deadline_after_reads_for_test(2);

    let interrupted = store.fetch_dashboard_quota_charge_read_model(bounds, 0).await;
    assert!(
        matches!(
            interrupted,
            Err(ProxyError::Deferred { operation, ref reason })
                if operation == "admin_alerts_read" && reason == "read_budget"
        ),
        "the recovery page must preserve the native 250ms deadline: {interrupted:?}"
    );

    let restarted = store
        .fetch_dashboard_quota_charge_read_model(bounds, 0)
        .await
        .expect("a later recovery starts from a clean bounded session");
    assert_eq!(restarted.watermark.source_id, 66);
    assert_eq!(restarted.snapshot.month.upstream_actual_credits, 65);
}

#[tokio::test]
async fn dashboard_quota_recovery_queries_use_existing_indexes() {
    let (_temp_dir, store, _bounds) = dashboard_quota_recovery_store().await;
    seed_dashboard_quota_recovery_samples(&store, 1).await;

    let window_rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT id, key_id, quota_remaining, captured_at
           FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
           WHERE captured_at >= ? AND captured_at < ? AND id <= ?
           ORDER BY captured_at DESC, key_id ASC, id ASC
           LIMIT ?"#,
    )
    .bind(500_i64)
    .bind(1_000_i64)
    .bind(2_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery window query");
    let window_plan = window_rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        window_plan.contains("idx_api_key_quota_sync_samples_captured"),
        "window query missed captured index: {window_plan}"
    );
    assert!(
        !window_plan.contains("SCAN api_key_quota_sync_samples"),
        "window query scanned quota samples: {window_plan}"
    );
    assert!(
        !window_plan.contains("USE TEMP B-TREE"),
        "window query sorted beyond its keyset page: {window_plan}"
    );

    let cursor_window_rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT id, key_id, quota_remaining, captured_at
           FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
           WHERE captured_at >= ? AND captured_at < ? AND id <= ?
             AND captured_at <= ?
             AND (
                 captured_at < ?
                 OR (
                     captured_at = ?
                     AND (key_id > ? OR (key_id = ? AND id > ?))
                 )
             )
           ORDER BY captured_at DESC, key_id ASC, id ASC
           LIMIT ?"#,
    )
    .bind(500_i64)
    .bind(1_000_i64)
    .bind(2_i64)
    .bind(500_i64)
    .bind(500_i64)
    .bind(500_i64)
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(1_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain cursor quota recovery window query");
    let cursor_window_plan = cursor_window_rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        cursor_window_plan.contains("idx_api_key_quota_sync_samples_captured"),
        "cursor window missed captured index: {cursor_window_plan}"
    );
    assert!(
        !cursor_window_plan.contains("SCAN api_key_quota_sync_samples")
            && !cursor_window_plan.contains("USE TEMP B-TREE"),
        "cursor window escaped its keyset: {cursor_window_plan}"
    );

    let baseline_capture_rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT captured_at
           FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_key_captured
           WHERE key_id = ? AND captured_at < ? AND id <= ?
           ORDER BY captured_at DESC
           LIMIT ?"#,
    )
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(500_i64)
    .bind(2_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery baseline capture query");
    let baseline_capture_plan = baseline_capture_rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        baseline_capture_plan.contains("idx_api_key_quota_sync_samples_key_captured"),
        "baseline capture missed key/captured index: {baseline_capture_plan}"
    );
    assert!(
        !baseline_capture_plan.contains("SCAN api_key_quota_sync_samples")
            && !baseline_capture_plan.contains("USE TEMP B-TREE"),
        "baseline capture escaped its keyset: {baseline_capture_plan}"
    );

    let baseline_id_rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT id, key_id, quota_remaining, captured_at
           FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_key_captured
           WHERE key_id = ? AND captured_at = ? AND id > ? AND id <= ?
           ORDER BY id ASC
           LIMIT ?"#,
    )
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(100_i64)
    .bind(0_i64)
    .bind(2_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery baseline id query");
    let baseline_id_plan = baseline_id_rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        baseline_id_plan.contains("idx_api_key_quota_sync_samples_key_captured"),
        "baseline id page missed key/captured index: {baseline_id_plan}"
    );
    assert!(
        !baseline_id_plan.contains("SCAN api_key_quota_sync_samples")
            && !baseline_id_plan.contains("USE TEMP B-TREE"),
        "baseline id page escaped its keyset: {baseline_id_plan}"
    );

    let source_rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT id, key_id, captured_at
           FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
           WHERE captured_at < ?
           ORDER BY captured_at DESC, key_id ASC, id ASC
           LIMIT ?"#,
    )
    .bind(1_000_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain quota recovery source query");
    let source_plan = source_rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        source_plan.contains("idx_api_key_quota_sync_samples_captured"),
        "source query missed captured index: {source_plan}"
    );
    assert!(
        !source_plan.contains("SCAN api_key_quota_sync_samples")
            && !source_plan.contains("USE TEMP B-TREE"),
        "source query escaped its keyset: {source_plan}"
    );

    let source_cursor_rows = sqlx::query(
        r#"EXPLAIN QUERY PLAN
           SELECT id, key_id, captured_at
           FROM api_key_quota_sync_samples INDEXED BY idx_api_key_quota_sync_samples_captured
           WHERE captured_at < ?
             AND captured_at <= ?
             AND (
                 captured_at < ?
                 OR (
                     captured_at = ?
                     AND (key_id > ? OR (key_id = ? AND id > ?))
                 )
             )
           ORDER BY captured_at DESC, key_id ASC, id ASC
           LIMIT ?"#,
    )
    .bind(1_000_i64)
    .bind(500_i64)
    .bind(500_i64)
    .bind(500_i64)
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(DASHBOARD_QUOTA_RECOVERY_TEST_KEY_ID)
    .bind(1_i64)
    .bind(32_i64)
    .fetch_all(&store.pool)
    .await
    .expect("explain cursor quota recovery source query");
    let source_cursor_plan = source_cursor_rows
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        source_cursor_plan.contains("idx_api_key_quota_sync_samples_captured"),
        "cursor source query missed captured index: {source_cursor_plan}"
    );
    assert!(
        !source_cursor_plan.contains("SCAN api_key_quota_sync_samples")
            && !source_cursor_plan.contains("USE TEMP B-TREE"),
        "cursor source query escaped its keyset: {source_cursor_plan}"
    );
}

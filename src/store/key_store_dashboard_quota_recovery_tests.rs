use std::time::Duration;

use tempfile::{TempDir, tempdir};

const DASHBOARD_QUOTA_SOURCE_PROBE_TEST_KEY_ID: &str = "dashboard-quota-source-probe-key";

async fn dashboard_quota_source_probe_store() -> (TempDir, std::sync::Arc<KeyStore>) {
    let temp_dir = tempdir().expect("create temp directory");
    let db_path = temp_dir.path().join("dashboard-quota-source-probe.db");
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
    .bind(DASHBOARD_QUOTA_SOURCE_PROBE_TEST_KEY_ID)
    .bind("tvly-dashboard-quota-source-probe-key")
    .execute(&store.pool)
    .await
    .expect("insert quota source probe key");
    (temp_dir, store)
}

async fn insert_dashboard_quota_source_probe_sample(
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
    .bind(DASHBOARD_QUOTA_SOURCE_PROBE_TEST_KEY_ID)
    .bind(quota_remaining)
    .bind(captured_at)
    .execute(&store.pool)
    .await
    .expect("insert quota source probe sample");
}

#[tokio::test]
async fn dashboard_quota_source_probe_walks_a_bounded_keyset_page() {
    let (_temp_dir, store) = dashboard_quota_source_probe_store().await;
    insert_dashboard_quota_source_probe_sample(&store, 500, 999).await;
    for offset in 0..DASHBOARD_QUOTA_SOURCE_PAGE_SIZE {
        insert_dashboard_quota_source_probe_sample(&store, 1_000 + offset, 998 - offset).await;
    }

    let watermark = store
        .fetch_dashboard_quota_sample_watermark(1_000)
        .await
        .expect("the second staged page finds the active source sample");

    assert_eq!(watermark.source_id, 1);
    assert_eq!(watermark.source_captured_at, 500);
    assert_eq!(watermark.source_count, 1);
}

#[tokio::test]
async fn dashboard_quota_source_probe_uses_the_source_id_as_its_revision() {
    let (_temp_dir, store) = dashboard_quota_source_probe_store().await;
    insert_dashboard_quota_source_probe_sample(&store, 500, 999).await;
    insert_dashboard_quota_source_probe_sample(&store, 1_000, 998).await;
    insert_dashboard_quota_source_probe_sample(&store, 600, 997).await;

    let watermark = store
        .fetch_dashboard_quota_sample_watermark(1_000)
        .await
        .expect("source probe finds the newest active source id");

    assert_eq!(watermark.source_id, 3);
    assert_eq!(watermark.source_captured_at, 600);
    assert_eq!(watermark.source_count, 3);

    let model = DashboardQuotaChargeReadModel::from_samples(
        SummaryWindowBounds {
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
        },
        0,
        DashboardQuotaSampleWatermark {
            source_id: 1,
            source_captured_at: 500,
            source_count: 1,
        },
        Vec::new(),
    );
    assert!(
        !model.can_hydrate(
            watermark,
            &[DashboardQuotaSample {
                id: 3,
                key_id: DASHBOARD_QUOTA_SOURCE_PROBE_TEST_KEY_ID.to_string(),
                quota_remaining: 997,
                captured_at: 600,
                previous_quota_remaining: Some(999),
            }],
        ),
        "a future-id gap must defer to the existing full recovery path"
    );
}

#[tokio::test]
async fn dashboard_quota_source_probe_defers_after_its_page_budget() {
    let (_temp_dir, store) = dashboard_quota_source_probe_store().await;
    insert_dashboard_quota_source_probe_sample(&store, 500, 999).await;
    for offset in 0..(DASHBOARD_QUOTA_SOURCE_PAGE_SIZE
        * i64::try_from(DASHBOARD_QUOTA_SOURCE_MAX_PAGES).expect("page budget fits i64"))
    {
        insert_dashboard_quota_source_probe_sample(&store, 1_000 + offset, 998 - offset).await;
    }

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        store.fetch_dashboard_quota_sample_watermark(1_000),
    )
    .await
    .expect("source probe stops at its staged read budget");

    assert!(
        matches!(
            result,
            Err(ProxyError::Deferred { operation, ref reason })
                if operation == "admin_alerts_read" && reason == "read_budget"
        ),
        "source samples beyond the page budget must preserve the typed defer: {result:?}"
    );
}

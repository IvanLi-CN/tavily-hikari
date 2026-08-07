use super::*;

#[tokio::test]
async fn summary_windows_page_quota_samples_without_losing_cross_page_delta() {
    let db_path = temp_db_path("summary-windows-quota-charge-pagination");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-summary-window-quota-charge-pagination".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let key_id = proxy
        .list_api_key_metrics()
        .await
        .expect("list key metrics")
        .into_iter()
        .next()
        .expect("seeded key")
        .id;
    let fallback_now = Local::now();
    let now_naive = fallback_now
        .date_naive()
        .and_hms_opt(12, 0, 0)
        .expect("valid midday");
    let now = match Local.from_local_datetime(&now_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
        chrono::LocalResult::None => fallback_now,
    };
    let today_start = start_of_local_day_utc_ts(now);
    let today_end = now.with_timezone(&Utc).timestamp().saturating_add(1);

    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (
            key_id, quota_limit, quota_remaining, captured_at, source
        ) VALUES (?, 1000, 1000, ?, 'quota_sync/pagination')
        "#,
    )
    .bind(&key_id)
    .bind(today_start - 60)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert quota sample baseline");

    for index in 0..501_i64 {
        sqlx::query(
            r#"
            INSERT INTO api_key_quota_sync_samples (
                key_id, quota_limit, quota_remaining, captured_at, source
            ) VALUES (?, 1000, ?, ?, 'quota_sync/pagination')
            "#,
        )
        .bind(&key_id)
        .bind(999 - index)
        .bind(today_start + 60 + index)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert paged quota sample");
    }

    let summary = proxy
        .summary_windows_at(now)
        .await
        .expect("summary windows");

    assert_eq!(
        summary.today.quota_charge.upstream_actual_credits, 501,
        "all paged samples should contribute their quota deltas"
    );
    assert_eq!(
        summary.today.quota_charge.latest_sync_at,
        Some(today_start + 560)
    );
    assert!(today_end > today_start + 560);

    let _ = std::fs::remove_file(db_path);
}

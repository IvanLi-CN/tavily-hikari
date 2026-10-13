use super::*;
use chrono::{Datelike, FixedOffset, Local, TimeZone};

const PUBLIC_BLOG_RUNTIME_TZ_CHILD_ENV: &str = "TAVILY_HIKARI_BLOG_RUNTIME_TZ_CHILD";

#[tokio::test]
async fn public_blog_runtime_reads_rollups_and_historical_eligible_quotas() {
    if std::env::var_os(PUBLIC_BLOG_RUNTIME_TZ_CHILD_ENV).is_none() {
        let child = std::process::Command::new(
            std::env::current_exe().expect("test executable path"),
        )
        .args([
            "--exact",
            "tests::blog_runtime::public_blog_runtime_reads_rollups_and_historical_eligible_quotas",
        ])
        .env("TZ", "America/Los_Angeles")
        .env(PUBLIC_BLOG_RUNTIME_TZ_CHILD_ENV, "1")
        .output()
        .expect("run persisted fixture with a non-Shanghai host timezone");
        assert!(
            child.status.success(),
            "non-Shanghai timezone child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
        return;
    }

    let db_path = temp_db_path("public-blog-runtime-data");
    let db_str = db_path.to_string_lossy().to_string();
    let now = Utc
        .with_ymd_and_hms(2026, 4, 7, 12, 0, 0)
        .single()
        .expect("valid evaluation time")
        .timestamp();
    let host_offset = Local
        .timestamp_opt(now, 0)
        .single()
        .expect("host local time")
        .offset()
        .local_minus_utc();
    assert_ne!(
        host_offset,
        8 * 60 * 60,
        "child must use a non-Shanghai timezone"
    );
    let zone = FixedOffset::east_opt(8 * 60 * 60).expect("fixed Shanghai offset");
    let local_now = zone.timestamp_opt(now, 0).single().expect("local time");
    let local_date = local_now.date_naive();
    let today_start = zone
        .from_local_datetime(&local_date.and_hms_opt(0, 0, 0).expect("day start"))
        .single()
        .expect("day boundary")
        .timestamp();
    let month_start_date = local_date.with_day(1).expect("month start date");
    let month_start = zone
        .from_local_datetime(&month_start_date.and_hms_opt(0, 0, 0).expect("month start"))
        .single()
        .expect("month boundary")
        .timestamp();
    let history_start_date = local_date
        .checked_sub_signed(chrono::Duration::days(90))
        .expect("history start date");
    let history_start = zone
        .from_local_datetime(
            &history_start_date
                .and_hms_opt(0, 0, 0)
                .expect("history start"),
        )
        .single()
        .expect("history boundary")
        .timestamp();

    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-blog-runtime-test-seed".to_string()],
        "http://127.0.0.1:1",
        &db_str,
    )
    .await
    .expect("proxy created with local-only upstream");

    sqlx::query(
        "UPDATE api_keys SET created_at = ?, quota_limit = 100, quota_synced_at = ? WHERE api_key = ?",
    )
    .bind(now - 86_400)
    .bind(now - 20_000)
    .bind("tvly-blog-runtime-test-seed")
    .execute(&proxy.key_store.pool)
    .await
    .expect("set seed key quota history");
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at, quota_limit, quota_synced_at)
        VALUES
            ('blog-runtime-active-a', 'tvly-blog-runtime-active-a', 'active', ?, 300, ?),
            ('blog-runtime-active-b', 'tvly-blog-runtime-active-b', 'active', ?, 500, ?),
            ('blog-runtime-quarantined', 'tvly-blog-runtime-quarantined', 'active', ?, 700, ?),
            ('blog-runtime-deleted', 'tvly-blog-runtime-deleted', 'active', ?, 800, ?),
            ('blog-runtime-reimported', 'tvly-blog-runtime-reimported', 'active', ?, 800, ?)
        "#,
    )
    .bind(now - 86_400)
    .bind(now - 20_000)
    .bind(now - 86_400)
    .bind(now - 3_600)
    .bind(now - 86_400)
    .bind(now - 20_000)
    .bind(now - 86_400)
    .bind(now - 20_000)
    .bind(now - 86_400)
    .bind(now - 20_000)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert lifecycle quota fixtures");
    sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE id = 'blog-runtime-deleted'")
        .bind(now - 3_600)
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete historical key");
    sqlx::query("UPDATE api_key_membership_history_state SET tracked_from = ? WHERE singleton = 1")
        .bind(history_start)
        .execute(&proxy.key_store.pool)
        .await
        .expect("set deterministic lifecycle tracking start");
    sqlx::query("DELETE FROM api_key_membership_intervals")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear initial lifecycle intervals");
    sqlx::query(
        r#"
        INSERT INTO api_key_membership_intervals (key_id, active_from, active_until)
        SELECT id, ?, NULL FROM api_keys WHERE api_key = 'tvly-blog-runtime-test-seed'
        UNION ALL SELECT id, ?, NULL FROM api_keys WHERE id = 'blog-runtime-active-a'
        UNION ALL SELECT id, ?, NULL FROM api_keys WHERE id = 'blog-runtime-active-b'
        UNION ALL SELECT id, ?, NULL FROM api_keys WHERE id = 'blog-runtime-quarantined'
        UNION ALL SELECT id, ?, ? FROM api_keys WHERE id = 'blog-runtime-deleted'
        UNION ALL SELECT id, ?, ? FROM api_keys WHERE id = 'blog-runtime-reimported'
        UNION ALL SELECT id, ?, NULL FROM api_keys WHERE id = 'blog-runtime-reimported'
        "#,
    )
    .bind(now - 86_400)
    .bind(now - 86_400)
    .bind(now - 86_400)
    .bind(now - 86_400)
    .bind(now - 86_400)
    .bind(now - 3_600)
    .bind(now - 86_400)
    .bind(now - 3_600)
    .bind(now - 1_800)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed complete and interrupted lifecycle intervals");
    sqlx::query(
        r#"
        INSERT INTO api_key_quarantines (
            id, key_id, source, reason_code, reason_summary, reason_detail, created_at
        ) VALUES (
            'blog-runtime-quarantine', 'blog-runtime-quarantined', 'test', 'test', 'test', 'test', ?
        )
        "#,
    )
    .bind(now - 36_000)
    .execute(&proxy.key_store.pool)
    .await
    .expect("quarantine key");
    sqlx::query(
        r#"
        INSERT INTO api_key_quota_sync_samples (key_id, quota_limit, quota_remaining, captured_at, source)
        VALUES
            ('blog-runtime-active-b', 450, 100, ?, 'test'),
            ('blog-runtime-active-b', 500, 120, ?, 'test'),
            ('blog-runtime-reimported', 800, 200, ?, 'test'),
            ('blog-runtime-reimported', 850, 100, ?, 'test')
        "#,
    )
    .bind(now - 18_000)
    .bind(now - 3_600)
    .bind(now - 20_000)
    .bind(now - 600)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert historical quota samples");
    sqlx::query(
        "UPDATE api_keys SET quota_limit = 850, quota_remaining = 100, quota_synced_at = ? WHERE id = 'blog-runtime-reimported'",
    )
    .bind(now - 600)
    .execute(&proxy.key_store.pool)
    .await
    .expect("set reimported key quota snapshot");

    let first_minute = today_start + 18 * 3600 + 15 * 60;
    let second_hour_minute = today_start + 19 * 3600 + 15 * 60;
    sqlx::query(
        r#"
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start, bucket_secs, total_requests, success_count, error_count,
            quota_exhausted_count, local_estimated_credits, updated_at
        ) VALUES
            (?, 60, 3, 3, 0, 0, 8, ?),
            (?, 60, 4, 4, 0, 0, 5, ?),
            (?, 60, 2, 2, 0, 0, 7, ?)
        "#,
    )
    .bind(first_minute)
    .bind(now)
    .bind(first_minute + 60)
    .bind(now)
    .bind(second_hour_minute)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert minute rollups");
    sqlx::query(
        r#"
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start, bucket_secs, total_requests, success_count, error_count,
            quota_exhausted_count, local_estimated_credits, updated_at
        ) VALUES
            (?, 60, 4, 4, 0, 0, 10, ?),
            (?, 60, 10, 10, 0, 0, 50, ?)
        "#,
    )
    .bind(history_start + 15 * 60)
    .bind(now)
    .bind(today_start - 86_400 + 15 * 60)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert minute rollups across UTC date boundaries");
    // Deliberately conflicting day rollups ensure this public read uses UTC minute buckets.
    sqlx::query(
        r#"
        INSERT INTO dashboard_request_rollup_buckets (
            bucket_start, bucket_secs, total_requests, success_count, error_count,
            quota_exhausted_count, local_estimated_credits, updated_at
        ) VALUES
            (?, 86400, 400, 400, 0, 0, 1000, ?),
            (?, 86400, 1000, 1000, 0, 0, 5000, ?),
            (?, 86400, 900, 900, 0, 0, 2000, ?)
        "#,
    )
    .bind(history_start)
    .bind(now)
    .bind(today_start - 86_400)
    .bind(now)
    .bind(today_start)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert daily rollups");

    let quota_times = [
        history_start - 1,
        now - 18_000,
        now - 14_400,
        now - 2_700,
        now - 1_200,
        now,
    ];
    let data = proxy
        .key_store
        .fetch_public_blog_runtime_data(
            month_start.min(today_start),
            history_start,
            now,
            &quota_times,
        )
        .await
        .expect("read public blog runtime data");

    assert_eq!(data.total_quota_limit, Some(1_750));
    assert_eq!(
        data.historical_quota_limits,
        vec![None, Some(2_450), Some(2_450), Some(900), None, Some(1_750)]
    );
    assert_eq!(
        data.hours
            .iter()
            .find(|hour| hour.local_hour_start == today_start + 18 * 3600)
            .map(|hour| (hour.requests, hour.credits)),
        Some((7, 13))
    );
    assert_eq!(
        data.hours
            .iter()
            .find(|hour| hour.local_hour_start == today_start + 19 * 3600)
            .map(|hour| (hour.requests, hour.credits)),
        Some((2, 7))
    );
    assert_eq!(
        data.days
            .iter()
            .find(|day| day.date == "2026-04-07")
            .map(|day| (day.requests, day.credits)),
        Some((9, 20))
    );
    assert_eq!(
        data.days
            .iter()
            .find(|day| day.date == "2026-04-06")
            .map(|day| (day.requests, day.credits)),
        Some((10, 50))
    );
    assert_eq!(
        data.days
            .iter()
            .find(|day| day.date == "2026-01-07")
            .map(|day| (day.requests, day.credits)),
        Some((4, 10))
    );

    sqlx::query("UPDATE api_keys SET quota_synced_at = NULL WHERE id = 'blog-runtime-active-a'")
        .execute(&proxy.key_store.pool)
        .await
        .expect("mark one eligible quota snapshot unsynced");
    let partially_unknown = proxy
        .key_store
        .fetch_public_blog_runtime_data(
            month_start.min(today_start),
            history_start,
            now,
            &quota_times,
        )
        .await
        .expect("read partially unknown current quota");
    assert_eq!(partially_unknown.total_quota_limit, None);

    sqlx::query("UPDATE api_keys SET quota_synced_at = 0 WHERE id = 'blog-runtime-active-a'")
        .execute(&proxy.key_store.pool)
        .await
        .expect("mark a zero timestamp quota snapshot unsynced");
    let zero_timestamp = proxy
        .key_store
        .fetch_public_blog_runtime_data(
            month_start.min(today_start),
            history_start,
            now,
            &quota_times,
        )
        .await
        .expect("read zero timestamp quota");
    assert_eq!(zero_timestamp.total_quota_limit, None);

    sqlx::query(
        "UPDATE api_keys SET quota_limit = NULL, quota_synced_at = ? WHERE id = 'blog-runtime-active-a'",
    )
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark a missing quota limit unknown");
    let missing_limit = proxy
        .key_store
        .fetch_public_blog_runtime_data(
            month_start.min(today_start),
            history_start,
            now,
            &quota_times,
        )
        .await
        .expect("read missing quota limit");
    assert_eq!(missing_limit.total_quota_limit, None);

    sqlx::query(
        r#"
        UPDATE api_keys
        SET quota_limit = NULL, quota_synced_at = NULL
        WHERE deleted_at IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM api_key_quarantines aq
              WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
          )
        "#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark every eligible quota unknown");
    let all_unknown = proxy
        .key_store
        .fetch_public_blog_runtime_data(
            month_start.min(today_start),
            history_start,
            now,
            &quota_times,
        )
        .await
        .expect("read all unknown quotas");
    assert_eq!(all_unknown.total_quota_limit, None);

    sqlx::query(
        r#"
        UPDATE api_keys SET deleted_at = ?
        WHERE deleted_at IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM api_key_quarantines aq
              WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
          )
        "#,
    )
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("remove every eligible key");
    let empty_pool = proxy
        .key_store
        .fetch_public_blog_runtime_data(
            month_start.min(today_start),
            history_start,
            now,
            &quota_times,
        )
        .await
        .expect("read empty eligible pool");
    assert_eq!(empty_pool.total_quota_limit, Some(0));
}

#[tokio::test]
async fn public_blog_runtime_tracks_key_reimport_membership_intervals() {
    let db_path = temp_db_path("public-blog-runtime-key-reimport");
    let db_str = db_path.to_string_lossy().to_string();
    let key = "tvly-blog-runtime-key-reimport".to_string();
    let proxy = TavilyProxy::with_endpoint(vec![key.clone()], "http://127.0.0.1:1", &db_str)
        .await
        .expect("proxy created with local-only upstream");

    proxy
        .key_store
        .sync_keys(crate::providers::ProviderId::Tavily, &[])
        .await
        .expect("soft-delete missing key");
    proxy
        .key_store
        .sync_keys(
            crate::providers::ProviderId::Tavily,
            std::slice::from_ref(&key),
        )
        .await
        .expect("reimport key");
    let key_id = sqlx::query_scalar::<_, String>("SELECT id FROM api_keys WHERE api_key = ?")
        .bind(&key)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("find key id");
    proxy
        .key_store
        .soft_delete_key_by_id(&key_id)
        .await
        .expect("admin soft-delete key");
    proxy
        .key_store
        .add_or_undelete_key(&key)
        .await
        .expect("admin reimport key");

    let intervals = sqlx::query_as::<_, (i64, Option<i64>)>(
        r#"
        SELECT active_from, active_until
        FROM api_key_membership_intervals
        WHERE key_id = (SELECT id FROM api_keys WHERE api_key = ?)
        ORDER BY active_from ASC, id ASC
        "#,
    )
    .bind(&key)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read key membership intervals");

    assert_eq!(intervals.len(), 3);
    assert!(
        intervals
            .iter()
            .take(2)
            .all(|(_, active_until)| active_until.is_some())
    );
    assert!(intervals[2].1.is_none());
}

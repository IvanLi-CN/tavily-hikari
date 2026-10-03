use super::*;

#[tokio::test]
async fn warm_schema_compatibility_restores_api_key_membership_intervals() {
    let db_path = temp_db_path("schema-migration-membership-intervals");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-membership-intervals".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    sqlx::query("DROP TABLE api_key_membership_intervals")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove membership intervals from the pre-feature schema");
    sqlx::query("DROP TABLE api_key_membership_history_state")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove membership history state from the pre-feature schema");
    let full_bootstrap = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect("warm schema verification must accept the pre-feature schema");
    assert!(!full_bootstrap);
    proxy
        .key_store
        .ensure_warm_schema_compatibility()
        .await
        .expect("warm schema compatibility must restore membership interval schema");
    let restored_objects: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN \
         ('api_key_membership_history_state', 'api_key_membership_intervals')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("check restored membership interval schema");
    assert_eq!(restored_objects, 2);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_compatibility_resets_history_after_lost_intervals() {
    let db_path = temp_db_path("schema-migration-membership-intervals-existing-marker");
    let db_str = db_path.to_string_lossy().to_string();
    let tracked_from = 1_700_000_000_i64;
    let rebuild_boundary = tracked_from + 7_200;
    let (backend_time, _) = BackendTime::manual_from_ts(rebuild_boundary);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-schema-migration-membership-intervals-existing-marker".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("create migrated database");
    let before_marker_key_id = "schema-migration-existing-marker-before-key";
    let reimported_key_id = "schema-migration-existing-marker-reimported-key";
    sqlx::query(
        r#"
        INSERT INTO api_keys (id, api_key, status, created_at)
        VALUES
            (?, ?, 'active', ?),
            (?, ?, 'active', ?)
        "#,
    )
    .bind(before_marker_key_id)
    .bind("tvly-schema-migration-existing-marker-before-key")
    .bind(tracked_from - 3_600)
    .bind(reimported_key_id)
    .bind("tvly-schema-migration-existing-marker-reimported-key")
    .bind(tracked_from - 1_800)
    .execute(&proxy.key_store.pool)
    .await
    .expect("create active API keys with old creation timestamps");
    sqlx::query("UPDATE api_key_membership_history_state SET tracked_from = ? WHERE singleton = 1")
        .bind(tracked_from)
        .execute(&proxy.key_store.pool)
        .await
        .expect("preserve the existing history marker");
    sqlx::query("DROP TABLE api_key_membership_intervals")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove only the interval table");

    proxy
        .key_store
        .ensure_warm_schema_compatibility()
        .await
        .expect("warm schema compatibility must reset the lost history boundary");
    let restored_tracked_from: i64 = sqlx::query_scalar(
        "SELECT tracked_from FROM api_key_membership_history_state WHERE singleton = 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read reset history boundary");
    assert_eq!(restored_tracked_from, rebuild_boundary);
    let active_from: Vec<i64> = sqlx::query_scalar(
        "SELECT active_from FROM api_key_membership_intervals WHERE key_id IN (?, ?) AND active_until IS NULL ORDER BY key_id",
    )
    .bind(before_marker_key_id)
    .bind(reimported_key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read rebuilt current membership intervals");
    assert_eq!(active_from, vec![rebuild_boundary, rebuild_boundary]);
    let old_history_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_key_membership_intervals WHERE active_from < ?",
    )
    .bind(rebuild_boundary)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("check that no pre-reset interval was fabricated");
    assert_eq!(old_history_rows, 0);

    drop(proxy);
    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("DROP TABLE api_key_membership_intervals")
        .execute(&lock_pool)
        .await
        .expect("remove intervals for the backward-clock restart");
    lock_pool.close().await;
    let (earlier_backend_time, _) = BackendTime::manual_from_ts(rebuild_boundary - 3_600);
    let restarted = KeyStore::new_with_time(&db_str, earlier_backend_time)
        .await
        .expect("restart schema migration with a backward wall clock");
    let retained_tracked_from: i64 = sqlx::query_scalar(
        "SELECT tracked_from FROM api_key_membership_history_state WHERE singleton = 1",
    )
    .fetch_one(&restarted.pool)
    .await
    .expect("read retained history boundary");
    assert_eq!(retained_tracked_from, rebuild_boundary);
    drop(restarted);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_compatibility_seeds_current_keys_when_history_marker_is_missing() {
    let db_path = temp_db_path("schema-migration-membership-intervals-missing-marker");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-membership-intervals-missing-marker".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    let key_id = "schema-migration-missing-marker-key";
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, status, created_at) VALUES (?, ?, 'active', ?)",
    )
    .bind(key_id)
    .bind("tvly-schema-migration-missing-marker-key")
    .bind(1_600_000_000_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("create active API key");
    sqlx::query("DELETE FROM api_key_membership_history_state")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove history marker");

    proxy
        .key_store
        .ensure_warm_schema_compatibility()
        .await
        .expect("warm schema compatibility must restore the missing marker");
    let tracked_from: i64 = sqlx::query_scalar(
        "SELECT tracked_from FROM api_key_membership_history_state WHERE singleton = 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read restored history marker");
    let active_from: i64 = sqlx::query_scalar(
        "SELECT active_from FROM api_key_membership_intervals WHERE key_id = ? AND active_until IS NULL",
    )
    .bind(key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read current membership interval");
    assert_eq!(active_from, tracked_from);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_compatibility_resets_history_for_active_key_without_interval() {
    let db_path = temp_db_path("schema-migration-membership-intervals-active-gap");
    let db_str = db_path.to_string_lossy().to_string();
    let tracked_from = 1_700_000_000_i64;
    let rebuild_boundary = tracked_from + 7_200;
    let (backend_time, _) = BackendTime::manual_from_ts(rebuild_boundary);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-schema-migration-membership-intervals-active-gap".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
        TavilyProxyOptions::from_database_path(&db_str),
        backend_time,
    )
    .await
    .expect("create migrated database");
    let key_id = "schema-migration-active-gap-key";
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, status, created_at) VALUES (?, ?, 'active', ?)",
    )
    .bind(key_id)
    .bind("tvly-schema-migration-active-gap-key")
    .bind(tracked_from - 3_600)
    .execute(&proxy.key_store.pool)
    .await
    .expect("create active API key without interval evidence");
    sqlx::query("UPDATE api_key_membership_history_state SET tracked_from = ? WHERE singleton = 1")
        .bind(tracked_from)
        .execute(&proxy.key_store.pool)
        .await
        .expect("preserve the existing history marker");

    proxy
        .key_store
        .ensure_warm_schema_compatibility()
        .await
        .expect("warm schema compatibility must repair the active membership gap");
    let restored_tracked_from: i64 = sqlx::query_scalar(
        "SELECT tracked_from FROM api_key_membership_history_state WHERE singleton = 1",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read reset history boundary");
    let active_from: i64 = sqlx::query_scalar(
        "SELECT active_from FROM api_key_membership_intervals WHERE key_id = ? AND active_until IS NULL",
    )
    .bind(key_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read repaired current membership interval");
    assert_eq!(restored_tracked_from, rebuild_boundary);
    assert_eq!(active_from, rebuild_boundary);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_compatibility_skips_complete_schema_write_under_lock() {
    let db_path = temp_db_path("schema-migration-membership-intervals-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-membership-intervals-read-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    let mut lock = lock_pool
        .acquire()
        .await
        .expect("acquire writer lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .expect("hold writer lock");
    tokio::time::timeout(
        Duration::from_secs(1),
        proxy.key_store.ensure_warm_schema_compatibility(),
    )
    .await
    .expect("complete warm schema must not wait for a writer")
    .expect("complete warm schema compatibility succeeds");
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn current_month_quota_rebase_skips_complete_state_under_lock() {
    let db_path = temp_db_path("schema-migration-monthly-rebase-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-monthly-rebase-read-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    let current_month_start = start_of_month(Utc::now()).timestamp();
    sqlx::query(
        "INSERT INTO meta (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
    .bind(current_month_start.to_string())
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark current month as rebased");

    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    let mut lock = lock_pool
        .acquire()
        .await
        .expect("acquire writer lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .expect("hold writer lock");
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        maybe_rebase_current_month_business_quota_with_pool(
            &proxy.key_store.pool,
            Utc::now,
            META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
            true,
        ),
    )
    .await
    .expect("complete monthly rebase must not wait for a writer")
    .expect("complete monthly rebase state succeeds");
    assert!(result.is_none());
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn linuxdo_system_tag_seed_skips_complete_state_under_lock() {
    let db_path = temp_db_path("schema-migration-linuxdo-tag-seed-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-linuxdo-tag-seed-read-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    let mut lock = lock_pool
        .acquire()
        .await
        .expect("acquire writer lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .expect("hold writer lock");
    tokio::time::timeout(
        Duration::from_secs(1),
        proxy.key_store.seed_linuxdo_system_tags(),
    )
    .await
    .expect("complete LinuxDo tags must not wait for a writer")
    .expect("complete LinuxDo tags succeed");
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn linuxdo_system_tag_delta_sync_skips_unchanged_state_under_lock() {
    let db_path = temp_db_path("schema-migration-linuxdo-tag-delta-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-linuxdo-tag-delta-read-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    sqlx::query(
        "INSERT INTO meta (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
    .bind(format_linuxdo_system_tag_default_deltas(
        linuxdo_system_tag_default_deltas(),
    ))
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark LinuxDo tag defaults as current");

    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    let mut lock = lock_pool
        .acquire()
        .await
        .expect("acquire writer lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .expect("hold writer lock");
    tokio::time::timeout(
        Duration::from_secs(1),
        proxy
            .key_store
            .sync_linuxdo_system_tag_default_deltas_with_env(),
    )
    .await
    .expect("unchanged LinuxDo defaults must not wait for a writer")
    .expect("unchanged LinuxDo defaults succeed");
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn account_quota_default_sync_skips_unchanged_state_under_lock() {
    let db_path = temp_db_path("schema-migration-account-quota-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-account-quota-read-only".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "schema-migration-account-quota".to_string(),
            username: Some("schema_migration_account_quota".to_string()),
            name: Some("Schema Migration Account Quota".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("create account");
    proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("create account quota row");
    let defaults = proxy
        .key_store
        .default_account_quota_limits_for_user(&user.user_id)
        .await
        .expect("read account quota defaults");
    sqlx::query(
        "UPDATE account_quota_limits \
         SET business_calls_1h_limit = ?, daily_credits_limit = ?, \
             monthly_credits_limit = ?, inherits_defaults = 1 \
         WHERE user_id = ?",
    )
    .bind(defaults.business_calls_1h_limit)
    .bind(defaults.daily_credits_limit)
    .bind(defaults.monthly_credits_limit)
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark account quota row as unchanged defaults");

    let lock_pool = connect_sqlite_test_pool(&db_str).await;
    let mut lock = lock_pool
        .acquire()
        .await
        .expect("acquire writer lock connection");
    sqlx::query("BEGIN IMMEDIATE")
        .execute(&mut *lock)
        .await
        .expect("hold writer lock");
    tokio::time::timeout(
        Duration::from_secs(1),
        proxy.key_store.sync_account_quota_limits_with_defaults(),
    )
    .await
    .expect("unchanged account defaults must not wait for a writer")
    .expect("unchanged account defaults succeed");
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn account_quota_default_sync_replays_missing_snapshot_when_limits_are_unchanged() {
    let db_path = temp_db_path("schema-migration-account-quota-snapshot-replay");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-account-quota-snapshot-replay".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "schema-migration-account-quota-snapshot-replay".to_string(),
            username: Some("schema_migration_account_quota_snapshot_replay".to_string()),
            name: Some("Schema Migration Account Quota Snapshot Replay".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("create account");
    proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("create account quota row and initial snapshot");
    let defaults = proxy
        .key_store
        .default_account_quota_limits_for_user(&user.user_id)
        .await
        .expect("read account quota defaults");
    sqlx::query(
        "UPDATE account_quota_limits \
         SET business_calls_1h_limit = ?, daily_credits_limit = ?, \
             monthly_credits_limit = ?, inherits_defaults = 1 \
         WHERE user_id = ?",
    )
    .bind(defaults.business_calls_1h_limit)
    .bind(defaults.daily_credits_limit)
    .bind(defaults.monthly_credits_limit)
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("mark account quota row as unchanged defaults");
    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove snapshot to simulate an interrupted prior sync");

    proxy
        .key_store
        .sync_account_quota_limits_with_defaults()
        .await
        .expect("unchanged defaults must replay the missing snapshot");

    let snapshot = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT business_calls_1h_limit, daily_credits_limit, monthly_credits_limit \
         FROM account_quota_limit_snapshots WHERE user_id = ? \
         ORDER BY changed_at DESC, id DESC LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("read replayed snapshot")
    .expect("replayed snapshot exists");
    assert_eq!(
        snapshot,
        (
            defaults.business_calls_1h_limit,
            defaults.daily_credits_limit,
            defaults.monthly_credits_limit,
        )
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

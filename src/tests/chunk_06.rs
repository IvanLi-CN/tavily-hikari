#[test]
fn build_account_quota_resolution_clamps_negative_tag_totals_to_zero() {
    let base = AccountQuotaLimits {
        hourly_any_limit: 10,
        hourly_limit: 20,
        daily_limit: 30,
        monthly_limit: 40,
        inherits_defaults: false,
    };
    let resolution = build_account_quota_resolution(
        base.clone(),
        vec![UserTagBindingRecord {
            source: USER_TAG_SOURCE_MANUAL.to_string(),
            tag: UserTagRecord {
                id: "custom-tag".to_string(),
                name: "custom_tag".to_string(),
                display_name: "Custom Tag".to_string(),
                icon: Some("sparkles".to_string()),
                system_key: None,
                effect_kind: USER_TAG_EFFECT_QUOTA_DELTA.to_string(),
                hourly_any_delta: -100,
                hourly_delta: -200,
                daily_delta: -300,
                monthly_delta: -400,
                user_count: 1,
            },
        }],
    );

    assert_eq!(resolution.base.hourly_any_limit, 10);
    assert_eq!(resolution.effective.hourly_any_limit, 0);
    assert_eq!(resolution.effective.hourly_limit, 0);
    assert_eq!(resolution.effective.daily_limit, 0);
    assert_eq!(resolution.effective.monthly_limit, 0);
    assert_eq!(resolution.breakdown.len(), 3);
    let effective_row = resolution
        .breakdown
        .iter()
        .find(|entry| entry.kind == "effective")
        .expect("effective row present");
    assert_eq!(effective_row.effect_kind, "effective");
    assert_eq!(effective_row.hourly_any_delta, 0);
    assert_eq!(effective_row.hourly_delta, 0);
    assert_eq!(effective_row.daily_delta, 0);
    assert_eq!(effective_row.monthly_delta, 0);
}

#[tokio::test]
async fn new_account_without_tags_defaults_to_zero_base_and_effective_quota() {
    let db_path = temp_db_path("new-account-zero-base");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "new-zero-base-user".to_string(),
            username: Some("new_zero_base_user".to_string()),
            name: Some("New Zero Base User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let resolution = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolve account quota");

    assert_eq!(resolution.base.hourly_any_limit, 0);
    assert_eq!(resolution.base.hourly_limit, 0);
    assert_eq!(resolution.base.daily_limit, 0);
    assert_eq!(resolution.base.monthly_limit, 0);
    assert!(!resolution.base.inherits_defaults);
    assert_eq!(resolution.effective.hourly_any_limit, 0);
    assert_eq!(resolution.effective.hourly_limit, 0);
    assert_eq!(resolution.effective.daily_limit, 0);
    assert_eq!(resolution.effective.monthly_limit, 0);

    let persisted: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read persisted zero base");
    assert_eq!(persisted, (0, 0, 0, 0, 0));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn new_linuxdo_account_effective_quota_comes_only_from_tags() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("new-linuxdo-tag-only-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "new-linuxdo-tag-only-user".to_string(),
            username: Some("new_linuxdo_tag_only_user".to_string()),
            name: Some("New LinuxDo Tag Only User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let resolution = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolve account quota");
    let tag_only_limits = AccountQuotaLimits::legacy_defaults();

    assert_eq!(resolution.base.hourly_any_limit, 0);
    assert_eq!(resolution.base.hourly_limit, 0);
    assert_eq!(resolution.base.daily_limit, 0);
    assert_eq!(resolution.base.monthly_limit, 0);
    assert_eq!(
        resolution.effective.hourly_any_limit,
        tag_only_limits.hourly_any_limit
    );
    assert_eq!(
        resolution.effective.hourly_limit,
        tag_only_limits.hourly_limit
    );
    assert_eq!(
        resolution.effective.daily_limit,
        tag_only_limits.daily_limit
    );
    assert_eq!(
        resolution.effective.monthly_limit,
        tag_only_limits.monthly_limit
    );
    assert!(
        resolution
            .tags
            .iter()
            .any(|binding| { binding.tag.system_key.as_deref() == Some("linuxdo_l2") })
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn historical_account_without_quota_row_keeps_legacy_defaults_on_first_resolution() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("historical-account-missing-quota-row");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "historical-missing-quota-row".to_string(),
            username: Some("historical_missing_quota_row".to_string()),
            name: Some("Historical Missing Quota Row".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    sqlx::query("DELETE FROM account_quota_limits WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete quota row");
    sqlx::query("UPDATE users SET created_at = ?, updated_at = ? WHERE id = ?")
        .bind(100_i64)
        .bind(100_i64)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("mark user historical");
    proxy
        .key_store
        .set_meta_i64(META_KEY_ACCOUNT_QUOTA_ZERO_BASE_CUTOVER_V1, 200)
        .await
        .expect("set zero-base cutover after user creation");

    let resolution = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolve historical account quota");
    let expected = AccountQuotaLimits::legacy_defaults();

    assert_eq!(resolution.base.hourly_any_limit, expected.hourly_any_limit);
    assert_eq!(resolution.base.hourly_limit, expected.hourly_limit);
    assert_eq!(resolution.base.daily_limit, expected.daily_limit);
    assert_eq!(resolution.base.monthly_limit, expected.monthly_limit);
    assert!(resolution.base.inherits_defaults);

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn manual_account_quota_matching_legacy_defaults_stays_custom_on_restart() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("manual-account-quota-matching-legacy-defaults");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "manual-legacy-default-tuple".to_string(),
            username: Some("manual_legacy_default_tuple".to_string()),
            name: Some("Manual Legacy Default Tuple".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let updated = proxy
        .key_store
        .update_account_quota_limits(&user.user_id, 11, 12, 13, 14)
        .await
        .expect("update account quota");
    assert!(updated);

    let first_row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read first row");
    assert_eq!(first_row, (11, 12, 13, 14, 0));

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let second_row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read second row");
    assert_eq!(second_row, (11, 12, 13, 14, 0));

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn legacy_default_following_account_keeps_inherits_defaults_on_noop_save() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("legacy-default-following-noop-save");
    let db_str = db_path.to_string_lossy().to_string();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "legacy-default-following-noop-save".to_string(),
            username: Some("legacy_default_following_noop_save".to_string()),
            name: Some("Legacy Default Following Noop Save".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 11,
               hourly_limit = 12,
               daily_limit = 13,
               monthly_limit = 14,
               inherits_defaults = 1
           WHERE user_id = ?"#,
    )
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed legacy default-following row");

    let updated = proxy
        .key_store
        .update_account_quota_limits(&user.user_id, 11, 12, 13, 14)
        .await
        .expect("update account quota");
    assert!(updated);

    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read row after noop save");
    assert_eq!(row, (11, 12, 13, 14, 1));

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_quota_resolution_cache_invalidates_on_binding_and_tag_updates() {
    let db_path = temp_db_path("account-quota-resolution-cache");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "quota-cache-user".to_string(),
            username: Some("quota_cache_user".to_string()),
            name: Some("Quota Cache User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let defaults = AccountQuotaLimits::zero_base();

    let initial = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("initial resolution");
    assert_eq!(
        initial.effective.hourly_any_limit,
        defaults.hourly_any_limit
    );
    assert_eq!(initial.effective.hourly_limit, defaults.hourly_limit);

    let tag = proxy
        .create_user_tag(
            "quota_cache_boost",
            "Quota Cache Boost",
            Some("sparkles"),
            USER_TAG_EFFECT_QUOTA_DELTA,
            7,
            8,
            9,
            10,
        )
        .await
        .expect("create custom tag");
    proxy
        .bind_user_tag_to_user(&user.user_id, &tag.id)
        .await
        .expect("bind user tag");

    let after_bind = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolution after bind");
    assert_eq!(
        after_bind.effective.hourly_any_limit,
        defaults.hourly_any_limit + 7
    );
    assert_eq!(after_bind.effective.hourly_limit, defaults.hourly_limit + 8);

    proxy
        .update_user_tag(
            &tag.id,
            "quota_cache_boost",
            "Quota Cache Boost",
            Some("sparkles"),
            USER_TAG_EFFECT_QUOTA_DELTA,
            11,
            12,
            13,
            14,
        )
        .await
        .expect("update user tag")
        .expect("updated user tag");

    let after_update = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolution after update");
    assert_eq!(
        after_update.effective.hourly_any_limit,
        defaults.hourly_any_limit + 11
    );
    assert_eq!(
        after_update.effective.hourly_limit,
        defaults.hourly_limit + 12
    );
    assert_eq!(
        after_update.effective.daily_limit,
        defaults.daily_limit + 13
    );
    assert_eq!(
        after_update.effective.monthly_limit,
        defaults.monthly_limit + 14
    );

    proxy
        .unbind_user_tag_from_user(&user.user_id, &tag.id)
        .await
        .expect("unbind user tag");
    let after_unbind = proxy
        .key_store
        .resolve_account_quota_resolution(&user.user_id)
        .await
        .expect("resolution after unbind");
    assert_eq!(
        after_unbind.effective.hourly_any_limit,
        defaults.hourly_any_limit
    );
    assert_eq!(after_unbind.effective.hourly_limit, defaults.hourly_limit);
    assert_eq!(after_unbind.effective.daily_limit, defaults.daily_limit);
    assert_eq!(after_unbind.effective.monthly_limit, defaults.monthly_limit);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_system_tag_defaults_backfill_repairs_legacy_zero_seed() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tag-defaults");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    sqlx::query(
        r#"UPDATE user_tags
           SET hourly_any_delta = 0,
               hourly_delta = 0,
               daily_delta = 0,
               monthly_delta = 0
           WHERE system_key LIKE 'linuxdo_l%'"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("zero system tag deltas");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear linuxdo defaults migration marker");
    drop(proxy);

    let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy recreated");
    let defaults = linuxdo_system_tag_default_deltas();
    let seeded_rows = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        "SELECT hourly_any_delta, hourly_delta, daily_delta, monthly_delta FROM user_tags WHERE system_key LIKE 'linuxdo_l%' ORDER BY system_key",
    )
    .fetch_all(&repaired.key_store.pool)
    .await
    .expect("read repaired seeded tag rows");
    assert_eq!(seeded_rows.len(), 5);
    assert!(
        seeded_rows
            .iter()
            .all(|row| *row == (defaults.0, defaults.1, defaults.2, defaults.3))
    );
}

#[tokio::test]
async fn linuxdo_system_tag_defaults_backfill_repairs_partial_legacy_zero_seed() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tag-defaults-partial");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    sqlx::query(
        r#"UPDATE user_tags
           SET hourly_any_delta = 0,
               hourly_delta = 0,
               daily_delta = 0,
               monthly_delta = 0
           WHERE system_key IN ('linuxdo_l1', 'linuxdo_l3')"#,
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("zero partial system tag deltas");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear linuxdo defaults migration marker");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_LINUXDO_SYSTEM_TAG_DEFAULTS_TUPLE_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear linuxdo defaults tuple marker");
    drop(proxy);

    let repaired = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy recreated");
    let defaults = linuxdo_system_tag_default_deltas();
    let seeded_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&repaired.key_store.pool)
    .await
    .expect("read repaired seeded tag rows");
    assert_eq!(seeded_rows.len(), 5);
    assert!(
        seeded_rows
            .iter()
            .all(|(_, hourly_any, hourly, daily, monthly)| {
                (*hourly_any, *hourly, *daily, *monthly)
                    == (defaults.0, defaults.1, defaults.2, defaults.3)
            })
    );
}

#[tokio::test]
async fn linuxdo_system_tag_defaults_follow_env_changes_without_overwriting_customized_system_tags()
{
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tag-default-sync");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let initial_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read initial linuxdo system tag rows");
    assert_eq!(initial_rows.len(), 5);
    assert!(
        initial_rows
            .iter()
            .all(|(_, hourly_any, hourly, daily, monthly)| {
                (*hourly_any, *hourly, *daily, *monthly) == (11, 12, 13, 14)
            })
    );
    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after_default_change =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened after default change");
    let synced_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&proxy_after_default_change.key_store.pool)
    .await
    .expect("read synced linuxdo system tag rows");
    assert!(
        synced_rows
            .iter()
            .all(|(_, hourly_any, hourly, daily, monthly)| {
                (*hourly_any, *hourly, *daily, *monthly) == (21, 22, 23, 24)
            })
    );

    proxy_after_default_change
        .update_user_tag(
            "linuxdo_l2",
            "linuxdo_l2",
            "L2",
            Some("linuxdo"),
            USER_TAG_EFFECT_QUOTA_DELTA,
            101,
            102,
            103,
            104,
        )
        .await
        .expect("update system tag")
        .expect("system tag present");
    drop(proxy_after_default_change);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "31");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "32");
        std::env::set_var("TOKEN_DAILY_LIMIT", "33");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "34");
    }

    let proxy_after_customization =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened after system tag customization");
    let final_rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        r#"SELECT system_key, hourly_any_delta, hourly_delta, daily_delta, monthly_delta
           FROM user_tags
           WHERE system_key LIKE 'linuxdo_l%'
           ORDER BY system_key"#,
    )
    .fetch_all(&proxy_after_customization.key_store.pool)
    .await
    .expect("read final linuxdo system tag rows");
    assert_eq!(final_rows.len(), 5);
    for (system_key, hourly_any, hourly, daily, monthly) in final_rows {
        if system_key == "linuxdo_l2" {
            assert_eq!((hourly_any, hourly, daily, monthly), (101, 102, 103, 104));
        } else {
            assert_eq!((hourly_any, hourly, daily, monthly), (31, 32, 33, 34));
        }
    }

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_system_tags_seed_backfill_and_trust_level_sync() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-system-tags");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let seeded_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_tags WHERE system_key LIKE 'linuxdo_l%'")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("count seeded tags");
    assert_eq!(seeded_count, 5);

    let defaults = linuxdo_system_tag_default_deltas();
    let seeded_rows = sqlx::query_as::<_, (String, String, Option<String>, i64, i64, i64, i64)>(
        "SELECT display_name, name, icon, hourly_any_delta, hourly_delta, daily_delta, monthly_delta FROM user_tags WHERE system_key LIKE 'linuxdo_l%' ORDER BY system_key",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read seeded tag rows");
    assert_eq!(seeded_rows.len(), 5);
    assert_eq!(
        seeded_rows[0],
        (
            "L0".to_string(),
            "linuxdo_l0".to_string(),
            Some("linuxdo".to_string()),
            defaults.0,
            defaults.1,
            defaults.2,
            defaults.3,
        )
    );
    assert_eq!(
        seeded_rows[4],
        (
            "L4".to_string(),
            "linuxdo_l4".to_string(),
            Some("linuxdo".to_string()),
            defaults.0,
            defaults.1,
            defaults.2,
            defaults.3,
        )
    );

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-system-user".to_string(),
            username: Some("linuxdo_system_user".to_string()),
            name: Some("LinuxDo System User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(3),
            raw_payload_json: None,
        })
        .await
        .expect("upsert linuxdo user");

    let first_key: String = sqlx::query_scalar(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read first linuxdo binding");
    assert_eq!(first_key, "linuxdo_l3");

    sqlx::query("DELETE FROM user_tag_bindings WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete bindings to simulate historical gap");
    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let restored_key: String = sqlx::query_scalar(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read restored linuxdo binding");
    assert_eq!(restored_key, "linuxdo_l3");

    proxy_after
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-system-user".to_string(),
            username: Some("linuxdo_system_user".to_string()),
            name: Some("LinuxDo System User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("update linuxdo trust level");
    let sync_keys = sqlx::query_scalar::<_, String>(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           ORDER BY t.system_key"#,
    )
    .bind(&user.user_id)
    .fetch_all(&proxy_after.key_store.pool)
    .await
    .expect("read synced linuxdo bindings");
    assert_eq!(sync_keys, vec!["linuxdo_l1".to_string()]);

    proxy_after
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-system-user".to_string(),
            username: Some("linuxdo_system_user".to_string()),
            name: Some("LinuxDo System User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("update linuxdo trust level to none");
    let retained_keys = sqlx::query_scalar::<_, String>(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           ORDER BY t.system_key"#,
    )
    .bind(&user.user_id)
    .fetch_all(&proxy_after.key_store.pool)
    .await
    .expect("read retained linuxdo bindings");
    assert_eq!(retained_keys, vec!["linuxdo_l1".to_string()]);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn linuxdo_oauth_upsert_skips_missing_tags_for_new_accounts_and_recovers_after_reseed() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("linuxdo-sync-best-effort");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    sqlx::query("DELETE FROM user_tags WHERE system_key LIKE 'linuxdo_l%'")
        .execute(&proxy.key_store.pool)
        .await
        .expect("delete linuxdo system tags");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-best-effort-user".to_string(),
            username: Some("linuxdo_best_effort_user".to_string()),
            name: Some("LinuxDo Best Effort User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("new linuxdo account should still succeed without system tags");

    let oauth_row_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM oauth_accounts WHERE provider = 'linuxdo' AND provider_user_id = ?",
    )
    .bind("linuxdo-best-effort-user")
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count oauth rows");
    assert_eq!(oauth_row_count, 1);
    let user_row_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE username = ?")
        .bind("linuxdo_best_effort_user")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count user rows");
    assert_eq!(user_row_count, 1);

    let binding_count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*)
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count linuxdo bindings after skipped sync");
    assert_eq!(binding_count, 0);

    proxy
        .key_store
        .seed_linuxdo_system_tags()
        .await
        .expect("reseed linuxdo system tags");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "linuxdo-best-effort-user".to_string(),
            username: Some("linuxdo_best_effort_user".to_string()),
            name: Some("LinuxDo Best Effort User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("oauth upsert should attach system tag after reseeding tags");

    let restored_key: String = sqlx::query_scalar(
        r#"SELECT t.system_key
           FROM user_tag_bindings b
           JOIN user_tags t ON t.id = b.tag_id
           WHERE b.user_id = ? AND t.system_key LIKE 'linuxdo_l%'
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read restored linuxdo binding");
    assert_eq!(restored_key, "linuxdo_l2");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn legacy_custom_account_quota_limits_with_initial_timestamps_are_reclassified_before_default_resync()
 {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-legacy-custom");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-custom-user".to_string(),
            username: Some("legacy_custom_user".to_string()),
            name: Some("Legacy Custom User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_custom_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("seed account quota row");
    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 101,
               hourly_limit = 102,
               daily_limit = 103,
               monthly_limit = 104,
               inherits_defaults = 1,
               updated_at = created_at
           WHERE user_id = ?"#,
    )
    .bind(&user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate legacy custom quota row with initial timestamps");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear inherits defaults backfill marker");

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read persisted legacy custom limits");
    assert_eq!(limits, (101, 102, 103, 104, 0));

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn custom_account_quota_limits_survive_default_resync() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-custom-persist");
    let db_str = db_path.to_string_lossy().to_string();
    let env_keys = [
        "TOKEN_HOURLY_REQUEST_LIMIT",
        "TOKEN_HOURLY_LIMIT",
        "TOKEN_DAILY_LIMIT",
        "TOKEN_MONTHLY_LIMIT",
    ];
    let previous: Vec<Option<String>> =
        env_keys.iter().map(|key| std::env::var(key).ok()).collect();

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "11");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "12");
        std::env::set_var("TOKEN_DAILY_LIMIT", "13");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "14");
    }

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "limit-custom-user".to_string(),
            username: Some("limit_custom_user".to_string()),
            name: Some("Limit Custom User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:limit_custom_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("seed account quota row");
    let updated = proxy
        .update_account_quota_limits(&user.user_id, 101, 102, 103, 104)
        .await
        .expect("update custom base quota");
    assert!(updated);

    let first_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read custom limits");
    assert_eq!(first_limits, (101, 102, 103, 104, 0));

    drop(proxy);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");
    let second_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read persisted custom limits");
    assert_eq!(second_limits, (101, 102, 103, 104, 0));

    unsafe {
        for (key, old_value) in env_keys.iter().zip(previous.into_iter()) {
            if let Some(value) = old_value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn block_all_user_tag_zeroes_effective_quota_and_blocks_account_usage() {
    let db_path = temp_db_path("user-tag-block-all");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "block-all-user".to_string(),
            username: Some("block_all_user".to_string()),
            name: Some("Block All User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:block_all_user"))
        .await
        .expect("bind token");
    let tag = proxy
        .create_user_tag(
            "blocked_all",
            "Blocked All",
            Some("ban"),
            USER_TAG_EFFECT_BLOCK_ALL,
            0,
            0,
            0,
            0,
        )
        .await
        .expect("create block all tag");
    let bound = proxy
        .bind_user_tag_to_user(&user.user_id, &tag.id)
        .await
        .expect("bind block all tag");
    assert!(bound);

    let details = proxy
        .get_admin_user_quota_details(&user.user_id)
        .await
        .expect("quota details")
        .expect("quota details present");
    assert_eq!(details.effective.hourly_any_limit, 0);
    assert_eq!(details.effective.hourly_limit, 0);
    assert_eq!(details.effective.daily_limit, 0);
    assert_eq!(details.effective.monthly_limit, 0);
    assert!(
        details
            .breakdown
            .iter()
            .any(|entry| entry.effect_kind == USER_TAG_EFFECT_BLOCK_ALL)
    );

    let hourly_any_verdict = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("hourly-any verdict");
    assert!(hourly_any_verdict.allowed);
    assert_eq!(hourly_any_verdict.hourly_limit, request_rate_limit());
    assert_eq!(
        hourly_any_verdict.window_minutes,
        request_rate_limit_window_minutes()
    );
    assert_eq!(hourly_any_verdict.scope, RequestRateScope::User);

    let quota_verdict = proxy
        .check_token_quota(&token.id)
        .await
        .expect("business quota verdict");
    assert!(!quota_verdict.allowed);
    assert_eq!(quota_verdict.hourly_limit, 0);
    assert_eq!(quota_verdict.daily_limit, 0);
    assert_eq!(quota_verdict.monthly_limit, 0);

    let request_usage: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_REQUEST_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read raw request usage");
    assert_eq!(request_usage, 0);
    let hourly_usage: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read hourly business usage");
    assert_eq!(hourly_usage, 0);
    let daily_usage: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read daily business usage");
    assert_eq!(daily_usage, 0);
    let monthly_usage = sqlx::query_scalar::<_, i64>(
        "SELECT month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("read monthly business usage")
    .unwrap_or(0);
    assert_eq!(monthly_usage, 0);

    let unbound = proxy
        .unbind_user_tag_from_user(&user.user_id, &tag.id)
        .await
        .expect("unbind block all tag");
    assert!(unbound);

    let hourly_any_after_unbind = proxy
        .check_token_hourly_requests(&token.id)
        .await
        .expect("hourly-any verdict after unbind");
    assert!(hourly_any_after_unbind.allowed);

    let quota_after_unbind = proxy
        .check_token_quota(&token.id)
        .await
        .expect("business quota verdict after unbind");
    assert!(quota_after_unbind.allowed);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn list_recent_jobs_paginated_includes_key_group() {
    let db_path = temp_db_path("jobs-list-key-group");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let grouped_key_id = proxy
        .add_or_undelete_key_in_group("tvly-jobs-grouped", Some("ops"))
        .await
        .expect("create grouped key");
    let ungrouped_key_id = proxy
        .add_or_undelete_key_in_group("tvly-jobs-ungrouped", None)
        .await
        .expect("create ungrouped key");

    let grouped_job_id = proxy
        .scheduled_job_start("quota_sync", Some(&grouped_key_id), 1)
        .await
        .expect("start grouped job");
    proxy
        .scheduled_job_finish(grouped_job_id, "error", Some("usage_http 401"))
        .await
        .expect("finish grouped job");

    let ungrouped_job_id = proxy
        .scheduled_job_start("quota_sync", Some(&ungrouped_key_id), 1)
        .await
        .expect("start ungrouped job");
    proxy
        .scheduled_job_finish(ungrouped_job_id, "success", Some("limit=100 remaining=99"))
        .await
        .expect("finish ungrouped job");

    let cleanup_job_id = proxy
        .scheduled_job_start("log_cleanup", None, 1)
        .await
        .expect("start cleanup job");
    proxy
        .scheduled_job_finish(cleanup_job_id, "success", Some("pruned=10"))
        .await
        .expect("finish cleanup job");

    let usage_job_id = proxy
        .scheduled_job_start("usage_aggregation", Some(&grouped_key_id), 1)
        .await
        .expect("start usage job");
    proxy
        .scheduled_job_finish(usage_job_id, "success", Some("aggregated_days=2"))
        .await
        .expect("finish usage job");

    let geo_job_id = proxy
        .scheduled_job_start("forward_proxy_geo_refresh", None, 1)
        .await
        .expect("start geo job");
    proxy
        .scheduled_job_finish(geo_job_id, "success", Some("refreshed_candidates=4"))
        .await
        .expect("finish geo job");

    let linuxdo_job_id = proxy
        .scheduled_job_start("linuxdo_user_status_sync", None, 1)
        .await
        .expect("start linuxdo job");
    proxy
        .scheduled_job_finish(
            linuxdo_job_id,
            "error",
            Some("attempted=8 success=7 failure=1"),
        )
        .await
        .expect("finish linuxdo job");

    let (items, total, group_counts) = proxy
        .list_recent_jobs_paginated("all", 1, 10)
        .await
        .expect("list jobs");

    assert_eq!(total, 6);
    assert_eq!(
        group_counts,
        JobGroupCounts {
            all: 6,
            quota: 2,
            usage: 1,
            logs: 1,
            geo: 1,
            linuxdo: 1,
        }
    );

    let grouped_job = items
        .iter()
        .find(|item| item.key_id.as_deref() == Some(grouped_key_id.as_str()))
        .expect("grouped job present");
    assert_eq!(grouped_job.key_group.as_deref(), Some("ops"));

    let ungrouped_job = items
        .iter()
        .find(|item| item.key_id.as_deref() == Some(ungrouped_key_id.as_str()))
        .expect("ungrouped job present");
    assert_eq!(ungrouped_job.key_group, None);

    let cleanup_job = items
        .iter()
        .find(|item| item.job_type == "log_cleanup")
        .expect("cleanup job present");
    assert_eq!(cleanup_job.key_group, None);

    let geo_job = items
        .iter()
        .find(|item| item.job_type == "forward_proxy_geo_refresh")
        .expect("geo job present");
    assert_eq!(geo_job.key_id, None);
    assert_eq!(geo_job.key_group, None);

    let (usage_items, usage_total, usage_counts) = proxy
        .list_recent_jobs_paginated("usage", 1, 10)
        .await
        .expect("list usage jobs");
    assert_eq!(usage_total, 1);
    assert_eq!(usage_items.len(), 1);
    assert_eq!(usage_items[0].job_type, "usage_aggregation");
    assert_eq!(usage_counts.usage, 1);

    let (geo_items, geo_total, geo_counts) = proxy
        .list_recent_jobs_paginated("geo", 1, 10)
        .await
        .expect("list geo jobs");
    assert_eq!(geo_total, 1);
    assert_eq!(geo_items.len(), 1);
    assert_eq!(geo_items[0].job_type, "forward_proxy_geo_refresh");
    assert_eq!(geo_counts.geo, 1);

    let (linuxdo_items, linuxdo_total, linuxdo_counts) = proxy
        .list_recent_jobs_paginated("linuxdo", 1, 10)
        .await
        .expect("list linuxdo jobs");
    assert_eq!(linuxdo_total, 1);
    assert_eq!(linuxdo_items.len(), 1);
    assert_eq!(linuxdo_items[0].job_type, "linuxdo_user_status_sync");
    assert_eq!(linuxdo_items[0].key_id, None);
    assert_eq!(linuxdo_counts.linuxdo, 1);

    let _ = std::fs::remove_file(db_path);
}

async fn seed_charged_business_attempt(proxy: &TavilyProxy, token_id: &str, credits: i64) {
    let log_id = proxy
        .record_pending_billing_attempt(
            token_id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("seed charged business attempt"),
            credits,
            None,
        )
        .await
        .expect("record pending billing attempt");
    let outcome = proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle pending billing attempt");
    assert_eq!(outcome, PendingBillingSettleOutcome::Charged);
}

async fn insert_charged_business_log(
    proxy: &TavilyProxy,
    token_id: &str,
    created_at: i64,
    credits: i64,
) {
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            query,
            http_status,
            mcp_status,
            result_status,
            error_message,
            created_at,
            counts_business_quota,
            billing_state,
            business_credits,
            billing_subject
        ) VALUES (?, 'POST', '/api/tavily/search', NULL, 200, 200, ?, NULL, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(token_id)
    .bind(OUTCOME_SUCCESS)
    .bind(created_at)
    .bind(BILLING_STATE_CHARGED)
    .bind(credits)
    .bind(format!("token:{token_id}"))
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert charged business log");
}

async fn current_month_charged_stats(pool: &SqlitePool) -> (i64, i64) {
    let now = Utc::now();
    let month_start = start_of_month(now).timestamp();
    sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            COUNT(*) AS charged_rows,
            COALESCE(SUM(business_credits), 0) AS charged_credits
        FROM auth_token_logs
        WHERE billing_state = ?
          AND COALESCE(business_credits, 0) > 0
          AND created_at >= ?
        "#,
    )
    .bind(BILLING_STATE_CHARGED)
    .bind(month_start)
    .fetch_one(pool)
    .await
    .expect("read current month charged stats")
}

async fn account_business_window_sums(pool: &SqlitePool, user_id: &str) -> (i64, i64) {
    let minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(pool)
    .await
    .expect("read account minute usage");
    let hour_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(pool)
    .await
    .expect("read account hour usage");
    (minute_sum, hour_sum)
}

#[tokio::test]
async fn billing_ledger_audit_detects_bound_token_month_residue_and_rebase_preserves_hour_day() {
    let db_path = temp_db_path("monthly-quota-rebase-bound-token");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "monthly-rebase-bound-user".to_string(),
            username: Some("monthly_bound".to_string()),
            name: Some("Monthly Bound".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:monthly_bound"))
        .await
        .expect("bind token");

    for _ in 0..5 {
        seed_charged_business_attempt(&proxy, &token.id, 1).await;
    }

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let baseline_verdict = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek bound quota");
    assert_eq!(baseline_verdict.hourly_used, 5);
    assert_eq!(baseline_verdict.daily_used, 5);
    assert_eq!(baseline_verdict.monthly_used, 5);

    let baseline_window_sums =
        account_business_window_sums(&proxy.key_store.pool, &user.user_id).await;
    let baseline_charged_stats = current_month_charged_stats(&proxy.key_store.pool).await;

    sqlx::query(
        r#"
        INSERT INTO account_monthly_quota (user_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(user_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&user.user_id)
    .bind(current_month_start)
    .bind(1_350_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt account month quota");
    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(token_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(1_959_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt token month quota");

    let audit_before = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, Utc::now())
        .await
        .expect("audit before rebase");
    assert_eq!(audit_before.summary.hour_only_mismatches, 0);
    assert_eq!(audit_before.summary.day_only_mismatches, 0);
    assert_eq!(audit_before.summary.month_only_mismatches, 2);
    assert_eq!(audit_before.summary.mixed_mismatches, 0);

    let token_subject = format!("token:{}", token.id);
    let token_entry = audit_before
        .entries
        .iter()
        .find(|entry| entry.billing_subject == token_subject)
        .expect("bound token entry present");
    assert_eq!(token_entry.hour.diff_credits, 0);
    assert_eq!(token_entry.day.diff_credits, 0);
    assert_eq!(token_entry.month.ledger_credits, 0);
    assert_eq!(token_entry.month.quota_credits, 1_959);

    let account_subject = format!("account:{}", user.user_id);
    let account_entry = audit_before
        .entries
        .iter()
        .find(|entry| entry.billing_subject == account_subject)
        .expect("bound account entry present");
    assert_eq!(account_entry.hour.diff_credits, 0);
    assert_eq!(account_entry.day.diff_credits, 0);
    assert_eq!(account_entry.month.ledger_credits, 5);
    assert_eq!(account_entry.month.quota_credits, 1_350);
    assert_eq!(account_entry.month.diff_credits, 1_345);

    let rebase_report = rebase_current_month_business_quota_with_pool(
        &proxy.key_store.pool,
        Utc::now(),
        META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
        true,
    )
    .await
    .expect("rebase current month");
    assert_eq!(rebase_report.current_month_charged_rows, 5);
    assert_eq!(rebase_report.current_month_charged_credits, 5);
    assert_eq!(rebase_report.rebased_subject_count, 1);
    assert_eq!(rebase_report.rebased_account_subjects, 1);
    assert_eq!(rebase_report.rebased_token_subjects, 0);
    assert!(rebase_report.cleared_token_rows >= 1);
    assert!(rebase_report.cleared_account_rows >= 1);

    let audit_after = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, Utc::now())
        .await
        .expect("audit after rebase");
    assert_eq!(audit_after.summary.mismatched_subjects, 0);

    let token_month_row: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read bound token monthly row");
    assert_eq!(token_month_row, (current_month_start, 0));

    let account_month_row: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account monthly row");
    assert_eq!(account_month_row, (current_month_start, 5));

    let charged_stats_after = current_month_charged_stats(&proxy.key_store.pool).await;
    assert_eq!(charged_stats_after, baseline_charged_stats);

    let post_window_sums = account_business_window_sums(&proxy.key_store.pool, &user.user_id).await;
    assert_eq!(post_window_sums, baseline_window_sums);

    let verdict_after = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek bound quota after");
    assert_eq!(verdict_after.hourly_used, 5);
    assert_eq!(verdict_after.daily_used, 5);
    assert_eq!(verdict_after.monthly_used, 5);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn billing_ledger_audit_day_quota_includes_same_day_legacy_hour_buckets() {
    let db_path = temp_db_path("billing-ledger-day-cutover-audit");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("billing-day-cutover"))
        .await
        .expect("create token");

    let now = Utc::now();
    let day_window = server_local_day_window_utc(now.with_timezone(&Local));
    let current_month_start = start_of_month(now).timestamp();
    let first_log = (day_window.start + 60).min(now.timestamp());
    let second_log = (day_window.start + 120).min(now.timestamp());

    insert_charged_business_log(&proxy, &token.id, first_log, 4).await;
    insert_charged_business_log(&proxy, &token.id, second_log, 6).await;

    sqlx::query(
        r#"
        INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
        VALUES (?, ?, ?, ?), (?, ?, ?, ?)
        "#,
    )
    .bind(&token.id)
    .bind(day_window.start)
    .bind(GRANULARITY_DAY)
    .bind(4_i64)
    .bind(&token.id)
    .bind(day_window.start)
    .bind(GRANULARITY_HOUR)
    .bind(6_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed day and legacy hour buckets");
    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(token_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(10_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token monthly quota");

    let audit = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, now)
        .await
        .expect("audit business quota ledger");
    let token_subject = format!("token:{}", token.id);
    let entry = audit
        .entries
        .iter()
        .find(|entry| entry.billing_subject == token_subject)
        .expect("token entry present");

    assert_eq!(entry.day.ledger_credits, 10);
    assert_eq!(entry.day.quota_credits, 10);
    assert_eq!(entry.day.diff_credits, 0);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn monthly_quota_rebase_startup_gate_runs_once_and_manual_rebase_remains_idempotent() {
    let db_path = temp_db_path("monthly-quota-rebase-startup-gate");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("monthly-rebase-unbound"))
        .await
        .expect("create unbound token");

    for _ in 0..3 {
        seed_charged_business_attempt(&proxy, &token.id, 2).await;
    }

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let charged_stats_before = current_month_charged_stats(&proxy.key_store.pool).await;
    assert_eq!(charged_stats_before, (3, 6));

    sqlx::query(
        r#"
        INSERT INTO auth_token_quota (token_id, month_start, month_count)
        VALUES (?, ?, ?)
        ON CONFLICT(token_id) DO UPDATE SET
            month_start = excluded.month_start,
            month_count = excluded.month_count
        "#,
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(17_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("corrupt token month quota");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset monthly rebase meta");

    let audit_before = audit_business_quota_ledger_with_pool(&proxy.key_store.pool, Utc::now())
        .await
        .expect("audit before startup rebase");
    assert_eq!(audit_before.summary.month_only_mismatches, 1);

    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened");

    let audit_after_startup =
        audit_business_quota_ledger_with_pool(&proxy_after.key_store.pool, Utc::now())
            .await
            .expect("audit after startup rebase");
    assert_eq!(audit_after_startup.summary.mismatched_subjects, 0);

    let token_month_count_after_startup: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy_after.key_store.pool)
            .await
            .expect("read token month after startup rebase");
    assert_eq!(token_month_count_after_startup, 6);

    let meta_value_after_startup: i64 =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
            .fetch_one(&proxy_after.key_store.pool)
            .await
            .expect("read startup rebase meta");
    assert_eq!(meta_value_after_startup, current_month_start);

    sqlx::query("UPDATE auth_token_quota SET month_count = ? WHERE token_id = ?")
        .bind(9_i64)
        .bind(&token.id)
        .execute(&proxy_after.key_store.pool)
        .await
        .expect("corrupt token month after startup rebase");
    drop(proxy_after);

    let proxy_third = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened third time");
    let token_month_count_after_third_start: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy_third.key_store.pool)
            .await
            .expect("read token month after third start");
    assert_eq!(
        token_month_count_after_third_start, 9,
        "startup gate should not rerun once current-month meta is already set"
    );

    let audit_after_third_start =
        audit_business_quota_ledger_with_pool(&proxy_third.key_store.pool, Utc::now())
            .await
            .expect("audit after third start");
    assert_eq!(audit_after_third_start.summary.month_only_mismatches, 1);

    let manual_rebase_report = rebase_current_month_business_quota_with_pool(
        &proxy_third.key_store.pool,
        Utc::now(),
        META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1,
        true,
    )
    .await
    .expect("manual rebase after startup gate");
    assert_eq!(
        manual_rebase_report.previous_rebase_month_start,
        Some(current_month_start)
    );
    assert!(!manual_rebase_report.meta_updated);
    assert_eq!(manual_rebase_report.rebased_subject_count, 1);
    assert_eq!(manual_rebase_report.rebased_token_subjects, 1);
    assert_eq!(manual_rebase_report.rebased_account_subjects, 0);

    let token_month_count_after_manual: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy_third.key_store.pool)
            .await
            .expect("read token month after manual rebase");
    assert_eq!(token_month_count_after_manual, 6);

    let audit_after_manual =
        audit_business_quota_ledger_with_pool(&proxy_third.key_store.pool, Utc::now())
            .await
            .expect("audit after manual rebase");
    assert_eq!(audit_after_manual.summary.mismatched_subjects, 0);

    let charged_stats_after_manual = current_month_charged_stats(&proxy_third.key_store.pool).await;
    assert_eq!(charged_stats_after_manual, charged_stats_before);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn begin_immediate_sqlite_connection_takes_write_lock_up_front() {
    use sqlx::Connection;

    let db_path = temp_db_path("monthly-quota-rebase-begin-immediate");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let mut immediate_conn = begin_immediate_sqlite_connection(&proxy.key_store.pool)
        .await
        .expect("begin immediate transaction");

    let writer_options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_millis(20));
    let mut competing_writer = sqlx::SqliteConnection::connect_with(&writer_options)
        .await
        .expect("connect competing writer");

    let write_err = sqlx::query("INSERT INTO meta (key, value) VALUES (?, ?)")
        .bind(format!("begin-immediate-lock-{}", nanoid!(6)))
        .bind("1")
        .execute(&mut competing_writer)
        .await
        .expect_err("write should wait on the immediate transaction lock");
    assert!(is_transient_sqlite_write_error(&ProxyError::Database(
        write_err
    )));

    sqlx::query("ROLLBACK")
        .execute(&mut *immediate_conn)
        .await
        .expect("rollback immediate transaction");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[cfg(unix)]
#[tokio::test]
async fn billing_ledger_audit_reads_read_only_database_copy() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("billing-ledger-audit-read-only-{}", nanoid!(8)));
    fs::create_dir_all(&dir).expect("create temp audit dir");
    let db_path = dir.join("audit.db");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("audit-read-only-copy"))
        .await
        .expect("create token");
    seed_charged_business_attempt(&proxy, &token.id, 2).await;
    drop(proxy);

    let original_dir_mode = fs::metadata(&dir)
        .expect("read dir metadata")
        .permissions()
        .mode();
    let original_db_mode = fs::metadata(&db_path)
        .expect("read db metadata")
        .permissions()
        .mode();

    fs::set_permissions(&db_path, fs::Permissions::from_mode(0o444)).expect("make db read-only");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).expect("make dir read-only");

    let audit_result = audit_business_quota_ledger(&db_str, Utc::now()).await;

    fs::set_permissions(&dir, fs::Permissions::from_mode(original_dir_mode))
        .expect("restore dir permissions");
    fs::set_permissions(&db_path, fs::Permissions::from_mode(original_db_mode))
        .expect("restore db permissions");

    let audit = audit_result.expect("audit read-only database");
    assert_eq!(audit.summary.current_month_charged_rows, 1);
    assert_eq!(audit.summary.current_month_charged_credits, 2);
    assert_eq!(audit.summary.mismatched_subjects, 0);

    let _ = fs::remove_file(&db_path);
    let _ = fs::remove_file(dir.join("audit.db-shm"));
    let _ = fs::remove_file(dir.join("audit.db-wal"));
    let _ = fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn startup_monthly_rebase_skips_legacy_charged_rows_without_billing_subject() {
    let db_path = temp_db_path("startup-monthly-rebase-legacy-billing-subject-gap");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("legacy-billing-subject-gap"))
        .await
        .expect("create token");

    seed_charged_business_attempt(&proxy, &token.id, 3).await;

    let month_count_before_restart: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read token month count before restart");
    assert_eq!(month_count_before_restart, 3);

    sqlx::query(
        r#"
        UPDATE auth_token_logs
        SET billing_subject = NULL
        WHERE token_id = ?
          AND billing_state = ?
          AND COALESCE(business_credits, 0) > 0
        "#,
    )
    .bind(&token.id)
    .bind(BILLING_STATE_CHARGED)
    .execute(&proxy.key_store.pool)
    .await
    .expect("clear billing subject on legacy charged row");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset monthly rebase meta");
    drop(proxy);

    let reopened = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened despite legacy billing_subject gap");

    let month_count_after_restart: i64 =
        sqlx::query_scalar("SELECT month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1")
            .bind(&token.id)
            .fetch_one(&reopened.key_store.pool)
            .await
            .expect("read token month count after restart");
    assert_eq!(month_count_after_restart, month_count_before_restart);

    let rebase_meta: Option<i64> =
        sqlx::query_scalar("SELECT CAST(value AS INTEGER) FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_BUSINESS_QUOTA_MONTHLY_REBASE_V1)
            .fetch_optional(&reopened.key_store.pool)
            .await
            .expect("read monthly rebase meta after skipped startup");
    assert_eq!(rebase_meta, None);

    let audit_err = audit_business_quota_ledger_with_pool(&reopened.key_store.pool, Utc::now())
        .await
        .expect_err("audit should still surface legacy billing_subject gap");
    assert!(is_invalid_current_month_billing_subject_error(&audit_err));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn manual_key_maintenance_actions_append_audit_records() {
    let db_path = temp_db_path("maintenance-manual");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-manual".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, api_key): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");

    proxy
        .key_store
        .quarantine_key_by_id(
            &key_id,
            "/mcp",
            "account_deactivated",
            "Tavily account deactivated (HTTP 401)",
            "deactivated",
        )
        .await
        .expect("seed quarantine");

    proxy
        .clear_key_quarantine_by_id_with_actor(
            &key_id,
            MaintenanceActor {
                auth_token_id: None,
                actor_user_id: Some("user-1".to_string()),
                actor_display_name: Some("Admin One".to_string()),
            },
        )
        .await
        .expect("clear quarantine with audit");
    proxy
        .mark_key_quota_exhausted_by_secret_with_actor(
            &api_key,
            MaintenanceActor {
                auth_token_id: None,
                actor_user_id: Some("user-1".to_string()),
                actor_display_name: Some("Admin One".to_string()),
            },
        )
        .await
        .expect("mark exhausted with audit");

    let rows = sqlx::query_as::<_, (String, Option<String>, Option<String>, i64, i64)>(
        r#"
        SELECT operation_code, actor_user_id, actor_display_name, quarantine_before, quarantine_after
        FROM api_key_maintenance_records
        WHERE key_id = ?
        ORDER BY operation_code ASC
        "#,
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch maintenance rows");

    assert_eq!(rows.len(), 2);
    let clear_row = rows
        .iter()
        .find(|row| row.0 == MAINTENANCE_OP_MANUAL_CLEAR_QUARANTINE)
        .expect("clear quarantine row");
    assert_eq!(clear_row.1.as_deref(), Some("user-1"));
    assert_eq!(clear_row.2.as_deref(), Some("Admin One"));
    assert_eq!(clear_row.3, 1);
    assert_eq!(clear_row.4, 0);
    assert!(
        rows.iter()
            .any(|row| row.0 == MAINTENANCE_OP_MANUAL_MARK_EXHAUSTED)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn manual_key_breakage_fanout_attributes_bound_subjects_as_breakers() {
    let db_path = temp_db_path("manual-breakage-fanout-subject-breakers");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-manual-breakage-fanout-subject-breakers".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "manual-breakage-subject-user".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice Wang".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:manual_breakage_subject"))
        .await
        .expect("bind token");
    let (key_id, api_key): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let now = Utc::now().timestamp();
    let month_start = start_of_month(Utc::now()).timestamp();

    let mut tx = proxy
        .key_store
        .pool
        .begin()
        .await
        .expect("begin binding tx");
    proxy
        .key_store
        .refresh_user_api_key_binding(&mut tx, &user.user_id, &key_id, now)
        .await
        .expect("refresh user key binding");
    proxy
        .key_store
        .refresh_token_api_key_binding(&mut tx, &token.id, &key_id, now)
        .await
        .expect("refresh token key binding");
    tx.commit().await.expect("commit binding tx");

    proxy
        .mark_key_quota_exhausted_by_secret_with_actor(
            &api_key,
            MaintenanceActor {
                auth_token_id: None,
                actor_user_id: Some("admin-user".to_string()),
                actor_display_name: Some("Ops Admin".to_string()),
            },
        )
        .await
        .expect("mark key exhausted");

    let user_page = proxy
        .key_store
        .fetch_monthly_broken_keys_page(BROKEN_KEY_SUBJECT_USER, &user.user_id, 1, 20, month_start)
        .await
        .expect("fetch user monthly broken keys");
    assert!(
        user_page.items.is_empty(),
        "manual quota exhaustion is not upstream-blocked evidence"
    );

    let token_page = proxy
        .key_store
        .fetch_monthly_broken_keys_page(BROKEN_KEY_SUBJECT_TOKEN, &token.id, 1, 20, month_start)
        .await
        .expect("fetch token monthly broken keys");
    assert!(
        token_page.items.is_empty(),
        "manual quota exhaustion is not upstream-blocked evidence"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn restored_keys_do_not_count_as_active_monthly_token_breakage_subjects() {
    let db_path = temp_db_path("monthly-broken-token-restored");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-monthly-broken-token-restored".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let token = proxy
        .create_access_token(Some("monthly-broken-token-restored"))
        .await
        .expect("token");
    let key_id: String = sqlx::query_scalar("SELECT id FROM api_keys LIMIT 1")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("fetch key id");
    let now = Utc::now().timestamp();
    let month_start = start_of_month(Utc::now()).timestamp();

    sqlx::query(
        r#"INSERT INTO subject_key_breakages (
               subject_kind,
               subject_id,
               key_id,
               month_start,
               created_at,
               updated_at,
               latest_break_at,
               key_status,
               reason_code,
               reason_summary,
               source,
               breaker_token_id,
               breaker_user_id,
               breaker_user_display_name,
               manual_actor_display_name
           ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
    )
    .bind(BROKEN_KEY_SUBJECT_TOKEN)
    .bind(&token.id)
    .bind(&key_id)
    .bind(month_start)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(STATUS_EXHAUSTED)
    .bind("manual_mark_exhausted")
    .bind("manually exhausted")
    .bind(BROKEN_KEY_SOURCE_MANUAL)
    .bind(&token.id)
    .bind(Option::<String>::None)
    .bind(Option::<String>::None)
    .bind(Some("Admin One"))
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert subject breakage");

    let counts = proxy
        .key_store
        .fetch_monthly_broken_counts_for_tokens(std::slice::from_ref(&token.id), month_start)
        .await
        .expect("fetch counts");
    assert_eq!(counts.get(&token.id).copied(), None);

    let subjects = proxy
        .key_store
        .list_monthly_broken_subjects_for_tokens(std::slice::from_ref(&token.id), month_start)
        .await
        .expect("fetch subjects");
    assert!(!subjects.contains(&token.id));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn auto_key_health_actions_append_audit_records() {
    let db_path = temp_db_path("maintenance-auto");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-maintenance-auto".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let (key_id, secret): (String, String) =
        sqlx::query_as("SELECT id, api_key FROM api_keys LIMIT 1")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("fetch key");
    let lease = ApiKeyLease {
        id: key_id.clone(),
        secret: secret.clone(),
    };

    let quarantine_effect = proxy
        .reconcile_key_health(
            &lease,
            "/mcp",
            &AttemptAnalysis {
                status: OUTCOME_ERROR,
                tavily_status_code: Some(401),
                key_health_action: KeyHealthAction::Quarantine(QuarantineDecision {
                    reason_code: "account_deactivated".to_string(),
                    reason_summary: "Tavily account deactivated (HTTP 401)".to_string(),
                    reason_detail: "deactivated".to_string(),
                }),
                failure_kind: Some(FAILURE_KIND_UPSTREAM_ACCOUNT_DEACTIVATED_401.to_string()),
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("auto quarantine");
    assert_eq!(quarantine_effect.code, KEY_EFFECT_QUARANTINED);

    proxy
        .key_store
        .mark_quota_exhausted(&secret)
        .await
        .expect("seed exhausted");
    let restore_effect = proxy
        .reconcile_key_health(
            &lease,
            "/api/tavily/search",
            &AttemptAnalysis {
                status: OUTCOME_SUCCESS,
                tavily_status_code: Some(200),
                key_health_action: KeyHealthAction::None,
                failure_kind: None,
                key_effect: KeyEffect::none(),
                api_key_id: Some(key_id.clone()),
            },
            None,
        )
        .await
        .expect("auto restore");
    assert_eq!(restore_effect.code, KEY_EFFECT_RESTORED_ACTIVE);

    let ops = sqlx::query_scalar::<_, String>(
        r#"
        SELECT operation_code
        FROM api_key_maintenance_records
        WHERE key_id = ?
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(&key_id)
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("fetch operation codes");

    assert!(ops.contains(&MAINTENANCE_OP_AUTO_QUARANTINE.to_string()));
    assert!(ops.contains(&MAINTENANCE_OP_AUTO_RESTORE_ACTIVE.to_string()));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_quota1h_uses_historical_limit_snapshots() {
    let db_path = temp_db_path("admin-user-usage-series-quota-limit-snapshots");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-quota-snapshots".to_string(),
            username: Some("usage_series_quota_snapshots".to_string()),
            name: Some("Usage Series Quota Snapshots".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    proxy
        .update_account_business_quota_limits(&user.user_id, 600, 6_000, 60_000)
        .await
        .expect("update current business quota");
    sqlx::query("DELETE FROM account_quota_limit_snapshots WHERE user_id = ?")
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear auto snapshots");

    let now = Utc::now();
    let current_bucket_start = now.timestamp() - now.timestamp().rem_euclid(SECS_PER_HOUR);
    let start = current_bucket_start - 71 * SECS_PER_HOUR;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    sqlx::query(
        r#"INSERT INTO account_quota_limit_snapshots
           (user_id, changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit)
           VALUES (?, ?, ?, ?, ?, ?), (?, ?, ?, ?, ?, ?), (?, ?, ?, ?, ?, ?)"#,
    )
    .bind(&user.user_id)
    .bind(start + 12 * SECS_PER_HOUR + 600)
    .bind(200)
    .bind(200)
    .bind(2_000)
    .bind(20_000)
    .bind(&user.user_id)
    .bind(start + 36 * SECS_PER_HOUR + 600)
    .bind(400)
    .bind(400)
    .bind(4_000)
    .bind(40_000)
    .bind(&user.user_id)
    .bind(start + 60 * SECS_PER_HOUR + 600)
    .bind(600)
    .bind(600)
    .bind(6_000)
    .bind(60_000)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed deterministic quota snapshots");

    proxy
        .key_store
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_QUOTA1H_COVERAGE_START, start)
        .await
        .expect("set quota1h coverage");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Quota1h)
        .await
        .expect("load quota1h series");

    assert_eq!(series.limit, 600);
    assert_eq!(series.points.len(), 72);
    assert_eq!(series.points[11].limit_value, None);
    assert_eq!(series.points[12].limit_value, Some(200));
    assert_eq!(series.points[35].limit_value, Some(200));
    assert_eq!(series.points[36].limit_value, Some(400));
    assert_eq!(series.points[59].limit_value, Some(400));
    assert_eq!(series.points[60].limit_value, Some(600));
    assert!(series.points.iter().all(|point| point.value == Some(0)));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn admin_user_usage_series_rate5m_uses_historical_request_limit_snapshots() {
    let db_path = temp_db_path("admin-user-usage-series-rate-limit-snapshots");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "usage-series-rate-snapshots".to_string(),
            username: Some("usage_series_rate_snapshots".to_string()),
            name: Some("Usage Series Rate Snapshots".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let mut settings = proxy.get_system_settings().await.expect("get system settings");
    settings.request_rate_limit = 120;
    proxy
        .set_system_settings(&settings)
        .await
        .expect("set current request rate");
    sqlx::query("DELETE FROM request_rate_limit_snapshots")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear auto request snapshots");

    let now = Utc::now();
    let current_bucket_start =
        now.timestamp() - now.timestamp().rem_euclid(SECS_PER_FIVE_MINUTES);
    let start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");
    sqlx::query(
        r#"INSERT INTO request_rate_limit_snapshots (changed_at, limit_value)
           VALUES (?, ?), (?, ?)"#,
    )
    .bind(start + 48 * SECS_PER_FIVE_MINUTES + 120)
    .bind(80)
    .bind(start + 200 * SECS_PER_FIVE_MINUTES + 120)
    .bind(120)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed deterministic request rate snapshots");

    proxy
        .key_store
        .set_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START, start)
        .await
        .expect("set rate5m coverage");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load rate5m series");

    assert_eq!(series.limit, 120);
    assert_eq!(series.points.len(), 288);
    assert_eq!(series.points[47].limit_value, None);
    assert_eq!(series.points[48].limit_value, Some(80));
    assert_eq!(series.points[199].limit_value, Some(80));
    assert_eq!(series.points[200].limit_value, Some(120));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_preserves_request_time_user_binding() {
    let db_path = temp_db_path("account-usage-rollup-request-user-snapshot");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let first_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-user-snapshot-a".to_string(),
            username: Some("request_user_snapshot_a".to_string()),
            name: Some("Request User Snapshot A".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert first user");
    let second_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "request-user-snapshot-b".to_string(),
            username: Some("request_user_snapshot_b".to_string()),
            name: Some("Request User Snapshot B".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert second user");
    let token = proxy
        .ensure_user_token_binding(&first_user.user_id, Some("request-user-snapshot"))
        .await
        .expect("bind token to first user");

    proxy
        .record_token_attempt(
            &token.id,
            &Method::GET,
            "/search",
            Some("q=request-user-snapshot"),
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
        )
        .await
        .expect("record token attempt");

    let (created_at, request_user_id): (i64, Option<String>) = sqlx::query_as(
        r#"
        SELECT created_at, request_user_id
        FROM auth_token_logs
        WHERE token_id = ?
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read stored request owner snapshot");
    assert_eq!(request_user_id.as_deref(), Some(first_user.user_id.as_str()));

    sqlx::query(
        r#"
        UPDATE user_token_bindings
        SET user_id = ?, updated_at = ?
        WHERE token_id = ?
        "#,
    )
    .bind(&second_user.user_id)
    .bind(created_at + 1)
    .bind(&token.id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("rebind token to second user");
    proxy
        .key_store
        .cache_token_binding(&token.id, Some(&second_user.user_id))
        .await;

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
    let first_user_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &first_user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load first user rollups");
    let second_user_values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &second_user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load second user rollups");

    assert_eq!(first_user_values.get(&bucket_start), Some(&1));
    assert_eq!(second_user_values.get(&bucket_start), None);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_uses_current_binding_for_pre_migration_requests() {
    let db_path = temp_db_path("account-usage-rollup-pre-migration-binding-fallback");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "pre-migration-binding-fallback".to_string(),
            username: Some("pre_migration_binding_fallback".to_string()),
            name: Some("Pre Migration Binding Fallback".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("pre-migration-binding"))
        .await
        .expect("bind token");

    proxy
        .record_token_attempt(
            &token.id,
            &Method::GET,
            "/search",
            Some("q=pre-migration-binding"),
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
        )
        .await
        .expect("record token attempt");

    let created_at: i64 = sqlx::query_scalar(
        r#"
        SELECT created_at
        FROM auth_token_logs
        WHERE token_id = ?
        ORDER BY id DESC
        LIMIT 1
        "#,
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load pre-migration request log");
    sqlx::query(
        r#"
        UPDATE auth_token_logs
        SET request_user_id = NULL,
            billing_subject = NULL
        WHERE token_id = ?
        "#,
    )
    .bind(&token.id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("strip request-time ownership to simulate pre-migration rows");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let bucket_start = created_at - created_at.rem_euclid(SECS_PER_FIVE_MINUTES);
    let values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            bucket_start,
            bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load rebuilt request rollups");

    assert_eq!(values.get(&bucket_start), Some(&1));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_zero_fills_inactive_rate5m_window() {
    let db_path = temp_db_path("account-usage-rollup-empty-rate5m-window");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "empty-rate5m-window".to_string(),
            username: Some("empty_rate5m_window".to_string()),
            name: Some("Empty Rate5m Window".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let now = Utc::now().timestamp();
    let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
    let window_start = current_bucket_start - 287 * SECS_PER_FIVE_MINUTES;
    sqlx::query("UPDATE users SET created_at = ? WHERE id = ?")
        .bind(window_start)
        .bind(&user.user_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("backdate user creation");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let series = proxy
        .admin_user_usage_series(&user.user_id, AdminUserUsageSeriesKind::Rate5m)
        .await
        .expect("load empty rate5m series");

    assert_eq!(series.points.len(), 288);
    assert!(series.points.iter().all(|point| point.value == Some(0)));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_usage_rollup_rebuild_clears_stale_rate5m_buckets_without_logs() {
    let db_path = temp_db_path("account-usage-rollup-clears-stale-rate5m");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "github".to_string(),
            provider_user_id: "clears-stale-rate5m".to_string(),
            username: Some("clears_stale_rate5m".to_string()),
            name: Some("Clears Stale Rate5m".to_string()),
            avatar_template: None,
            active: true,
            trust_level: None,
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");

    let stale_bucket_start = {
        let now = Utc::now().timestamp();
        let current_bucket_start = now - now.rem_euclid(SECS_PER_FIVE_MINUTES);
        current_bucket_start - SECS_PER_FIVE_MINUTES
    };
    sqlx::query(
        r#"
        INSERT INTO account_usage_rollup_buckets (
            user_id,
            metric_kind,
            bucket_kind,
            bucket_start,
            value,
            updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&user.user_id)
    .bind(AccountUsageRollupMetricKind::RequestCount.as_str())
    .bind(AccountUsageRollupBucketKind::FiveMinute.as_str())
    .bind(stale_bucket_start)
    .bind(9_i64)
    .bind(Utc::now().timestamp())
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert stale rate5m rollup");

    proxy
        .key_store
        .rebuild_account_usage_rollup_buckets_v1()
        .await
        .expect("rebuild account usage rollups");

    let values = proxy
        .key_store
        .fetch_account_usage_rollup_values(
            &user.user_id,
            AccountUsageRollupMetricKind::RequestCount,
            AccountUsageRollupBucketKind::FiveMinute,
            stale_bucket_start,
            stale_bucket_start + SECS_PER_FIVE_MINUTES,
        )
        .await
        .expect("load rebuilt stale bucket");
    assert!(values.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_reuses_existing_binding() {
    let db_path = temp_db_path("user-token-binding-reuse");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "alice-uid".to_string(),
            username: Some("alice".to_string()),
            name: Some("Alice".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alice");

    let first = proxy
        .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
        .await
        .expect("bind alice first");
    let second = proxy
        .ensure_user_token_binding(&alice.user_id, Some("linuxdo:alice"))
        .await
        .expect("bind alice second");

    assert_eq!(
        first.id, second.id,
        "same user should reuse one token binding"
    );
    assert_eq!(first.token, second.token);

    let bob = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "bob-uid".to_string(),
            username: Some("bob".to_string()),
            name: Some("Bob".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert bob");
    let bob_token = proxy
        .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob"))
        .await
        .expect("bind bob");

    assert_ne!(
        first.id, bob_token.id,
        "different users must not share the same token binding"
    );

    let store = proxy.key_store.clone();
    let (alice_bindings,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
            .bind(&alice.user_id)
            .fetch_one(&store.pool)
            .await
            .expect("count alice bindings");
    assert_eq!(
        alice_bindings, 1,
        "alice should have exactly one binding row"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_with_preferred_keeps_existing_binding_and_adds_preferred() {
    let db_path = temp_db_path("user-token-binding-preferred-rebind");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-rebind-user".to_string(),
            username: Some("preferred_rebind".to_string()),
            name: Some("Preferred Rebind".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let original = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:preferred_rebind"))
        .await
        .expect("ensure initial binding");
    let mistaken = proxy
        .create_access_token(Some("linuxdo:mistaken"))
        .await
        .expect("create mistaken token");

    let store = proxy.key_store.clone();
    sqlx::query("UPDATE user_token_bindings SET token_id = ?, updated_at = ? WHERE user_id = ?")
        .bind(&mistaken.id)
        .bind(Utc::now().timestamp() - 30)
        .bind(&user.user_id)
        .execute(&store.pool)
        .await
        .expect("simulate mistaken binding");

    let rebound = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:preferred_rebind"),
            Some(&original.id),
        )
        .await
        .expect("rebind preferred token");

    assert_eq!(
        rebound.id, original.id,
        "preferred token should be bound to the user"
    );

    let (binding_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
            .bind(&user.user_id)
            .fetch_one(&store.pool)
            .await
            .expect("count user bindings");
    assert_eq!(
        binding_count, 2,
        "preferred binding should be added without removing existing token"
    );

    let preferred_owner = sqlx::query_scalar::<_, Option<String>>(
        "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
    )
    .bind(&original.id)
    .fetch_optional(&store.pool)
    .await
    .expect("query preferred owner")
    .flatten();
    assert_eq!(
        preferred_owner.as_deref(),
        Some(user.user_id.as_str()),
        "preferred token should belong to the user"
    );

    let mistaken_owner = sqlx::query_scalar::<_, Option<String>>(
        "SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1",
    )
    .bind(&mistaken.id)
    .fetch_optional(&store.pool)
    .await
    .expect("query mistaken token owner")
    .flatten();
    assert_eq!(
        mistaken_owner.as_deref(),
        Some(user.user_id.as_str()),
        "existing token must stay bound to the same user"
    );

    let primary = proxy
        .get_user_token(&user.user_id)
        .await
        .expect("query primary user token");
    match primary {
        UserTokenLookup::Found(secret) => assert_eq!(
            secret.id, original.id,
            "latest preferred binding should be selected as primary token"
        ),
        other => panic!("expected found user token, got {other:?}"),
    }

    let (enabled, deleted_at): (i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, deleted_at FROM auth_tokens WHERE id = ? LIMIT 1")
            .bind(&mistaken.id)
            .fetch_one(&store.pool)
            .await
            .expect("query mistaken token state");
    assert_eq!(enabled, 1, "mistaken token should remain active");
    assert!(
        deleted_at.is_none(),
        "mistaken token should not be soft-deleted"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_with_preferred_falls_back_when_preferred_owned_by_other_user() {
    let db_path = temp_db_path("user-token-binding-preferred-conflict");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let alice = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-conflict-alice".to_string(),
            username: Some("alice_conflict".to_string()),
            name: Some("Alice Conflict".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alice");
    let bob = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-conflict-bob".to_string(),
            username: Some("bob_conflict".to_string()),
            name: Some("Bob Conflict".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert bob");
    let bob_token = proxy
        .ensure_user_token_binding(&bob.user_id, Some("linuxdo:bob_conflict"))
        .await
        .expect("ensure bob token");

    let alice_result = proxy
        .ensure_user_token_binding_with_preferred(
            &alice.user_id,
            Some("linuxdo:alice_conflict"),
            Some(&bob_token.id),
        )
        .await
        .expect("fallback binding for alice");

    assert_ne!(
        alice_result.id, bob_token.id,
        "preferred token owned by other user must not be rebound"
    );

    let store = proxy.key_store.clone();
    let (owner,): (String,) =
        sqlx::query_as("SELECT user_id FROM user_token_bindings WHERE token_id = ?")
            .bind(&bob_token.id)
            .fetch_one(&store.pool)
            .await
            .expect("query bob token owner");
    assert_eq!(
        owner, bob.user_id,
        "conflicting token owner must remain unchanged"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ensure_user_token_binding_with_preferred_falls_back_when_preferred_unavailable() {
    let db_path = temp_db_path("user-token-binding-preferred-unavailable");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "preferred-unavailable-user".to_string(),
            username: Some("preferred_unavailable".to_string()),
            name: Some("Preferred Unavailable".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let original = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:preferred_unavailable"))
        .await
        .expect("ensure original binding");
    let disabled = proxy
        .create_access_token(Some("linuxdo:disabled_preferred"))
        .await
        .expect("create disabled preferred token");
    proxy
        .set_access_token_enabled(&disabled.id, false)
        .await
        .expect("disable preferred token");

    let fallback_disabled = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:preferred_unavailable"),
            Some(&disabled.id),
        )
        .await
        .expect("fallback when preferred disabled");
    assert_eq!(
        fallback_disabled.id, original.id,
        "disabled preferred token should be ignored"
    );

    let deleted = proxy
        .create_access_token(Some("linuxdo:deleted_preferred"))
        .await
        .expect("create deleted preferred token");
    proxy
        .delete_access_token(&deleted.id)
        .await
        .expect("soft delete preferred token");

    let fallback_deleted = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:preferred_unavailable"),
            Some(&deleted.id),
        )
        .await
        .expect("fallback when preferred deleted");
    assert_eq!(
        fallback_deleted.id, original.id,
        "soft-deleted preferred token should be ignored"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn force_user_relogin_migration_revokes_existing_sessions_once() {
    let db_path = temp_db_path("force-user-relogin-v1");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "force-relogin-user".to_string(),
            username: Some("force_relogin".to_string()),
            name: Some("Force Relogin".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let session = proxy
        .create_user_session(&user, 3600)
        .await
        .expect("create session");

    let store = proxy.key_store.clone();
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_FORCE_USER_RELOGIN_V1)
        .execute(&store.pool)
        .await
        .expect("delete relogin migration meta key");
    drop(proxy);

    let _proxy_after_restart =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy restarted");

    let revoked_at = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT revoked_at FROM user_sessions WHERE token = ? LIMIT 1",
    )
    .bind(&session.token)
    .fetch_optional(&store.pool)
    .await
    .expect("query session after restart")
    .flatten();
    assert!(
        revoked_at.is_some(),
        "existing sessions must be revoked by one-time relogin migration"
    );

    let relogin_migration_mark =
        sqlx::query_scalar::<_, Option<String>>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(META_KEY_FORCE_USER_RELOGIN_V1)
            .fetch_optional(&store.pool)
            .await
            .expect("query relogin migration mark")
            .flatten();
    assert!(
        relogin_migration_mark.is_some(),
        "relogin migration must record one-time completion mark"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn user_token_bindings_migration_supports_multi_binding_without_backfill() {
    let db_path = temp_db_path("user-token-bindings-multi-binding-migration");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-binding-user".to_string(),
            username: Some("legacy_binding_user".to_string()),
            name: Some("Legacy Binding User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert legacy user");
    let legacy = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_binding_user"))
        .await
        .expect("create legacy binding");
    drop(proxy);

    let options = SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open db pool");

    let legacy_row = sqlx::query_as::<_, (String, String, i64, i64)>(
        "SELECT user_id, token_id, created_at, updated_at FROM user_token_bindings WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_one(&pool)
    .await
    .expect("read legacy binding row");
    sqlx::query("DROP TABLE user_token_bindings")
        .execute(&pool)
        .await
        .expect("drop user_token_bindings");
    sqlx::query(
        r#"
        CREATE TABLE user_token_bindings (
            user_id TEXT PRIMARY KEY,
            token_id TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id),
            FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("recreate legacy user_token_bindings");
    sqlx::query(
        "INSERT INTO user_token_bindings (user_id, token_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&legacy_row.0)
    .bind(&legacy_row.1)
    .bind(legacy_row.2)
    .bind(legacy_row.3)
    .execute(&pool)
    .await
    .expect("insert legacy binding row");
    drop(pool);

    let proxy_after_restart =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy restarted");
    let preferred = proxy_after_restart
        .create_access_token(Some("linuxdo:preferred_after_migration"))
        .await
        .expect("create preferred token");
    proxy_after_restart
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:legacy_binding_user"),
            Some(&preferred.id),
        )
        .await
        .expect("bind preferred token after migration");

    let store = proxy_after_restart.key_store.clone();
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM user_token_bindings WHERE user_id = ?")
            .bind(&user.user_id)
            .fetch_one(&store.pool)
            .await
            .expect("count user bindings after migration");
    assert_eq!(
        count, 2,
        "migrated schema should allow multiple token bindings per user"
    );

    let owners = sqlx::query_as::<_, (String, String)>(
        "SELECT token_id, user_id FROM user_token_bindings WHERE user_id = ? ORDER BY token_id ASC",
    )
    .bind(&user.user_id)
    .fetch_all(&store.pool)
    .await
    .expect("query owners after migration");
    assert!(
        owners
            .iter()
            .any(|(token_id, owner)| token_id == &legacy.id && owner == &user.user_id),
        "legacy binding should be preserved"
    );
    assert!(
        owners
            .iter()
            .any(|(token_id, owner)| token_id == &preferred.id && owner == &user.user_id),
        "preferred binding should be added"
    );

    let _ = std::fs::remove_file(db_path);
}
#[tokio::test]
async fn get_user_token_returns_unavailable_after_soft_delete() {
    let db_path = temp_db_path("user-token-unavailable");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "charlie-uid".to_string(),
            username: Some("charlie".to_string()),
            name: Some("Charlie".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert charlie");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:charlie"))
        .await
        .expect("bind charlie");

    let before = proxy
        .get_user_token(&user.user_id)
        .await
        .expect("lookup user token before delete");
    assert!(
        matches!(before, UserTokenLookup::Found(_)),
        "token should be available before delete"
    );

    proxy
        .delete_access_token(&token.id)
        .await
        .expect("soft delete token");

    let after = proxy
        .get_user_token(&user.user_id)
        .await
        .expect("lookup user token after delete");
    assert!(
        matches!(after, UserTokenLookup::Unavailable),
        "soft-deleted binding should report unavailable"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn get_user_token_secret_returns_none_when_token_disabled() {
    let db_path = temp_db_path("user-token-secret-disabled");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "disabled-secret-user".to_string(),
            username: Some("disabled_secret_user".to_string()),
            name: Some("Disabled Secret User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(0),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:disabled_secret_user"))
        .await
        .expect("bind token");

    let before = proxy
        .get_user_token_secret(&user.user_id, &token.id)
        .await
        .expect("secret before disable");
    assert!(before.is_some(), "enabled token should expose secret");

    proxy
        .set_access_token_enabled(&token.id, false)
        .await
        .expect("disable token");

    let after = proxy
        .get_user_token_secret(&user.user_id, &token.id)
        .await
        .expect("secret after disable");
    assert!(after.is_none(), "disabled token should not expose secret");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_for_previous_subject_stays_pending_after_token_binding_changes_subject() {
    let db_path = temp_db_path("pending-billing-subject-flip");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-subject-flip"))
        .await
        .expect("create token");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("simulated pending charge"),
            3,
            None,
        )
        .await
        .expect("record pending billing attempt");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-subject-user".to_string(),
            username: Some("pending_billing_subject".to_string()),
            name: Some("Pending Billing Subject".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:pending_billing_subject"),
            Some(&token.id),
        )
        .await
        .expect("bind existing token to user");

    let _guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("reconcile pending billing after subject flip");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(token_minute_sum, 3);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_request_log_metadata_persists_binding_and_selection_effects() {
    let db_path = temp_db_path("pending-billing-effect-metadata");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-effect-metadata"))
        .await
        .expect("create token");
    let key_id = proxy
        .add_or_undelete_key("tvly-pending-billing-effect-metadata")
        .await
        .expect("create api key");

    let first_log_id = proxy
        .record_pending_billing_attempt_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
            1,
            Some(&key_id),
            None,
            Some(KEY_EFFECT_NONE),
            None,
            Some(KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND),
            Some("bound"),
            Some(KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED),
            Some("pressure avoided"),
            None,
        )
        .await
        .expect("record pending billing with request metadata");

    let second_log_id = proxy
        .record_pending_billing_attempt_for_subject_request_log_metadata(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            None,
            1,
            "user:test-user",
            Some(&key_id),
            None,
            Some(KEY_EFFECT_NONE),
            None,
            Some(KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND),
            Some("rebound"),
            Some(KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED),
            Some("cooldown avoided"),
            None,
        )
        .await
        .expect("record pending billing with subject metadata");

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open sqlite pool");

    for (log_id, expected_binding, expected_selection) in [
        (
            first_log_id,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_BOUND,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_PRESSURE_AVOIDED,
        ),
        (
            second_log_id,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_REBOUND,
            KEY_EFFECT_HTTP_PROJECT_AFFINITY_COOLDOWN_AVOIDED,
        ),
    ] {
        let row = sqlx::query(
            r#"
            SELECT key_effect_code, binding_effect_code, selection_effect_code
            FROM auth_token_logs
            WHERE id = ?
            "#,
        )
        .bind(log_id)
        .fetch_one(&pool)
        .await
        .expect("read pending billing token log");
        assert_eq!(
            row.try_get::<String, _>("key_effect_code")
                .expect("key effect code"),
            KEY_EFFECT_NONE
        );
        assert_eq!(
            row.try_get::<String, _>("binding_effect_code")
                .expect("binding effect code"),
            expected_binding
        );
        assert_eq!(
            row.try_get::<String, _>("selection_effect_code")
                .expect("selection effect code"),
            expected_selection
        );
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn startup_migration_preserves_legacy_mcp_session_retry_key_effects() {
    let db_path = temp_db_path("mcp-session-retry-effect-migration");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("mcp-session-retry-effect-migration"))
        .await
        .expect("create token");
    drop(proxy);

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open sqlite pool");

    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            auth_token_id,
            method,
            path,
            result_status,
            key_effect_code,
            key_effect_summary,
            visibility,
            created_at
        ) VALUES (?, 'POST', '/mcp', 'success', 'mcp_session_retry_waited', 'legacy retry waited', 'visible', ?)
        "#,
    )
    .bind(&token.id)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert legacy request log");

    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id,
            method,
            path,
            result_status,
            key_effect_code,
            key_effect_summary,
            created_at
        ) VALUES (?, 'POST', '/mcp', 'success', 'mcp_session_retry_scheduled', 'legacy retry scheduled', ?)
        "#,
    )
    .bind(&token.id)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert legacy token log");
    drop(pool);

    let migrated_proxy =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened for migration");

    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db_str)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("reopen sqlite pool");

    let request_row = sqlx::query(
        "SELECT key_effect_code, key_effect_summary FROM request_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("read migrated request log");
    assert_eq!(
        request_row
            .try_get::<String, _>("key_effect_code")
            .expect("request key effect"),
        KEY_EFFECT_MCP_SESSION_RETRY_WAITED
    );
    assert_eq!(
        request_row
            .try_get::<Option<String>, _>("key_effect_summary")
            .expect("request key effect summary"),
        Some("legacy retry waited".to_string())
    );

    let token_row = sqlx::query(
        "SELECT key_effect_code, key_effect_summary FROM auth_token_logs ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .expect("read migrated token log");
    assert_eq!(
        token_row
            .try_get::<String, _>("key_effect_code")
            .expect("token key effect"),
        KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED
    );
    assert_eq!(
        token_row
            .try_get::<Option<String>, _>("key_effect_summary")
            .expect("token key effect summary"),
        Some("legacy retry scheduled".to_string())
    );

    let request_logs_catalog = migrated_proxy
        .request_logs_catalog(&[], None, None, None, None, None, None, None)
        .await
        .expect("load request logs catalog");
    assert!(
        request_logs_catalog
            .facets
            .key_effects
            .iter()
            .any(|option| option.value == KEY_EFFECT_MCP_SESSION_RETRY_WAITED),
        "legacy retry waited effect should remain queryable in request log facets"
    );

    let token_logs_catalog = migrated_proxy
        .token_logs_catalog(&token.id, 0, None, &[], None, None, None, None, None, None)
        .await
        .expect("load token logs catalog");
    assert!(
        token_logs_catalog
            .facets
            .key_effects
            .iter()
            .any(|option| option.value == KEY_EFFECT_MCP_SESSION_RETRY_SCHEDULED),
        "legacy retry scheduled effect should remain queryable in token log facets"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_for_previous_account_subject_stays_pending_after_token_becomes_unbound() {
    let db_path = temp_db_path("pending-billing-account-to-token-subject-flip");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-account-user".to_string(),
            username: Some("pending_billing_account".to_string()),
            name: Some("Pending Billing Account".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:pending_billing_account"))
        .await
        .expect("bind token");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("simulated pending charge"),
            4,
            None,
        )
        .await
        .expect("record pending billing attempt");

    sqlx::query("DELETE FROM user_token_bindings WHERE token_id = ?")
        .bind(&token.id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("unbind token");
    proxy.key_store.cache_token_binding(&token.id, None).await;

    let _guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("reconcile pending billing after unbind");

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(account_minute_sum, 4);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn locked_billing_subject_keeps_original_precheck_after_binding_change() {
    let db_path = temp_db_path("locked-billing-subject-precheck");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("locked-billing-subject-precheck"))
        .await
        .expect("create token");
    proxy
        .charge_token_quota(&token.id, 1)
        .await
        .expect("seed token quota before binding change");

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing");
    assert_eq!(guard.billing_subject(), format!("token:{}", token.id));

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "locked-billing-subject-precheck-user".to_string(),
            username: Some("locked_billing_precheck".to_string()),
            name: Some("Locked Billing Precheck".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:locked_billing_precheck"),
            Some(&token.id),
        )
        .await
        .expect("bind existing token to user");

    let locked_verdict = proxy
        .peek_token_quota_for_subject(guard.billing_subject())
        .await
        .expect("peek locked subject quota");
    assert_eq!(locked_verdict.hourly_used, 1);

    let current_verdict = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek current token quota");
    assert_eq!(current_verdict.hourly_used, 0);

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_attempt_for_subject_charges_original_subject_after_binding_change() {
    let db_path = temp_db_path("pending-billing-for-subject");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-for-subject"))
        .await
        .expect("create token");

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-for-subject-user".to_string(),
            username: Some("pending_billing_subject_charge".to_string()),
            name: Some("Pending Billing Subject Charge".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:pending_billing_subject_charge"),
            Some(&token.id),
        )
        .await
        .expect("bind existing token to user");

    let log_id = proxy
        .record_pending_billing_attempt_for_subject(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("subject pinned to original token"),
            2,
            guard.billing_subject(),
            None,
        )
        .await
        .expect("record pending billing attempt with pinned subject");
    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle pending billing attempt");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(token_minute_sum, 2);

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(account_minute_sum, 0);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn lock_token_billing_uses_fresh_binding_after_external_rebind() {
    let db_path = temp_db_path("lock-token-billing-fresh-binding");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");
    let token = proxy_a
        .create_access_token(Some("fresh-binding-rebind"))
        .await
        .expect("create token");

    // Warm proxy_a's cache with the old unbound subject first.
    let initial = proxy_a
        .peek_token_quota(&token.id)
        .await
        .expect("peek unbound quota");
    assert_eq!(initial.hourly_used, 0);

    let user = proxy_b
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "fresh-binding-user".to_string(),
            username: Some("fresh_binding_user".to_string()),
            name: Some("Fresh Binding User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy_b
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:fresh_binding_user"),
            Some(&token.id),
        )
        .await
        .expect("bind token on proxy b");

    let guard = proxy_a
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing after external rebind");
    assert_eq!(guard.billing_subject(), format!("account:{}", user.user_id));

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn settling_successful_account_billing_resyncs_primary_affinity_to_the_success_key() {
    let db_path = temp_db_path("pending-billing-success-resyncs-primary-affinity");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(
        vec![
            "tvly-pending-billing-primary-a".to_string(),
            "tvly-pending-billing-primary-b".to_string(),
        ],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-primary-user".to_string(),
            username: Some("pending_billing_primary_user".to_string()),
            name: Some("Pending Billing Primary User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding_with_preferred(
            &user.user_id,
            Some("linuxdo:pending_billing_primary_user"),
            None,
        )
        .await
        .expect("bind token to user");

    let key_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM api_keys ORDER BY id ASC")
        .fetch_all(&proxy.key_store.pool)
        .await
        .expect("load key ids");
    let old_key_id = key_ids[0].clone();
    let success_key_id = key_ids[1].clone();
    proxy
        .key_store
        .sync_user_primary_api_key_affinity(&user.user_id, &old_key_id)
        .await
        .expect("seed stale primary affinity");

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("lock token billing");
    assert_eq!(guard.billing_subject(), format!("account:{}", user.user_id));

    let log_id = proxy
        .record_pending_billing_attempt_for_subject(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("success on a newer key"),
            2,
            guard.billing_subject(),
            Some(&success_key_id),
        )
        .await
        .expect("record pending billing attempt");
    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle pending billing attempt");

    let user_primary: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM user_primary_api_key_affinity
           WHERE user_id = ?
           LIMIT 1"#,
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load user primary affinity");
    assert_eq!(user_primary, success_key_id);

    let token_primary: String = sqlx::query_scalar(
        r#"SELECT api_key_id
           FROM token_primary_api_key_affinity
           WHERE token_id = ?
           LIMIT 1"#,
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("load token primary affinity");
    assert_eq!(token_primary, success_key_id);

    drop(guard);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_replay_does_not_backfill_previous_month_into_current_token_quota() {
    let db_path = temp_db_path("pending-billing-token-old-month");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-token-old-month"))
        .await
        .expect("create token");

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let previous_month_ts = current_month_start - 60;

    sqlx::query(
        "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&token.id)
    .bind(current_month_start)
    .bind(7_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed current token month");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("previous month token charge"),
            3,
            None,
        )
        .await
        .expect("record pending token billing");
    sqlx::query("UPDATE auth_token_logs SET created_at = ? WHERE id = ?")
        .bind(previous_month_ts)
        .bind(log_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("rewrite token log timestamp");

    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle previous month token billing");

    let token_month: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ? LIMIT 1",
    )
    .bind(&token.id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token monthly quota");
    assert_eq!(token_month, (current_month_start, 7));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_replay_does_not_backfill_previous_month_into_current_account_quota() {
    let db_path = temp_db_path("pending-billing-account-old-month");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "pending-billing-account-old-month-user".to_string(),
            username: Some("pending_billing_account_old_month".to_string()),
            name: Some("Pending Billing Account Old Month".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(
            &user.user_id,
            Some("linuxdo:pending_billing_account_old_month"),
        )
        .await
        .expect("bind token");

    let current_month_start = start_of_month(Utc::now()).timestamp();
    let previous_month_ts = current_month_start - 60;

    sqlx::query(
        "INSERT INTO account_monthly_quota (user_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(current_month_start)
    .bind(11_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed current account month");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("previous month account charge"),
            4,
            None,
        )
        .await
        .expect("record pending account billing");
    sqlx::query("UPDATE auth_token_logs SET created_at = ? WHERE id = ?")
        .bind(previous_month_ts)
        .bind(log_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("rewrite account log timestamp");

    proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("settle previous month account billing");

    let account_month: (i64, i64) = sqlx::query_as(
        "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ? LIMIT 1",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account monthly quota");
    assert_eq!(account_month, (current_month_start, 11));

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn settle_pending_billing_attempt_is_idempotent_across_instances() {
    let db_path = temp_db_path("pending-billing-idempotent-settle");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");
    let token = proxy_a
        .create_access_token(Some("pending-billing-idempotent-settle"))
        .await
        .expect("create token");

    let log_id = proxy_a
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("concurrent settle"),
            5,
            None,
        )
        .await
        .expect("record pending billing attempt");

    let settle_a = tokio::spawn(async move {
        proxy_a
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle on proxy a");
    });
    let proxy_b_settle = proxy_b.clone();
    let settle_b = tokio::spawn(async move {
        proxy_b_settle
            .settle_pending_billing_attempt(log_id)
            .await
            .expect("settle on proxy b");
    });

    tokio::try_join!(settle_a, settle_b).expect("join settle tasks");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_b.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(token_minute_sum, 5);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy_b.key_store.pool)
            .await
            .expect("read billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn pending_billing_claim_miss_is_retry_later_until_next_replay() {
    let db_path = temp_db_path("pending-billing-claim-miss-retry");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let token = proxy
        .create_access_token(Some("pending-billing-claim-miss-retry"))
        .await
        .expect("create token");

    let log_id = proxy
        .record_pending_billing_attempt(
            &token.id,
            &Method::POST,
            "/api/tavily/search",
            None,
            Some(StatusCode::OK.as_u16() as i64),
            Some(200),
            true,
            OUTCOME_SUCCESS,
            Some("forced claim miss"),
            3,
            None,
        )
        .await
        .expect("record pending billing attempt");

    proxy.force_pending_billing_claim_miss_once(log_id).await;

    let outcome = proxy
        .settle_pending_billing_attempt(log_id)
        .await
        .expect("forced claim miss should surface retry-later outcome");
    assert_eq!(outcome, PendingBillingSettleOutcome::RetryLater);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read pending billing state");
    assert_eq!(billing_state, BILLING_STATE_PENDING);

    let verdict = proxy.peek_token_quota(&token.id).await.expect("peek quota");
    assert_eq!(verdict.hourly_used, 0);

    let guard = proxy
        .lock_token_billing(&token.id)
        .await
        .expect("next billing lock should replay the pending charge before precheck");
    drop(guard);

    let billing_state: String =
        sqlx::query_scalar("SELECT billing_state FROM auth_token_logs WHERE id = ? LIMIT 1")
            .bind(log_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read charged billing state");
    assert_eq!(billing_state, BILLING_STATE_CHARGED);

    let verdict = proxy
        .peek_token_quota(&token.id)
        .await
        .expect("peek quota after replay");
    assert_eq!(verdict.hourly_used, 3);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn token_billing_lock_serializes_across_proxy_instances() {
    let db_path = temp_db_path("billing-lock-cross-instance");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");
    let token = proxy_a
        .create_access_token(Some("billing-lock-cross-instance"))
        .await
        .expect("create token");

    let guard = proxy_a
        .lock_token_billing(&token.id)
        .await
        .expect("acquire first billing lock");

    let token_id = token.id.clone();
    let waiter = tokio::spawn(async move {
        let _guard = proxy_b
            .lock_token_billing(&token_id)
            .await
            .expect("acquire second billing lock");
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !waiter.is_finished(),
        "second proxy instance should wait for the shared billing lock"
    );

    drop(guard);
    tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("second proxy acquires after release")
        .expect("waiter joins");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn research_usage_lock_serializes_across_proxy_instances() {
    let db_path = temp_db_path("research-usage-cross-instance-lock");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy_a = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy a created");
    let proxy_b = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy b created");

    let guard = proxy_a
        .lock_research_key_usage("shared-upstream-key")
        .await
        .expect("acquire first research lock");

    let waiter = tokio::spawn(async move {
        let _guard = proxy_b
            .lock_research_key_usage("shared-upstream-key")
            .await
            .expect("acquire second research lock");
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !waiter.is_finished(),
        "second proxy instance should wait for the shared research lock"
    );

    drop(guard);
    tokio::time::timeout(Duration::from_secs(2), waiter)
        .await
        .expect("second proxy acquires after release")
        .expect("waiter joins");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn bound_token_quota_checks_use_account_counters() {
    let db_path = temp_db_path("bound-token-account-quota");
    let db_str = db_path.to_string_lossy().to_string();

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "quota-user".to_string(),
            username: Some("quota_user".to_string()),
            name: Some("Quota User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:quota_user"))
        .await
        .expect("bind token");

    proxy
        .charge_token_quota(&token.id, 2)
        .await
        .expect("charge business quota credits");

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(
        account_minute_sum, 2,
        "account buckets should count charged credits"
    );

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(
        token_minute_sum, 0,
        "bound token should no longer mutate token-level buckets"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn business_quota_credits_cutover_preserves_existing_counters_once() {
    let db_path = temp_db_path("business-quota-credits-cutover");
    let db_str = db_path.to_string_lossy().to_string();

    // First start: create schema + seed token/user rows for FK constraints.
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let unbound_token = proxy
        .create_access_token(Some("cutover-unbound-token"))
        .await
        .expect("create token");
    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "cutover-user".to_string(),
            username: Some("cutover".to_string()),
            name: Some("Cutover User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let bound_token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:cutover"))
        .await
        .expect("bind token");

    // Simulate an older DB (pre-cutover) by clearing the cutover meta key and writing
    // legacy request-count counters into the buckets/quota tables. The migration should
    // preserve them so deploys do not silently reset active customer quotas.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_BUSINESS_QUOTA_CREDITS_CUTOVER_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset cutover meta");

    let now = Utc::now();
    let now_ts = now.timestamp();
    let minute_bucket = now_ts - (now_ts % SECS_PER_MINUTE);
    let hour_bucket = now_ts - (now_ts % SECS_PER_HOUR);
    let month_start = start_of_month(now).timestamp();

    // Token-scoped legacy counters.
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(minute_bucket)
    .bind(GRANULARITY_MINUTE)
    .bind(9_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token minute bucket");
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(hour_bucket)
    .bind(GRANULARITY_HOUR)
    .bind(11_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token hour bucket");
    // Ensure the request limiter bucket is not affected by the cutover reset.
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(minute_bucket)
    .bind(GRANULARITY_REQUEST_MINUTE)
    .bind(5_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token request_minute bucket");
    sqlx::query(
        "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&unbound_token.id)
    .bind(month_start)
    .bind(13_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token monthly quota");

    // Account-scoped legacy counters (e.g. from old backfill).
    sqlx::query(
        "INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(minute_bucket)
    .bind(GRANULARITY_MINUTE)
    .bind(7_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account minute bucket");
    sqlx::query(
        "INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(hour_bucket)
    .bind(GRANULARITY_HOUR)
    .bind(8_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account hour bucket");
    sqlx::query(
        "INSERT INTO account_monthly_quota (user_id, month_start, month_count) VALUES (?, ?, ?)",
    )
    .bind(&user.user_id)
    .bind(month_start)
    .bind(14_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed account monthly quota");

    drop(proxy);

    // Second start: cutover migration should preserve legacy counters exactly once.
    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy restarted");

    let token_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read token minute buckets");
    assert_eq!(
        token_minute_sum, 9,
        "cutover should preserve token minute buckets"
    );

    let token_hour_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read token hour buckets");
    assert_eq!(
        token_hour_sum, 11,
        "cutover should preserve token hour buckets"
    );

    let token_request_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_REQUEST_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read token request_minute buckets");
    assert_eq!(
        token_request_minute_sum, 5,
        "cutover must not clear raw request limiter buckets"
    );

    let token_monthly_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM auth_token_quota WHERE token_id = ?",
    )
    .bind(&unbound_token.id)
    .fetch_optional(&proxy_after.key_store.pool)
    .await
    .expect("read token monthly quota")
    .unwrap_or(0);
    assert_eq!(
        token_monthly_count, 13,
        "cutover should preserve token monthly quota"
    );

    let account_minute_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account minute buckets");
    assert_eq!(
        account_minute_sum, 7,
        "cutover should preserve account minute buckets"
    );

    let account_hour_sum: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account hour buckets");
    assert_eq!(
        account_hour_sum, 8,
        "cutover should preserve account hour buckets"
    );

    let account_monthly_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_optional(&proxy_after.key_store.pool)
    .await
    .expect("read account monthly quota")
    .unwrap_or(0);
    assert_eq!(
        account_monthly_count, 14,
        "cutover should preserve account monthly quota"
    );

    // Third start: cutover meta key exists, so preserved counters should remain untouched.
    sqlx::query(
        "UPDATE token_usage_buckets SET count = ? WHERE token_id = ? AND bucket_start = ? AND granularity = ?",
    )
    .bind(12_i64)
    .bind(&unbound_token.id)
    .bind(minute_bucket)
    .bind(GRANULARITY_MINUTE)
    .execute(&proxy_after.key_store.pool)
    .await
    .expect("update post-cutover token bucket");
    drop(proxy_after);

    let proxy_third = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy restarted again");

    let token_minute_sum_after: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM token_usage_buckets WHERE token_id = ? AND granularity = ?",
    )
    .bind(&unbound_token.id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_third.key_store.pool)
    .await
    .expect("read token minute buckets after third start");
    assert_eq!(
        token_minute_sum_after, 12,
        "cutover migration must not rerun after meta is set"
    );

    // Silence unused warning for the bound token variable; it exists only for FK seeding.
    assert!(!bound_token.id.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_quota_backfill_is_idempotent() {
    let db_path = temp_db_path("account-backfill-idempotent");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "backfill-user".to_string(),
            username: Some("backfill".to_string()),
            name: Some("Backfill User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    let token = proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:backfill"))
        .await
        .expect("bind token");

    let month_start = start_of_month(Utc::now()).timestamp();
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&token.id)
    .bind(month_start)
    .bind(GRANULARITY_MINUTE)
    .bind(3_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token minute bucket");
    sqlx::query(
        "INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count) VALUES (?, ?, ?, ?)",
    )
    .bind(&token.id)
    .bind(month_start)
    .bind(GRANULARITY_HOUR)
    .bind(5_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token hour bucket");
    sqlx::query(
        "INSERT INTO auth_token_quota (token_id, month_start, month_count) VALUES (?, ?, ?)\n             ON CONFLICT(token_id) DO UPDATE SET month_start = excluded.month_start, month_count = excluded.month_count",
    )
    .bind(&token.id)
    .bind(month_start)
    .bind(7_i64)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed token monthly quota");

    sqlx::query("DELETE FROM account_usage_buckets")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear account buckets");
    sqlx::query("DELETE FROM account_monthly_quota")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear account monthly");
    sqlx::query("DELETE FROM account_quota_limits")
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear account limits");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("reset backfill meta");

    drop(proxy);

    let proxy_after = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened for first backfill");

    let first_account_minute: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account minute after first backfill");
    let first_account_hour: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account hour after first backfill");
    let first_month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read account month after first backfill");

    assert_eq!(first_account_minute, 3);
    assert_eq!(first_account_hour, 5);
    assert_eq!(first_month_count, 7);

    drop(proxy_after);

    let proxy_again = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy reopened for idempotent check");
    let second_account_minute: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_MINUTE)
    .fetch_one(&proxy_again.key_store.pool)
    .await
    .expect("read account minute after second init");
    let second_account_hour: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(count), 0) FROM account_usage_buckets WHERE user_id = ? AND granularity = ?",
    )
    .bind(&user.user_id)
    .bind(GRANULARITY_HOUR)
    .fetch_one(&proxy_again.key_store.pool)
    .await
    .expect("read account hour after second init");
    let second_month_count: i64 = sqlx::query_scalar(
        "SELECT COALESCE(month_count, 0) FROM account_monthly_quota WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_again.key_store.pool)
    .await
    .expect("read account month after second init");

    assert_eq!(second_account_minute, first_account_minute);
    assert_eq!(second_account_hour, first_account_hour);
    assert_eq!(second_month_count, first_month_count);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn account_quota_limits_sync_with_env_defaults_on_restart() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-sync");
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
            provider: "linuxdo".to_string(),
            provider_user_id: "limit-sync-user".to_string(),
            username: Some("limit_sync_user".to_string()),
            name: Some("Limit Sync User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:limit_sync_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("seed account quota row");

    let seeded_limits: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read seeded limits");
    assert_eq!(seeded_limits, (0, 0, 0, 0));

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
    let second_limits: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read second limits");
    assert_eq!(second_limits, (21, 22, 23, 24));

    unsafe {
        std::env::remove_var("TOKEN_HOURLY_REQUEST_LIMIT");
        std::env::remove_var("TOKEN_HOURLY_LIMIT");
        std::env::remove_var("TOKEN_DAILY_LIMIT");
        std::env::remove_var("TOKEN_MONTHLY_LIMIT");
    }
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn legacy_current_default_account_quota_limits_keep_following_defaults_after_reclassification()
 {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-legacy-current-default");
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
            provider_user_id: "legacy-current-default-user".to_string(),
            username: Some("legacy_current_default_user".to_string()),
            name: Some("Legacy Current Default User".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert user");
    proxy
        .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_current_default_user"))
        .await
        .expect("bind token");
    proxy
        .user_dashboard_summary(&user.user_id, None)
        .await
        .expect("seed account quota row");
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
    .expect("seed legacy current default row");
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_ACCOUNT_QUOTA_INHERITS_DEFAULTS_BACKFILL_V1)
        .execute(&proxy.key_store.pool)
        .await
        .expect("clear inherits defaults backfill marker");

    drop(proxy);

    let proxy_after_backfill =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened for backfill");
    let first_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after_backfill.key_store.pool)
    .await
    .expect("read reclassified default limits");
    assert_eq!(first_limits, (11, 12, 13, 14, 1));

    drop(proxy_after_backfill);

    unsafe {
        std::env::set_var("TOKEN_HOURLY_REQUEST_LIMIT", "21");
        std::env::set_var("TOKEN_HOURLY_LIMIT", "22");
        std::env::set_var("TOKEN_DAILY_LIMIT", "23");
        std::env::set_var("TOKEN_MONTHLY_LIMIT", "24");
    }

    let proxy_after_sync =
        TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy reopened for sync");
    let second_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&user.user_id)
    .fetch_one(&proxy_after_sync.key_store.pool)
    .await
    .expect("read synced default limits");
    assert_eq!(second_limits, (21, 22, 23, 24, 1));

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
async fn shared_legacy_noncurrent_tuple_is_left_custom_during_reclassification() {
    let _guard = env_lock().lock_owned().await;
    let db_path = temp_db_path("account-limit-legacy-shared-noncurrent");
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
    let alpha = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-shared-alpha".to_string(),
            username: Some("legacy_shared_alpha".to_string()),
            name: Some("Legacy Shared Alpha".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(1),
            raw_payload_json: None,
        })
        .await
        .expect("upsert alpha");
    let beta = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-shared-beta".to_string(),
            username: Some("legacy_shared_beta".to_string()),
            name: Some("Legacy Shared Beta".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(2),
            raw_payload_json: None,
        })
        .await
        .expect("upsert beta");
    let custom_user = proxy
        .upsert_oauth_account(&OAuthAccountProfile {
            provider: "linuxdo".to_string(),
            provider_user_id: "legacy-shared-custom".to_string(),
            username: Some("legacy_shared_custom".to_string()),
            name: Some("Legacy Shared Custom".to_string()),
            avatar_template: None,
            active: true,
            trust_level: Some(3),
            raw_payload_json: None,
        })
        .await
        .expect("upsert custom user");
    for user in [&alpha, &beta, &custom_user] {
        proxy
            .ensure_user_token_binding(&user.user_id, Some("linuxdo:legacy_shared"))
            .await
            .expect("bind token");
        proxy
            .user_dashboard_summary(&user.user_id, None)
            .await
            .expect("seed account quota row");
    }
    sqlx::query(
        r#"UPDATE account_quota_limits
           SET hourly_any_limit = 11,
               hourly_limit = 12,
               daily_limit = 13,
               monthly_limit = 14,
               inherits_defaults = 1,
               updated_at = created_at + 5
           WHERE user_id IN (?, ?)"#,
    )
    .bind(&alpha.user_id)
    .bind(&beta.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate shared non-current tuple rows");
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
    .bind(&custom_user.user_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate legacy custom row");
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
    for user_id in [&alpha.user_id, &beta.user_id] {
        let limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_one(&proxy_after.key_store.pool)
        .await
        .expect("read shared tuple limits");
        assert_eq!(limits, (11, 12, 13, 14, 0));
    }
    let custom_limits: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT hourly_any_limit, hourly_limit, daily_limit, monthly_limit, inherits_defaults FROM account_quota_limits WHERE user_id = ?",
    )
    .bind(&custom_user.user_id)
    .fetch_one(&proxy_after.key_store.pool)
    .await
    .expect("read shared custom limits");
    assert_eq!(custom_limits, (101, 102, 103, 104, 0));

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

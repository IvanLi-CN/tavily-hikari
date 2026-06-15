#[tokio::test]
async fn large_legacy_single_db_request_logs_stay_in_core_database_for_startup() {
    let db_path = temp_db_path("observability-sidecar-large-legacy-compat");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create api_keys");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            auth_token_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            query TEXT,
            status_code INTEGER,
            error_message TEXT,
            result_status TEXT NOT NULL DEFAULT 'success',
            request_body BLOB,
            response_body BLOB,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-large-legacy-sidecar', 1, 1)",
    )
    .execute(&mut conn)
    .await
    .expect("insert api key");
    sqlx::query(
        r#"
        INSERT INTO request_logs (
            api_key_id,
            auth_token_id,
            method,
            path,
            query,
            status_code,
            error_message,
            result_status,
            request_body,
            response_body,
            visibility,
            created_at
        ) VALUES ('k1', NULL, 'POST', '/api/tavily/search', NULL, 200, NULL, 'success', NULL, NULL, 'visible', 1710000000)
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("insert legacy request log");
    drop(conn);

    std::fs::OpenOptions::new()
        .write(true)
        .open(&db_path)
        .expect("open sqlite file for resize")
        .set_len(LEGACY_REQUEST_LOGS_INLINE_SIDECAR_MIGRATION_MAX_BYTES + 4096)
        .expect("expand sqlite file");

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let attached_path = proxy
        .sqlite_observability_database_path()
        .expect("observability database attached");
    assert!(
        sqlite_paths_match(attached_path, &db_str),
        "large legacy databases should keep observability attached to the core file on startup"
    );

    let first_pool_connection = proxy
        .key_store
        .pool
        .acquire()
        .await
        .expect("acquire first large-legacy pool connection");
    let second_pool_connection = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        proxy.key_store.pool.acquire(),
    )
    .await
    .expect("large legacy compatibility should keep the default sqlite pool capacity")
    .expect("acquire second large-legacy pool connection");

    let main_request_logs_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main request_logs")
    .is_some();
    assert!(
        main_request_logs_exists,
        "large legacy request_logs should stay in the core database during startup"
    );

    let observed_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observability.request_logs")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("count observable request logs");
    assert_eq!(observed_rows, 1);

    drop(second_pool_connection);
    drop(first_pool_connection);
    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let observability_path = layout
        .observability_database_path
        .expect("sidecar path should still be derivable");
    assert!(
        !std::path::Path::new(&observability_path).exists(),
        "legacy compatibility startup should not create a sidecar file"
    );
}

#[tokio::test]
async fn legacy_request_logs_backfill_uses_main_schema_when_sidecar_table_already_exists() {
    let db_path = temp_db_path("observability-sidecar-preseeded-request-logs");
    let db_str = db_path.to_string_lossy().to_string();
    let layout = SqliteDatabaseLayout::from_database_path(&db_str);
    let observability_path = layout
        .observability_database_path
        .clone()
        .expect("sidecar path");

    let mut conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true),
    )
    .await
    .expect("open legacy sqlite");
    sqlx::query(
        r#"
        CREATE TABLE api_keys (
            id TEXT PRIMARY KEY,
            api_key TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL DEFAULT 0,
            last_used_at INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create api_keys");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (api_key_id) REFERENCES api_keys(id)
        )
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("create legacy request_logs");
    sqlx::query(
        "INSERT INTO api_keys (id, api_key, created_at, last_used_at) VALUES ('k1', 'tvly-preseeded-sidecar', 1, 1)",
    )
    .execute(&mut conn)
    .await
    .expect("insert api key");
    sqlx::query(
        r#"
        INSERT INTO request_logs (api_key_id, method, path, created_at)
        VALUES ('k1', 'POST', '/api/tavily/search', 1710000000)
        "#,
    )
    .execute(&mut conn)
    .await
    .expect("insert legacy request log");
    drop(conn);

    let mut observability_conn = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&observability_path)
            .create_if_missing(true),
    )
    .await
    .expect("open preseeded sidecar sqlite");
    sqlx::query(
        r#"
        CREATE TABLE request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            api_key_id TEXT,
            auth_token_id TEXT,
            request_user_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            query TEXT,
            status_code INTEGER,
            tavily_status_code INTEGER,
            error_message TEXT,
            result_status TEXT NOT NULL DEFAULT 'unknown',
            request_kind_key TEXT,
            request_kind_label TEXT,
            request_kind_detail TEXT,
            counts_business_quota INTEGER,
            business_credits INTEGER,
            failure_kind TEXT,
            key_effect_code TEXT NOT NULL DEFAULT 'none',
            key_effect_summary TEXT,
            binding_effect_code TEXT NOT NULL DEFAULT 'none',
            binding_effect_summary TEXT,
            selection_effect_code TEXT NOT NULL DEFAULT 'none',
            selection_effect_summary TEXT,
            gateway_mode TEXT,
            experiment_variant TEXT,
            proxy_session_id TEXT,
            routing_subject_hash TEXT,
            upstream_operation TEXT,
            fallback_reason TEXT,
            request_body BLOB,
            response_body BLOB,
            request_body_bytes INTEGER,
            response_body_bytes INTEGER,
            request_body_sha256 TEXT,
            response_body_sha256 TEXT,
            body_retention_days INTEGER,
            body_retention_profile TEXT,
            body_cleaned_reason TEXT,
            body_cleaned_at INTEGER,
            forwarded_headers TEXT,
            dropped_headers TEXT,
            remote_addr TEXT,
            client_ip TEXT,
            client_ip_source TEXT,
            client_ip_trusted INTEGER NOT NULL DEFAULT 0,
            ip_headers TEXT,
            visibility TEXT NOT NULL DEFAULT 'visible',
            created_at INTEGER NOT NULL
        )
        "#,
    )
    .execute(&mut observability_conn)
    .await
    .expect("create preseeded sidecar request_logs");
    drop(observability_conn);

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let migrated_row = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT api_key_id, path, proxy_session_id
        FROM observability.request_logs
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("fetch migrated request log");
    assert_eq!(migrated_row.0, "k1");
    assert_eq!(migrated_row.1, "/api/tavily/search");
    assert_eq!(
        migrated_row.2, None,
        "backfill should use the legacy main schema instead of probing sidecar-only columns"
    );

    let main_request_logs_exists = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'request_logs' LIMIT 1",
    )
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("check main request_logs after preseeded migration")
    .is_some();
    assert!(
        !main_request_logs_exists,
        "legacy main request_logs should still be removed after migrating into a preseeded sidecar"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(&observability_path);
    let _ = std::fs::remove_file(format!("{observability_path}-shm"));
    let _ = std::fs::remove_file(format!("{observability_path}-wal"));
}

#[tokio::test]
async fn heal_orphan_auth_tokens_from_logs_creates_soft_deleted_token() {
    let db_path = temp_db_path("heal-orphan");
    let db_str = db_path.to_string_lossy().to_string();

    // Initialize schema.
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    // Insert an auth_token_logs entry for a token id that does not exist in auth_tokens.
    let orphan_token_id = "ZZZZ";
    sqlx::query(
        r#"
        INSERT INTO auth_token_logs (
            token_id, method, path, query, http_status, mcp_status, result_status, error_message, created_at
        ) VALUES (?, 'GET', '/mcp', NULL, 200, NULL, 'success', NULL, 1234567890)
        "#,
    )
    .bind(orphan_token_id)
    .execute(&store.pool)
    .await
    .expect("insert orphan log");

    // Clear healer meta key so that we can invoke the healer path again for this test.
    sqlx::query("DELETE FROM meta WHERE key = ?")
        .bind(META_KEY_HEAL_ORPHAN_TOKENS_V1)
        .execute(&store.pool)
        .await
        .expect("delete meta gate");

    // Run healer directly.
    store
        .heal_orphan_auth_tokens_from_logs()
        .await
        .expect("heal orphan tokens");

    // Verify that a soft-deleted auth_tokens row was created for the orphan id.
    let (enabled, total_requests, deleted_at): (i64, i64, Option<i64>) =
        sqlx::query_as("SELECT enabled, total_requests, deleted_at FROM auth_tokens WHERE id = ?")
            .bind(orphan_token_id)
            .fetch_one(&store.pool)
            .await
            .expect("restored token row");

    assert_eq!(enabled, 0, "restored token should be disabled");
    assert_eq!(
        total_requests, 1,
        "restored token should count orphan log entries"
    );
    assert!(
        deleted_at.is_some(),
        "restored token should be marked soft-deleted"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn published_announcement_update_archives_previous_version() {
    let db_path = temp_db_path("announcement-published-update-archives");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    let draft = store
        .create_announcement(AnnouncementMutation {
            title: "Initial notice".to_string(),
            body: "Initial body".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_MODAL.to_string(),
        })
        .await
        .expect("create announcement");
    assert_eq!(draft.status, ANNOUNCEMENT_STATUS_DRAFT);
    assert!(
        store
            .list_user_active_announcements()
            .await
            .expect("list active before publish")
            .is_empty()
    );

    let published = store
        .publish_announcement(&draft.id)
        .await
        .expect("publish announcement")
        .expect("published announcement exists");
    assert_eq!(published.status, ANNOUNCEMENT_STATUS_PUBLISHED);
    assert!(published.published_at.is_some());

    let revised = store
        .update_announcement(
            &published.id,
            AnnouncementMutation {
                title: "Updated notice".to_string(),
                body: "Updated body".to_string(),
                display_kind: ANNOUNCEMENT_DISPLAY_MODAL.to_string(),
            },
        )
        .await
        .expect("update published announcement")
        .expect("updated announcement exists");
    assert_ne!(revised.id, published.id);
    assert_eq!(revised.status, ANNOUNCEMENT_STATUS_PUBLISHED);
    assert_eq!(revised.title, "Updated notice");

    let archived = store
        .get_announcement(&published.id)
        .await
        .expect("load previous announcement")
        .expect("previous announcement exists");
    assert_eq!(archived.status, ANNOUNCEMENT_STATUS_ARCHIVED);
    assert!(archived.archived_at.is_some());

    let active = store
        .list_user_active_announcements()
        .await
        .expect("list active announcements");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, revised.id);

    let history = store
        .list_user_announcement_history()
        .await
        .expect("list announcement history");
    assert!(history.iter().any(|item| item.id == revised.id));
    assert!(history.iter().any(|item| item.id == published.id));

    let draft_only = store
        .create_announcement(AnnouncementMutation {
            title: "Draft-only notice".to_string(),
            body: "Never published".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_TICKER.to_string(),
        })
        .await
        .expect("create draft-only announcement");
    let archived_draft = store
        .archive_announcement(&draft_only.id)
        .await
        .expect("archive draft-only announcement")
        .expect("archived draft exists");
    assert_eq!(archived_draft.status, ANNOUNCEMENT_STATUS_ARCHIVED);

    let history_after_draft_archive = store
        .list_user_announcement_history()
        .await
        .expect("list announcement history after draft archive");
    assert!(!history_after_draft_archive
        .iter()
        .any(|item| item.id == archived_draft.id));

    let archived_revised = store
        .archive_announcement(&revised.id)
        .await
        .expect("archive revised announcement")
        .expect("archived revised announcement exists");
    assert_eq!(archived_revised.status, ANNOUNCEMENT_STATUS_ARCHIVED);

    let edited_archived = store
        .update_announcement(
            &archived_revised.id,
            AnnouncementMutation {
                title: "Edited archived notice".to_string(),
                body: "Edited archived body".to_string(),
                display_kind: ANNOUNCEMENT_DISPLAY_TICKER.to_string(),
            },
        )
        .await
        .expect("edit archived announcement")
        .expect("edited archived announcement creates draft");
    assert_ne!(edited_archived.id, archived_revised.id);
    assert_eq!(edited_archived.status, ANNOUNCEMENT_STATUS_DRAFT);

    let archived_revised_after_edit = store
        .get_announcement(&archived_revised.id)
        .await
        .expect("load archived revised after edit")
        .expect("archived revised still exists after edit");
    assert_eq!(archived_revised_after_edit.status, ANNOUNCEMENT_STATUS_ARCHIVED);
    assert_eq!(archived_revised_after_edit.title, archived_revised.title);

    let history_after_archived_edit = store
        .list_user_announcement_history()
        .await
        .expect("list announcement history after archived edit");
    assert!(!history_after_archived_edit
        .iter()
        .any(|item| item.id == edited_archived.id));

    let republished = store
        .publish_announcement(&archived_revised.id)
        .await
        .expect("republish archived announcement")
        .expect("republished announcement exists");
    assert_ne!(republished.id, archived_revised.id);
    assert_eq!(republished.status, ANNOUNCEMENT_STATUS_PUBLISHED);
    assert_eq!(republished.title, archived_revised.title);

    let archived_revised_after_republish = store
        .get_announcement(&archived_revised.id)
        .await
        .expect("load archived revised after republish")
        .expect("archived revised still exists");
    assert_eq!(archived_revised_after_republish.status, ANNOUNCEMENT_STATUS_ARCHIVED);

    let active_after_republish = store
        .list_user_active_announcements()
        .await
        .expect("list active after republish");
    assert_eq!(active_after_republish.len(), 1);
    assert_eq!(active_after_republish[0].id, republished.id);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn active_announcements_use_insert_order_for_same_second_ties() {
    let db_path = temp_db_path("announcement-active-same-second-order");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");
    let same_second = 1_764_300_000_i64;

    sqlx::query(
        r#"
        INSERT INTO announcements (
            id, title, body, display_kind, status,
            created_at, updated_at, published_at, archived_at
        ) VALUES
            ('zzzzzzzz', 'Older modal', 'Older body', 'modal', 'published', ?, ?, ?, NULL),
            ('22222222', 'Newer modal', 'Newer body', 'modal', 'published', ?, ?, ?, NULL)
        "#,
    )
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .bind(same_second)
    .execute(&store.pool)
    .await
    .expect("insert same-second announcements");

    let active = store
        .list_user_active_announcements()
        .await
        .expect("list active announcements");

    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "22222222");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn ticker_announcements_may_omit_body_but_modal_announcements_may_not() {
    let db_path = temp_db_path("announcement-ticker-empty-body");
    let db_str = db_path.to_string_lossy().to_string();
    let store = KeyStore::new(&db_str).await.expect("keystore created");

    let ticker = store
        .create_announcement(AnnouncementMutation {
            title: "Ticker without details".to_string(),
            body: "   ".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_TICKER.to_string(),
        })
        .await
        .expect("create ticker without body");
    assert_eq!(ticker.body, "");

    let modal = store
        .create_announcement(AnnouncementMutation {
            title: "Modal without details".to_string(),
            body: "   ".to_string(),
            display_kind: ANNOUNCEMENT_DISPLAY_MODAL.to_string(),
        })
        .await;
    assert!(modal.is_err(), "modal announcements still require body content");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_is_single_use() {
    let db_path = temp_db_path("oauth-state-single-use");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state("linuxdo", Some("/"), 120)
        .await
        .expect("create oauth state");
    let first = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state first");
    let second = proxy
        .consume_oauth_login_state("linuxdo", &state)
        .await
        .expect("consume oauth state second");

    assert_eq!(first, Some(Some("/".to_string())));
    assert_eq!(second, None, "oauth state must be single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_binding_hash_must_match() {
    let db_path = temp_db_path("oauth-state-binding-hash");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding("linuxdo", Some("/"), 120, Some("nonce-hash-a"))
        .await
        .expect("create oauth state");

    let wrong_hash = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-b"))
        .await
        .expect("consume oauth state with wrong hash");
    assert_eq!(wrong_hash, None, "wrong hash must not consume oauth state");

    let matched = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state with matching hash");
    assert_eq!(matched, Some(Some("/".to_string())));

    let reused = proxy
        .consume_oauth_login_state_with_binding("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state reused");
    assert_eq!(reused, None, "oauth state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn oauth_login_state_payload_carries_bind_token_id() {
    let db_path = temp_db_path("oauth-state-bind-token-id");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let state = proxy
        .create_oauth_login_state_with_binding_and_token(
            "linuxdo",
            Some("/console"),
            120,
            Some("nonce-hash-a"),
            Some("a1b2"),
        )
        .await
        .expect("create oauth state");

    let payload = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state")
        .expect("payload exists");

    assert_eq!(payload.redirect_to.as_deref(), Some("/console"));
    assert_eq!(payload.bind_token_id.as_deref(), Some("a1b2"));

    let consumed_again = proxy
        .consume_oauth_login_state_with_binding_and_token("linuxdo", &state, Some("nonce-hash-a"))
        .await
        .expect("consume oauth state second");
    assert!(consumed_again.is_none(), "state must remain single-use");

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn request_stats_coalescer_waits_for_window_before_background_flush() {
    let db_path = temp_db_path("request-stats-coalescer-windowed-flush");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-request-stats-coalescer-window".to_string()],
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
    let now_ts = Utc::now().timestamp();

    proxy
        .key_store
        .enqueue_request_stats_rollup_for_test(Some(&key_id), now_ts, OUTCOME_SUCCESS)
        .await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    let stored_total: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT total_requests
        FROM api_key_usage_buckets
        WHERE api_key_id = ?
        ORDER BY bucket_start DESC
        LIMIT 1
        "#,
    )
    .bind(&key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query usage buckets before flush window");
    assert!(
        stored_total.is_none(),
        "background worker should not flush request stats before the coalescing window elapses"
    );

    tokio::time::sleep(RequestStatsCoalescer::FLUSH_INTERVAL + Duration::from_millis(150)).await;

    let stored_total: Option<i64> = sqlx::query_scalar(
        r#"
        SELECT total_requests
        FROM api_key_usage_buckets
        WHERE api_key_id = ?
        ORDER BY bucket_start DESC
        LIMIT 1
        "#,
    )
    .bind(&key_id)
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query usage buckets after flush window");
    assert_eq!(stored_total, Some(1));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

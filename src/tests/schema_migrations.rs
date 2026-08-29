use super::*;

#[tokio::test]
async fn versioned_schema_migrations_are_idempotent_and_fail_closed_on_drift() {
    let db_path = temp_db_path("versioned-schema-migrations");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migrations".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("bootstrap database");
    drop(proxy);

    let reopened = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migrations".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("reopen migrated database");
    drop(reopened);

    let pool = connect_sqlite_test_pool(&db_str).await;
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .expect("read migration ledger");
    assert_eq!(
        versions,
        vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
        ]
    );
    let transport_observation_column: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('upstream_reconciliation_run_observation') WHERE name = 'last_transport_kind'",
    )
    .fetch_one(&pool)
    .await
    .expect("read transport observation column");
    assert_eq!(transport_observation_column, 1);
    let transport_state_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('upstream_reconciliation_run_observation') WHERE name IN ('last_transport_kind_at', 'last_retryable_outcome')",
    )
    .fetch_one(&pool)
    .await
    .expect("read transport state columns");
    assert_eq!(transport_state_columns, 2);
    let observation_metric_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('upstream_reconciliation_run_observation') WHERE name IN ('partial_key_observation_count', 'multi_key_pending_count', 'remote_attempt_budget_defer_count', 'resumed_run_count', 'terminal_run_count')",
    )
    .fetch_one(&pool)
    .await
    .expect("read reconciliation observation metric columns");
    assert_eq!(observation_metric_columns, 5);
    let research_progress_window: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'upstream_reconciliation_research_progress_window'",
    )
    .fetch_one(&pool)
    .await
    .expect("read research progress window table");
    assert_eq!(research_progress_window, 1);
    let key_observations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'upstream_reconciliation_key_observations'",
    )
    .fetch_one(&pool)
    .await
    .expect("read reconciliation key observations table");
    assert_eq!(key_observations, 1);
    let projection_state: (i64, i64, i64) = sqlx::query_as(
        "SELECT batch_size, scanned_rows, completed FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&pool)
    .await
    .expect("read reconciliation engine projection state");
    assert_eq!(projection_state, (25, 0, 1));
    let projection_complete: i64 = sqlx::query_scalar(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'upstream_reconciliation_work_projection_complete_v1'",
    )
    .fetch_one(&pool)
    .await
    .expect("read empty-database projection lifecycle");
    assert_eq!(
        projection_complete, 1,
        "a new empty database must not schedule historical reconciliation projection"
    );
    let controller: (String, i64) = sqlx::query_as(
        "SELECT mode, legacy_active FROM upstream_reconciliation_control_state WHERE id = 'local'",
    )
    .fetch_one(&pool)
    .await
    .expect("read fresh reconciliation controller");
    assert_eq!(controller, ("compare".to_string(), 0));
    let projection_sources: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM observability.dashboard_alert_projection_state")
            .fetch_one(&pool)
            .await
            .expect("read fresh alert projection sources");
    assert_eq!(projection_sources, 3);
    let recent_tail_sources: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_state \
         WHERE cursor_occurred_at = 0 AND cursor_row_sort_id = '' AND phase = 'catching_up'",
    )
    .fetch_one(&pool)
    .await
    .expect("read fresh full-history alert projection cursors");
    assert_eq!(
        recent_tail_sources, 0,
        "the Dashboard tail starts at its bounded recent cursor"
    );
    let research_scan_state: (i64, String, String) = sqlx::query_as(
        "SELECT cursor_next_poll_at, cursor_key_id, cursor_request_id
           FROM upstream_reconciliation_research_scan_state WHERE id = 'local'",
    )
    .fetch_one(&pool)
    .await
    .expect("read research scan state");
    assert_eq!(research_scan_state, (-1, String::new(), String::new()));
    let research_scan_index: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_upstream_reconciliation_research_due_scan'",
    )
    .fetch_one(&pool)
    .await
    .expect("read research scan index");
    assert_eq!(research_scan_index, 1);
    let full_history_cursor_sources: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM observability.dashboard_alert_projection_history_state \
         WHERE cursor_occurred_at = 0 AND cursor_row_sort_id = '' AND phase = 'catching_up'",
    )
    .fetch_one(&pool)
    .await
    .expect("read fresh full-history alert projection cursors");
    assert_eq!(
        full_history_cursor_sources, 3,
        "the administrator sidecar starts from a durable full-history cursor without startup scans"
    );
    sqlx::query("UPDATE schema_migrations SET checksum = 'drifted' WHERE version = 2")
        .execute(&pool)
        .await
        .expect("corrupt migration checksum");
    pool.close().await;

    let error = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migrations".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect_err("checksum drift must reject startup");
    assert!(error.to_string().contains("checksum mismatch"));

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn reconciliation_transport_observation_migration_is_additive_and_warm_safe() {
    let db_path = temp_db_path("reconciliation-transport-observation-migration");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-transport-observation".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    sqlx::query(
        "INSERT INTO upstream_reconciliation_work (token_id, period_code, project_id, billing_subject, settlement_mode, period_start, period_end, scheduling_key_id, updated_at) VALUES ('transport-migration-token', '2026-08-18/S1', 'transport-migration-project', 'token:transport-migration-token', 'shadow', 1, 2, 'transport-migration-key', 2)",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed durable reconciliation work");
    sqlx::query(
        "ALTER TABLE upstream_reconciliation_run_observation DROP COLUMN last_transport_kind",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("shape v17 observation table");
    sqlx::query(
        "ALTER TABLE upstream_reconciliation_run_observation DROP COLUMN last_transport_kind_at",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("shape v18 transport state table");
    sqlx::query(
        "ALTER TABLE upstream_reconciliation_run_observation DROP COLUMN last_retryable_outcome",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("shape v18 retryable state table");
    sqlx::query("DELETE FROM schema_migrations WHERE version = 18")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove v18 ledger record");
    sqlx::query("DELETE FROM schema_migrations WHERE version = 19")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove v19 ledger record");

    assert!(
        !proxy
            .key_store
            .prepare_versioned_schema()
            .await
            .expect("apply additive transport observation migration"),
        "an existing database must not request full bootstrap"
    );
    let column_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('upstream_reconciliation_run_observation') WHERE name = 'last_transport_kind'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read re-added transport column");
    let work_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_work WHERE token_id = 'transport-migration-token'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("verify migration did not scan or rewrite durable work");
    assert_eq!(column_count, 1);
    assert_eq!(work_count, 1);
    let transport_state_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('upstream_reconciliation_run_observation') WHERE name IN ('last_transport_kind_at', 'last_retryable_outcome')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read re-added transport state columns");
    assert_eq!(transport_state_columns, 2);

    drop(proxy);
    let reopened = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-transport-observation".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("warm reopen after v18 migration");
    drop(reopened);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn reconciliation_engine_state_migration_resumes_an_incomplete_legacy_projection() {
    let db_path = temp_db_path("reconciliation-engine-state-v9");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-engine-state-v9".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    for statement in [
        "DROP TRIGGER trg_upstream_reconciliation_work_failure_reset_insert",
        "DROP TRIGGER trg_upstream_reconciliation_work_failure_reset_update",
        "DROP TABLE upstream_reconciliation_projection_state",
        "DROP TABLE upstream_reconciliation_run_observation",
        "DROP TABLE upstream_reconciliation_control_transitions",
        "DROP TABLE upstream_reconciliation_control_state",
        "ALTER TABLE upstream_reconciliation_work DROP COLUMN transport_failure_streak",
        "ALTER TABLE upstream_reconciliation_work DROP COLUMN transport_retry_at",
        "ALTER TABLE upstream_reconciliation_work DROP COLUMN semantic_failure_streak",
        "ALTER TABLE upstream_reconciliation_work DROP COLUMN semantic_retry_at",
        // Rebuild the full post-v8 migration tail from the legacy fixture.  Keeping
        // v19 recorded while its observation table is intentionally dropped would
        // correctly trigger warm-start drift rejection before the missing migrations
        // can be replayed.
        "DELETE FROM schema_migrations WHERE version BETWEEN 9 AND 19",
    ] {
        sqlx::query(statement)
            .execute(&proxy.key_store.pool)
            .await
            .unwrap_or_else(|err| panic!("apply legacy fixture statement {statement}: {err}"));
    }
    for (suffix, delta_credits) in [("zero", 0_i64), ("nonzero", 3_i64)] {
        let token_id = format!("migration-shadow-{suffix}");
        let period_code = format!("2026-07-15/{suffix}");
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 settlement_mode, period_start, period_end, request_count,
                 first_used_at, last_used_at, updated_at
               ) VALUES (?, 'migration-key', ?, 'migration-project', ?, 'shadow',
                         1, 2, 1, 1, 2, 2)"#,
        )
        .bind(&token_id)
        .bind(&period_code)
        .bind(format!("token:{token_id}"))
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert historical shadow usage");
        sqlx::query(
            r#"INSERT INTO upstream_reconciliation_settlements (
                 settlement_key, token_id, period_code, project_id, billing_subject,
                 period_start, period_end, status, delta_credits, created_at,
                 updated_at, settled_at
               ) VALUES (?, ?, ?, 'migration-project', ?, 1, 2,
                         'shadow_settled', ?, 2, 2, 2)"#,
        )
        .bind(format!("v1:{token_id}:{period_code}"))
        .bind(&token_id)
        .bind(&period_code)
        .bind(format!("token:{token_id}"))
        .bind(delta_credits)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert historical shadow settlement");
        sqlx::query(
            "UPDATE upstream_reconciliation_work SET completed_generation = work_generation, last_outcome = 'settled' WHERE token_id = ? AND period_code = ?",
        )
        .bind(&token_id)
        .bind(&period_code)
        .execute(&proxy.key_store.pool)
        .await
        .expect("shape legacy terminal outcome");
    }
    proxy
        .key_store
        .set_meta_i64(
            META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1,
            0,
        )
        .await
        .expect("mark legacy projection incomplete");
    proxy
        .key_store
        .set_meta_i64(META_KEY_UPSTREAM_PRECISE_RECONCILIATION_ENABLED_V1, 1)
        .await
        .expect("preserve legacy active setting for controller adoption");

    assert!(
        !proxy
            .key_store
            .prepare_versioned_schema()
            .await
            .expect("resume additive reconciliation migration"),
        "an existing database must not request full bootstrap"
    );
    let state: (String, String, String, i64) = sqlx::query_as(
        "SELECT cursor_token_id, cursor_key_id, cursor_period_code, completed FROM upstream_reconciliation_projection_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read migrated stable projection cursor");
    assert_eq!(state, (String::new(), String::new(), String::new(), 0));
    let controller: (String, i64, Option<String>) = sqlx::query_as(
        "SELECT mode, legacy_active, activation_period_code FROM upstream_reconciliation_control_state WHERE id = 'local'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("adopt legacy active reconciliation controller");
    assert_eq!(controller, ("active".to_string(), 1, None));
    let recorded_v9_checksum: String =
        sqlx::query_scalar("SELECT checksum FROM schema_migrations WHERE version = 9")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read the immutable v9 migration identity");
    assert_eq!(
        recorded_v9_checksum,
        "sha256:614b3746410a20742499208d97764b88"
    );
    let startup_outcomes: Vec<String> = sqlx::query_scalar(
        "SELECT last_outcome FROM upstream_reconciliation_work WHERE token_id LIKE 'migration-shadow-%' ORDER BY token_id",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("startup migration must not scan and repair historical work");
    assert_eq!(startup_outcomes, vec!["settled", "settled"]);
    for _ in 0..10 {
        let slice = proxy
            .key_store
            .advance_upstream_reconciliation_work_projection()
            .await
            .expect("advance bounded outcome repair projection");
        if matches!(
            slice,
            crate::store::ReconciliationProjectionSliceOutcome::Advanced {
                completed: true,
                ..
            }
        ) {
            break;
        }
    }
    let repaired_outcomes: Vec<(String, String)> = sqlx::query_as(
        "SELECT token_id, last_outcome FROM upstream_reconciliation_work WHERE token_id LIKE 'migration-shadow-%' ORDER BY token_id",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read repaired shadow outcomes");
    assert_eq!(
        repaired_outcomes,
        vec![
            (
                "migration-shadow-nonzero".to_string(),
                "observed".to_string()
            ),
            (
                "migration-shadow-zero".to_string(),
                "no_adjustment".to_string()
            ),
        ]
    );
    let repair_plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        r#"EXPLAIN QUERY PLAN
           UPDATE upstream_reconciliation_work
              SET last_outcome = CASE
                    WHEN token_id = ? AND period_code = ? THEN 'observed'
                    ELSE last_outcome
                  END
            WHERE completed_generation >= work_generation
              AND ((token_id = ? AND period_code = ?)
                OR (token_id = ? AND period_code = ?))"#,
    )
    .bind("migration-shadow-nonzero")
    .bind("2026-07-15/nonzero")
    .bind("migration-shadow-nonzero")
    .bind("2026-07-15/nonzero")
    .bind("migration-shadow-zero")
    .bind("2026-07-15/zero")
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("explain bounded terminal repair");
    assert!(
        repair_plan
            .iter()
            .all(|(_, _, _, detail)| !detail.contains("SCAN ")),
        "terminal repair must not scan the work table: {repair_plan:?}"
    );
    assert!(
        repair_plan
            .iter()
            .any(|(_, _, _, detail)| detail.contains("SEARCH ")),
        "terminal repair must seek work by primary key: {repair_plan:?}"
    );
    for statement in [
        "DROP TRIGGER trg_upstream_reconciliation_work_failure_reset_insert",
        "DROP TRIGGER trg_upstream_reconciliation_work_failure_reset_update",
        "ALTER TABLE upstream_reconciliation_work DROP COLUMN semantic_retry_at",
        "CREATE TRIGGER trg_upstream_reconciliation_work_failure_reset_insert AFTER INSERT ON upstream_reconciliation_usage BEGIN SELECT 1; END",
        "CREATE TRIGGER trg_upstream_reconciliation_work_failure_reset_update AFTER UPDATE ON upstream_reconciliation_usage BEGIN SELECT 1; END",
    ] {
        sqlx::query(statement)
            .execute(&proxy.key_store.pool)
            .await
            .unwrap_or_else(|err| panic!("apply v9 drift fixture statement {statement}: {err}"));
    }
    let drift_error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("recorded v9 must reject missing retry state");
    assert!(drift_error.to_string().contains("version 9"));

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_migration_adds_the_per_channel_legacy_cursor() {
    let db_path = temp_db_path("schema-migration-ha-gc-legacy-cursor");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-ha-gc-legacy-cursor".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");

    sqlx::query(
        "UPDATE ha_outbox_gc_state SET last_legacy_control_seq = 101, \
         last_legacy_billing_seq = 202, last_legacy_runtime_seq = 303 WHERE id = 'local'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed pre-v8 shared cursor state");
    sqlx::query("DELETE FROM schema_migrations WHERE version = 8")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate an existing database before v8");
    sqlx::query("ALTER TABLE ha_outbox_gc_channel_state DROP COLUMN legacy_cursor_seq")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate the pre-v8 channel state");

    assert!(
        !proxy
            .key_store
            .prepare_versioned_schema()
            .await
            .expect("warm migration must converge an existing database"),
        "an existing database must not request full bootstrap"
    );
    let cursors: Vec<(String, i64)> = sqlx::query_as(
        "SELECT channel, legacy_cursor_seq FROM ha_outbox_gc_channel_state ORDER BY channel",
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("read migrated per-channel cursors");
    assert_eq!(
        cursors,
        vec![
            ("billing".to_string(), 202),
            ("control".to_string(), 101),
            ("runtime".to_string(), 303),
        ]
    );
    let v8_recorded: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 8)")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read v8 migration ledger record");
    assert_eq!(v8_recorded, 1);

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn versioned_schema_migrations_reject_missing_recorded_objects() {
    let db_path = temp_db_path("schema-migration-missing-object");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-object".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_insert")
        .execute(&pool)
        .await
        .expect("remove recorded migration object");
    pool.close().await;

    let error = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-object".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect_err("missing recorded migration object must reject startup");
    assert!(
        error
            .to_string()
            .contains("object validation failed at version 3")
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn terminal_outcome_migration_rejects_a_missing_usage_update_trigger() {
    let db_path = temp_db_path("schema-migration-terminal-outcome-trigger");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-terminal-outcome".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    drop(proxy);

    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("DROP TRIGGER trg_upstream_reconciliation_usage_work_update")
        .execute(&pool)
        .await
        .expect("remove terminal outcome trigger");
    pool.close().await;

    let error = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-terminal-outcome".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect_err("missing terminal outcome trigger must reject startup");
    assert!(
        error
            .to_string()
            .contains("object validation failed at version 4")
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn terminal_outcome_migration_reopens_same_second_usage_update_after_prior_settlement() {
    let db_path = temp_db_path("schema-migration-terminal-outcome-reopens-same-second-usage");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-terminal-outcome-reopens-same-second".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    let now = 1_752_500_000_i64;
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject,
            settlement_mode, period_start, period_end, request_count,
            first_used_at, last_used_at, updated_at
        ) VALUES ('migration-reopen-token', 'migration-reopen-key', '2026-07-15/S1',
                   'migration-reopen-project', 'account:migration-reopen', 'shadow',
                   ?, ?, 1, ?, ?, ?)
        "#,
    )
    .bind(now - 1_000)
    .bind(now - 300)
    .bind(now - 900)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert usage row");
    sqlx::query(
        r#"
        INSERT INTO upstream_reconciliation_settlements (
            settlement_key, token_id, period_code, project_id, billing_subject,
            period_start, period_end, status, upstream_usage, local_billed_credits,
            delta_credits, attempt_count, created_at, updated_at, settled_at
        ) VALUES ('v1:migration-reopen-token:2026-07-15/S1', 'migration-reopen-token',
                   '2026-07-15/S1', 'migration-reopen-project', 'account:migration-reopen',
                   ?, ?, 'shadow_settled', 1, 1, 0, 1, ?, ?, ?)
        "#,
    )
    .bind(now - 1_000)
    .bind(now - 300)
    .bind(now - 900)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert prior terminal settlement");
    sqlx::query(
        "UPDATE upstream_reconciliation_work SET work_generation = 1, completed_generation = 0, last_outcome = NULL WHERE token_id = 'migration-reopen-token'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("shape pre-v4 work row");
    let recorded_v5_checksum: String =
        sqlx::query_scalar("SELECT checksum FROM schema_migrations WHERE version = 5")
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read immutable v5 checksum");
    assert_eq!(
        recorded_v5_checksum,
        "sha256:8e4f4cc3f832d24d4f7d7dc3d6f2a8c1"
    );
    sqlx::query("DELETE FROM schema_migrations WHERE version = 6")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove same-second repair migration record");

    proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect("apply same-second repair migration after immutable v5");
    let reopened: (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT work_generation, completed_generation, last_outcome FROM upstream_reconciliation_work WHERE token_id = 'migration-reopen-token'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read reopened work row");
    assert_eq!(reopened, (1, 0, None));

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_verification_rejects_missing_backfill_time_index() {
    let db_path = temp_db_path("schema-migration-missing-backfill-time-index");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-backfill-time-index".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    sqlx::query("DROP INDEX observability.idx_request_logs_time")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove backfill index");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("missing backfill index must reject startup");
    assert!(
        error
            .to_string()
            .contains("missing observability.idx_request_logs_time")
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn versioned_schema_migrations_reject_unknown_future_versions() {
    let db_path = temp_db_path("schema-migration-future-version");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-future-version".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    sqlx::query(
        "INSERT INTO schema_migrations (version, name, checksum, applied_at) VALUES (99, 'future', 'sha256:future', 1)",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("record a future migration");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("older binaries must reject unknown migration versions");
    assert!(error.to_string().contains("unknown version 99"));

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn missing_meta_with_domain_data_fails_closed() {
    let db_path = temp_db_path("schema-migration-missing-meta");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-meta".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create production-shaped database");
    sqlx::query("INSERT INTO announcements (id, content, display_kind, status, created_at, updated_at) VALUES ('migration-meta-announcement', 'durable data', 'info', 'active', 1, 1)")
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert domain row");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("isolate non-ledger domain classification");
    sqlx::query("DROP TABLE meta")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove schema identity table");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("domain data without meta must fail closed");
    assert!(
        error
            .to_string()
            .contains("domain data exists without main.meta")
    );
    let meta_exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("check meta remains absent");
    assert_eq!(
        meta_exists, 0,
        "failed classification must not recreate meta"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn missing_meta_with_migration_ledger_fails_closed() {
    let db_path = temp_db_path("schema-migration-missing-meta-ledger");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-meta-ledger".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    sqlx::query("DROP TABLE meta")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove schema identity table");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("migration ledger without meta must fail closed");
    assert!(
        error
            .to_string()
            .contains("schema_migrations exists without main.meta")
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn interrupted_new_database_bootstrap_retries_with_seed_rows() {
    let db_path = temp_db_path("schema-migration-interrupted-new-database");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-interrupted-new-database".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create migrated database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove migration ledger");
    sqlx::query("DROP TABLE meta")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove schema identity table");
    sqlx::query("CREATE TABLE schema_bootstrap_state (marker TEXT PRIMARY KEY NOT NULL)")
        .execute(&proxy.key_store.pool)
        .await
        .expect("create bootstrap marker table");
    sqlx::query(
        "INSERT INTO schema_bootstrap_state (marker) VALUES ('tavily-hikari-schema-bootstrap-v1')",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("record bootstrap marker");

    assert!(
        proxy
            .key_store
            .prepare_versioned_schema()
            .await
            .expect("interrupted bootstrap must be retryable")
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn schema_startup_lock_rejects_concurrent_startup() {
    let db_path = temp_db_path("schema-migration-startup-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let first_lock = acquire_schema_startup_lock(&db_str).expect("acquire first startup lock");
    let error = acquire_schema_startup_lock(&db_str)
        .expect_err("active startup lock must reject another startup");
    assert!(error.to_string().contains("another schema startup"));

    drop(first_lock);
    let stem = db_path
        .file_stem()
        .and_then(|value| value.to_str())
        .expect("database stem");
    let _ = std::fs::remove_file(db_path.with_file_name(format!("{stem}-schema-startup.lock")));
}

#[tokio::test]
async fn schema_startup_lock_rejects_request_logs_gc_bootstrap() {
    let db_path = temp_db_path("schema-migration-gc-startup-lock");
    let db_str = db_path.to_string_lossy().to_string();
    let first_lock = acquire_schema_startup_lock(&db_str).expect("acquire startup lock");
    let error = KeyStore::open_for_request_logs_gc(&db_str)
        .await
        .expect_err("request logs GC bootstrap must honor startup lock");
    assert!(error.to_string().contains("another schema startup"));

    drop(first_lock);
    let stem = db_path
        .file_stem()
        .and_then(|value| value.to_str())
        .expect("database stem");
    let _ = std::fs::remove_file(db_path.with_file_name(format!("{stem}-schema-startup.lock")));
}

#[tokio::test]
async fn baseline_adoption_records_compatible_existing_schema_without_full_bootstrap() {
    let db_path = temp_db_path("schema-migration-compatible-adoption");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-compatible-adoption".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create compatible database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-ledger production database");

    assert!(
        proxy
            .key_store
            .prepare_versioned_schema()
            .await
            .expect("adopt compatible database"),
        "compatible adoption must converge schema before recording the baseline"
    );
    proxy
        .key_store
        .finish_new_database_schema_migrations()
        .await
        .expect("record compatible schema baseline");
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM schema_migrations ORDER BY version")
            .fetch_all(&proxy.key_store.pool)
            .await
            .expect("read adopted ledger");
    assert_eq!(
        versions,
        vec![
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23
        ]
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn baseline_adoption_rejects_runtime_schema_drift() {
    let db_path = temp_db_path("schema-migration-incomplete-baseline");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-incomplete-baseline".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create baseline database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-ledger production database");
    sqlx::query(
        "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, checksum TEXT NOT NULL, applied_at INTEGER NOT NULL)",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate interruption after ledger creation but before baseline record");
    sqlx::query("ALTER TABLE users DROP COLUMN debug_info_shared")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove a runtime-required historical column");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("runtime schema drift must reject adoption");
    assert!(
        error
            .to_string()
            .contains("missing main.users.debug_info_shared")
    );
    let recorded: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schema_migrations")
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("check interrupted ledger");
    assert_eq!(recorded, 0, "rejected drift must not record a baseline");

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn baseline_adoption_rejects_missing_source_schema_before_recording() {
    let db_path = temp_db_path("schema-migration-missing-source");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-missing-source".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("create baseline database");
    sqlx::query("DROP TABLE schema_migrations")
        .execute(&proxy.key_store.pool)
        .await
        .expect("simulate a pre-ledger production database");
    sqlx::query("ALTER TABLE billing_ledger RENAME TO billing_ledger_missing")
        .execute(&proxy.key_store.pool)
        .await
        .expect("remove an irreplaceable source table");

    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("missing source data schema must reject adoption");
    assert!(
        error
            .to_string()
            .contains("missing source table main.billing_ledger")
    );
    let ledger_exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("check migration ledger absence");
    assert_eq!(
        ledger_exists, 0,
        "rejected source schemas must not be recorded"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn warm_schema_verification_is_read_only_and_rejects_runtime_column_drift() {
    let db_path = temp_db_path("schema-migration-warm-read-only");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-schema-migration-warm-read-only".to_string()],
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
    let verified = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        proxy.key_store.prepare_versioned_schema(),
    )
    .await
    .expect("warm verification must not wait for the writer")
    .expect("warm verification succeeds");
    assert!(!verified);
    sqlx::query("ROLLBACK")
        .execute(&mut *lock)
        .await
        .expect("release writer lock");
    drop(lock);
    lock_pool.close().await;

    sqlx::query("ALTER TABLE users DROP COLUMN debug_info_shared")
        .execute(&proxy.key_store.pool)
        .await
        .expect("corrupt a required runtime column");
    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("warm verification must reject runtime column drift");
    assert!(
        error
            .to_string()
            .contains("missing main.users.debug_info_shared")
    );
    proxy
        .key_store
        .initialize_schema()
        .await
        .expect("repair the user column before checking request log drift");
    sqlx::query("ALTER TABLE observability.request_logs DROP COLUMN forwarded_headers")
        .execute(&proxy.key_store.pool)
        .await
        .expect("corrupt a request-log write column");
    let error = proxy
        .key_store
        .prepare_versioned_schema()
        .await
        .expect_err("warm verification must reject request-log write column drift");
    assert!(
        error
            .to_string()
            .contains("missing observability.request_logs.forwarded_headers")
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

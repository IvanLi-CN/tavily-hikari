use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_source_updates_rederive_current_group_identity() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(local_ts(2026, 9, 2, 12, 0));
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-source-identity"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let first_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-source-identity-a")
        .await
        .expect("create first upstream key");
    let second_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-source-identity-b")
        .await
        .expect("create second upstream key");
    let token_id = "source-identity-token";
    let period_code = "2026-09-02/S1";
    let insert_usage = r#"INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
          ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, 1, 2, 3)"#;
    for (key_id, project_id, billing_subject, period_start, period_end) in [
        (
            &first_key_id,
            "identity-a",
            "token:identity-a",
            100_i64,
            400_i64,
        ),
        (
            &second_key_id,
            "identity-m",
            "token:identity-m",
            200_i64,
            500_i64,
        ),
    ] {
        sqlx::query(insert_usage)
            .bind(token_id)
            .bind(key_id)
            .bind(period_code)
            .bind(project_id)
            .bind(billing_subject)
            .bind(period_start)
            .bind(period_end)
            .execute(&proxy.key_store.pool)
            .await
            .expect("insert reconciliation source row");
    }

    sqlx::query(
        "UPDATE upstream_reconciliation_usage SET token_id = 'source-identity-next-token' \
         WHERE token_id = ? AND key_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(&first_key_id)
    .bind(period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("move lexically first source row into a new token group");

    let old_group: (String, String, String, i64, i64, String, i64) = sqlx::query_as(
        "SELECT project_id, billing_subject, settlement_mode, period_start, period_end, \
         scheduling_key_id, work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rederived old source group");
    assert_eq!(
        old_group,
        (
            "identity-m".to_string(),
            "token:identity-m".to_string(),
            "shadow".to_string(),
            200,
            500,
            second_key_id,
            3,
        )
    );
    let new_group: (String, String, String, i64, i64, String, i64) = sqlx::query_as(
        "SELECT project_id, billing_subject, settlement_mode, period_start, period_end, \
         scheduling_key_id, work_generation FROM upstream_reconciliation_work \
         WHERE token_id = 'source-identity-next-token' AND period_code = ?",
    )
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rederived new source group");
    assert_eq!(
        new_group,
        (
            "identity-a".to_string(),
            "token:identity-a".to_string(),
            "shadow".to_string(),
            100,
            400,
            first_key_id,
            1,
        )
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reconciliation_source_key_removal_rederives_identity_and_fences_observations() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-source-key-removal".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
    )
    .await
    .expect("create proxy");
    let first_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-source-key-removal-a")
        .await
        .expect("create first upstream key");
    let second_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-source-key-removal-b")
        .await
        .expect("create second upstream key");
    let token_id = "source-key-removal-token";
    let period_code = "2026-09-02/S1";
    let insert_usage = r#"INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
          ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, 1, 2, 3)"#;
    for (key_id, project_id, billing_subject, period_start, period_end) in [
        (
            &first_key_id,
            "identity-a",
            "token:identity-a",
            100_i64,
            400_i64,
        ),
        (
            &second_key_id,
            "identity-m",
            "token:identity-m",
            200_i64,
            500_i64,
        ),
    ] {
        sqlx::query(insert_usage)
            .bind(token_id)
            .bind(key_id)
            .bind(period_code)
            .bind(project_id)
            .bind(billing_subject)
            .bind(period_start)
            .bind(period_end)
            .execute(&proxy.key_store.pool)
            .await
            .expect("insert reconciliation source row");
    }

    let initial_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read initial work generation");
    for key_id in [&first_key_id, &second_key_id] {
        sqlx::query(
            "INSERT INTO upstream_reconciliation_key_observations (\
             token_id, period_code, work_generation, key_id, upstream_usage, observed_at\
             ) VALUES (?, ?, ?, ?, 7, 3)",
        )
        .bind(token_id)
        .bind(period_code)
        .bind(initial_generation)
        .bind(key_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("record prior-generation observation");
    }

    sqlx::query(
        "DELETE FROM upstream_reconciliation_usage \
         WHERE token_id = ? AND key_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(&first_key_id)
    .bind(period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("remove one source Key");

    let retained_group: (String, String, i64, i64, String, i64, i64) = sqlx::query_as(
        "SELECT project_id, billing_subject, period_start, period_end, scheduling_key_id, \
                work_generation, next_attempt_at \
         FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rederived source group after key removal");
    assert_eq!(
        retained_group,
        (
            "identity-m".to_string(),
            "token:identity-m".to_string(),
            200,
            500,
            second_key_id.clone(),
            initial_generation + 1,
            0,
        )
    );
    let current_observations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_key_observations o \
         JOIN upstream_reconciliation_work w \
           ON w.token_id = o.token_id AND w.period_code = o.period_code \
         WHERE o.token_id = ? AND o.period_code = ? \
           AND o.work_generation = w.work_generation",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read current-generation observations");
    assert_eq!(
        current_observations, 0,
        "a removed Key must fence every prior-generation partial observation"
    );

    sqlx::query(
        "DELETE FROM upstream_reconciliation_usage \
         WHERE token_id = ? AND key_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(&second_key_id)
    .bind(period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("remove final source Key");
    let empty_group_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read fenced empty source group");
    assert_eq!(empty_group_generation, initial_generation + 2);

    drop(proxy);
    let reopened = TavilyProxy::with_endpoint(
        vec!["tvly-reconciliation-source-key-removal".to_string()],
        DEFAULT_UPSTREAM,
        &db_string,
    )
    .await
    .expect("warm reopen preserves the delete trigger migration");
    let persisted_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(token_id)
    .bind(period_code)
    .fetch_one(&reopened.key_store.pool)
    .await
    .expect("read fenced generation after restart");
    assert_eq!(persisted_generation, empty_group_generation);

    drop(reopened);
    let _ = std::fs::remove_file(db_path);
}

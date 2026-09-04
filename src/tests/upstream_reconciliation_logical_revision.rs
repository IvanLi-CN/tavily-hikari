use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_source_revisions_distinguish_storage_and_logical_timestamps() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 9, 2, 12, 0);
    let (backend_time, _) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-source-revision"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let first_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-source-revision-a")
        .await
        .expect("create first upstream key");
    let additional_key_ids = vec![
        proxy
            .add_or_undelete_key("tvly-reconciliation-source-revision-b")
            .await
            .expect("create second upstream key"),
        proxy
            .add_or_undelete_key("tvly-reconciliation-source-revision-c")
            .await
            .expect("create third upstream key"),
    ];
    let candidate = UpstreamReconciliationCandidate {
        token_id: "source-revision-token".to_string(),
        period_code: "2026-09-02/S1".to_string(),
        project_id: "source-revision-project".to_string(),
        billing_subject: "token:source-revision-token".to_string(),
        settlement_mode: "shadow".to_string(),
        period_start: now - 4_000,
        period_end: now - 900,
        pending_research: 0,
        degraded: false,
    };
    let insert_usage_sql = r#"INSERT INTO upstream_reconciliation_usage (
                 token_id, key_id, period_code, project_id, billing_subject,
                 period_start, period_end, request_count, first_used_at,
                 last_used_at, updated_at, settlement_mode
               ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')"#;
    sqlx::query(insert_usage_sql)
        .bind(&candidate.token_id)
        .bind(&first_key_id)
        .bind(&candidate.period_code)
        .bind(&candidate.project_id)
        .bind(&candidate.billing_subject)
        .bind(candidate.period_start)
        .bind(candidate.period_end)
        .bind(now - 1_000)
        .bind(now - 900)
        .bind(now - 900)
        .execute(&proxy.key_store.pool)
        .await
        .expect("insert initial reconciliation source");
    proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            1,
            &[ReconciliationKeyObservation {
                key_id: first_key_id.clone(),
                upstream_usage: 7,
            }],
            Some(ReconciliationWorkFence {
                work_generation: 1,
                claimed_job: None,
            }),
        )
        .await
        .expect("persist first-generation observation");
    sqlx::query(
        "UPDATE upstream_reconciliation_work \
         SET next_attempt_at = ?, last_outcome = 'remote_attempt_budget' \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(now + 30)
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed partial work state");

    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_usage (
             token_id, key_id, period_code, project_id, billing_subject,
             period_start, period_end, request_count, first_used_at,
             last_used_at, updated_at, settlement_mode
           ) VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, 'shadow')
           ON CONFLICT(token_id, key_id, period_code) DO UPDATE SET
             project_id = excluded.project_id,
             billing_subject = excluded.billing_subject,
             period_start = excluded.period_start,
             period_end = excluded.period_end,
             request_count = excluded.request_count,
             first_used_at = excluded.first_used_at,
             last_used_at = excluded.last_used_at,
             updated_at = excluded.updated_at,
             settlement_mode = excluded.settlement_mode"#,
    )
    .bind(&candidate.token_id)
    .bind(&first_key_id)
    .bind(&candidate.period_code)
    .bind(&candidate.project_id)
    .bind(&candidate.billing_subject)
    .bind(candidate.period_start)
    .bind(candidate.period_end)
    .bind(now - 1_000)
    .bind(now - 900)
    .bind(now - 900)
    .execute(&proxy.key_store.pool)
    .await
    .expect("replay an equal reconciliation source payload");
    let replayed_state: (i64, i64, i64, Option<String>) = sqlx::query_as(
        "SELECT work_generation, completed_generation, next_attempt_at, last_outcome \
         FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read replayed work state");
    assert_eq!(
        replayed_state,
        (1, 0, now + 30, Some("remote_attempt_budget".to_string()))
    );

    sqlx::query(
        "UPDATE upstream_reconciliation_usage \
         SET updated_at = updated_at + 1 \
         WHERE token_id = ? AND key_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&first_key_id)
    .bind(&candidate.period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("refresh storage-only source timestamp");
    let no_op_state: (i64, i64, i64, Option<String>) = sqlx::query_as(
        "SELECT work_generation, completed_generation, next_attempt_at, last_outcome \
         FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read storage-only refresh work state");
    assert_eq!(
        no_op_state,
        (1, 0, now + 30, Some("remote_attempt_budget".to_string()))
    );
    let first_generation_observations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_key_observations \
         WHERE token_id = ? AND period_code = ? AND work_generation = 1",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count preserved storage-only observations");
    assert_eq!(first_generation_observations, 1);

    for (field, delta, expected_generation) in [
        ("first_used_at", -1_i64, 2_i64),
        ("last_used_at", 1_i64, 3_i64),
    ] {
        sqlx::query(&format!(
            "UPDATE upstream_reconciliation_usage SET {field} = {field} + {delta} \
             WHERE token_id = ? AND key_id = ? AND period_code = ?",
        ))
        .bind(&candidate.token_id)
        .bind(&first_key_id)
        .bind(&candidate.period_code)
        .execute(&proxy.key_store.pool)
        .await
        .expect("record logical source timestamp revision");
        let timestamp_state: (i64, i64, Option<String>) = sqlx::query_as(
            "SELECT work_generation, next_attempt_at, last_outcome \
             FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = ?",
        )
        .bind(&candidate.token_id)
        .bind(&candidate.period_code)
        .fetch_one(&proxy.key_store.pool)
        .await
        .expect("read logical timestamp revision");
        assert_eq!(timestamp_state, (expected_generation, 0, None), "{field}");
    }

    sqlx::query(
        "UPDATE upstream_reconciliation_usage SET request_count = request_count + 1 \
         WHERE token_id = ? AND key_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&first_key_id)
    .bind(&candidate.period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("record logical source revision");
    let reopened_state: (i64, i64, Option<String>) = sqlx::query_as(
        "SELECT work_generation, next_attempt_at, last_outcome \
         FROM upstream_reconciliation_work WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read logically reopened work state");
    assert_eq!(reopened_state, (4, 0, None));
    proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            4,
            &[ReconciliationKeyObservation {
                key_id: first_key_id.clone(),
                upstream_usage: 8,
            }],
            Some(ReconciliationWorkFence {
                work_generation: 4,
                claimed_job: None,
            }),
        )
        .await
        .expect("persist logically reopened observation");

    for key_id in &additional_key_ids {
        sqlx::query(insert_usage_sql)
            .bind(&candidate.token_id)
            .bind(key_id)
            .bind(&candidate.period_code)
            .bind(&candidate.project_id)
            .bind(&candidate.billing_subject)
            .bind(candidate.period_start)
            .bind(candidate.period_end)
            .bind(now - 1_000)
            .bind(now - 900)
            .bind(now - 900)
            .execute(&proxy.key_store.pool)
            .await
            .expect("add current reconciliation key");
    }
    let key_set_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read key-set revision generation");
    assert_eq!(key_set_generation, 6);
    let retained_observations: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_key_observations \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count retained pre-terminal observations");
    assert_eq!(retained_observations, 2);
    let ordered_key_sets = proxy
        .key_store
        .reconciliation_key_ids_batch(&[(
            candidate.token_id.clone(),
            candidate.period_code.clone(),
        )])
        .await
        .expect("read deterministically ordered current key set");
    let mut expected_key_ids = vec![first_key_id];
    expected_key_ids.extend(additional_key_ids);
    expected_key_ids.sort();
    assert_eq!(
        ordered_key_sets.get(&(candidate.token_id.clone(), candidate.period_code.clone())),
        Some(&expected_key_ids)
    );
    let current_generation_observations = proxy
        .key_store
        .reconciliation_key_observations(&candidate, key_set_generation, &expected_key_ids)
        .await
        .expect("read current generation observations");
    assert!(current_generation_observations.is_empty());

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

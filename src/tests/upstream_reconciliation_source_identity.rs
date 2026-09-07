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

#[tokio::test]
async fn reconciliation_key_source_identity_reuses_unchanged_keys() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let (backend_time, _) = BackendTime::manual_from_ts(local_ts(2026, 9, 2, 12, 0));
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-key-source-identity"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let first_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-key-source-identity-a")
        .await
        .expect("create first upstream key");
    let second_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-key-source-identity-b")
        .await
        .expect("create second upstream key");
    let candidate = UpstreamReconciliationCandidate {
        token_id: "key-source-identity-token".to_string(),
        period_code: "2026-09-02/S1".to_string(),
        project_id: "key-source-identity-project".to_string(),
        billing_subject: "token:key-source-identity-token".to_string(),
        settlement_mode: "shadow".to_string(),
        period_start: 100,
        period_end: 200,
        pending_research: 0,
        degraded: false,
    };
    let insert_usage = r#"INSERT INTO upstream_reconciliation_usage (
            token_id, key_id, period_code, project_id, billing_subject, settlement_mode,
            period_start, period_end, request_count, first_used_at, last_used_at, updated_at
          ) VALUES (?, ?, ?, ?, ?, 'shadow', ?, ?, 1, 1, 2, 3)"#;
    for key_id in [&first_key_id, &second_key_id] {
        sqlx::query(insert_usage)
            .bind(&candidate.token_id)
            .bind(key_id)
            .bind(&candidate.period_code)
            .bind(&candidate.project_id)
            .bind(&candidate.billing_subject)
            .bind(candidate.period_start)
            .bind(candidate.period_end)
            .execute(&proxy.key_store.pool)
            .await
            .expect("insert reconciliation source row");
    }
    let current_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read initial work generation");
    proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            current_generation,
            &[
                ReconciliationKeyObservation {
                    key_id: first_key_id.clone(),
                    upstream_usage: 7,
                },
                ReconciliationKeyObservation {
                    key_id: second_key_id.clone(),
                    upstream_usage: 11,
                },
            ],
            Some(ReconciliationWorkFence {
                work_generation: current_generation,
                claimed_job: None,
            }),
        )
        .await
        .expect("persist initial observations");

    sqlx::query(
        "UPDATE upstream_reconciliation_usage SET request_count = request_count + 1 \
         WHERE token_id = ? AND key_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&first_key_id)
    .bind(&candidate.period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("change one Key's logical source identity");
    let next_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read reopened work generation");
    assert_eq!(next_generation, current_generation + 1);

    let observations = proxy
        .key_store
        .reconciliation_key_observations(
            &candidate,
            next_generation,
            &[first_key_id.clone(), second_key_id.clone()],
        )
        .await
        .expect("read observations after one Key changes");
    assert_eq!(
        observations.get(&second_key_id),
        Some(&11),
        "an unchanged Key observation must survive another Key's logical source revision"
    );
    assert!(
        !observations.contains_key(&first_key_id),
        "the changed Key must be observed again before terminal reconciliation"
    );

    let third_key_id = proxy
        .add_or_undelete_key("tvly-reconciliation-key-source-identity-c")
        .await
        .expect("create third upstream key");
    sqlx::query(insert_usage)
        .bind(&candidate.token_id)
        .bind(&third_key_id)
        .bind(&candidate.period_code)
        .bind(&candidate.project_id)
        .bind(&candidate.billing_subject)
        .bind(candidate.period_start)
        .bind(candidate.period_end)
        .execute(&proxy.key_store.pool)
        .await
        .expect("add a Key to the logical source set");
    let key_set_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read work generation after Key set change");
    assert_eq!(key_set_generation, next_generation + 1);
    let key_set_changed_observations = proxy
        .key_store
        .reconciliation_key_observations(
            &candidate,
            key_set_generation,
            &[
                first_key_id.clone(),
                second_key_id.clone(),
                third_key_id.clone(),
            ],
        )
        .await
        .expect("read observations after a Key set change");
    assert!(
        key_set_changed_observations.is_empty(),
        "a different Key set must fence every partial observation"
    );

    let changed_billing_subject = "token:key-source-identity-next";
    sqlx::query(
        "UPDATE upstream_reconciliation_usage SET billing_subject = ? \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(changed_billing_subject)
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("change the candidate-global logical source identity");
    let globally_changed_candidate = UpstreamReconciliationCandidate {
        billing_subject: changed_billing_subject.to_string(),
        ..candidate.clone()
    };
    let global_generation: i64 = sqlx::query_scalar(
        "SELECT work_generation FROM upstream_reconciliation_work \
         WHERE token_id = ? AND period_code = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read work generation after candidate identity change");
    assert!(
        global_generation > key_set_generation,
        "each changed source row may advance the existing work-generation trigger"
    );
    let global_changed_observations = proxy
        .key_store
        .reconciliation_key_observations(
            &globally_changed_candidate,
            global_generation,
            &[
                first_key_id.clone(),
                second_key_id.clone(),
                third_key_id.clone(),
            ],
        )
        .await
        .expect("read observations after a candidate-global change");
    assert!(
        global_changed_observations.is_empty(),
        "a changed candidate-global identity must fence every partial observation"
    );

    proxy
        .key_store
        .persist_reconciliation_key_observations(
            &globally_changed_candidate,
            global_generation,
            &[ReconciliationKeyObservation {
                key_id: third_key_id.clone(),
                upstream_usage: 17,
            }],
            Some(ReconciliationWorkFence {
                work_generation: global_generation,
                claimed_job: None,
            }),
        )
        .await
        .expect("persist a current observation before simulating v31 state");
    sqlx::query(
        "UPDATE upstream_reconciliation_key_observations SET candidate_identity = '' \
         WHERE token_id = ? AND period_code = ? AND key_id = ?",
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .bind(&third_key_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate a legacy observation without source identity");
    let legacy_observations = proxy
        .key_store
        .reconciliation_key_observations(
            &globally_changed_candidate,
            global_generation,
            &[third_key_id],
        )
        .await
        .expect("read legacy observations safely");
    assert!(
        legacy_observations.is_empty(),
        "observations written before v32 must conservatively re-read their Key"
    );

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

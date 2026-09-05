use super::upstream_reconciliation::{local_ts, reconciliation_test_db_path};
use super::*;

#[tokio::test]
async fn reconciliation_key_observations_reject_stale_generation_and_claim() {
    let db_path = reconciliation_test_db_path();
    let db_string = db_path.to_string_lossy().to_string();
    let now = local_ts(2026, 7, 15, 12, 0);
    let (backend_time, clock) = BackendTime::manual_from_ts(now);
    let proxy = TavilyProxy::with_options_and_time(
        vec!["tvly-reconciliation-key-observation-fence"],
        DEFAULT_UPSTREAM,
        &db_string,
        TavilyProxyOptions::from_database_path(&db_string),
        backend_time,
    )
    .await
    .expect("create proxy");
    let candidate = UpstreamReconciliationCandidate {
        token_id: "key-observation-fence-token".to_string(),
        period_code: "2026-07-15/S1".to_string(),
        project_id: "key-observation-fence-project".to_string(),
        billing_subject: "token:key-observation-fence-token".to_string(),
        settlement_mode: "shadow".to_string(),
        period_start: now - 4_000,
        period_end: now - 900,
        pending_research: 0,
        degraded: false,
    };
    sqlx::query(
        r#"INSERT INTO upstream_reconciliation_work (
             token_id, period_code, project_id, billing_subject, settlement_mode,
             period_start, period_end, scheduling_key_id, updated_at
           ) VALUES (?, ?, ?, ?, ?, ?, ?, 'key-observation-fence-key', ?)
        "#,
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .bind(&candidate.project_id)
    .bind(&candidate.billing_subject)
    .bind(&candidate.settlement_mode)
    .bind(candidate.period_start)
    .bind(candidate.period_end)
    .bind(now)
    .execute(&proxy.key_store.pool)
    .await
    .expect("insert durable work");
    proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            1,
            &[ReconciliationKeyObservation {
                key_id: "key-observation-fence-key".to_string(),
                upstream_usage: 4,
            }],
            Some(ReconciliationWorkFence {
                work_generation: 1,
                claimed_job: None,
            }),
        )
        .await
        .expect("persist a partial observation before stale work");

    let stale_generation = proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            2,
            &[ReconciliationKeyObservation {
                key_id: "key-observation-fence-key".to_string(),
                upstream_usage: 7,
            }],
            Some(ReconciliationWorkFence {
                work_generation: 2,
                claimed_job: None,
            }),
        )
        .await
        .expect("stale generation is handled");
    assert!(matches!(
        stale_generation,
        ReconciliationKeyObservationPersistOutcome::StaleGeneration
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_key_observations WHERE token_id = ?",
    )
    .bind(&candidate.token_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rejected generation observations");
    assert_eq!(
        count, 1,
        "a stale generation must retain prior observations"
    );

    let queued = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue representative");
    let claim = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim representative")
        .expect("representative is claimed");
    clock.set_now_ts(now + 61);
    assert_eq!(proxy.recover_stale_scheduled_jobs().await.unwrap(), 1);
    let stale_claim = proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            1,
            &[ReconciliationKeyObservation {
                key_id: "key-observation-fence-key".to_string(),
                upstream_usage: 7,
            }],
            Some(ReconciliationWorkFence {
                work_generation: 1,
                claimed_job: Some((claim.id, claim.claim_generation)),
            }),
        )
        .await
        .expect("stale claim is handled");
    assert!(matches!(
        stale_claim,
        ReconciliationKeyObservationPersistOutcome::StaleClaim
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM upstream_reconciliation_key_observations WHERE token_id = ?",
    )
    .bind(&candidate.token_id)
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read rejected stale-claim observations");
    assert_eq!(count, 1, "a stale claim must retain prior observations");

    sqlx::query(
        "UPDATE scheduled_jobs SET available_at = 0 WHERE job_type = 'upstream_reconciliation' AND status = 'queued'",
    )
    .execute(&proxy.key_store.pool)
    .await
    .expect("make recovered representative due");
    let queued_generation_change = proxy
        .scheduled_job_enqueue("upstream_reconciliation", "auto", None, 1)
        .await
        .expect("enqueue generation-change representative");
    let generation_claim = proxy
        .scheduled_job_mark_running(queued_generation_change.job_id)
        .await
        .expect("claim generation-change representative")
        .expect("generation-change representative is claimed");
    sqlx::query(
        r#"UPDATE upstream_reconciliation_work
              SET work_generation = 2, completed_generation = 0
            WHERE token_id = ? AND period_code = ?"#,
    )
    .bind(&candidate.token_id)
    .bind(&candidate.period_code)
    .execute(&proxy.key_store.pool)
    .await
    .expect("advance work generation while claim remains current");
    let generation_changed = proxy
        .key_store
        .persist_reconciliation_key_observations(
            &candidate,
            1,
            &[ReconciliationKeyObservation {
                key_id: "key-observation-fence-key".to_string(),
                upstream_usage: 7,
            }],
            Some(ReconciliationWorkFence {
                work_generation: 1,
                claimed_job: Some((generation_claim.id, generation_claim.claim_generation)),
            }),
        )
        .await
        .expect("generation change is classified without losing the claim");
    assert!(matches!(
        generation_changed,
        ReconciliationKeyObservationPersistOutcome::StaleGeneration
    ));
    let continuation = proxy
        .finalize_deferred_upstream_reconciliation_claim(
            generation_claim.id,
            generation_claim.claim_generation,
            RECONCILIATION_RETRY_REASON_GENERATION_CHANGED,
            now + 90,
        )
        .await
        .expect("generation change has a durable continuation");
    let generation_claim_status: String =
        sqlx::query_scalar("SELECT status FROM scheduled_jobs WHERE id = ?")
            .bind(generation_claim.id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read generation-change claim status");
    assert_eq!(generation_claim_status, "success");
    let active_representatives: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM scheduled_jobs WHERE job_type = 'upstream_reconciliation' AND status IN ('queued', 'running')",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("count generation-change representatives");
    assert_eq!(active_representatives, 1);
    let continuation_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(continuation.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read generation-change continuation");
    assert_eq!(continuation_at, now + 90);

    drop(proxy);
    let _ = std::fs::remove_file(db_path);
}

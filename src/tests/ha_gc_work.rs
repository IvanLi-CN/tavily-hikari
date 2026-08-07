use super::*;

#[tokio::test]
async fn ha_gc_work_fences_stale_generations_without_cross_channel_blocking() {
    let db_path = temp_db_path("ha-gc-work-generation-fence");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-generation-fence".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let now = Utc::now().timestamp();

    let control_claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Control)
        .await
        .expect("claim control work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected control claim, got {other:?}"),
    };
    let billing_claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Billing)
        .await
        .expect("claim billing work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected billing claim, got {other:?}"),
    };

    assert_eq!(
        control_claim.claim_generation,
        billing_claim.claim_generation
    );
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("UPDATE ha_outbox_gc_work SET claim_expires_at = ? WHERE channel = 'control'")
        .bind(now - 1)
        .execute(&pool)
        .await
        .expect("expire control claim");
    let replacement_control_claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Control)
        .await
        .expect("take over expired control work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected replacement control claim, got {other:?}"),
    };
    assert_eq!(
        replacement_control_claim.claim_generation,
        control_claim.claim_generation + 1
    );
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(control_claim, HaOutboxGcWorkOutcome::Completed, now + 3600,)
            .await
            .expect("reject expired control claim"),
        HaOutboxGcWorkFinishResult::Stale
    ));
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(
                replacement_control_claim,
                HaOutboxGcWorkOutcome::Deferred,
                now + 300,
            )
            .await
            .expect("finish control work"),
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Deferred)
    ));
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(control_claim, HaOutboxGcWorkOutcome::Completed, now + 3600,)
            .await
            .expect("fence stale control completion"),
        HaOutboxGcWorkFinishResult::Stale
    ));
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(billing_claim, HaOutboxGcWorkOutcome::Completed, now + 3600,)
            .await
            .expect("finish billing work"),
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Completed)
    ));

    let control_row = sqlx::query(
        "SELECT last_outcome, eligible_at, claim_generation, claim_started_at FROM ha_outbox_gc_work WHERE channel = 'control'",
    )
    .fetch_one(&pool)
    .await
    .expect("read control work");
    assert_eq!(
        control_row
            .try_get::<String, _>("last_outcome")
            .expect("control outcome"),
        "deferred"
    );
    assert_eq!(
        control_row
            .try_get::<i64, _>("eligible_at")
            .expect("control eligibility"),
        now + 300
    );
    assert_eq!(
        control_row
            .try_get::<i64, _>("claim_generation")
            .expect("control generation"),
        replacement_control_claim.claim_generation
    );
    assert!(
        control_row
            .try_get::<Option<i64>, _>("claim_started_at")
            .expect("control claim state")
            .is_none()
    );

    pool.close().await;
    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_work_claim_maps_writer_contention_to_typed_busy() {
    let db_path = temp_db_path("ha-gc-work-busy");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-busy".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let lock = begin_immediate_sqlite_connection(&proxy.key_store.pool)
        .await
        .expect("hold HA work writer lock");
    let started = std::time::Instant::now();

    assert_eq!(
        proxy
            .claim_ha_outbox_gc_work(HaSyncChannel::Runtime)
            .await
            .expect("claim under writer contention"),
        HaOutboxGcWorkClaimResult::Busy
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(750));
    lock.rollback().await.expect("release HA work writer lock");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_work_busy_handoff_does_not_overwrite_completed_generation() {
    let db_path = temp_db_path("ha-gc-work-stale-busy-handoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-stale-busy-handoff".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let now = Utc::now().timestamp();
    let claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Runtime)
        .await
        .expect("claim runtime work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected runtime work claim, got {other:?}"),
    };
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(claim, HaOutboxGcWorkOutcome::Completed, now + 3600)
            .await
            .expect("finish runtime work"),
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Completed)
    ));
    let job = proxy
        .scheduled_job_enqueue("ha_outbox_gc/runtime", "scheduler", None, 1)
        .await
        .expect("enqueue stale handoff job");
    let running_job = proxy
        .scheduled_job_mark_running(job.job_id)
        .await
        .expect("claim stale handoff job")
        .expect("stale handoff job is queued");

    let (result, continuation) = proxy
        .defer_ha_outbox_gc_job_and_enqueue(
            running_job.id,
            running_job.claim_generation,
            HaSyncChannel::Runtime,
            HaOutboxGcWorkOutcome::Busy,
            now + 30,
            "deferred=stale_busy",
            now + 30,
        )
        .await
        .expect("persist stale handoff");
    assert_eq!(result, HaOutboxGcWorkFinishResult::Stale);
    assert!(continuation.is_none());

    let row = sqlx::query(
        "SELECT last_outcome, eligible_at FROM ha_outbox_gc_work WHERE channel = 'runtime'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read fenced work");
    assert_eq!(
        row.try_get::<String, _>("last_outcome").expect("outcome"),
        "completed"
    );
    assert_eq!(
        row.try_get::<i64, _>("eligible_at").expect("eligibility"),
        now + 3600
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_work_busy_handoff_does_not_overwrite_newer_deferred_generation() {
    let db_path = temp_db_path("ha-gc-work-stale-deferred-handoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-stale-deferred-handoff".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let job = proxy
        .scheduled_job_enqueue("ha_outbox_gc/runtime", "scheduler", None, 1)
        .await
        .expect("enqueue runtime GC job");
    let running_job = proxy
        .scheduled_job_mark_running(job.job_id)
        .await
        .expect("claim scheduled job")
        .expect("scheduled job is queued");
    let claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Runtime)
        .await
        .expect("claim runtime work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected runtime work claim, got {other:?}"),
    };
    let deferred_at = Utc::now().timestamp() + 300;
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(claim, HaOutboxGcWorkOutcome::Deferred, deferred_at)
            .await
            .expect("finish newer runtime work"),
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Deferred)
    ));

    let (finish, continuation) = proxy
        .defer_ha_outbox_gc_job_and_enqueue(
            running_job.id,
            running_job.claim_generation,
            HaSyncChannel::Runtime,
            HaOutboxGcWorkOutcome::Busy,
            Utc::now().timestamp() + 30,
            "deferred=sqlite_busy",
            Utc::now().timestamp() + 30,
        )
        .await
        .expect("persist stale busy handoff");
    assert_eq!(finish, HaOutboxGcWorkFinishResult::Stale);
    assert!(continuation.is_none());

    let work_row = sqlx::query(
        "SELECT last_outcome, eligible_at FROM ha_outbox_gc_work WHERE channel = 'runtime'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read fenced runtime work");
    assert_eq!(
        work_row
            .try_get::<String, _>("last_outcome")
            .expect("runtime outcome"),
        "deferred"
    );
    assert_eq!(
        work_row
            .try_get::<i64, _>("eligible_at")
            .expect("runtime eligibility"),
        deferred_at
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_work_due_channels_survive_restart_independently() {
    let db_path = temp_db_path("ha-gc-work-restart-independence");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-restart-independence".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let now = Utc::now().timestamp();
    let control_claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Control)
        .await
        .expect("claim delayed control work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected control claim, got {other:?}"),
    };
    assert!(matches!(
        proxy
            .finish_ha_outbox_gc_work(control_claim, HaOutboxGcWorkOutcome::Deferred, now + 300)
            .await
            .expect("persist delayed control work"),
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Deferred)
    ));
    drop(proxy);

    let restarted = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-restart-independence".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("restarted proxy created");
    let due_channels = match restarted
        .ha_outbox_gc_work_due_channels()
        .await
        .expect("read due channels after restart")
    {
        HaOutboxGcWorkDueChannelsResult::Ready(channels) => channels,
        HaOutboxGcWorkDueChannelsResult::Busy => panic!("restart due read was busy"),
    };
    assert!(!due_channels.contains(&HaSyncChannel::Control));
    assert!(due_channels.contains(&HaSyncChannel::Billing));
    assert!(due_channels.contains(&HaSyncChannel::Runtime));

    drop(restarted);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_outbox_update_triggers_only_emit_when_wire_payload_changes() {
    let db_path = temp_db_path("ha-outbox-update-wire-noop");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-outbox-update-wire-noop".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    repair_ha_triggers_once(&db_str, HaMode::ActiveStandby)
        .await
        .expect("HA triggers repaired");
    let pool = connect_sqlite_test_pool(&db_str).await;
    sqlx::query("DELETE FROM ha_outbox_suppression WHERE id = 'local'")
        .execute(&pool)
        .await
        .expect("enable HA outbox writes");
    sqlx::query("INSERT OR REPLACE INTO meta (key, value) VALUES ('request_rate_limit_v1', '{}')")
        .execute(&pool)
        .await
        .expect("seed meta row");
    let before_noop: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'meta'")
            .fetch_one(&pool)
            .await
            .expect("count initial meta events");

    sqlx::query("UPDATE meta SET value = '{}' WHERE key = 'request_rate_limit_v1'")
        .execute(&pool)
        .await
        .expect("apply wire-identical update");
    let after_noop: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'meta'")
            .fetch_one(&pool)
            .await
            .expect("count wire-identical update events");
    assert_eq!(after_noop, before_noop);

    sqlx::query("UPDATE meta SET value = '{\"limit\":1}' WHERE key = 'request_rate_limit_v1'")
        .execute(&pool)
        .await
        .expect("apply effective update");
    let after_change: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ha_outbox WHERE resource = 'meta'")
            .fetch_one(&pool)
            .await
            .expect("count effective update events");
    assert_eq!(after_change, after_noop + 1);

    sqlx::query(
        "INSERT OR IGNORE INTO api_keys (id, api_key, status, created_at) VALUES ('ha-wire-key', 'tvly-ha-wire-key', 'active', 1)",
    )
    .execute(&pool)
    .await
    .expect("seed maintenance record key");
    let before_maintenance: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ha_outbox WHERE resource = 'api_key_maintenance_records'",
    )
    .fetch_one(&pool)
    .await
    .expect("count maintenance events");
    sqlx::query(
        "INSERT INTO api_key_maintenance_records (id, key_id, source, operation_code, operation_summary, request_log_id, created_at) VALUES ('ha-wire-maintenance', 'ha-wire-key', 'test', 'unlink', 'unlink request log', 41, 1)",
    )
    .execute(&pool)
    .await
    .expect("seed maintenance record");
    let after_maintenance_insert: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ha_outbox WHERE resource = 'api_key_maintenance_records'",
    )
    .fetch_one(&pool)
    .await
    .expect("count inserted maintenance event");
    assert_eq!(after_maintenance_insert, before_maintenance + 1);
    sqlx::query(
        "UPDATE api_key_maintenance_records SET request_log_id = NULL WHERE id = 'ha-wire-maintenance'",
    )
    .execute(&pool)
    .await
    .expect("unlink request log");
    let after_maintenance_noop: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ha_outbox WHERE resource = 'api_key_maintenance_records'",
    )
    .fetch_one(&pool)
    .await
    .expect("count wire-identical maintenance update");
    assert_eq!(after_maintenance_noop, after_maintenance_insert);
    sqlx::query(
        "UPDATE api_key_maintenance_records SET operation_summary = 'effective change' WHERE id = 'ha-wire-maintenance'",
    )
    .execute(&pool)
    .await
    .expect("apply effective maintenance update");
    let after_maintenance_change: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM ha_outbox WHERE resource = 'api_key_maintenance_records'",
    )
    .fetch_one(&pool)
    .await
    .expect("count effective maintenance update");
    assert_eq!(after_maintenance_change, after_maintenance_noop + 1);

    pool.close().await;
    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_work_finish_commits_job_and_channel_continuation_atomically() {
    let db_path = temp_db_path("ha-gc-work-atomic-finish");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-atomic-finish".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let job = proxy
        .scheduled_job_enqueue("ha_outbox_gc/control", "scheduler", None, 1)
        .await
        .expect("enqueue channel GC job");
    let running_job = proxy
        .scheduled_job_mark_running(job.job_id)
        .await
        .expect("claim scheduled job")
        .expect("scheduled job is queued");
    let claim = match proxy
        .claim_ha_outbox_gc_work(HaSyncChannel::Control)
        .await
        .expect("claim channel work")
    {
        HaOutboxGcWorkClaimResult::Claimed(claim) => claim,
        other => panic!("expected channel work claim, got {other:?}"),
    };
    let continuation_at = Utc::now().timestamp() + 30;
    let (finish, continuation) = proxy
        .finish_ha_outbox_gc_work_and_enqueue(
            running_job.id,
            running_job.claim_generation,
            claim,
            HaOutboxGcWorkOutcome::Deferred,
            continuation_at,
            "success",
            Some("deferred=slice_budget_exhausted"),
            Some("ha_outbox_gc/control"),
            Some(continuation_at),
            3,
        )
        .await
        .expect("finish channel work atomically");
    assert_eq!(
        finish,
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Deferred)
    );
    let continuation = continuation.expect("continuation enqueued");
    assert!(continuation.created);
    assert_eq!(continuation.status, "queued");

    let finished_job = proxy
        .scheduled_job_by_id(running_job.id)
        .await
        .expect("read finished job")
        .expect("finished job exists");
    assert_eq!(finished_job.status, "success");
    let continuation_job = proxy
        .scheduled_job_by_id(continuation.job_id)
        .await
        .expect("read continuation job")
        .expect("continuation job exists");
    assert_eq!(continuation_job.status, "queued");

    let pool = connect_sqlite_test_pool(&db_str).await;
    let persisted_continuation_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(continuation.job_id)
            .fetch_one(&pool)
            .await
            .expect("read continuation availability");
    assert_eq!(persisted_continuation_at, continuation_at);
    let work_row = sqlx::query(
        "SELECT last_outcome, last_deleted_rows, claim_started_at FROM ha_outbox_gc_work WHERE channel = 'control'",
    )
    .fetch_one(&pool)
    .await
    .expect("read finished channel work");
    assert_eq!(
        work_row
            .try_get::<String, _>("last_outcome")
            .expect("work outcome"),
        "deferred"
    );
    assert_eq!(
        work_row
            .try_get::<i64, _>("last_deleted_rows")
            .expect("work deleted rows"),
        3
    );
    assert!(
        work_row
            .try_get::<Option<i64>, _>("claim_started_at")
            .expect("work claim state")
            .is_none()
    );

    pool.close().await;
    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_work_busy_handoff_requeues_channel_job_atomically() {
    let db_path = temp_db_path("ha-gc-work-busy-handoff");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(
        vec!["tvly-ha-gc-work-busy-handoff".to_string()],
        DEFAULT_UPSTREAM,
        &db_str,
    )
    .await
    .expect("proxy created");
    let job = proxy
        .scheduled_job_enqueue("ha_outbox_gc/runtime", "scheduler", None, 1)
        .await
        .expect("enqueue runtime GC job");
    let running_job = proxy
        .scheduled_job_mark_running(job.job_id)
        .await
        .expect("claim scheduled job")
        .expect("scheduled job is queued");
    let continuation_at = Utc::now().timestamp() + 30;
    let (finish, continuation) = proxy
        .defer_ha_outbox_gc_job_and_enqueue(
            running_job.id,
            running_job.claim_generation,
            HaSyncChannel::Runtime,
            HaOutboxGcWorkOutcome::Busy,
            continuation_at,
            "deferred=sqlite_busy",
            continuation_at,
        )
        .await
        .expect("persist busy handoff");
    assert_eq!(
        finish,
        HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Busy)
    );
    let continuation = continuation.expect("continuation enqueued");
    assert!(continuation.created);

    let finished_job = proxy
        .scheduled_job_by_id(running_job.id)
        .await
        .expect("read finished job")
        .expect("finished job exists");
    assert_eq!(finished_job.status, "success");
    let work_row = sqlx::query(
        "SELECT last_outcome, eligible_at, claim_started_at FROM ha_outbox_gc_work WHERE channel = 'runtime'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read busy work state");
    assert_eq!(
        work_row
            .try_get::<String, _>("last_outcome")
            .expect("busy outcome"),
        "busy"
    );
    assert_eq!(
        work_row
            .try_get::<i64, _>("eligible_at")
            .expect("busy eligibility"),
        continuation_at
    );
    assert!(
        work_row
            .try_get::<Option<i64>, _>("claim_started_at")
            .expect("busy claim state")
            .is_none()
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

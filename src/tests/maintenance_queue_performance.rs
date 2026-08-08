use super::*;

#[tokio::test]
async fn scheduled_job_aging_prevents_request_logs_gc_from_starving_ha_gc() {
    let db_path = temp_db_path("scheduled-job-priority-aging");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");

    let ha_gc = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "scheduler", None, 1)
        .await
        .expect("enqueue HA outbox gc");
    let request_logs_gc = proxy
        .scheduled_job_enqueue("request_logs_gc", "scheduler", None, 1)
        .await
        .expect("enqueue request logs gc");
    let aged_at = Utc::now().timestamp() - 5 * 60;
    sqlx::query("UPDATE scheduled_jobs SET queued_at = ? WHERE id = ?")
        .bind(aged_at)
        .bind(ha_gc.job_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("age HA outbox gc job");

    let queued = proxy
        .fetch_queued_scheduled_jobs(2)
        .await
        .expect("fetch queued jobs");
    assert_eq!(
        queued[0].id, ha_gc.job_id,
        "an HA GC job that has waited five minutes must run before a fresh request-log GC continuation"
    );
    assert_eq!(queued[1].id, request_logs_gc.job_id);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn ha_gc_continuation_finishes_job_and_requeues_atomically() {
    let db_path = temp_db_path("ha-gc-continuation-transaction");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let job = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "scheduler", None, 1)
        .await
        .expect("enqueue HA GC");
    let claimed = proxy
        .scheduled_job_mark_running(job.job_id)
        .await
        .expect("mark HA GC running")
        .expect("claimed HA GC job");
    let available_at = Utc::now().timestamp() + 30;
    let continuation = proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            job.job_id,
            claimed.claim_generation,
            "ha_outbox_gc",
            None,
            1,
            Some("deferred=foreground_pressure"),
            available_at,
        )
        .await
        .expect("finish and enqueue continuation");
    assert!(continuation.created);
    assert_eq!(continuation.trigger_source, "auto");
    assert_eq!(
        proxy
            .scheduled_job_by_id(job.job_id)
            .await
            .expect("read finished job")
            .expect("finished job exists")
            .status,
        "success"
    );
    let queued = proxy
        .scheduled_job_by_id(continuation.job_id)
        .await
        .expect("read continuation")
        .expect("continuation exists");
    assert_eq!(queued.status, "queued");
    assert_eq!(queued.trigger_source, "auto");
    let (queued_at, queued_available_at): (i64, i64) =
        sqlx::query_as("SELECT queued_at, available_at FROM scheduled_jobs WHERE id = ?")
            .bind(continuation.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read continuation availability");
    assert_eq!(queued_available_at, available_at);
    let (next_retry_at, last_defer_reason, last_continuation_delay_secs): (
        Option<i64>,
        Option<String>,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT next_retry_at, last_defer_reason, last_continuation_delay_secs \
         FROM ha_outbox_gc_channel_state WHERE channel = 'control'",
    )
    .fetch_one(&proxy.key_store.pool)
    .await
    .expect("read persisted GC continuation diagnostics");
    assert_eq!(next_retry_at, None);
    assert_eq!(last_defer_reason, None);
    assert_eq!(last_continuation_delay_secs, None);
    assert!(queued_available_at > queued_at);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn delayed_gc_continuations_survive_restart_and_manual_trigger_unlocks_request_logs_gc() {
    let db_path = temp_db_path("scheduled-job-delayed-continuation");
    let db_str = db_path.to_string_lossy().to_string();
    let available_at = Utc::now().timestamp() + 5 * 60;
    let continuation_id = {
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let continuation = proxy
            .scheduled_job_enqueue_at("request_logs_gc", "auto", None, 1, available_at)
            .await
            .expect("enqueue delayed continuation");
        assert!(
            proxy
                .fetch_queued_scheduled_jobs(1)
                .await
                .expect("fetch delayed continuation")
                .is_empty(),
            "delayed continuation must not be eligible before its available_at"
        );
        continuation.job_id
    };
    let ha_continuation_id = {
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        proxy
            .scheduled_job_enqueue_at("ha_outbox_gc", "auto", None, 1, available_at)
            .await
            .expect("enqueue delayed HA continuation")
            .job_id
    };

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy recreated");
    let unrelated = proxy
        .scheduled_job_enqueue("auth_token_logs_gc", "scheduler", None, 1)
        .await
        .expect("enqueue unrelated queued job");
    assert_eq!(
        proxy
            .abandon_active_scheduled_jobs()
            .await
            .expect("run startup stale-job cleanup"),
        1,
        "startup cleanup must abandon unrelated queued work"
    );
    assert_eq!(
        proxy
            .scheduled_job_by_id(continuation_id)
            .await
            .expect("read delayed continuation after startup cleanup")
            .expect("delayed continuation remains")
            .status,
        "queued",
        "startup cleanup must preserve the durable automatic continuation"
    );
    assert_eq!(
        proxy
            .scheduled_job_by_id(ha_continuation_id)
            .await
            .expect("read delayed HA continuation after startup cleanup")
            .expect("delayed HA continuation remains")
            .status,
        "queued",
        "startup cleanup must preserve the durable HA continuation"
    );
    assert_eq!(
        proxy
            .scheduled_job_by_id(unrelated.job_id)
            .await
            .expect("read abandoned unrelated job")
            .expect("unrelated job remains in history")
            .status,
        "abandoned"
    );
    assert!(
        proxy
            .fetch_queued_scheduled_jobs(1)
            .await
            .expect("fetch delayed continuation after restart")
            .is_empty(),
        "available_at must survive process recreation and startup cleanup"
    );
    let manual = proxy
        .scheduled_job_enqueue("request_logs_gc", "manual", None, 1)
        .await
        .expect("manually promote delayed continuation");
    assert_eq!(manual.job_id, continuation_id);
    assert!(manual.promoted);
    let queued = proxy
        .fetch_queued_scheduled_jobs(1)
        .await
        .expect("fetch manually unlocked continuation");
    assert_eq!(queued[0].id, continuation_id);
    assert_eq!(queued[0].trigger_source, "manual");
    assert!(queued[0].available_at <= Utc::now().timestamp());

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn running_ha_gc_is_requeued_after_restart() {
    let db_path = temp_db_path("running-ha-gc-restart-recovery");
    let db_str = db_path.to_string_lossy().to_string();
    let job_id = {
        let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
            .await
            .expect("proxy created");
        let job = proxy
            .scheduled_job_enqueue("ha_outbox_gc", "auto", None, 1)
            .await
            .expect("enqueue HA continuation");
        proxy
            .scheduled_job_mark_running(job.job_id)
            .await
            .expect("mark HA continuation running")
            .expect("claim HA continuation");
        job.job_id
    };

    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("recreate proxy");
    assert_eq!(
        proxy
            .abandon_active_scheduled_jobs()
            .await
            .expect("recover running HA continuation"),
        1
    );
    let recovered = proxy
        .scheduled_job_by_id(job_id)
        .await
        .expect("read recovered HA continuation")
        .expect("recovered HA continuation exists");
    assert_eq!(recovered.status, "queued");
    assert_eq!(recovered.trigger_source, "auto");
    assert!(recovered.started_at.is_none());
    assert!(recovered.finished_at.is_none());
    let available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read recovered availability");
    assert!(
        available_at >= Utc::now().timestamp() + 29,
        "recovered HA continuation should retain a short retry delay"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn stale_claim_generation_cannot_finish_reclaimed_job() {
    let db_path = temp_db_path("scheduled-job-stale-generation");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let queued = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "auto", None, 1)
        .await
        .expect("enqueue HA GC");
    let first = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim first generation")
        .expect("first claim exists");
    sqlx::query(
        "UPDATE scheduled_jobs SET status = 'queued', started_at = NULL, available_at = 0, claim_generation = claim_generation + 1 WHERE id = ?",
    )
    .bind(queued.job_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate stale recovery");
    let second = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim second generation")
        .expect("second claim exists");
    assert!(second.claim_generation > first.claim_generation);
    let stale = proxy
        .scheduled_job_finish_claimed(
            queued.job_id,
            first.claim_generation,
            "success",
            Some("stale"),
        )
        .await;
    assert!(stale.is_err());
    assert_eq!(
        proxy
            .scheduled_job_by_id(queued.job_id)
            .await
            .expect("read job")
            .expect("job exists")
            .status,
        "running"
    );
}

#[tokio::test]
async fn stale_claim_cannot_be_misreported_as_a_non_running_job_when_requeueing() {
    let db_path = temp_db_path("scheduled-job-stale-requeue");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let queued = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "auto", None, 1)
        .await
        .expect("enqueue HA GC");
    let first = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim first generation")
        .expect("first claim exists");
    sqlx::query(
        "UPDATE scheduled_jobs SET status = 'queued', started_at = NULL, available_at = 0, claim_generation = claim_generation + 1 WHERE id = ?",
    )
    .bind(queued.job_id)
    .execute(&proxy.key_store.pool)
    .await
    .expect("simulate stale recovery");
    let _second = proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim second generation")
        .expect("second claim exists");
    let err = proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            queued.job_id,
            first.claim_generation,
            "ha_outbox_gc",
            None,
            1,
            Some("deferred=has_more"),
            Utc::now().timestamp() + 30,
        )
        .await
        .expect_err("stale continuation must be rejected");
    assert!(
        !err.to_string().contains("was not running"),
        "stale claims need a distinct internal outcome so the scheduler does not retry them"
    );

    drop(proxy);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

#[tokio::test]
async fn stale_reaper_recovers_ha_gc_once_with_delay() {
    let db_path = temp_db_path("scheduled-job-stale-reaper");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let queued = proxy
        .scheduled_job_enqueue("ha_outbox_gc", "auto", None, 1)
        .await
        .expect("enqueue HA GC");
    proxy
        .scheduled_job_mark_running(queued.job_id)
        .await
        .expect("claim HA GC")
        .expect("claim exists");
    sqlx::query("UPDATE scheduled_jobs SET started_at = ? WHERE id = ?")
        .bind(Utc::now().timestamp() - 121)
        .bind(queued.job_id)
        .execute(&proxy.key_store.pool)
        .await
        .expect("age running job");
    assert_eq!(
        proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("recover stale"),
        1
    );
    assert_eq!(
        proxy
            .recover_stale_scheduled_jobs()
            .await
            .expect("repeat reaper"),
        0
    );
    let recovered = proxy
        .scheduled_job_by_id(queued.job_id)
        .await
        .expect("read recovered job")
        .expect("recovered job exists");
    assert_eq!(recovered.status, "queued");
    let available_at: i64 =
        sqlx::query_scalar("SELECT available_at FROM scheduled_jobs WHERE id = ?")
            .bind(queued.job_id)
            .fetch_one(&proxy.key_store.pool)
            .await
            .expect("read retry time");
    assert!(available_at >= Utc::now().timestamp() + 29);
}

#[tokio::test]
async fn request_log_body_gc_candidate_query_uses_partial_body_index() {
    let db_path = temp_db_path("request-log-body-gc-partial-index");
    let db_str = db_path.to_string_lossy().to_string();
    let proxy = TavilyProxy::with_endpoint(Vec::<String>::new(), DEFAULT_UPSTREAM, &db_str)
        .await
        .expect("proxy created");
    let index_before_maintenance: Option<String> = sqlx::query_scalar(
        "SELECT name FROM observability.sqlite_master WHERE type = 'index' AND name = ?",
    )
    .bind("idx_request_logs_body_gc_cursor")
    .fetch_optional(&proxy.key_store.pool)
    .await
    .expect("query body GC index before maintenance task");
    assert!(
        index_before_maintenance.is_none(),
        "body-GC partial index must not be built on the startup schema path"
    );
    sqlx::query(
        r#"
        WITH RECURSIVE candidates(id) AS (
            VALUES(1)
            UNION ALL
            SELECT id + 1 FROM candidates WHERE id < 512
        )
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        )
        SELECT 'POST', '/api/tavily/search', 'success', 'api:search', NULL, NULL, ?, id
        FROM candidates
        "#,
    )
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed body-less request logs");
    sqlx::query(
        r#"
        INSERT INTO observability.request_logs (
            method, path, result_status, request_kind_key,
            request_body, response_body, visibility, created_at
        ) VALUES ('POST', '/api/tavily/search', 'success', 'api:search', ?, NULL, ?, 513)
        "#,
    )
    .bind(br#"{"query":"index"}"#.as_slice())
    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
    .execute(&proxy.key_store.pool)
    .await
    .expect("seed body-bearing request log");
    proxy
        .ensure_request_log_body_gc_cursor_index()
        .await
        .expect("create partial body-GC index");
    proxy
        .ensure_request_log_body_gc_cursor_index()
        .await
        .expect("repeat partial body-GC index creation");

    let plan = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, created_at
        FROM observability.request_logs
        WHERE (request_body IS NOT NULL OR response_body IS NOT NULL)
          AND created_at >= 0
        ORDER BY created_at ASC, id ASC
        LIMIT 100
        "#,
    )
    .fetch_all(&proxy.key_store.pool)
    .await
    .expect("explain request log body GC candidate query");
    let details = plan
        .iter()
        .map(|row| {
            row.try_get::<String, _>("detail")
                .expect("query-plan detail")
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        details.contains("idx_request_logs_body_gc_cursor"),
        "expected partial body-GC index, got query plan:\n{details}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
}

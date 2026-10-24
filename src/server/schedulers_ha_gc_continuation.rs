async fn finish_ha_gc_with_continuation(
    state: &Arc<AppState>,
    job_id: i64,
    claim_generation: i64,
    message: String,
    continuation_delay_secs: i64,
) -> bool {
    let available_at = state
        .proxy
        .backend_time()
        .now_ts()
        .saturating_add(continuation_delay_secs);
    #[cfg(test)]
    wait_for_ha_gc_continuation_pause_for_test(state, job_id).await;
    let result = state
        .proxy
        .scheduled_job_finish_and_enqueue_auto_at(
            job_id,
            claim_generation,
            "ha_outbox_gc",
            None,
            1,
            Some(&message),
            available_at,
        )
        .await;
    match result {
        Ok(result) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "continuation_queued",
                job_id,
                continuation_job_id = result.job_id,
                continuation_created = result.created,
                continuation_delay_secs,
                available_at,
            );
        }
        Err(err) if err.is_stale_claim() => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "stale_claim_ignored",
                job_id,
                claim_generation,
                "stale GC claim cannot finish or enqueue a continuation"
            );
            return true;
        }
        Err(err) if tavily_hikari::is_transient_sqlite_write_error(&err) => {
            tracing::warn!(
                component = "ha_outbox_gc",
                event = "continuation_persist_deferred",
                job_id,
                claim_generation,
                continuation_delay_secs,
                retry_count = HA_OUTBOX_GC_CONTINUATION_PERSIST_RETRY_DELAYS_MS.len(),
                stale_reaper_after_secs = 120_u64,
                err = %err,
                "HA outbox GC continuation hit a transient SQLite conflict; scheduling fixed same-generation retries"
            );
            spawn_ha_gc_continuation_persistence_retries(
                state.clone(),
                job_id,
                claim_generation,
                message,
                continuation_delay_secs,
            );
            return true;
        }
        Err(err) => {
            tracing::error!(
                component = "ha_outbox_gc",
                event = "continuation_transaction_failed",
                job_id,
                continuation_delay_secs,
                err = %err,
                "HA outbox GC could not persist its deferred continuation"
            );
            if let Err(finish_err) = state
                .proxy
                .scheduled_job_finish_claimed(job_id, claim_generation, "error", Some(&message))
                .await
            {
                tracing::error!(
                    component = "ha_outbox_gc",
                    event = "deferred_job_finish_failed",
                    job_id,
                    err = %finish_err,
                    "HA outbox GC deferred job remains running for stale-reaper recovery"
                );
            }
            return false;
        }
    }
    true
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct HaGcContinuationPause {
    arrived: Arc<Notify>,
    release: Arc<Notify>,
}

#[cfg(test)]
impl HaGcContinuationPause {
    pub(crate) async fn wait_until_arrived(&self) {
        self.arrived.notified().await;
    }

    pub(crate) fn release(&self) {
        self.release.notify_one();
    }
}

#[cfg(test)]
static HA_GC_CONTINUATION_PAUSE: OnceLock<
    StdMutex<Option<(usize, i64, HaGcContinuationPause)>>,
> = OnceLock::new();

#[cfg(test)]
pub(crate) fn install_ha_gc_continuation_pause_for_test(
    state: &Arc<AppState>,
    job_id: i64,
) -> HaGcContinuationPause {
    let pause = HaGcContinuationPause {
        arrived: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
    };
    *HA_GC_CONTINUATION_PAUSE
        .get_or_init(|| StdMutex::new(None))
        .lock()
        .expect("HA GC continuation pause mutex") =
        Some((Arc::as_ptr(state) as usize, job_id, pause.clone()));
    pause
}

#[cfg(test)]
async fn wait_for_ha_gc_continuation_pause_for_test(state: &Arc<AppState>, job_id: i64) {
    let state_key = Arc::as_ptr(state) as usize;
    let pause = HA_GC_CONTINUATION_PAUSE.get().and_then(|slot| {
        let mut slot = slot.lock().expect("HA GC continuation pause mutex");
        if slot
            .as_ref()
            .is_some_and(|(pause_state_key, pause_job_id, _)| {
                *pause_state_key == state_key && *pause_job_id == job_id
            })
        {
            slot.take().map(|(_, _, pause)| pause)
        } else {
            None
        }
    });
    if let Some(pause) = pause {
        pause.arrived.notify_one();
        pause.release.notified().await;
    }
}

fn spawn_ha_gc_continuation_persistence_retries(
    state: Arc<AppState>,
    job_id: i64,
    claim_generation: i64,
    message: String,
    continuation_delay_secs: i64,
) {
    tokio::spawn(async move {
        for (retry_index, retry_delay_ms) in
            HA_OUTBOX_GC_CONTINUATION_PERSIST_RETRY_DELAYS_MS.iter().copied().enumerate()
        {
            tokio::time::sleep(Duration::from_millis(retry_delay_ms)).await;
            let available_at = state
                .proxy
                .backend_time()
                .now_ts()
                .saturating_add(continuation_delay_secs);
            match state
                .proxy
                .scheduled_job_finish_and_enqueue_auto_at(
                    job_id,
                    claim_generation,
                    "ha_outbox_gc",
                    None,
                    1,
                    Some(&message),
                    available_at,
                )
                .await
            {
                Ok(result) => {
                    tracing::debug!(
                        component = "ha_outbox_gc",
                        event = "continuation_persist_recovered",
                        job_id,
                        claim_generation,
                        continuation_job_id = result.job_id,
                        continuation_created = result.created,
                        continuation_delay_secs,
                        available_at,
                        retry_attempt = retry_index + 1,
                        retry_delay_ms,
                    );
                    maintenance_worker_wake_for_state(state.as_ref()).notify_one();
                    return;
                }
                Err(err) if err.is_stale_claim() => {
                    tracing::debug!(
                        component = "ha_outbox_gc",
                        event = "stale_claim_ignored",
                        job_id,
                        claim_generation,
                        retry_attempt = retry_index + 1,
                        "HA GC continuation retry observed a stale claim"
                    );
                    return;
                }
                Err(err) if tavily_hikari::is_transient_sqlite_write_error(&err) => {
                    tracing::debug!(
                        component = "ha_outbox_gc",
                        event = "continuation_persist_retry_deferred",
                        job_id,
                        claim_generation,
                        retry_attempt = retry_index + 1,
                        retry_delay_ms,
                        err = %err,
                    );
                }
                Err(err) => {
                    tracing::error!(
                        component = "ha_outbox_gc",
                        event = "continuation_transaction_failed",
                        job_id,
                        claim_generation,
                        retry_attempt = retry_index + 1,
                        retry_delay_ms,
                        err = %err,
                        "HA GC continuation retry stopped on a permanent error"
                    );
                    return;
                }
            }
        }
        tracing::warn!(
            component = "ha_outbox_gc",
            event = "continuation_persist_retry_exhausted",
            job_id,
            claim_generation,
            retry_count = HA_OUTBOX_GC_CONTINUATION_PERSIST_RETRY_DELAYS_MS.len(),
            "HA GC continuation retries exhausted; stale reaper remains the final recovery path"
        );
    });
}

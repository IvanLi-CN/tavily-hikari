async fn run_ha_outbox_gc_claimed_job(
    state: Arc<AppState>,
    claimed_job: ClaimedScheduledJob,
) -> ScheduledJobCompletion {
    let job_id = claimed_job.job_id;
    let claim_generation = claimed_job.claim_generation;
    let due_channels = match state.proxy.ha_outbox_gc_work_due_channels().await {
        Ok(HaOutboxGcWorkDueChannelsResult::Ready(channels)) => channels,
        Ok(HaOutboxGcWorkDueChannelsResult::Busy) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "compatibility_due_read_deferred",
                job_id,
                claim_generation,
                defer_reason = "sqlite_busy",
            );
            return ScheduledJobCompletion::Deferred;
        }
        Err(err) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "compatibility_due_read_failed",
                job_id,
                claim_generation,
                err = %err,
            );
            return ScheduledJobCompletion::Deferred;
        }
    };
    if due_channels.is_empty() {
        return match state
            .proxy
            .scheduled_job_finish_claimed(job_id, claim_generation, "success", Some("not_eligible"))
            .await
        {
            Ok(()) => ScheduledJobCompletion::Completed,
            Err(err) if err.is_stale_claim() => ScheduledJobCompletion::Deferred,
            Err(err) => {
                tracing::debug!(
                    component = "ha_outbox_gc",
                    event = "compatibility_finish_failed",
                    job_id,
                    claim_generation,
                    err = %err,
                );
                ScheduledJobCompletion::Deferred
            }
        };
    }

    let mut enqueued = 0;
    for channel in due_channels {
        if enqueue_scheduled_job_logged(
            state.as_ref(),
            ha_outbox_gc_job_type(channel),
            None,
            TRIGGER_SOURCE_AUTO,
            "ha-outbox-gc-compatibility",
        )
        .await
        .is_some()
        {
            enqueued += 1;
        }
    }
    let message = format!("compatibility_handoff=enqueued_{enqueued}");
    match state
        .proxy
        .scheduled_job_finish_claimed(job_id, claim_generation, "success", Some(&message))
        .await
    {
        Ok(()) => ScheduledJobCompletion::Completed,
        Err(err) if err.is_stale_claim() => ScheduledJobCompletion::Deferred,
        Err(err) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "compatibility_handoff_finish_failed",
                job_id,
                claim_generation,
                err = %err,
            );
            ScheduledJobCompletion::Deferred
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn finish_ha_outbox_gc_channel(
    state: &Arc<AppState>,
    claimed_job: &ClaimedScheduledJob,
    claim: HaOutboxGcWorkClaim,
    outcome: HaOutboxGcWorkOutcome,
    eligible_at: i64,
    status: &'static str,
    message: String,
    continuation_available_at: Option<i64>,
    deleted_rows: i64,
) -> ScheduledJobCompletion {
    let continuation_job_type = continuation_available_at.map(|_| ha_outbox_gc_job_type(claim.channel));
    let result = state
        .proxy
        .finish_ha_outbox_gc_work_and_enqueue(
            claimed_job.job_id,
            claimed_job.claim_generation,
            claim,
            outcome,
            eligible_at,
            status,
            Some(&message),
            continuation_job_type,
            continuation_available_at,
            deleted_rows,
        )
        .await;
    let result = match result {
        Ok((HaOutboxGcWorkFinishResult::Busy, _)) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "finish_retry",
                job_id = claimed_job.job_id,
                channel = claim.channel.as_str(),
                defer_reason = "sqlite_busy",
                "HA GC finish retry remains bounded by the second SQLite operation budget"
            );
            state
                .proxy
                .finish_ha_outbox_gc_work_and_enqueue(
                    claimed_job.job_id,
                    claimed_job.claim_generation,
                    claim,
                    outcome,
                    eligible_at,
                    status,
                    Some(&message),
                    continuation_job_type,
                    continuation_available_at,
                    deleted_rows,
                )
                .await
        }
        other => other,
    };
    match result {
        Ok((HaOutboxGcWorkFinishResult::Finished(finished_outcome), continuation)) => {
            if let Some(continuation) = continuation {
                tracing::debug!(
                    component = "ha_outbox_gc",
                    event = "continuation_queued",
                    job_id = claimed_job.job_id,
                    channel = claim.channel.as_str(),
                    continuation_job_id = continuation.job_id,
                    continuation_created = continuation.created,
                    continuation_available_at,
                );
            }
            match finished_outcome {
                HaOutboxGcWorkOutcome::Completed => ScheduledJobCompletion::Completed,
                HaOutboxGcWorkOutcome::Failed => ScheduledJobCompletion::Failed,
                HaOutboxGcWorkOutcome::Deferred
                | HaOutboxGcWorkOutcome::Busy
                | HaOutboxGcWorkOutcome::Pending
                | HaOutboxGcWorkOutcome::Running
                | HaOutboxGcWorkOutcome::Stale => ScheduledJobCompletion::Deferred,
            }
        }
        Ok((HaOutboxGcWorkFinishResult::Stale, _)) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "stale_claim_ignored",
                job_id = claimed_job.job_id,
                claim_generation = claimed_job.claim_generation,
                channel = claim.channel.as_str(),
                "stale HA GC generation cannot finish or enqueue a continuation"
            );
            ScheduledJobCompletion::Deferred
        }
        Ok((HaOutboxGcWorkFinishResult::Busy, _)) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "finish_deferred",
                job_id = claimed_job.job_id,
                channel = claim.channel.as_str(),
                defer_reason = "sqlite_busy",
                "HA GC finish was busy; stale recovery will revisit the durable claim"
            );
            ScheduledJobCompletion::Deferred
        }
        Err(err) => {
            tracing::warn!(
                component = "ha_outbox_gc",
                event = "finish_failed",
                job_id = claimed_job.job_id,
                channel = claim.channel.as_str(),
                err = %err,
                "HA GC work and scheduled job finish could not be committed"
            );
            let now = state.proxy.backend_time().now_ts();
            let eligible_at = now.saturating_add(HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS);
            let handoff_message = format!("deferred=finish_error error={err}");
            match state
                .proxy
                .defer_ha_outbox_gc_claim_and_enqueue(
                    claimed_job.job_id,
                    claimed_job.claim_generation,
                    claim,
                    eligible_at,
                    &handoff_message,
                    eligible_at,
                )
                .await
            {
                Ok((HaOutboxGcWorkFinishResult::Finished(_), continuation)) => {
                    tracing::debug!(
                        component = "ha_outbox_gc",
                        event = "finish_failure_handoff_queued",
                        job_id = claimed_job.job_id,
                        channel = claim.channel.as_str(),
                        continuation_job_id = continuation.map(|value| value.job_id),
                        available_at = eligible_at,
                    );
                }
                Ok((HaOutboxGcWorkFinishResult::Stale, _)) => {}
                Ok((HaOutboxGcWorkFinishResult::Busy, _)) => {
                    tracing::debug!(
                        component = "ha_outbox_gc",
                        event = "finish_failure_handoff_deferred",
                        job_id = claimed_job.job_id,
                        channel = claim.channel.as_str(),
                        defer_reason = "sqlite_busy",
                    );
                }
                Err(handoff_err) => tracing::warn!(
                    component = "ha_outbox_gc",
                    event = "finish_failure_handoff_failed",
                    job_id = claimed_job.job_id,
                    channel = claim.channel.as_str(),
                    err = %handoff_err,
                ),
            }
            ScheduledJobCompletion::Deferred
        }
    }
}

async fn defer_ha_outbox_gc_channel_job(
    state: &Arc<AppState>,
    claimed_job: &ClaimedScheduledJob,
    channel: HaSyncChannel,
    outcome: HaOutboxGcWorkOutcome,
    eligible_at: i64,
    message: String,
) -> ScheduledJobCompletion {
    let mut result = state
        .proxy
        .defer_ha_outbox_gc_job_and_enqueue(
            claimed_job.job_id,
            claimed_job.claim_generation,
            channel,
            outcome,
            eligible_at,
            &message,
            eligible_at,
        )
        .await;
    if matches!(
        result,
        Ok((HaOutboxGcWorkFinishResult::Busy, _))
    ) {
        result = state
            .proxy
            .defer_ha_outbox_gc_job_and_enqueue(
                claimed_job.job_id,
                claimed_job.claim_generation,
                channel,
                outcome,
                eligible_at,
                &message,
                eligible_at,
            )
            .await;
    }
    match result {
        Ok((HaOutboxGcWorkFinishResult::Finished(_), continuation)) => {
            if let Some(continuation) = continuation {
                tracing::debug!(
                    component = "ha_outbox_gc",
                    event = "handoff_continuation_queued",
                    job_id = claimed_job.job_id,
                    channel = channel.as_str(),
                    continuation_job_id = continuation.job_id,
                    continuation_created = continuation.created,
                    available_at = eligible_at,
                );
            }
            ScheduledJobCompletion::Deferred
        }
        Ok((HaOutboxGcWorkFinishResult::Stale, _)) => ScheduledJobCompletion::Deferred,
        Ok((HaOutboxGcWorkFinishResult::Busy, _)) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "handoff_deferred",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                "HA GC handoff remained busy; lease recovery will revisit it"
            );
            ScheduledJobCompletion::Deferred
        }
        Err(err) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "handoff_failed",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                err = %err,
            );
            ScheduledJobCompletion::Deferred
        }
    }
}

async fn run_ha_outbox_gc_channel_claimed_job(
    state: Arc<AppState>,
    claimed_job: ClaimedScheduledJob,
    channel: HaSyncChannel,
) -> ScheduledJobCompletion {
    let claim = match state.proxy.claim_ha_outbox_gc_work(channel).await {
        Ok(HaOutboxGcWorkClaimResult::Claimed(claim)) => claim,
        Ok(HaOutboxGcWorkClaimResult::NotEligible { eligible_at }) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "not_eligible",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                eligible_at,
            );
            return match state
                .proxy
                .scheduled_job_finish_claimed(
                    claimed_job.job_id,
                    claimed_job.claim_generation,
                    "success",
                    Some("not_eligible"),
                )
                .await
            {
                Ok(()) => ScheduledJobCompletion::Completed,
                Err(err) if err.is_stale_claim() => ScheduledJobCompletion::Deferred,
                Err(err) => {
                    let now = state.proxy.backend_time().now_ts();
                    defer_ha_outbox_gc_channel_job(
                        &state,
                        &claimed_job,
                        channel,
                        HaOutboxGcWorkOutcome::Busy,
                        now.saturating_add(HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS),
                        format!("deferred=not_eligible_finish error={err}"),
                    )
                    .await
                }
            };
        }
        Ok(HaOutboxGcWorkClaimResult::AlreadyClaimed {
            claim_generation,
            claim_expires_at,
        }) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "claim_already_owned",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                claim_generation,
                claim_expires_at,
            );
            return ScheduledJobCompletion::Deferred;
        }
        Ok(HaOutboxGcWorkClaimResult::Busy) => {
            tracing::debug!(
                component = "ha_outbox_gc",
                event = "claim_deferred",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                defer_reason = "sqlite_busy",
                "HA GC claim hit the bounded SQLite write budget"
            );
            let now = state.proxy.backend_time().now_ts();
            let eligible_at = now.saturating_add(HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS);
            return defer_ha_outbox_gc_channel_job(
                &state,
                &claimed_job,
                channel,
                HaOutboxGcWorkOutcome::Busy,
                eligible_at,
                "deferred=sqlite_busy".to_string(),
            )
            .await;
        }
        Err(err) if tavily_hikari::is_transient_sqlite_write_error(&err) => {
            tracing::warn!(
                component = "ha_outbox_gc",
                event = "claim_failed",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                err = %err,
            );
            let now = state.proxy.backend_time().now_ts();
            let eligible_at = now.saturating_add(HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS);
            return defer_ha_outbox_gc_channel_job(
                &state,
                &claimed_job,
                channel,
                HaOutboxGcWorkOutcome::Busy,
                eligible_at,
                format!("deferred=sqlite_busy error={err}"),
            )
            .await;
        }
        Err(err) => {
            tracing::warn!(
                component = "ha_outbox_gc",
                event = "claim_failed",
                job_id = claimed_job.job_id,
                channel = channel.as_str(),
                err = %err,
            );
            let now = state.proxy.backend_time().now_ts();
            let eligible_at = now.saturating_add(HA_OUTBOX_GC_BASELINE_SECS);
            return defer_ha_outbox_gc_channel_job(
                &state,
                &claimed_job,
                channel,
                HaOutboxGcWorkOutcome::Failed,
                eligible_at,
                format!("failed=claim error={err}"),
            )
            .await;
        }
    };

    let result = state
        .proxy
        .gc_ha_outbox_online_for_channel_with_foreground_pressure(
            channel,
            foreground_activity_rps(),
            foreground_activity_low_pressure_since_floor(),
        )
        .await;
    match result {
        Ok(report) => {
            let needs_continuation = report.has_more || !report.completed;
            let foreground_rps_after_slice = needs_continuation.then(foreground_activity_rps);
            let continuation_delay_secs = foreground_rps_after_slice.map_or(
                report.continuation_delay_secs,
                |foreground_rps| {
                    Some(ha_gc_continuation_delay_secs(
                        report.continuation_delay_secs,
                        foreground_rps,
                    ))
                },
            );
            let now = state.proxy.backend_time().now_ts();
            if needs_continuation {
                let continuation_delay_secs = continuation_delay_secs
                    .unwrap_or(HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS);
                let foreground_rps_after_slice = foreground_rps_after_slice.unwrap_or_default();
                let defer_reason = if foreground_rps_after_slice > HA_OUTBOX_GC_LOW_PRESSURE_RPS {
                    "foreground_pressure"
                } else if continuation_delay_secs
                    == HA_OUTBOX_GC_LEGACY_SCAN_CONTINUATION_DELAY_SECS
                {
                    "legacy_scan"
                } else if continuation_delay_secs < HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS {
                    "fast_progress"
                } else {
                    "slice_budget_exhausted"
                };
                let continuation_at = now.saturating_add(continuation_delay_secs);
                let message = format!(
                    "deferred={defer_reason} {}",
                    format_ha_outbox_gc_report_message(&report, 1)
                );
                finish_ha_outbox_gc_channel(
                    &state,
                    &claimed_job,
                    claim,
                    HaOutboxGcWorkOutcome::Deferred,
                    continuation_at,
                    "success",
                    message,
                    Some(continuation_at),
                    report.deleted_rows,
                )
                .await
            } else {
                let eligible_at = now.saturating_add(HA_OUTBOX_GC_BASELINE_SECS);
                finish_ha_outbox_gc_channel(
                    &state,
                    &claimed_job,
                    claim,
                    HaOutboxGcWorkOutcome::Completed,
                    eligible_at,
                    "success",
                    format_ha_outbox_gc_report_message(&report, 1),
                    None,
                    report.deleted_rows,
                )
                .await
            }
        }
        Err(err) if tavily_hikari::is_transient_sqlite_write_error(&err) => {
            let now = state.proxy.backend_time().now_ts();
            let eligible_at = now.saturating_add(HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS);
            finish_ha_outbox_gc_channel(
                &state,
                &claimed_job,
                claim,
                HaOutboxGcWorkOutcome::Busy,
                eligible_at,
                "success",
                format!("deferred=sqlite_busy error={err}"),
                Some(eligible_at),
                0,
            )
            .await
        }
        Err(err) => {
            let now = state.proxy.backend_time().now_ts();
            finish_ha_outbox_gc_channel(
                &state,
                &claimed_job,
                claim,
                HaOutboxGcWorkOutcome::Failed,
                now.saturating_add(HA_OUTBOX_GC_BASELINE_SECS),
                "error",
                err.to_string(),
                None,
                0,
            )
            .await
        }
    }
}

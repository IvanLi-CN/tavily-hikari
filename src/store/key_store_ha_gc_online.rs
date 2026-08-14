type HaOutboxGcChannelStateRow = (
    i64,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
    String,
    Option<i64>,
    f64,
    String,
);
const HA_GC_CHANNEL_CLAIM_STALE_SECS: i64 = 120;
type HaGcChannelEligibility = (String, Option<i64>, i64, Option<i64>, Option<i64>);

#[derive(Debug, Clone, Copy)]
struct HaGcChannelClaim {
    channel: HaSyncChannel,
    previous_generation: i64,
    generation: i64,
}

struct HaGcController;

impl HaGcController {
    const CHANNELS: [HaSyncChannel; 3] = [
        HaSyncChannel::Control,
        HaSyncChannel::Billing,
        HaSyncChannel::Runtime,
    ];

    fn claim_eligible_channel(
        preferred: HaSyncChannel,
        pending_channel_mask: i64,
        now: i64,
        eligibility: &[HaGcChannelEligibility],
    ) -> Option<HaGcChannelClaim> {
        let preferred_index = Self::CHANNELS
            .iter()
            .position(|candidate| *candidate == preferred)
            .unwrap_or(0);
        (0..Self::CHANNELS.len()).find_map(|offset| {
            let candidate = Self::CHANNELS[(preferred_index + offset) % Self::CHANNELS.len()];
            let mask = KeyStore::ha_outbox_gc_channel_mask(candidate);
            if pending_channel_mask & mask == 0 {
                return None;
            }
            eligibility
                .iter()
                .find_map(|(name, next_retry_at, generation, claim_started_at, _)| {
                (name == candidate.as_str()
                    && next_retry_at.is_none_or(|available_at| available_at <= now)
                    && claim_started_at.is_none_or(|started_at| {
                        started_at <= now.saturating_sub(HA_GC_CHANNEL_CLAIM_STALE_SECS)
                    }))
                    .then_some(HaGcChannelClaim {
                        channel: candidate,
                        previous_generation: *generation,
                        generation: generation.saturating_add(1),
                    })
                })
        })
    }

    fn next_wake_delay_secs(
        preferred: HaSyncChannel,
        pending_channel_mask: i64,
        now: i64,
        eligibility: &[HaGcChannelEligibility],
        ready_delay_secs: i64,
    ) -> Option<i64> {
        let preferred_index = Self::CHANNELS
            .iter()
            .position(|candidate| *candidate == preferred)
            .unwrap_or(0);
        let mut earliest_retry_at: Option<i64> = None;
        for offset in 0..Self::CHANNELS.len() {
            let candidate = Self::CHANNELS[(preferred_index + offset) % Self::CHANNELS.len()];
            if pending_channel_mask & KeyStore::ha_outbox_gc_channel_mask(candidate) == 0 {
                continue;
            }
            let Some((_, next_retry_at, _, claim_started_at, _)) = eligibility
                .iter()
                .find(|(name, _, _, _, _)| name == candidate.as_str())
            else {
                continue;
            };
            let eligible_at = next_retry_at.unwrap_or(now).max(
                claim_started_at.map_or(now, |started_at| {
                    started_at.saturating_add(HA_GC_CHANNEL_CLAIM_STALE_SECS)
                }),
            );
            if eligible_at <= now {
                return Some(ready_delay_secs.max(1));
            }
            earliest_retry_at = Some(
                earliest_retry_at
                    .map_or(eligible_at, |earliest| earliest.min(eligible_at)),
            );
        }
        earliest_retry_at.map(|retry_at| retry_at.saturating_sub(now).max(1))
    }

    fn stale_observation_mask(now: i64, eligibility: &[HaGcChannelEligibility]) -> i64 {
        let observed_before = now.saturating_sub(crate::HA_OUTBOX_GC_IDLE_DISCOVERY_SECS);
        eligibility.iter().fold(0, |mask, (name, _, _, _, observed_at)| {
            let channel = KeyStore::ha_outbox_gc_channel_from_name(name);
            if observed_at.is_none_or(|observed_at| observed_at <= observed_before)
            {
                mask | KeyStore::ha_outbox_gc_channel_mask(channel)
            } else {
                mask
            }
        })
    }
}

impl KeyStore {
    pub(crate) fn try_admit_ha_outbox_gc(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_maintenance_bulk(SqliteOperation::HaOutboxGc)
    }

    pub(crate) async fn defer_claimed_ha_gc_channel_for_busy(
        &self,
        channel: HaSyncChannel,
        claim_generation: i64,
        pending_channel_mask: i64,
        options: HaOutboxGcOptions,
        foreground_rps: i64,
        started: Instant,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::HaOutboxGc)
            .await?;
        let result = async {
            let now = self.backend_time.now_ts();
            let channel_delay_secs = crate::HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS;
            let channel_next_retry_at = now.saturating_add(channel_delay_secs);
            let completed_claim = sqlx::query(
                r#"UPDATE ha_outbox_gc_channel_state
                   SET last_attempt_at = ?,
                       last_deleted_rows = 0,
                       last_defer_reason = 'sqlite_busy',
                       next_retry_at = ?,
                       consecutive_no_progress = consecutive_no_progress + 1,
                       last_continuation_delay_secs = ?,
                       debt_mode = 'sqlite_busy',
                       foreground_rps = ?,
                       claim_started_at = NULL
                   WHERE channel = ? AND claim_generation = ?"#,
            )
            .bind(now)
            .bind(channel_next_retry_at)
            .bind(channel_delay_secs)
            .bind(foreground_rps)
            .bind(channel.as_str())
            .bind(claim_generation)
            .execute(&mut *conn)
            .await?;
            if completed_claim.rows_affected() == 0 {
                return Err(ProxyError::Other(format!(
                    "stale HA GC busy defer for {} generation {claim_generation}",
                    channel.as_str(),
                )));
            }
            let next_channel = Self::next_ha_outbox_gc_channel(channel);
            let next_pending_channel_mask =
                pending_channel_mask | Self::ha_outbox_gc_channel_mask(channel);
            sqlx::query(
                "UPDATE ha_outbox_gc_state SET next_channel = ?, pending_channel_mask = ?, updated_at = ? WHERE id = 'local'",
            )
            .bind(next_channel.as_str())
            .bind(next_pending_channel_mask)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let eligibility = sqlx::query_as::<_, HaGcChannelEligibility>(
                "SELECT channel, next_retry_at, claim_generation, claim_started_at, last_observed_at FROM ha_outbox_gc_channel_state",
            )
            .fetch_all(&mut *conn)
            .await?;
            let continuation_delay_secs = HaGcController::next_wake_delay_secs(
                next_channel,
                next_pending_channel_mask,
                now,
                &eligibility,
                crate::HA_OUTBOX_GC_RECOVERY_CONTINUATION_DELAY_SECS,
            );
            Ok::<_, ProxyError>(HaOutboxGcReport {
                batch_size: options.batch_size,
                max_batches: options.max_batches,
                deleted_rows: 0,
                batches: 0,
                completed: false,
                has_more: true,
                channels: Vec::new(),
                wal_checkpoint_busy: false,
                wal_checkpoint_log_frames: 0,
                wal_checkpoint_checkpointed_frames: 0,
                active_elapsed_ms: 0,
                max_batch_elapsed_ms: 0,
                elapsed_ms: started.elapsed().as_millis(),
                continuation_delay_secs,
            })
        }
        .await;
        let report = result?;
        conn.finish(Ok(())).await?;
        Ok(report)
    }

    pub(crate) async fn ha_outbox_gc_watchdog_needed(&self) -> Result<bool, ProxyError> {
        // A zero pending mask is only a completed observation, not a permanent
        // proof that every channel stays empty. Recheck the tiny controller
        // state table at the scheduler's five-minute cadence so a historical
        // channel missed by an older state writer is admitted for a fresh,
        // indexed slice without putting an outbox scan in the watchdog.
        let observed_before = self
            .backend_time
            .now_ts()
            .saturating_sub(crate::HA_OUTBOX_GC_IDLE_DISCOVERY_SECS);
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::HaOutboxGcWatchdog)
            .await?;
        let result: Result<bool, ProxyError> = async {
            let state: Option<(i64, i64)> = sqlx::query_as(
                r#"
                SELECT pending_channel_mask,
                       EXISTS(
                           SELECT 1
                             FROM ha_outbox_gc_channel_state
                            WHERE last_observed_at IS NULL OR last_observed_at <= ?
                       )
                  FROM ha_outbox_gc_state
                 WHERE id = 'local'
                "#,
            )
            .bind(observed_before)
            .fetch_optional(&mut *conn)
            .await?;
            Ok(state.is_none_or(|(pending_channel_mask, discovery_due)| {
                pending_channel_mask & 7 != 0 || discovery_due != 0
            }))
        }
        .await;
        conn.close().await?;
        result
    }

    pub(crate) async fn gc_ha_outbox_online(&self) -> Result<HaOutboxGcReport, ProxyError> {
        self.gc_ha_outbox_online_with_foreground_rps(0).await
    }

    pub(crate) async fn gc_ha_outbox_online_with_foreground_rps(
        &self,
        foreground_rps: i64,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        self.gc_ha_outbox_online_with_foreground_pressure(foreground_rps, 0)
            .await
    }

    pub(crate) async fn gc_ha_outbox_online_with_foreground_pressure(
        &self,
        foreground_rps: i64,
        low_pressure_since_floor: i64,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        self.gc_ha_outbox_online_with_foreground_activity(
            foreground_rps,
            low_pressure_since_floor,
            move || foreground_rps,
        )
        .await
    }

    pub(crate) async fn gc_ha_outbox_online_with_foreground_activity<F>(
        &self,
        foreground_rps: i64,
        low_pressure_since_floor: i64,
        foreground_rps_now: F,
    ) -> Result<HaOutboxGcReport, ProxyError>
    where
        F: Fn() -> i64,
    {
        self.gc_ha_outbox_online_with_options(
            HaOutboxGcOptions::online(),
            foreground_rps,
            low_pressure_since_floor,
            foreground_rps_now,
        )
        .await
    }

    async fn gc_ha_outbox_online_with_options<F>(
        &self,
        options: HaOutboxGcOptions,
        foreground_rps: i64,
        low_pressure_since_floor: i64,
        foreground_rps_now: F,
    ) -> Result<HaOutboxGcReport, ProxyError>
    where
        F: Fn() -> i64,
    {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(options.max_runtime_secs.max(1));
        let mut pooled_conn = Some(
            self.sqlite_runtime
                .acquire_operation_connection(SqliteOperation::HaOutboxGc)
                .await?,
        );
        let result = async {
            let conn = pooled_conn
                .as_mut()
                .expect("online HA GC must own a pooled connection");
            let state_now = self.backend_time.now_ts();
            let (persisted_low_pressure_since, _persisted_recovery_mode, persisted_recovery_deadline):
                (Option<i64>, i64, Option<i64>) = sqlx::query_as(
                "SELECT low_pressure_since, recovery_mode, recovery_deadline_at FROM ha_outbox_gc_state WHERE id = 'local'",
            )
            .fetch_one(&mut **conn)
            .await?;
            let low_pressure_since = if foreground_rps <= crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS {
                persisted_low_pressure_since
                    .filter(|started| *started >= low_pressure_since_floor)
                    .or(Some(state_now))
            } else {
                None
            };
            let recovery_mode = foreground_rps <= crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS
                && low_pressure_since
                    .is_some_and(|started| state_now.saturating_sub(started) >= crate::HA_OUTBOX_GC_LOW_PRESSURE_WINDOW_SECS);
            let recovery_deadline_at = if recovery_mode {
                persisted_recovery_deadline
                    .or_else(|| Some(state_now.saturating_add(crate::HA_OUTBOX_GC_RECOVERY_SLO_SECS)))
            } else {
                None
            };
            sqlx::query(
                "UPDATE ha_outbox_gc_state SET low_pressure_since = ?, recovery_mode = ?, recovery_deadline_at = ?, last_foreground_rps = ?, updated_at = ? WHERE id = 'local'",
            )
            .bind(low_pressure_since)
            .bind(i64::from(recovery_mode))
            .bind(recovery_deadline_at)
            .bind(foreground_rps)
            .bind(state_now)
            .execute(&mut **conn)
            .await?;
            let maximum_batch_size = options.batch_size.clamp(
                crate::HA_OUTBOX_GC_MIN_BATCH_SIZE,
                crate::HA_OUTBOX_GC_MAX_ONLINE_BATCH_SIZE,
            );
            let max_batches = options.max_batches.max(1);
            let (next_channel, pending_channel_mask): (String, i64) = sqlx::query_as(
                "SELECT next_channel, pending_channel_mask FROM ha_outbox_gc_state WHERE id = 'local'",
            )
            .fetch_one(&mut **conn)
            .await?;
            let was_completed_observation = pending_channel_mask == 0;
            let persisted_pending_channel_mask = if was_completed_observation {
                7
            } else {
                pending_channel_mask & 7
            };
            let preferred = Self::ha_outbox_gc_channel_from_name(&next_channel);
            let mut eligibility = sqlx::query_as::<_, HaGcChannelEligibility>(
                "SELECT channel, next_retry_at, claim_generation, claim_started_at, last_observed_at FROM ha_outbox_gc_channel_state",
            )
            .fetch_all(&mut **conn)
            .await?;
            let state_now = self.backend_time.now_ts();
            // Discovery is an in-memory probe set. It must join this slice's
            // fair rotation, but it is not durable debt until the selected
            // channel confirms more work. Otherwise an empty unknown channel
            // would create a one-second wake loop after every foreground burst.
            let discovery_channel_mask = HaGcController::stale_observation_mask(state_now, &eligibility);
            let pending_channel_mask = persisted_pending_channel_mask | discovery_channel_mask;
            let selected = HaGcController::claim_eligible_channel(
                preferred,
                pending_channel_mask,
                state_now,
                &eligibility,
            );
            let Some(claim) = selected else {
                let next_retry_at = eligibility
                    .iter()
                    .filter_map(|(name, retry_at, _, claim_started_at, _)| {
                        let channel = Self::ha_outbox_gc_channel_from_name(name);
                        if pending_channel_mask & Self::ha_outbox_gc_channel_mask(channel) == 0 {
                            return None;
                        }
                        Some(
                            retry_at
                                .unwrap_or(state_now)
                                .max(claim_started_at.map_or(state_now, |started_at| {
                                    started_at.saturating_add(HA_GC_CHANNEL_CLAIM_STALE_SECS)
                                })),
                        )
                    })
                    .min();
                return Ok(HaOutboxGcReport {
                    batch_size: options.batch_size,
                    max_batches: options.max_batches,
                    deleted_rows: 0,
                    batches: 0,
                    completed: false,
                    has_more: true,
                    channels: Vec::new(),
                    wal_checkpoint_busy: false,
                    wal_checkpoint_log_frames: 0,
                    wal_checkpoint_checkpointed_frames: 0,
                    active_elapsed_ms: 0,
                    max_batch_elapsed_ms: 0,
                    elapsed_ms: started.elapsed().as_millis(),
                    continuation_delay_secs: next_retry_at
                        .map(|retry_at| retry_at.saturating_sub(state_now).max(1)),
                });
            };
            let channel = claim.channel;
            let claimed = sqlx::query(
                r#"UPDATE ha_outbox_gc_channel_state
                   SET claim_generation = ?, claim_started_at = ?
                   WHERE channel = ? AND claim_generation = ?
                     AND (next_retry_at IS NULL OR next_retry_at <= ?)
                     AND (claim_started_at IS NULL OR claim_started_at <= ?)"#,
            )
            .bind(claim.generation)
            .bind(state_now)
            .bind(channel.as_str())
            .bind(claim.previous_generation)
            .bind(state_now)
            .bind(state_now.saturating_sub(HA_GC_CHANNEL_CLAIM_STALE_SECS))
            .execute(&mut **conn)
            .await?;
            if claimed.rows_affected() == 0 {
                return Ok(HaOutboxGcReport {
                    batch_size: options.batch_size,
                    max_batches: options.max_batches,
                    deleted_rows: 0,
                    batches: 0,
                    completed: false,
                    has_more: true,
                    channels: Vec::new(),
                    wal_checkpoint_busy: false,
                    wal_checkpoint_log_frames: 0,
                    wal_checkpoint_checkpointed_frames: 0,
                    active_elapsed_ms: 0,
                    max_batch_elapsed_ms: 0,
                    elapsed_ms: started.elapsed().as_millis(),
                    continuation_delay_secs: Some(1),
                });
            }
            let (persisted_batch_size, last_attempt_at, last_observed_at, last_high_watermark,
                _total_deleted_rows, _persisted_debt_mode, _persisted_oldest_age_secs,
                persisted_deleted_rows_per_minute, persisted_slo_state): HaOutboxGcChannelStateRow = sqlx::query_as(
                r#"SELECT batch_size, last_attempt_at, last_observed_at, last_high_watermark,
                          total_deleted_rows, debt_mode, oldest_deletable_age_secs,
                          deleted_rows_per_minute, slo_state
                   FROM ha_outbox_gc_channel_state WHERE channel = ?"#,
            )
            .bind(channel.as_str())
            .fetch_one(&mut **conn)
            .await?;
            let batch_size = persisted_batch_size.clamp(
                crate::HA_OUTBOX_GC_MIN_BATCH_SIZE,
                maximum_batch_size,
            );
            let retention_secs = ha_channel_retention_secs(channel);
            let threshold = self.backend_time.now_ts() - retention_secs;
            Self::remember_ha_channel_valid_watermark_on_conn(
                &mut *conn,
                channel,
                self.backend_time.now_ts(),
            )
            .await?;
            let mut deleted_rows = 0_i64;
            let mut batches = 0_i64;
            let mut invalid_legacy_deleted_rows = 0_i64;
            let mut retention_deleted_rows = 0_i64;
            let mut legacy_has_more = false;
            let mut legacy_cursor_advanced = false;
            let mut active_elapsed_ms = 0_u128;
            let mut max_batch_elapsed_ms = 0_u128;
            let mut observed_foreground_rps = foreground_rps;
            // Foreground pressure observed before the slice is authoritative:
            // claim/fairness state may advance, but no deletion transaction may
            // start until a later eligible wake.
            let mut foreground_yielded =
                foreground_rps > crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS;
            let mut batch_conn = Some(
                pooled_conn
                    .take()
                    .expect("online HA GC must retain its pooled connection"),
            );
            let gc_result = async {

            // A slice owns one persisted channel. Advancing the cursor after every
            // slice keeps a hot control stream from monopolizing online maintenance.
            while !foreground_yielded && batches < max_batches && Instant::now() < deadline {
                // Do not start another writer transaction after foreground work
                // arrives. The batch already in progress is allowed to finish so
                // the controller never leaks an open transaction.
                if batches > 0 {
                    observed_foreground_rps = foreground_rps_now();
                    if observed_foreground_rps > crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS {
                        foreground_yielded = true;
                        break;
                    }
                }
                let batch_started = Instant::now();
                let mut transaction = batch_conn
                    .as_mut()
                    .expect("online HA GC batch must retain its pooled connection")
                    .begin_immediate()
                    .await?;
                let batch_result = async {
                    let (deleted_invalid, scanned_more_legacy, scanned_legacy_rows) =
                        Self::delete_ha_invalid_legacy_events_bounded_on_conn(
                        &mut transaction,
                        channel,
                        batch_size,
                        self.backend_time.now_ts(),
                    )
                    .await?;
                    let mut deleted_retention = 0_i64;
                    if deleted_invalid == 0 {
                        let (deleted, max_deleted_valid_seq) =
                            Self::delete_ha_channel_events_returning_max_seq_on_conn(
                            &mut transaction,
                            channel,
                            threshold,
                            batch_size,
                        )
                        .await?;
                        deleted_retention = deleted;
                        if deleted_retention > 0
                            && let Some(max_deleted_valid_seq) = max_deleted_valid_seq
                        {
                            Self::remember_ha_channel_expired_valid_watermark_on_conn(
                                &mut transaction,
                                channel,
                                max_deleted_valid_seq,
                                self.backend_time.now_ts(),
                            )
                            .await?;
                        }
                    }
                    let batch_deleted_rows = deleted_invalid.saturating_add(deleted_retention);
                    if batch_deleted_rows > 0 {
                        sqlx::query(
                            "UPDATE ha_outbox_gc_channel_state SET total_deleted_rows = total_deleted_rows + ? WHERE channel = ?",
                        )
                        .bind(batch_deleted_rows)
                        .bind(channel.as_str())
                        .execute(&mut *transaction)
                        .await?;
                    }
                    Ok::<_, ProxyError>((
                        deleted_invalid,
                        deleted_retention,
                        scanned_more_legacy,
                        scanned_legacy_rows,
                    ))
                }
                .await;
                let (deleted_invalid, deleted_retention, scanned_more_legacy, scanned_legacy_rows) =
                    match batch_result {
                        Ok(result) => {
                            transaction.commit().await?;
                            result
                        }
                        Err(err) => {
                            let _ = transaction.rollback().await;
                            return Err(err);
                        }
                    };
                legacy_has_more = scanned_more_legacy;
                legacy_cursor_advanced |= scanned_legacy_rows;
                invalid_legacy_deleted_rows += deleted_invalid;
                retention_deleted_rows += deleted_retention;
                deleted_rows += deleted_invalid.saturating_add(deleted_retention);
                batches += 1;
                let retention_exhausted = deleted_invalid == 0 && deleted_retention < batch_size;

                record_ha_outbox_gc_batch_timing(
                    &mut active_elapsed_ms,
                    &mut max_batch_elapsed_ms,
                    batch_started,
                );
                if retention_exhausted {
                    break;
                }

                if batches < max_batches && Instant::now() < deadline {
                    self.backend_time
                        .sleep(Duration::from_millis(options.inter_batch_sleep_ms))
                        .await;
                }
            }

            let conn = batch_conn
                .as_mut()
                .expect("online HA GC must retain its pooled connection after each batch");
            let allowed_resources = ha_channel_allowed_resources_sql(channel);
            let has_more_retention: bool = sqlx::query_scalar(&format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE created_at < ? AND resource IN ({allowed_resources}) LIMIT 1)",
                quote_sqlite_identifier(ha_channel_event_table(channel)),
            ))
            .bind(threshold)
            .fetch_one(&mut **conn)
            .await?;
            let channel_has_more = legacy_has_more || has_more_retention;
            let oldest_deletable_created_at: Option<i64> = sqlx::query_scalar(&format!(
                "SELECT MIN(created_at) FROM {} WHERE created_at < ? AND resource IN ({allowed_resources})",
                quote_sqlite_identifier(ha_channel_event_table(channel)),
            ))
            .bind(threshold)
            .fetch_one(&mut **conn)
            .await?;
            let high_watermark = Self::ha_channel_high_watermark_on_conn(conn, channel).await?;
            let channel_mask = Self::ha_outbox_gc_channel_mask(channel);
            let next_pending_channel_mask = if channel_has_more {
                persisted_pending_channel_mask | channel_mask
            } else {
                persisted_pending_channel_mask & !channel_mask
            };
            let next_channel = Self::next_ha_outbox_gc_channel(channel);
            sqlx::query(
                r#"
                UPDATE ha_outbox_gc_state
                   SET next_channel = ?,
                       pending_channel_mask = ?,
                       low_pressure_since = CASE WHEN ? > ? THEN NULL ELSE low_pressure_since END,
                       recovery_mode = CASE WHEN ? > ? THEN 0 ELSE recovery_mode END,
                       recovery_deadline_at = CASE WHEN ? > ? THEN NULL ELSE recovery_deadline_at END,
                       last_foreground_rps = ?,
                       updated_at = ?
                 WHERE id = 'local'
                "#,
            )
            .bind(next_channel.as_str())
            .bind(next_pending_channel_mask)
            .bind(observed_foreground_rps)
            .bind(crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS)
            .bind(observed_foreground_rps)
            .bind(crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS)
            .bind(observed_foreground_rps)
            .bind(crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS)
            .bind(observed_foreground_rps)
            .bind(self.backend_time.now_ts())
            .execute(&mut **conn)
            .await?;
            let completed = next_pending_channel_mask == 0;
            let now = self.backend_time.now_ts();
            let channel_continuation_delay_secs = if has_more_retention {
                crate::ha_outbox_gc_continuation_delay_secs_for_pressure(
                    true,
                    max_batch_elapsed_ms,
                    recovery_mode,
                    observed_foreground_rps,
                )
            } else if legacy_has_more {
                Some(crate::HA_OUTBOX_GC_LEGACY_SCAN_CONTINUATION_DELAY_SECS)
            } else {
                None
            };
            let channel_next_retry_at =
                channel_continuation_delay_secs.map(|delay| now.saturating_add(delay));
            if let Some((_, next_retry_at, generation, claim_started_at, last_observed_at)) = eligibility
                .iter_mut()
                .find(|(name, _, _, _, _)| name == channel.as_str())
            {
                *next_retry_at = channel_next_retry_at;
                *generation = claim.generation;
                *claim_started_at = None;
                *last_observed_at = Some(now);
            }
            let wake_channel_mask = (!completed).then(|| {
                next_pending_channel_mask | HaGcController::stale_observation_mask(now, &eligibility)
            });
            let continuation_delay_secs = (!completed).then(|| {
                HaGcController::next_wake_delay_secs(
                    next_channel,
                    wake_channel_mask.expect("pending debt retains a wake mask"),
                    now,
                    &eligibility,
                    // A durable debt channel gives another eligible channel a
                    // prompt fair handoff. Unknown-only probes do not persist a
                    // continuation, so they cannot form a normal-mode fast loop.
                    crate::HA_OUTBOX_GC_RECOVERY_CONTINUATION_DELAY_SECS,
                )
            })
            .flatten();
            let defer_reason = channel_continuation_delay_secs.map(|delay| {
                if delay == crate::HA_OUTBOX_GC_LEGACY_SCAN_CONTINUATION_DELAY_SECS {
                    "legacy_scan"
                } else if foreground_yielded
                    || observed_foreground_rps > crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS
                {
                    "foreground_pressure"
                } else if max_batch_elapsed_ms > crate::HA_OUTBOX_GC_ACTIVE_BUDGET_MS {
                    "slow_slice"
                } else if delay == crate::HA_OUTBOX_GC_RECOVERY_CONTINUATION_DELAY_SECS {
                    "recovery"
                } else {
                    "fast_progress"
                }
            });
            let ingress_seq_delta = last_observed_at.map(|_| {
                high_watermark
                    .saturating_sub(last_high_watermark)
                    .max(0)
            });
            let net_rows_delta_estimate = ingress_seq_delta
                .map(|ingress| ingress.saturating_sub(deleted_rows));
            let next_batch_size = crate::next_ha_outbox_gc_batch_size(
                batch_size,
                maximum_batch_size,
                max_batch_elapsed_ms,
            );
            let oldest_deletable_age_secs = oldest_deletable_created_at
                .map(|created_at| now.saturating_sub(created_at).max(0));
            let elapsed_since_last_attempt_secs = last_attempt_at
                .map(|attempted_at| now.saturating_sub(attempted_at).max(1));
            let observed_deleted_rows_per_minute = elapsed_since_last_attempt_secs
                .map(|elapsed| deleted_rows.saturating_mul(60) as f64 / elapsed as f64)
                .unwrap_or(persisted_deleted_rows_per_minute);
            let debt_mode = if observed_foreground_rps > crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS {
                "foreground_pressure"
            } else if recovery_mode {
                "recovering"
            } else if channel_has_more {
                "draining"
            } else {
                "normal"
            };
            let slo_state = if recovery_mode
                && observed_foreground_rps <= crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS
            {
                match oldest_deletable_age_secs {
                    Some(age) if age > retention_secs.saturating_add(60 * 60) => "breached",
                    Some(_) => "on_track",
                    None => "clear",
                }
            } else if persisted_slo_state == "breached" && recovery_deadline_at.is_some() {
                "breached"
            } else {
                "unknown"
            };
            let slo_state_transition = if persisted_slo_state != slo_state {
                match (persisted_slo_state.as_str(), slo_state) {
                    (_, "breached") => Some("breached".to_string()),
                    ("breached", "on_track" | "clear") => Some("recovered".to_string()),
                    _ => None,
                }
            } else {
                None
            };
            let completed_claim = sqlx::query(
                r#"UPDATE ha_outbox_gc_channel_state
                   SET last_attempt_at = ?,
                       last_progress_at = CASE WHEN ? > 0 OR ? THEN ? ELSE last_progress_at END,
                       last_deleted_rows = ?,
                       last_defer_reason = ?,
                       next_retry_at = ?,
                       consecutive_no_progress = CASE
                           WHEN ? > 0 OR ? OR ? = 0 THEN 0
                           ELSE consecutive_no_progress + 1
                       END,
                       batch_size = ?,
                       last_observed_at = ?,
                       last_high_watermark = ?,
                       last_ingress_seq_delta = ?,
                       last_net_rows_delta_estimate = ?,
                       last_continuation_delay_secs = ?,
                       debt_mode = ?,
                       oldest_deletable_age_secs = ?,
                       deleted_rows_per_minute = ?,
                       recovery_deadline_at = ?,
                       slo_state = ?,
                       foreground_rps = ?,
                       claim_started_at = NULL
                   WHERE channel = ? AND claim_generation = ?"#,
            )
            .bind(now)
            .bind(deleted_rows)
            .bind(legacy_cursor_advanced)
            .bind(now)
            .bind(deleted_rows)
            .bind(defer_reason)
            .bind(channel_next_retry_at)
            .bind(deleted_rows)
            .bind(legacy_cursor_advanced)
            .bind(i64::from(channel_has_more))
            .bind(next_batch_size)
            .bind(now)
            .bind(high_watermark)
            .bind(ingress_seq_delta)
            .bind(net_rows_delta_estimate)
            .bind(channel_continuation_delay_secs)
            .bind(debt_mode)
            .bind(oldest_deletable_age_secs)
            .bind(observed_deleted_rows_per_minute)
            .bind(recovery_deadline_at)
            .bind(slo_state)
            .bind(observed_foreground_rps)
            .bind(channel.as_str())
            .bind(claim.generation)
            .execute(&mut **conn)
            .await?;
            if completed_claim.rows_affected() == 0 {
                return Err(ProxyError::Other(format!(
                    "stale HA GC channel claim for {} generation {}",
                    channel.as_str(),
                    claim.generation
                )));
            }
            let report = HaOutboxGcReport {
                batch_size,
                max_batches: options.max_batches,
                deleted_rows,
                batches,
                completed,
                has_more: !completed,
                channels: vec![HaOutboxGcChannelReport {
                    channel,
                    retention_secs,
                    threshold,
                    invalid_legacy_deleted_rows,
                    retention_deleted_rows,
                    deleted_rows,
                    batches,
                    has_more: channel_has_more,
                    debt_mode: debt_mode.to_string(),
                    oldest_deletable_age_secs,
                    deleted_rows_per_minute: observed_deleted_rows_per_minute,
                    recovery_deadline_at,
                    slo_state: slo_state.to_string(),
                    slo_state_transition,
                    foreground_rps: observed_foreground_rps,
                    observed_at: now,
                }],
                wal_checkpoint_busy: false,
                wal_checkpoint_log_frames: 0,
                wal_checkpoint_checkpointed_frames: 0,
                active_elapsed_ms,
                max_batch_elapsed_ms,
                elapsed_ms: started.elapsed().as_millis(),
                continuation_delay_secs,
            };
            Ok::<HaOutboxGcReport, ProxyError>(report)
            }
            .await;
            pooled_conn = batch_conn;
            match gc_result {
                Err(err) if crate::is_transient_sqlite_write_error(&err) => {
                    // This claim already identifies the affected channel. Persist
                    // its defer before returning so a busy control slice cannot
                    // turn into a global scheduler delay for billing or runtime.
                    drop(pooled_conn.take());
                    self.defer_claimed_ha_gc_channel_for_busy(
                        channel,
                        claim.generation,
                        persisted_pending_channel_mask,
                        options,
                        observed_foreground_rps,
                        started,
                    )
                    .await
                }
                other => other,
            }
        }
        .await;
        let deleted_rows = result
            .as_ref()
            .map(|report| report.deleted_rows)
            .unwrap_or_default();
        let connection_closed = if let Some(conn) = pooled_conn.take() {
            match conn.close().await {
                Ok(()) => true,
                Err(err) => {
                    tracing::warn!(
                        component = "ha_outbox_gc",
                        event = "connection_cleanup_failed",
                        error = %err,
                        "discarded online HA GC connection after cleanup failure"
                    );
                    false
                }
            }
        } else {
            false
        };
        if connection_closed && deleted_rows > 0 {
            self.sqlite_runtime
                .release_bulk_heap_after_connection_close();
        }
        result
    }
}

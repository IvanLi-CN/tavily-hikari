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

impl KeyStore {
    pub(crate) async fn ha_outbox_gc_watchdog_needed(&self) -> Result<bool, ProxyError> {
        let pending_channel_mask: Option<i64> = sqlx::query_scalar(
            "SELECT pending_channel_mask FROM ha_outbox_gc_state WHERE id = 'local'",
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(pending_channel_mask.is_none_or(|mask| mask & 7 != 0))
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
        self.gc_ha_outbox_online_with_options(
            HaOutboxGcOptions::online(),
            foreground_rps,
            low_pressure_since_floor,
        )
            .await
    }

    pub(crate) async fn record_ha_outbox_gc_deferred(
        &self,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let mut conn = tokio::time::timeout(Duration::from_millis(100), self.pool.acquire())
            .await
            .map_err(|_| ProxyError::Database(sqlx::Error::PoolTimedOut))??;
        sqlx::query("PRAGMA busy_timeout = 100")
            .execute(&mut *conn)
            .await?;
        let result = async {
            let mut transaction = ImmediateSqliteTransaction::begin(conn).await?;
            let (next_channel, persisted_pending_channel_mask): (String, i64) = sqlx::query_as(
                "SELECT next_channel, pending_channel_mask FROM ha_outbox_gc_state WHERE id = 'local'",
            )
            .fetch_one(&mut *transaction)
            .await?;
            let pending_channel_mask = if persisted_pending_channel_mask == 0 {
                7
            } else {
                persisted_pending_channel_mask & 7
            };
            let channel = Self::select_ha_outbox_gc_channel(
                Self::ha_outbox_gc_channel_from_name(&next_channel),
                pending_channel_mask,
            );
            let now = self.backend_time.now_ts();
            sqlx::query(
                r#"UPDATE ha_outbox_gc_state
                   SET pending_channel_mask = (? & 7) | ?, updated_at = ?
                   WHERE id = 'local'"#,
            )
            .bind(persisted_pending_channel_mask)
            .bind(Self::ha_outbox_gc_channel_mask(channel))
            .bind(now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                r#"UPDATE ha_outbox_gc_channel_state
                   SET last_attempt_at = ?, last_defer_reason = ?, next_retry_at = ?,
                       consecutive_no_progress = consecutive_no_progress + 1,
                       last_continuation_delay_secs = ?
                   WHERE channel = ?"#,
            )
            .bind(now)
            .bind(reason)
            .bind(now.saturating_add(crate::HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS))
            .bind(crate::HA_OUTBOX_GC_DEFERRED_CONTINUATION_DELAY_SECS)
            .bind(channel.as_str())
            .execute(&mut *transaction)
            .await?;
            transaction.commit_connection().await
        }
        .await;
        let mut conn = result?;
        let restore_result = sqlx::query(&format!(
            "PRAGMA busy_timeout = {}",
            SQLITE_BUSY_TIMEOUT_DEFAULT.as_millis()
        ))
        .execute(&mut *conn)
        .await;
        restore_result?;
        Ok(())
    }

    async fn gc_ha_outbox_online_with_options(
        &self,
        options: HaOutboxGcOptions,
        foreground_rps: i64,
        low_pressure_since_floor: i64,
    ) -> Result<HaOutboxGcReport, ProxyError> {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(options.max_runtime_secs.max(1));
        let mut pooled_conn = Some(
            tokio::time::timeout(Duration::from_millis(100), self.pool.acquire())
                .await
                .map_err(|_| ProxyError::Database(sqlx::Error::PoolTimedOut))??,
        );
        let result = async {
            let conn = pooled_conn
                .as_mut()
                .expect("online HA GC must own a pooled connection");
            sqlx::query("PRAGMA busy_timeout = 100")
                .execute(&mut **conn)
                .await?;
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
            let pending_channel_mask = if pending_channel_mask == 0 {
                7
            } else {
                pending_channel_mask & 7
            };
            let channel = Self::select_ha_outbox_gc_channel(
                Self::ha_outbox_gc_channel_from_name(&next_channel),
                pending_channel_mask,
            );
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
            let mut batch_conn = Some(
                pooled_conn
                    .take()
                    .expect("online HA GC must retain its pooled connection"),
            );
            let gc_result = async {

            // A slice owns one persisted channel. Advancing the cursor after every
            // slice keeps a hot control stream from monopolizing online maintenance.
            while batches < max_batches && Instant::now() < deadline {
                let batch_started = Instant::now();
                let mut transaction = ImmediateSqliteTransaction::begin(
                    batch_conn
                        .take()
                        .expect("online HA GC batch must retain its pooled connection"),
                )
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
                            batch_conn = Some(transaction.commit_connection().await?);
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
                pending_channel_mask | channel_mask
            } else {
                pending_channel_mask & !channel_mask
            };
            let next_channel = Self::next_ha_outbox_gc_channel(channel);
            sqlx::query(
                r#"
                UPDATE ha_outbox_gc_state
                   SET next_channel = ?, pending_channel_mask = ?, updated_at = ?
                 WHERE id = 'local'
                "#,
            )
            .bind(next_channel.as_str())
            .bind(next_pending_channel_mask)
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
                    foreground_rps,
                )
            } else if legacy_has_more {
                Some(crate::HA_OUTBOX_GC_LEGACY_SCAN_CONTINUATION_DELAY_SECS)
            } else {
                None
            };
            let continuation_delay_secs = if completed {
                None
            } else if let Some(delay) = channel_continuation_delay_secs {
                Some(delay)
            } else {
                crate::ha_outbox_gc_continuation_delay_secs_for_pressure(
                    true,
                    max_batch_elapsed_ms,
                    recovery_mode,
                    foreground_rps,
                )
            };
            let defer_reason = channel_continuation_delay_secs.map(|delay| {
                if delay == crate::HA_OUTBOX_GC_FAST_CONTINUATION_DELAY_SECS {
                    "fast_progress"
                } else if delay == crate::HA_OUTBOX_GC_LEGACY_SCAN_CONTINUATION_DELAY_SECS {
                    "legacy_scan"
                } else {
                    "slice_budget"
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
            let debt_mode = if recovery_mode {
                "recovering"
            } else if foreground_rps > crate::HA_OUTBOX_GC_LOW_PRESSURE_RPS {
                "foreground_pressure"
            } else if channel_has_more {
                "draining"
            } else {
                "normal"
            };
            let slo_state = if recovery_mode {
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
            sqlx::query(
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
                       foreground_rps = ?
                   WHERE channel = ?"#,
            )
            .bind(now)
            .bind(deleted_rows)
            .bind(legacy_cursor_advanced)
            .bind(now)
            .bind(deleted_rows)
            .bind(defer_reason)
            .bind(channel_continuation_delay_secs.map(|delay| now.saturating_add(delay)))
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
            .bind(foreground_rps)
            .bind(channel.as_str())
            .execute(&mut **conn)
            .await?;
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
                    foreground_rps,
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
            gc_result
        }
        .await;
        if let Some(mut conn) = pooled_conn.take() {
            let _ = sqlx::query(&format!(
                "PRAGMA busy_timeout = {}",
                crate::store::SQLITE_BUSY_TIMEOUT_DEFAULT.as_millis()
            ))
            .execute(&mut *conn)
            .await;
        }
        result
    }
}

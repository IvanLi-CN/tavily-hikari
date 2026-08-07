const HA_OUTBOX_GC_WORK_CLAIM_LEASE_SECS: i64 = 120;

fn ha_outbox_gc_work_job_type(channel: HaSyncChannel) -> &'static str {
    match channel {
        HaSyncChannel::Control => "ha_outbox_gc/control",
        HaSyncChannel::Billing => "ha_outbox_gc/billing",
        HaSyncChannel::Runtime => "ha_outbox_gc/runtime",
    }
}

impl KeyStore {
    async fn ensure_ha_outbox_gc_work_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS ha_outbox_gc_work (
                channel TEXT PRIMARY KEY CHECK (channel IN ('control', 'billing', 'runtime')),
                eligible_at INTEGER NOT NULL DEFAULT 0,
                claim_generation INTEGER NOT NULL DEFAULT 0,
                claim_started_at INTEGER,
                claim_expires_at INTEGER,
                batch_size INTEGER NOT NULL DEFAULT 250,
                last_outcome TEXT NOT NULL DEFAULT 'pending',
                last_outcome_detail TEXT,
                last_attempt_at INTEGER,
                last_progress_at INTEGER,
                last_deleted_rows INTEGER NOT NULL DEFAULT 0,
                last_continuation_delay_secs INTEGER
            )"#,
        )
        .execute(&self.pool)
        .await?;
        for channel in ["control", "billing", "runtime"] {
            sqlx::query(
                "INSERT OR IGNORE INTO ha_outbox_gc_work (channel) VALUES (?)",
            )
            .bind(channel)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn ha_outbox_gc_work_due_channels(
        &self,
    ) -> Result<HaOutboxGcWorkDueChannelsResult, ProxyError> {
        let mut connection = match self
            .sqlite_runtime
            .read(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(connection) => connection,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok(HaOutboxGcWorkDueChannelsResult::Busy);
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };
        let now = self.backend_time.now_ts();
        let rows = sqlx::query(
            r#"SELECT channel
               FROM ha_outbox_gc_work
               WHERE eligible_at <= ?
                 AND (
                     claim_started_at IS NULL
                     OR claim_expires_at IS NULL
                     OR claim_expires_at <= ?
                 )
               ORDER BY CASE channel
                   WHEN 'control' THEN 0
                   WHEN 'billing' THEN 1
                   WHEN 'runtime' THEN 2
               END"#,
        )
        .bind(now)
        .bind(now)
        .fetch_all(&mut *connection)
        .await?;
        let channels = rows
            .into_iter()
            .map(|row| {
                let channel = row.try_get::<String, _>("channel")?;
                HaSyncChannel::parse(&channel).ok_or_else(|| {
                    ProxyError::Other(format!("invalid HA GC work channel {channel}"))
                })
            })
            .collect::<Result<Vec<_>, ProxyError>>()?;
        Ok(HaOutboxGcWorkDueChannelsResult::Ready(channels))
    }

    pub(crate) async fn make_ha_outbox_gc_work_due(
        &self,
    ) -> Result<HaOutboxGcWorkDueResult, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = match self
            .sqlite_runtime
            .begin_immediate(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok(HaOutboxGcWorkDueResult::Busy);
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };
        sqlx::query(
            r#"UPDATE ha_outbox_gc_work
               SET eligible_at = MIN(eligible_at, ?),
                   last_outcome_detail = 'baseline'
               WHERE claim_started_at IS NULL"#,
        )
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(HaOutboxGcWorkDueResult::Refreshed)
    }

    pub(crate) async fn claim_ha_outbox_gc_work(
        &self,
        channel: HaSyncChannel,
    ) -> Result<HaOutboxGcWorkClaimResult, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = match self
            .sqlite_runtime
            .begin_immediate(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok(HaOutboxGcWorkClaimResult::Busy);
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };

        let row = sqlx::query(
            r#"SELECT eligible_at, claim_generation, claim_started_at,
                      claim_expires_at, batch_size
               FROM ha_outbox_gc_work
               WHERE channel = ?"#,
        )
        .bind(channel.as_str())
        .fetch_one(&mut *transaction)
        .await?;
        let eligible_at = row.try_get::<i64, _>("eligible_at")?;
        let claim_generation = row.try_get::<i64, _>("claim_generation")?;
        let claim_started_at = row.try_get::<Option<i64>, _>("claim_started_at")?;
        let claim_expires_at = row.try_get::<Option<i64>, _>("claim_expires_at")?;
        let batch_size = row.try_get::<i64, _>("batch_size")?;

        if eligible_at > now {
            transaction.commit().await?;
            return Ok(HaOutboxGcWorkClaimResult::NotEligible { eligible_at });
        }
        if claim_started_at.is_some()
            && let Some(claim_expires_at) = claim_expires_at.filter(|expires| *expires > now)
        {
            transaction.commit().await?;
            return Ok(HaOutboxGcWorkClaimResult::AlreadyClaimed {
                claim_generation,
                claim_expires_at,
            });
        }

        let claim_generation = claim_generation.saturating_add(1);
        let claim_expires_at = now.saturating_add(HA_OUTBOX_GC_WORK_CLAIM_LEASE_SECS);
        sqlx::query(
            r#"UPDATE ha_outbox_gc_work
               SET claim_generation = ?,
                   claim_started_at = ?,
                   claim_expires_at = ?,
                   last_attempt_at = ?,
                   last_outcome = 'running',
                   last_outcome_detail = NULL
               WHERE channel = ?"#,
        )
        .bind(claim_generation)
        .bind(now)
        .bind(claim_expires_at)
        .bind(now)
        .bind(channel.as_str())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        Ok(HaOutboxGcWorkClaimResult::Claimed(HaOutboxGcWorkClaim {
            channel,
            claim_generation,
            batch_size,
            eligible_at,
        }))
    }

    pub(crate) async fn finish_ha_outbox_gc_work(
        &self,
        claim: HaOutboxGcWorkClaim,
        outcome: HaOutboxGcWorkOutcome,
        eligible_at: i64,
    ) -> Result<HaOutboxGcWorkFinishResult, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = match self
            .sqlite_runtime
            .begin_immediate(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok(HaOutboxGcWorkFinishResult::Busy);
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };

        let updated = sqlx::query(
            r#"UPDATE ha_outbox_gc_work
               SET eligible_at = ?,
                   claim_started_at = NULL,
                   claim_expires_at = NULL,
                   last_outcome = ?,
                   last_progress_at = ?,
                   last_continuation_delay_secs = MAX(0, ? - ?)
               WHERE channel = ?
                 AND claim_generation = ?
                 AND claim_started_at IS NOT NULL
                 AND claim_expires_at > ?"#,
        )
        .bind(eligible_at)
        .bind(outcome.as_str())
        .bind(now)
        .bind(eligible_at)
        .bind(now)
        .bind(claim.channel.as_str())
        .bind(claim.claim_generation)
        .bind(now)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        transaction.commit().await?;

        if updated == 0 {
            Ok(HaOutboxGcWorkFinishResult::Stale)
        } else {
            Ok(HaOutboxGcWorkFinishResult::Finished(outcome))
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn finish_ha_outbox_gc_work_and_enqueue(
        &self,
        scheduled_job_id: i64,
        scheduled_claim_generation: i64,
        claim: HaOutboxGcWorkClaim,
        outcome: HaOutboxGcWorkOutcome,
        eligible_at: i64,
        status: &str,
        message: Option<&str>,
        continuation_job_type: Option<&str>,
        continuation_available_at: Option<i64>,
        deleted_rows: i64,
    ) -> Result<
        (
            HaOutboxGcWorkFinishResult,
            Option<ScheduledJobEnqueueResult>,
        ),
        ProxyError,
    > {
        let finished_at = self.backend_time.now_ts();
        let mut transaction = match self
            .sqlite_runtime
            .begin_immediate(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok((HaOutboxGcWorkFinishResult::Busy, None));
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };

        let updated_work = sqlx::query(
            r#"UPDATE ha_outbox_gc_work
               SET eligible_at = ?,
                   claim_started_at = NULL,
                   claim_expires_at = NULL,
                   last_outcome = ?,
                   last_outcome_detail = ?,
                   last_progress_at = ?,
                   last_deleted_rows = ?,
                   last_continuation_delay_secs = MAX(0, ? - ?),
                   batch_size = COALESCE(
                       (SELECT batch_size FROM ha_outbox_gc_channel_state WHERE channel = ?),
                       batch_size
                   )
               WHERE channel = ?
                 AND claim_generation = ?
                 AND claim_started_at IS NOT NULL
                 AND claim_expires_at > ?"#,
        )
        .bind(eligible_at)
        .bind(outcome.as_str())
        .bind(message)
        .bind(finished_at)
        .bind(deleted_rows)
        .bind(eligible_at)
        .bind(finished_at)
        .bind(claim.channel.as_str())
        .bind(claim.channel.as_str())
        .bind(claim.claim_generation)
        .bind(finished_at)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated_work == 0 {
            let _ = transaction.rollback().await;
            return Ok((HaOutboxGcWorkFinishResult::Stale, None));
        }

        let updated_job = sqlx::query(
            r#"UPDATE scheduled_jobs
               SET status = ?, message = ?, finished_at = ?
               WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
        )
        .bind(status)
        .bind(message)
        .bind(finished_at)
        .bind(scheduled_job_id)
        .bind(scheduled_claim_generation)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated_job == 0 {
            let _ = transaction.rollback().await;
            return Ok((HaOutboxGcWorkFinishResult::Stale, None));
        }

        let continuation = match (continuation_job_type, continuation_available_at) {
            (Some(job_type), Some(available_at)) => {
                if let Some((continuation_id, continuation_status, trigger_source)) =
                    Self::scheduled_job_lookup_active_locked(&mut transaction, job_type, None)
                        .await?
                {
                    if continuation_status == "queued" {
                        sqlx::query(
                            "UPDATE scheduled_jobs
                             SET available_at = MIN(available_at, ?), ha_gc_work_generation = ?
                             WHERE id = ?",
                        )
                        .bind(available_at)
                        .bind(claim.claim_generation)
                        .bind(continuation_id)
                        .execute(&mut *transaction)
                        .await?;
                    }
                    Some(ScheduledJobEnqueueResult {
                        job_id: continuation_id,
                        created: false,
                        promoted: false,
                        status: continuation_status,
                        trigger_source,
                    })
                } else {
                    let inserted = sqlx::query(
                        r#"INSERT INTO scheduled_jobs (
                               job_type,
                               trigger_source,
                               key_id,
                               status,
                               attempt,
                               queued_at,
                               available_at,
                               ha_gc_work_generation,
                               started_at,
                               finished_at
                           ) VALUES (?, 'auto', NULL, 'queued', 1, ?, ?, ?, NULL, NULL)"#,
                    )
                    .bind(job_type)
                    .bind(finished_at)
                    .bind(available_at)
                    .bind(claim.claim_generation)
                    .execute(&mut *transaction)
                    .await?;
                    Some(ScheduledJobEnqueueResult {
                        job_id: inserted.last_insert_rowid(),
                        created: true,
                        promoted: false,
                        status: "queued".to_string(),
                        trigger_source: "auto".to_string(),
                    })
                }
            }
            _ => None,
        };
        transaction.commit().await?;
        Ok((
            HaOutboxGcWorkFinishResult::Finished(outcome),
            continuation,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn defer_ha_outbox_gc_job_and_enqueue(
        &self,
        scheduled_job_id: i64,
        scheduled_claim_generation: i64,
        channel: HaSyncChannel,
        outcome: HaOutboxGcWorkOutcome,
        eligible_at: i64,
        message: &str,
        continuation_available_at: i64,
    ) -> Result<
        (
            HaOutboxGcWorkFinishResult,
            Option<ScheduledJobEnqueueResult>,
        ),
        ProxyError,
    > {
        let finished_at = self.backend_time.now_ts();
        let mut transaction = match self
            .sqlite_runtime
            .begin_immediate(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok((HaOutboxGcWorkFinishResult::Busy, None));
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };

        let Some((Some(expected_work_generation),)) = sqlx::query_as::<_, (Option<i64>,)>(
            r#"SELECT ha_gc_work_generation
               FROM scheduled_jobs
               WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
        )
        .bind(scheduled_job_id)
        .bind(scheduled_claim_generation)
        .fetch_optional(&mut *transaction)
        .await?
        else {
            let _ = transaction.rollback().await;
            return Ok((HaOutboxGcWorkFinishResult::Stale, None));
        };

        let updated_work = sqlx::query(
            r#"UPDATE ha_outbox_gc_work
               SET eligible_at = ?,
                   claim_started_at = CASE
                       WHEN claim_expires_at IS NULL OR claim_expires_at <= ? THEN NULL
                       ELSE claim_started_at
                   END,
                   claim_expires_at = CASE
                       WHEN claim_expires_at IS NULL OR claim_expires_at <= ? THEN NULL
                       ELSE claim_expires_at
                   END,
                   last_outcome = ?,
                   last_outcome_detail = ?,
                   last_attempt_at = ?,
                   last_continuation_delay_secs = MAX(0, ? - ?)
               WHERE channel = ?
                 AND claim_generation = ?
                 AND (
                     claim_started_at IS NULL
                     OR claim_expires_at IS NULL
                     OR claim_expires_at <= ?
                 )
                 AND last_outcome IN ('pending', 'busy', 'deferred')"#,
        )
        .bind(eligible_at)
        .bind(finished_at)
        .bind(finished_at)
        .bind(outcome.as_str())
        .bind(message)
        .bind(finished_at)
        .bind(eligible_at)
        .bind(finished_at)
        .bind(channel.as_str())
        .bind(expected_work_generation)
        .bind(finished_at)
        .execute(&mut *transaction)
        .await?
        .rows_affected();

        let updated_job = sqlx::query(
            r#"UPDATE scheduled_jobs
               SET status = 'success', message = ?, finished_at = ?
               WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
        )
        .bind(message)
        .bind(finished_at)
        .bind(scheduled_job_id)
        .bind(scheduled_claim_generation)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated_job == 0 {
            let _ = transaction.rollback().await;
            return Ok((HaOutboxGcWorkFinishResult::Stale, None));
        }

        let job_type = ha_outbox_gc_work_job_type(channel);
        let continuation = if updated_work == 0 {
            None
        } else if let Some((continuation_id, status, trigger_source)) =
            Self::scheduled_job_lookup_active_locked(&mut transaction, job_type, None).await?
        {
            if status == "queued" {
                sqlx::query(
                    "UPDATE scheduled_jobs
                     SET available_at = MIN(available_at, ?), ha_gc_work_generation = ?
                     WHERE id = ?",
                )
                .bind(continuation_available_at)
                .bind(expected_work_generation)
                .bind(continuation_id)
                .execute(&mut *transaction)
                .await?;
            }
            Some(ScheduledJobEnqueueResult {
                job_id: continuation_id,
                created: false,
                promoted: false,
                status,
                trigger_source,
            })
        } else {
            let inserted = sqlx::query(
                r#"INSERT INTO scheduled_jobs (
                       job_type,
                       trigger_source,
                       key_id,
                       status,
                       attempt,
                       queued_at,
                       available_at,
                       ha_gc_work_generation,
                       started_at,
                       finished_at
                   ) VALUES (?, 'auto', NULL, 'queued', 1, ?, ?, ?, NULL, NULL)"#,
            )
            .bind(job_type)
            .bind(finished_at)
            .bind(continuation_available_at)
            .bind(expected_work_generation)
            .execute(&mut *transaction)
            .await?;
            Some(ScheduledJobEnqueueResult {
                job_id: inserted.last_insert_rowid(),
                created: true,
                promoted: false,
                status: "queued".to_string(),
                trigger_source: "auto".to_string(),
            })
        };

        transaction.commit().await?;
        Ok((
            if updated_work == 0 {
                HaOutboxGcWorkFinishResult::Stale
            } else {
                HaOutboxGcWorkFinishResult::Finished(outcome)
            },
            continuation,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn defer_ha_outbox_gc_claim_and_enqueue(
        &self,
        scheduled_job_id: i64,
        scheduled_claim_generation: i64,
        claim: HaOutboxGcWorkClaim,
        eligible_at: i64,
        message: &str,
        continuation_available_at: i64,
    ) -> Result<
        (
            HaOutboxGcWorkFinishResult,
            Option<ScheduledJobEnqueueResult>,
        ),
        ProxyError,
    > {
        let finished_at = self.backend_time.now_ts();
        let mut transaction = match self
            .sqlite_runtime
            .begin_immediate(&CancellationToken::new())
            .await
        {
            SqliteOperationOutcome::Completed(transaction) => transaction,
            SqliteOperationOutcome::Deferred(SqliteDeferredReason::Busy)
            | SqliteOperationOutcome::Deferred(SqliteDeferredReason::Cancelled) => {
                return Ok((HaOutboxGcWorkFinishResult::Busy, None));
            }
            SqliteOperationOutcome::Failed(err) => return Err(err),
        };

        let updated_work = sqlx::query(
            r#"UPDATE ha_outbox_gc_work
               SET eligible_at = ?,
                   claim_started_at = NULL,
                   claim_expires_at = NULL,
                   last_outcome = 'busy',
                   last_outcome_detail = ?,
                   last_progress_at = ?,
                   last_continuation_delay_secs = MAX(0, ? - ?)
               WHERE channel = ?
                 AND claim_generation = ?
                 AND claim_started_at IS NOT NULL
                 AND claim_expires_at > ?"#,
        )
        .bind(eligible_at)
        .bind(message)
        .bind(finished_at)
        .bind(eligible_at)
        .bind(finished_at)
        .bind(claim.channel.as_str())
        .bind(claim.claim_generation)
        .bind(finished_at)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated_work == 0 {
            let _ = transaction.rollback().await;
            return Ok((HaOutboxGcWorkFinishResult::Stale, None));
        }

        let updated_job = sqlx::query(
            r#"UPDATE scheduled_jobs
               SET status = 'success', message = ?, finished_at = ?
               WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
        )
        .bind(message)
        .bind(finished_at)
        .bind(scheduled_job_id)
        .bind(scheduled_claim_generation)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if updated_job == 0 {
            let _ = transaction.rollback().await;
            return Ok((HaOutboxGcWorkFinishResult::Stale, None));
        }

        let job_type = ha_outbox_gc_work_job_type(claim.channel);
        let continuation = if let Some((continuation_id, status, trigger_source)) =
            Self::scheduled_job_lookup_active_locked(&mut transaction, job_type, None).await?
        {
            if status == "queued" {
                sqlx::query(
                    "UPDATE scheduled_jobs
                     SET available_at = MIN(available_at, ?), ha_gc_work_generation = ?
                     WHERE id = ?",
                )
                .bind(continuation_available_at)
                .bind(claim.claim_generation)
                .bind(continuation_id)
                .execute(&mut *transaction)
                .await?;
            }
            Some(ScheduledJobEnqueueResult {
                job_id: continuation_id,
                created: false,
                promoted: false,
                status,
                trigger_source,
            })
        } else {
            let inserted = sqlx::query(
                r#"INSERT INTO scheduled_jobs (
                       job_type,
                       trigger_source,
                       key_id,
                       status,
                       attempt,
                       queued_at,
                       available_at,
                       ha_gc_work_generation,
                       started_at,
                       finished_at
                   ) VALUES (?, 'auto', NULL, 'queued', 1, ?, ?, ?, NULL, NULL)"#,
            )
            .bind(job_type)
            .bind(finished_at)
            .bind(continuation_available_at)
            .bind(claim.claim_generation)
            .execute(&mut *transaction)
            .await?;
            Some(ScheduledJobEnqueueResult {
                job_id: inserted.last_insert_rowid(),
                created: true,
                promoted: false,
                status: "queued".to_string(),
                trigger_source: "auto".to_string(),
            })
        };

        transaction.commit().await?;
        Ok((
            HaOutboxGcWorkFinishResult::Finished(HaOutboxGcWorkOutcome::Busy),
            continuation,
        ))
    }
}

use crate::store::sqlite_runtime::SqliteCooperativeQueryOutcome;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconciliationProjectionSliceOutcome {
    Advanced { scanned_rows: i64, completed: bool },
    Deferred { reason: &'static str },
    StaleClaim,
}

#[derive(Clone)]
struct ReconciliationProjectionAggregate {
    project_id: String,
    billing_subject: String,
    settlement_mode: String,
    period_start: i64,
    period_end: i64,
    scheduling_key_id: String,
    updated_at: i64,
    terminal_outcome: Option<String>,
}

type ReconciliationProjectionSourceRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    Option<String>,
    Option<i64>,
);

type ReconciliationProjectionStateRow =
    (String, String, String, i64, i64, i64, i64, i64, i64, i64, i64, i64);

struct ReconciliationProjectionController<'a> {
    store: &'a KeyStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReconciliationProjectionWriteStatus {
    Advanced,
    StaleClaim,
    CursorConflict,
}

impl<'a> ReconciliationProjectionController<'a> {
    const SQLITE_PRESSURE_DEFER_SECS: i64 = 30;
    const SOURCE_READ_BUDGET: std::time::Duration = std::time::Duration::from_millis(250);

    fn new(store: &'a KeyStore) -> Self {
        Self { store }
    }

    async fn advance_slice(
        &self,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<ReconciliationProjectionSliceOutcome, ProxyError> {
        match self.advance_slice_inner(claimed_job).await {
            Err(err) if is_transient_sqlite_write_error(&err) => {
                match self
                    .record_deferred_slice(claimed_job, "sqlite_pressure")
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        return Ok(ReconciliationProjectionSliceOutcome::Advanced {
                            scanned_rows: 0,
                            completed: true,
                        });
                    }
                    Err(record_err) if is_transient_sqlite_write_error(&record_err) => {}
                    Err(ProxyError::StaleClaim { .. }) => {
                        return Ok(ReconciliationProjectionSliceOutcome::StaleClaim);
                    }
                    Err(record_err) => return Err(record_err),
                }
                Ok(ReconciliationProjectionSliceOutcome::Deferred {
                    reason: "sqlite_pressure",
                })
            }
            result => result,
        }
    }

    /// Preserve the controller-owned delay whenever a short retry window is available. A
    /// contended writer can prevent this optional observation update too; the caller's atomic
    /// representative continuation remains the durable recovery path in that case.
    async fn record_deferred_slice(
        &self,
        claimed_job: Option<(i64, i64)>,
        reason: &'static str,
    ) -> Result<bool, ProxyError> {
        let now = self.store.backend_time.now_ts();
        let mut tx = self
            .store
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        let write_result = async {
            if let Some((job_id, claim_generation)) = claimed_job
                && !Self::claim_is_current(&mut tx, claimed_job).await?
            {
                return Err(ProxyError::StaleClaim {
                    job_id,
                    claim_generation,
                });
            }
            let updated = sqlx::query(
                r#"UPDATE upstream_reconciliation_projection_state
                   SET next_retry_at = ?, last_defer_reason = ?, updated_at = ?
                   WHERE id = 'local' AND completed = 0"#,
            )
            .bind(now.saturating_add(Self::SQLITE_PRESSURE_DEFER_SECS))
            .bind(reason)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Ok(false);
            }
            sqlx::query(
                "UPDATE upstream_reconciliation_run_observation SET projection_state = 'deferred', cursor_advanced = 0, observed_at = ? WHERE id = 'local'",
            )
            .bind(now)
            .execute(&mut *tx)
            .await?;
            Ok(true)
        }
        .await;
        match write_result {
            Ok(persisted) => {
                tx.finish(Ok(())).await?;
                Ok(persisted)
            }
            Err(err) => {
                tx.finish(Err(err)).await?;
                unreachable!("finishing a failed deferred projection transaction returns the error")
            }
        }
    }

    async fn advance_slice_inner(
        &self,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<ReconciliationProjectionSliceOutcome, ProxyError> {
        let mut snapshot = self
            .store
            .sqlite_runtime
            .begin_read_snapshot(SqliteOperation::ReconciliationProjection)
            .await?;
        snapshot
            .arm_cooperative_run_budget(Self::SOURCE_READ_BUDGET)
            .await?;
        let read_result = async {
            let state: ReconciliationProjectionStateRow = sqlx::query_as(
                r#"SELECT cursor_token_id, cursor_key_id, cursor_period_code,
                      batch_size, fast_slice_streak, completed,
                      tx_hold_le_10, tx_hold_le_25, tx_hold_le_50,
                      tx_hold_le_100, tx_hold_le_250, tx_hold_over_250
               FROM upstream_reconciliation_projection_state WHERE id = 'local'"#,
            )
            .fetch_one(&mut *snapshot)
            .await?;
            if state.5 != 0 {
                return Ok((state, Vec::new()));
            }
            let batch_size = state.3.clamp(
                RECONCILIATION_PROJECTION_MIN_BATCH,
                RECONCILIATION_PROJECTION_MAX_BATCH,
            );
            let rows = sqlx::query_as(
                r#"SELECT u.token_id, u.key_id, u.period_code, u.project_id,
                          u.billing_subject, u.settlement_mode, u.period_start,
                          u.period_end, u.updated_at, s.status, s.delta_credits
                   FROM upstream_reconciliation_usage u
                   LEFT JOIN upstream_reconciliation_settlements s
                     ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                   WHERE (u.token_id, u.key_id, u.period_code) > (?, ?, ?)
                   ORDER BY u.token_id, u.key_id, u.period_code
                   LIMIT ?"#,
            )
            .bind(&state.0)
            .bind(&state.1)
            .bind(&state.2)
            .bind(batch_size)
            .fetch_all(&mut *snapshot)
            .await?;
            Ok((state, rows))
        }
        .await;
        let (state, rows): (ReconciliationProjectionStateRow, Vec<ReconciliationProjectionSourceRow>) =
            match snapshot.complete_cooperative_query(read_result).await? {
                SqliteCooperativeQueryOutcome::Completed(result) => result,
                SqliteCooperativeQueryOutcome::DeadlineExceeded => {
                    return self.defer_source_read_budget(claimed_job).await;
                }
            };
        if state.5 != 0 {
            return Ok(ReconciliationProjectionSliceOutcome::Advanced {
                scanned_rows: 0,
                completed: true,
            });
        }
        let batch_size = state
            .3
            .clamp(RECONCILIATION_PROJECTION_MIN_BATCH, RECONCILIATION_PROJECTION_MAX_BATCH);
        let Some(last) = rows.last().map(|row| (row.0.clone(), row.1.clone(), row.2.clone()))
        else {
            let mut tx = self
                .store
                .sqlite_runtime
                .begin_immediate(SqliteOperation::ReconciliationProjection)
                .await?;
            let write_result = async {
                if !Self::claim_is_current(&mut tx, claimed_job).await? {
                    return Ok(ReconciliationProjectionWriteStatus::StaleClaim);
                }
                let updated = sqlx::query(
                    r#"UPDATE upstream_reconciliation_projection_state
                       SET completed = 1, next_retry_at = 0, last_defer_reason = NULL,
                           updated_at = ?
                       WHERE id = 'local' AND cursor_token_id = ? AND cursor_key_id = ?
                         AND cursor_period_code = ? AND completed = 0"#,
                )
                .bind(self.store.backend_time.now_ts())
                .bind(&state.0)
                .bind(&state.1)
                .bind(&state.2)
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() != 1 {
                    return Ok(ReconciliationProjectionWriteStatus::CursorConflict);
                }
                sqlx::query("INSERT INTO meta (key, value) VALUES (?, '1') ON CONFLICT(key) DO UPDATE SET value = excluded.value")
                    .bind(META_KEY_UPSTREAM_RECONCILIATION_WORK_PROJECTION_COMPLETE_V1)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "UPDATE upstream_reconciliation_run_observation SET projection_state = 'complete', cursor_advanced = 0, observed_at = ? WHERE id = 'local'",
                )
                .bind(self.store.backend_time.now_ts())
                .execute(&mut *tx)
                .await?;
                Ok(ReconciliationProjectionWriteStatus::Advanced)
            }
            .await;
            return match write_result {
                Ok(ReconciliationProjectionWriteStatus::Advanced) => {
                    tx.finish(Ok(())).await?;
                    Ok(ReconciliationProjectionSliceOutcome::Advanced {
                        scanned_rows: 0,
                        completed: true,
                    })
                }
                Ok(ReconciliationProjectionWriteStatus::StaleClaim) => {
                    tx.rollback().await?;
                    Ok(ReconciliationProjectionSliceOutcome::StaleClaim)
                }
                Ok(ReconciliationProjectionWriteStatus::CursorConflict) => {
                    tx.rollback().await?;
                    Ok(ReconciliationProjectionSliceOutcome::Deferred {
                        reason: "cursor_conflict",
                    })
                }
                Err(err) => {
                    tx.finish(Err(err)).await?;
                    unreachable!("finishing a failed projection transaction returns the error")
                }
            };
        };

        let mut aggregates = std::collections::BTreeMap::<
            (String, String),
            ReconciliationProjectionAggregate,
        >::new();
        for row in &rows {
            let entry = aggregates
                .entry((row.0.clone(), row.2.clone()))
                .or_insert_with(|| ReconciliationProjectionAggregate {
                    project_id: row.3.clone(),
                    billing_subject: row.4.clone(),
                    settlement_mode: row.5.clone(),
                    period_start: row.6,
                    period_end: row.7,
                    scheduling_key_id: row.1.clone(),
                    updated_at: row.8,
                    terminal_outcome: projection_terminal_outcome(
                        row.9.as_deref(),
                        row.10,
                    ),
                });
            if row.3 < entry.project_id {
                entry.project_id.clone_from(&row.3);
            }
            if row.4 < entry.billing_subject {
                entry.billing_subject.clone_from(&row.4);
            }
            if row.5 < entry.settlement_mode {
                entry.settlement_mode.clone_from(&row.5);
            }
            entry.period_start = entry.period_start.min(row.6);
            entry.period_end = entry.period_end.max(row.7);
            if row.1 < entry.scheduling_key_id {
                entry.scheduling_key_id.clone_from(&row.1);
            }
            entry.updated_at = entry.updated_at.max(row.8);
            if entry.terminal_outcome.is_none() {
                entry.terminal_outcome = projection_terminal_outcome(row.9.as_deref(), row.10);
            }
        }

        let terminal_repairs = aggregates
            .iter()
            .filter_map(|((token_id, period_code), aggregate)| {
                aggregate.terminal_outcome.as_ref().map(|outcome| {
                    (token_id.clone(), period_code.clone(), outcome.clone())
                })
            })
            .collect::<Vec<_>>();

        let mut tx = self
            .store
            .sqlite_runtime
            .begin_immediate(SqliteOperation::ReconciliationProjection)
            .await?;
        let write_started = std::time::Instant::now();
        let write_result = async {
            if !Self::claim_is_current(&mut tx, claimed_job).await? {
                return Ok(ReconciliationProjectionWriteStatus::StaleClaim);
            }
            if !aggregates.is_empty() {
            let mut merge = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                r#"INSERT INTO upstream_reconciliation_work (
                     token_id, period_code, project_id, billing_subject, settlement_mode,
                     period_start, period_end, scheduling_key_id, updated_at
                   ) "#,
            );
            merge.push_values(aggregates, |mut values, ((token_id, period_code), aggregate)| {
                values
                    .push_bind(token_id)
                    .push_bind(period_code)
                    .push_bind(aggregate.project_id)
                    .push_bind(aggregate.billing_subject)
                    .push_bind(aggregate.settlement_mode)
                    .push_bind(aggregate.period_start)
                    .push_bind(aggregate.period_end)
                    .push_bind(aggregate.scheduling_key_id)
                    .push_bind(aggregate.updated_at);
            });
            merge.push(
                r#" ON CONFLICT(token_id, period_code) DO UPDATE SET
                     project_id = MIN(upstream_reconciliation_work.project_id, excluded.project_id),
                     billing_subject = MIN(upstream_reconciliation_work.billing_subject, excluded.billing_subject),
                     settlement_mode = MIN(upstream_reconciliation_work.settlement_mode, excluded.settlement_mode),
                     period_start = MIN(upstream_reconciliation_work.period_start, excluded.period_start),
                     period_end = MAX(upstream_reconciliation_work.period_end, excluded.period_end),
                     scheduling_key_id = MIN(upstream_reconciliation_work.scheduling_key_id, excluded.scheduling_key_id),
                     updated_at = MAX(upstream_reconciliation_work.updated_at, excluded.updated_at)"#,
            );
                merge.build().execute(&mut *tx).await?;
            }
            if !terminal_repairs.is_empty() {
                let mut repair = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                    "UPDATE upstream_reconciliation_work SET last_outcome = CASE ",
                );
                for (token_id, period_code, outcome) in &terminal_repairs {
                    repair
                        .push("WHEN token_id = ")
                        .push_bind(token_id)
                        .push(" AND period_code = ")
                        .push_bind(period_code)
                        .push(" THEN ")
                        .push_bind(outcome)
                        .push(" ");
                }
                repair.push(
                    "ELSE last_outcome END WHERE completed_generation >= work_generation AND (",
                );
                for (index, (token_id, period_code, _)) in terminal_repairs.iter().enumerate() {
                    if index > 0 {
                        repair.push(" OR ");
                    }
                    repair
                        .push("(token_id = ")
                        .push_bind(token_id)
                        .push(" AND period_code = ")
                        .push_bind(period_code)
                        .push(")");
                }
                repair.push(")");
                repair.build().execute(&mut *tx).await?;
            }
        let write_ms = write_started.elapsed().as_millis() as i64;
        let mut hold_histogram = [state.6, state.7, state.8, state.9, state.10, state.11];
        let hold_bucket = RECONCILIATION_PROJECTION_HOLD_BUCKETS_MS
            .iter()
            .position(|upper| write_ms <= *upper)
            .unwrap_or(RECONCILIATION_PROJECTION_HOLD_BUCKETS_MS.len() - 1);
        hold_histogram[hold_bucket] = hold_histogram[hold_bucket].saturating_add(1);
        let transaction_p95_ms = reconciliation_projection_hold_p95_ms(&hold_histogram);
        let fast_streak = if write_ms <= 50 { state.4 + 1 } else { 0 };
        let next_batch = if write_ms > 100 {
            (batch_size / 2).max(RECONCILIATION_PROJECTION_MIN_BATCH)
        } else if fast_streak >= 2 {
            (batch_size + 25).min(RECONCILIATION_PROJECTION_MAX_BATCH)
        } else {
            batch_size
        };
        let continuation_secs = if self.store.foreground_activity_rps() <= 5 {
            1
        } else {
            5
        };
            let updated = sqlx::query(
            r#"UPDATE upstream_reconciliation_projection_state
               SET cursor_token_id = ?, cursor_key_id = ?, cursor_period_code = ?,
                   batch_size = ?, fast_slice_streak = ?, scanned_rows = scanned_rows + ?,
                   transaction_p95_ms = ?, tx_hold_le_10 = ?, tx_hold_le_25 = ?,
                   tx_hold_le_50 = ?, tx_hold_le_100 = ?, tx_hold_le_250 = ?,
                   tx_hold_over_250 = ?,
                   next_retry_at = ?, last_defer_reason = NULL, updated_at = ?
               WHERE id = 'local' AND cursor_token_id = ? AND cursor_key_id = ?
                 AND cursor_period_code = ? AND completed = 0"#,
        )
        .bind(&last.0)
        .bind(&last.1)
        .bind(&last.2)
        .bind(next_batch)
        .bind(fast_streak)
        .bind(rows.len() as i64)
        .bind(transaction_p95_ms)
        .bind(hold_histogram[0])
        .bind(hold_histogram[1])
        .bind(hold_histogram[2])
        .bind(hold_histogram[3])
        .bind(hold_histogram[4])
        .bind(hold_histogram[5])
        .bind(
            self.store
                .backend_time
                .now_ts()
                .saturating_add(continuation_secs),
        )
        .bind(self.store.backend_time.now_ts())
        .bind(&state.0)
        .bind(&state.1)
        .bind(&state.2)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 1 {
                return Ok(ReconciliationProjectionWriteStatus::CursorConflict);
            }
            sqlx::query(
            "UPDATE upstream_reconciliation_run_observation SET projection_state = 'projecting', cursor_advanced = 1, observed_at = ? WHERE id = 'local'",
        )
        .bind(self.store.backend_time.now_ts())
            .execute(&mut *tx)
            .await?;
            Ok(ReconciliationProjectionWriteStatus::Advanced)
        }
        .await;
        match write_result {
            Ok(ReconciliationProjectionWriteStatus::Advanced) => {
                tx.finish(Ok(())).await?;
                Ok(ReconciliationProjectionSliceOutcome::Advanced {
                    scanned_rows: rows.len() as i64,
                    completed: false,
                })
            }
            Ok(ReconciliationProjectionWriteStatus::StaleClaim) => {
                tx.rollback().await?;
                Ok(ReconciliationProjectionSliceOutcome::StaleClaim)
            }
            Ok(ReconciliationProjectionWriteStatus::CursorConflict) => {
                tx.rollback().await?;
                Ok(ReconciliationProjectionSliceOutcome::Deferred {
                    reason: "cursor_conflict",
                })
            }
            Err(err) => {
                tx.finish(Err(err)).await?;
                unreachable!("finishing a failed projection transaction returns the error")
            }
        }
    }

    async fn claim_is_current(
        tx: &mut SqliteImmediateTransaction,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<bool, ProxyError> {
        let Some((job_id, claim_generation)) = claimed_job else {
            return Ok(true);
        };
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS(SELECT 1 FROM scheduled_jobs WHERE id = ? AND status = 'running' AND claim_generation = ?)",
        )
        .bind(job_id)
        .bind(claim_generation)
        .fetch_one(&mut **tx)
        .await?
            != 0)
    }

    async fn defer_source_read_budget(
        &self,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<ReconciliationProjectionSliceOutcome, ProxyError> {
        match self
            .record_deferred_slice(claimed_job, "projection_read_budget")
            .await
        {
            Ok(true) => Ok(ReconciliationProjectionSliceOutcome::Deferred {
                reason: "projection_read_budget",
            }),
            Ok(false) => Ok(ReconciliationProjectionSliceOutcome::Advanced {
                scanned_rows: 0,
                completed: true,
            }),
            Err(err) if is_transient_sqlite_write_error(&err) => {
                Ok(ReconciliationProjectionSliceOutcome::Deferred {
                    reason: "projection_read_budget",
                })
            }
            Err(ProxyError::StaleClaim { .. }) => Ok(ReconciliationProjectionSliceOutcome::StaleClaim),
            Err(err) => Err(err),
        }
    }
}

fn projection_terminal_outcome(status: Option<&str>, delta_credits: Option<i64>) -> Option<String> {
    match status {
        Some("shadow_settled" | "shadow_degraded") if delta_credits == Some(0) => {
            Some("no_adjustment".to_string())
        }
        Some("shadow_settled" | "shadow_degraded") => Some("observed".to_string()),
        Some("settled" | "degraded") if delta_credits == Some(0) => {
            Some("no_adjustment".to_string())
        }
        Some("settled" | "degraded") => Some("settled".to_string()),
        _ => None,
    }
}

fn reconciliation_projection_hold_p95_ms(histogram: &[i64; 6]) -> i64 {
    let samples = histogram
        .iter()
        .fold(0_i64, |total, count| total.saturating_add(*count));
    if samples == 0 {
        return 0;
    }
    let target = samples.saturating_mul(95).saturating_add(99) / 100;
    let mut cumulative = 0_i64;
    for (index, count) in histogram.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= target {
            return RECONCILIATION_PROJECTION_HOLD_BUCKETS_MS[index];
        }
    }
    RECONCILIATION_PROJECTION_HOLD_BUCKETS_MS[RECONCILIATION_PROJECTION_HOLD_BUCKETS_MS.len() - 1]
}

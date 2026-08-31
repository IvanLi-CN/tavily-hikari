use crate::store::sqlite_runtime::ReconciliationReadKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpstreamReconciliationResearchCursor {
    pub(crate) next_poll_at: i64,
    pub(crate) key_id: String,
    pub(crate) request_id: String,
}

#[derive(Debug)]
pub(crate) struct UpstreamReconciliationResearchCandidatePage {
    pub(crate) candidates: Vec<crate::models::UpstreamReconciliationResearchCandidate>,
    pub(crate) cooled_due_count: i64,
    pub(crate) earliest_cooldown_until: Option<i64>,
    pub(crate) start_cursor: UpstreamReconciliationResearchCursor,
    pub(crate) candidate_cursors:
        std::collections::HashMap<String, UpstreamReconciliationResearchCursor>,
    pub(crate) next_cursor: Option<UpstreamReconciliationResearchCursor>,
    pub(crate) wrapped: bool,
}

impl UpstreamReconciliationResearchCandidatePage {
    pub(crate) fn empty() -> Self {
        Self {
            candidates: Vec::new(),
            cooled_due_count: 0,
            earliest_cooldown_until: None,
            start_cursor: UpstreamReconciliationResearchCursor {
                next_poll_at: -1,
                key_id: String::new(),
                request_id: String::new(),
            },
            candidate_cursors: std::collections::HashMap::new(),
            next_cursor: None,
            wrapped: false,
        }
    }
}

pub(crate) enum UpstreamReconciliationResearchDrainPoll<'a> {
    Terminal,
    Unavailable {
        error_kind: &'a str,
    },
    Pending {
        next_poll_at: i64,
        outcome: &'a str,
        error_kind: Option<&'a str>,
    },
}

pub(crate) struct UpstreamReconciliationResearchDrainCommit<'a> {
    pub(crate) request_id: &'a str,
    pub(crate) expected_cursor: &'a UpstreamReconciliationResearchCursor,
    pub(crate) accepted_cursor: &'a UpstreamReconciliationResearchCursor,
    pub(crate) wrapped: bool,
    pub(crate) poll: UpstreamReconciliationResearchDrainPoll<'a>,
    pub(crate) key_backoff: Option<ApiKeyTransientBackoffArm<'a>>,
    pub(crate) clear_key_backoff_scope: Option<&'a str>,
    pub(crate) job_id: i64,
    pub(crate) claim_generation: i64,
}

/// The drain publishes observations only after its durable state and scheduler handoff commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResearchDrainCommitReceipt {
    Accepted { next_at: i64 },
    Deferred { retry_at: i64 },
    StaleClaim,
}

impl KeyStore {
    pub(crate) async fn mark_upstream_reconciliation_research_unavailable(
        &self,
        request_id: &str,
        error_kind: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = self.begin_reconciliation_control().await?;
        sqlx::query(
            r#"
            UPDATE upstream_reconciliation_research
            SET last_polled_at = ?, next_poll_at = 0,
                poll_attempt_count = poll_attempt_count + 1,
                poll_resolution = 'unavailable', last_poll_outcome = 'unavailable',
                last_poll_error_kind = ?, updated_at = ?
            WHERE request_id = ? AND terminal_at IS NULL
            "#,
        )
        .bind(now)
        .bind(error_kind)
        .bind(now)
        .bind(request_id)
        .execute(&mut *transaction)
        .await?;
        transaction.finish(Ok(())).await
    }

    pub(crate) async fn next_upstream_reconciliation_research_candidates(
        &self,
        limit: i64,
    ) -> Result<UpstreamReconciliationResearchCandidatePage, ProxyError> {
        let now = self.backend_time.now_ts();
        let day_window =
            server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::ResearchCandidates)
            .await?;
        #[cfg(test)]
        if self.sqlite_runtime.take_reconciliation_research_read_failure_for_test() {
            let injected_result = Err::<Vec<(String, String, String, String, i64, i64)>, _>(
                sqlx::Error::PoolTimedOut,
            );
            return session.complete_query_or_defer(injected_result).await.map(|_| {
                UpstreamReconciliationResearchCandidatePage {
                    candidates: Vec::new(),
                    cooled_due_count: 0,
                    earliest_cooldown_until: None,
                    start_cursor: UpstreamReconciliationResearchCursor {
                        next_poll_at: -1,
                        key_id: String::new(),
                        request_id: String::new(),
                    },
                    candidate_cursors: std::collections::HashMap::new(),
                    next_cursor: None,
                    wrapped: false,
                }
            });
        }

        let cursor_result = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT cursor_next_poll_at, cursor_key_id, cursor_request_id, updated_at
             FROM upstream_reconciliation_research_scan_state WHERE id = 'local'",
        )
        .fetch_one(&mut *session)
        .await;
        let cursor = match cursor_result {
            Ok(cursor) => cursor,
            Err(error) => {
                return session
                    .complete_query_or_defer::<(i64, String, String, i64)>(Err(error))
                    .await
                    .map(|_| unreachable!("a failed cursor read cannot complete"));
            }
        };
        let start_cursor = UpstreamReconciliationResearchCursor {
            next_poll_at: cursor.0,
            key_id: cursor.1.clone(),
            request_id: cursor.2.clone(),
        };
        let page_limit = limit.clamp(1, 80);
        let mut raw_rows = Vec::new();
        let force_wrap = cursor.0 != -1 && now.saturating_sub(cursor.3) >= 300;
        let mut wrapped = force_wrap;
        for pass in 0..2 {
            let (cursor_next_poll_at, cursor_key_id, cursor_request_id) = if pass == 1 || force_wrap {
                wrapped = true;
                (-1_i64, String::new(), String::new())
            } else {
                (cursor.0, cursor.1.clone(), cursor.2.clone())
            };
            let raw_result = sqlx::query_as::<_, (String, String, String, String, i64, i64)>(
                r#"
                SELECT r.request_id, r.token_id, r.key_id, r.period_code,
                       r.next_poll_at, r.poll_attempt_count
                 FROM upstream_reconciliation_research r
                 WHERE r.terminal_at IS NULL
                   AND r.poll_resolution = 'pollable'
                   AND r.next_poll_at <= ?
                   AND EXISTS (
                       SELECT 1 FROM upstream_reconciliation_usage eligible_u
                        INDEXED BY idx_upstream_reconciliation_usage_window_mode
                        WHERE eligible_u.token_id = r.token_id
                          AND eligible_u.period_code = r.period_code
                          AND eligible_u.period_end <= ?
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM upstream_reconciliation_usage future_u
                        INDEXED BY idx_upstream_reconciliation_usage_window_mode
                        WHERE future_u.token_id = r.token_id
                          AND future_u.period_code = r.period_code
                          AND future_u.period_end > ?
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM api_key_transient_backoffs b
                        WHERE b.key_id = r.key_id
                          AND b.scope IN ('period_reconciliation', 'reconciliation_research_credentials')
                          AND b.cooldown_until > ?
                   )
                   AND (
                       r.next_poll_at > ?
                       OR (r.next_poll_at = ? AND r.key_id > ?)
                       OR (r.next_poll_at = ? AND r.key_id = ? AND r.request_id > ?)
                   )
                 ORDER BY r.next_poll_at, r.key_id, r.request_id
                 LIMIT ?
                "#,
            )
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(cursor_next_poll_at)
            .bind(cursor_next_poll_at)
            .bind(&cursor_key_id)
            .bind(cursor_next_poll_at)
            .bind(&cursor_key_id)
            .bind(&cursor_request_id)
            .bind(page_limit)
            .fetch_all(&mut *session)
            .await;
            raw_rows = match raw_result {
                Ok(rows) => rows,
                Err(error) => {
                    return session
                        .complete_query_or_defer::<Vec<(
                            String,
                            String,
                            String,
                            String,
                            i64,
                            i64,
                        )>>(Err(error))
                        .await
                        .map(|_| unreachable!("a failed research page read cannot complete"));
                }
            };
            if !raw_rows.is_empty() || pass == 1 || cursor.0 == -1 {
                break;
            }
        }
        let rows = if raw_rows.is_empty() {
            Vec::new()
        } else {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                r#"WITH hydrated AS (
                    SELECT r.request_id, r.token_id, r.key_id, r.period_code,
                        MIN(u.billing_subject) AS billing_subject, MAX(u.period_end) AS period_end,
                        r.poll_attempt_count
                      FROM upstream_reconciliation_research r
                      JOIN upstream_reconciliation_usage u
                        INDEXED BY idx_upstream_reconciliation_usage_window_mode
                        ON u.token_id = r.token_id AND u.period_code = r.period_code
                  WHERE r.terminal_at IS NULL AND r.poll_resolution = 'pollable' AND r.request_id IN ("#,
            );
            {
                let mut separated = query.separated(", ");
                for (request_id, _, _, _, _, _) in &raw_rows {
                    separated.push_bind(request_id);
                }
            }
            query.push(" ) GROUP BY r.request_id HAVING MAX(u.period_end) <= ");
            query.push_bind(now);
            query.push(
                r#")
                SELECT h.request_id, h.token_id, h.key_id, h.period_code,
                       h.billing_subject, h.period_end, h.poll_attempt_count,
                       CASE WHEN EXISTS (
                          SELECT 1 FROM upstream_reconciliation_usage prior_u
                            INDEXED BY idx_upstream_reconciliation_usage_subject_mode_period
                          JOIN upstream_reconciliation_settlements prior_s
                            ON prior_s.settlement_key = 'v1:' || prior_u.token_id || ':' || prior_u.period_code
                         WHERE prior_u.billing_subject = h.billing_subject
                           AND prior_u.period_start >= "#,
            );
            query.push_bind(day_window.start);
            query.push(" AND prior_u.period_start < ");
            query.push_bind(day_window.end);
            query.push(" AND prior_s.status = 'shadow_settled') THEN 1 ELSE 0 END AS has_settled_period FROM hydrated h ORDER BY has_settled_period, h.period_end DESC, h.request_id");
            let hydrated_result = query
                .build_query_as::<(
                    String,
                    String,
                    String,
                    String,
                    String,
                    i64,
                    i64,
                    i64,
                )>()
                .fetch_all(&mut *session)
                .await;
            match hydrated_result {
                Ok(rows) => rows,
                Err(error) => {
                    return session
                        .complete_query_or_defer::<Vec<(
                            String,
                            String,
                            String,
                            String,
                            String,
                            i64,
                            i64,
                            i64,
                        )>>(Err(error))
                        .await
                        .map(|_| unreachable!("a failed research hydrate cannot complete"));
                }
            }
        };
        let cooled_due_result = if raw_rows.is_empty() {
            Some(sqlx::query_as::<_, (Option<i64>, i64)>(
                "SELECT MIN(c.key_cooldown_until), COUNT(*)
                   FROM (
                       SELECT b.key_id, MAX(b.cooldown_until) AS key_cooldown_until
                         FROM api_key_transient_backoffs b
                        WHERE b.scope IN ('period_reconciliation', 'reconciliation_research_credentials')
                          AND b.cooldown_until > ?
                        GROUP BY b.key_id
                   ) c
                  WHERE EXISTS (
                      SELECT 1 FROM upstream_reconciliation_research r
                       WHERE r.key_id = c.key_id AND r.terminal_at IS NULL AND r.poll_resolution = 'pollable'
                         AND r.next_poll_at <= ?
                         AND EXISTS (SELECT 1 FROM upstream_reconciliation_usage eligible_u
                                     INDEXED BY idx_upstream_reconciliation_usage_window_mode
                                     WHERE eligible_u.token_id = r.token_id
                                       AND eligible_u.period_code = r.period_code
                                       AND eligible_u.period_end <= ?)
                         AND NOT EXISTS (SELECT 1 FROM upstream_reconciliation_usage future_u
                                         INDEXED BY idx_upstream_reconciliation_usage_window_mode
                                         WHERE future_u.token_id = r.token_id
                                           AND future_u.period_code = r.period_code
                                           AND future_u.period_end > ?)
                       LIMIT 1)
                  ",
            )
                .bind(now)
                .bind(now)
                .bind(now)
                .bind(now)
                .fetch_one(&mut *session)
                .await)
        } else {
            None
        };
        let (earliest_cooldown_until, cooled_due_count) = match cooled_due_result {
            Some(Ok(value)) => value,
            Some(Err(error)) => {
                return session
                    .complete_query_or_defer::<i64>(Err(error))
                    .await
                    .map(|_| unreachable!("a failed cooldown count cannot complete"));
            }
            None => (None, 0),
        };
        let mut selected_per_key = std::collections::HashMap::<String, usize>::new();
        let mut candidates = rows
            .into_iter()
            .filter_map(
                |(
                    request_id,
                    token_id,
                    key_id,
                    period_code,
                    billing_subject,
                    period_end,
                    poll_attempt_count,
                    has_settled_period,
                )| {
                    let slot = selected_per_key.entry(key_id.clone()).or_default();
                    if *slot >= 4 || has_settled_period > 1 {
                        return None;
                    }
                    *slot += 1;
                    Some(crate::models::UpstreamReconciliationResearchCandidate {
                        request_id,
                        token_id,
                        key_id,
                        period_code,
                        billing_subject,
                        period_end,
                        poll_attempt_count,
                    })
                },
            )
            .collect::<Vec<_>>();
        candidates.truncate(page_limit as usize);
        let candidate_cursors = raw_rows
            .iter()
            .map(|(request_id, _, key_id, _, next_poll_at, _)| {
                (
                    request_id.clone(),
                    UpstreamReconciliationResearchCursor {
                        next_poll_at: *next_poll_at,
                        key_id: key_id.clone(),
                        request_id: request_id.clone(),
                    },
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let next_cursor = raw_rows.last().map(
            |(_, _, key_id, _, next_poll_at, _)| UpstreamReconciliationResearchCursor {
                next_poll_at: *next_poll_at,
                key_id: key_id.clone(),
                request_id: raw_rows
                    .last()
                    .map(|(request_id, _, _, _, _, _)| request_id.clone())
                    .unwrap_or_default(),
            },
        );
        let result = session
            .complete_query_or_defer(Ok::<_, sqlx::Error>((
                candidates,
                candidate_cursors,
                next_cursor,
                wrapped,
                cooled_due_count,
                earliest_cooldown_until,
            )))
            .await?;
        Ok(UpstreamReconciliationResearchCandidatePage {
            candidates: result.0,
            cooled_due_count: result.4,
            earliest_cooldown_until: result.5,
            start_cursor,
            candidate_cursors: result.1,
            next_cursor: result.2,
            wrapped: result.3,
        })
    }

    pub(crate) async fn commit_upstream_reconciliation_research_drain(
        &self,
        commit: UpstreamReconciliationResearchDrainCommit<'_>,
    ) -> Result<ResearchDrainCommitReceipt, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut transaction = self.begin_reconciliation_control().await?;
        let claimed_job = Some((commit.job_id, commit.claim_generation));
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            transaction.rollback().await?;
            return Ok(ResearchDrainCommitReceipt::StaleClaim);
        }
        let cursor_matches: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM upstream_reconciliation_research_scan_state \
             WHERE id = 'local' AND cursor_next_poll_at = ? AND cursor_key_id = ? \
             AND cursor_request_id = ?)",
        )
        .bind(commit.expected_cursor.next_poll_at)
        .bind(&commit.expected_cursor.key_id)
        .bind(&commit.expected_cursor.request_id)
        .fetch_one(&mut *transaction)
        .await?;
        if cursor_matches == 0 {
            Self::finish_research_drain_claim_locked(
                &mut transaction,
                commit.job_id,
                commit.claim_generation,
                "deferred=research_drain_budget",
                now.saturating_add(30),
                now,
            )
            .await?;
            transaction.finish(Ok(())).await?;
            return Ok(ResearchDrainCommitReceipt::Deferred {
                retry_at: now.saturating_add(30),
            });
        }

        let changed = match commit.poll {
            UpstreamReconciliationResearchDrainPoll::Terminal => {
                let changed = sqlx::query(
                    "UPDATE upstream_reconciliation_research SET terminal_at = ?, \
                     last_polled_at = ?, next_poll_at = 0, \
                     poll_attempt_count = poll_attempt_count + 1, \
                     poll_resolution = 'pollable', last_poll_outcome = 'terminal', last_poll_error_kind = NULL, updated_at = ? \
                     WHERE request_id = ? AND terminal_at IS NULL",
                )
                .bind(now)
                .bind(now)
                .bind(now)
                .bind(commit.request_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected();
                if changed > 0 {
                    sqlx::query(
                        "INSERT INTO meta (key, value) VALUES (?, ?) \
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    )
                    .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_TERMINAL_AT_V1)
                    .bind(now.to_string())
                    .execute(&mut *transaction)
                        .await?;
                }
                changed
            }
            UpstreamReconciliationResearchDrainPoll::Pending {
                next_poll_at,
                outcome,
                error_kind,
            } => {
                sqlx::query(
                    "UPDATE upstream_reconciliation_research SET last_polled_at = ?, \
                     next_poll_at = ?, poll_attempt_count = poll_attempt_count + 1, \
                     poll_resolution = 'pollable', last_poll_outcome = ?, last_poll_error_kind = ?, updated_at = ? \
                     WHERE request_id = ? AND terminal_at IS NULL",
                )
                .bind(now)
                .bind(next_poll_at)
                .bind(outcome)
                .bind(error_kind)
                .bind(now)
                .bind(commit.request_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected()
            }
            UpstreamReconciliationResearchDrainPoll::Unavailable { error_kind } => {
                sqlx::query(
                    "UPDATE upstream_reconciliation_research SET last_polled_at = ?, \
                     next_poll_at = 0, poll_attempt_count = poll_attempt_count + 1, \
                     poll_resolution = 'unavailable', last_poll_outcome = 'unavailable', \
                     last_poll_error_kind = ?, updated_at = ? \
                     WHERE request_id = ? AND terminal_at IS NULL",
                )
                .bind(now)
                .bind(error_kind)
                .bind(now)
                .bind(commit.request_id)
                .execute(&mut *transaction)
                .await?
                .rows_affected()
            }
        };
        if changed == 0 {
            // The selected row resolved between the read session and this claim-fenced
            // transaction. Do not advance the cursor; let the next drain retry selection.
            Self::finish_research_drain_claim_locked(
                &mut transaction,
                commit.job_id,
                commit.claim_generation,
                "deferred=research_drain_budget",
                now.saturating_add(30),
                now,
            )
            .await?;
            transaction.finish(Ok(())).await?;
            return Ok(ResearchDrainCommitReceipt::Deferred {
                retry_at: now.saturating_add(30),
            });
        }

        if let Some(arm) = commit.key_backoff {
            sqlx::query(
                "INSERT INTO api_key_transient_backoffs (key_id, scope, cooldown_until, \
                 retry_after_secs, reason_code, source_request_log_id, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(key_id, scope) DO UPDATE SET \
                 cooldown_until = MAX(api_key_transient_backoffs.cooldown_until, excluded.cooldown_until), \
                 retry_after_secs = CASE WHEN excluded.cooldown_until >= api_key_transient_backoffs.cooldown_until \
                 THEN excluded.retry_after_secs ELSE api_key_transient_backoffs.retry_after_secs END, \
                 reason_code = CASE WHEN excluded.cooldown_until >= api_key_transient_backoffs.cooldown_until \
                 THEN COALESCE(excluded.reason_code, api_key_transient_backoffs.reason_code) \
                 ELSE api_key_transient_backoffs.reason_code END, updated_at = MAX(api_key_transient_backoffs.updated_at, excluded.updated_at)",
            )
            .bind(arm.key_id)
            .bind(arm.scope)
            .bind(arm.cooldown_until)
            .bind(arm.retry_after_secs)
            .bind(arm.reason_code)
            .bind(arm.source_request_log_id)
            .bind(arm.now)
            .bind(arm.now)
            .execute(&mut *transaction)
            .await?;
        }
        if let Some(scope) = commit.clear_key_backoff_scope {
            sqlx::query(
                "DELETE FROM api_key_transient_backoffs WHERE key_id = (SELECT key_id FROM upstream_reconciliation_research WHERE request_id = ?) AND scope = ?",
            )
            .bind(commit.request_id)
            .bind(scope)
            .execute(&mut *transaction)
            .await?;
        }

        let wrapped = commit.wrapped
            || commit.expected_cursor.next_poll_at == -1
            || (
                commit.accepted_cursor.next_poll_at,
                commit.accepted_cursor.key_id.as_str(),
                commit.accepted_cursor.request_id.as_str(),
            ) < (
                commit.expected_cursor.next_poll_at,
                commit.expected_cursor.key_id.as_str(),
                commit.expected_cursor.request_id.as_str(),
            );
        sqlx::query(
            "UPDATE upstream_reconciliation_research_scan_state SET cursor_next_poll_at = ?, \
             cursor_key_id = ?, cursor_request_id = ?, \
             updated_at = CASE WHEN ? THEN ? ELSE updated_at END WHERE id = 'local'",
        )
        .bind(commit.accepted_cursor.next_poll_at)
        .bind(&commit.accepted_cursor.key_id)
        .bind(&commit.accepted_cursor.request_id)
        .bind(wrapped)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO meta (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1)
        .bind(now.to_string())
        .execute(&mut *transaction)
        .await?;
        self.record_reconciliation_research_progress_window_locked(&mut transaction, now)
            .await?;
        let next_at = now.saturating_add(5);
        Self::finish_research_drain_claim_locked(
            &mut transaction,
            commit.job_id,
            commit.claim_generation,
            "poll_persisted",
            next_at,
            now,
        )
        .await?;
        transaction.finish(Ok(())).await?;
        Ok(ResearchDrainCommitReceipt::Accepted { next_at })
    }

    async fn finish_research_drain_claim_locked(
        transaction: &mut sqlx::SqliteConnection,
        job_id: i64,
        claim_generation: i64,
        message: &str,
        available_at: i64,
        now: i64,
    ) -> Result<(), ProxyError> {
        let updated = sqlx::query(
            r#"UPDATE scheduled_jobs
                   SET status = 'success', message = ?, finished_at = ?
                 WHERE id = ? AND status = 'running' AND claim_generation = ?"#,
        )
        .bind(message)
        .bind(now)
        .bind(job_id)
        .bind(claim_generation)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }

        if let Some((continuation_id, status, _)) = Self::scheduled_job_lookup_active_locked(
            transaction,
            "upstream_reconciliation_research_drain",
            None,
        )
        .await?
        {
            if status == "queued" {
                sqlx::query(
                    "UPDATE scheduled_jobs SET available_at = MIN(available_at, ?) WHERE id = ?",
                )
                .bind(available_at)
                .bind(continuation_id)
                .execute(&mut *transaction)
                .await?;
            }
            return Ok(());
        }

        sqlx::query(
            r#"INSERT INTO scheduled_jobs (
                    job_type, trigger_source, key_id, status, attempt, queued_at,
                    available_at, started_at, finished_at
                ) VALUES ('upstream_reconciliation_research_drain', 'auto', NULL, 'queued', 1, ?, ?, NULL, NULL)"#,
        )
        .bind(now)
        .bind(available_at)
        .execute(&mut *transaction)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn accept_upstream_reconciliation_research_cursor(
        &self,
        cursor: Option<&UpstreamReconciliationResearchCursor>,
        _wrapped: bool,
        claimed_job: Option<(i64, i64)>,
    ) -> Result<(), ProxyError> {
        self.accept_upstream_reconciliation_research_page(cursor, _wrapped, claimed_job, false)
            .await
    }

    pub(crate) async fn accept_upstream_reconciliation_research_page(
        &self,
        cursor: Option<&UpstreamReconciliationResearchCursor>,
        _wrapped: bool,
        claimed_job: Option<(i64, i64)>,
        mark_sweep_completed: bool,
    ) -> Result<(), ProxyError> {
        let mut transaction = self.begin_reconciliation_control().await?;
        if !Self::reconciliation_claim_is_current_locked(&mut transaction, claimed_job).await? {
            let (job_id, claim_generation) = claimed_job.expect("claimed job was checked");
            transaction.rollback().await?;
            return Err(ProxyError::StaleClaim {
                job_id,
                claim_generation,
            });
        }
        let (next_poll_at, key_id, request_id) = cursor
            .map(|value| (value.next_poll_at, value.key_id.as_str(), value.request_id.as_str()))
            .unwrap_or((-1, "", ""));
        sqlx::query(
            "UPDATE upstream_reconciliation_research_scan_state
                SET cursor_next_poll_at = ?, cursor_key_id = ?, cursor_request_id = ?, updated_at = ?
              WHERE id = 'local'",
        )
        .bind(next_poll_at)
        .bind(key_id)
        .bind(request_id)
        .bind(self.backend_time.now_ts())
        .execute(&mut *transaction)
        .await?;
        if mark_sweep_completed {
            sqlx::query(
                "INSERT INTO meta (key, value) VALUES (?, ?) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(META_KEY_UPSTREAM_RECONCILIATION_LAST_RESEARCH_SWEEP_AT_V1)
            .bind(self.backend_time.now_ts().to_string())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.finish(Ok(())).await
    }

    pub(crate) async fn has_due_upstream_reconciliation_research(&self) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut session = self
            .sqlite_runtime
            .begin_reconciliation_read(ReconciliationReadKind::ResearchCandidates)
            .await?;
        #[cfg(test)]
        if self.sqlite_runtime.take_reconciliation_research_read_failure_for_test() {
            let injected_result = Err::<i64, _>(sqlx::Error::PoolTimedOut);
            return session.complete_query_or_defer(injected_result).await.map(|_| false);
        }
        let due_result = sqlx::query_scalar::<_, i64>(r#"
            SELECT EXISTS(
                SELECT 1
                 FROM upstream_reconciliation_research r
                 WHERE r.terminal_at IS NULL
                   AND r.poll_resolution = 'pollable'
                   AND r.next_poll_at <= ?
                   AND EXISTS (
                       SELECT 1
                         FROM upstream_reconciliation_usage u
                        WHERE u.token_id = r.token_id
                          AND u.period_code = r.period_code
                          AND u.period_end <= ?
                   )
                 ORDER BY r.next_poll_at, r.key_id, r.request_id
                 LIMIT 1
            )
            "#)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *session)
        .await;
        Ok(session.complete_query_or_defer(due_result).await? != 0)
    }
}

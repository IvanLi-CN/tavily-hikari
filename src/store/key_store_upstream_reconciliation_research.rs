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
    pub(crate) next_cursor: Option<UpstreamReconciliationResearchCursor>,
    pub(crate) wrapped: bool,
}

impl UpstreamReconciliationResearchCandidatePage {
    pub(crate) fn empty() -> Self {
        Self {
            candidates: Vec::new(),
            next_cursor: None,
            wrapped: false,
        }
    }
}

impl KeyStore {
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
                    next_cursor: None,
                    wrapped: false,
                }
            });
        }

        let cursor_result = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT cursor_next_poll_at, cursor_key_id, cursor_request_id
             FROM upstream_reconciliation_research_scan_state WHERE id = 'local'",
        )
        .fetch_one(&mut *session)
        .await;
        let cursor = match cursor_result {
            Ok(cursor) => cursor,
            Err(error) => {
                return session
                    .complete_query_or_defer::<(i64, String, String)>(Err(error))
                    .await
                    .map(|_| unreachable!("a failed cursor read cannot complete"));
            }
        };
        let page_limit = limit.clamp(1, 80);
        let mut raw_rows = Vec::new();
        let mut wrapped = false;
        for pass in 0..2 {
            let (cursor_next_poll_at, cursor_key_id, cursor_request_id) = if pass == 1 {
                wrapped = true;
                (-1_i64, String::new(), String::new())
            } else {
                cursor.clone()
            };
            let raw_result = sqlx::query_as::<_, (String, String, String, String, i64, i64)>(
                r#"
                SELECT r.request_id, r.token_id, r.key_id, r.period_code,
                       r.next_poll_at, r.poll_attempt_count
                  FROM upstream_reconciliation_research r
                 WHERE r.terminal_at IS NULL
                   AND r.next_poll_at <= ?
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
                        ON u.token_id = r.token_id AND u.period_code = r.period_code
                     WHERE r.terminal_at IS NULL AND r.request_id IN ("#,
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
            .complete_query_or_defer(Ok::<_, sqlx::Error>((candidates, next_cursor, wrapped)))
            .await?;
        Ok(UpstreamReconciliationResearchCandidatePage {
            candidates: result.0,
            next_cursor: result.1,
            wrapped: result.2,
        })
    }

    pub(crate) async fn accept_upstream_reconciliation_research_cursor(
        &self,
        cursor: Option<&UpstreamReconciliationResearchCursor>,
        _wrapped: bool,
        claimed_job: Option<(i64, i64)>,
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

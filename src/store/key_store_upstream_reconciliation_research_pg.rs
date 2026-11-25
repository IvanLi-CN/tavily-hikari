/// PostgreSQL branch of the reconciliation research selector.
///
/// The SQLite implementation stays in `key_store_upstream_reconciliation_research.rs` and keeps its
/// cooperative read session and defer mapping. This twin runs the same selection on PostgreSQL
/// without the SQLite `INDEXED BY` planner hints.
impl KeyStore {
    /// Pure assembly of a research candidate page from already-read rows. Both dialects share it:
    /// per-key selection (at most four per key, settled periods last), page truncation, and the
    /// cursor derivation are dialect independent.
    #[allow(clippy::type_complexity)]
    fn assemble_research_candidate_page(
        page_limit: i64,
        start_cursor: UpstreamReconciliationResearchCursor,
        raw_rows: &[(String, String, String, String, i64, i64)],
        rows: Vec<(String, String, String, String, String, i64, i64, i64)>,
        cooled_due_count: i64,
        earliest_cooldown_until: Option<i64>,
        wrapped: bool,
    ) -> UpstreamReconciliationResearchCandidatePage {
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
        let next_cursor = raw_rows.last().map(|(_, _, key_id, _, next_poll_at, _)| {
            UpstreamReconciliationResearchCursor {
                next_poll_at: *next_poll_at,
                key_id: key_id.clone(),
                request_id: raw_rows
                    .last()
                    .map(|(request_id, _, _, _, _, _)| request_id.clone())
                    .unwrap_or_default(),
            }
        });
        UpstreamReconciliationResearchCandidatePage {
            candidates,
            cooled_due_count,
            earliest_cooldown_until,
            start_cursor,
            candidate_cursors,
            next_cursor,
            wrapped,
        }
    }
}

impl KeyStore {
    /// PostgreSQL twin of [`Self::next_upstream_reconciliation_research_candidates`]. The query
    /// text mirrors the SQLite selector without its `INDEXED BY` planner hints, which PostgreSQL
    /// does not accept; relation names carry the `core` schema.
    async fn next_upstream_reconciliation_research_candidates_pg(
        &self,
        limit: i64,
    ) -> Result<UpstreamReconciliationResearchCandidatePage, ProxyError> {
        let now = self.backend_time.now_ts();
        let day_window =
            server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let mut session = self.begin_pg_read_session().await?;
        let cursor = sqlx::query_as::<_, (i64, String, String, i64)>(
            "SELECT cursor_next_poll_at, cursor_key_id, cursor_request_id, updated_at
             FROM core.upstream_reconciliation_research_scan_state WHERE id = 'local'",
        )
        .fetch_one(&mut *session.connection())
        .await?;
        let start_cursor = UpstreamReconciliationResearchCursor {
            next_poll_at: cursor.0,
            key_id: cursor.1.clone(),
            request_id: cursor.2.clone(),
        };
        let page_limit = limit.clamp(1, 80);
        let mut raw_rows: Vec<(String, String, String, String, i64, i64)> = Vec::new();
        let force_wrap = cursor.0 != -1 && now.saturating_sub(cursor.3) >= 300;
        let mut wrapped = force_wrap;
        for pass in 0..2 {
            let (cursor_next_poll_at, cursor_key_id, cursor_request_id) =
                if pass == 1 || force_wrap {
                    wrapped = true;
                    (-1_i64, String::new(), String::new())
                } else {
                    (cursor.0, cursor.1.clone(), cursor.2.clone())
                };
            raw_rows = sqlx::query_as::<_, (String, String, String, String, i64, i64)>(
                r#"
                SELECT r.request_id, r.token_id, r.key_id, r.period_code,
                       r.next_poll_at, r.poll_attempt_count
                 FROM core.upstream_reconciliation_research r
                 WHERE r.terminal_at IS NULL
                   AND r.poll_resolution = 'pollable'
                   AND r.next_poll_at <= $1
                   AND EXISTS (
                       SELECT 1 FROM core.upstream_reconciliation_usage eligible_u
                        WHERE eligible_u.token_id = r.token_id
                          AND eligible_u.period_code = r.period_code
                          AND eligible_u.period_end <= $2
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM core.upstream_reconciliation_usage future_u
                        WHERE future_u.token_id = r.token_id
                          AND future_u.period_code = r.period_code
                          AND future_u.period_end > $3
                   )
                   AND NOT EXISTS (
                       SELECT 1 FROM core.api_key_transient_backoffs b
                        WHERE b.key_id = r.key_id
                          AND b.scope IN ('period_reconciliation', 'reconciliation_research_credentials')
                          AND b.cooldown_until > $4
                   )
                   AND (
                       r.next_poll_at > $5
                       OR (r.next_poll_at = $5 AND r.key_id > $6)
                       OR (r.next_poll_at = $5 AND r.key_id = $6 AND r.request_id > $7)
                   )
                 ORDER BY r.next_poll_at, r.key_id, r.request_id
                 LIMIT $8
                "#,
            )
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(cursor_next_poll_at)
            .bind(&cursor_key_id)
            .bind(&cursor_request_id)
            .bind(page_limit)
            .fetch_all(&mut *session.connection())
            .await?;
            if !raw_rows.is_empty() || pass == 1 || cursor.0 == -1 {
                break;
            }
        }
        let rows = if raw_rows.is_empty() {
            Vec::new()
        } else {
            let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                r#"WITH hydrated AS (
                    SELECT r.request_id, r.token_id, r.key_id, r.period_code,
                        MIN(u.billing_subject) AS billing_subject, MAX(u.period_end) AS period_end,
                        r.poll_attempt_count
                      FROM core.upstream_reconciliation_research r
                      JOIN core.upstream_reconciliation_usage u
                        ON u.token_id = r.token_id AND u.period_code = r.period_code
                  WHERE r.terminal_at IS NULL AND r.poll_resolution = 'pollable' AND r.request_id IN ("#,
            );
            {
                let mut separated = query.separated(", ");
                for (request_id, _, _, _, _, _) in &raw_rows {
                    separated.push_bind(request_id);
                }
            }
            query.push(" ) GROUP BY r.request_id, r.token_id, r.key_id, r.period_code, r.poll_attempt_count HAVING MAX(u.period_end) <= ");
            query.push_bind(now);
            query.push(
                r#")
                SELECT h.request_id, h.token_id, h.key_id, h.period_code,
                       h.billing_subject, h.period_end, h.poll_attempt_count,
                       CAST(CASE WHEN EXISTS (
                          SELECT 1 FROM core.upstream_reconciliation_usage prior_u
                          JOIN core.upstream_reconciliation_settlements prior_s
                            ON prior_s.settlement_key = 'v1:' || prior_u.token_id || ':' || prior_u.period_code
                         WHERE prior_u.billing_subject = h.billing_subject
                           AND prior_u.period_start >= "#,
            );
            query.push_bind(day_window.start);
            query.push(" AND prior_u.period_start < ");
            query.push_bind(day_window.end);
            query.push(" AND prior_s.status = 'shadow_settled') THEN 1 ELSE 0 END AS BIGINT) AS has_settled_period FROM hydrated h ORDER BY has_settled_period, h.period_end DESC, h.request_id");
            query
                .build_query_as::<(String, String, String, String, String, i64, i64, i64)>()
                .fetch_all(&mut *session.connection())
                .await?
        };
        let (earliest_cooldown_until, cooled_due_count) = if raw_rows.is_empty() {
            sqlx::query_as::<_, (Option<i64>, i64)>(
                "SELECT MIN(c.key_cooldown_until), COUNT(*)::bigint
                   FROM (
                       SELECT b.key_id, MAX(b.cooldown_until) AS key_cooldown_until
                         FROM core.api_key_transient_backoffs b
                        WHERE b.scope IN ('period_reconciliation', 'reconciliation_research_credentials')
                          AND b.cooldown_until > $1
                        GROUP BY b.key_id
                   ) c
                  WHERE EXISTS (
                      SELECT 1 FROM core.upstream_reconciliation_research r
                       WHERE r.key_id = c.key_id AND r.terminal_at IS NULL AND r.poll_resolution = 'pollable'
                         AND r.next_poll_at <= $2
                         AND EXISTS (SELECT 1 FROM core.upstream_reconciliation_usage eligible_u
                                     WHERE eligible_u.token_id = r.token_id
                                       AND eligible_u.period_code = r.period_code
                                       AND eligible_u.period_end <= $3)
                         AND NOT EXISTS (SELECT 1 FROM core.upstream_reconciliation_usage future_u
                                         WHERE future_u.token_id = r.token_id
                                           AND future_u.period_code = r.period_code
                                           AND future_u.period_end > $4)
                       LIMIT 1)",
            )
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(now)
            .fetch_one(&mut *session.connection())
            .await?
        } else {
            (None, 0)
        };
        session.finish().await?;
        Ok(Self::assemble_research_candidate_page(
            page_limit,
            start_cursor,
            &raw_rows,
            rows,
            cooled_due_count,
            earliest_cooldown_until,
            wrapped,
        ))
    }
}

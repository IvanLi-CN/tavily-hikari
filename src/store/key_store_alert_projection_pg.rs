/// PostgreSQL branches for the alert projection readers.
///
/// The projection is one logical transaction unit: the slice advancer reads source state, reads
/// source rows, writes projected rows, and bumps the generation. Every one of those steps must run
/// against the same database, so this file only holds the reader twins that the advancer calls; the
/// writer twin lands together with them.
use crate::models::ProxyError;

impl KeyStore {
    async fn alert_projection_source_state_pg(
        &self,
        lane: AlertProjectionLane,
    ) -> Result<Option<AlertProjectionSourceState>, ProxyError> {
        let query = match lane {
            AlertProjectionLane::RecentTail => {
                r#"SELECT source_kind, cursor_occurred_at, cursor_row_sort_id,
                          fence_occurred_at, fence_row_sort_id, generation, phase
                     FROM observability.dashboard_alert_projection_state
                    ORDER BY CASE WHEN phase = 'catching_up' THEN 0 ELSE 1 END,
                             generation ASC, source_kind ASC
                    LIMIT 1"#
            }
            AlertProjectionLane::History => {
                r#"SELECT source_kind, cursor_occurred_at, cursor_row_sort_id,
                          fence_occurred_at, fence_row_sort_id, generation, phase
                     FROM observability.dashboard_alert_projection_history_state
                    WHERE phase = 'catching_up'
                    ORDER BY generation ASC, source_kind ASC
                    LIMIT 1"#
            }
        };
        let row = sqlx::query_as::<_, (String, i64, String, Option<i64>, Option<String>, i64, String)>(
            query,
        )
        .fetch_optional(self.pg()?)
        .await?;
        Ok(row.map(
            |(
                source_kind,
                cursor_occurred_at,
                cursor_row_sort_id,
                fence_occurred_at,
                fence_row_sort_id,
                generation,
                phase,
            )| AlertProjectionSourceState {
                source_kind,
                cursor_occurred_at,
                cursor_row_sort_id,
                fence_occurred_at,
                fence_row_sort_id,
                generation,
                phase,
                lane,
            },
        ))
    }

    async fn alert_projection_source_state_for_kind_pg(
        &self,
        lane: AlertProjectionLane,
        source_kind: &str,
    ) -> Result<Option<AlertProjectionSourceState>, ProxyError> {
        let table = match lane {
            AlertProjectionLane::RecentTail => "observability.dashboard_alert_projection_state",
            AlertProjectionLane::History => {
                "observability.dashboard_alert_projection_history_state"
            }
        };
        let row = sqlx::query_as::<_, (String, i64, String, Option<i64>, Option<String>, i64, String)>(
            &format!(
                "SELECT source_kind, cursor_occurred_at, cursor_row_sort_id, \
                        fence_occurred_at, fence_row_sort_id, generation, phase \
                   FROM {table} WHERE source_kind = $1"
            ),
        )
        .bind(source_kind)
        .fetch_optional(self.pg()?)
        .await?;
        Ok(row.map(
            |(
                source_kind,
                cursor_occurred_at,
                cursor_row_sort_id,
                fence_occurred_at,
                fence_row_sort_id,
                generation,
                phase,
            )| AlertProjectionSourceState {
                source_kind,
                cursor_occurred_at,
                cursor_row_sort_id,
                fence_occurred_at,
                fence_row_sort_id,
                generation,
                phase,
                lane,
            },
        ))
    }

    async fn alert_projection_source_fence_pg(
        &self,
        source_kind: &str,
    ) -> Result<Option<(i64, String)>, ProxyError> {
        let retention_since = self.alert_projection_retention_since();
        let row = match source_kind {
            ALERT_SOURCE_AUTH_TOKEN_LOG => sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT created_at, id
                     FROM core.auth_token_logs
                    WHERE (failure_kind = 'upstream_rate_limited_429'
                           OR result_status = 'quota_exhausted')
                      AND created_at >= $1
                    ORDER BY created_at DESC, id DESC
                    LIMIT 1"#,
            )
            .bind(retention_since)
            .fetch_optional(self.pg()?)
            .await?
            .map(|(occurred_at, id)| (occurred_at, format!("atl:{id:020}"))),
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD => sqlx::query_as::<_, (i64, String)>(
                r#"SELECT created_at, id
                     FROM core.api_key_maintenance_records
                    WHERE (COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                           OR (source = 'system'
                               AND operation_code = 'auto_mark_exhausted'
                               AND reason_code = 'quota_exhausted'))
                      AND created_at >= $1
                    ORDER BY created_at DESC, id DESC
                    LIMIT 1"#,
            )
            .bind(retention_since)
            .fetch_optional(self.pg()?)
            .await?
            .map(|(occurred_at, source_id)| (occurred_at, format!("maint:{source_id}"))),
            ALERT_SOURCE_SCHEDULED_JOB => sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                     FROM core.scheduled_jobs
                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                      AND COALESCE(finished_at, started_at, queued_at) >= $1
                    ORDER BY COALESCE(finished_at, started_at, queued_at) DESC, id DESC
                    LIMIT 1"#,
            )
            .bind(retention_since)
            .fetch_optional(self.pg()?)
            .await?
            .map(|(occurred_at, id)| (occurred_at, format!("job:{id:020}"))),
            other => {
                return Err(ProxyError::Other(format!(
                    "unknown alert projection source: {other}"
                )));
            }
        };
        Ok(row)
    }

    async fn prune_expired_alert_projection_slice_pg(&self) -> Result<bool, ProxyError> {
        let retention_since = self.alert_projection_retention_since();
        let observed_at = self.backend_time.now_ts();
        let mut tx = self.begin_pg_write().await?;
        // PostgreSQL has no `rowid`; the projected rows are keyed by
        // `(source_kind, source_id)`, which is a stable and unique identity here.
        let deleted = sqlx::query(
            r#"DELETE FROM observability.dashboard_alert_projection_events
                WHERE (source_kind, source_id) IN (
                    SELECT source_kind, source_id
                      FROM observability.dashboard_alert_projection_events
                     WHERE occurred_at < $1
                     ORDER BY occurred_at ASC, source_kind ASC, source_id ASC
                     LIMIT $2
                )"#,
        )
        .bind(retention_since)
        .bind(ALERT_PROJECTION_BATCH_ROWS)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if deleted > 0 {
            sqlx::query(
                "UPDATE observability.dashboard_alert_projection_revision_state \
                 SET revision = revision + 1 WHERE singleton = 1",
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE observability.dashboard_alert_projection_state \
                 SET generation = generation + 1, observed_at = $1, stale_reason = 'retention_pruned'",
            )
            .bind(observed_at)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE observability.dashboard_alert_projection_history_state \
                 SET generation = generation + 1",
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(deleted > 0)
    }
}

impl KeyStore {
    /// PostgreSQL twin of [`Self::alert_projection_source_keys`]. The candidate ranges are the same
    /// three intervals; the predicates only change dialect.
    async fn alert_projection_source_keys_pg(
        &self,
        source_kind: &str,
        cursor: (i64, &str),
        fence: (i64, &str),
    ) -> Result<Vec<AlertProjectionSourceKey>, ProxyError> {
        let cursor_id = Self::alert_projection_source_cursor_id(source_kind, cursor.1);
        let fence_id = Self::alert_projection_source_cursor_id(source_kind, fence.1);
        let retention_since = self.alert_projection_retention_since();
        let ranges = if cursor.0 == fence.0 {
            vec![AlertProjectionCandidateRange::CursorSecond]
        } else {
            vec![
                AlertProjectionCandidateRange::CursorSecond,
                AlertProjectionCandidateRange::Middle,
                AlertProjectionCandidateRange::FenceSecond,
            ]
        };
        let mut candidates = Vec::new();
        match source_kind {
            ALERT_SOURCE_AUTH_TOKEN_LOG => {
                let cursor_id = cursor_id.parse::<i64>().unwrap_or_default();
                let fence_id = fence_id.parse::<i64>().unwrap_or_default();
                for range in ranges {
                    let rows = match range {
                        AlertProjectionCandidateRange::CursorSecond if cursor.0 == fence.0 => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT created_at, id FROM core.auth_token_logs
                                    WHERE (failure_kind = 'upstream_rate_limited_429'
                                           OR result_status = 'quota_exhausted')
                                      AND created_at >= $1 AND created_at = $2
                                      AND id > $3 AND id <= $4
                                    ORDER BY created_at ASC, id ASC LIMIT $5"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(cursor_id)
                            .bind(fence_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::CursorSecond => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT created_at, id FROM core.auth_token_logs
                                    WHERE (failure_kind = 'upstream_rate_limited_429'
                                           OR result_status = 'quota_exhausted')
                                      AND created_at >= $1 AND created_at = $2 AND id > $3
                                    ORDER BY created_at ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(cursor_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::Middle => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT created_at, id FROM core.auth_token_logs
                                    WHERE (failure_kind = 'upstream_rate_limited_429'
                                           OR result_status = 'quota_exhausted')
                                      AND created_at >= $1 AND created_at > $2 AND created_at < $3
                                    ORDER BY created_at ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(fence.0)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::FenceSecond => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT created_at, id FROM core.auth_token_logs
                                    WHERE (failure_kind = 'upstream_rate_limited_429'
                                           OR result_status = 'quota_exhausted')
                                      AND created_at >= $1 AND created_at = $2 AND id <= $3
                                    ORDER BY created_at ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(fence.0)
                            .bind(fence_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                    };
                    candidates.extend(rows.into_iter().map(|(occurred_at, id)| {
                        AlertProjectionSourceKey {
                            source_id: id.to_string(),
                            occurred_at,
                            row_sort_id: format!("atl:{id:020}"),
                        }
                    }));
                }
            }
            ALERT_SOURCE_SCHEDULED_JOB => {
                let cursor_id = cursor_id.parse::<i64>().unwrap_or_default();
                let fence_id = fence_id.parse::<i64>().unwrap_or_default();
                for range in ranges {
                    let rows = match range {
                        AlertProjectionCandidateRange::CursorSecond if cursor.0 == fence.0 => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                                     FROM core.scheduled_jobs
                                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                                      AND COALESCE(finished_at, started_at, queued_at) >= $1
                                      AND COALESCE(finished_at, started_at, queued_at) = $2
                                      AND id > $3 AND id <= $4
                                    ORDER BY 1 ASC, id ASC LIMIT $5"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(cursor_id)
                            .bind(fence_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::CursorSecond => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                                     FROM core.scheduled_jobs
                                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                                      AND COALESCE(finished_at, started_at, queued_at) >= $1
                                      AND COALESCE(finished_at, started_at, queued_at) = $2
                                      AND id > $3
                                    ORDER BY 1 ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(cursor_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::Middle => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                                     FROM core.scheduled_jobs
                                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                                      AND COALESCE(finished_at, started_at, queued_at) >= $1
                                      AND COALESCE(finished_at, started_at, queued_at) > $2
                                      AND COALESCE(finished_at, started_at, queued_at) < $3
                                    ORDER BY 1 ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(fence.0)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::FenceSecond => {
                            sqlx::query_as::<_, (i64, i64)>(
                                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                                     FROM core.scheduled_jobs
                                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                                      AND COALESCE(finished_at, started_at, queued_at) >= $1
                                      AND COALESCE(finished_at, started_at, queued_at) = $2
                                      AND id <= $3
                                    ORDER BY 1 ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(fence.0)
                            .bind(fence_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                    };
                    candidates.extend(rows.into_iter().map(|(occurred_at, id)| {
                        AlertProjectionSourceKey {
                            source_id: id.to_string(),
                            occurred_at,
                            row_sort_id: format!("job:{id:020}"),
                        }
                    }));
                }
            }
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD => {
                for range in ranges {
                    let rows = match range {
                        AlertProjectionCandidateRange::CursorSecond if cursor.0 == fence.0 => {
                            sqlx::query_as::<_, (i64, String)>(
                                r#"SELECT created_at, id FROM core.api_key_maintenance_records
                                    WHERE (COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                                           OR (source = 'system' AND operation_code = 'auto_mark_exhausted'
                                               AND reason_code = 'quota_exhausted'))
                                      AND created_at >= $1 AND created_at = $2
                                      AND id > $3 AND id <= $4
                                    ORDER BY created_at ASC, id ASC LIMIT $5"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(&cursor_id)
                            .bind(&fence_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::CursorSecond => {
                            sqlx::query_as::<_, (i64, String)>(
                                r#"SELECT created_at, id FROM core.api_key_maintenance_records
                                    WHERE (COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                                           OR (source = 'system' AND operation_code = 'auto_mark_exhausted'
                                               AND reason_code = 'quota_exhausted'))
                                      AND created_at >= $1 AND created_at = $2 AND id > $3
                                    ORDER BY created_at ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(&cursor_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::Middle => {
                            sqlx::query_as::<_, (i64, String)>(
                                r#"SELECT created_at, id FROM core.api_key_maintenance_records
                                    WHERE (COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                                           OR (source = 'system' AND operation_code = 'auto_mark_exhausted'
                                               AND reason_code = 'quota_exhausted'))
                                      AND created_at >= $1 AND created_at > $2 AND created_at < $3
                                    ORDER BY created_at ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(cursor.0)
                            .bind(fence.0)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                        AlertProjectionCandidateRange::FenceSecond => {
                            sqlx::query_as::<_, (i64, String)>(
                                r#"SELECT created_at, id FROM core.api_key_maintenance_records
                                    WHERE (COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                                           OR (source = 'system' AND operation_code = 'auto_mark_exhausted'
                                               AND reason_code = 'quota_exhausted'))
                                      AND created_at >= $1 AND created_at = $2 AND id <= $3
                                    ORDER BY created_at ASC, id ASC LIMIT $4"#,
                            )
                            .bind(retention_since)
                            .bind(fence.0)
                            .bind(&fence_id)
                            .bind(ALERT_PROJECTION_BATCH_ROWS)
                            .fetch_all(self.pg()?)
                            .await?
                        }
                    };
                    candidates.extend(rows.into_iter().map(|(occurred_at, source_id)| {
                        let row_sort_id = format!("maint:{source_id}");
                        AlertProjectionSourceKey {
                            source_id,
                            occurred_at,
                            row_sort_id,
                        }
                    }));
                }
            }
            other => {
                return Err(ProxyError::Other(format!(
                    "unknown alert projection source: {other}"
                )));
            }
        }
        candidates.sort_unstable_by(|left, right| {
            (left.occurred_at, &left.row_sort_id).cmp(&(right.occurred_at, &right.row_sort_id))
        });
        candidates.truncate(ALERT_PROJECTION_BATCH_ROWS as usize);
        Ok(candidates)
    }

    /// PostgreSQL twin of [`Self::alert_projection_hydrate_source_keys`].
    async fn alert_projection_hydrate_source_keys_pg(
        &self,
        source_kind: &str,
        source_keys: &[AlertProjectionSourceKey],
    ) -> Result<Vec<AlertEventProjectionRow>, ProxyError> {
        if source_keys.is_empty() {
            return Ok(Vec::new());
        }
        let filters = AlertEventFilters {
            alert_type: None,
            since: None,
            until: None,
            user_id: None,
            token_id: None,
            key_id: None,
            request_kinds: &[],
        };
        let source_ids = source_keys
            .iter()
            .map(|key| key.source_id.clone())
            .collect::<Vec<_>>();
        let mut query = AlertSql::new(SqlDialect::Postgres);
        Self::push_alert_events_cte_for_projection_source_keys_any(
            &mut query,
            filters,
            source_kind,
            &source_ids,
        );
        query.push(" SELECT * FROM alerts ORDER BY occurred_at ASC, row_sort_id ASC LIMIT ");
        query.push_bind(ALERT_PROJECTION_BATCH_ROWS);
        let rows = query
            .into_postgres()
            .expect("postgres alert builder")
            .build()
            .fetch_all(self.pg()?)
            .await?;
        rows.into_iter()
            .map(Self::decode_alert_event_projection_row_pg)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }
}

impl KeyStore {
    /// PostgreSQL twin of [`Self::persist_alert_projection_slice`]. Unlike the SQLite branch it does
    /// not need the runtime's owned immediate wrapper: the slice is one transaction and the state
    /// update is an optimistic compare on `generation`.
    async fn persist_alert_projection_slice_pg(
        &self,
        state: &AlertProjectionSourceState,
        fence: Option<&(i64, String)>,
        rows: &[AlertEventProjectionRow],
        next_cursor: Option<(i64, String)>,
        complete: bool,
        observed_at: i64,
    ) -> Result<(), ProxyError> {
        let next_cursor = next_cursor
            .unwrap_or_else(|| (state.cursor_occurred_at, state.cursor_row_sort_id.clone()));
        let mut tx = self.begin_pg_write().await?;

        let (build_generation, build_projection_revision) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT build_generation, build_projection_revision \
             FROM observability.admin_alert_canonical_groups_state \
             WHERE singleton = 1 AND build_generation > 0",
        )
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or_default();

        let projection_revision = if rows.is_empty() {
            sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM observability.dashboard_alert_projection_revision_state \
                 WHERE singleton = 1",
            )
            .fetch_one(&mut *tx)
            .await?
        } else {
            sqlx::query_scalar::<_, i64>(
                "UPDATE observability.dashboard_alert_projection_revision_state \
                 SET revision = revision + 1 WHERE singleton = 1 RETURNING revision",
            )
            .fetch_one(&mut *tx)
            .await?
        };

        for source_row in rows {
            let row = source_row.clone();
            let payload_json = serialize_alert_event_projection_payload(row.clone())?;
            if build_generation > 0 {
                sqlx::query(
                    r#"INSERT INTO observability.admin_alert_canonical_group_overrides
                           (build_generation, source_kind, source_id, occurred_at, row_sort_id, payload_json)
                       SELECT $1, source_kind, source_id, occurred_at, row_sort_id, payload_json
                         FROM observability.dashboard_alert_projection_events
                        WHERE source_kind = $2 AND source_id = $3 AND projection_revision <= $4
                       ON CONFLICT DO NOTHING"#,
                )
                .bind(build_generation)
                .bind(&row.source_kind)
                .bind(&row.source_id)
                .bind(build_projection_revision)
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query(
                r#"INSERT INTO observability.dashboard_alert_projection_events
                    (source_kind, source_id, occurred_at, row_sort_id, payload_json, projected_at,
                     projection_revision)
                   VALUES ($1, $2, $3, $4, $5, $6, $7)
                   ON CONFLICT (source_kind, source_id) DO UPDATE SET
                     occurred_at = excluded.occurred_at,
                     row_sort_id = excluded.row_sort_id,
                     payload_json = excluded.payload_json,
                     projected_at = excluded.projected_at,
                     projection_revision = excluded.projection_revision"#,
            )
            .bind(&row.source_kind)
            .bind(&row.source_id)
            .bind(row.occurred_at)
            .bind(&row.row_sort_id)
            .bind(payload_json)
            .bind(observed_at)
            .bind(projection_revision)
            .execute(&mut *tx)
            .await?;
        }

        let (fence_occurred_at, fence_row_sort_id, phase) = if complete {
            (None, None, "idle")
        } else {
            let (occurred_at, row_sort_id) = fence.ok_or_else(|| {
                ProxyError::Other("alert projection slice is missing source fence".to_string())
            })?;
            (Some(*occurred_at), Some(row_sort_id.clone()), "catching_up")
        };

        let changed = match state.lane {
            AlertProjectionLane::RecentTail => sqlx::query(
                r#"UPDATE observability.dashboard_alert_projection_state
                      SET cursor_occurred_at = $1, cursor_row_sort_id = $2,
                          fence_occurred_at = $3, fence_row_sort_id = $4,
                          generation = generation + 1, phase = $5, observed_at = $6,
                          stale_reason = NULL
                    WHERE source_kind = $7 AND generation = $8"#,
            )
            .bind(next_cursor.0)
            .bind(&next_cursor.1)
            .bind(fence_occurred_at)
            .bind(fence_row_sort_id.as_deref())
            .bind(phase)
            .bind(observed_at)
            .bind(&state.source_kind)
            .bind(state.generation)
            .execute(&mut *tx)
            .await?,
            AlertProjectionLane::History => sqlx::query(
                r#"UPDATE observability.dashboard_alert_projection_history_state
                      SET cursor_occurred_at = $1, cursor_row_sort_id = $2,
                          fence_occurred_at = $3, fence_row_sort_id = $4,
                          generation = generation + 1, phase = $5
                    WHERE source_kind = $6 AND generation = $7"#,
            )
            .bind(next_cursor.0)
            .bind(&next_cursor.1)
            .bind(fence_occurred_at)
            .bind(fence_row_sort_id.as_deref())
            .bind(phase)
            .bind(&state.source_kind)
            .bind(state.generation)
            .execute(&mut *tx)
            .await?,
        };
        if changed.rows_affected() != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Other(
                "alert projection state changed while a slice was in flight".to_string(),
            ));
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(())
    }

    /// PostgreSQL twin of [`Self::alert_projection_status`].
    async fn alert_projection_status_pg(&self) -> Result<AlertProjectionStatus, ProxyError> {
        let now = self.backend_time.now_ts();
        let retention_since = now.saturating_sub(ALERT_PROJECTION_STALE_SECS);
        let mut session = self.begin_pg_read_session().await?;
        let (sources, observed_at, idle_sources, fresh_sources, stale_reason, history_sources, idle_history_sources) =
            sqlx::query_as::<_, (i64, Option<i64>, i64, i64, Option<String>, i64, i64)>(
                r#"SELECT COUNT(tail.source_kind), MIN(tail.observed_at),
                          CAST(COALESCE(SUM(CASE WHEN tail.phase = 'idle' THEN 1 ELSE 0 END), 0) AS BIGINT),
                          CAST(COALESCE(SUM(CASE WHEN tail.observed_at IS NOT NULL AND tail.observed_at >= $1 THEN 1 ELSE 0 END), 0) AS BIGINT),
                          MAX(tail.stale_reason),
                          COUNT(history.source_kind),
                          CAST(COALESCE(SUM(CASE WHEN history.phase = 'idle' THEN 1 ELSE 0 END), 0) AS BIGINT)
                     FROM observability.dashboard_alert_projection_state AS tail
                     LEFT JOIN observability.dashboard_alert_projection_history_state AS history
                       ON history.source_kind = tail.source_kind"#,
            )
            .bind(retention_since)
            .fetch_one(&mut *session.connection())
            .await?;
        let observations_expired = sources == ALERT_PROJECTION_SOURCES.len() as i64
            && idle_sources == sources
            && fresh_sources != sources;
        let mut recent_coverage = if sources == ALERT_PROJECTION_SOURCES.len() as i64
            && idle_sources == sources
            && fresh_sources == sources
        {
            "ok"
        } else if stale_reason.is_some() || observations_expired {
            "stale"
        } else {
            "projecting"
        };
        if recent_coverage == "projecting" && stale_reason.is_none() {
            let incomplete_sources = sqlx::query_scalar::<_, String>(
                r#"SELECT source_kind
                     FROM observability.dashboard_alert_projection_state
                    WHERE phase <> 'idle'
                    ORDER BY source_kind ASC"#,
            )
            .fetch_all(&mut *session.connection())
            .await?;
            if !incomplete_sources.is_empty() {
                let mut sources_have_events = false;
                for source_kind in incomplete_sources {
                    if self
                        .alert_projection_source_fence_pg(&source_kind)
                        .await?
                        .is_some()
                    {
                        sources_have_events = true;
                        break;
                    }
                }
                if !sources_have_events {
                    recent_coverage = "ok";
                }
            }
        }
        session.finish().await?;
        let coverage = if recent_coverage == "ok"
            && history_sources == ALERT_PROJECTION_SOURCES.len() as i64
            && idle_history_sources == history_sources
        {
            "ok"
        } else if recent_coverage == "stale" {
            "stale"
        } else {
            "projecting"
        };
        Ok(AlertProjectionStatus {
            coverage: coverage.to_string(),
            recent_coverage: recent_coverage.to_string(),
            observed_at,
            stale_reason: stale_reason
                .or_else(|| observations_expired.then(|| "observation_expired".to_string())),
        })
    }
}

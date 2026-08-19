// Dashboard consumes only recent alerts, while administrator Events and Groups
// wait for the same sidecar to finish its full durable history. Keep every
// write well below the foreground writer budget; catch-up remains durable via
// the source fence and cursor rather than a large transaction.
const ALERT_PROJECTION_BATCH_ROWS: i64 = 25;
const ALERT_PROJECTION_STALE_SECS: i64 = 90;
const ALERT_PROJECTION_SUMMARY_REFRESH_SECS: i64 = 60;
const ALERT_PROJECTION_DASHBOARD_WINDOW_HOURS: i64 = 24;
const ALERT_PROJECTION_SOURCES: [&str; 3] = [
    ALERT_SOURCE_AUTH_TOKEN_LOG,
    ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD,
    ALERT_SOURCE_SCHEDULED_JOB,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlertProjectionStatus {
    pub(crate) coverage: String,
    pub(crate) recent_coverage: String,
    pub(crate) observed_at: Option<i64>,
    pub(crate) stale_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertProjectionLane {
    RecentTail,
    History,
}

#[derive(Debug, Clone)]
struct AlertProjectionSourceState {
    source_kind: String,
    cursor_occurred_at: i64,
    cursor_row_sort_id: String,
    fence_occurred_at: Option<i64>,
    fence_row_sort_id: Option<String>,
    generation: i64,
    phase: String,
    lane: AlertProjectionLane,
}

#[derive(Debug, Clone)]
struct AlertProjectionSourceKey {
    source_id: String,
    occurred_at: i64,
    row_sort_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlertProjectionSliceOutcome {
    Advanced {
        rows: i64,
        complete: bool,
        dashboard_dirty: bool,
    },
    Idle,
    Deferred { reason: SqliteAdmissionDeferReason },
}

impl KeyStore {
    pub(crate) fn try_admit_alert_projection(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_maintenance_bulk(SqliteOperation::AlertProjection)
    }

    pub(crate) fn try_admit_admin_alert_read(
        &self,
    ) -> Result<SqliteMaintenanceBulkPermit, SqliteAdmissionDeferReason> {
        self.sqlite_runtime
            .try_admit_maintenance_bulk(SqliteOperation::AdminRead)
    }

    async fn alert_projection_source_state(
        &self,
        lane: AlertProjectionLane,
    ) -> Result<Option<AlertProjectionSourceState>, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let query = match lane {
            AlertProjectionLane::RecentTail => {
                r#"SELECT source_kind, cursor_occurred_at, cursor_row_sort_id,
                          fence_occurred_at, fence_row_sort_id, generation, phase
                     FROM observability.dashboard_alert_projection_state
                    ORDER BY
                      CASE WHEN phase = 'catching_up' THEN 0 ELSE 1 END,
                      generation ASC,
                      source_kind ASC
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
        let query = sqlx::query_as::<_, (
            String,
            i64,
            String,
            Option<i64>,
            Option<String>,
            i64,
            String,
        )>(query);
        let result = query.fetch_optional(&mut *conn).await;
        let row = conn.complete_query(result).await?;
        let Some((
            source_kind,
            cursor_occurred_at,
            cursor_row_sort_id,
            fence_occurred_at,
            fence_row_sort_id,
            generation,
            phase,
        )) = row
        else {
            return Ok(None);
        };
        Ok(Some(AlertProjectionSourceState {
            source_kind,
            cursor_occurred_at,
            cursor_row_sort_id,
            fence_occurred_at,
            fence_row_sort_id,
            generation,
            phase,
            lane,
        }))
    }

    async fn alert_projection_source_state_for_kind(
        &self,
        lane: AlertProjectionLane,
        source_kind: &str,
    ) -> Result<Option<AlertProjectionSourceState>, ProxyError> {
        let table = match lane {
            AlertProjectionLane::RecentTail => {
                "observability.dashboard_alert_projection_state"
            }
            AlertProjectionLane::History => {
                "observability.dashboard_alert_projection_history_state"
            }
        };
        let query = format!(
            "SELECT source_kind, cursor_occurred_at, cursor_row_sort_id, \
                    fence_occurred_at, fence_row_sort_id, generation, phase \
               FROM {table} WHERE source_kind = ?"
        );
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = sqlx::query_as::<_, (
            String,
            i64,
            String,
            Option<i64>,
            Option<String>,
            i64,
            String,
        )>(&query)
        .bind(source_kind)
        .fetch_optional(&mut *conn)
        .await;
        let row = conn.complete_query(result).await?;
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

    async fn alert_projection_source_fence(
        &self,
        source_kind: &str,
    ) -> Result<Option<(i64, String)>, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = match source_kind {
            ALERT_SOURCE_AUTH_TOKEN_LOG => sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT created_at, id
                     FROM auth_token_logs
                    WHERE failure_kind = 'upstream_rate_limited_429'
                       OR result_status = 'quota_exhausted'
                    ORDER BY created_at DESC, id DESC
                    LIMIT 1"#,
            )
            .fetch_optional(&mut *conn)
            .await
            .map(|row| row.map(|(occurred_at, id)| (occurred_at, format!("atl:{id:020}")))),
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD => sqlx::query_as::<_, (i64, String)>(
                r#"SELECT occurred_at, source_id
                     FROM (
                        SELECT created_at AS occurred_at, id AS source_id
                          FROM api_key_maintenance_records
                         WHERE COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                        UNION ALL
                        SELECT created_at AS occurred_at, id AS source_id
                          FROM api_key_maintenance_records
                         WHERE source = 'system'
                           AND operation_code = 'auto_mark_exhausted'
                           AND reason_code = 'quota_exhausted'
                     )
                    ORDER BY occurred_at DESC, source_id DESC
                    LIMIT 1"#,
            )
            .fetch_optional(&mut *conn)
            .await
            .map(|row| row.map(|(occurred_at, source_id)| (occurred_at, format!("maint:{source_id}")))),
            ALERT_SOURCE_SCHEDULED_JOB => sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                     FROM scheduled_jobs
                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                    ORDER BY COALESCE(finished_at, started_at, queued_at) DESC, id DESC
                    LIMIT 1"#,
            )
            .fetch_optional(&mut *conn)
            .await
            .map(|row| row.map(|(occurred_at, id)| (occurred_at, format!("job:{id:020}")))),
            other => Err(sqlx::Error::Protocol(format!(
                "unknown alert projection source: {other}"
            ))),
        };
        conn.complete_query(result).await
    }

    fn alert_projection_source_cursor_id(source_kind: &str, row_sort_id: &str) -> String {
        match source_kind {
            ALERT_SOURCE_AUTH_TOKEN_LOG => row_sort_id
                .strip_prefix("atl:")
                .unwrap_or_default()
                .trim_start_matches('0')
                .to_string(),
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD => row_sort_id
                .strip_prefix("maint:")
                .unwrap_or_default()
                .to_string(),
            ALERT_SOURCE_SCHEDULED_JOB => row_sort_id
                .strip_prefix("job:")
                .unwrap_or_default()
                .trim_start_matches('0')
                .to_string(),
            _ => String::new(),
        }
    }

    async fn alert_projection_source_keys(
        &self,
        source_kind: &str,
        cursor: (i64, &str),
        fence: (i64, &str),
    ) -> Result<Vec<AlertProjectionSourceKey>, ProxyError> {
        let cursor_id = Self::alert_projection_source_cursor_id(source_kind, cursor.1);
        let fence_id = Self::alert_projection_source_cursor_id(source_kind, fence.1);
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = match source_kind {
            ALERT_SOURCE_AUTH_TOKEN_LOG => sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT created_at, id
                     FROM auth_token_logs
                    WHERE (failure_kind = 'upstream_rate_limited_429'
                           OR result_status = 'quota_exhausted')
                      AND (created_at > ? OR (created_at = ? AND id > ?))
                      AND (created_at < ? OR (created_at = ? AND id <= ?))
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?"#,
            )
            .bind(cursor.0)
            .bind(cursor.0)
            .bind(cursor_id.parse::<i64>().unwrap_or_default())
            .bind(fence.0)
            .bind(fence.0)
            .bind(fence_id.parse::<i64>().unwrap_or_default())
            .bind(ALERT_PROJECTION_BATCH_ROWS)
            .fetch_all(&mut *conn)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|(occurred_at, id)| AlertProjectionSourceKey {
                        source_id: id.to_string(),
                        occurred_at,
                        row_sort_id: format!("atl:{id:020}"),
                    })
                    .collect()
            }),
            ALERT_SOURCE_API_KEY_MAINTENANCE_RECORD => sqlx::query_as::<_, (i64, String)>(
                r#"SELECT occurred_at, source_id
                     FROM (
                        SELECT created_at AS occurred_at, id AS source_id
                          FROM api_key_maintenance_records
                         WHERE COALESCE(reason_code, '') IN ('account_deactivated', 'key_revoked', 'invalid_api_key')
                        UNION ALL
                        SELECT created_at AS occurred_at, id AS source_id
                          FROM api_key_maintenance_records
                         WHERE source = 'system'
                           AND operation_code = 'auto_mark_exhausted'
                           AND reason_code = 'quota_exhausted'
                     )
                    WHERE (occurred_at > ? OR (occurred_at = ? AND source_id > ?))
                      AND (occurred_at < ? OR (occurred_at = ? AND source_id <= ?))
                    ORDER BY occurred_at ASC, source_id ASC
                    LIMIT ?"#,
            )
            .bind(cursor.0)
            .bind(cursor.0)
            .bind(cursor_id)
            .bind(fence.0)
            .bind(fence.0)
            .bind(fence_id)
            .bind(ALERT_PROJECTION_BATCH_ROWS)
            .fetch_all(&mut *conn)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|(occurred_at, source_id)| AlertProjectionSourceKey {
                        row_sort_id: format!("maint:{source_id}"),
                        source_id,
                        occurred_at,
                    })
                    .collect()
            }),
            ALERT_SOURCE_SCHEDULED_JOB => sqlx::query_as::<_, (i64, i64)>(
                r#"SELECT COALESCE(finished_at, started_at, queued_at), id
                     FROM scheduled_jobs
                    WHERE LOWER(TRIM(status)) IN ('error', 'failed')
                      AND (COALESCE(finished_at, started_at, queued_at) > ?
                           OR (COALESCE(finished_at, started_at, queued_at) = ? AND id > ?))
                      AND (COALESCE(finished_at, started_at, queued_at) < ?
                           OR (COALESCE(finished_at, started_at, queued_at) = ? AND id <= ?))
                    ORDER BY COALESCE(finished_at, started_at, queued_at) ASC, id ASC
                    LIMIT ?"#,
            )
            .bind(cursor.0)
            .bind(cursor.0)
            .bind(cursor_id.parse::<i64>().unwrap_or_default())
            .bind(fence.0)
            .bind(fence.0)
            .bind(fence_id.parse::<i64>().unwrap_or_default())
            .bind(ALERT_PROJECTION_BATCH_ROWS)
            .fetch_all(&mut *conn)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|(occurred_at, id)| AlertProjectionSourceKey {
                        source_id: id.to_string(),
                        occurred_at,
                        row_sort_id: format!("job:{id:020}"),
                    })
                    .collect()
            }),
            other => Err(sqlx::Error::Protocol(format!(
                "unknown alert projection source: {other}"
            ))),
        };
        conn.complete_query(result).await
    }

    async fn alert_projection_hydrate_source_keys(
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
        let mut query = QueryBuilder::new("");
        let source_ids = source_keys
            .iter()
            .map(|key| key.source_id.clone())
            .collect::<Vec<_>>();
        Self::push_alert_events_cte_for_projection_source_keys(
            &mut query,
            filters,
            source_kind,
            &source_ids,
        );
        query.push(" SELECT * FROM alerts ORDER BY occurred_at ASC, row_sort_id ASC");
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = query.build().fetch_all(&mut *conn).await;
        let rows = conn.complete_query(result).await?;
        rows.into_iter()
            .map(Self::decode_alert_event_projection_row)
            .collect::<Result<Vec<_>, _>>()
            .map_err(ProxyError::from)
    }

    pub(crate) async fn advance_alert_projection_slice(
        &self,
    ) -> Result<AlertProjectionSliceOutcome, ProxyError> {
        // A lazy three-connection pool can transiently have one checked-out
        // connection and one idle connection without foreground pressure.
        // Grow the unopened final slot within the runtime's short budget
        // before deciding whether a projection slice must defer.
        self.sqlite_runtime
            .prewarm_maintenance_bulk_capacity()
            .await;
        let _admission = match self.try_admit_alert_projection() {
            Ok(permit) => permit,
            Err(reason) => {
                tracing::debug!(
                    component = "dashboard_alert_projection",
                    event = "deferred",
                    defer_reason = reason.as_str(),
                    "deferred an alert projection slice before SQLite acquisition"
                );
                return Ok(AlertProjectionSliceOutcome::Deferred { reason });
            }
        };
        match self.advance_admitted_alert_projection_slice().await {
            Ok(outcome) => Ok(outcome),
            Err(err) if crate::is_transient_sqlite_write_error(&err) => {
                // No cursor update has committed, so a later scheduler wake
                // can replay this exact fenced page. Treat all bounded slice
                // contention uniformly; an acquire/read timeout is just as
                // recoverable as a contended final write.
                self.sqlite_runtime.record_deferred(
                    SqliteOperation::AlertProjection,
                    SqliteAdmissionDeferReason::RecentContention,
                );
                tracing::debug!(
                    component = "dashboard_alert_projection",
                    event = "deferred",
                    defer_reason = "sqlite_contention",
                    "deferred an alert projection slice after SQLite contention"
                );
                Ok(AlertProjectionSliceOutcome::Deferred {
                    reason: SqliteAdmissionDeferReason::RecentContention,
                })
            }
            Err(err) => Err(err),
        }
    }

    pub(crate) async fn refresh_alert_projection_observation(
        &self,
    ) -> Result<bool, ProxyError> {
        let _admission = match self.try_admit_alert_projection() {
            Ok(permit) => permit,
            Err(_) => return Ok(false),
        };
        let now = self.backend_time.now_ts();
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::AlertProjection)
            .await?;
        let mut rows_affected = 0;
        let result = async {
            let changed = sqlx::query(
                r#"UPDATE observability.dashboard_alert_projection_state
                       SET observed_at = ?, stale_reason = NULL
                     WHERE phase = 'idle'
                       AND (observed_at IS NULL OR observed_at < ?)"#,
            )
            .bind(now)
            .bind(now.saturating_sub(ALERT_PROJECTION_STALE_SECS / 2))
            .execute(&mut *tx)
            .await?;
            rows_affected = changed.rows_affected();
            Ok::<(), ProxyError>(())
        }
        .await;
        tx.finish(result).await?;
        Ok(rows_affected > 0)
    }

    async fn advance_admitted_alert_projection_slice(
        &self,
    ) -> Result<AlertProjectionSliceOutcome, ProxyError> {
        let mut recent = self
            .alert_projection_source_state(AlertProjectionLane::RecentTail)
            .await?
            .ok_or_else(|| ProxyError::Other("missing alert projection tail state".to_string()))?;
        let history = self
            .alert_projection_source_state(AlertProjectionLane::History)
            .await?;
        let mut recent_fence = match (
            recent.fence_occurred_at,
            recent.fence_row_sort_id.as_deref(),
        ) {
            (Some(occurred_at), Some(row_sort_id)) => Some((occurred_at, row_sort_id.to_string())),
            _ => self.alert_projection_source_fence(&recent.source_kind).await?,
        };
        let mut recent_has_debt = recent_fence.as_ref().is_some_and(|fence| {
            (fence.0, fence.1.as_str())
                > (recent.cursor_occurred_at, recent.cursor_row_sort_id.as_str())
        });
        // A catching-up source must not hide a newly arrived alert from an
        // otherwise idle source. Probe the other two bounded watermarks every
        // slice and let newly eligible tail work preempt historical backlog.
        // This advances a cursor only when there is actual source work.
        for source_kind in ALERT_PROJECTION_SOURCES {
            if source_kind == recent.source_kind {
                continue;
            }
            let Some(candidate) = self
                .alert_projection_source_state_for_kind(
                    AlertProjectionLane::RecentTail,
                    source_kind,
                )
                .await?
            else {
                continue;
            };
            if candidate.phase != "idle" {
                continue;
            }
            let candidate_fence = match (
                candidate.fence_occurred_at,
                candidate.fence_row_sort_id.as_deref(),
            ) {
                (Some(occurred_at), Some(row_sort_id)) => {
                    Some((occurred_at, row_sort_id.to_string()))
                }
                _ => self.alert_projection_source_fence(&candidate.source_kind).await?,
            };
            let candidate_has_debt = candidate_fence.as_ref().is_some_and(|fence| {
                (fence.0, fence.1.as_str())
                    > (candidate.cursor_occurred_at, candidate.cursor_row_sort_id.as_str())
            });
            if candidate_has_debt {
                recent = candidate;
                recent_fence = candidate_fence;
                recent_has_debt = true;
                break;
            }
        }
        // The durable history lane owns catch-up whenever the selected recent
        // source is already current. A bounded recent fence probe before each
        // history slice keeps the Dashboard tail responsive without using tail
        // generation bumps as an implicit round-robin clock.
        let (state, fence) = match history {
            Some(history) if !recent_has_debt && recent.phase == "idle" => (history, None),
            _ => (recent, recent_fence),
        };
        let fence = match (
            fence,
            state.fence_occurred_at,
            state.fence_row_sort_id.as_deref(),
        ) {
            (Some(fence), _, _) => Some(fence),
            (None, Some(occurred_at), Some(row_sort_id)) => {
                Some((occurred_at, row_sort_id.to_string()))
            }
            (None, _, _) if state.lane == AlertProjectionLane::RecentTail => {
                self.alert_projection_source_fence(&state.source_kind).await?
            }
            (None, _, _) => None,
        };
        let now = self.backend_time.now_ts();
        let Some(fence) = fence else {
            if state.lane == AlertProjectionLane::RecentTail && state.phase == "idle" {
                return Ok(AlertProjectionSliceOutcome::Idle);
            }
            self.persist_alert_projection_slice(&state, None, &[], None, true, now)
                .await?;
            return Ok(AlertProjectionSliceOutcome::Advanced {
                rows: 0,
                complete: true,
                dashboard_dirty: false,
            });
        };
        if state.lane == AlertProjectionLane::RecentTail
            && state.phase == "idle"
            && (fence.0, fence.1.as_str())
                <= (state.cursor_occurred_at, state.cursor_row_sort_id.as_str())
        {
            return Ok(AlertProjectionSliceOutcome::Idle);
        }
        let source_keys = self
            .alert_projection_source_keys(
                &state.source_kind,
                (state.cursor_occurred_at, &state.cursor_row_sort_id),
                (fence.0, &fence.1),
            )
            .await?;
        let rows = self
            .alert_projection_hydrate_source_keys(&state.source_kind, &source_keys)
            .await?;
        let next_cursor = source_keys
            .last()
            .map(|row| (row.occurred_at, row.row_sort_id.clone()));
        let complete = next_cursor
            .as_ref()
            .map(|cursor| cursor.0 == fence.0 && cursor.1 == fence.1)
            .unwrap_or(true)
            || source_keys.len() < ALERT_PROJECTION_BATCH_ROWS as usize;
        self.persist_alert_projection_slice(
            &state,
            Some(&fence),
            &rows,
            next_cursor,
            complete,
            now,
        )
            .await?;
        tracing::debug!(
            component = "dashboard_alert_projection",
            event = "slice",
            source = state.source_kind,
            lane = ?state.lane,
            rows = rows.len(),
            complete,
            "advanced a bounded alert projection slice"
        );
        Ok(AlertProjectionSliceOutcome::Advanced {
            rows: rows.len() as i64,
            complete,
            dashboard_dirty: state.lane == AlertProjectionLane::RecentTail && !rows.is_empty(),
        })
    }

    async fn persist_alert_projection_slice(
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
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::AlertProjection)
            .await?;
        let result = async {
            for row in rows {
                let payload_json = serde_json::to_string(row).map_err(|err| {
                    ProxyError::Other(format!("serialize alert projection event: {err}"))
                })?;
                sqlx::query(
                    r#"INSERT INTO observability.dashboard_alert_projection_events
                        (source_kind, source_id, occurred_at, row_sort_id, payload_json, projected_at)
                       VALUES (?, ?, ?, ?, ?, ?)
                       ON CONFLICT(source_kind, source_id) DO UPDATE SET
                         occurred_at = excluded.occurred_at,
                         row_sort_id = excluded.row_sort_id,
                         payload_json = excluded.payload_json,
                         projected_at = excluded.projected_at"#,
                )
                .bind(&row.source_kind)
                .bind(&row.source_id)
                .bind(row.occurred_at)
                .bind(&row.row_sort_id)
                .bind(payload_json)
                .bind(observed_at)
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
                        SET cursor_occurred_at = ?, cursor_row_sort_id = ?,
                            fence_occurred_at = ?, fence_row_sort_id = ?,
                            generation = generation + 1, phase = ?, observed_at = ?, stale_reason = NULL
                      WHERE source_kind = ? AND generation = ?"#,
                )
                .bind(next_cursor.0)
                .bind(next_cursor.1)
                .bind(fence_occurred_at)
                .bind(fence_row_sort_id)
                .bind(phase)
                .bind(observed_at)
                .bind(&state.source_kind)
                .bind(state.generation)
                .execute(&mut *tx)
                .await?,
                AlertProjectionLane::History => sqlx::query(
                    r#"UPDATE observability.dashboard_alert_projection_history_state
                        SET cursor_occurred_at = ?, cursor_row_sort_id = ?,
                            fence_occurred_at = ?, fence_row_sort_id = ?,
                            generation = generation + 1, phase = ?
                      WHERE source_kind = ? AND generation = ?"#,
                )
                .bind(next_cursor.0)
                .bind(next_cursor.1)
                .bind(fence_occurred_at)
                .bind(fence_row_sort_id)
                .bind(phase)
                .bind(&state.source_kind)
                .bind(state.generation)
                .execute(&mut *tx)
                .await?,
            };
            if changed.rows_affected() != 1 {
                return Err(ProxyError::Other(
                    "alert projection state changed while a slice was in flight".to_string(),
                ));
            }
            Ok::<(), ProxyError>(())
        }
        .await;
        tx.finish(result).await
    }

    pub(crate) async fn alert_projection_status(&self) -> Result<AlertProjectionStatus, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = sqlx::query_as::<_, (i64, Option<i64>, i64, i64, Option<String>, i64, i64)>(
            r#"SELECT COUNT(tail.source_kind), MIN(tail.observed_at),
                      SUM(CASE WHEN tail.phase = 'idle' THEN 1 ELSE 0 END),
                      SUM(CASE WHEN tail.observed_at IS NOT NULL AND tail.observed_at >= ? THEN 1 ELSE 0 END),
                      MAX(tail.stale_reason),
                      COUNT(history.source_kind),
                      SUM(CASE WHEN history.phase = 'idle' THEN 1 ELSE 0 END)
                 FROM observability.dashboard_alert_projection_state AS tail
                 LEFT JOIN observability.dashboard_alert_projection_history_state AS history
                   ON history.source_kind = tail.source_kind"#,
        )
        .bind(now.saturating_sub(ALERT_PROJECTION_STALE_SECS))
        .fetch_one(&mut *conn)
        .await;
        let (
            sources,
            observed_at,
            idle_sources,
            fresh_sources,
            stale_reason,
            history_sources,
            idle_history_sources,
        ) =
            conn.complete_query(result).await?;
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
        // A newly created sidecar has no observation timestamps yet. It is
        // nevertheless safe for Dashboard to serve an empty recent-alert
        // view when every unfinished tail source has an empty direct
        // watermark. This uses three bounded source seeks, not the raw alert
        // CTE, and avoids turning a cold no-alert database into a 5xx.
        if recent_coverage == "projecting" && stale_reason.is_none() {
            let mut conn = self
                .sqlite_runtime
                .acquire_operation_connection(SqliteOperation::AlertProjection)
                .await?;
            let result = sqlx::query_scalar::<_, String>(
                r#"SELECT source_kind
                     FROM observability.dashboard_alert_projection_state
                    WHERE phase <> 'idle'
                    ORDER BY source_kind ASC"#,
            )
            .fetch_all(&mut *conn)
            .await;
            let incomplete_sources = conn.complete_query(result).await?;
            if !incomplete_sources.is_empty() {
                let mut sources_have_events = false;
                for source_kind in incomplete_sources {
                    if self.alert_projection_source_fence(&source_kind).await?.is_some() {
                        sources_have_events = true;
                        break;
                    }
                }
                if !sources_have_events {
                    recent_coverage = "ok";
                }
            }
        }
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
            stale_reason: stale_reason.or_else(|| {
                observations_expired.then(|| "observation_expired".to_string())
            }),
        })
    }

    pub(crate) async fn fetch_projected_recent_alerts_summary(
        &self,
        window_hours: i64,
    ) -> Result<RecentAlertsSummary, ProxyError> {
        let clamped_window_hours = window_hours.clamp(1, 24 * 30);
        let status = self.alert_projection_status().await?;
        let (source_generation, materialized_summary) = self
            .load_materialized_projected_recent_alerts_summary_with_generation(
                clamped_window_hours,
            )
            .await?;
        if let Some((cached_generation, summary)) = materialized_summary
        {
            return Ok(Self::apply_alert_projection_status(
                summary,
                &status,
                cached_generation != source_generation,
            ));
        }
        // Dashboard and SSE reads own no aggregation work. Until the worker
        // materializes its first snapshot, callers receive an explicit stale
        // value rather than rebuilding the alert projection CTE themselves.
        Ok(Self::unavailable_projected_recent_alerts_summary(
            clamped_window_hours,
            &status,
        ))
    }

    pub(crate) async fn refresh_dashboard_alert_projection_summary(&self) -> Result<bool, ProxyError> {
        self.sqlite_runtime
            .prewarm_maintenance_bulk_capacity()
            .await;
        let Ok(_permit) = self.try_admit_alert_projection() else {
            return Ok(false);
        };
        let window_hours = ALERT_PROJECTION_DASHBOARD_WINDOW_HOURS;
        let now = self.backend_time.now_ts();
        let status = self.alert_projection_status().await?;
        // A materialized Dashboard summary is all-or-nothing for the recent
        // tail. Waiting until every source reaches its fence prevents the
        // first completed source from publishing a partial value that would
        // otherwise consume the 60-second refresh window.
        if status.recent_coverage != "ok" {
            return Ok(false);
        }
        if let Some((_cached_generation, computed_at)) = self
            .materialized_projected_recent_alerts_summary_metadata(window_hours)
            .await?
            && now.saturating_sub(computed_at) < ALERT_PROJECTION_SUMMARY_REFRESH_SECS
        {
            return Ok(false);
        }
        let source_generation = self.alert_projection_recent_generation().await?;
        let summary = self
            .compute_projected_recent_alerts_summary(window_hours, &status)
            .await?;
        self.persist_materialized_projected_recent_alerts_summary(
            window_hours,
            source_generation,
            &summary,
        )
        .await?;
        Ok(true)
    }

    async fn alert_projection_recent_generation(&self) -> Result<i64, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state",
        )
        .fetch_one(&mut *conn)
        .await;
        conn.complete_query(result).await
    }

    async fn materialized_projected_recent_alerts_summary_metadata(
        &self,
        window_hours: i64,
    ) -> Result<Option<(i64, i64)>, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT source_generation, computed_at
                 FROM observability.dashboard_alert_projection_recent_summaries
                WHERE window_hours = ?"#,
        )
        .bind(window_hours)
        .fetch_optional(&mut *conn)
        .await;
        conn.complete_query(result).await
    }

    async fn load_materialized_projected_recent_alerts_summary_with_generation(
        &self,
        window_hours: i64,
    ) -> Result<(i64, Option<(i64, RecentAlertsSummary)>), ProxyError> {
        let mut snapshot = self
            .sqlite_runtime
            .begin_read_snapshot(SqliteOperation::AlertProjection)
            .await?;
        let result = async {
            let source_generation = sqlx::query_scalar::<_, i64>(
                "SELECT COALESCE(SUM(generation), 0) FROM observability.dashboard_alert_projection_state",
            )
            .fetch_one(&mut *snapshot)
            .await?;
            let payload = sqlx::query_as::<_, (i64, String)>(
                r#"SELECT source_generation, summary_json
                     FROM observability.dashboard_alert_projection_recent_summaries
                    WHERE window_hours = ?"#,
            )
            .bind(window_hours)
            .fetch_optional(&mut *snapshot)
            .await?;
            Ok::<_, sqlx::Error>((source_generation, payload))
        }
        .await;
        let (source_generation, payload) = snapshot.complete_query(result).await?;
        let summary = payload
            .map(|(source_generation, payload)| {
                serde_json::from_str(&payload).map_err(|err| {
                    ProxyError::Other(format!(
                        "deserialize materialized recent alert summary: {err}"
                    ))
                }).map(|summary| (source_generation, summary))
            })
            .transpose()?;
        Ok((source_generation, summary))
    }

    async fn persist_materialized_projected_recent_alerts_summary(
        &self,
        window_hours: i64,
        source_generation: i64,
        summary: &RecentAlertsSummary,
    ) -> Result<(), ProxyError> {
        let payload = serde_json::to_string(summary).map_err(|err| {
            ProxyError::Other(format!("serialize materialized recent alert summary: {err}"))
        })?;
        let mut tx = self
            .sqlite_runtime
            .begin_immediate(SqliteOperation::AlertProjection)
            .await?;
        let result = async {
            sqlx::query(
                r#"INSERT INTO observability.dashboard_alert_projection_recent_summaries
                    (window_hours, source_generation, computed_at, summary_json)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(window_hours) DO UPDATE SET
                   source_generation = excluded.source_generation,
                   computed_at = excluded.computed_at,
                   summary_json = excluded.summary_json"#,
            )
            .bind(window_hours)
            .bind(source_generation)
            .bind(self.backend_time.now_ts())
            .bind(payload)
            .execute(&mut *tx)
            .await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        tx.finish(result).await
    }

    fn apply_alert_projection_status(
        mut summary: RecentAlertsSummary,
        status: &AlertProjectionStatus,
        materialization_stale: bool,
    ) -> RecentAlertsSummary {
        summary.coverage = if status.recent_coverage == "projecting" {
            "projecting".to_string()
        } else if status.recent_coverage == "ok" && !materialization_stale {
            "ok".to_string()
        } else {
            "stale".to_string()
        };
        summary.stale = summary.coverage != "ok" || status.stale_reason.is_some();
        summary.error = status
            .stale_reason
            .clone()
            .or_else(|| {
                (status.recent_coverage != "ok").then(|| "projection_catching_up".to_string())
            })
            .or_else(|| materialization_stale.then(|| "summary_refresh_pending".to_string()));
        summary
    }

    fn unavailable_projected_recent_alerts_summary(
        window_hours: i64,
        status: &AlertProjectionStatus,
    ) -> RecentAlertsSummary {
        let summary = RecentAlertsSummary {
            window_hours,
            ..Default::default()
        };
        Self::apply_alert_projection_status(summary, status, true)
    }

    async fn compute_projected_recent_alerts_summary(
        &self,
        clamped_window_hours: i64,
        status: &AlertProjectionStatus,
    ) -> Result<RecentAlertsSummary, ProxyError> {
        let now = self.backend_time.now_ts();
        let since = now.saturating_sub(clamped_window_hours.saturating_mul(3600));
        let filters = AlertEventFilters {
            alert_type: None,
            since: Some(since),
            until: None,
            user_id: None,
            token_id: None,
            key_id: None,
            request_kinds: &[],
        };
        let (top_groups, grouped_count) = self
            .fetch_projected_alert_group_page(filters, 1, 10)
            .await?;
        let mut grouped_count_windows = Vec::with_capacity(3);
        for window_hours in [1_i64, 24, 24 * 7] {
            let grouped_since = now.saturating_sub(window_hours.saturating_mul(3600));
            let grouped_filters = AlertEventFilters {
                alert_type: None,
                since: Some(grouped_since),
                until: None,
                user_id: None,
                token_id: None,
                key_id: None,
                request_kinds: &[],
            };
            let grouped_count = if window_hours == clamped_window_hours {
                grouped_count
            } else {
                self.fetch_projected_alert_group_count(grouped_filters).await?
            };
            grouped_count_windows.push(RecentAlertsGroupedWindowCount {
                window_hours,
                grouped_count,
            });
        }
        let mut count_query = QueryBuilder::new(
            "SELECT json_extract(payload_json, '$.alert_type') AS alert_type, COUNT(*) AS count \
             FROM observability.dashboard_alert_projection_events WHERE occurred_at >= ",
        );
        count_query.push_bind(since);
        count_query.push(" GROUP BY json_extract(payload_json, '$.alert_type')");
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let result = count_query.build().fetch_all(&mut *conn).await;
        let count_rows = conn.complete_query(result).await?;
        let counts_by_type = Self::summarize_alert_type_count_rows(count_rows);
        let mut total_conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AlertProjection)
            .await?;
        let total_result = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM observability.dashboard_alert_projection_events WHERE occurred_at >= ?",
        )
        .bind(since)
        .fetch_one(&mut *total_conn)
        .await;
        let total_events = total_conn.complete_query(total_result).await?;
        Ok(RecentAlertsSummary {
            window_hours: clamped_window_hours,
            total_events,
            grouped_count,
            grouped_count_windows,
            counts_by_type,
            top_groups,
            coverage: status.recent_coverage.clone(),
            stale: status.recent_coverage != "ok" || status.stale_reason.is_some(),
            error: status.stale_reason.clone().or_else(|| {
                (status.recent_coverage != "ok").then(|| "projection_catching_up".to_string())
            }),
        })
    }
}

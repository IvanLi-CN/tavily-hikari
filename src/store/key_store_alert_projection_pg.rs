/// PostgreSQL branch of the alert projection writer.
///
/// The SQLite implementation stays in `key_store_alert_projection.rs`; it runs inside the runtime's
/// owned immediate transaction, which exists to serialise native SQLite writers. PostgreSQL needs
/// no such wrapper: the slice is one transaction and the state update is an optimistic compare on
/// `generation`.
use sqlx::Postgres;

use crate::models::ProxyError;

impl KeyStore {
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
        let mut tx: sqlx::Transaction<'_, Postgres> = self.begin_pg_write().await?;

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
                // Keep the pre-update row for an in-flight canonical build, exactly as the
                // SQLite branch does; `ON CONFLICT DO NOTHING` replaces `INSERT OR IGNORE`.
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
}

impl KeyStore {
    /// PostgreSQL twin of [`Self::alert_projection_status`]. It lands with the writer above: a
    /// read-only view of a SQLite-written projection would report `projecting` forever.
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
                    if Self::alert_projection_source_fence_pg(
                        &mut session,
                        &source_kind,
                        retention_since,
                    )
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

    /// PostgreSQL twin of [`Self::alert_projection_source_fence`]: one bounded seek per source.
    async fn alert_projection_source_fence_pg(
        session: &mut crate::store::PgReadSession,
        source_kind: &str,
        retention_since: i64,
    ) -> Result<Option<(i64, String)>, ProxyError> {
        let row = match source_kind {
            ALERT_SOURCE_AUTH_TOKEN_LOG => sqlx::query_as::<_, (i64, String)>(
                r#"SELECT created_at, id
                     FROM core.auth_token_logs
                    WHERE (failure_kind = 'upstream_rate_limited_429'
                           OR result_status = 'quota_exhausted')
                      AND created_at >= $1
                    ORDER BY created_at DESC, id DESC
                    LIMIT 1"#,
            )
            .bind(retention_since)
            .fetch_optional(&mut *session.connection())
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
            .fetch_optional(&mut *session.connection())
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
            .fetch_optional(&mut *session.connection())
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
}

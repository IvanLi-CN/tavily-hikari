/// PostgreSQL branches for the canonical alert Groups build controller.
///
/// The SQLite implementation in `key_store_alert_canonical_groups.rs` owns the build state
/// machine: clear, copy, aggregate, publish. These twins change the storage dialect only.
///
/// Two SQLite-only mechanisms need PostgreSQL equivalents:
///
/// - `rowid` bounded deletes become `ctid` bounded deletes. Both name a physical row, both are
///   valid only inside one statement, and both give the bounded-batch delete its "any 25 rows"
///   semantics without needing a schema column.
/// - `build_source_rowid_upper_bound` bounds the copy to rows that existed when the build started.
///   PostgreSQL keeps `projection_revision` on every projection row and bumps it per write, so the
///   bound is captured as that revision and the copy filters `projection_revision <= $n`. This is a
///   stricter fence than `rowid`: it also excludes rows rewritten after the snapshot.
impl KeyStore {
    async fn load_admin_alert_canonical_groups_state_pg(
        &self,
    ) -> Result<AdminAlertCanonicalGroupsState, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let row = sqlx::query(
            r#"SELECT active_generation, active_row_count, active_projection_revision,
                      source_recent_generation, source_history_generation,
                      build_generation, build_projection_revision,
                      build_source_recent_generation, build_source_history_generation,
                      build_phase,
                      build_source_rowid_upper_bound, build_cursor_occurred_at,
                      build_cursor_row_sort_id,
                      build_partition_key, build_partition_after_key,
                      build_partition_cursor_occurred_at, build_partition_cursor_row_sort_id,
                      build_partition_events_json,
                      build_partition_source_complete, build_partition_fragment_next_position,
                      build_partition_finalize_fragment_position,
                      build_next_position,
                      payload_read_generation, payload_read_position,
                      payload_read_chunk_position, payload_read_json
                 FROM observability.admin_alert_canonical_groups_state
                WHERE singleton = 1"#,
        )
        .fetch_optional(&mut *session.connection())
        .await?;
        session.finish().await?;
        let row = row.ok_or_else(|| {
            ProxyError::Other("canonical alert Groups state is unavailable".to_string())
        })?;
        Ok(AdminAlertCanonicalGroupsState {
            active_generation: row.try_get("active_generation")?,
            active_projection_revision: row.try_get("active_projection_revision")?,
            active_source_fence: (
                row.try_get("source_recent_generation")?,
                row.try_get("source_history_generation")?,
            ),
            build_generation: row.try_get("build_generation")?,
            build_projection_revision: row.try_get("build_projection_revision")?,
            build_source_fence: (
                row.try_get("build_source_recent_generation")?,
                row.try_get("build_source_history_generation")?,
            ),
            build_source_rowid_upper_bound: row.try_get("build_source_rowid_upper_bound")?,
            build_cursor_occurred_at: row.try_get("build_cursor_occurred_at")?,
            build_cursor_row_sort_id: row.try_get("build_cursor_row_sort_id")?,
            build_phase: row.try_get("build_phase")?,
            build_partition_key: row.try_get("build_partition_key")?,
            build_partition_after_key: row.try_get("build_partition_after_key")?,
            build_partition_cursor: (
                row.try_get("build_partition_cursor_occurred_at")?,
                row.try_get("build_partition_cursor_row_sort_id")?,
            ),
            build_partition_events_json: row.try_get("build_partition_events_json")?,
            build_partition_source_complete: row.try_get("build_partition_source_complete")?,
            build_partition_fragment_next_position: row
                .try_get("build_partition_fragment_next_position")?,
            build_partition_finalize_fragment_position: row
                .try_get("build_partition_finalize_fragment_position")?,
            build_next_position: row.try_get("build_next_position")?,
            payload_read_generation: row.try_get("payload_read_generation")?,
            payload_read_position: row.try_get("payload_read_position")?,
            payload_read_chunk_position: row.try_get("payload_read_chunk_position")?,
            payload_read_json: row.try_get("payload_read_json")?,
        })
    }

    async fn start_admin_alert_canonical_groups_build_pg(
        &self,
    ) -> Result<AdminAlertsCanonicalSnapshot, ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let (revision, recent, history, active, building) = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"SELECT
                   (SELECT revision FROM observability.dashboard_alert_projection_revision_state
                     WHERE singleton = 1),
                   (SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                      FROM observability.dashboard_alert_projection_state),
                   (SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                      FROM observability.dashboard_alert_projection_history_state),
                   active_generation,
                   build_generation
              FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1"#,
        )
        .fetch_one(&mut *tx)
        .await?;
        if building > 0 {
            let (projection_revision, recent_fence, history_fence) = sqlx::query_as::<_, (i64, i64, i64)>(
                "SELECT build_projection_revision, build_source_recent_generation, \
                        build_source_history_generation \
                   FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
            )
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await.map_err(ProxyError::Database)?;
            return Ok(AdminAlertsCanonicalSnapshot {
                build_generation: building,
                projection_revision,
                source_fence: (recent_fence, history_fence),
            });
        }
        let build_generation = match active {
            1 => 2,
            2 => 1,
            _ => 1,
        };
        sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_generation = $1, build_projection_revision = $2,
                      build_source_recent_generation = $3, build_source_history_generation = $4,
                      build_cursor_occurred_at = 9223372036854775807,
                      build_cursor_row_sort_id = chr(1114111),
                      build_source_rowid_upper_bound = $5,
                      build_cursor_source_rowid = 9223372036854775807,
                      build_phase = 'clearing',
                      build_partition_key = '', build_partition_after_key = '',
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_partition_finalize_fragment_position = 1,
                      build_next_position = 1,
                      payload_read_generation = 0,
                      payload_read_position = 0,
                      payload_read_chunk_position = 0,
                      payload_read_json = ''
                WHERE singleton = 1"#,
        )
        .bind(build_generation)
        .bind(revision)
        .bind(recent)
        .bind(history)
        .bind(revision)
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(AdminAlertsCanonicalSnapshot {
            build_generation,
            projection_revision: revision,
            source_fence: (recent, history),
        })
    }

    async fn discard_admin_alert_canonical_groups_build_pg(
        &self,
        state: &AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_generation = 0, build_projection_revision = -1,
                      build_source_recent_generation = -1,
                      build_source_history_generation = -1,
                      build_source_rowid_upper_bound = 0,
                      build_cursor_source_rowid = 0, build_phase = 'idle',
                      build_partition_key = '', build_partition_after_key = '',
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_partition_finalize_fragment_position = 1,
                      build_next_position = 1,
                      payload_read_generation = 0,
                      payload_read_position = 0,
                      payload_read_chunk_position = 0,
                      payload_read_json = ''
                WHERE singleton = 1 AND build_generation = $1
                  AND build_projection_revision = $2
                  AND build_source_recent_generation = $3
                  AND build_source_history_generation = $4"#,
        )
        .bind(state.build_generation)
        .bind(state.build_projection_revision)
        .bind(state.build_source_fence.0)
        .bind(state.build_source_fence.1)
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(ProxyError::Database)?;
        if changed.rows_affected() != 1 {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_defer();
        Ok(())
    }

    /// Mirrors `clear_admin_alert_canonical_groups_build_slot`: delete at most 25 rows per table
    /// per slice, then either report progress or advance the phase to `copying`.
    async fn clear_admin_alert_canonical_groups_build_slot_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        const CLEAR_TABLES: [&str; 13] = [
            "admin_alert_canonical_groups",
            "admin_alert_canonical_group_events",
            "admin_alert_canonical_group_overrides",
            "admin_alert_canonical_group_fragments",
            "admin_alert_canonical_group_reduction_events",
            "admin_alert_canonical_group_reduction_children",
            "admin_alert_canonical_group_reduction_mothers",
            "admin_alert_canonical_group_payload_chunks",
            "admin_alert_canonical_group_payload_read_chunks_v2",
            "admin_alert_canonical_catalog_facets",
            "admin_alert_canonical_catalog_payloads",
            "admin_alert_canonical_catalog_payload_items",
            "admin_alert_canonical_catalog_payload_items_v2",
        ];
        let mut tx = self.begin_pg_write().await?;
        let still_current: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
             WHERE singleton = 1 AND build_generation = $1 \
               AND build_projection_revision = $2 AND build_phase = 'clearing')",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .fetch_one(&mut *tx)
        .await?;
        if !still_current {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        for table in CLEAR_TABLES {
            let delete = format!(
                "DELETE FROM observability.{table} WHERE ctid IN ( \
                 SELECT ctid FROM observability.{table} \
                  WHERE build_generation = $1 LIMIT 25)"
            );
            sqlx::query(&delete)
                .bind(snapshot.build_generation)
                .execute(&mut *tx)
                .await?;
        }
        let mut remaining = false;
        for table in CLEAR_TABLES {
            let exists = format!(
                "SELECT EXISTS(SELECT 1 FROM observability.{table} WHERE build_generation = $1)"
            );
            if sqlx::query_scalar::<_, bool>(&exists)
                .bind(snapshot.build_generation)
                .fetch_one(&mut *tx)
                .await?
            {
                remaining = true;
                break;
            }
        }
        if !remaining {
            let changed = sqlx::query(
                r#"UPDATE observability.admin_alert_canonical_groups_state
                      SET build_phase = 'copying',
                          build_cursor_source_rowid = 0,
                          build_partition_key = '', build_partition_after_key = '',
                          build_partition_cursor_occurred_at = -9223372036854775808,
                          build_partition_cursor_row_sort_id = '',
                          build_partition_source_complete = 0,
                          build_partition_fragment_next_position = 1,
                          build_partition_finalize_fragment_position = 1,
                          build_next_position = 1,
                          payload_read_generation = 0,
                          payload_read_position = 0,
                          payload_read_chunk_position = 0,
                          payload_read_json = ''
                    WHERE singleton = 1 AND build_generation = $1
                      AND build_projection_revision = $2 AND build_phase = 'clearing'"#,
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .execute(&mut *tx)
            .await?;
            if changed.rows_affected() != 1 {
                tx.rollback().await.ok();
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_replaced".to_string(),
                });
            }
            sqlx::query(
                r#"UPDATE observability.admin_alert_canonical_catalog_state
                      SET build_generation = $1,
                          cursor_occurred_at = -9223372036854775808,
                          cursor_row_sort_id = '', source_complete = 0
                    WHERE singleton = 1"#,
            )
            .bind(snapshot.build_generation)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }
}

impl KeyStore {
    /// PostgreSQL twin of `copy_admin_alert_canonical_groups_snapshot_slice`.
    ///
    /// The snapshot fence is `projection_revision <= snapshot.projection_revision` instead of
    /// `rowid <= upper bound`: PostgreSQL has no rowid, and the revision column is the monotonic
    /// write counter the projection already maintains.
    async fn copy_admin_alert_canonical_groups_snapshot_slice_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        const READ_SLICE_ROWS: i64 = ADMIN_ALERT_CANONICAL_GROUPS_READ_SLICE_ROWS;
        const WRITE_SLICE_ROWS: usize = ADMIN_ALERT_CANONICAL_GROUPS_WRITE_SLICE_ROWS;
        let mut session = self.begin_pg_read_session().await?;
        let rows = sqlx::query(
            r#"SELECT current.source_kind AS source_kind,
                      current.source_id AS source_id,
                      current.occurred_at AS occurred_at,
                      current.row_sort_id AS row_sort_id,
                      current.payload_json AS payload_json
                 FROM observability.dashboard_alert_projection_events AS current
                WHERE current.projection_revision <= $1
                  AND current.occurred_at >= $2
                  AND (current.occurred_at, current.row_sort_id) < ($3, $4)
                ORDER BY current.occurred_at DESC, current.row_sort_id DESC
                LIMIT $5"#,
        )
        .bind(snapshot.projection_revision)
        .bind(self.alert_projection_retention_since())
        .bind(state.build_cursor_occurred_at)
        .bind(&state.build_cursor_row_sort_id)
        .bind(READ_SLICE_ROWS)
        .fetch_all(&mut *session.connection())
        .await?;
        session.finish().await?;
        let row_count = rows.len();
        let staged: Vec<(String, String, i64, String, String, String, String)> = rows
            .into_iter()
            .map(|row| {
                let source_kind = row.try_get::<String, _>("source_kind")?;
                let source_id = row.try_get::<String, _>("source_id")?;
                let occurred_at = row.try_get::<i64, _>("occurred_at")?;
                let cursor_row_sort_id = row.try_get::<String, _>("row_sort_id")?;
                let projection = Self::decode_default_alert_event_projection_row_pg(row)?;
                let payload_json = serialize_alert_event_projection_payload(projection.clone())?;
                let event = Self::build_alert_event_from_projection(projection);
                let partition_key =
                    event.as_ref().map_or_else(String::new, canonical_alert_group_partition_key);
                let row_sort_id = event
                    .as_ref()
                    .map(|event| event.id.clone())
                    .unwrap_or_else(|| cursor_row_sort_id.clone());
                Ok::<_, ProxyError>((
                    source_kind,
                    source_id,
                    occurred_at,
                    cursor_row_sort_id,
                    row_sort_id,
                    partition_key,
                    payload_json,
                ))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let complete = row_count < READ_SLICE_ROWS as usize;
        let chunk_count = if staged.is_empty() {
            1
        } else {
            staged.len().div_ceil(WRITE_SLICE_ROWS)
        };
        let mut expected_cursor = (
            state.build_cursor_occurred_at,
            state.build_cursor_row_sort_id.clone(),
        );
        for (chunk_index, chunk) in staged
            .chunks(WRITE_SLICE_ROWS)
            .chain((staged.is_empty()).then_some(&staged[..]))
            .enumerate()
        {
            let chunk_cursor = chunk
                .last()
                .map(|row| (row.2, row.3.clone()))
                .unwrap_or_else(|| expected_cursor.clone());
            let chunk_complete = chunk_index + 1 == chunk_count && complete;
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let chunk = chunk.to_vec();
            let prior_cursor = expected_cursor.clone();
            let committed_cursor = chunk_cursor.clone();
            let mut tx = self.begin_pg_write().await?;
            let valid: bool = sqlx::query_scalar(
                r#"SELECT EXISTS(
                     SELECT 1
                       FROM observability.admin_alert_canonical_groups_state
                      WHERE singleton = 1 AND build_generation = $1
                        AND build_projection_revision = $2
                        AND build_source_recent_generation = $3
                        AND build_source_history_generation = $4
                        AND build_cursor_occurred_at = $5
                        AND build_cursor_row_sort_id = $6
                        AND build_phase = 'copying'
                   )"#,
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(snapshot.source_fence.0)
            .bind(snapshot.source_fence.1)
            .bind(prior_cursor.0)
            .bind(&prior_cursor.1)
            .fetch_one(&mut *tx)
            .await?;
            if !valid {
                tx.rollback().await.ok();
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_replaced".to_string(),
                });
            }
            for (
                source_kind,
                source_id,
                occurred_at,
                _cursor_row_sort_id,
                row_sort_id,
                partition_key,
                payload_json,
            ) in chunk
            {
                sqlx::query(
                    r#"INSERT INTO observability.admin_alert_canonical_group_events
                           (build_generation, source_kind, source_id, occurred_at, row_sort_id,
                            partition_key, payload_json)
                       VALUES ($1, $2, $3, $4, $5, $6, $7)
                       ON CONFLICT (build_generation, source_kind, source_id)
                       DO UPDATE SET occurred_at = excluded.occurred_at,
                                     row_sort_id = excluded.row_sort_id,
                                     partition_key = excluded.partition_key,
                                     payload_json = excluded.payload_json"#,
                )
                .bind(snapshot.build_generation)
                .bind(source_kind)
                .bind(source_id)
                .bind(occurred_at)
                .bind(row_sort_id)
                .bind(partition_key)
                .bind(payload_json)
                .execute(&mut *tx)
                .await?;
            }
            let changed = sqlx::query(
                r#"UPDATE observability.admin_alert_canonical_groups_state
                      SET build_cursor_occurred_at = $1,
                          build_cursor_row_sort_id = $2,
                          build_phase = CASE WHEN $3 THEN 'aggregating' ELSE 'copying' END
                    WHERE singleton = 1 AND build_generation = $4
                      AND build_projection_revision = $5
                      AND build_cursor_occurred_at = $6
                      AND build_cursor_row_sort_id = $7
                      AND build_source_recent_generation = (
                          SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                            FROM observability.dashboard_alert_projection_state
                      )
                      AND build_source_history_generation = (
                          SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                            FROM observability.dashboard_alert_projection_history_state
                      )"#,
            )
            .bind(committed_cursor.0)
            .bind(&committed_cursor.1)
            .bind(chunk_complete)
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(prior_cursor.0)
            .bind(&prior_cursor.1)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if changed != 1 {
                tx.rollback().await.ok();
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_replaced".to_string(),
                });
            }
            tx.commit().await.map_err(ProxyError::Database)?;
            expected_cursor = chunk_cursor;
            self.record_admin_alerts_warm_slice();
            self.sqlite_runtime
                .record_admin_alerts_canonical_group_build_slice();
        }
        Ok(())
    }
}

impl KeyStore {
    /// PostgreSQL twin of `capture_admin_alert_canonical_groups_partition_slice`.
    async fn capture_admin_alert_canonical_groups_partition_slice_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        const CAPTURE_SLICE_ROWS: i64 = ADMIN_ALERT_CANONICAL_GROUPS_CAPTURE_SLICE_ROWS;
        let partition = state.build_partition_key.clone();
        let mut session = self.begin_pg_read_session().await?;
        let rows = sqlx::query(
            "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
             FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = $1 AND partition_key = $2 \
               AND (occurred_at > $3 OR (occurred_at = $4 AND row_sort_id > $5)) \
             ORDER BY occurred_at ASC, row_sort_id ASC LIMIT $6",
        )
        .bind(snapshot.build_generation)
        .bind(&partition)
        .bind(state.build_partition_cursor.0)
        .bind(state.build_partition_cursor.0)
        .bind(&state.build_partition_cursor.1)
        .bind(CAPTURE_SLICE_ROWS)
        .fetch_all(&mut *session.connection())
        .await?;
        session.finish().await?;
        let complete = rows.len() < CAPTURE_SLICE_ROWS as usize;
        let next_cursor = rows
            .last()
            .map(|row| {
                Ok::<_, sqlx::Error>((
                    row.try_get::<i64, _>("occurred_at")?,
                    row.try_get::<String, _>("row_sort_id")?,
                ))
            })
            .transpose()?
            .unwrap_or_else(|| state.build_partition_cursor.clone());
        let events = rows
            .into_iter()
            .map(Self::decode_default_alert_event_projection_row_pg)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(Self::build_alert_event_from_projection)
            .collect::<Vec<_>>();
        let fragments = canonical_alert_event_fragment_payloads(events)?;
        let fragment_position = state.build_partition_fragment_next_position;
        let fragment_count = fragments.len();
        let mut fragment_writes = Vec::with_capacity(fragment_count);
        for (offset, fragment_payload) in fragments.into_iter().enumerate() {
            let position = fragment_position + offset as i64;
            let (events_json, oversized_chunks) = match fragment_payload {
                CanonicalAlertFragmentPayload::Events(events_json) => (events_json, None),
                CanonicalAlertFragmentPayload::OversizedEventChunks(chunks) => {
                    let marker = format!(r#"{{"__canonical_event_chunks":{}}}"#, chunks.len());
                    (marker, Some(chunks))
                }
            };
            fragment_writes.push((position, events_json, oversized_chunks.unwrap_or_default()));
        }
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let next_fragment_position = fragment_position + fragment_count as i64;
        let mut tx = self.begin_pg_write().await?;
        for (position, events_json, chunks) in fragment_writes {
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_group_fragments
                       (build_generation, partition_key, position, events_json)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (build_generation, partition_key, position)
                   DO UPDATE SET events_json = excluded.events_json"#,
            )
            .bind(snapshot.build_generation)
            .bind(&partition)
            .bind(position)
            .bind(events_json)
            .execute(&mut *tx)
            .await?;
            for (chunk_position, payload_chunk) in chunks.into_iter().enumerate() {
                sqlx::query(
                    r#"INSERT INTO observability.admin_alert_canonical_group_payload_chunks
                           (build_generation, position, chunk_position, payload_chunk)
                       VALUES ($1, $2, $3, $4)
                       ON CONFLICT (build_generation, position, chunk_position)
                       DO UPDATE SET payload_chunk = excluded.payload_chunk"#,
                )
                .bind(snapshot.build_generation)
                .bind(-position)
                .bind(chunk_position as i64)
                .bind(payload_chunk)
                .execute(&mut *tx)
                .await?;
            }
        }
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_partition_cursor_occurred_at = $1,
                      build_partition_cursor_row_sort_id = $2,
                      build_partition_source_complete = $3,
                      build_partition_fragment_next_position = $4
                WHERE singleton = 1 AND build_generation = $5
                  AND build_projection_revision = $6 AND build_phase = 'aggregating'
                  AND build_partition_key = $7
                  AND build_partition_cursor_occurred_at = $8
                  AND build_partition_cursor_row_sort_id = $9
                  AND build_source_recent_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_state
                  )
                  AND build_source_history_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_history_state
                  )"#,
        )
        .bind(next_cursor.0)
        .bind(&next_cursor.1)
        .bind(complete)
        .bind(next_fragment_position)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(&partition)
        .bind(state.build_partition_cursor.0)
        .bind(&state.build_partition_cursor.1)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }

    /// PostgreSQL twin of the compat branch of
    /// `finalize_admin_alert_canonical_groups_partition`: advance the reduction state by one
    /// fragment.
    async fn finalize_admin_alert_canonical_groups_partition_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let Some((fragment_position, events)) = self
            .read_admin_alert_canonical_groups_partition_fragment_pg(
                snapshot,
                &state.build_partition_key,
                state.build_partition_finalize_fragment_position,
            )
            .await?
        else {
            return self
                .publish_admin_alert_canonical_compat_partition_pg(snapshot, state)
                .await;
        };
        if events.iter().any(|event| {
            matches!(
                event.alert_type.as_str(),
                ALERT_TYPE_USER_REQUEST_RATE_LIMITED | ALERT_TYPE_USER_QUOTA_EXHAUSTED
            ) && event.semantic_window.is_some()
        }) {
            return self
                .finalize_admin_alert_canonical_semantic_partition(snapshot, state)
                .await;
        }
        let current =
            serde_json::from_str::<CompatGroupReductionState>(&state.build_partition_events_json)
                .ok();
        let Some(latest_event) = events.last().cloned() else {
            return Err(ProxyError::Other(
                "canonical alert group fragment cannot be empty".to_string(),
            ));
        };
        let next = CompatGroupReductionState {
            event_count: current
                .as_ref()
                .map_or(events.len() as i64, |reduction| {
                    reduction.event_count + events.len() as i64
                }),
            first_seen: current
                .as_ref()
                .map_or_else(|| events[0].occurred_at, |reduction| reduction.first_seen),
            latest_event,
        };
        let reduction_json = serde_json::to_string(&next).map_err(|error| {
            ProxyError::Other(format!("serialize canonical compat reduction state: {error}"))
        })?;
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_partition_events_json = $1,
                      build_partition_finalize_fragment_position = $2
                WHERE singleton = 1 AND build_generation = $3
                  AND build_projection_revision = $4 AND build_phase = 'aggregating'
                  AND build_partition_key = $5
                  AND build_partition_finalize_fragment_position = $6
                  AND build_source_recent_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_state
                  )
                  AND build_source_history_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_history_state
                  )"#,
        )
        .bind(reduction_json)
        .bind(fragment_position + 1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(&state.build_partition_key)
        .bind(state.build_partition_finalize_fragment_position)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }
}

impl KeyStore {
    async fn reset_admin_alert_canonical_group_payload_read_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        sqlx::query(
            "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
              WHERE build_generation = $1 AND build_projection_revision = $2 \
                AND source_recent_generation = $3 AND source_history_generation = $4 \
                AND position = $5",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(position)
        .execute(&mut *tx)
        .await?;
        let changed = sqlx::query(
            "UPDATE observability.admin_alert_canonical_groups_state \
                SET payload_read_generation = $1, payload_read_position = $2, \
                    payload_read_chunk_position = 0, \
                    payload_read_json = '' \
              WHERE singleton = 1 \
                AND ((active_generation = $3 AND active_projection_revision = $4 \
                      AND source_recent_generation = $5 AND source_history_generation = $6 \
                      AND build_generation = 0) \
                  OR (build_generation = $7 AND build_projection_revision = $8 \
                      AND build_source_recent_generation = $9 \
                      AND build_source_history_generation = $10 \
                      AND build_phase = 'aggregating'))",
        )
        .bind(snapshot.build_generation)
        .bind(position)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await.map_err(ProxyError::Database)?;
        if changed != 1 {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn checkpoint_admin_alert_canonical_group_payload_read_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
        next_chunk_position: i64,
        chunks: Vec<(i64, String)>,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let owner: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
              WHERE singleton = 1 \
                AND ((active_generation = $1 AND active_projection_revision = $2 \
                      AND source_recent_generation = $3 AND source_history_generation = $4 \
                      AND build_generation = 0) \
                  OR (build_generation = $5 AND build_projection_revision = $6 \
                      AND build_source_recent_generation = $7 \
                      AND build_source_history_generation = $8 \
                      AND build_phase = 'aggregating')))",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .fetch_one(&mut *tx)
        .await?;
        if !owner {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_owner_lost".to_string(),
            });
        }
        for (chunk_position, payload_chunk) in chunks {
            sqlx::query(
                "INSERT INTO observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                 (build_generation, build_projection_revision, source_recent_generation, \
                  source_history_generation, position, chunk_position, payload_chunk) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (build_generation, build_projection_revision, \
                              source_recent_generation, source_history_generation, \
                              position, chunk_position) \
                 DO UPDATE SET payload_chunk = excluded.payload_chunk",
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(snapshot.source_fence.0)
            .bind(snapshot.source_fence.1)
            .bind(position)
            .bind(chunk_position)
            .bind(payload_chunk)
            .execute(&mut *tx)
            .await?;
        }
        let changed = sqlx::query(
            "UPDATE observability.admin_alert_canonical_groups_state \
                SET payload_read_chunk_position = $1 \
              WHERE singleton = 1 \
                AND ((active_generation = $2 AND active_projection_revision = $3 \
                      AND source_recent_generation = $4 AND source_history_generation = $5 \
                      AND build_generation = 0) \
                  OR (build_generation = $6 AND build_projection_revision = $7 \
                      AND build_source_recent_generation = $8 \
                      AND build_source_history_generation = $9 \
                      AND build_phase = 'aggregating')) \
                AND payload_read_generation = $10 AND payload_read_position = $11 \
                AND payload_read_chunk_position = $12",
        )
        .bind(next_chunk_position)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(position)
        .bind(prior_chunk_position)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(())
    }

    async fn mark_admin_alert_canonical_group_payload_read_ready_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let owner: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
              WHERE singleton = 1 \
                AND ((active_generation = $1 AND active_projection_revision = $2 \
                      AND source_recent_generation = $3 AND source_history_generation = $4 \
                      AND build_generation = 0) \
                  OR (build_generation = $5 AND build_projection_revision = $6 \
                      AND build_source_recent_generation = $7 \
                      AND build_source_history_generation = $8 \
                      AND build_phase = 'aggregating')))",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .fetch_one(&mut *tx)
        .await?;
        if !owner {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        let changed = sqlx::query(
            "UPDATE observability.admin_alert_canonical_groups_state \
                SET payload_read_chunk_position = $1, payload_read_json = '' \
              WHERE singleton = 1 \
                AND ((active_generation = $2 AND active_projection_revision = $3 \
                      AND source_recent_generation = $4 AND source_history_generation = $5 \
                      AND build_generation = 0) \
                  OR (build_generation = $6 AND build_projection_revision = $7 \
                      AND build_source_recent_generation = $8 \
                      AND build_source_history_generation = $9 \
                      AND build_phase = 'aggregating')) \
                AND payload_read_generation = $10 AND payload_read_position = $11 \
                AND payload_read_chunk_position = $12",
        )
        .bind(encode_payload_assembly_cursor(0, 0))
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(position)
        .bind(prior_chunk_position)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(())
    }
}

impl KeyStore {
    async fn read_admin_alert_canonical_groups_partition_fragment_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        partition_key: &str,
        position: i64,
    ) -> Result<Option<(i64, Vec<AlertEventRecord>)>, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let row = sqlx::query_as::<_, (i64, String)>(
            "SELECT position, events_json \
             FROM observability.admin_alert_canonical_group_fragments \
             WHERE build_generation = $1 AND partition_key = $2 AND position >= $3 \
             ORDER BY position ASC LIMIT 1",
        )
        .bind(snapshot.build_generation)
        .bind(partition_key)
        .bind(position)
        .fetch_optional(&mut *session.connection())
        .await?;
        session.finish().await?;
        let Some((position, events_json)) = row else {
            return Ok(None);
        };
        if let Ok(events) = serde_json::from_str::<Vec<AlertEventRecord>>(&events_json) {
            return Ok(Some((position, events)));
        }
        let marker =
            serde_json::from_str::<CanonicalAlertOversizedEventMarker>(&events_json).map_err(
                |_| ProxyError::Other("invalid canonical alert group fragment".to_string()),
            )?;
        if marker.chunk_count <= 0 {
            return Err(ProxyError::Other(
                "canonical oversized alert event has no chunks".to_string(),
            ));
        }
        let event_json = self
            .read_admin_alert_canonical_group_payload_chunks_pg(
                snapshot,
                -position,
                Some(marker.chunk_count),
            )
            .await?;
        let event = serde_json::from_str::<AlertEventRecord>(&event_json).map_err(|_| {
            ProxyError::Other("invalid canonical oversized alert event".to_string())
        })?;
        self.clear_admin_alert_canonical_group_payload_read_pg(snapshot, -position)
            .await?;
        Ok(Some((position, vec![event])))
    }

    async fn publish_admin_alert_canonical_compat_partition_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let reduction =
            serde_json::from_str::<CompatGroupReductionState>(&state.build_partition_events_json)
                .map_err(|_| {
                    ProxyError::Other(
                        "canonical compat reduction state is unavailable".to_string(),
                    )
                })?;
        let mut group = build_compat_group_record(std::slice::from_ref(&reduction.latest_event))
            .ok_or_else(|| {
                ProxyError::Other("canonical compat reduction has no latest event".to_string())
            })?;
        group.count = reduction.event_count;
        group.event_count = reduction.event_count;
        group.first_seen = reduction.first_seen;
        let payload_json = serde_json::to_string(&group).map_err(|error| {
            ProxyError::Other(format!("serialize canonical alert group: {error}"))
        })?;
        let position = state.build_next_position;
        let payload_chunks = canonical_alert_payload_chunks(&payload_json);
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        for (chunk_position, payload_chunk) in payload_chunks.into_iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO observability.admin_alert_canonical_group_payload_chunks
                       (build_generation, position, chunk_position, payload_chunk)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (build_generation, position, chunk_position)
                   DO UPDATE SET payload_chunk = excluded.payload_chunk"#,
            )
            .bind(snapshot.build_generation)
            .bind(position)
            .bind(chunk_position as i64)
            .bind(payload_chunk)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            r#"INSERT INTO observability.admin_alert_canonical_groups
                   (build_generation, position, last_seen, total_count,
                    alert_type, group_id, payload_json)
               VALUES ($1, $2, $3, $4, $5, $6, '')
               ON CONFLICT (build_generation, position)
               DO UPDATE SET last_seen = excluded.last_seen,
                             total_count = excluded.total_count,
                             alert_type = excluded.alert_type,
                             group_id = excluded.group_id,
                             payload_json = excluded.payload_json"#,
        )
        .bind(snapshot.build_generation)
        .bind(position)
        .bind(group.last_seen)
        .bind(group.count)
        .bind(group.alert_type.clone())
        .bind(group.id.clone())
        .execute(&mut *tx)
        .await?;
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_partition_key = '', build_partition_after_key = $1,
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_partition_finalize_fragment_position = 1,
                      build_next_position = $2
                WHERE singleton = 1 AND build_generation = $3
                  AND build_projection_revision = $4 AND build_phase = 'aggregating'
                  AND build_partition_key = $5
                  AND build_partition_finalize_fragment_position = $6
                  AND build_source_recent_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_state
                  )
                  AND build_source_history_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_history_state
                  )"#,
        )
        .bind(&state.build_partition_key)
        .bind(position + 1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(&state.build_partition_key)
        .bind(state.build_partition_finalize_fragment_position)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_reduction_slice();
        Ok(())
    }

    async fn select_admin_alert_canonical_groups_partition_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        state: AdminAlertCanonicalGroupsState,
    ) -> Result<(), ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let partition = sqlx::query_scalar::<_, String>(
            "SELECT partition_key FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = $1 AND partition_key > '' AND partition_key > $2 \
             ORDER BY partition_key ASC LIMIT 1",
        )
        .bind(snapshot.build_generation)
        .bind(&state.build_partition_after_key)
        .fetch_optional(&mut *session.connection())
        .await?;
        session.finish().await?;
        let Some(partition) = partition else {
            return self
                .publish_admin_alert_canonical_groups_snapshot_pg(
                    snapshot,
                    state.build_next_position - 1,
                )
                .await;
        };
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET build_partition_key = $1,
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_partition_finalize_fragment_position = 1
                WHERE singleton = 1 AND build_generation = $2
                  AND build_projection_revision = $3 AND build_phase = 'aggregating'
                  AND build_partition_key = ''"#,
        )
        .bind(partition)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_build_slice();
        Ok(())
    }
}

impl KeyStore {
    async fn publish_admin_alert_canonical_groups_snapshot_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        row_count: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let changed = sqlx::query(
            r#"UPDATE observability.admin_alert_canonical_groups_state
                  SET active_generation = $1, active_row_count = $2,
                      active_projection_revision = $3, source_recent_generation = $4,
                      source_history_generation = $5, build_generation = 0,
                      build_projection_revision = -1,
                      build_source_rowid_upper_bound = 0,
                      build_cursor_source_rowid = 0, build_phase = 'idle',
                      build_partition_key = '', build_partition_after_key = '',
                      build_partition_cursor_occurred_at = -9223372036854775808,
                      build_partition_cursor_row_sort_id = '',
                      build_partition_events_json = '[]',
                      build_partition_source_complete = 0,
                      build_partition_fragment_next_position = 1,
                      build_partition_finalize_fragment_position = 1,
                      build_next_position = 1,
                      payload_read_generation = 0,
                      payload_read_position = 0,
                      payload_read_chunk_position = 0,
                      payload_read_json = ''
                WHERE singleton = 1 AND build_generation = $6
                  AND build_projection_revision = $7
                  AND build_source_recent_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_state
                  )
                  AND build_source_history_generation = (
                      SELECT CAST(COALESCE(SUM(generation), 0) AS BIGINT)
                        FROM observability.dashboard_alert_projection_history_state
                  )"#,
        )
        .bind(snapshot.build_generation)
        .bind(row_count)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        self.sqlite_runtime
            .record_admin_alerts_canonical_group_publish();
        Ok(())
    }

    async fn checkpoint_admin_alert_canonical_group_payload_assembly_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
        next_chunk_position: i64,
        segment_position: i64,
        segment_payload: String,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let owner: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
              WHERE singleton = 1 \
                AND ((active_generation = $1 AND active_projection_revision = $2 \
                      AND source_recent_generation = $3 AND source_history_generation = $4 \
                      AND build_generation = 0) \
                  OR (build_generation = $5 AND build_projection_revision = $6 \
                      AND build_source_recent_generation = $7 \
                      AND build_source_history_generation = $8 \
                      AND build_phase = 'aggregating')))",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .fetch_one(&mut *tx)
        .await?;
        if !owner {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_owner_lost".to_string(),
            });
        }
        if !segment_payload.is_empty() {
            sqlx::query(
                "INSERT INTO observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                 (build_generation, build_projection_revision, source_recent_generation, \
                  source_history_generation, position, chunk_position, payload_chunk) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) \
                 ON CONFLICT (build_generation, build_projection_revision, \
                              source_recent_generation, source_history_generation, \
                              position, chunk_position) \
                 DO UPDATE SET payload_chunk = excluded.payload_chunk",
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(snapshot.source_fence.0)
            .bind(snapshot.source_fence.1)
            .bind(position)
            .bind(segment_position)
            .bind(segment_payload)
            .execute(&mut *tx)
            .await?;
        }
        let changed = sqlx::query(
            "UPDATE observability.admin_alert_canonical_groups_state \
                SET payload_read_chunk_position = $1 \
              WHERE singleton = 1 \
                AND ((active_generation = $2 AND active_projection_revision = $3 \
                      AND source_recent_generation = $4 AND source_history_generation = $5 \
                      AND build_generation = 0) \
                  OR (build_generation = $6 AND build_projection_revision = $7 \
                      AND build_source_recent_generation = $8 \
                      AND build_source_history_generation = $9 \
                      AND build_phase = 'aggregating')) \
                AND payload_read_generation = $10 AND payload_read_position = $11 \
                AND payload_read_chunk_position = $12",
        )
        .bind(next_chunk_position)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(position)
        .bind(prior_chunk_position)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(())
    }
}

impl KeyStore {
    /// PostgreSQL twin of `read_admin_alert_canonical_group_payload_chunks`. The assembly state
    /// machine is shape-identical; only the two chunk queries and the three checkpoint helpers
    /// change dialect.
    pub(crate) async fn read_admin_alert_canonical_group_payload_chunks_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        expected_chunks: Option<i64>,
    ) -> Result<String, ProxyError> {
        const PAYLOAD_CHUNK_READ_ROWS: i64 = 16;
        let state = self.load_admin_alert_canonical_groups_state_pg().await?;
        let state_matches = state.payload_read_generation == snapshot.build_generation
            && state.payload_read_position == position
            && ((state.build_generation == 0
                && state.active_generation == snapshot.build_generation
                && state.active_projection_revision == snapshot.projection_revision
                && state.active_source_fence == snapshot.source_fence)
                || (state.build_generation == snapshot.build_generation
                    && state.build_projection_revision == snapshot.projection_revision
                    && state.build_source_fence == snapshot.source_fence
                    && state.build_phase == "aggregating"));
        if !state_matches {
            self.reset_admin_alert_canonical_group_payload_read_pg(snapshot, position)
                .await?;
        }
        let state = if state_matches {
            state
        } else {
            self.load_admin_alert_canonical_groups_state_pg().await?
        };
        let mut next_chunk_position = state.payload_read_chunk_position;
        let legacy_assembly_position =
            (!state.payload_read_json.is_empty()).then_some(next_chunk_position);
        let mut assembling =
            state_matches && (next_chunk_position < 0 || legacy_assembly_position.is_some());

        if !assembling {
            let limit = expected_chunks
                .map(|expected| (expected - next_chunk_position).min(PAYLOAD_CHUNK_READ_ROWS))
                .unwrap_or(PAYLOAD_CHUNK_READ_ROWS);
            if limit <= 0 {
                return Err(ProxyError::Other(
                    "canonical alert group payload chunks are incomplete".to_string(),
                ));
            }
            let mut session = self.begin_pg_read_session().await?;
            let rows = sqlx::query_as::<_, (i64, String)>(
                "SELECT chunk_position, payload_chunk \
                   FROM observability.admin_alert_canonical_group_payload_chunks \
                  WHERE build_generation = $1 AND position = $2 AND chunk_position >= $3 \
                  ORDER BY chunk_position ASC LIMIT $4",
            )
            .bind(snapshot.build_generation)
            .bind(position)
            .bind(next_chunk_position)
            .bind(limit)
            .fetch_all(&mut *session.connection())
            .await?;
            session.finish().await?;
            if rows.is_empty() {
                if expected_chunks.is_none() && next_chunk_position > 0 {
                    self.mark_admin_alert_canonical_group_payload_read_ready_pg(
                        snapshot,
                        position,
                        next_chunk_position,
                    )
                    .await?;
                    next_chunk_position = encode_payload_assembly_cursor(0, 0);
                    assembling = true;
                } else {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks are incomplete".to_string(),
                    ));
                }
            }
            if !rows.is_empty() {
                let prior_chunk_position = next_chunk_position;
                for (chunk_position, _chunk) in &rows {
                    if *chunk_position != next_chunk_position {
                        return Err(ProxyError::Other(
                            "canonical alert group payload chunks are non-contiguous".to_string(),
                        ));
                    }
                    next_chunk_position += 1;
                }
                if expected_chunks.is_some_and(|expected| next_chunk_position > expected) {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks exceed marker".to_string(),
                    ));
                }
                let batch_len = rows.len();
                self.checkpoint_admin_alert_canonical_group_payload_read_pg(
                    snapshot,
                    position,
                    prior_chunk_position,
                    next_chunk_position,
                    rows.into_iter().collect(),
                )
                .await?;
                let source_complete = expected_chunks
                    .is_some_and(|expected| next_chunk_position >= expected)
                    || batch_len < PAYLOAD_CHUNK_READ_ROWS as usize;
                if !source_complete {
                    return Err(ProxyError::Deferred {
                        operation: "admin_alerts_cache_warm",
                        reason: "groups_build_in_progress".to_string(),
                    });
                }
                self.mark_admin_alert_canonical_group_payload_read_ready_pg(
                    snapshot,
                    position,
                    next_chunk_position,
                )
                .await?;
                next_chunk_position = encode_payload_assembly_cursor(0, 0);
                assembling = true;
            }
        }

        if assembling {
            let (assembly_level, assembly_position) = if legacy_assembly_position.is_some() {
                (0, 0)
            } else {
                decode_payload_assembly_cursor(next_chunk_position)
            };
            let mut session = self.begin_pg_read_session().await?;
            let source_level = assembly_level == 0;
            let source_position = if source_level {
                assembly_position
            } else {
                encode_payload_assembly_segment(assembly_level, assembly_position)
            };
            let source_upper_bound = if source_level {
                encode_payload_assembly_segment(1, 0)
            } else {
                encode_payload_assembly_segment(assembly_level, assembly_position + 16)
            };
            let rows = sqlx::query_as::<_, (i64, String)>(
                "SELECT chunk_position, payload_chunk \
                   FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                  WHERE build_generation = $1 AND build_projection_revision = $2 \
                    AND source_recent_generation = $3 AND source_history_generation = $4 \
                    AND position = $5 AND chunk_position >= $6 AND chunk_position < $7 \
                  ORDER BY chunk_position ASC LIMIT $8",
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(snapshot.source_fence.0)
            .bind(snapshot.source_fence.1)
            .bind(position)
            .bind(source_position)
            .bind(source_upper_bound)
            .bind(PAYLOAD_CHUNK_READ_ROWS)
            .fetch_all(&mut *session.connection())
            .await?;
            session.finish().await?;
            if rows.is_empty() {
                if assembly_level == 0 && assembly_position == 0 {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks are incomplete".to_string(),
                    ));
                }
                let next_cursor = encode_payload_assembly_cursor(assembly_level + 1, 0);
                self.checkpoint_admin_alert_canonical_group_payload_assembly_pg(
                    snapshot,
                    position,
                    next_chunk_position,
                    next_cursor,
                    encode_payload_assembly_segment(assembly_level + 1, 0),
                    String::new(),
                )
                .await?;
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_in_progress".to_string(),
                });
            }
            let mut after_position = assembly_position;
            let mut segment_payload = String::new();
            for (chunk_position, chunk) in &rows {
                let expected_position = if source_level {
                    after_position
                } else {
                    encode_payload_assembly_segment(assembly_level, after_position)
                };
                if *chunk_position != expected_position {
                    return Err(ProxyError::Other(format!(
                        "canonical alert group staged chunks are non-contiguous (expected {expected_position}, got {chunk_position})",
                    )));
                }
                segment_payload.push_str(chunk);
                after_position += 1;
            }
            if rows.len() == PAYLOAD_CHUNK_READ_ROWS as usize {
                let next_output_index = assembly_position / 16;
                let next_cursor = encode_payload_assembly_cursor(assembly_level, after_position);
                self.checkpoint_admin_alert_canonical_group_payload_assembly_pg(
                    snapshot,
                    position,
                    next_chunk_position,
                    next_cursor,
                    encode_payload_assembly_segment(assembly_level + 1, next_output_index),
                    segment_payload,
                )
                .await?;
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "groups_build_in_progress".to_string(),
                });
            }
            if assembly_level > 0 && assembly_position == 0 && rows.len() == 1 {
                return Ok(segment_payload);
            }
            let next_output_index = assembly_position / 16;
            let next_cursor = encode_payload_assembly_cursor(assembly_level + 1, 0);
            self.checkpoint_admin_alert_canonical_group_payload_assembly_pg(
                snapshot,
                position,
                next_chunk_position,
                next_cursor,
                encode_payload_assembly_segment(assembly_level + 1, next_output_index),
                segment_payload,
            )
            .await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_build_in_progress".to_string(),
            });
        }
        Err(ProxyError::Other(
            "canonical alert group payload chunks are incomplete".to_string(),
        ))
    }

    async fn read_admin_alert_canonical_group_payload_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<AlertGroupRecord, ProxyError> {
        let payload = self
            .read_admin_alert_canonical_group_payload_chunks_pg(snapshot, position, None)
            .await?;
        let group = serde_json::from_str(&payload)
            .map_err(|_| ProxyError::Other("invalid canonical alert group payload".to_string()))?;
        self.complete_admin_alert_canonical_group_payload(snapshot, position, payload)
            .await?;
        Ok(group)
    }

    async fn read_admin_alert_canonical_groups_model_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
    ) -> Result<PaginatedAlertGroups, ProxyError> {
        let mut session = self.begin_pg_read_session().await?;
        let state = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            "SELECT active_generation, active_row_count, active_projection_revision, \
                    source_recent_generation, source_history_generation \
             FROM observability.admin_alert_canonical_groups_state WHERE singleton = 1",
        )
        .fetch_optional(&mut *session.connection())
        .await?;
        let Some((generation, row_count, revision, recent, history)) = state else {
            session.finish().await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_model_unavailable".to_string(),
            });
        };
        if generation != snapshot.build_generation
            || revision != snapshot.projection_revision
            || (recent, history) != snapshot.source_fence
        {
            session.finish().await?;
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        let retention = self.alert_projection_retention_since();
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM observability.admin_alert_canonical_groups \
             WHERE build_generation = $1 AND position <= $2 AND last_seen >= $3",
        )
        .bind(generation)
        .bind(row_count)
        .bind(retention)
        .fetch_one(&mut *session.connection())
        .await?;
        let positions = sqlx::query_as::<_, (i64, String)>(
            "SELECT position, payload_json FROM observability.admin_alert_canonical_groups \
             WHERE build_generation = $1 AND position <= $2 AND last_seen >= $3 \
             ORDER BY last_seen DESC, total_count DESC, alert_type DESC, group_id DESC \
             LIMIT 20 OFFSET 0",
        )
        .bind(generation)
        .bind(row_count)
        .bind(retention)
        .fetch_all(&mut *session.connection())
        .await?;
        session.finish().await?;
        let mut items = Vec::with_capacity(positions.len());
        for (position, payload_json) in positions {
            if payload_json.is_empty() {
                items.push(
                    self.read_admin_alert_canonical_group_payload_pg(snapshot, position)
                        .await?,
                );
            } else {
                items.push(serde_json::from_str(&payload_json).map_err(|_| {
                    ProxyError::Other("invalid canonical alert group payload".to_string())
                })?);
            }
        }
        Ok(PaginatedAlertGroups {
            items,
            total,
            page: 1,
            per_page: 20,
        })
    }
}

impl KeyStore {
    async fn clear_admin_alert_canonical_group_payload_read_pg(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
        let changed = sqlx::query(
            "UPDATE observability.admin_alert_canonical_groups_state \
                SET payload_read_generation = 0, payload_read_position = 0, \
                    payload_read_chunk_position = 0, \
                    payload_read_json = '' \
              WHERE singleton = 1 \
                AND ((active_generation = $1 AND active_projection_revision = $2 \
                      AND source_recent_generation = $3 AND source_history_generation = $4 \
                      AND build_generation = 0) \
                  OR (build_generation = $5 AND build_projection_revision = $6 \
                      AND build_source_recent_generation = $7 \
                      AND build_source_history_generation = $8 \
                      AND build_phase = 'aggregating')) \
                AND payload_read_generation = $9 AND payload_read_position = $10",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(snapshot.build_generation)
        .bind(position)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        sqlx::query(
            "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
              WHERE build_generation = $1 AND build_projection_revision = $2 \
                AND source_recent_generation = $3 AND source_history_generation = $4 \
                AND position = $5",
        )
        .bind(snapshot.build_generation)
        .bind(snapshot.projection_revision)
        .bind(snapshot.source_fence.0)
        .bind(snapshot.source_fence.1)
        .bind(position)
        .execute(&mut *tx)
        .await?;
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(())
    }
}

impl KeyStore {
    pub(crate) async fn reclaim_admin_alert_canonical_groups_generations_pg(
        &self,
    ) -> Result<bool, ProxyError> {
        self.ensure_admin_alerts_cache_warm_reclaimer_write_admitted()?;
        let mut tx = self.begin_pg_write().await?;
                    let (active_generation, active_row_count, build_generation) = sqlx::query_as::<_, (i64, i64, i64)>(
                        "SELECT active_generation, active_row_count, build_generation \
                         FROM observability.admin_alert_canonical_groups_state \
                         WHERE singleton = 1",
                    )
                    .fetch_optional(&mut *tx)
                    .await?
                    .unwrap_or_default();
                    let cleanup_events = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_events
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_events
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, occurred_at ASC, row_sort_id ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_overrides = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_overrides
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_overrides
                                  WHERE build_generation <> $1
                                  ORDER BY build_generation ASC, source_kind ASC, source_id ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_fragments = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_fragments
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_fragments
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, partition_key ASC, position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_reduction_events = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_reduction_events
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_reduction_events
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, partition_key ASC, event_position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_reduction_children = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_reduction_children
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_reduction_children
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, partition_key ASC, child_ordinal ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_reduction_mothers = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_reduction_mothers
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_reduction_mothers
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, partition_key ASC, mother_ordinal ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_group_payload_chunks = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_payload_chunks
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_payload_chunks
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, position ASC, chunk_position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_group_payload_read_chunks = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_group_payload_read_chunks_v2
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, position ASC, chunk_position ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_facets
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_catalog_facets
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, facet_kind ASC, facet_value ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog_payloads = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_payloads
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_catalog_payloads
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, facet_kind ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog_payload_items = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_payload_items
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_catalog_payload_items
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, facet_kind ASC, facet_value ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_catalog_payload_items_v2 = sqlx::query(
                        r#"DELETE FROM observability.admin_alert_canonical_catalog_payload_items_v2
                             WHERE ctid IN (
                                 SELECT ctid
                                   FROM observability.admin_alert_canonical_catalog_payload_items_v2
                                  WHERE build_generation <> $1 AND build_generation <> $2
                                  ORDER BY build_generation ASC, facet_kind ASC, facet_value ASC,
                                           facet_label ASC
                                  LIMIT 25
                             )"#,
                    )
                    .bind(active_generation)
                    .bind(build_generation)
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                    let cleanup_remaining = cleanup_events > 0
                        || cleanup_overrides > 0
                        || cleanup_fragments > 0
                        || cleanup_reduction_events > 0
                        || cleanup_reduction_children > 0
                        || cleanup_reduction_mothers > 0
                        || cleanup_group_payload_chunks > 0
                        || cleanup_group_payload_read_chunks > 0
                        || cleanup_catalog > 0
                        || cleanup_catalog_payloads > 0
                        || cleanup_catalog_payload_items > 0
                        || cleanup_catalog_payload_items_v2 > 0
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_events \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_overrides \
                             WHERE build_generation <> $1)",
                        )
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_fragments \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_payload_chunks \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_reduction_events \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_reduction_children \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_group_reduction_mothers \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_facets \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_payloads \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_payload_items \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?
                        || sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_catalog_payload_items_v2 \
                             WHERE build_generation <> $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?;
                    let reusable_slot_active = matches!(active_generation, 1 | 2);
                    let reclaimed = if reusable_slot_active {
                        let legacy_deleted = sqlx::query(
                            r#"DELETE FROM observability.admin_alert_canonical_groups
                                 WHERE ctid IN (
                                     SELECT ctid
                                      FROM observability.admin_alert_canonical_groups
                                      WHERE build_generation > 2 AND build_generation <> $1
                                      ORDER BY build_generation ASC, position ASC
                                      LIMIT 25
                                 )"#,
                        )
                        .bind(build_generation)
                        .execute(&mut *tx)
                        .await?
                        .rows_affected();
                        if legacy_deleted == 0 {
                            let inactive_generation = if active_generation == 1 { 2 } else { 1 };
                            let inactive_deleted = sqlx::query(
                                r#"DELETE FROM observability.admin_alert_canonical_groups
                                     WHERE ctid IN (
                                         SELECT ctid
                                          FROM observability.admin_alert_canonical_groups
                                          WHERE build_generation = $1 AND build_generation <> $2
                                          ORDER BY position ASC
                                          LIMIT 25
                                     )"#,
                            )
                            .bind(inactive_generation)
                            .bind(build_generation)
                            .execute(&mut *tx)
                            .await?
                            .rows_affected();
                            if inactive_deleted == 0 {
                                sqlx::query(
                                    r#"DELETE FROM observability.admin_alert_canonical_groups
                                         WHERE ctid IN (
                                             SELECT ctid
                                               FROM observability.admin_alert_canonical_groups
                                              WHERE build_generation = $1 AND build_generation <> $2
                                                AND position > $3
                                              ORDER BY position ASC
                                              LIMIT 25
                                         )"#,
                                )
                                .bind(active_generation)
                                .bind(build_generation)
                                .bind(active_row_count)
                                .execute(&mut *tx)
                                .await?;
                            }
                        }
                        let inactive_generation = if active_generation == 1 { 2 } else { 1 };
                        let legacy_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation > 2 AND build_generation <> $1)",
                        )
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?;
                        let inactive_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation = $1 AND build_generation <> $2)",
                        )
                        .bind(inactive_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?;
                        let active_tail_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation = $1 AND build_generation <> $2 AND position > $3)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .bind(active_row_count)
                        .fetch_one(&mut *tx)
                        .await?;
                        legacy_remaining
                            || inactive_remaining
                            || active_tail_remaining
                            || cleanup_remaining
                    } else {
                        let retired_before_active = sqlx::query(
                            r#"DELETE FROM observability.admin_alert_canonical_groups
                                 WHERE ctid IN (
                                     SELECT ctid
                                      FROM observability.admin_alert_canonical_groups
                                      WHERE build_generation < $1 AND build_generation <> $2
                                      ORDER BY build_generation ASC, position ASC
                                      LIMIT 25
                                 )"#,
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .execute(&mut *tx)
                        .await?
                        .rows_affected();
                        if retired_before_active == 0 {
                            sqlx::query(
                                r#"DELETE FROM observability.admin_alert_canonical_groups
                                     WHERE ctid IN (
                                         SELECT ctid
                                           FROM observability.admin_alert_canonical_groups
                                          WHERE build_generation > $1 AND build_generation <> $2
                                          ORDER BY build_generation ASC, position ASC
                                          LIMIT 25
                                     )"#,
                            )
                            .bind(active_generation)
                            .bind(build_generation)
                            .execute(&mut *tx)
                            .await?;
                        }
                        let retired_before_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation < $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?;
                        let retired_after_remaining = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups \
                             WHERE build_generation > $1 AND build_generation <> $2)",
                        )
                        .bind(active_generation)
                        .bind(build_generation)
                        .fetch_one(&mut *tx)
                        .await?;
                        retired_before_remaining
                            || retired_after_remaining
                            || cleanup_remaining
                    };
        tx.commit().await.map_err(ProxyError::Database)?;
        Ok(reclaimed)
    }
}

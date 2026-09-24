impl KeyStore {
    async fn reset_admin_alert_canonical_group_payload_read(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    sqlx::query(
                        "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                          WHERE build_generation = ? AND build_projection_revision = ? \
                            AND source_recent_generation = ? AND source_history_generation = ? \
                            AND position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_generation = ?, payload_read_position = ?, \
                                payload_read_chunk_position = 0, \
                                payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) ",
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
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(result.rows_affected() == 1)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn checkpoint_admin_alert_canonical_group_payload_read(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
        next_chunk_position: i64,
        chunks: Vec<(i64, String)>,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating'))) ",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_owner_lost".to_string(),
                        });
                    }
                    for (chunk_position, payload_chunk) in chunks {
                        sqlx::query(
                            "INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                             (build_generation, build_projection_revision, source_recent_generation, \
                              source_history_generation, position, chunk_position, payload_chunk) \
                             VALUES (?, ?, ?, ?, ?, ?, ?)",
                        )
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(snapshot.source_fence.0)
                        .bind(snapshot.source_fence.1)
                        .bind(position)
                        .bind(chunk_position)
                        .bind(payload_chunk)
                        .execute(&mut **tx)
                        .await?;
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_chunk_position = ? \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ? \
                            AND payload_read_chunk_position = ?",
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
                    .execute(&mut **tx)
                    .await?;
                    if result.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn mark_admin_alert_canonical_group_payload_read_ready(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
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
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Ok::<_, ProxyError>(false);
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_chunk_position = ?, payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ? \
                            AND payload_read_chunk_position = ?",
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
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(result.rows_affected() == 1)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        Ok(())
    }

    async fn checkpoint_admin_alert_canonical_group_payload_assembly(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        prior_chunk_position: i64,
        next_chunk_position: i64,
        segment_position: i64,
        segment_payload: String,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating'))) ",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_owner_lost".to_string(),
                        });
                    }
                    if !segment_payload.is_empty() {
                        sqlx::query(
                            "INSERT OR REPLACE INTO observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                             (build_generation, build_projection_revision, source_recent_generation, \
                              source_history_generation, position, chunk_position, payload_chunk) \
                             VALUES (?, ?, ?, ?, ?, ?, ?)",
                        )
                        .bind(snapshot.build_generation)
                        .bind(snapshot.projection_revision)
                        .bind(snapshot.source_fence.0)
                        .bind(snapshot.source_fence.1)
                        .bind(position)
                        .bind(segment_position)
                        .bind(segment_payload)
                        .execute(&mut **tx)
                        .await?;
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_chunk_position = ? \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ? \
                            AND payload_read_chunk_position = ?",
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
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(result.rows_affected())
                })
            })
            .await?;
        if changed != 1 {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_snapshot_replaced".to_string(),
            });
        }
        Ok(())
    }

    async fn complete_admin_alert_canonical_group_payload(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        payload: String,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let owner = sqlx::query_scalar::<_, bool>(
                        "SELECT EXISTS(SELECT 1 FROM observability.admin_alert_canonical_groups_state \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
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
                    .fetch_one(&mut **tx)
                    .await?;
                    if !owner {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_owner_lost".to_string(),
                        });
                    }
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups \
                            SET payload_json = ? \
                          WHERE build_generation = ? AND position = ?",
                    )
                    .bind(payload)
                    .bind(snapshot.build_generation)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    if result.rows_affected() != 1 {
                        return Err(ProxyError::Other(
                            "canonical alert group payload row is unavailable".to_string(),
                        ));
                    }
                    sqlx::query(
                        "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                          WHERE build_generation = ? AND build_projection_revision = ? \
                            AND source_recent_generation = ? AND source_history_generation = ? \
                            AND position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    let state = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_generation = 0, payload_read_position = 0, \
                                payload_read_chunk_position = 0, payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ?",
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
                    .execute(&mut **tx)
                    .await?;
                    if state.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_replaced".to_string(),
                        });
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) async fn clear_admin_alert_canonical_group_payload_read(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
    ) -> Result<(), ProxyError> {
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let changed = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let result = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_groups_state \
                            SET payload_read_generation = 0, payload_read_position = 0, \
                                payload_read_chunk_position = 0, \
                                payload_read_json = '' \
                          WHERE singleton = 1 \
                            AND ((active_generation = ? AND active_projection_revision = ? \
                                  AND source_recent_generation = ? AND source_history_generation = ? \
                                  AND build_generation = 0) \
                              OR (build_generation = ? AND build_projection_revision = ? \
                                  AND build_source_recent_generation = ? \
                                  AND build_source_history_generation = ? \
                                  AND build_phase = 'aggregating')) \
                            AND payload_read_generation = ? AND payload_read_position = ?",
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
                    .execute(&mut **tx)
                    .await?;
                    if result.rows_affected() != 1 {
                        return Err(ProxyError::Deferred {
                            operation: "admin_alerts_cache_warm",
                            reason: "groups_snapshot_replaced".to_string(),
                        });
                    }
                    sqlx::query(
                        "DELETE FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                          WHERE build_generation = ? AND build_projection_revision = ? \
                            AND source_recent_generation = ? AND source_history_generation = ? \
                            AND position = ?",
                    )
                    .bind(snapshot.build_generation)
                    .bind(snapshot.projection_revision)
                    .bind(snapshot.source_fence.0)
                    .bind(snapshot.source_fence.1)
                    .bind(position)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !changed {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "groups_payload_read_state_changed".to_string(),
            });
        }
        Ok(())
    }

    pub(crate) async fn read_admin_alert_canonical_group_payload_chunks(
        &self,
        snapshot: AdminAlertsCanonicalSnapshot,
        position: i64,
        expected_chunks: Option<i64>,
    ) -> Result<String, ProxyError> {
        const PAYLOAD_CHUNK_READ_ROWS: i64 = 16;
        let state = self.load_admin_alert_canonical_groups_state().await?;
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
            self.reset_admin_alert_canonical_group_payload_read(snapshot, position)
                .await?;
        }

        let state = if state_matches {
            state
        } else {
            self.load_admin_alert_canonical_groups_state().await?
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
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
            let rows_result = sqlx::query_as::<_, (i64, String)>(
                "SELECT chunk_position, payload_chunk \
                   FROM observability.admin_alert_canonical_group_payload_chunks \
                  WHERE build_generation = ? AND position = ? AND chunk_position >= ? \
                  ORDER BY chunk_position ASC LIMIT ?",
            )
            .bind(snapshot.build_generation)
            .bind(position)
            .bind(next_chunk_position)
            .bind(limit)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(rows_result).await;
            let finish = session.finish().await;
            finish?;
            let rows = rows?;
            if rows.is_empty() {
                if expected_chunks.is_none() && next_chunk_position > 0 {
                    self.mark_admin_alert_canonical_group_payload_read_ready(
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
                self.checkpoint_admin_alert_canonical_group_payload_read(
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
                self.mark_admin_alert_canonical_group_payload_read_ready(
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
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?;
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
            let rows_result = sqlx::query_as::<_, (i64, String)>(
                "SELECT chunk_position, payload_chunk \
                   FROM observability.admin_alert_canonical_group_payload_read_chunks_v2 \
                  WHERE build_generation = ? AND build_projection_revision = ? \
                    AND source_recent_generation = ? AND source_history_generation = ? \
                    AND position = ? AND chunk_position >= ? AND chunk_position < ? \
                  ORDER BY chunk_position ASC LIMIT ?",
            )
            .bind(snapshot.build_generation)
            .bind(snapshot.projection_revision)
            .bind(snapshot.source_fence.0)
            .bind(snapshot.source_fence.1)
            .bind(position)
            .bind(source_position)
            .bind(source_upper_bound)
            .bind(PAYLOAD_CHUNK_READ_ROWS)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(rows_result).await;
            let finish = session.finish().await;
            finish?;
            let rows = rows?;
            if rows.is_empty() {
                if assembly_level == 0 && assembly_position == 0 {
                    return Err(ProxyError::Other(
                        "canonical alert group payload chunks are incomplete".to_string(),
                    ));
                }
                let next_cursor = encode_payload_assembly_cursor(assembly_level + 1, 0);
                self.checkpoint_admin_alert_canonical_group_payload_assembly(
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
                self.checkpoint_admin_alert_canonical_group_payload_assembly(
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
            self.checkpoint_admin_alert_canonical_group_payload_assembly(
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
}

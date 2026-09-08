impl KeyStore {
    pub(crate) async fn fetch_admin_alert_catalog_for_canonical_snapshot(
        &self,
        build_generation: i64,
    ) -> Result<AlertCatalog, ProxyError> {
        self.fetch_admin_alert_catalog_from_canonical_snapshot(build_generation)
            .await
    }

    async fn fetch_admin_alert_catalog_from_canonical_snapshot(
        &self,
        build_generation: i64,
    ) -> Result<AlertCatalog, ProxyError> {
        self.advance_admin_alert_canonical_catalog_snapshot(build_generation)
            .await?;
        self.advance_admin_alert_canonical_catalog_payload(build_generation)
            .await?;
        self.read_admin_alert_canonical_catalog_snapshot(build_generation)
            .await
    }

    async fn advance_admin_alert_canonical_catalog_snapshot(
        &self,
        build_generation: i64,
    ) -> Result<(), ProxyError> {
        const CATALOG_BUILD_SLICE_ROWS: i64 = 50;
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let state_result = sqlx::query_as::<_, (i64, i64, String, bool)>(
            "SELECT build_generation, cursor_occurred_at, cursor_row_sort_id, source_complete \
             FROM observability.admin_alert_canonical_catalog_state WHERE singleton = 1",
        )
        .fetch_optional(&mut *session)
        .await;
        let state = session.query(state_result).await?;
        session.finish().await?;
        let Some((state_generation, cursor_occurred_at, cursor_row_sort_id, source_complete)) = state else {
            return Err(ProxyError::Other(
                "canonical alert catalog state is unavailable".to_string(),
            ));
        };
        if state_generation != build_generation {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_snapshot_replaced".to_string(),
            });
        }
        if source_complete {
            return Ok(());
        }

        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let rows_result = sqlx::query(
            "SELECT source_kind, source_id, occurred_at, row_sort_id, payload_json \
             FROM observability.admin_alert_canonical_group_events \
             WHERE build_generation = ? \
               AND (occurred_at > ? OR (occurred_at = ? AND row_sort_id > ?)) \
             ORDER BY occurred_at ASC, row_sort_id ASC LIMIT ?",
        )
        .bind(build_generation)
        .bind(cursor_occurred_at)
        .bind(cursor_occurred_at)
        .bind(&cursor_row_sort_id)
        .bind(CATALOG_BUILD_SLICE_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(rows_result).await;
        session.finish().await?;
        let rows = rows?;
        let complete = rows.len() < CATALOG_BUILD_SLICE_ROWS as usize;
        let next_cursor = rows
            .last()
            .map(|row| {
                Ok::<_, sqlx::Error>((
                    row.try_get::<i64, _>("occurred_at")?,
                    row.try_get::<String, _>("row_sort_id")?,
                ))
            })
            .transpose()?
            .unwrap_or((cursor_occurred_at, cursor_row_sort_id.clone()));
        let mut facets = HashMap::<(String, String, String, String), i64>::new();
        for row in rows {
            let projection = Self::decode_default_alert_event_projection_row(row)?;
            let insert = |kind: &str,
                          identity: String,
                          value: String,
                          label: String,
                          facets: &mut HashMap<(String, String, String, String), i64>| {
                if !value.trim().is_empty() {
                    *facets
                        .entry((kind.to_string(), identity, value, label))
                        .or_default() += 1;
                }
            };
            insert(
                "type",
                projection.alert_type.trim().to_string(),
                projection.alert_type.trim().to_string(),
                projection.alert_type.trim().to_string(),
                &mut facets,
            );
            if let Some(request_kind) = projection.request_kind_key.as_deref() {
                let key = request_kind.trim();
                if !key.is_empty() && key != "unknown" {
                    insert(
                        "request_kind",
                        key.to_string(),
                        key.to_string(),
                        projection
                            .request_kind_label
                            .as_deref()
                            .unwrap_or(key)
                            .to_string(),
                        &mut facets,
                    );
                }
            }
            if let Some(user_id) = projection.user_id {
                let label = projection
                    .user_display_name
                    .or(projection.user_username)
                    .filter(|label| !label.trim().is_empty())
                    .unwrap_or_else(|| user_id.clone());
                insert(
                    "user",
                    format!("{user_id}\u{001f}{label}"),
                    user_id,
                    label,
                    &mut facets,
                );
            }
            if let Some(token_id) = projection.token_id {
                insert(
                    "token",
                    token_id.clone(),
                    token_id.clone(),
                    token_id,
                    &mut facets,
                );
            }
            if let Some(key_id) = projection.key_id {
                insert(
                    "key",
                    key_id.clone(),
                    key_id.clone(),
                    key_id,
                    &mut facets,
                );
            }
        }
        let facets = facets.into_iter().collect::<Vec<_>>();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let advanced = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    let changed = sqlx::query(
                        r#"UPDATE observability.admin_alert_canonical_catalog_state
                              SET cursor_occurred_at = ?, cursor_row_sort_id = ?, source_complete = ?
                            WHERE singleton = 1 AND build_generation = ?
                              AND cursor_occurred_at = ? AND cursor_row_sort_id = ?
                              AND ? = (SELECT active_generation
                                         FROM observability.admin_alert_canonical_groups_state
                                        WHERE singleton = 1)"#,
                    )
                    .bind(next_cursor.0)
                    .bind(next_cursor.1)
                    .bind(complete)
                    .bind(build_generation)
                    .bind(cursor_occurred_at)
                    .bind(cursor_row_sort_id)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?;
                    if changed.rows_affected() != 1 {
                        return Ok::<_, ProxyError>(false);
                    }
                    for ((facet_kind, facet_identity, facet_value, facet_label), item_count) in facets {
                        sqlx::query(
                            r#"INSERT INTO observability.admin_alert_canonical_catalog_facets
                                   (build_generation, facet_kind, facet_identity, facet_value, facet_label, item_count)
                               VALUES (?, ?, ?, ?, ?, ?)
                               ON CONFLICT(build_generation, facet_kind, facet_identity)
                               DO UPDATE SET item_count = item_count + excluded.item_count,
                                             facet_label = MIN(facet_label, excluded.facet_label)"#,
                        )
                        .bind(build_generation)
                        .bind(facet_kind)
                        .bind(facet_identity)
                        .bind(facet_value)
                        .bind(facet_label)
                        .bind(item_count)
                        .execute(&mut **tx)
                        .await?;
                    }
                    Ok::<_, ProxyError>(true)
                })
            })
            .await?;
        if !advanced {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_snapshot_replaced".to_string(),
            });
        }
        self.record_admin_alerts_warm_slice();
        if complete {
            Ok(())
        } else {
            Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_build_in_progress".to_string(),
            })
        }
    }

    async fn advance_admin_alert_canonical_catalog_payload(
        &self,
        build_generation: i64,
    ) -> Result<(), ProxyError> {
        const CATALOG_FACET_KINDS: [&str; 5] = ["type", "request_kind", "user", "token", "key"];
        const CATALOG_PAYLOAD_SLICE_ROWS: i64 = 250;

        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let state_result = sqlx::query_as::<_, (i64, bool)>(
            "SELECT build_generation, source_complete \
             FROM observability.admin_alert_canonical_catalog_state WHERE singleton = 1",
        )
        .fetch_optional(&mut *session)
        .await;
        let payloads_result = sqlx::query_as::<_, (String, String)>(
            "SELECT facet_kind, payload_status \
             FROM observability.admin_alert_canonical_catalog_payloads \
             WHERE build_generation = ?",
        )
        .bind(build_generation)
        .fetch_all(&mut *session)
        .await;
        let state = session.query(state_result).await?;
        let payloads = session.query(payloads_result).await?;
        session.finish().await?;
        let Some((state_generation, source_complete)) = state else {
            return Err(ProxyError::Other(
                "canonical alert catalog state is unavailable".to_string(),
            ));
        };
        if state_generation != build_generation {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_snapshot_replaced".to_string(),
            });
        }
        if !source_complete {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_build_in_progress".to_string(),
            });
        }

        let payload_statuses = payloads.into_iter().collect::<HashMap<_, _>>();
        let Some(facet_kind) = CATALOG_FACET_KINDS
            .into_iter()
            .find(|kind| payload_statuses.get(*kind).is_none_or(|status| status != "complete"))
        else {
            return Ok(());
        };
        let Some((cursor_item_count, cursor_label, cursor_value, payload_status)) = self
            .read_admin_alert_canonical_catalog_payload_state(build_generation, facet_kind)
            .await?
        else {
            self.ensure_admin_alerts_cache_warm_write_admitted()?;
            let inserted = self
                .sqlite_runtime
                .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                    Box::pin(async move {
                        let changed = sqlx::query(
                            "INSERT OR IGNORE INTO observability.admin_alert_canonical_catalog_payloads \
                             (build_generation, facet_kind) \
                             SELECT ?, ? WHERE ? = (SELECT active_generation \
                                                     FROM observability.admin_alert_canonical_groups_state \
                                                    WHERE singleton = 1)",
                        )
                        .bind(build_generation)
                        .bind(facet_kind)
                        .bind(build_generation)
                        .execute(&mut **tx)
                        .await?;
                        Ok::<_, ProxyError>(changed.rows_affected() == 1)
                    })
                })
                .await?;
            if !inserted {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "catalog_snapshot_replaced".to_string(),
                });
            }
            self.record_admin_alerts_warm_slice();
            self.sqlite_runtime
                .record_admin_alerts_canonical_catalog_payload_slice();
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_payload_build_in_progress".to_string(),
            });
        };
        if payload_status == "complete" {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_payload_build_in_progress".to_string(),
            });
        }

        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let rows_result = sqlx::query_as::<_, (String, String, i64)>(
            "SELECT facet_value, facet_label, item_count \
             FROM observability.admin_alert_canonical_catalog_facets \
             WHERE build_generation = ? AND facet_kind = ? \
               AND (item_count < ? OR (item_count = ? AND \
                    (facet_label > ? OR (facet_label = ? AND facet_value > ?)))) \
             ORDER BY item_count DESC, facet_label ASC, facet_value ASC LIMIT ?",
        )
        .bind(build_generation)
        .bind(facet_kind)
        .bind(cursor_item_count)
        .bind(cursor_item_count)
        .bind(&cursor_label)
        .bind(&cursor_label)
        .bind(&cursor_value)
        .bind(CATALOG_PAYLOAD_SLICE_ROWS)
        .fetch_all(&mut *session)
        .await;
        let rows = session.query(rows_result).await?;
        session.finish().await?;
        let complete = rows.len() < CATALOG_PAYLOAD_SLICE_ROWS as usize;
        let next_cursor = rows
            .last()
            .map(|(value, label, count)| (*count, label.clone(), value.clone()))
            .unwrap_or((cursor_item_count, cursor_label.clone(), cursor_value.clone()));
        let next_status = if complete {
            "complete"
        } else {
            "building"
        };
        let facet_kind = facet_kind.to_string();
        self.ensure_admin_alerts_cache_warm_write_admitted()?;
        let advanced = self
            .sqlite_runtime
            .run_owned_immediate(SqliteOperation::AlertProjection, move |tx| {
                Box::pin(async move {
                    for (facet_value, facet_label, item_count) in rows {
                        sqlx::query(
                            "INSERT OR REPLACE INTO observability.admin_alert_canonical_catalog_payload_items \
                             (build_generation, facet_kind, facet_value, facet_label, item_count) \
                             VALUES (?, ?, ?, ?, ?)",
                        )
                        .bind(build_generation)
                        .bind(&facet_kind)
                        .bind(facet_value)
                        .bind(facet_label)
                        .bind(item_count)
                        .execute(&mut **tx)
                        .await?;
                    }
                    let changed = sqlx::query(
                        "UPDATE observability.admin_alert_canonical_catalog_payloads \
                         SET cursor_item_count = ?, cursor_label = ?, cursor_value = ?, \
                             payload_status = ? \
                     WHERE build_generation = ? AND facet_kind = ? \
                       AND cursor_item_count = ? AND cursor_label = ? AND cursor_value = ? \
                       AND payload_status = 'building' \
                       AND ? = (SELECT active_generation \
                                  FROM observability.admin_alert_canonical_groups_state \
                                 WHERE singleton = 1)",
                    )
                    .bind(next_cursor.0)
                    .bind(next_cursor.1)
                    .bind(next_cursor.2)
                    .bind(next_status)
                    .bind(build_generation)
                    .bind(facet_kind)
                    .bind(cursor_item_count)
                    .bind(cursor_label)
                    .bind(cursor_value)
                    .bind(build_generation)
                    .execute(&mut **tx)
                    .await?;
                    Ok::<_, ProxyError>(changed.rows_affected() == 1)
                })
            })
            .await?;
        if !advanced {
            return Err(ProxyError::Deferred {
                operation: "admin_alerts_cache_warm",
                reason: "catalog_snapshot_replaced".to_string(),
            });
        }
        self.record_admin_alerts_warm_slice();
        self.sqlite_runtime
            .record_admin_alerts_canonical_catalog_payload_slice();
        Err(ProxyError::Deferred {
            operation: "admin_alerts_cache_warm",
            reason: "catalog_payload_build_in_progress".to_string(),
        })
    }

    async fn read_admin_alert_canonical_catalog_payload_state(
        &self,
        build_generation: i64,
        facet_kind: &str,
    ) -> Result<Option<(i64, String, String, String)>, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let result = sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT cursor_item_count, cursor_label, cursor_value, payload_status \
             FROM observability.admin_alert_canonical_catalog_payloads \
             WHERE build_generation = ? AND facet_kind = ?",
        )
        .bind(build_generation)
        .bind(facet_kind)
        .fetch_optional(&mut *session)
        .await;
        let state = session.query(result).await?;
        session.finish().await?;
        Ok(state)
    }

    async fn read_admin_alert_canonical_catalog_snapshot(
        &self,
        build_generation: i64,
    ) -> Result<AlertCatalog, ProxyError> {
        let mut session = self
            .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
            .await?;
        let payloads_result = sqlx::query_as::<_, (String, String)>(
            "SELECT facet_kind, payload_status \
             FROM observability.admin_alert_canonical_catalog_payloads \
             WHERE build_generation = ?",
        )
        .bind(build_generation)
        .fetch_all(&mut *session)
        .await;
        let payloads = session.query(payloads_result).await?;
        session.finish().await?;
        let payload_statuses = payloads.into_iter().collect::<HashMap<_, _>>();
        for facet_kind in ["type", "request_kind", "user", "token", "key"] {
            let Some(payload_status) = payload_statuses.get(facet_kind) else {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "catalog_payload_build_in_progress".to_string(),
                });
            };
            if payload_status != "complete" {
                return Err(ProxyError::Deferred {
                    operation: "admin_alerts_cache_warm",
                    reason: "catalog_payload_build_in_progress".to_string(),
                });
            }
        }
        let type_rows = self
            .read_admin_alert_canonical_catalog_payload_items(build_generation, "type")
            .await?;
        let request_kind_rows = self
            .read_admin_alert_canonical_catalog_payload_items(build_generation, "request_kind")
            .await?;
        let user_rows = self
            .read_admin_alert_canonical_catalog_payload_items(build_generation, "user")
            .await?;
        let token_rows = self
            .read_admin_alert_canonical_catalog_payload_items(build_generation, "token")
            .await?;
        let key_rows = self
            .read_admin_alert_canonical_catalog_payload_items(build_generation, "key")
            .await?;
        let type_counts = type_rows
            .into_iter()
            .map(|(value, _, count)| (value, count))
            .collect::<HashMap<_, _>>();
        let mut types = default_alert_type_counts();
        for item in &mut types {
            item.count = type_counts.get(&item.alert_type).copied().unwrap_or_default();
        }
        let request_kind_options = request_kind_rows
            .into_iter()
            .map(|(key, label, count)| TokenRequestKindOption {
                protocol_group: token_request_kind_protocol_group(&key).to_string(),
                billing_group: token_request_kind_billing_group(&key).to_string(),
                key,
                label,
                count,
            })
            .collect();
        let facet_options = |rows: Vec<(String, String, i64)>| {
            rows.into_iter()
                .map(|(value, label, count)| AlertFacetOption { value, label, count })
                .collect()
        };
        Ok(AlertCatalog {
            retention_days: self
                .effective_auth_token_log_retention_days_for_operation(
                    SqliteOperation::AdminAlertsCacheWarm,
                )
                .await?,
            types: types
                .into_iter()
                .map(|item| LogFacetOption {
                    value: item.alert_type,
                    count: item.count,
                })
                .collect(),
            request_kind_options,
            users: facet_options(user_rows),
            tokens: facet_options(token_rows),
            keys: facet_options(key_rows),
        })
    }

    async fn read_admin_alert_canonical_catalog_payload_items(
        &self,
        build_generation: i64,
        facet_kind: &str,
    ) -> Result<Vec<(String, String, i64)>, ProxyError> {
        const CATALOG_PAYLOAD_SLICE_ROWS: i64 = 250;
        let mut cursor = (i64::MAX, String::new(), String::new());
        let mut items = Vec::new();
        loop {
            let mut session = self
                .begin_admin_alerts_read_session_for_operation(SqliteOperation::AdminAlertsCacheWarm)
                .await?;
            let result = sqlx::query_as::<_, (String, String, i64)>(
                "SELECT facet_value, facet_label, item_count \
                 FROM observability.admin_alert_canonical_catalog_payload_items \
                 WHERE build_generation = ? AND facet_kind = ? \
                   AND (item_count < ? OR (item_count = ? AND \
                        (facet_label > ? OR (facet_label = ? AND facet_value > ?)))) \
                 ORDER BY item_count DESC, facet_label ASC, facet_value ASC LIMIT ?",
            )
            .bind(build_generation)
            .bind(facet_kind)
            .bind(cursor.0)
            .bind(cursor.0)
            .bind(&cursor.1)
            .bind(&cursor.1)
            .bind(&cursor.2)
            .bind(CATALOG_PAYLOAD_SLICE_ROWS)
            .fetch_all(&mut *session)
            .await;
            let rows = session.query(result).await;
            session.finish().await?;
            let rows = rows?;
            let complete = rows.len() < CATALOG_PAYLOAD_SLICE_ROWS as usize;
            if let Some((value, label, count)) = rows.last() {
                cursor = (*count, label.clone(), value.clone());
            }
            items.extend(rows);
            if complete {
                return Ok(items);
            }
        }
    }
}

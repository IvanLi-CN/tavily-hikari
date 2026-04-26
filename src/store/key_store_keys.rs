impl KeyStore {
    pub async fn fetch_key_summary_since(
        &self,
        key_id: &str,
        since: i64,
    ) -> Result<ProxySummary, ProxyError> {
        // `api_key_usage_buckets.bucket_start` is aligned to *server-local midnight* (stored as UTC ts).
        // Callers might pass `since` aligned to UTC midnight (e.g. from browser). Normalize so daily
        // bucket queries remain correct under non-UTC server timezones.
        let since_bucket_start = local_day_bucket_start_utc_ts(since);

        let totals_row = sqlx::query(
            r#"
            SELECT
              COALESCE(SUM(total_requests), 0) AS total_requests,
              COALESCE(SUM(success_count), 0) AS success_count,
              COALESCE(SUM(error_count), 0) AS error_count,
              COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE api_key_id = ? AND bucket_secs = 86400 AND bucket_start >= ?
            "#,
        )
        .bind(key_id)
        .bind(since_bucket_start)
        .fetch_one(&self.pool)
        .await?;

        // Active/exhausted counts in this scope are not meaningful per single key; expose 1/0 for convenience
        // We will compute based on current key status
        let status: Option<String> =
            sqlx::query_scalar("SELECT status FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;

        let key_last_used_at: Option<i64> =
            sqlx::query_scalar("SELECT last_used_at FROM api_keys WHERE id = ? LIMIT 1")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?;
        let last_activity = key_last_used_at
            .and_then(normalize_timestamp)
            .filter(|ts| *ts >= since_bucket_start);

        let quarantined = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM api_key_quarantines
            WHERE key_id = ? AND cleared_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await?
        .is_some();

        let (active_keys, exhausted_keys, quarantined_keys) = if quarantined {
            (0, 0, 1)
        } else {
            match status.as_deref() {
                Some(STATUS_EXHAUSTED) => (0, 1, 0),
                _ => (1, 0, 0),
            }
        };

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys,
            exhausted_keys,
            quarantined_keys,
            last_activity,
            total_quota_limit: 0,
            total_quota_remaining: 0,
        })
    }

    pub async fn fetch_key_logs(
        &self,
        key_id: &str,
        limit: usize,
        since: Option<i64>,
    ) -> Result<Vec<RequestLogRecord>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = if let Some(since_ts) = since {
            sqlx::query(
                r#"
                SELECT id, api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code, error_message,
                       result_status, request_kind_key, request_kind_label, request_kind_detail,
                       business_credits, failure_kind, key_effect_code, key_effect_summary,
                binding_effect_code, binding_effect_summary,
                selection_effect_code, selection_effect_summary,
                gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                upstream_operation, fallback_reason,
                       request_body, response_body, created_at, forwarded_headers, dropped_headers
                FROM request_logs
                WHERE api_key_id = ? AND visibility = ? AND created_at >= ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
            .bind(key_id)
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(since_ts)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, api_key_id, auth_token_id, method, path, query, status_code, tavily_status_code, error_message,
                       result_status, request_kind_key, request_kind_label, request_kind_detail,
                       business_credits, failure_kind, key_effect_code, key_effect_summary,
                binding_effect_code, binding_effect_summary,
                selection_effect_code, selection_effect_summary,
                gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                upstream_operation, fallback_reason,
                       request_body, response_body, created_at, forwarded_headers, dropped_headers
                FROM request_logs
                WHERE api_key_id = ? AND visibility = ?
                ORDER BY created_at DESC
                LIMIT ?
                "#,
            )
            .bind(key_id)
            .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(Self::map_request_log_row)
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) async fn sync_keys(&self, keys: &[String]) -> Result<(), ProxyError> {
        let mut tx = self.pool.begin().await?;

        let now = Utc::now().timestamp();

        for key in keys {
            // If key exists, undelete by clearing deleted_at
            if let Some((id, deleted_at)) = sqlx::query_as::<_, (String, Option<i64>)>(
                "SELECT id, deleted_at FROM api_keys WHERE api_key = ? LIMIT 1",
            )
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?
            {
                if deleted_at.is_some() {
                    sqlx::query("UPDATE api_keys SET deleted_at = NULL WHERE id = ?")
                        .bind(id)
                        .execute(&mut *tx)
                        .await?;
                }
                continue;
            }

            let id = Self::generate_unique_key_id(&mut tx).await?;
            sqlx::query(
                r#"
                INSERT INTO api_keys (id, api_key, status, created_at, status_changed_at)
                VALUES (?, ?, ?, ?, ?)
                "#,
            )
            .bind(&id)
            .bind(key)
            .bind(STATUS_ACTIVE)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        // Soft delete any keys not present in the provided set
        if keys.is_empty() {
            sqlx::query("UPDATE api_keys SET deleted_at = ? WHERE deleted_at IS NULL")
                .bind(now)
                .execute(&mut *tx)
                .await?;
        } else {
            let mut builder = QueryBuilder::new("UPDATE api_keys SET deleted_at = ");
            builder.push_bind(now);
            builder.push(" WHERE deleted_at IS NULL AND api_key NOT IN (");
            {
                let mut separated = builder.separated(", ");
                for key in keys {
                    separated.push_bind(key);
                }
            }
            builder.push(")");
            builder.build().execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn acquire_key(&self) -> Result<ApiKeyLease, ProxyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();
        let month_start = start_of_month(Utc::now()).timestamp();

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_low_quota_depletions d
                  WHERE d.key_id = api_keys.id AND d.month_start = ?
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY last_used_at ASC, id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(month_start)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_low_quota_depletions d
                  WHERE d.key_id = api_keys.id AND d.month_start = ?
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY
                CASE WHEN status_changed_at IS NULL THEN 1 ELSE 0 END ASC,
                status_changed_at ASC,
                id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        if let Some((id, api_key)) = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND EXISTS (
                  SELECT 1
                  FROM api_key_low_quota_depletions d
                  WHERE d.key_id = api_keys.id AND d.month_start = ?
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY
                CASE WHEN status_changed_at IS NULL THEN 1 ELSE 0 END ASC,
                status_changed_at ASC,
                id ASC
            LIMIT 1
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .fetch_optional(&self.pool)
        .await?
        {
            self.touch_key(&api_key, now).await?;
            return Ok(ApiKeyLease {
                id,
                secret: api_key,
            });
        }

        Err(ProxyError::NoAvailableKeys)
    }

    pub(crate) async fn list_mcp_session_candidate_key_ids(
        &self,
    ) -> Result<Vec<String>, ProxyError> {
        self.reset_monthly().await?;
        let month_start = start_of_month(Utc::now()).timestamp();

        let active = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_low_quota_depletions d
                  WHERE d.key_id = api_keys.id AND d.month_start = ?
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY id ASC
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(month_start)
        .fetch_all(&self.pool)
        .await?;
        if !active.is_empty() {
            return Ok(active);
        }

        let exhausted = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_low_quota_depletions d
                  WHERE d.key_id = api_keys.id AND d.month_start = ?
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY id ASC
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .fetch_all(&self.pool)
        .await?;
        if !exhausted.is_empty() {
            return Ok(exhausted);
        }

        sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE status = ? AND deleted_at IS NULL
              AND EXISTS (
                  SELECT 1
                  FROM api_key_low_quota_depletions d
                  WHERE d.key_id = api_keys.id AND d.month_start = ?
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            ORDER BY id ASC
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(month_start)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn try_acquire_specific_key(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyLease>, ProxyError> {
        self.reset_monthly().await?;
        if let Some(lease) = self
            .try_acquire_specific_key_with_status(key_id, STATUS_ACTIVE, false)
            .await?
        {
            return Ok(Some(lease));
        }

        self.try_acquire_specific_key_with_status(key_id, STATUS_EXHAUSTED, true)
            .await
    }

    pub(crate) async fn try_acquire_affinity_specific_key(
        &self,
        key_id: &str,
    ) -> Result<Option<ApiKeyLease>, ProxyError> {
        self.reset_monthly().await?;

        if let Some(lease) = self
            .try_acquire_specific_key_with_status(key_id, STATUS_ACTIVE, false)
            .await?
        {
            return Ok(Some(lease));
        }

        if self.has_available_active_key_excluding(None).await? {
            return Ok(None);
        }

        if let Some(lease) = self
            .try_acquire_specific_key_with_status(key_id, STATUS_EXHAUSTED, false)
            .await?
        {
            return Ok(Some(lease));
        }

        if self.has_available_regular_exhausted_key_excluding(Some(key_id)).await? {
            return Ok(None);
        }

        self.try_acquire_specific_key_with_status(key_id, STATUS_EXHAUSTED, true)
            .await
    }

    async fn try_acquire_specific_key_with_status(
        &self,
        key_id: &str,
        status: &str,
        allow_low_quota_depleted: bool,
    ) -> Result<Option<ApiKeyLease>, ProxyError> {
        let now = Utc::now().timestamp();
        let month_start = start_of_month(Utc::now()).timestamp();

        let lease = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT id, api_key
            FROM api_keys
            WHERE id = ? AND status = ? AND deleted_at IS NULL
              AND (
                  ? = 1
                  OR NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines q
                  WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
              )
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .bind(status)
        .bind(if allow_low_quota_depleted { 1 } else { 0 })
        .bind(month_start)
        .fetch_optional(&self.pool)
        .await?;

        let Some((id, api_key)) = lease else {
            return Ok(None);
        };

        self.touch_key(&api_key, now).await?;
        Ok(Some(ApiKeyLease {
            id,
            secret: api_key,
        }))
    }

    pub(crate) async fn has_available_active_key_excluding(
        &self,
        excluded_key_id: Option<&str>,
    ) -> Result<bool, ProxyError> {
        self.reset_monthly().await?;
        let month_start = start_of_month(Utc::now()).timestamp();

        let count: i64 = if let Some(excluded_key_id) = excluded_key_id {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM api_keys
                WHERE id != ? AND status = ? AND deleted_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_quarantines q
                      WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
                  )
                "#,
            )
            .bind(excluded_key_id)
            .bind(STATUS_ACTIVE)
            .bind(month_start)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM api_keys
                WHERE status = ? AND deleted_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_quarantines q
                      WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
                  )
                "#,
            )
            .bind(STATUS_ACTIVE)
            .bind(month_start)
            .fetch_one(&self.pool)
            .await?
        };

        Ok(count > 0)
    }

    pub(crate) async fn has_available_regular_exhausted_key_excluding(
        &self,
        excluded_key_id: Option<&str>,
    ) -> Result<bool, ProxyError> {
        self.reset_monthly().await?;
        let month_start = start_of_month(Utc::now()).timestamp();

        let count: i64 = if let Some(excluded_key_id) = excluded_key_id {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM api_keys
                WHERE id != ? AND status = ? AND deleted_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_quarantines q
                      WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
                  )
                "#,
            )
            .bind(excluded_key_id)
            .bind(STATUS_EXHAUSTED)
            .bind(month_start)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar(
                r#"
                SELECT COUNT(*)
                FROM api_keys
                WHERE status = ? AND deleted_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_quarantines q
                      WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
                  )
                "#,
            )
            .bind(STATUS_EXHAUSTED)
            .bind(month_start)
            .fetch_one(&self.pool)
            .await?
        };

        Ok(count > 0)
    }

    pub(crate) async fn is_low_quota_depleted_this_month(
        &self,
        key_id: &str,
    ) -> Result<bool, ProxyError> {
        let month_start = start_of_month(Utc::now()).timestamp();
        let exists = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT 1
            FROM api_key_low_quota_depletions
            WHERE key_id = ? AND month_start = ?
            LIMIT 1
            "#,
        )
        .bind(key_id)
        .bind(month_start)
        .fetch_optional(&self.pool)
        .await?
        .is_some();
        Ok(exists)
    }

    pub(crate) async fn record_low_quota_depletion_if_needed(
        &self,
        key_id: &str,
        threshold: i64,
    ) -> Result<bool, ProxyError> {
        let Some(quota_remaining) =
            sqlx::query_scalar::<_, Option<i64>>("SELECT quota_remaining FROM api_keys WHERE id = ?")
                .bind(key_id)
                .fetch_optional(&self.pool)
                .await?
                .flatten()
        else {
            return Ok(false);
        };

        if quota_remaining > threshold {
            return Ok(false);
        }

        let now = Utc::now();
        let month_start = start_of_month(now).timestamp();
        sqlx::query(
            r#"
            INSERT INTO api_key_low_quota_depletions (
                key_id,
                month_start,
                threshold,
                quota_remaining,
                created_at
            ) VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(key_id, month_start) DO UPDATE SET
                threshold = excluded.threshold,
                quota_remaining = MIN(api_key_low_quota_depletions.quota_remaining, excluded.quota_remaining)
            "#,
        )
        .bind(key_id)
        .bind(month_start)
        .bind(threshold)
        .bind(quota_remaining)
        .bind(now.timestamp())
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    pub(crate) async fn acquire_active_key_excluding(
        &self,
        excluded_key_id: Option<&str>,
    ) -> Result<ApiKeyLease, ProxyError> {
        self.reset_monthly().await?;

        let now = Utc::now().timestamp();
        let month_start = start_of_month(Utc::now()).timestamp();

        let active_candidate = if let Some(excluded_key_id) = excluded_key_id {
            sqlx::query_as::<_, (String, String)>(
                r#"
                SELECT id, api_key
                FROM api_keys
                WHERE id != ? AND status = ? AND deleted_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_quarantines q
                      WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
                  )
                ORDER BY last_used_at ASC, id ASC
                LIMIT 1
                "#,
            )
            .bind(excluded_key_id)
            .bind(STATUS_ACTIVE)
            .bind(month_start)
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String)>(
                r#"
                SELECT id, api_key
                FROM api_keys
                WHERE status = ? AND deleted_at IS NULL
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_low_quota_depletions d
                      WHERE d.key_id = api_keys.id AND d.month_start = ?
                  )
                  AND NOT EXISTS (
                      SELECT 1
                      FROM api_key_quarantines q
                      WHERE q.key_id = api_keys.id AND q.cleared_at IS NULL
                  )
                ORDER BY last_used_at ASC, id ASC
                LIMIT 1
            "#,
            )
            .bind(STATUS_ACTIVE)
            .bind(month_start)
            .fetch_optional(&self.pool)
            .await?
        };

        let Some((id, api_key)) = active_candidate else {
            return Err(ProxyError::NoAvailableKeys);
        };

        self.touch_key(&api_key, now).await?;
        Ok(ApiKeyLease {
            id,
            secret: api_key,
        })
    }

    pub(crate) async fn save_research_request_affinity(
        &self,
        request_id: &str,
        key_id: &str,
        token_id: &str,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO research_requests (
                request_id,
                key_id,
                token_id,
                expires_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(request_id) DO UPDATE SET
                key_id = excluded.key_id,
                token_id = excluded.token_id,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(request_id)
        .bind(key_id)
        .bind(token_id)
        .bind(expires_at)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Opportunistic cleanup to keep this small over time.
        sqlx::query("DELETE FROM research_requests WHERE expires_at <= ?")
            .bind(now)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn ensure_api_key_maintenance_records_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_maintenance_records (
                id TEXT PRIMARY KEY,
                key_id TEXT NOT NULL,
                source TEXT NOT NULL,
                operation_code TEXT NOT NULL,
                operation_summary TEXT NOT NULL,
                reason_code TEXT,
                reason_summary TEXT,
                reason_detail TEXT,
                request_log_id INTEGER,
                auth_token_log_id INTEGER,
                auth_token_id TEXT,
                actor_user_id TEXT,
                actor_display_name TEXT,
                status_before TEXT,
                status_after TEXT,
                quarantine_before INTEGER NOT NULL DEFAULT 0,
                quarantine_after INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                FOREIGN KEY (key_id) REFERENCES api_keys(id),
                FOREIGN KEY (auth_token_id) REFERENCES auth_tokens(id),
                FOREIGN KEY (request_log_id) REFERENCES request_logs(id),
                FOREIGN KEY (auth_token_log_id) REFERENCES auth_token_logs(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_key_created
               ON api_key_maintenance_records(key_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_request_log
               ON api_key_maintenance_records(request_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_auth_token_log
               ON api_key_maintenance_records(auth_token_log_id)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_maintenance_records_auto_exhausted_window
               ON api_key_maintenance_records(created_at, key_id)
               WHERE source = 'system'
                 AND operation_code = 'auto_mark_exhausted'
                 AND reason_code = 'quota_exhausted'"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn ensure_api_key_transient_backoffs_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_key_transient_backoffs (
                key_id TEXT NOT NULL,
                scope TEXT NOT NULL,
                cooldown_until INTEGER NOT NULL,
                retry_after_secs INTEGER NOT NULL,
                reason_code TEXT,
                source_request_log_id INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (key_id, scope),
                FOREIGN KEY (key_id) REFERENCES api_keys(id),
                FOREIGN KEY (source_request_log_id) REFERENCES request_logs(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_transient_backoffs_scope_cooldown
               ON api_key_transient_backoffs(scope, cooldown_until)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_api_key_transient_backoffs_key_scope
               ON api_key_transient_backoffs(key_id, scope)"#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn get_research_request_affinity(
        &self,
        request_id: &str,
        now: i64,
    ) -> Result<Option<(String, String)>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT key_id, token_id
            FROM research_requests
            WHERE request_id = ? AND expires_at > ?
            LIMIT 1
            "#,
        )
        .bind(request_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        if row.is_none() {
            sqlx::query(
                r#"
                DELETE FROM research_requests
                WHERE request_id = ? AND expires_at <= ?
                "#,
            )
            .bind(request_id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        Ok(row)
    }

    // ----- Access token helpers -----

    pub(crate) fn compose_full_token(id: &str, secret: &str) -> String {
        format!("th-{}-{}", id, secret)
    }

    pub(crate) async fn validate_access_token(&self, token: &str) -> Result<bool, ProxyError> {
        // Expect format th-<id>-<secret>
        let Some(rest) = token.strip_prefix("th-") else {
            return Ok(false);
        };
        let parts: Vec<&str> = rest.splitn(2, '-').collect();
        if parts.len() != 2 {
            return Ok(false);
        }
        let id = parts[0];
        let secret = parts[1];
        // Keep short, human-friendly id; strengthen total entropy by lengthening secret.
        // Backward-compatible: accept legacy 12-char secrets and new longer secrets.
        const LEGACY_SECRET_LEN: usize = 12;
        const NEW_SECRET_LEN: usize = 24; // chosen to significantly raise entropy
        let secret_len_ok = secret.len() == LEGACY_SECRET_LEN || secret.len() == NEW_SECRET_LEN;
        if id.len() != 4 || !secret_len_ok {
            return Ok(false);
        }

        // Validation should be a pure check. Do NOT mutate usage counters here,
        // otherwise the token's total_requests will be double-counted (once here,
        // and once when we actually record the attempt). Only return whether the
        // token exists and is enabled.
        let row = sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT t.enabled, COALESCE(u.active, 1) AS user_active
               FROM auth_tokens t
               LEFT JOIN user_token_bindings b ON b.token_id = t.id
               LEFT JOIN users u ON u.id = b.user_id
               WHERE t.id = ? AND t.secret = ? AND t.deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(id)
        .bind(secret)
        .fetch_optional(&self.pool)
        .await?;

        Ok(matches!(row, Some((enabled, user_active)) if enabled == 1 && user_active == 1))
    }

    pub(crate) async fn create_access_token(
        &self,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        loop {
            let id = random_string(ALPHABET, 4);
            // Increase secret length to strengthen token entropy while keeping id short.
            let secret = random_string(ALPHABET, 24);
            let res = sqlx::query(
                r#"INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                   VALUES (?, ?, 1, ?, NULL, 0, ?, NULL, NULL)"#,
            )
            .bind(&id)
            .bind(&secret)
            .bind(note.unwrap_or(""))
            .bind(Utc::now().timestamp())
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    let token_str = Self::compose_full_token(&id, &secret);
                    return Ok(AuthTokenSecret {
                        id,
                        token: token_str,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    // Retry on rare id collision
                    continue;
                }
                Err(e) => return Err(ProxyError::Database(e)),
            }
        }
    }

    /// Batch-create access tokens with required group name. Optional note applied to each row.
    pub(crate) async fn create_access_tokens_batch(
        &self,
        group: &str,
        count: usize,
        note: Option<&str>,
    ) -> Result<Vec<AuthTokenSecret>, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut tx = self.pool.begin().await?;
        let mut out: Vec<AuthTokenSecret> = Vec::with_capacity(count);
        for _ in 0..count {
            loop {
                let id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);
                let res = sqlx::query(
                    r#"INSERT INTO auth_tokens (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES (?, ?, 1, ?, ?, 0, ?, NULL, NULL)"#,
                )
                .bind(&id)
                .bind(&secret)
                .bind(note.unwrap_or(""))
                .bind(group)
                .bind(Utc::now().timestamp())
                .execute(&mut *tx)
                .await;

                match res {
                    Ok(_) => {
                        let token = Self::compose_full_token(&id, &secret);
                        out.push(AuthTokenSecret { id, token });
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                        continue;
                    }
                    Err(e) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(e));
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(out)
    }
    // Generate random string of given length from provided alphabet
    // Alphabet is a byte slice of ASCII alphanumerics
    // Using ThreadRng for simplicity

    pub(crate) async fn list_access_tokens(&self) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL
               ORDER BY created_at DESC, id DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    /// Paginated list of access tokens ordered by created_at desc. Returns (items, total)
    pub(crate) async fn list_access_tokens_paged(
        &self,
        page: i64,
        per_page: i64,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;

        let total: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM auth_tokens WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL
               ORDER BY created_at DESC, id DESC
               LIMIT ? OFFSET ?"#,
        )
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let items = rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect();
        Ok((items, total))
    }

    pub(crate) async fn list_disabled_access_tokens(
        &self,
        limit: usize,
    ) -> Result<Vec<AuthToken>, ProxyError> {
        let limit = limit.clamp(1, 100) as i64;
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE deleted_at IS NULL AND enabled = 0
               ORDER BY created_at DESC, id DESC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at: last_used,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    pub(crate) async fn list_disabled_access_token_ids(
        &self,
        limit: usize,
    ) -> Result<Vec<String>, ProxyError> {
        let limit = limit.clamp(1, 100) as i64;
        sqlx::query_scalar::<_, String>(
            r#"SELECT id
               FROM auth_tokens
               WHERE deleted_at IS NULL AND enabled = 0
               ORDER BY created_at DESC, id DESC
               LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn delete_access_token(&self, id: &str) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE auth_tokens SET enabled = 0, deleted_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn set_access_token_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET enabled = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(if enabled { 1 } else { 0 })
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn update_access_token_note(
        &self,
        id: &str,
        note: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query("UPDATE auth_tokens SET note = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(note)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn get_access_token_secret(
        &self,
        id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            "SELECT secret FROM auth_tokens WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &secret),
        }))
    }

    /// Update the secret for an existing token id and return the new full token string.
    pub(crate) async fn rotate_access_token_secret(
        &self,
        id: &str,
    ) -> Result<AuthTokenSecret, ProxyError> {
        // Ensure token exists first to provide a clearer error on missing id
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM auth_tokens WHERE id = ? AND deleted_at IS NULL LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if exists.is_none() {
            return Err(ProxyError::Database(sqlx::Error::RowNotFound));
        }

        // Generate a new secret with the current strong length
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let new_secret = random_string(ALPHABET, 24);

        sqlx::query("UPDATE auth_tokens SET secret = ? WHERE id = ? AND deleted_at IS NULL")
            .bind(&new_secret)
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(AuthTokenSecret {
            id: id.to_string(),
            token: Self::compose_full_token(id, &new_secret),
        })
    }

    pub(crate) async fn list_user_tokens(
        &self,
        user_id: &str,
    ) -> Result<Vec<AuthToken>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            ),
        >(
            r#"SELECT t.id, t.enabled, t.note, t.group_name, t.total_requests, t.created_at, t.last_used_at
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ? AND t.deleted_at IS NULL
               ORDER BY b.updated_at DESC, b.created_at DESC, t.id DESC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(id, enabled, note, group_name, total, created_at, last_used_at)| AuthToken {
                    id,
                    enabled: enabled == 1,
                    note,
                    group_name,
                    total_requests: total,
                    created_at,
                    last_used_at,
                    quota: None,
                    quota_hourly_reset_at: None,
                    quota_daily_reset_at: None,
                    quota_monthly_reset_at: None,
                },
            )
            .collect())
    }

    pub(crate) async fn is_user_token_bound(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT 1
               FROM user_token_bindings
               WHERE user_id = ? AND token_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    pub(crate) async fn list_user_bindings_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<HashMap<String, String>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, user_id FROM user_token_bindings WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");
        let rows = builder
            .build_query_as::<(String, String)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, user_id) in rows {
            map.insert(token_id, user_id);
        }
        Ok(map)
    }

    pub(crate) async fn get_user_token_secret(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT t.secret
               FROM user_token_bindings b
               JOIN auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = ? AND b.token_id = ? AND t.deleted_at IS NULL AND t.enabled = 1
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: token_id.to_string(),
            token: Self::compose_full_token(token_id, &secret),
        }))
    }

    #[allow(dead_code)]
    pub(crate) async fn find_user_id_by_token(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.token_binding_cache.read().await;
            cache.get(token_id).cloned()
        } && cached.expires_at > now
        {
            return Ok(cached.user_id);
        }

        self.find_user_id_by_token_fresh(token_id).await
    }

    pub(crate) async fn find_user_id_by_token_fresh(
        &self,
        token_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT user_id FROM user_token_bindings WHERE token_id = ? LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let user_id = row.map(|(id,)| id);
        self.cache_token_binding(token_id, user_id.as_deref()).await;
        Ok(user_id)
    }

    pub(crate) async fn cache_token_binding(&self, token_id: &str, user_id: Option<&str>) {
        let mut cache = self.token_binding_cache.write().await;
        cache.insert(
            token_id.to_string(),
            TokenBindingCacheEntry {
                user_id: user_id.map(str::to_string),
                expires_at: Instant::now() + Duration::from_secs(TOKEN_BINDING_CACHE_TTL_SECS),
            },
        );

        if cache.len() <= TOKEN_BINDING_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() <= TOKEN_BINDING_CACHE_MAX_ENTRIES {
            return;
        }
        let overflow = cache.len() - TOKEN_BINDING_CACHE_MAX_ENTRIES;
        let keys: Vec<String> = cache.keys().take(overflow).cloned().collect();
        for key in keys {
            cache.remove(&key);
        }
    }

    pub(crate) async fn cached_account_quota_resolution(
        &self,
        user_id: &str,
    ) -> Option<AccountQuotaResolution> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.account_quota_resolution_cache.read().await;
            cache.get(user_id).cloned()
        } && cached.expires_at > now
        {
            return Some(cached.resolution);
        }
        None
    }

    pub(crate) async fn cache_account_quota_resolution(
        &self,
        user_id: &str,
        resolution: &AccountQuotaResolution,
    ) {
        let mut cache = self.account_quota_resolution_cache.write().await;
        cache.insert(
            user_id.to_string(),
            AccountQuotaResolutionCacheEntry {
                resolution: resolution.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(ACCOUNT_QUOTA_RESOLUTION_CACHE_TTL_SECS),
            },
        );

        if cache.len() <= ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() <= ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES {
            return;
        }
        let overflow = cache.len() - ACCOUNT_QUOTA_RESOLUTION_CACHE_MAX_ENTRIES;
        let keys: Vec<String> = cache.keys().take(overflow).cloned().collect();
        for key in keys {
            cache.remove(&key);
        }
    }

    pub(crate) async fn invalidate_account_quota_resolution(&self, user_id: &str) {
        self.account_quota_resolution_cache
            .write()
            .await
            .remove(user_id);
    }

    pub(crate) async fn invalidate_account_quota_resolutions<I, S>(&self, user_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut cache = self.account_quota_resolution_cache.write().await;
        for user_id in user_ids {
            cache.remove(user_id.as_ref());
        }
    }

    pub(crate) async fn invalidate_all_account_quota_resolutions(&self) {
        self.account_quota_resolution_cache.write().await.clear();
    }

    async fn cached_request_logs_catalog(&self, cache_key: &str) -> Option<RequestLogsCatalog> {
        let now = Instant::now();
        if let Some(cached) = {
            let cache = self.request_logs_catalog_cache.read().await;
            cache.get(cache_key).cloned()
        } && cached.expires_at > now
        {
            return Some(cached.value);
        }
        None
    }

    async fn cache_request_logs_catalog(&self, cache_key: String, value: &RequestLogsCatalog) {
        let mut cache = self.request_logs_catalog_cache.write().await;
        cache.insert(
            cache_key,
            RequestLogsCatalogCacheEntry {
                value: value.clone(),
                expires_at: Instant::now()
                    + Duration::from_secs(ADMIN_REQUEST_LOGS_CATALOG_CACHE_TTL_SECS as u64),
            },
        );
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
    }

    pub(crate) async fn invalidate_request_logs_catalog_cache(&self) {
        self.request_logs_catalog_cache.write().await.clear();
    }

    pub(crate) async fn list_user_ids_for_tag(
        &self,
        tag_id: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT user_id FROM user_tag_bindings WHERE tag_id = ?",
        )
        .bind(tag_id)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn list_admin_users_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let search = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let tag_id = tag_id.map(str::trim).filter(|value| !value.is_empty());

        let total = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_one(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_one(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_scalar::<_, i64>(
                    r#"SELECT COUNT(*)
                       FROM users u
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )"#,
                )
                .bind(tag_id)
                .fetch_one(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
                    .fetch_one(&self.pool)
                    .await?
            }
        };

        let rows = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(tag_id)
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC
                       LIMIT ? OFFSET ?"#,
                )
                .bind(per_page)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?
            }
        };

        let items = rows
            .into_iter()
            .map(
                |(user_id, display_name, username, active, last_login_at, token_count)| {
                    AdminUserIdentity {
                        user_id,
                        display_name,
                        username,
                        active: active == 1,
                        last_login_at,
                        token_count,
                    }
                },
            )
            .collect();
        Ok((items, total))
    }

    pub(crate) async fn list_admin_users_filtered(
        &self,
        query: Option<&str>,
        tag_id: Option<&str>,
    ) -> Result<Vec<AdminUserIdentity>, ProxyError> {
        let search = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let tag_id = tag_id.map(str::trim).filter(|value| !value.is_empty());

        let rows = match (search.as_ref(), tag_id) {
            (Some(search), Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               WHERE utb.user_id = u.id
                                 AND utb.tag_id = ?
                           )
                         AND (
                               u.id LIKE ?
                               OR COALESCE(u.display_name, '') LIKE ?
                               OR COALESCE(u.username, '') LIKE ?
                               OR EXISTS (
                                   SELECT 1
                                   FROM user_tag_bindings utb
                                   JOIN user_tags ut ON ut.id = utb.tag_id
                                   WHERE utb.user_id = u.id
                                     AND (
                                         ut.name LIKE ?
                                         OR COALESCE(ut.display_name, '') LIKE ?
                                     )
                               )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC"#,
                )
                .bind(tag_id)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(search), None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE u.id LIKE ?
                          OR COALESCE(u.display_name, '') LIKE ?
                          OR COALESCE(u.username, '') LIKE ?
                          OR EXISTS (
                               SELECT 1
                               FROM user_tag_bindings utb
                               JOIN user_tags ut ON ut.id = utb.tag_id
                               WHERE utb.user_id = u.id
                                 AND (
                                   ut.name LIKE ?
                                   OR COALESCE(ut.display_name, '') LIKE ?
                                 )
                           )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC"#,
                )
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .bind(search)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(tag_id)) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       WHERE EXISTS (
                           SELECT 1
                           FROM user_tag_bindings utb
                           WHERE utb.user_id = u.id
                             AND utb.tag_id = ?
                       )
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC"#,
                )
                .bind(tag_id)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query_as::<
                    _,
                    (
                        String,
                        Option<String>,
                        Option<String>,
                        i64,
                        Option<i64>,
                        i64,
                    ),
                >(
                    r#"SELECT
                         u.id,
                         u.display_name,
                         u.username,
                         u.active,
                         u.last_login_at,
                         COALESCE(COUNT(b.token_id), 0) AS token_count
                       FROM users u
                       LEFT JOIN user_token_bindings b ON b.user_id = u.id
                       GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
                       ORDER BY (u.last_login_at IS NULL) ASC, u.last_login_at DESC, u.id ASC"#,
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows
            .into_iter()
            .map(
                |(user_id, display_name, username, active, last_login_at, token_count)| {
                    AdminUserIdentity {
                        user_id,
                        display_name,
                        username,
                        active: active == 1,
                        last_login_at,
                        token_count,
                    }
                },
            )
            .collect())
    }

    pub(crate) async fn get_admin_user_identity(
        &self,
        user_id: &str,
    ) -> Result<Option<AdminUserIdentity>, ProxyError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            ),
        >(
            r#"SELECT
                 u.id,
                 u.display_name,
                 u.username,
                 u.active,
                 u.last_login_at,
                 COALESCE(COUNT(b.token_id), 0) AS token_count
               FROM users u
               LEFT JOIN user_token_bindings b ON b.user_id = u.id
               WHERE u.id = ?
               GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(user_id, display_name, username, active, last_login_at, token_count)| {
                AdminUserIdentity {
                    user_id,
                    display_name,
                    username,
                    active: active == 1,
                    last_login_at,
                    token_count,
                }
            },
        ))
    }

    pub(crate) async fn get_admin_user_identities(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, AdminUserIdentity>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            r#"SELECT
                 u.id,
                 u.display_name,
                 u.username,
                 u.active,
                 u.last_login_at,
                 COALESCE(COUNT(b.token_id), 0) AS token_count
               FROM users u
               LEFT JOIN user_token_bindings b ON b.user_id = u.id
               WHERE u.id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at");

        let rows = builder
            .build_query_as::<(
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;

        let mut items = HashMap::with_capacity(rows.len());
        for (user_id, display_name, username, active, last_login_at, token_count) in rows {
            items.insert(
                user_id.clone(),
                AdminUserIdentity {
                    user_id,
                    display_name,
                    username,
                    active: active == 1,
                    last_login_at,
                    token_count,
                },
            );
        }
        Ok(items)
    }

    pub(crate) async fn get_user_primary_api_key_affinity(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            r#"SELECT api_key_id
               FROM user_primary_api_key_affinity
               WHERE user_id = ?
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn get_token_primary_api_key_affinity(
        &self,
        token_id: &str,
    ) -> Result<Option<TokenPrimaryApiKeyAffinity>, ProxyError> {
        let row = sqlx::query_as::<_, (String, Option<String>, String)>(
            r#"SELECT token_id, user_id, api_key_id
               FROM token_primary_api_key_affinity
               WHERE token_id = ?
               LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(token_id, user_id, api_key_id)| TokenPrimaryApiKeyAffinity {
                token_id,
                user_id,
                api_key_id,
            },
        ))
    }

    pub(crate) async fn find_recent_primary_candidate_for_user(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT api_key_id
            FROM user_api_key_bindings
            WHERE user_id = ?
            ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn sync_user_primary_api_key_affinity(
        &self,
        user_id: &str,
        api_key_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO user_primary_api_key_affinity (user_id, api_key_id, created_at, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(user_id)
        .bind(api_key_id)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO token_primary_api_key_affinity (
                token_id,
                user_id,
                api_key_id,
                created_at,
                updated_at
            )
            SELECT token_id, user_id, ?, ?, ?
            FROM user_token_bindings
            WHERE user_id = ?
            ON CONFLICT(token_id) DO UPDATE SET
                user_id = excluded.user_id,
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(api_key_id)
        .bind(now)
        .bind(now)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn set_token_primary_api_key_affinity(
        &self,
        token_id: &str,
        user_id: Option<&str>,
        api_key_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO token_primary_api_key_affinity (
                token_id,
                user_id,
                api_key_id,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                user_id = excluded.user_id,
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(token_id)
        .bind(user_id)
        .bind(api_key_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_http_project_api_key_affinity(
        &self,
        owner_subject: &str,
        project_id_hash: &str,
    ) -> Result<Option<HttpProjectAffinityBinding>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            r#"SELECT owner_subject, project_id_hash, api_key_id
               FROM http_project_api_key_affinity
               WHERE owner_subject = ? AND project_id_hash = ?
               LIMIT 1"#,
        )
        .bind(owner_subject)
        .bind(project_id_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(owner_subject, project_id_hash, api_key_id)| HttpProjectAffinityBinding {
                owner_subject,
                project_id_hash,
                api_key_id,
            },
        ))
    }

    pub(crate) async fn set_http_project_api_key_affinity(
        &self,
        owner_subject: &str,
        project_id_hash: &str,
        api_key_id: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO http_project_api_key_affinity (
                owner_subject,
                project_id_hash,
                api_key_id,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(owner_subject, project_id_hash) DO UPDATE SET
                api_key_id = excluded.api_key_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(owner_subject)
        .bind(project_id_hash)
        .bind(api_key_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn create_or_replace_mcp_session(
        &self,
        binding: &McpSessionBinding,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO mcp_sessions (
                proxy_session_id,
                upstream_session_id,
                upstream_key_id,
                auth_token_id,
                user_id,
                protocol_version,
                last_event_id,
                gateway_mode,
                experiment_variant,
                ab_bucket,
                routing_subject_hash,
                fallback_reason,
                rate_limited_until,
                last_rate_limited_at,
                last_rate_limit_reason,
                created_at,
                updated_at,
                expires_at,
                revoked_at,
                revoke_reason
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(proxy_session_id) DO UPDATE SET
                upstream_session_id = excluded.upstream_session_id,
                upstream_key_id = excluded.upstream_key_id,
                auth_token_id = excluded.auth_token_id,
                user_id = excluded.user_id,
                protocol_version = excluded.protocol_version,
                last_event_id = excluded.last_event_id,
                gateway_mode = excluded.gateway_mode,
                experiment_variant = excluded.experiment_variant,
                ab_bucket = excluded.ab_bucket,
                routing_subject_hash = excluded.routing_subject_hash,
                fallback_reason = excluded.fallback_reason,
                rate_limited_until = excluded.rate_limited_until,
                last_rate_limited_at = excluded.last_rate_limited_at,
                last_rate_limit_reason = excluded.last_rate_limit_reason,
                updated_at = excluded.updated_at,
                expires_at = excluded.expires_at,
                revoked_at = excluded.revoked_at,
                revoke_reason = excluded.revoke_reason
            "#,
        )
        .bind(&binding.proxy_session_id)
        .bind(binding.upstream_session_id.as_deref())
        .bind(binding.upstream_key_id.as_deref())
        .bind(binding.auth_token_id.as_deref())
        .bind(binding.user_id.as_deref())
        .bind(binding.protocol_version.as_deref())
        .bind(binding.last_event_id.as_deref())
        .bind(&binding.gateway_mode)
        .bind(&binding.experiment_variant)
        .bind(binding.ab_bucket)
        .bind(binding.routing_subject_hash.as_deref())
        .bind(binding.fallback_reason.as_deref())
        .bind(binding.rate_limited_until)
        .bind(binding.last_rate_limited_at)
        .bind(binding.last_rate_limit_reason.as_deref())
        .bind(binding.created_at)
        .bind(binding.updated_at)
        .bind(binding.expires_at)
        .bind(binding.revoked_at)
        .bind(binding.revoke_reason.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn get_active_mcp_session(
        &self,
        proxy_session_id: &str,
        now: i64,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                proxy_session_id,
                upstream_session_id,
                upstream_key_id,
                auth_token_id,
                user_id,
                protocol_version,
                last_event_id,
                gateway_mode,
                experiment_variant,
                ab_bucket,
                routing_subject_hash,
                fallback_reason,
                rate_limited_until,
                last_rate_limited_at,
                last_rate_limit_reason,
                created_at,
                updated_at,
                expires_at,
                revoked_at,
                revoke_reason
            FROM mcp_sessions
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            LIMIT 1
            "#,
        )
        .bind(proxy_session_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| McpSessionBinding {
            proxy_session_id: row.get("proxy_session_id"),
            upstream_session_id: row.get("upstream_session_id"),
            upstream_key_id: row.get("upstream_key_id"),
            auth_token_id: row.get("auth_token_id"),
            user_id: row.get("user_id"),
            protocol_version: row.get("protocol_version"),
            last_event_id: row.get("last_event_id"),
            gateway_mode: row.get("gateway_mode"),
            experiment_variant: row.get("experiment_variant"),
            ab_bucket: row.get("ab_bucket"),
            routing_subject_hash: row.get("routing_subject_hash"),
            fallback_reason: row.get("fallback_reason"),
            rate_limited_until: row.get("rate_limited_until"),
            last_rate_limited_at: row.get("last_rate_limited_at"),
            last_rate_limit_reason: row.get("last_rate_limit_reason"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
            revoke_reason: row.get("revoke_reason"),
        }))
    }

    pub(crate) async fn get_latest_active_mcp_session_for_token(
        &self,
        token_id: &str,
        now: i64,
    ) -> Result<Option<McpSessionBinding>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                proxy_session_id,
                upstream_session_id,
                upstream_key_id,
                auth_token_id,
                user_id,
                protocol_version,
                last_event_id,
                gateway_mode,
                experiment_variant,
                ab_bucket,
                routing_subject_hash,
                fallback_reason,
                rate_limited_until,
                last_rate_limited_at,
                last_rate_limit_reason,
                created_at,
                updated_at,
                expires_at,
                revoked_at,
                revoke_reason
            FROM mcp_sessions
            WHERE auth_token_id = ?
              AND revoked_at IS NULL
              AND expires_at > ?
            ORDER BY updated_at DESC, expires_at DESC
            LIMIT 1
            "#,
        )
        .bind(token_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| McpSessionBinding {
            proxy_session_id: row.get("proxy_session_id"),
            upstream_session_id: row.get("upstream_session_id"),
            upstream_key_id: row.get("upstream_key_id"),
            auth_token_id: row.get("auth_token_id"),
            user_id: row.get("user_id"),
            protocol_version: row.get("protocol_version"),
            last_event_id: row.get("last_event_id"),
            gateway_mode: row.get("gateway_mode"),
            experiment_variant: row.get("experiment_variant"),
            ab_bucket: row.get("ab_bucket"),
            routing_subject_hash: row.get("routing_subject_hash"),
            fallback_reason: row.get("fallback_reason"),
            rate_limited_until: row.get("rate_limited_until"),
            last_rate_limited_at: row.get("last_rate_limited_at"),
            last_rate_limit_reason: row.get("last_rate_limit_reason"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
            revoke_reason: row.get("revoke_reason"),
        }))
    }

    pub(crate) async fn has_active_non_rebalance_mcp_session_for_token(
        &self,
        token_id: &str,
        now: i64,
    ) -> Result<bool, ProxyError> {
        let exists: i64 = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM mcp_sessions
                WHERE auth_token_id = ?
                  AND gateway_mode <> ?
                  AND revoked_at IS NULL
                  AND expires_at > ?
            )
            "#,
        )
        .bind(token_id)
        .bind(MCP_GATEWAY_MODE_REBALANCE)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists != 0)
    }

    pub(crate) async fn touch_mcp_session(
        &self,
        proxy_session_id: &str,
        protocol_version: Option<&str>,
        last_event_id: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                protocol_version = COALESCE(?, protocol_version),
                last_event_id = COALESCE(?, last_event_id),
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(protocol_version)
        .bind(last_event_id)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn mark_mcp_session_rate_limited(
        &self,
        proxy_session_id: &str,
        rate_limited_until: i64,
        reason: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                rate_limited_until = ?,
                last_rate_limited_at = ?,
                last_rate_limit_reason = ?,
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(rate_limited_until)
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn clear_mcp_session_rate_limit(
        &self,
        proxy_session_id: &str,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                rate_limited_until = NULL,
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_mcp_session_upstream_identity(
        &self,
        proxy_session_id: &str,
        upstream_session_id: &str,
        protocol_version: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                upstream_session_id = ?,
                protocol_version = COALESCE(?, protocol_version),
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(upstream_session_id)
        .bind(protocol_version)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_mcp_session_rebalance_metadata(
        &self,
        proxy_session_id: &str,
        routing_subject_hash: Option<&str>,
        fallback_reason: Option<&str>,
        now: i64,
        expires_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET
                routing_subject_hash = COALESCE(?, routing_subject_hash),
                fallback_reason = COALESCE(?, fallback_reason),
                updated_at = ?,
                expires_at = ?
            WHERE proxy_session_id = ?
              AND revoked_at IS NULL
            "#,
        )
        .bind(routing_subject_hash)
        .bind(fallback_reason)
        .bind(now)
        .bind(expires_at)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn revoke_mcp_session(
        &self,
        proxy_session_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET revoked_at = ?, revoke_reason = ?, updated_at = ?
            WHERE proxy_session_id = ? AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(proxy_session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn revoke_mcp_sessions_for_user_key(
        &self,
        user_id: &str,
        upstream_key_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET revoked_at = ?, revoke_reason = ?, updated_at = ?
            WHERE user_id = ? AND upstream_key_id = ? AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(user_id)
        .bind(upstream_key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn revoke_mcp_sessions_for_token_key(
        &self,
        token_id: &str,
        upstream_key_id: &str,
        reason: &str,
    ) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE mcp_sessions
            SET revoked_at = ?, revoke_reason = ?, updated_at = ?
            WHERE auth_token_id = ? AND upstream_key_id = ? AND revoked_at IS NULL
            "#,
        )
        .bind(now)
        .bind(reason)
        .bind(now)
        .bind(token_id)
        .bind(upstream_key_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

}

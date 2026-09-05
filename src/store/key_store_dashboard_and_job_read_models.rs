impl KeyStore {
    pub(crate) async fn list_keys_pending_quota_sync(
        &self,
        older_than_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let now = self.backend_time.now_ts();
        let threshold = now - older_than_secs;
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE deleted_at IS NULL
              AND status <> ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND (
                quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
            )
            ORDER BY CASE WHEN quota_synced_at IS NULL OR quota_synced_at = 0 THEN 0 ELSE 1 END, quota_synced_at ASC
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(threshold)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn list_keys_pending_hot_quota_sync(
        &self,
        active_within_secs: i64,
        stale_after_secs: i64,
    ) -> Result<Vec<String>, ProxyError> {
        let now = self.backend_time.now_ts();
        let active_since = now - active_within_secs;
        let stale_before = now - stale_after_secs;
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT id
            FROM api_keys
            WHERE deleted_at IS NULL
              AND status <> ?
              AND last_used_at >= ?
              AND NOT EXISTS (
                  SELECT 1
                  FROM api_key_quarantines aq
                  WHERE aq.key_id = api_keys.id AND aq.cleared_at IS NULL
              )
              AND (
                quota_synced_at IS NULL OR quota_synced_at = 0 OR quota_synced_at < ?
              )
            ORDER BY last_used_at DESC, quota_synced_at ASC, id ASC
            "#,
        )
        .bind(STATUS_EXHAUSTED)
        .bind(active_since)
        .bind(stale_before)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub(crate) async fn list_recent_jobs(&self, limit: usize) -> Result<Vec<JobLog>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        let rows = sqlx::query(
            r#"SELECT
                    j.id,
                    j.job_type,
                    j.trigger_source,
                    j.key_id,
                    k.group_name AS key_group,
                    j.status,
                    j.attempt,
                    j.message,
                    j.queued_at,
                    j.started_at,
                    j.finished_at,
                    j.claim_generation
                FROM scheduled_jobs j
                LEFT JOIN api_keys k ON k.id = j.key_id
                ORDER BY COALESCE(j.started_at, j.queued_at) DESC, j.id DESC
                LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    trigger_source: row.try_get("trigger_source")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    queued_at: row.try_get("queued_at")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                    claim_generation: row.try_get("claim_generation")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(items)
    }

    pub(crate) async fn list_recent_job_signatures(
        &self,
        limit: usize,
    ) -> Result<Vec<(i64, String, Option<i64>)>, ProxyError> {
        let limit = limit.clamp(1, 500) as i64;
        sqlx::query_as::<_, (i64, String, Option<i64>)>(
            r#"
            SELECT id, status, finished_at
            FROM scheduled_jobs
            ORDER BY COALESCE(started_at, queued_at) DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn list_recent_jobs_paginated(
        &self,
        group: &str,
        page: usize,
        per_page: usize,
    ) -> Result<(Vec<JobLog>, i64, JobGroupCounts), ProxyError> {
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100) as i64;
        let offset = ((page - 1) as i64).saturating_mul(per_page);

        let where_clause = Self::scheduled_job_group_filter_clause(group, "j.job_type");
        let count_where_clause = Self::scheduled_job_group_filter_clause(group, "job_type");

        let count_query = format!("SELECT COUNT(*) FROM scheduled_jobs {}", count_where_clause);
        let total: i64 = sqlx::query_scalar(&count_query)
            .fetch_one(&self.pool)
            .await?;
        let group_counts = self.fetch_recent_job_group_counts().await?;

        let select_query = format!(
            r#"
            SELECT
                j.id,
                j.job_type,
                j.trigger_source,
                j.key_id,
                k.group_name AS key_group,
                j.status,
                j.attempt,
                j.message,
                j.queued_at,
                j.started_at,
                j.finished_at,
                j.claim_generation
            FROM scheduled_jobs j
            LEFT JOIN api_keys k ON k.id = j.key_id
            {}
            ORDER BY COALESCE(j.started_at, j.queued_at) DESC, j.id DESC
            LIMIT ? OFFSET ?
            "#,
            where_clause
        );

        let rows = sqlx::query(&select_query)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;

        let items = rows
            .into_iter()
            .map(|row| -> Result<JobLog, sqlx::Error> {
                Ok(JobLog {
                    id: row.try_get("id")?,
                    job_type: row.try_get("job_type")?,
                    trigger_source: row.try_get("trigger_source")?,
                    key_id: row.try_get::<Option<String>, _>("key_id")?,
                    key_group: row.try_get::<Option<String>, _>("key_group")?,
                    status: row.try_get("status")?,
                    attempt: row.try_get("attempt")?,
                    message: row.try_get::<Option<String>, _>("message")?,
                    queued_at: row.try_get("queued_at")?,
                    started_at: row.try_get("started_at")?,
                    finished_at: row.try_get::<Option<i64>, _>("finished_at")?,
                    claim_generation: row.try_get("claim_generation")?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((items, total, group_counts))
    }

    fn scheduled_job_group_filter_clause(group: &str, column: &str) -> String {
        let condition = match group {
            "quota" => format!(
                "{column} = 'quota_sync' OR {column} = 'quota_sync/manual' OR {column} = 'quota_sync/hot'"
            ),
            "usage" => format!("{column} = 'token_usage_rollup' OR {column} = 'usage_aggregation'"),
            "logs" => format!(
                "{column} = 'auth_token_logs_gc' OR {column} = 'ha_outbox_gc' OR {column} = 'request_logs_gc' OR {column} = 'mcp_sessions_gc' OR {column} = 'mcp_session_init_backoffs_gc' OR {column} = 'log_cleanup'"
            ),
            "db" => format!("{column} = 'db_compaction'"),
            "geo" => format!("{column} = 'forward_proxy_geo_refresh'"),
            "linuxdo" => format!(
                "{column} = 'linuxdo_user_status_sync' OR {column} = 'linuxdo_user_tag_binding_refresh'"
            ),
            _ => return String::new(),
        };
        format!("WHERE {condition}")
    }

    async fn fetch_recent_job_group_counts(&self) -> Result<JobGroupCounts, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) AS all_count,
                COALESCE(SUM(CASE WHEN job_type = 'quota_sync' OR job_type = 'quota_sync/manual' OR job_type = 'quota_sync/hot' THEN 1 ELSE 0 END), 0) AS quota_count,
                COALESCE(SUM(CASE WHEN job_type = 'token_usage_rollup' OR job_type = 'usage_aggregation' THEN 1 ELSE 0 END), 0) AS usage_count,
                COALESCE(SUM(CASE WHEN job_type = 'auth_token_logs_gc' OR job_type = 'ha_outbox_gc' OR job_type = 'request_logs_gc' OR job_type = 'mcp_sessions_gc' OR job_type = 'mcp_session_init_backoffs_gc' OR job_type = 'log_cleanup' THEN 1 ELSE 0 END), 0) AS logs_count,
                COALESCE(SUM(CASE WHEN job_type = 'db_compaction' THEN 1 ELSE 0 END), 0) AS db_count,
                COALESCE(SUM(CASE WHEN job_type = 'forward_proxy_geo_refresh' THEN 1 ELSE 0 END), 0) AS geo_count,
                COALESCE(SUM(CASE WHEN job_type = 'linuxdo_user_status_sync' OR job_type = 'linuxdo_user_tag_binding_refresh' THEN 1 ELSE 0 END), 0) AS linuxdo_count
            FROM scheduled_jobs
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(JobGroupCounts {
            all: row.try_get("all_count")?,
            quota: row.try_get("quota_count")?,
            usage: row.try_get("usage_count")?,
            logs: row.try_get("logs_count")?,
            db: row.try_get("db_count")?,
            geo: row.try_get("geo_count")?,
            linuxdo: row.try_get("linuxdo_count")?,
        })
    }

    pub(crate) async fn fetch_summary_without_flush_tx(
        tx: &mut sqlx::Transaction<'_, Sqlite>,
    ) -> Result<ProxySummary, ProxyError> {
        let totals_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(total_requests), 0) AS total_requests,
                COALESCE(SUM(success_count), 0) AS success_count,
                COALESCE(SUM(error_count), 0) AS error_count,
                COALESCE(SUM(quota_exhausted_count), 0) AS quota_exhausted_count
            FROM api_key_usage_buckets
            WHERE bucket_secs = 86400
            "#,
        )
        .fetch_one(&mut **tx)
        .await?;

        let key_counts_row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL AND tb.key_id IS NULL THEN 1 ELSE 0 END), 0) AS active_keys,
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL THEN 1 ELSE 0 END), 0) AS exhausted_keys,
                COALESCE(SUM(CASE WHEN aq.key_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS quarantined_keys,
                COALESCE(SUM(CASE WHEN ak.status = ? AND aq.key_id IS NULL AND tb.key_id IS NOT NULL THEN 1 ELSE 0 END), 0) AS temporary_isolated_keys
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            LEFT JOIN (
                SELECT key_id, MAX(cooldown_until) AS cooldown_until
                FROM api_key_transient_backoffs
                WHERE cooldown_until > strftime('%s', 'now')
                  AND reason_code = 'upstream_unknown_403'
                GROUP BY key_id
            ) AS tb
              ON tb.key_id = ak.id
            WHERE ak.deleted_at IS NULL
            "#,
        )
        .bind(STATUS_ACTIVE)
        .bind(STATUS_EXHAUSTED)
        .bind(STATUS_ACTIVE)
        .fetch_one(&mut **tx)
        .await?;

        let last_activity = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT MAX(last_used_at) FROM api_keys WHERE deleted_at IS NULL",
        )
        .fetch_one(&mut **tx)
        .await?
        .and_then(normalize_timestamp);

        // Aggregate quotas for overview
        let quotas_row = sqlx::query(
            r#"
            SELECT COALESCE(SUM(quota_limit), 0) AS total_quota_limit,
                   COALESCE(SUM(quota_remaining), 0) AS total_quota_remaining
            FROM api_keys ak
            LEFT JOIN api_key_quarantines aq
              ON aq.key_id = ak.id AND aq.cleared_at IS NULL
            WHERE ak.deleted_at IS NULL
              AND aq.key_id IS NULL
            "#,
        )
        .fetch_one(&mut **tx)
        .await?;

        Ok(ProxySummary {
            total_requests: totals_row.try_get("total_requests")?,
            success_count: totals_row.try_get("success_count")?,
            error_count: totals_row.try_get("error_count")?,
            quota_exhausted_count: totals_row.try_get("quota_exhausted_count")?,
            active_keys: key_counts_row.try_get("active_keys")?,
            exhausted_keys: key_counts_row.try_get("exhausted_keys")?,
            quarantined_keys: key_counts_row.try_get("quarantined_keys")?,
            temporary_isolated_keys: key_counts_row.try_get("temporary_isolated_keys")?,
            last_activity,
            total_quota_limit: quotas_row.try_get("total_quota_limit")?,
            total_quota_remaining: quotas_row.try_get("total_quota_remaining")?,
        })
    }

    pub(crate) async fn fetch_summary_without_flush(&self) -> Result<ProxySummary, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let summary = Self::fetch_summary_without_flush_tx(&mut tx).await?;
        tx.commit().await?;
        Ok(summary)
    }

    pub(crate) async fn fetch_summary(&self) -> Result<ProxySummary, ProxyError> {
        self.fetch_summary_without_flush().await
    }
}

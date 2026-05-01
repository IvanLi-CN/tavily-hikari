impl KeyStore {
    async fn reset_request_kind_canonical_migration_v1_markers(&self) -> Result<(), ProxyError> {
        let mut conn = begin_immediate_sqlite_connection(&self.pool).await?;
        for key in [
            META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_DONE,
            META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE,
            META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_REQUEST_LOGS_UPPER_BOUND,
            META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_AUTH_TOKEN_LOGS_UPPER_BOUND,
            META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_REQUEST_LOGS_CURSOR_V1,
            META_KEY_REQUEST_KIND_CANONICAL_BACKFILL_AUTH_TOKEN_LOGS_CURSOR_V1,
        ] {
            delete_meta_key_with_connection(&mut conn, key).await?;
        }
        sqlx::query("COMMIT").execute(&mut *conn).await?;
        Ok(())
    }

    pub(crate) async fn ensure_request_kind_canonical_migration_v1(
        &self,
    ) -> Result<(), ProxyError> {
        loop {
            match self
                .try_claim_request_kind_canonical_migration_v1(Utc::now().timestamp())
                .await?
            {
                RequestKindCanonicalMigrationClaim::AlreadyDone(_) => return Ok(()),
                RequestKindCanonicalMigrationClaim::Claimed => break,
                RequestKindCanonicalMigrationClaim::RunningElsewhere(_)
                | RequestKindCanonicalMigrationClaim::RetryLater => {
                    tokio::time::sleep(Duration::from_millis(
                        REQUEST_KIND_CANONICAL_MIGRATION_WAIT_POLL_MS,
                    ))
                    .await;
                }
            }
        }

        let upper_bounds = read_request_kind_canonical_backfill_upper_bounds(&self.pool)
            .await?
            .ok_or_else(|| {
                ProxyError::Other(
                    "request kind canonical migration missing persisted upper bounds".to_string(),
                )
            })?;

        match run_request_kind_canonical_backfill_with_pool(
            &self.pool,
            REQUEST_KIND_CANONICAL_BACKFILL_BATCH_SIZE,
            false,
            Some(META_KEY_REQUEST_KIND_CANONICAL_MIGRATION_V1_STATE),
            Some(upper_bounds),
        )
        .await
        {
            Ok(_) => {
                self.finish_request_kind_canonical_migration_v1(
                    RequestKindCanonicalMigrationState::Done(Utc::now().timestamp()),
                )
                .await
            }
            Err(err) => {
                self.finish_request_kind_canonical_migration_v1(
                    RequestKindCanonicalMigrationState::Failed(Utc::now().timestamp()),
                )
                .await?;
                Err(err)
            }
        }
    }

    pub(crate) async fn ensure_dev_open_admin_token(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO auth_tokens (
                id,
                secret,
                enabled,
                note,
                group_name,
                total_requests,
                created_at,
                last_used_at,
                deleted_at
            ) VALUES (?, ?, 0, ?, NULL, 0, ?, NULL, ?)
            ON CONFLICT(id) DO NOTHING
            "#,
        )
        .bind(DEV_OPEN_ADMIN_TOKEN_ID)
        .bind(DEV_OPEN_ADMIN_TOKEN_SECRET)
        .bind(DEV_OPEN_ADMIN_TOKEN_NOTE)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn user_token_bindings_uses_single_binding_primary_key(
        &self,
    ) -> Result<bool, ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64)>(
            "SELECT name, pk FROM pragma_table_info('user_token_bindings')",
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(false);
        }

        let mut user_id_pk = 0;
        let mut token_id_pk = 0;
        for (name, pk) in rows {
            if name == "user_id" {
                user_id_pk = pk;
            } else if name == "token_id" {
                token_id_pk = pk;
            }
        }

        Ok(user_id_pk == 1 && token_id_pk == 0)
    }

    pub(crate) async fn migrate_user_token_bindings_to_multi_binding(
        &self,
    ) -> Result<(), ProxyError> {
        if !self
            .user_token_bindings_uses_single_binding_primary_key()
            .await?
        {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            CREATE TABLE user_token_bindings_v2 (
                user_id TEXT NOT NULL,
                token_id TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, token_id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (token_id) REFERENCES auth_tokens(id)
            )
            "#,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_token_bindings_v2 (user_id, token_id, created_at, updated_at)
               SELECT user_id, token_id, created_at, updated_at
               FROM user_token_bindings"#,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE user_token_bindings")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE user_token_bindings_v2 RENAME TO user_token_bindings")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn force_user_relogin_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        sqlx::query("UPDATE user_sessions SET revoked_at = ? WHERE revoked_at IS NULL")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub(crate) async fn migrate_api_key_usage_buckets_v1(&self) -> Result<(), ProxyError> {
        self.rebuild_api_key_usage_buckets().await
    }

    pub(crate) async fn backfill_api_key_usage_bucket_request_value_counts_v2(
        &self,
    ) -> Result<(), ProxyError> {
        let now_ts = Utc::now().timestamp();
        let mut read_conn = self.pool.acquire().await?;
        let mut tx = self.pool.begin().await?;

        #[derive(Clone, Copy, Default)]
        struct BucketCounts {
            total_requests: i64,
            success_count: i64,
            error_count: i64,
            quota_exhausted_count: i64,
            valuable_success_count: i64,
            valuable_failure_count: i64,
            other_success_count: i64,
            other_failure_count: i64,
            unknown_count: i64,
        }

        async fn flush_bucket_request_value_counts(
            tx: &mut Transaction<'_, Sqlite>,
            now_ts: i64,
            key: &str,
            bucket_start: i64,
            counts: BucketCounts,
        ) -> Result<(), ProxyError> {
            if counts.total_requests <= 0 {
                return Ok(());
            }
            sqlx::query(
                r#"
                INSERT INTO api_key_usage_buckets (
                    api_key_id,
                    bucket_start,
                    bucket_secs,
                    total_requests,
                    success_count,
                    error_count,
                    quota_exhausted_count,
                    valuable_success_count,
                    valuable_failure_count,
                    other_success_count,
                    other_failure_count,
                    unknown_count,
                    updated_at
                ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(api_key_id, bucket_start, bucket_secs) DO UPDATE SET
                    valuable_success_count = excluded.valuable_success_count,
                    valuable_failure_count = excluded.valuable_failure_count,
                    other_success_count = excluded.other_success_count,
                    other_failure_count = excluded.other_failure_count,
                    unknown_count = excluded.unknown_count,
                    updated_at = excluded.updated_at
                WHERE (
                    api_key_usage_buckets.valuable_success_count = 0
                    AND api_key_usage_buckets.valuable_failure_count = 0
                    AND api_key_usage_buckets.other_success_count = 0
                    AND api_key_usage_buckets.other_failure_count = 0
                    AND api_key_usage_buckets.unknown_count = 0
                ) OR (
                    api_key_usage_buckets.total_requests = excluded.total_requests
                    AND api_key_usage_buckets.success_count = excluded.success_count
                    AND api_key_usage_buckets.error_count = excluded.error_count
                    AND api_key_usage_buckets.quota_exhausted_count = excluded.quota_exhausted_count
                )
                "#,
            )
            .bind(key)
            .bind(bucket_start)
            .bind(counts.total_requests)
            .bind(counts.success_count)
            .bind(counts.error_count)
            .bind(counts.quota_exhausted_count)
            .bind(counts.valuable_success_count)
            .bind(counts.valuable_failure_count)
            .bind(counts.other_success_count)
            .bind(counts.other_failure_count)
            .bind(counts.unknown_count)
            .bind(now_ts)
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut rows = sqlx::query(
            r#"
            SELECT api_key_id, created_at, result_status, request_kind_key, request_body, path
            FROM request_logs
            WHERE visibility = ?
              AND api_key_id IS NOT NULL
            ORDER BY api_key_id ASC, created_at ASC, id ASC
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch(&mut *read_conn);

        let mut current_key: Option<String> = None;
        let mut current_bucket_start: i64 = 0;
        let mut counts = BucketCounts::default();

        while let Some(row) = rows.try_next().await? {
            let key_id: String = row.try_get("api_key_id")?;
            let created_at: i64 = row.try_get("created_at")?;
            let status: String = row.try_get("result_status")?;
            let stored_request_kind_key: Option<String> = row.try_get("request_kind_key")?;
            let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
            let path: String = row.try_get("path")?;

            let bucket_start = local_day_bucket_start_utc_ts(created_at);

            let needs_flush = match current_key.as_deref() {
                None => false,
                Some(k) if k != key_id.as_str() => true,
                Some(_) if current_bucket_start != bucket_start => true,
                _ => false,
            };

            if needs_flush {
                let key = current_key.as_deref().expect("flush key present");
                flush_bucket_request_value_counts(
                    &mut tx,
                    now_ts,
                    key,
                    current_bucket_start,
                    counts,
                )
                .await?;
                counts = BucketCounts::default();
            }

            current_key = Some(key_id);
            current_bucket_start = bucket_start;
            counts.total_requests += 1;

            let request_kind_key = canonicalize_request_log_request_kind(
                &path,
                request_body.as_deref(),
                stored_request_kind_key,
                None,
                None,
            )
            .key;
            match request_value_bucket_for_request_log(&request_kind_key, request_body.as_deref()) {
                RequestValueBucket::Valuable => match status.as_str() {
                    OUTCOME_SUCCESS => counts.valuable_success_count += 1,
                    OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => counts.valuable_failure_count += 1,
                    _ => {}
                },
                RequestValueBucket::Other => match status.as_str() {
                    OUTCOME_SUCCESS => counts.other_success_count += 1,
                    OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => counts.other_failure_count += 1,
                    _ => {}
                },
                RequestValueBucket::Unknown => counts.unknown_count += 1,
            }
            match status.as_str() {
                OUTCOME_SUCCESS => counts.success_count += 1,
                OUTCOME_ERROR => counts.error_count += 1,
                OUTCOME_QUOTA_EXHAUSTED => counts.quota_exhausted_count += 1,
                _ => {}
            }
        }

        if let Some(key) = current_key.as_deref() {
            flush_bucket_request_value_counts(&mut tx, now_ts, key, current_bucket_start, counts)
                .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn rebuild_api_key_usage_buckets(&self) -> Result<(), ProxyError> {
        // Rebuild buckets from request_logs to preserve cumulative statistics after retention.
        // This is safe to rerun because we clear and recompute deterministically.
        let now_ts = Utc::now().timestamp();
        let mut read_conn = self.pool.acquire().await?;
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM api_key_usage_buckets")
            .execute(&mut *tx)
            .await?;

        let mut rows = sqlx::query(
            r#"
            SELECT api_key_id, created_at, result_status, request_kind_key, request_body, path
            FROM request_logs
            WHERE visibility = ?
              AND api_key_id IS NOT NULL
            ORDER BY api_key_id ASC, created_at ASC, id ASC
            "#,
        )
        .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
        .fetch(&mut *read_conn);

        #[derive(Clone, Copy, Default)]
        struct BucketCounts {
            total_requests: i64,
            success_count: i64,
            error_count: i64,
            quota_exhausted_count: i64,
            valuable_success_count: i64,
            valuable_failure_count: i64,
            other_success_count: i64,
            other_failure_count: i64,
            unknown_count: i64,
        }

        async fn flush_bucket(
            tx: &mut Transaction<'_, Sqlite>,
            now_ts: i64,
            key: &str,
            bucket_start: i64,
            counts: BucketCounts,
        ) -> Result<(), ProxyError> {
            if counts.total_requests <= 0 {
                return Ok(());
            }
            sqlx::query(
                r#"
                INSERT INTO api_key_usage_buckets (
                    api_key_id,
                    bucket_start,
                    bucket_secs,
                    total_requests,
                    success_count,
                    error_count,
                    quota_exhausted_count,
                    valuable_success_count,
                    valuable_failure_count,
                    other_success_count,
                    other_failure_count,
                    unknown_count,
                    updated_at
                ) VALUES (?, ?, 86400, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(key)
            .bind(bucket_start)
            .bind(counts.total_requests)
            .bind(counts.success_count)
            .bind(counts.error_count)
            .bind(counts.quota_exhausted_count)
            .bind(counts.valuable_success_count)
            .bind(counts.valuable_failure_count)
            .bind(counts.other_success_count)
            .bind(counts.other_failure_count)
            .bind(counts.unknown_count)
            .bind(now_ts)
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut current_key: Option<String> = None;
        let mut current_bucket_start: i64 = 0;
        let mut counts = BucketCounts::default();

        while let Some(row) = rows.try_next().await? {
            let key_id: String = row.try_get("api_key_id")?;
            let created_at: i64 = row.try_get("created_at")?;
            let status: String = row.try_get("result_status")?;
            let stored_request_kind_key: Option<String> = row.try_get("request_kind_key")?;
            let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
            let path: String = row.try_get("path")?;

            let bucket_start = local_day_bucket_start_utc_ts(created_at);

            let needs_flush = match current_key.as_deref() {
                None => false,
                Some(k) if k != key_id.as_str() => true,
                Some(_) if current_bucket_start != bucket_start => true,
                _ => false,
            };

            if needs_flush {
                let key = current_key.as_deref().expect("flush key present");
                flush_bucket(&mut tx, now_ts, key, current_bucket_start, counts).await?;

                counts = BucketCounts::default();
            }

            current_key = Some(key_id);
            current_bucket_start = bucket_start;

            counts.total_requests += 1;
            let request_kind_key = canonicalize_request_log_request_kind(
                &path,
                request_body.as_deref(),
                stored_request_kind_key,
                None,
                None,
            )
            .key;
            match request_value_bucket_for_request_log(&request_kind_key, request_body.as_deref()) {
                RequestValueBucket::Valuable => match status.as_str() {
                    OUTCOME_SUCCESS => counts.valuable_success_count += 1,
                    OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => counts.valuable_failure_count += 1,
                    _ => {}
                },
                RequestValueBucket::Other => match status.as_str() {
                    OUTCOME_SUCCESS => counts.other_success_count += 1,
                    OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => counts.other_failure_count += 1,
                    _ => {}
                },
                RequestValueBucket::Unknown => counts.unknown_count += 1,
            }
            match status.as_str() {
                OUTCOME_SUCCESS => counts.success_count += 1,
                OUTCOME_ERROR => counts.error_count += 1,
                OUTCOME_QUOTA_EXHAUSTED => counts.quota_exhausted_count += 1,
                _ => {}
            }
        }

        if let Some(key) = current_key.as_deref() {
            flush_bucket(&mut tx, now_ts, key, current_bucket_start, counts).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    fn dashboard_rollup_counts_for_request(
        request_kind_key: &str,
        body: Option<&[u8]>,
        outcome: &str,
        failure_kind: Option<&str>,
        local_estimated_credits: i64,
    ) -> DashboardRequestRollupCounts {
        let mut counts = DashboardRequestRollupCounts {
            total_requests: 1,
            local_estimated_credits: local_estimated_credits.max(0),
            ..DashboardRequestRollupCounts::default()
        };

        match outcome {
            OUTCOME_SUCCESS => counts.success_count = 1,
            OUTCOME_ERROR => counts.error_count = 1,
            OUTCOME_QUOTA_EXHAUSTED => counts.quota_exhausted_count = 1,
            _ => {}
        }

        match request_value_bucket_for_request_log(request_kind_key, body) {
            RequestValueBucket::Valuable => match outcome {
                OUTCOME_SUCCESS => counts.valuable_success_count = 1,
                OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => {
                    counts.valuable_failure_count = 1;
                    if failure_kind == Some(FAILURE_KIND_UPSTREAM_RATE_LIMITED_429) {
                        counts.valuable_failure_429_count = 1;
                    }
                }
                _ => {}
            },
            RequestValueBucket::Other => match outcome {
                OUTCOME_SUCCESS => counts.other_success_count = 1,
                OUTCOME_ERROR | OUTCOME_QUOTA_EXHAUSTED => counts.other_failure_count = 1,
                _ => {}
            },
            RequestValueBucket::Unknown => counts.unknown_count = 1,
        }

        match (
            token_request_kind_protocol_group(request_kind_key),
            token_request_kind_billing_group_for_request_log(request_kind_key, body),
        ) {
            ("mcp", "non_billable") => counts.mcp_non_billable = 1,
            ("mcp", "billable") => counts.mcp_billable = 1,
            ("api", "non_billable") => counts.api_non_billable = 1,
            _ => counts.api_billable = 1,
        }

        counts
    }

    async fn upsert_dashboard_request_rollup_bucket(
        tx: &mut Transaction<'_, Sqlite>,
        bucket_start: i64,
        bucket_secs: i64,
        counts: DashboardRequestRollupCounts,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        if counts.total_requests <= 0 && counts.local_estimated_credits <= 0 {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO dashboard_request_rollup_buckets (
                bucket_start,
                bucket_secs,
                total_requests,
                success_count,
                error_count,
                quota_exhausted_count,
                valuable_success_count,
                valuable_failure_count,
                valuable_failure_429_count,
                other_success_count,
                other_failure_count,
                unknown_count,
                mcp_non_billable,
                mcp_billable,
                api_non_billable,
                api_billable,
                local_estimated_credits,
                updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(bucket_start, bucket_secs)
            DO UPDATE SET
                total_requests = dashboard_request_rollup_buckets.total_requests + excluded.total_requests,
                success_count = dashboard_request_rollup_buckets.success_count + excluded.success_count,
                error_count = dashboard_request_rollup_buckets.error_count + excluded.error_count,
                quota_exhausted_count = dashboard_request_rollup_buckets.quota_exhausted_count + excluded.quota_exhausted_count,
                valuable_success_count = dashboard_request_rollup_buckets.valuable_success_count + excluded.valuable_success_count,
                valuable_failure_count = dashboard_request_rollup_buckets.valuable_failure_count + excluded.valuable_failure_count,
                valuable_failure_429_count = dashboard_request_rollup_buckets.valuable_failure_429_count + excluded.valuable_failure_429_count,
                other_success_count = dashboard_request_rollup_buckets.other_success_count + excluded.other_success_count,
                other_failure_count = dashboard_request_rollup_buckets.other_failure_count + excluded.other_failure_count,
                unknown_count = dashboard_request_rollup_buckets.unknown_count + excluded.unknown_count,
                mcp_non_billable = dashboard_request_rollup_buckets.mcp_non_billable + excluded.mcp_non_billable,
                mcp_billable = dashboard_request_rollup_buckets.mcp_billable + excluded.mcp_billable,
                api_non_billable = dashboard_request_rollup_buckets.api_non_billable + excluded.api_non_billable,
                api_billable = dashboard_request_rollup_buckets.api_billable + excluded.api_billable,
                local_estimated_credits = dashboard_request_rollup_buckets.local_estimated_credits + excluded.local_estimated_credits,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(bucket_start)
        .bind(bucket_secs)
        .bind(counts.total_requests)
        .bind(counts.success_count)
        .bind(counts.error_count)
        .bind(counts.quota_exhausted_count)
        .bind(counts.valuable_success_count)
        .bind(counts.valuable_failure_count)
        .bind(counts.valuable_failure_429_count)
        .bind(counts.other_success_count)
        .bind(counts.other_failure_count)
        .bind(counts.unknown_count)
        .bind(counts.mcp_non_billable)
        .bind(counts.mcp_billable)
        .bind(counts.api_non_billable)
        .bind(counts.api_billable)
        .bind(counts.local_estimated_credits)
        .bind(updated_at)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn rebuild_dashboard_request_rollup_buckets(&self) -> Result<(), ProxyError> {
        self.rebuild_dashboard_request_rollup_buckets_window(None, None)
            .await
    }

    pub(crate) async fn rebuild_dashboard_request_rollup_buckets_window(
        &self,
        start: Option<i64>,
        end: Option<i64>,
    ) -> Result<(), ProxyError> {
        let now_ts = Utc::now().timestamp();
        let mut tx = self.pool.begin().await?;

        match (start, end) {
            (Some(start), Some(end)) if start < end => {
                let minute_start = start.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
                let minute_end = (end.saturating_sub(1)).div_euclid(SECS_PER_MINUTE)
                    * SECS_PER_MINUTE
                    + SECS_PER_MINUTE;
                let day_start = local_day_bucket_start_utc_ts(start);
                let day_end = next_local_day_start_utc_ts(local_day_bucket_start_utc_ts(
                    end.saturating_sub(1),
                ));
                sqlx::query(
                    r#"
                    DELETE FROM dashboard_request_rollup_buckets
                    WHERE (bucket_secs = 60 AND bucket_start >= ? AND bucket_start < ?)
                       OR (bucket_secs = 86400 AND bucket_start >= ? AND bucket_start < ?)
                    "#,
                )
                .bind(minute_start)
                .bind(minute_end)
                .bind(day_start)
                .bind(day_end)
                .execute(&mut *tx)
                .await?;

                {
                    let mut read_conn = self.pool.acquire().await?;
                    let mut rows = sqlx::query(
                        r#"
                        SELECT created_at, result_status, failure_kind, request_kind_key, request_body, path, business_credits
                        FROM request_logs
                        WHERE visibility = ?
                          AND created_at >= ?
                          AND created_at < ?
                        ORDER BY created_at ASC, id ASC
                        "#,
                    )
                    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                    .bind(minute_start)
                    .bind(minute_end)
                    .fetch(&mut *read_conn);

                    let mut current_bucket_start: Option<i64> = None;
                    let mut counts = DashboardRequestRollupCounts::default();
                    while let Some(row) = rows.try_next().await? {
                        let created_at: i64 = row.try_get("created_at")?;
                        let result_status: String = row.try_get("result_status")?;
                        let failure_kind: Option<String> = row.try_get("failure_kind")?;
                        let stored_request_kind_key: Option<String> =
                            row.try_get("request_kind_key")?;
                        let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                        let path: String = row.try_get("path")?;
                        let business_credits: Option<i64> = row.try_get("business_credits")?;
                        let bucket_start = created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;

                        if current_bucket_start != Some(bucket_start) {
                            if let Some(previous_bucket_start) = current_bucket_start {
                                Self::upsert_dashboard_request_rollup_bucket(
                                    &mut tx,
                                    previous_bucket_start,
                                    SECS_PER_MINUTE,
                                    counts,
                                    now_ts,
                                )
                                .await?;
                            }
                            current_bucket_start = Some(bucket_start);
                            counts = DashboardRequestRollupCounts::default();
                        }

                        let request_kind_key = canonicalize_request_log_request_kind(
                            &path,
                            request_body.as_deref(),
                            stored_request_kind_key,
                            None,
                            None,
                        )
                        .key;
                        counts.add(Self::dashboard_rollup_counts_for_request(
                            &request_kind_key,
                            request_body.as_deref(),
                            &result_status,
                            failure_kind.as_deref(),
                            business_credits.unwrap_or_default(),
                        ));
                    }

                    if let Some(bucket_start) = current_bucket_start {
                        Self::upsert_dashboard_request_rollup_bucket(
                            &mut tx,
                            bucket_start,
                            SECS_PER_MINUTE,
                            counts,
                            now_ts,
                        )
                        .await?;
                    }
                }

                {
                    let mut read_conn = self.pool.acquire().await?;
                    let mut rows = sqlx::query(
                        r#"
                        SELECT created_at, result_status, failure_kind, request_kind_key, request_body, path, business_credits
                        FROM request_logs
                        WHERE visibility = ?
                          AND created_at >= ?
                          AND created_at < ?
                        ORDER BY created_at ASC, id ASC
                        "#,
                    )
                    .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                    .bind(day_start)
                    .bind(day_end)
                    .fetch(&mut *read_conn);

                    let mut current_bucket_start: Option<i64> = None;
                    let mut counts = DashboardRequestRollupCounts::default();
                    while let Some(row) = rows.try_next().await? {
                        let created_at: i64 = row.try_get("created_at")?;
                        let result_status: String = row.try_get("result_status")?;
                        let failure_kind: Option<String> = row.try_get("failure_kind")?;
                        let stored_request_kind_key: Option<String> =
                            row.try_get("request_kind_key")?;
                        let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                        let path: String = row.try_get("path")?;
                        let business_credits: Option<i64> = row.try_get("business_credits")?;
                        let bucket_start = local_day_bucket_start_utc_ts(created_at);

                        if current_bucket_start != Some(bucket_start) {
                            if let Some(previous_bucket_start) = current_bucket_start {
                                Self::upsert_dashboard_request_rollup_bucket(
                                    &mut tx,
                                    previous_bucket_start,
                                    SECS_PER_DAY,
                                    counts,
                                    now_ts,
                                )
                                .await?;
                            }
                            current_bucket_start = Some(bucket_start);
                            counts = DashboardRequestRollupCounts::default();
                        }

                        let request_kind_key = canonicalize_request_log_request_kind(
                            &path,
                            request_body.as_deref(),
                            stored_request_kind_key,
                            None,
                            None,
                        )
                        .key;
                        counts.add(Self::dashboard_rollup_counts_for_request(
                            &request_kind_key,
                            request_body.as_deref(),
                            &result_status,
                            failure_kind.as_deref(),
                            business_credits.unwrap_or_default(),
                        ));
                    }

                    if let Some(bucket_start) = current_bucket_start {
                        Self::upsert_dashboard_request_rollup_bucket(
                            &mut tx,
                            bucket_start,
                            SECS_PER_DAY,
                            counts,
                            now_ts,
                        )
                        .await?;
                    }
                }
            }
            (Some(_), Some(_)) => return Ok(()),
            _ => {
                sqlx::query("DELETE FROM dashboard_request_rollup_buckets")
                    .execute(&mut *tx)
                    .await?;
                let mut read_conn = self.pool.acquire().await?;
                let mut rows = sqlx::query(
                    r#"
                    SELECT created_at, result_status, failure_kind, request_kind_key, request_body, path, business_credits
                    FROM request_logs
                    WHERE visibility = ?
                    ORDER BY created_at ASC, id ASC
                    "#,
                )
                .bind(REQUEST_LOG_VISIBILITY_VISIBLE)
                .fetch(&mut *read_conn);

                let mut current_minute_bucket_start: Option<i64> = None;
                let mut current_day_bucket_start: Option<i64> = None;
                let mut minute_counts = DashboardRequestRollupCounts::default();
                let mut day_counts = DashboardRequestRollupCounts::default();

                while let Some(row) = rows.try_next().await? {
                    let created_at: i64 = row.try_get("created_at")?;
                    let result_status: String = row.try_get("result_status")?;
                    let failure_kind: Option<String> = row.try_get("failure_kind")?;
                    let stored_request_kind_key: Option<String> =
                        row.try_get("request_kind_key")?;
                    let request_body: Option<Vec<u8>> = row.try_get("request_body")?;
                    let path: String = row.try_get("path")?;
                    let business_credits: Option<i64> = row.try_get("business_credits")?;
                    let minute_bucket_start =
                        created_at.div_euclid(SECS_PER_MINUTE) * SECS_PER_MINUTE;
                    let day_bucket_start = local_day_bucket_start_utc_ts(created_at);

                    if current_minute_bucket_start != Some(minute_bucket_start) {
                        if let Some(bucket_start) = current_minute_bucket_start {
                            Self::upsert_dashboard_request_rollup_bucket(
                                &mut tx,
                                bucket_start,
                                SECS_PER_MINUTE,
                                minute_counts,
                                now_ts,
                            )
                            .await?;
                        }
                        current_minute_bucket_start = Some(minute_bucket_start);
                        minute_counts = DashboardRequestRollupCounts::default();
                    }

                    if current_day_bucket_start != Some(day_bucket_start) {
                        if let Some(bucket_start) = current_day_bucket_start {
                            Self::upsert_dashboard_request_rollup_bucket(
                                &mut tx,
                                bucket_start,
                                SECS_PER_DAY,
                                day_counts,
                                now_ts,
                            )
                            .await?;
                        }
                        current_day_bucket_start = Some(day_bucket_start);
                        day_counts = DashboardRequestRollupCounts::default();
                    }

                    let request_kind_key = canonicalize_request_log_request_kind(
                        &path,
                        request_body.as_deref(),
                        stored_request_kind_key,
                        None,
                        None,
                    )
                    .key;
                    let delta = Self::dashboard_rollup_counts_for_request(
                        &request_kind_key,
                        request_body.as_deref(),
                        &result_status,
                        failure_kind.as_deref(),
                        business_credits.unwrap_or_default(),
                    );
                    minute_counts.add(delta);
                    day_counts.add(delta);
                }

                if let Some(bucket_start) = current_minute_bucket_start {
                    Self::upsert_dashboard_request_rollup_bucket(
                        &mut tx,
                        bucket_start,
                        SECS_PER_MINUTE,
                        minute_counts,
                        now_ts,
                    )
                    .await?;
                }
                if let Some(bucket_start) = current_day_bucket_start {
                    Self::upsert_dashboard_request_rollup_bucket(
                        &mut tx,
                        bucket_start,
                        SECS_PER_DAY,
                        day_counts,
                        now_ts,
                    )
                    .await?;
                }
            }
        }

        tx.commit().await?;
        Ok(())
    }

    /// Reconcile derived fields to ensure cross-table consistency.
    /// This migration is idempotent and safe to run on every startup.
    pub(crate) async fn migrate_data_consistency(&self) -> Result<(), ProxyError> {
        // 1) Access tokens: recompute total_requests and last_used_at from auth_token_logs
        //    Older versions incremented total_requests during validation, which
        //    inflated counters. The canonical source of truth is auth_token_logs.
        sqlx::query(
            r#"
            UPDATE auth_tokens
            SET total_requests = COALESCE((
                    SELECT COUNT(*) FROM auth_token_logs l WHERE l.token_id = auth_tokens.id
                ), 0),
                last_used_at = (
                    SELECT MAX(created_at) FROM auth_token_logs l WHERE l.token_id = auth_tokens.id
                )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 2) API keys: refresh last_used_at from request_logs to avoid stale values
        //    (This is a best-effort consistency update; it's safe and general.)
        sqlx::query(
            r#"
            UPDATE api_keys
            SET last_used_at = COALESCE((
                SELECT MAX(created_at) FROM request_logs r WHERE r.api_key_id = api_keys.id
            ), last_used_at)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Ensure that every token_id referenced in auth_token_logs has a corresponding
    /// auth_tokens row. Missing rows are backfilled as disabled, soft-deleted tokens
    /// so that downstream usage aggregation into token_usage_stats (with FOREIGN KEYs)
    /// does not fail for legacy data.
    pub(crate) async fn heal_orphan_auth_tokens_from_logs(&self) -> Result<(), ProxyError> {
        // Skip if auth_token_logs table does not exist (very old databases).
        let has_logs_table = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'auth_token_logs' LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        if has_logs_table.is_none() {
            self.set_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1, 0).await?;
            return Ok(());
        }

        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO auth_tokens (
                id,
                secret,
                enabled,
                note,
                group_name,
                total_requests,
                created_at,
                last_used_at,
                deleted_at
            )
            SELECT
                l.token_id,
                'restored-from-logs',
                0,
                '[auto-restored from logs]',
                NULL,
                COUNT(*) AS total_requests,
                MIN(l.created_at) AS created_at,
                MAX(l.created_at) AS last_used_at,
                ?
            FROM auth_token_logs l
            LEFT JOIN auth_tokens t ON t.id = l.token_id
            WHERE t.id IS NULL
            GROUP BY l.token_id
            "#,
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Record completion so this healer is only ever run once per database.
        self.set_meta_i64(META_KEY_HEAL_ORPHAN_TOKENS_V1, now)
            .await?;

        Ok(())
    }

    pub(crate) async fn backfill_account_quota_v1(&self) -> Result<(), ProxyError> {
        let now = Utc::now().timestamp();
        let hourly_any_limit = effective_token_hourly_request_limit();
        let hourly_limit = effective_token_hourly_limit();
        let daily_limit = effective_token_daily_limit();
        let monthly_limit = effective_token_monthly_limit();

        // Ensure every bound account has a default limits row.
        sqlx::query(
            r#"
            INSERT INTO account_quota_limits (
                user_id,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
                created_at,
                updated_at
            )
            SELECT
                b.user_id,
                ?,
                ?,
                ?,
                ?,
                ?,
                ?
            FROM user_token_bindings b
            GROUP BY b.user_id
            ON CONFLICT(user_id) DO NOTHING
            "#,
        )
        .bind(hourly_any_limit)
        .bind(hourly_limit)
        .bind(daily_limit)
        .bind(monthly_limit)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Copy existing token rolling buckets to account scope.
        sqlx::query(
            r#"
            INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
            SELECT
                b.user_id,
                u.bucket_start,
                u.granularity,
                SUM(u.count) AS count
            FROM user_token_bindings b
            JOIN token_usage_buckets u ON u.token_id = b.token_id
            GROUP BY b.user_id, u.bucket_start, u.granularity
            ON CONFLICT(user_id, bucket_start, granularity)
            DO UPDATE SET count = excluded.count
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Copy monthly counters to account scope. If multiple tokens map to one account,
        // keep the latest month_start and aggregate counts in that month.
        sqlx::query(
            r#"
            WITH mapped AS (
                SELECT b.user_id AS user_id, q.month_start AS month_start, q.month_count AS month_count
                FROM user_token_bindings b
                JOIN auth_token_quota q ON q.token_id = b.token_id
            ),
            latest AS (
                SELECT user_id, MAX(month_start) AS latest_month_start
                FROM mapped
                GROUP BY user_id
            )
            INSERT INTO account_monthly_quota (user_id, month_start, month_count)
            SELECT
                l.user_id,
                l.latest_month_start,
                COALESCE(SUM(CASE WHEN m.month_start = l.latest_month_start THEN m.month_count ELSE 0 END), 0)
            FROM latest l
            LEFT JOIN mapped m ON m.user_id = l.user_id
            GROUP BY l.user_id, l.latest_month_start
            ON CONFLICT(user_id) DO UPDATE SET
                month_start = excluded.month_start,
                month_count = excluded.month_count
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub(crate) async fn increment_usage_bucket_by(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
        delta: i64,
    ) -> Result<(), ProxyError> {
        if delta <= 0 {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO token_usage_buckets (token_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(token_id, bucket_start, granularity)
            DO UPDATE SET count = token_usage_buckets.count + excluded.count
            "#,
        )
        .bind(token_id)
        .bind(bucket_start)
        .bind(granularity)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn increment_usage_bucket(
        &self,
        token_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        self.increment_usage_bucket_by(token_id, bucket_start, granularity, 1)
            .await
    }

    pub(crate) async fn increment_account_usage_bucket_by(
        &self,
        user_id: &str,
        bucket_start: i64,
        granularity: &str,
        delta: i64,
    ) -> Result<(), ProxyError> {
        if delta <= 0 {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO account_usage_buckets (user_id, bucket_start, granularity, count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(user_id, bucket_start, granularity)
            DO UPDATE SET count = account_usage_buckets.count + excluded.count
            "#,
        )
        .bind(user_id)
        .bind(bucket_start)
        .bind(granularity)
        .bind(delta)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn increment_account_usage_bucket(
        &self,
        user_id: &str,
        bucket_start: i64,
        granularity: &str,
    ) -> Result<(), ProxyError> {
        self.increment_account_usage_bucket_by(user_id, bucket_start, granularity, 1)
            .await
    }

    pub(crate) async fn increment_api_key_user_usage_bucket(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        api_key_id: &str,
        user_id: &str,
        bucket_start: i64,
        credits: i64,
        result_status: &str,
    ) -> Result<(), ProxyError> {
        if credits <= 0 {
            return Ok(());
        }
        let (success_credits, failure_credits) = if result_status == OUTCOME_SUCCESS {
            (credits, 0_i64)
        } else {
            (0_i64, credits)
        };
        let now = Utc::now().timestamp();
        sqlx::query(
            r#"
            INSERT INTO api_key_user_usage_buckets (
                api_key_id,
                user_id,
                bucket_start,
                bucket_secs,
                success_credits,
                failure_credits,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(api_key_id, user_id, bucket_start, bucket_secs)
            DO UPDATE SET
                success_credits = api_key_user_usage_buckets.success_credits + excluded.success_credits,
                failure_credits = api_key_user_usage_buckets.failure_credits + excluded.failure_credits,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(api_key_id)
        .bind(user_id)
        .bind(bucket_start)
        .bind(SECS_PER_DAY)
        .bind(success_credits)
        .bind(failure_credits)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn refresh_user_api_key_binding(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        api_key_id: &str,
        success_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO user_api_key_bindings (
                user_id,
                api_key_id,
                created_at,
                updated_at,
                last_success_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(user_id, api_key_id)
            DO UPDATE SET
                updated_at = CASE
                    WHEN excluded.last_success_at >= user_api_key_bindings.last_success_at THEN excluded.updated_at
                    ELSE user_api_key_bindings.updated_at
                END,
                last_success_at = MAX(user_api_key_bindings.last_success_at, excluded.last_success_at)
            "#,
        )
        .bind(user_id)
        .bind(api_key_id)
        .bind(success_at)
        .bind(success_at)
        .bind(success_at)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM user_api_key_bindings
            WHERE user_id = ?
              AND api_key_id IN (
                  SELECT api_key_id
                  FROM user_api_key_bindings
                  WHERE user_id = ?
                  ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
                  LIMIT -1 OFFSET ?
              )
            "#,
        )
        .bind(user_id)
        .bind(user_id)
        .bind(USER_API_KEY_BINDING_RECENT_LIMIT)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub(crate) async fn refresh_token_api_key_binding(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        token_id: &str,
        api_key_id: &str,
        success_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO token_api_key_bindings (
                token_id,
                api_key_id,
                created_at,
                updated_at,
                last_success_at
            )
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(token_id, api_key_id)
            DO UPDATE SET
                updated_at = CASE
                    WHEN excluded.last_success_at >= token_api_key_bindings.last_success_at THEN excluded.updated_at
                    ELSE token_api_key_bindings.updated_at
                END,
                last_success_at = MAX(token_api_key_bindings.last_success_at, excluded.last_success_at)
            "#,
        )
        .bind(token_id)
        .bind(api_key_id)
        .bind(success_at)
        .bind(success_at)
        .bind(success_at)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM token_api_key_bindings
            WHERE token_id = ?
              AND api_key_id IN (
                  SELECT api_key_id
                  FROM token_api_key_bindings
                  WHERE token_id = ?
                  ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
                  LIMIT -1 OFFSET ?
              )
            "#,
        )
        .bind(token_id)
        .bind(token_id)
        .bind(TOKEN_API_KEY_BINDING_RECENT_LIMIT)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_subject_key_breakage_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        subject_kind: &str,
        subject_id: &str,
        key_id: &str,
        break_at: i64,
        key_status: &str,
        reason_code: Option<&str>,
        reason_summary: Option<&str>,
        source: &str,
        breaker_token_id: Option<&str>,
        breaker_user_id: Option<&str>,
        breaker_user_display_name: Option<&str>,
        manual_actor_display_name: Option<&str>,
    ) -> Result<(), ProxyError> {
        let month_start = start_of_month(
            Utc.timestamp_opt(break_at, 0)
                .single()
                .unwrap_or_else(Utc::now),
        )
        .timestamp();
        sqlx::query(
            r#"
            INSERT INTO subject_key_breakages (
                subject_kind,
                subject_id,
                key_id,
                month_start,
                created_at,
                updated_at,
                latest_break_at,
                key_status,
                reason_code,
                reason_summary,
                source,
                breaker_token_id,
                breaker_user_id,
                breaker_user_display_name,
                manual_actor_display_name
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(subject_kind, subject_id, key_id, month_start)
            DO UPDATE SET
                updated_at = excluded.updated_at,
                latest_break_at = MAX(subject_key_breakages.latest_break_at, excluded.latest_break_at),
                key_status = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.key_status
                    ELSE subject_key_breakages.key_status
                END,
                reason_code = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.reason_code
                    ELSE subject_key_breakages.reason_code
                END,
                reason_summary = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.reason_summary
                    ELSE subject_key_breakages.reason_summary
                END,
                source = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.source
                    ELSE subject_key_breakages.source
                END,
                breaker_token_id = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.breaker_token_id
                    ELSE subject_key_breakages.breaker_token_id
                END,
                breaker_user_id = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.breaker_user_id
                    ELSE subject_key_breakages.breaker_user_id
                END,
                breaker_user_display_name = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.breaker_user_display_name
                    ELSE subject_key_breakages.breaker_user_display_name
                END,
                manual_actor_display_name = CASE
                    WHEN excluded.latest_break_at >= subject_key_breakages.latest_break_at THEN excluded.manual_actor_display_name
                    ELSE subject_key_breakages.manual_actor_display_name
                END
            "#,
        )
        .bind(subject_kind)
        .bind(subject_id)
        .bind(key_id)
        .bind(month_start)
        .bind(break_at)
        .bind(break_at)
        .bind(break_at)
        .bind(key_status)
        .bind(reason_code)
        .bind(reason_summary)
        .bind(source)
        .bind(breaker_token_id)
        .bind(breaker_user_id)
        .bind(breaker_user_display_name)
        .bind(manual_actor_display_name)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn sum_usage_buckets(
        &self,
        token_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM token_usage_buckets
            WHERE token_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(token_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_usage_buckets_between(
        &self,
        token_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM token_usage_buckets
            WHERE token_id = ?
              AND granularity = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            "#,
        )
        .bind(token_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets(
        &self,
        user_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM account_usage_buckets
            WHERE user_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(user_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets_between(
        &self,
        user_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<i64, ProxyError> {
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM account_usage_buckets
            WHERE user_id = ?
              AND granularity = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            "#,
        )
        .bind(user_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, SUM(count) as total FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, total) in rows {
            map.insert(user_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn sum_account_usage_buckets_bulk_between(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, SUM(count) as total FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND bucket_start < ");
        builder.push_bind(bucket_start_before);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, total) in rows {
            map.insert(user_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn sum_usage_buckets_bulk(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, SUM(count) as total FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, total) in rows {
            map.insert(token_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn sum_usage_buckets_bulk_between(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, SUM(count) as total FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND bucket_start < ");
        builder.push_bind(bucket_start_before);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, total) in rows {
            map.insert(token_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn earliest_usage_bucket_since_bulk(
        &self,
        token_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, MIN(bucket_start) as earliest FROM token_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND token_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY token_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (token_id, bucket_start) in rows {
            map.insert(token_id, bucket_start);
        }
        Ok(map)
    }

    pub(crate) async fn earliest_account_usage_bucket_since_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, MIN(bucket_start) as earliest FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, bucket_start) in rows {
            map.insert(user_id, bucket_start);
        }
        Ok(map)
    }

    pub(crate) async fn fetch_monthly_counts(
        &self,
        token_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT token_id, month_start, month_count FROM auth_token_quota WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (token_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(token_id.clone(), 0);
                stale_ids.push(token_id);
            } else {
                map.insert(token_id, stored_count);
            }
        }

        for token_id in stale_ids {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(&token_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    pub(crate) async fn fetch_monthly_count(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(token_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    pub(crate) async fn fetch_account_monthly_count(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    pub(crate) async fn fetch_account_monthly_counts(
        &self,
        user_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::new(
            "SELECT user_id, month_start, month_count FROM account_monthly_quota WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (user_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(user_id.clone(), 0);
                stale_ids.push(user_id);
            } else {
                map.insert(user_id, stored_count);
            }
        }

        for user_id in stale_ids {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(&user_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    pub(crate) async fn delete_old_usage_buckets(
        &self,
        granularity: &str,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM token_usage_buckets
            WHERE granularity = ? AND bucket_start < ?
            "#,
        )
        .bind(granularity)
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn delete_old_account_usage_buckets(
        &self,
        granularity: &str,
        threshold: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            DELETE FROM account_usage_buckets
            WHERE granularity = ? AND bucket_start < ?
            "#,
        )
        .bind(granularity)
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete per-token usage logs older than the given threshold.
    /// This is strictly time-based and deliberately independent of token status,
    /// so that audit trails are not coupled to enable/disable/delete operations.
    pub(crate) async fn delete_old_auth_token_logs(
        &self,
        threshold: i64,
    ) -> Result<i64, ProxyError> {
        let result = sqlx::query(
            r#"
            DELETE FROM auth_token_logs
            WHERE created_at < ?
            "#,
        )
        .bind(threshold)
        .execute(&self.pool)
        .await?;

        self.invalidate_request_logs_catalog_cache().await;
        Ok(result.rows_affected() as i64)
    }

    pub(crate) async fn delete_old_request_logs(&self, threshold: i64) -> Result<i64, ProxyError> {
        // Batched deletes reduce long-running write locks on large tables.
        const BATCH_SIZE: i64 = 5_000;
        let mut total_deleted = 0_i64;
        loop {
            let result = sqlx::query(
                r#"
                DELETE FROM request_logs
                WHERE id IN (
                    SELECT id
                    FROM request_logs
                    WHERE created_at < ?
                    ORDER BY created_at ASC, id ASC
                    LIMIT ?
                )
                "#,
            )
            .bind(threshold)
            .bind(BATCH_SIZE)
            .execute(&self.pool)
            .await?;
            let deleted = result.rows_affected() as i64;
            total_deleted += deleted;
            if deleted == 0 {
                break;
            }
        }
        sqlx::query(
            r#"
            DELETE FROM request_log_catalog_rollups
            WHERE bucket_start < ?
            "#,
        )
        .bind(threshold)
        .execute(&self.pool)
        .await?;
        self.invalidate_request_logs_catalog_cache().await;
        Ok(total_deleted)
    }

    /// Aggregate per-token usage logs into hourly buckets in token_usage_stats.
    /// Returns (rows_affected, new_last_rollup_ts). When there are no new logs,
    /// rows_affected is 0 and new_last_rollup_ts is None.
    pub(crate) async fn rollup_token_usage_stats(&self) -> Result<(i64, Option<i64>), ProxyError> {
        async fn read_meta_i64(
            tx: &mut Transaction<'_, Sqlite>,
            key: &str,
        ) -> Result<Option<i64>, ProxyError> {
            let value =
                sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
                    .bind(key)
                    .fetch_optional(&mut **tx)
                    .await?;
            Ok(value.and_then(|v| v.parse::<i64>().ok()))
        }

        async fn write_meta_i64(
            tx: &mut Transaction<'_, Sqlite>,
            key: &str,
            value: i64,
        ) -> Result<(), ProxyError> {
            sqlx::query(
                r#"
                INSERT INTO meta (key, value)
                VALUES (?, ?)
                ON CONFLICT(key) DO UPDATE SET value = excluded.value
                "#,
            )
            .bind(key)
            .bind(value.to_string())
            .execute(&mut **tx)
            .await?;
            Ok(())
        }

        let mut tx = self.pool.begin().await?;

        // v2 cursor: strictly monotonic auth_token_logs.id to guarantee idempotent rollup.
        // Backward compatibility: on first v2 run, legacy timestamp is used only to filter
        // the migration batch, then the cursor permanently switches to id-based mode.
        let v2_cursor = read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2).await?;
        let (last_log_id, migration_legacy_ts) = if let Some(id) = v2_cursor {
            (id, None)
        } else {
            (
                0,
                read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?,
            )
        };

        let (max_log_id, max_created_at): (Option<i64>, Option<i64>) =
            if let Some(legacy_ts) = migration_legacy_ts {
                sqlx::query_as(
                    r#"
                    SELECT
                        MAX(id) AS max_log_id,
                        MAX(CASE WHEN created_at >= ? THEN created_at END) AS max_created_at
                    FROM auth_token_logs
                    WHERE counts_business_quota = 1
                    "#,
                )
                .bind(legacy_ts)
                .fetch_one(&mut *tx)
                .await?
            } else {
                sqlx::query_as(
                    r#"
                    SELECT
                        MAX(id) AS max_log_id,
                        MAX(created_at) AS max_created_at
                    FROM auth_token_logs
                    WHERE counts_business_quota = 1
                      AND id > ?
                    "#,
                )
                .bind(last_log_id)
                .fetch_one(&mut *tx)
                .await?
            };

        let Some(max_log_id) = max_log_id else {
            if migration_legacy_ts.is_some() {
                // No billable logs yet: initialize v2 cursor to complete migration.
                write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2, 0).await?;
            }
            tx.commit().await?;
            return Ok((0, None));
        };

        let bucket_secs = TOKEN_USAGE_STATS_BUCKET_SECS;

        let result = if let Some(legacy_ts) = migration_legacy_ts {
            sqlx::query(
                r#"
                INSERT INTO token_usage_stats (
                    token_id,
                    bucket_start,
                    bucket_secs,
                    success_count,
                    system_failure_count,
                    external_failure_count,
                    quota_exhausted_count
                )
                SELECT
                    token_id,
                    (created_at / ?) * ? AS bucket_start,
                    ? AS bucket_secs,
                    SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS system_failure_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND NOT (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS external_failure_count,
                    SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM auth_token_logs
                WHERE counts_business_quota = 1
                  AND created_at >= ? AND id <= ?
                GROUP BY token_id, bucket_start
                ON CONFLICT(token_id, bucket_start, bucket_secs) DO UPDATE SET
                    success_count = token_usage_stats.success_count + excluded.success_count,
                    system_failure_count =
                        token_usage_stats.system_failure_count + excluded.system_failure_count,
                    external_failure_count =
                        token_usage_stats.external_failure_count + excluded.external_failure_count,
                    quota_exhausted_count =
                        token_usage_stats.quota_exhausted_count + excluded.quota_exhausted_count
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(legacy_ts)
            .bind(max_log_id)
            .execute(&mut *tx)
            .await?
        } else {
            sqlx::query(
                r#"
                INSERT INTO token_usage_stats (
                    token_id,
                    bucket_start,
                    bucket_secs,
                    success_count,
                    system_failure_count,
                    external_failure_count,
                    quota_exhausted_count
                )
                SELECT
                    token_id,
                    (created_at / ?) * ? AS bucket_start,
                    ? AS bucket_secs,
                    SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS system_failure_count,
                    SUM(
                        CASE
                            WHEN result_status != 'success'
                                 AND result_status != 'quota_exhausted'
                                 AND NOT (
                                    (http_status BETWEEN 400 AND 599)
                                    OR (mcp_status BETWEEN 400 AND 599)
                                ) THEN 1
                            ELSE 0
                        END
                    ) AS external_failure_count,
                    SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END) AS quota_exhausted_count
                FROM auth_token_logs
                WHERE counts_business_quota = 1
                  AND id > ? AND id <= ?
                GROUP BY token_id, bucket_start
                ON CONFLICT(token_id, bucket_start, bucket_secs) DO UPDATE SET
                    success_count = token_usage_stats.success_count + excluded.success_count,
                    system_failure_count =
                        token_usage_stats.system_failure_count + excluded.system_failure_count,
                    external_failure_count =
                        token_usage_stats.external_failure_count + excluded.external_failure_count,
                    quota_exhausted_count =
                        token_usage_stats.quota_exhausted_count + excluded.quota_exhausted_count
                "#,
            )
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(bucket_secs)
            .bind(last_log_id)
            .bind(max_log_id)
            .execute(&mut *tx)
            .await?
        };

        let affected = result.rows_affected() as i64;
        let mut new_last_rollup_ts = max_created_at;

        write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_LOG_ID_V2, max_log_id).await?;
        if let Some(ts) = max_created_at {
            // Keep legacy timestamp cursor monotonic for observability and downgrade compatibility.
            // This prevents accidental timestamp regression when newer log ids carry older created_at.
            let legacy_ts = read_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS).await?;
            let clamped_ts = legacy_ts.map_or(ts, |old| old.max(ts));
            write_meta_i64(&mut tx, META_KEY_TOKEN_USAGE_ROLLUP_TS, clamped_ts).await?;
            new_last_rollup_ts = Some(clamped_ts);
        }

        tx.commit().await?;
        Ok((affected, new_last_rollup_ts))
    }

    pub(crate) async fn rebuild_token_usage_stats_for_tokens(
        &self,
        token_ids: &[String],
    ) -> Result<i64, ProxyError> {
        let mut normalized = Vec::new();
        let mut seen = HashSet::new();
        for token_id in token_ids {
            let value = token_id.trim();
            if value.is_empty() || !seen.insert(value.to_string()) {
                continue;
            }
            normalized.push(value.to_string());
        }
        if normalized.is_empty() {
            return Ok(0);
        }

        let bucket_secs = TOKEN_USAGE_STATS_BUCKET_SECS;
        let bucket_start_sql = format!("(created_at / {bucket_secs}) * {bucket_secs}");
        let mut tx = self.pool.begin().await?;

        let mut delete_query =
            QueryBuilder::<Sqlite>::new("DELETE FROM token_usage_stats WHERE token_id IN (");
        {
            let mut separated = delete_query.separated(", ");
            for token_id in &normalized {
                separated.push_bind(token_id);
            }
        }
        delete_query.push(")");
        let deleted = delete_query
            .build()
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64;

        let mut insert_query = QueryBuilder::<Sqlite>::new(format!(
            r#"
            INSERT INTO token_usage_stats (
                token_id,
                bucket_start,
                bucket_secs,
                success_count,
                system_failure_count,
                external_failure_count,
                quota_exhausted_count
            )
            SELECT
                token_id,
                {bucket_start_sql} AS bucket_start,
                {bucket_secs} AS bucket_secs,
                SUM(CASE WHEN result_status = 'success' THEN 1 ELSE 0 END) AS success_count,
                SUM(
                    CASE
                        WHEN result_status != 'success'
                             AND result_status != 'quota_exhausted'
                             AND (
                                (http_status BETWEEN 400 AND 599)
                                OR (mcp_status BETWEEN 400 AND 599)
                            ) THEN 1
                        ELSE 0
                    END
                ) AS system_failure_count,
                SUM(
                    CASE
                        WHEN result_status != 'success'
                             AND result_status != 'quota_exhausted'
                             AND NOT (
                                (http_status BETWEEN 400 AND 599)
                                OR (mcp_status BETWEEN 400 AND 599)
                            ) THEN 1
                        ELSE 0
                    END
                ) AS external_failure_count,
                SUM(CASE WHEN result_status = 'quota_exhausted' THEN 1 ELSE 0 END)
                    AS quota_exhausted_count
            FROM auth_token_logs
            WHERE counts_business_quota = 1
              AND token_id IN (
            "#,
            bucket_start_sql = bucket_start_sql,
            bucket_secs = bucket_secs,
        ));
        {
            let mut separated = insert_query.separated(", ");
            for token_id in &normalized {
                separated.push_bind(token_id);
            }
        }
        insert_query.push(format!(
            r#"
              )
            GROUP BY token_id, {bucket_start_sql}
            "#,
            bucket_start_sql = bucket_start_sql,
        ));
        let inserted = insert_query
            .build()
            .execute(&mut *tx)
            .await?
            .rows_affected() as i64;

        tx.commit().await?;
        Ok(deleted + inserted)
    }

    pub(crate) async fn increment_monthly_quota_by(
        &self,
        token_id: &str,
        current_month_start: i64,
        delta: i64,
    ) -> Result<i64, ProxyError> {
        if delta <= 0 {
            let month_count = self
                .fetch_monthly_count(token_id, current_month_start)
                .await?;
            return Ok(month_count);
        }
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO auth_token_quota (token_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(token_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_start
                    ELSE auth_token_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > auth_token_quota.month_start THEN excluded.month_count
                    WHEN excluded.month_start < auth_token_quota.month_start THEN auth_token_quota.month_count
                    ELSE auth_token_quota.month_count + excluded.month_count
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(token_id)
        .bind(current_month_start)
        .bind(delta)
        .fetch_one(&self.pool)
        .await?;

        Ok(month_count)
    }

    pub(crate) async fn increment_monthly_quota(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        self.increment_monthly_quota_by(token_id, current_month_start, 1)
            .await
    }

    pub(crate) async fn increment_account_monthly_quota_by(
        &self,
        user_id: &str,
        current_month_start: i64,
        delta: i64,
    ) -> Result<i64, ProxyError> {
        if delta <= 0 {
            let month_count = self
                .fetch_account_monthly_count(user_id, current_month_start)
                .await?;
            return Ok(month_count);
        }
        let (_month_start, month_count): (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO account_monthly_quota (user_id, month_start, month_count)
            VALUES (?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                month_start = CASE
                    WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_start
                    ELSE account_monthly_quota.month_start
                END,
                month_count = CASE
                    WHEN excluded.month_start > account_monthly_quota.month_start THEN excluded.month_count
                    WHEN excluded.month_start < account_monthly_quota.month_start THEN account_monthly_quota.month_count
                    ELSE account_monthly_quota.month_count + excluded.month_count
                END
            RETURNING month_start, month_count
            "#,
        )
        .bind(user_id)
        .bind(current_month_start)
        .bind(delta)
        .fetch_one(&self.pool)
        .await?;
        Ok(month_count)
    }

    pub(crate) async fn increment_account_monthly_quota(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        self.increment_account_monthly_quota_by(user_id, current_month_start, 1)
            .await
    }

    pub(crate) async fn upgrade_auth_tokens_schema(&self) -> Result<(), ProxyError> {
        // Future-proof placeholder for migrations
        // Ensure required columns exist if table is from older version
        // enabled
        if !self.auth_tokens_column_exists("enabled").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1")
                .execute(&self.pool)
                .await?;
        }

        if !self.auth_tokens_column_exists("note").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN note TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("total_requests").await? {
            sqlx::query(
                "ALTER TABLE auth_tokens ADD COLUMN total_requests INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&self.pool)
            .await?;
        }
        if !self.auth_tokens_column_exists("created_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("last_used_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN last_used_at INTEGER")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("group_name").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN group_name TEXT")
                .execute(&self.pool)
                .await?;
        }
        if !self.auth_tokens_column_exists("deleted_at").await? {
            sqlx::query("ALTER TABLE auth_tokens ADD COLUMN deleted_at INTEGER")
                .execute(&self.pool)
                .await?;
        }

        Ok(())
    }

    pub(crate) async fn auth_tokens_column_exists(&self, column: &str) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info('auth_tokens') WHERE name = ? LIMIT 1",
        )
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    pub(crate) async fn table_column_exists(
        &self,
        table: &str,
        column: &str,
    ) -> Result<bool, ProxyError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM pragma_table_info(?) WHERE name = ? LIMIT 1",
        )
        .bind(table)
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    pub(crate) async fn table_column_not_null(
        &self,
        table: &str,
        column: &str,
    ) -> Result<bool, ProxyError> {
        let not_null = sqlx::query_scalar::<_, i64>(
            r#"SELECT "notnull" FROM pragma_table_info(?) WHERE name = ? LIMIT 1"#,
        )
        .bind(table)
        .bind(column)
        .fetch_optional(&self.pool)
        .await?;
        Ok(not_null.unwrap_or_default() != 0)
    }

    async fn ensure_mcp_sessions_schema(&self) -> Result<(), ProxyError> {
        let needs_rebuild = self
            .table_column_not_null("mcp_sessions", "upstream_session_id")
            .await?
            || self
                .table_column_not_null("mcp_sessions", "upstream_key_id")
                .await?;
        if needs_rebuild {
            self.rebuild_mcp_sessions_table().await?;
        }

        for (column, sql) in [
            (
                "gateway_mode",
                "ALTER TABLE mcp_sessions ADD COLUMN gateway_mode TEXT NOT NULL DEFAULT 'upstream_mcp'",
            ),
            (
                "experiment_variant",
                "ALTER TABLE mcp_sessions ADD COLUMN experiment_variant TEXT NOT NULL DEFAULT 'control'",
            ),
            (
                "ab_bucket",
                "ALTER TABLE mcp_sessions ADD COLUMN ab_bucket INTEGER",
            ),
            (
                "routing_subject_hash",
                "ALTER TABLE mcp_sessions ADD COLUMN routing_subject_hash TEXT",
            ),
            (
                "fallback_reason",
                "ALTER TABLE mcp_sessions ADD COLUMN fallback_reason TEXT",
            ),
            (
                "rate_limited_until",
                "ALTER TABLE mcp_sessions ADD COLUMN rate_limited_until INTEGER",
            ),
            (
                "last_rate_limited_at",
                "ALTER TABLE mcp_sessions ADD COLUMN last_rate_limited_at INTEGER",
            ),
            (
                "last_rate_limit_reason",
                "ALTER TABLE mcp_sessions ADD COLUMN last_rate_limit_reason TEXT",
            ),
        ] {
            if !self.table_column_exists("mcp_sessions", column).await? {
                sqlx::query(sql).execute(&self.pool).await?;
            }
        }

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_mcp_sessions_user_active
               ON mcp_sessions(user_id, revoked_at, expires_at DESC, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_mcp_sessions_token_active
               ON mcp_sessions(auth_token_id, revoked_at, expires_at DESC, updated_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_mcp_sessions_expires_at
               ON mcp_sessions(expires_at, revoked_at)"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn rebuild_mcp_sessions_table(&self) -> Result<(), ProxyError> {
        let mut conn = self.pool.acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;

        let rebuild_result = async {
            sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
            sqlx::query("DROP TABLE IF EXISTS mcp_sessions_new")
                .execute(&mut *conn)
                .await?;
            sqlx::query(
                r#"
                CREATE TABLE mcp_sessions_new (
                    proxy_session_id TEXT PRIMARY KEY,
                    upstream_session_id TEXT,
                    upstream_key_id TEXT,
                    auth_token_id TEXT,
                    user_id TEXT,
                    protocol_version TEXT,
                    last_event_id TEXT,
                    gateway_mode TEXT NOT NULL DEFAULT 'upstream_mcp',
                    experiment_variant TEXT NOT NULL DEFAULT 'control',
                    ab_bucket INTEGER,
                    routing_subject_hash TEXT,
                    fallback_reason TEXT,
                    rate_limited_until INTEGER,
                    last_rate_limited_at INTEGER,
                    last_rate_limit_reason TEXT,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    expires_at INTEGER NOT NULL,
                    revoked_at INTEGER,
                    revoke_reason TEXT,
                    FOREIGN KEY (upstream_key_id) REFERENCES api_keys(id)
                )
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query(
                r#"
                INSERT INTO mcp_sessions_new (
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
                SELECT
                    proxy_session_id,
                    NULLIF(upstream_session_id, ''),
                    NULLIF(upstream_key_id, ''),
                    auth_token_id,
                    user_id,
                    protocol_version,
                    last_event_id,
                    'upstream_mcp',
                    'control',
                    NULL,
                    NULL,
                    NULL,
                    rate_limited_until,
                    last_rate_limited_at,
                    last_rate_limit_reason,
                    created_at,
                    updated_at,
                    expires_at,
                    revoked_at,
                    revoke_reason
                FROM mcp_sessions
                "#,
            )
            .execute(&mut *conn)
            .await?;
            sqlx::query("DROP TABLE mcp_sessions")
                .execute(&mut *conn)
                .await?;
            sqlx::query("ALTER TABLE mcp_sessions_new RENAME TO mcp_sessions")
                .execute(&mut *conn)
                .await?;
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            Ok::<(), ProxyError>(())
        }
        .await;

        if rebuild_result.is_err() {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
        }

        let reenable_result = sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await;

        match (rebuild_result, reenable_result) {
            (Err(err), _) => Err(err),
            (Ok(_), Err(err)) => Err(err.into()),
            (Ok(_), Ok(_)) => Ok(()),
        }
    }

    async fn ensure_api_key_usage_bucket_request_value_columns(&self) -> Result<bool, ProxyError> {
        let mut schema_changed = false;

        for column in [
            "valuable_success_count",
            "valuable_failure_count",
            "other_success_count",
            "other_failure_count",
            "unknown_count",
        ] {
            if !self
                .table_column_exists("api_key_usage_buckets", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE api_key_usage_buckets ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"
                ))
                .execute(&self.pool)
                .await?;
                schema_changed = true;
            }
        }

        Ok(schema_changed)
    }

    async fn ensure_dashboard_request_rollup_bucket_columns(&self) -> Result<bool, ProxyError> {
        let mut schema_changed = false;

        for column in ["valuable_failure_429_count"] {
            if !self
                .table_column_exists("dashboard_request_rollup_buckets", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE dashboard_request_rollup_buckets ADD COLUMN {column} INTEGER NOT NULL DEFAULT 0"
                ))
                .execute(&self.pool)
                .await?;
                schema_changed = true;
            }
        }

        Ok(schema_changed)
    }

}

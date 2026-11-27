// Session accounting: monthly breakage fan-out, per-subject broken-key counts and the
// user/token log metric aggregations. Split out of `key_store_sessions.rs` to keep the
// session lifecycle module inside its line budget.

impl KeyStore {
    pub(crate) async fn record_manual_key_breakage_fanout(
        &self,
        key_id: &str,
        key_status: &str,
        reason_code: Option<&str>,
        reason_summary: Option<&str>,
        _actor: &MaintenanceActor,
        break_at: i64,
    ) -> Result<(), ProxyError> {
        if self.pg_pool.is_some() {
            let user_rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
                r#"
                SELECT u.id, u.display_name, u.username
                FROM core.user_api_key_bindings b
                JOIN core.users u ON u.id = b.user_id
                WHERE b.api_key_id = $1
                ORDER BY u.username ASC, u.id ASC
                "#,
            )
            .bind(key_id)
            .fetch_all(self.pg()?)
            .await?;
            let token_ids = sqlx::query_scalar::<_, String>(
                "SELECT token_id FROM core.token_api_key_bindings WHERE api_key_id = $1 ORDER BY token_id ASC",
            )
            .bind(key_id)
            .fetch_all(self.pg()?)
            .await?;
            if user_rows.is_empty() && token_ids.is_empty() {
                return Ok(());
            }
            let mut tx = self.pg()?.begin().await?;
            for (user_id, display_name, username) in &user_rows {
                self.upsert_subject_key_breakage_tx_pg(
                    &mut tx,
                    BROKEN_KEY_SUBJECT_USER,
                    user_id,
                    key_id,
                    break_at,
                    key_status,
                    reason_code,
                    reason_summary,
                    BROKEN_KEY_SOURCE_MANUAL,
                    None,
                    Some(user_id.as_str()),
                    display_name.as_deref().or(username.as_deref()),
                    None,
                )
                .await?;
            }
            for token_id in &token_ids {
                self.upsert_subject_key_breakage_tx_pg(
                    &mut tx,
                    BROKEN_KEY_SUBJECT_TOKEN,
                    token_id,
                    key_id,
                    break_at,
                    key_status,
                    reason_code,
                    reason_summary,
                    BROKEN_KEY_SOURCE_MANUAL,
                    Some(token_id.as_str()),
                    None,
                    None,
                    None,
                )
                .await?;
            }
            tx.commit().await?;
            return Ok(());
        }
        let user_rows = sqlx::query_as::<_, (String, Option<String>, Option<String>)>(
            r#"
            SELECT u.id, u.display_name, u.username
            FROM user_api_key_bindings b
            JOIN users u ON u.id = b.user_id
            WHERE b.api_key_id = ?
            ORDER BY u.username ASC, u.id ASC
            "#,
        )
        .bind(key_id)
        .fetch_all(&self.pool)
        .await?;
        let token_ids = sqlx::query_scalar::<_, String>(
            "SELECT token_id FROM token_api_key_bindings WHERE api_key_id = ? ORDER BY token_id ASC",
        )
        .bind(key_id)
        .fetch_all(&self.pool)
        .await?;
        if user_rows.is_empty() && token_ids.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        // Manual maintenance is billed to whichever subjects were still bound to the key.
        for (user_id, display_name, username) in &user_rows {
            self.upsert_subject_key_breakage_tx(
                &mut tx,
                BROKEN_KEY_SUBJECT_USER,
                user_id,
                key_id,
                break_at,
                key_status,
                reason_code,
                reason_summary,
                BROKEN_KEY_SOURCE_MANUAL,
                None,
                Some(user_id.as_str()),
                display_name.as_deref().or(username.as_deref()),
                None,
            )
            .await?;
        }
        for token_id in &token_ids {
            self.upsert_subject_key_breakage_tx(
                &mut tx,
                BROKEN_KEY_SUBJECT_TOKEN,
                token_id,
                key_id,
                break_at,
                key_status,
                reason_code,
                reason_summary,
                BROKEN_KEY_SOURCE_MANUAL,
                Some(token_id.as_str()),
                None,
                None,
                None,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn backfill_current_month_auto_subject_breakages(
        &self,
    ) -> Result<(), ProxyError> {
        let month_start = start_of_month(self.backend_time.now_utc()).timestamp();
        let rows =
            sqlx::query_as::<_, (String, String, String, i64, Option<String>, Option<String>)>(
                r#"
            SELECT
                key_id,
                auth_token_id,
                operation_code,
                created_at,
                reason_code,
                reason_summary
            FROM api_key_maintenance_records
            WHERE source = ?
              AND created_at >= ?
              AND auth_token_id IS NOT NULL
              AND operation_code = ?
              AND COALESCE(reason_code, '') IN (?, ?, ?)
            ORDER BY created_at ASC, key_id ASC
            "#,
            )
            .bind(MAINTENANCE_SOURCE_SYSTEM)
            .bind(month_start)
            .bind(MAINTENANCE_OP_AUTO_QUARANTINE)
            .bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED)
            .bind(BLOCKED_KEY_REASON_KEY_REVOKED)
            .bind(BLOCKED_KEY_REASON_INVALID_API_KEY)
            .fetch_all(&self.pool)
            .await?;
        if rows.is_empty() {
            return Ok(());
        }

        let mut token_ids: Vec<String> = rows
            .iter()
            .map(|(_, token_id, _, _, _, _)| token_id.clone())
            .collect();
        token_ids.sort_unstable();
        token_ids.dedup();
        let token_bindings = self.list_user_bindings_for_tokens(&token_ids).await?;

        let mut user_ids: Vec<String> = token_bindings.values().cloned().collect();
        user_ids.sort_unstable();
        user_ids.dedup();
        let user_map = self.get_admin_user_identities(&user_ids).await?;

        let mut tx = self.pool.begin().await?;
        for (key_id, token_id, operation_code, created_at, reason_code, reason_summary) in rows {
            let _operation_code = operation_code;
            let key_status = KEY_EFFECT_QUARANTINED;
            let breaker_user_id = token_bindings.get(&token_id).cloned();
            let breaker_identity = breaker_user_id
                .as_ref()
                .and_then(|user_id| user_map.get(user_id));
            let breaker_display = breaker_identity.and_then(|identity| {
                identity
                    .display_name
                    .clone()
                    .or(identity.username.clone())
                    .or(Some(identity.user_id.clone()))
            });

            self.upsert_subject_key_breakage_tx(
                &mut tx,
                BROKEN_KEY_SUBJECT_TOKEN,
                &token_id,
                &key_id,
                created_at,
                key_status,
                reason_code.as_deref(),
                reason_summary.as_deref(),
                BROKEN_KEY_SOURCE_AUTO,
                Some(&token_id),
                breaker_user_id.as_deref(),
                breaker_display.as_deref(),
                None,
            )
            .await?;

            if let Some(user_id) = breaker_user_id.as_deref() {
                self.upsert_subject_key_breakage_tx(
                    &mut tx,
                    BROKEN_KEY_SUBJECT_USER,
                    user_id,
                    &key_id,
                    created_at,
                    key_status,
                    reason_code.as_deref(),
                    reason_summary.as_deref(),
                    BROKEN_KEY_SOURCE_AUTO,
                    Some(&token_id),
                    Some(user_id),
                    breaker_display.as_deref(),
                    None,
                )
                .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding_in_tx_pg(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        trust_level: Option<i64>,
        force_refresh: bool,
    ) -> Result<bool, ProxyError> {
        let Some(level) = normalize_linuxdo_trust_level(trust_level) else {
            return Ok(false);
        };
        let desired_key = linuxdo_system_key_for_level(level);
        let Some((tag_id,)) = sqlx::query_as::<_, (String,)>(
            "SELECT id FROM core.user_tags WHERE system_key = $1 LIMIT 1",
        )
        .bind(&desired_key)
        .fetch_optional(&mut **tx)
        .await?
        else {
            eprintln!(
                "linuxdo system tag sync skipped for user {} trust_level {:?}: missing system tag for LinuxDo trust level {}",
                user_id, trust_level, level
            );
            return Ok(false);
        };
        let existing_bindings = sqlx::query_as::<_, (String, String)>(
            r#"SELECT b.tag_id, b.source
               FROM core.user_tag_bindings b
               JOIN core.user_tags t ON t.id = b.tag_id
               WHERE b.user_id = $1
                 AND t.system_key LIKE 'linuxdo_l%'
               ORDER BY b.tag_id"#,
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?;
        if existing_bindings.len() == 1
            && existing_bindings[0].0 == tag_id
            && existing_bindings[0].1 == USER_TAG_SOURCE_SYSTEM_LINUXDO
            && !force_refresh
        {
            return Ok(false);
        }
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"DELETE FROM core.user_tag_bindings
               WHERE user_id = $1
                 AND tag_id IN (
                     SELECT id FROM core.user_tags WHERE system_key LIKE 'linuxdo_l%'
                 )
                 AND tag_id <> $2"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO core.user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .bind(USER_TAG_SOURCE_SYSTEM_LINUXDO)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(true)
    }

    pub(crate) async fn sync_linuxdo_system_tag_binding_in_tx(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        user_id: &str,
        trust_level: Option<i64>,
        force_refresh: bool,
    ) -> Result<bool, ProxyError> {
        let Some(level) = normalize_linuxdo_trust_level(trust_level) else {
            return Ok(false);
        };
        let desired_key = linuxdo_system_key_for_level(level);
        let Some((tag_id,)) =
            sqlx::query_as::<_, (String,)>("SELECT id FROM user_tags WHERE system_key = ? LIMIT 1")
                .bind(&desired_key)
                .fetch_optional(&mut **tx)
                .await?
        else {
            eprintln!(
                "linuxdo system tag sync skipped for user {} trust_level {:?}: missing system tag for LinuxDo trust level {}",
                user_id, trust_level, level
            );
            return Ok(false);
        };

        let existing_bindings = sqlx::query_as::<_, (String, String)>(
            r#"SELECT b.tag_id, b.source
               FROM user_tag_bindings b
               JOIN user_tags t ON t.id = b.tag_id
               WHERE b.user_id = ?
                 AND t.system_key LIKE 'linuxdo_l%'
               ORDER BY b.tag_id"#,
        )
        .bind(user_id)
        .fetch_all(&mut **tx)
        .await?;
        if existing_bindings.len() == 1
            && existing_bindings[0].0 == tag_id
            && existing_bindings[0].1 == USER_TAG_SOURCE_SYSTEM_LINUXDO
            && !force_refresh
        {
            return Ok(false);
        }

        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"DELETE FROM user_tag_bindings
               WHERE user_id = ?
                 AND tag_id IN (
                     SELECT id FROM user_tags WHERE system_key LIKE 'linuxdo_l%'
                 )
                 AND tag_id <> ?"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            r#"INSERT INTO user_tag_bindings (user_id, tag_id, source, created_at, updated_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(user_id, tag_id) DO UPDATE SET
                   source = excluded.source,
                   updated_at = excluded.updated_at"#,
        )
        .bind(user_id)
        .bind(&tag_id)
        .bind(USER_TAG_SOURCE_SYSTEM_LINUXDO)
        .bind(now)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        Ok(true)
    }

    pub(crate) async fn fetch_user_log_metrics_bulk(
        &self,
        user_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, UserLogMetricsSummary>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let now = self.backend_time.now_utc();
        let month_start = start_of_month(now).timestamp();

        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                r#"
                SELECT
                  b.user_id,
                  COALESCE(SUM(CASE WHEN l.result_status = "#,
            );
            builder.push_bind(OUTCOME_SUCCESS);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(day_start);
            builder.push(" AND l.created_at < ");
            builder.push_bind(day_end);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS daily_success, ");
            builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
            builder.push_bind(OUTCOME_ERROR);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(day_start);
            builder.push(" AND l.created_at < ");
            builder.push_bind(day_end);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS daily_failure, ");
            builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
            builder.push_bind(OUTCOME_SUCCESS);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(month_start);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS monthly_success, ");
            builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
            builder.push_bind(OUTCOME_ERROR);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(month_start);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS monthly_failure, ");
            builder.push(
                r#"MAX(l.created_at) AS last_activity
                FROM core.user_token_bindings b
                LEFT JOIN core.auth_token_logs l ON l.token_id = b.token_id
                WHERE b.user_id IN ("#,
            );
            {
                let mut separated = builder.separated(", ");
                for user_id in user_ids {
                    separated.push_bind(user_id);
                }
            }
            builder.push(") GROUP BY b.user_id");
            let rows = builder
                .build_query_as::<(String, i64, i64, i64, i64, Option<i64>)>()
                .fetch_all(self.pg()?)
                .await?;
            return Ok(rows
                .into_iter()
                .map(|(user_id, daily_success, daily_failure, monthly_success, monthly_failure, last_activity)| {
                    (
                        user_id,
                        UserLogMetricsSummary {
                            daily_success,
                            daily_failure,
                            monthly_success,
                            monthly_failure,
                            last_activity,
                        },
                    )
                })
                .collect());
        }
        let mut builder = QueryBuilder::new(
            r#"
            SELECT
              b.user_id,
              COALESCE(SUM(CASE WHEN l.result_status = "#,
        );
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_failure, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_failure, ");
        builder.push(
            r#"MAX(l.created_at) AS last_activity
            FROM user_token_bindings b
            LEFT JOIN auth_token_logs l ON l.token_id = b.token_id
            WHERE b.user_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY b.user_id");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, Option<i64>)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    user_id,
                    daily_success,
                    daily_failure,
                    monthly_success,
                    monthly_failure,
                    last_activity,
                )| {
                    (
                        user_id,
                        UserLogMetricsSummary {
                            daily_success,
                            daily_failure,
                            monthly_success,
                            monthly_failure,
                            last_activity,
                        },
                    )
                },
            )
            .collect())
    }

    pub(crate) async fn fetch_token_log_metrics_bulk(
        &self,
        token_ids: &[String],
        day_start: i64,
        day_end: i64,
    ) -> Result<HashMap<String, TokenLogMetricsSummary>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let now = self.backend_time.now_utc();
        let month_start = start_of_month(now).timestamp();

        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                r#"
                SELECT
                  l.token_id,
                  COALESCE(SUM(CASE WHEN l.result_status = "#,
            );
            builder.push_bind(OUTCOME_SUCCESS);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(day_start);
            builder.push(" AND l.created_at < ");
            builder.push_bind(day_end);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS daily_success, ");
            builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
            builder.push_bind(OUTCOME_ERROR);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(day_start);
            builder.push(" AND l.created_at < ");
            builder.push_bind(day_end);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS daily_failure, ");
            builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
            builder.push_bind(OUTCOME_SUCCESS);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(month_start);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS monthly_success, ");
            builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
            builder.push_bind(OUTCOME_ERROR);
            builder.push(" AND l.created_at >= ");
            builder.push_bind(month_start);
            builder.push(" THEN 1 ELSE 0 END), 0)::bigint AS monthly_failure, ");
            builder.push(
                r#"MAX(l.created_at) AS last_activity
                FROM core.auth_token_logs l
                WHERE l.token_id IN ("#,
            );
            {
                let mut separated = builder.separated(", ");
                for token_id in token_ids {
                    separated.push_bind(token_id);
                }
            }
            builder.push(") GROUP BY l.token_id");
            let rows = builder
                .build_query_as::<(String, i64, i64, i64, i64, Option<i64>)>()
                .fetch_all(self.pg()?)
                .await?;
            return Ok(rows
                .into_iter()
                .map(|(token_id, daily_success, daily_failure, monthly_success, monthly_failure, last_activity)| {
                    (
                        token_id,
                        TokenLogMetricsSummary {
                            daily_success,
                            daily_failure,
                            monthly_success,
                            monthly_failure,
                            last_activity,
                        },
                    )
                })
                .collect());
        }
        let mut builder = QueryBuilder::new(
            r#"
            SELECT
              l.token_id,
              COALESCE(SUM(CASE WHEN l.result_status = "#,
        );
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(day_start);
        builder.push(" AND l.created_at < ");
        builder.push_bind(day_end);
        builder.push(" THEN 1 ELSE 0 END), 0) AS daily_failure, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_SUCCESS);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_success, ");
        builder.push("COALESCE(SUM(CASE WHEN l.result_status = ");
        builder.push_bind(OUTCOME_ERROR);
        builder.push(" AND l.created_at >= ");
        builder.push_bind(month_start);
        builder.push(" THEN 1 ELSE 0 END), 0) AS monthly_failure, ");
        builder.push(
            r#"MAX(l.created_at) AS last_activity
            FROM auth_token_logs l
            WHERE l.token_id IN ("#,
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY l.token_id");

        let rows = builder
            .build_query_as::<(String, i64, i64, i64, i64, Option<i64>)>()
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    token_id,
                    daily_success,
                    daily_failure,
                    monthly_success,
                    monthly_failure,
                    last_activity,
                )| {
                    (
                        token_id,
                        TokenLogMetricsSummary {
                            daily_success,
                            daily_failure,
                            monthly_success,
                            monthly_failure,
                            last_activity,
                        },
                    )
                },
            )
            .collect())
    }

    pub(crate) async fn fetch_monthly_broken_counts_for_users(
        &self,
        user_ids: &[String],
        month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                r#"SELECT skb.subject_id, COUNT(*) AS broken_count
                   FROM core.subject_key_breakages skb
                   JOIN core.api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
                   LEFT JOIN core.api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
                   WHERE skb.subject_kind = "#,
            );
            builder.push_bind(BROKEN_KEY_SUBJECT_USER);
            builder.push(" AND skb.month_start = ");
            builder.push_bind(month_start);
            builder.push(" AND aq.reason_code IN (");
            builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
            builder.push(") AND skb.reason_code IN (");
            builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
            builder.push(") AND skb.subject_id IN (");
            {
                let mut separated = builder.separated(", ");
                for id in user_ids {
                    separated.push_bind(id);
                }
            }
            builder.push(") GROUP BY skb.subject_id");
            let rows = builder
                .build_query_as::<(String, i64)>()
                .fetch_all(self.pg()?)
                .await?;
            return Ok(rows.into_iter().collect());
        }
        let mut builder = QueryBuilder::new(
            r#"SELECT skb.subject_id, COUNT(*) AS broken_count
               FROM subject_key_breakages skb
               JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
               LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
               WHERE skb.subject_kind = "#,
        );
        builder.push_bind(BROKEN_KEY_SUBJECT_USER);
        builder.push(" AND skb.month_start = ");
        builder.push_bind(month_start);
        builder.push(" AND aq.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.subject_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY skb.subject_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn fetch_monthly_broken_counts_for_tokens(
        &self,
        token_ids: &[String],
        month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                r#"SELECT skb.subject_id, COUNT(*) AS broken_count
                   FROM core.subject_key_breakages skb
                   JOIN core.api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
                   LEFT JOIN core.api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
                   WHERE skb.subject_kind = "#,
            );
            builder.push_bind(BROKEN_KEY_SUBJECT_TOKEN);
            builder.push(" AND skb.month_start = ");
            builder.push_bind(month_start);
            builder.push(" AND aq.reason_code IN (");
            builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
            builder.push(") AND skb.reason_code IN (");
            builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
            builder.push(", ");
            builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
            builder.push(") AND skb.subject_id IN (");
            {
                let mut separated = builder.separated(", ");
                for id in token_ids {
                    separated.push_bind(id);
                }
            }
            builder.push(") GROUP BY skb.subject_id");
            let rows = builder
                .build_query_as::<(String, i64)>()
                .fetch_all(self.pg()?)
                .await?;
            return Ok(rows.into_iter().collect());
        }
        let mut builder = QueryBuilder::new(
            r#"SELECT skb.subject_id, COUNT(*) AS broken_count
               FROM subject_key_breakages skb
               JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL
               LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL
               WHERE skb.subject_kind = "#,
        );
        builder.push_bind(BROKEN_KEY_SUBJECT_TOKEN);
        builder.push(" AND skb.month_start = ");
        builder.push_bind(month_start);
        builder.push(" AND aq.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.reason_code IN (");
        builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
        builder.push(", ");
        builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
        builder.push(") AND skb.subject_id IN (");
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(") GROUP BY skb.subject_id");

        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }
}

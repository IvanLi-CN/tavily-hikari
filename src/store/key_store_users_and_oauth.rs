#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct QuotaSubjectDbLease {
    pub(crate) subject: String,
    pub(crate) owner: String,
    pub(crate) ttl: Duration,
}

impl KeyStore {
    /// PostgreSQL twin of `increment_api_key_user_usage_bucket`; the SQLite original stays in
    /// `key_store_migrations_a.rs` for the serving path.
    async fn increment_api_key_user_usage_bucket_pg(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"
            INSERT INTO core.api_key_user_usage_buckets (
                api_key_id,
                user_id,
                bucket_start,
                bucket_secs,
                success_credits,
                failure_credits,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (api_key_id, user_id, bucket_start, bucket_secs)
            DO UPDATE SET
                success_credits = core.api_key_user_usage_buckets.success_credits + excluded.success_credits,
                failure_credits = core.api_key_user_usage_buckets.failure_credits + excluded.failure_credits,
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

    /// PostgreSQL twin of `refresh_user_api_key_binding`.
    async fn refresh_user_api_key_binding_pg(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        api_key_id: &str,
        success_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO core.user_api_key_bindings (
                user_id,
                api_key_id,
                created_at,
                updated_at,
                last_success_at
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (user_id, api_key_id)
            DO UPDATE SET
                updated_at = CASE
                    WHEN excluded.last_success_at >= core.user_api_key_bindings.last_success_at THEN excluded.updated_at
                    ELSE core.user_api_key_bindings.updated_at
                END,
                last_success_at = GREATEST(core.user_api_key_bindings.last_success_at, excluded.last_success_at)
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
            DELETE FROM core.user_api_key_bindings
            WHERE user_id = $1
              AND api_key_id IN (
                  SELECT api_key_id
                  FROM core.user_api_key_bindings
                  WHERE user_id = $2
                  ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
                  OFFSET $3
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

    /// PostgreSQL twin of `refresh_token_api_key_binding`.
    async fn refresh_token_api_key_binding_pg(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        token_id: &str,
        api_key_id: &str,
        success_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO core.token_api_key_bindings (
                token_id,
                api_key_id,
                created_at,
                updated_at,
                last_success_at
            )
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (token_id, api_key_id)
            DO UPDATE SET
                updated_at = CASE
                    WHEN excluded.last_success_at >= core.token_api_key_bindings.last_success_at THEN excluded.updated_at
                    ELSE core.token_api_key_bindings.updated_at
                END,
                last_success_at = GREATEST(core.token_api_key_bindings.last_success_at, excluded.last_success_at)
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
            DELETE FROM core.token_api_key_bindings
            WHERE token_id = $1
              AND api_key_id IN (
                  SELECT api_key_id
                  FROM core.token_api_key_bindings
                  WHERE token_id = $2
                  ORDER BY last_success_at DESC, updated_at DESC, api_key_id DESC
                  OFFSET $3
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

    const USER_DEBUG_INFO_SHARED_CACHE_TTL: Duration = Duration::from_secs(5);

    pub(crate) async fn user_debug_info_shared(&self, user_id: &str) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        if let Some(cached) = self
            .user_debug_info_shared_cache
            .read()
            .await
            .get(user_id)
            .filter(|entry| entry.expires_at > now)
        {
            return Ok(cached.shared);
        }

        let value = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT debug_info_shared FROM core.users WHERE id = $1 LIMIT 1",
        )
        .bind(user_id)
        .fetch_optional(self.pg()?)
        .await?;
        let shared = value.flatten().unwrap_or(0) != 0;
        self.user_debug_info_shared_cache.write().await.insert(
            user_id.to_string(),
            UserDebugInfoSharedCacheEntry {
                shared,
                expires_at: now + Self::USER_DEBUG_INFO_SHARED_CACHE_TTL.as_secs() as i64,
            },
        );
        Ok(shared)
    }

    pub(crate) async fn set_user_debug_info_shared(
        &self,
        user_id: &str,
        shared: bool,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let result = sqlx::query(
            r#"
            UPDATE core.users
            SET debug_info_shared = $1, updated_at = $2
            WHERE id = $3
            "#,
        )
        .bind(i64::from(shared))
        .bind(now)
        .bind(user_id)
        .execute(self.pg()?)
        .await?;
        if result.rows_affected() > 0 {
            self.user_debug_info_shared_cache.write().await.insert(
                user_id.to_string(),
                UserDebugInfoSharedCacheEntry {
                    shared,
                    expires_at: now + Self::USER_DEBUG_INFO_SHARED_CACHE_TTL.as_secs() as i64,
                },
            );
            self.clear_request_log_body_gc_cursor().await?;
        }
        Ok(shared)
    }

    pub(crate) async fn user_debug_info_shared_bulk(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, bool>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder: QueryBuilder<sqlx::Postgres> =
            QueryBuilder::new("SELECT id, debug_info_shared FROM core.users WHERE id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder.build().fetch_all(self.pg()?).await?;
        let mut map = HashMap::with_capacity(rows.len());
        for row in rows {
            let id: String = row.try_get("id")?;
            let shared: i64 = row.try_get("debug_info_shared")?;
            map.insert(id, shared != 0);
        }
        let now = self.backend_time.now_ts();
        let mut cache = self.user_debug_info_shared_cache.write().await;
        cache.retain(|_, entry| entry.expires_at > now);
        for (id, shared) in &map {
            cache.insert(
                id.clone(),
                UserDebugInfoSharedCacheEntry {
                    shared: *shared,
                    expires_at: now + Self::USER_DEBUG_INFO_SHARED_CACHE_TTL.as_secs() as i64,
                },
            );
        }
        Ok(map)
    }

    async fn resolve_request_rollup_user_id(
        &self,
        token_id: &str,
        billing_subject: Option<&str>,
    ) -> Result<Option<String>, ProxyError> {
        if let Some(user_id) = billing_subject.and_then(|subject| subject.strip_prefix("account:"))
        {
            return Ok(Some(user_id.to_string()));
        }

        self.find_user_id_by_token_fresh(token_id).await
    }

    async fn upsert_oauth_account_with_options(
        &self,
        profile: &OAuthAccountProfile,
        touch_last_login_at: bool,
        refresh_token_update: Option<(&str, &str)>,
    ) -> Result<UserIdentity, ProxyError> {
        let display_name = profile
            .name
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .or_else(|| {
                profile
                    .username
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
            });
        let username = profile
            .username
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let avatar = profile
            .avatar_template
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let active = if profile.active { 1 } else { 0 };
        let now = self.backend_time.now_ts();

        for _ in 0..4 {
            let mut tx = self.pg()?.begin().await?;

            let existing = sqlx::query_as::<_, (String,)>(
                r#"SELECT user_id
                   FROM core.oauth_accounts
                   WHERE provider = $1 AND provider_user_id = $2
                   LIMIT 1"#,
            )
            .bind(&profile.provider)
            .bind(&profile.provider_user_id)
            .fetch_optional(&mut *tx)
            .await?;

            let user_id = if let Some((user_id,)) = existing {
                user_id
            } else {
                const ALPHABET: &[u8] =
                    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
                let mut created_user_id = None;
                for _ in 0..8 {
                    let candidate = random_string(ALPHABET, 12);
                    let inserted = sqlx::query(
                        r#"INSERT INTO core.users
                           (id, display_name, username, avatar_template, active, created_at, updated_at, last_login_at)
                           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
                    )
                    .bind(&candidate)
                    .bind(display_name.clone())
                    .bind(username.clone())
                    .bind(avatar.clone())
                    .bind(active)
                    .bind(now)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *tx)
                    .await;

                    match inserted {
                        Ok(_) => {
                            created_user_id = Some(candidate);
                            break;
                        }
                        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                            continue;
                        }
                        Err(err) => {
                            tx.rollback().await.ok();
                            return Err(ProxyError::Database(err));
                        }
                    }
                }

                let Some(user_id) = created_user_id else {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Other(
                        "failed to allocate unique local user id".to_string(),
                    ));
                };

                let zero_base = AccountQuotaLimits::zero_base();
                sqlx::query(
                    r#"INSERT INTO core.account_quota_limits (
                           user_id,
                           business_calls_1h_limit,
                           daily_credits_limit,
                           monthly_credits_limit,
                           inherits_defaults,
                           created_at,
                           updated_at
                       )
                       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                )
                .bind(&user_id)
                .bind(zero_base.business_calls_1h_limit)
                .bind(zero_base.daily_credits_limit)
                .bind(zero_base.monthly_credits_limit)
                .bind(0)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;

                let inserted_account = sqlx::query(
                    r#"INSERT INTO core.oauth_accounts
                       (provider, provider_user_id, user_id, username, name, avatar_template, active, trust_level, raw_payload, created_at, updated_at)
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
                )
                .bind(&profile.provider)
                .bind(&profile.provider_user_id)
                .bind(&user_id)
                .bind(username.clone())
                .bind(display_name.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(profile.trust_level)
                .bind(profile.raw_payload_json.clone())
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_account {
                    Ok(_) => user_id,
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                        tx.rollback().await.ok();
                        continue;
                    }
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            };

            if touch_last_login_at {
                sqlx::query(
                    r#"UPDATE core.users
                       SET display_name = $1, username = $2, avatar_template = $3, active = $4, updated_at = $5, last_login_at = $6
                       WHERE id = $7"#,
                )
                .bind(display_name.clone())
                .bind(username.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(now)
                .bind(now)
                .bind(&user_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    r#"UPDATE core.users
                       SET display_name = $1, username = $2, avatar_template = $3, active = $4, updated_at = $5
                       WHERE id = $6"#,
                )
                .bind(display_name.clone())
                .bind(username.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(now)
                .bind(&user_id)
                .execute(&mut *tx)
                .await?;
            }

            if let Some((refresh_token_ciphertext, refresh_token_nonce)) = refresh_token_update {
                sqlx::query(
                    r#"UPDATE core.oauth_accounts
                       SET username = $1,
                           name = $2,
                           avatar_template = $3,
                           active = $4,
                           trust_level = $5,
                           raw_payload = $6,
                           refresh_token_ciphertext = $7,
                           refresh_token_nonce = $8,
                           updated_at = $9
                       WHERE provider = $10 AND provider_user_id = $11"#,
                )
                .bind(username.clone())
                .bind(display_name.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(profile.trust_level)
                .bind(profile.raw_payload_json.clone())
                .bind(refresh_token_ciphertext)
                .bind(refresh_token_nonce)
                .bind(now)
                .bind(&profile.provider)
                .bind(&profile.provider_user_id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    r#"UPDATE core.oauth_accounts
                       SET username = $1, name = $2, avatar_template = $3, active = $4, trust_level = $5, raw_payload = $6, updated_at = $7
                       WHERE provider = $8 AND provider_user_id = $9"#,
                )
                .bind(username.clone())
                .bind(display_name.clone())
                .bind(avatar.clone())
                .bind(active)
                .bind(profile.trust_level)
                .bind(profile.raw_payload_json.clone())
                .bind(now)
                .bind(&profile.provider)
                .bind(&profile.provider_user_id)
                .execute(&mut *tx)
                .await?;
            }

            tx.commit().await?;
            if profile.provider == "linuxdo" {
                self.sync_linuxdo_system_tag_binding_best_effort(&user_id, profile.trust_level)
                    .await;
            }
            self.record_effective_account_quota_snapshot_at(&user_id, now)
                .await?;
            return Ok(UserIdentity {
                user_id,
                provider: profile.provider.clone(),
                provider_user_id: profile.provider_user_id.clone(),
                display_name,
                username,
                avatar_template: avatar,
            });
        }

        Err(ProxyError::Other(
            "failed to upsert oauth account after retries".to_string(),
        ))
    }

    pub(crate) async fn upsert_oauth_account(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        self.upsert_oauth_account_with_options(profile, true, None)
            .await
    }

    pub(crate) async fn refresh_oauth_account_profile(
        &self,
        profile: &OAuthAccountProfile,
    ) -> Result<UserIdentity, ProxyError> {
        self.upsert_oauth_account_with_options(profile, false, None)
            .await
    }

    pub(crate) async fn refresh_oauth_account_profile_with_refresh_token(
        &self,
        profile: &OAuthAccountProfile,
        refresh_token_ciphertext: &str,
        refresh_token_nonce: &str,
    ) -> Result<UserIdentity, ProxyError> {
        self.upsert_oauth_account_with_options(
            profile,
            false,
            Some((refresh_token_ciphertext, refresh_token_nonce)),
        )
        .await
    }

    pub(crate) async fn oauth_account_exists(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<bool, ProxyError> {
        let row = sqlx::query_scalar::<_, i64>(
            r#"SELECT 1
               FROM core.oauth_accounts
               WHERE provider = $1 AND provider_user_id = $2
               LIMIT 1"#,
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(self.pg()?)
        .await?;
        Ok(row.is_some())
    }

    pub(crate) async fn set_oauth_account_refresh_token(
        &self,
        provider: &str,
        provider_user_id: &str,
        refresh_token_ciphertext: &str,
        refresh_token_nonce: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"UPDATE core.oauth_accounts
               SET refresh_token_ciphertext = $1, refresh_token_nonce = $2, updated_at = $3
               WHERE provider = $4 AND provider_user_id = $5"#,
        )
        .bind(refresh_token_ciphertext)
        .bind(refresh_token_nonce)
        .bind(now)
        .bind(provider)
        .bind(provider_user_id)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn set_user_active_status(
        &self,
        user_id: &str,
        active: bool,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"UPDATE core.users
               SET active = $1, updated_at = $2
               WHERE id = $3"#,
        )
        .bind(if active { 1 } else { 0 })
        .bind(now)
        .bind(user_id)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn list_oauth_accounts_with_refresh_token(
        &self,
        provider: &str,
    ) -> Result<Vec<OAuthAccountRefreshTokenRecord>, ProxyError> {
        let rows = sqlx::query(
            r#"SELECT
                    provider,
                    provider_user_id,
                    user_id,
                    username,
                    name,
                    refresh_token_ciphertext,
                    refresh_token_nonce
               FROM core.oauth_accounts
               WHERE provider = $1
                 AND COALESCE(refresh_token_ciphertext, '') != ''
                 AND COALESCE(refresh_token_nonce, '') != ''
               ORDER BY id ASC"#,
        )
        .bind(provider)
        .fetch_all(self.pg()?)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(OAuthAccountRefreshTokenRecord {
                    provider: row.try_get("provider")?,
                    provider_user_id: row.try_get("provider_user_id")?,
                    user_id: row.try_get("user_id")?,
                    username: row.try_get("username")?,
                    name: row.try_get("name")?,
                    refresh_token_ciphertext: row.try_get("refresh_token_ciphertext")?,
                    refresh_token_nonce: row.try_get("refresh_token_nonce")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn record_oauth_account_profile_sync_success(
        &self,
        provider: &str,
        provider_user_id: &str,
        attempted_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE core.oauth_accounts
               SET last_profile_sync_attempt_at = $1,
                   last_profile_sync_success_at = $2,
                   last_profile_sync_error = NULL
               WHERE provider = $3 AND provider_user_id = $4"#,
        )
        .bind(attempted_at)
        .bind(attempted_at)
        .bind(provider)
        .bind(provider_user_id)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn record_oauth_account_profile_sync_failure(
        &self,
        provider: &str,
        provider_user_id: &str,
        attempted_at: i64,
        error: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"UPDATE core.oauth_accounts
               SET last_profile_sync_attempt_at = $1,
                   last_profile_sync_error = $2
               WHERE provider = $3 AND provider_user_id = $4"#,
        )
        .bind(attempted_at)
        .bind(error.trim())
        .bind(provider)
        .bind(provider_user_id)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    pub(crate) async fn ensure_user_token_binding(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        self.ensure_user_token_binding_with_preferred(user_id, note, None)
            .await
    }

    pub(crate) async fn create_user_bound_access_token(
        &self,
        user_id: &str,
        note: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let note = note.unwrap_or("").trim().to_string();

        for _ in 0..4 {
            let now = self.backend_time.now_ts();
            let mut tx = self.pg()?.begin().await?;

            let user_exists = sqlx::query_scalar::<_, Option<i64>>(
                r#"SELECT 1 FROM core.users WHERE id = $1 LIMIT 1"#,
            )
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
            if user_exists.is_none() {
                tx.rollback().await.ok();
                return Err(ProxyError::Database(sqlx::Error::RowNotFound));
            }

            let mut created: Option<(String, String)> = None;
            for _ in 0..8 {
                let token_id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);
                let inserted_token = sqlx::query(
                    r#"INSERT INTO core.auth_tokens
                       (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES ($1, $2, 1, $3, NULL, 0, $4, NULL, NULL)"#,
                )
                .bind(&token_id)
                .bind(&secret)
                .bind(&note)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_token {
                    Ok(_) => {
                        created = Some((token_id, secret));
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            }

            let Some((token_id, secret)) = created else {
                tx.rollback().await.ok();
                return Err(ProxyError::Other(
                    "failed to create auth token for user binding".to_string(),
                ));
            };

            let inserted_binding = sqlx::query(
                r#"INSERT INTO core.user_token_bindings (user_id, token_id, created_at, updated_at)
                   VALUES ($1, $2, $3, $4)"#,
            )
            .bind(user_id)
            .bind(&token_id)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await;

            match inserted_binding {
                Ok(_) => {
                    tx.commit().await?;
                    self.cache_token_binding(&token_id, Some(user_id)).await;
                    return Ok(AuthTokenSecret {
                        id: token_id.clone(),
                        token: Self::compose_full_token(&token_id, &secret),
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    tx.rollback().await.ok();
                    continue;
                }
                Err(err) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Database(err));
                }
            }
        }

        Err(ProxyError::Other(
            "failed to create user-bound token after retries".to_string(),
        ))
    }

    pub(crate) async fn delete_user_bound_access_token(
        &self,
        user_id: &str,
        token_id: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let result = sqlx::query(
            r#"UPDATE core.auth_tokens
               SET enabled = 0, deleted_at = $1
               WHERE id = $2
                 AND deleted_at IS NULL
                 AND EXISTS (
                   SELECT 1
                   FROM core.user_token_bindings b
                   WHERE b.user_id = $3 AND b.token_id = auth_tokens.id
                 )
                 AND (
                   SELECT COUNT(*)
                   FROM core.user_token_bindings b_count
                   JOIN core.auth_tokens t_count ON t_count.id = b_count.token_id
                   WHERE b_count.user_id = $4 AND t_count.deleted_at IS NULL
                 ) > 1"#,
        )
        .bind(now)
        .bind(token_id)
        .bind(user_id)
        .bind(user_id)
        .execute(self.pg()?)
        .await?;

        if result.rows_affected() > 0 {
            self.cache_token_binding(token_id, None).await;
            return Ok(());
        }

        let bound_active_count = sqlx::query_scalar::<_, i64>(
            r#"SELECT COUNT(*)
               FROM core.user_token_bindings b
               JOIN core.auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = $1 AND t.deleted_at IS NULL"#,
        )
        .bind(user_id)
        .fetch_one(self.pg()?)
        .await?;
        let token_belongs_to_user = sqlx::query_scalar::<_, Option<i64>>(
            r#"SELECT 1
               FROM core.user_token_bindings b
               JOIN core.auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = $1 AND b.token_id = $2 AND t.deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(user_id)
        .bind(token_id)
        .fetch_optional(self.pg()?)
        .await?
        .is_some();

        if token_belongs_to_user && bound_active_count <= 1 {
            return Err(ProxyError::Other(
                "cannot delete the user's last token".to_string(),
            ));
        }

        Err(ProxyError::Database(sqlx::Error::RowNotFound))
    }

    pub(crate) async fn fetch_active_token_secret_by_id(
        &self,
        token_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String,)>(
            r#"SELECT secret
               FROM core.auth_tokens
               WHERE id = $1 AND enabled = 1 AND deleted_at IS NULL
               LIMIT 1"#,
        )
        .bind(token_id)
        .fetch_optional(self.pg()?)
        .await?;

        Ok(row.map(|(secret,)| AuthTokenSecret {
            id: token_id.to_string(),
            token: Self::compose_full_token(token_id, &secret),
        }))
    }

    pub(crate) async fn ensure_user_token_binding_with_preferred(
        &self,
        user_id: &str,
        note: Option<&str>,
        preferred_token_id: Option<&str>,
    ) -> Result<AuthTokenSecret, ProxyError> {
        let retry_deadline = self.backend_time.deadline_after(Duration::from_secs(5));
        let mut retry_attempt = 0usize;
        let preferred_token_id = preferred_token_id
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let Some(preferred_token_id) = preferred_token_id
            && let Some(preferred_secret) = self
                .fetch_active_token_secret_by_id(preferred_token_id)
                .await?
        {
            for _ in 0..4 {
                let now = self.backend_time.now_ts();
                let mut tx = match self.pg()?.begin().await {
                    Ok(tx) => tx,
                    Err(err) => {
                        let err = ProxyError::Database(err);
                        if sleep_before_sqlite_transient_write_retry(
                            &self.backend_time,
                            "ensure user token binding preferred begin",
                            retry_attempt,
                            retry_deadline,
                            &err,
                        )
                        .await
                        {
                            retry_attempt += 1;
                            continue;
                        }
                        return Err(err);
                    }
                };

                let owner = sqlx::query_as::<_, (String,)>(
                    r#"SELECT user_id
                       FROM core.user_token_bindings
                       WHERE token_id = $1
                       LIMIT 1"#,
                )
                .bind(preferred_token_id)
                .fetch_optional(&mut *tx)
                .await?;

                match owner {
                    Some((owner_user_id,)) if owner_user_id != user_id => {
                        tx.rollback().await.ok();
                        break;
                    }
                    Some(_) => {
                        let touch = sqlx::query(
                            r#"UPDATE core.user_token_bindings
                               SET updated_at = $1
                               WHERE user_id = $2 AND token_id = $3"#,
                        )
                        .bind(now)
                        .bind(user_id)
                        .bind(preferred_token_id)
                        .execute(&mut *tx)
                        .await;
                        match touch {
                            Ok(_) => {
                                if let Err(err) = tx.commit().await {
                                    let err = ProxyError::Database(err);
                                    if sleep_before_sqlite_transient_write_retry(
                                        &self.backend_time,
                                        "ensure user token binding preferred touch commit",
                                        retry_attempt,
                                        retry_deadline,
                                        &err,
                                    )
                                    .await
                                    {
                                        retry_attempt += 1;
                                        continue;
                                    }
                                    return Err(err);
                                }
                                self.cache_token_binding(preferred_token_id, Some(user_id))
                                    .await;
                                return Ok(preferred_secret);
                            }
                            Err(sqlx::Error::Database(db_err))
                                if db_err.message().contains("database is locked") =>
                            {
                                tx.rollback().await.ok();
                                let err = ProxyError::Database(sqlx::Error::Database(db_err));
                                if sleep_before_sqlite_transient_write_retry(
                                    &self.backend_time,
                                    "ensure user token binding preferred touch",
                                    retry_attempt,
                                    retry_deadline,
                                    &err,
                                )
                                .await
                                {
                                    retry_attempt += 1;
                                    continue;
                                }
                                return Err(err);
                            }
                            Err(err) => {
                                tx.rollback().await.ok();
                                return Err(ProxyError::Database(err));
                            }
                        }
                    }
                    None => {
                        let result = sqlx::query(
                            r#"INSERT INTO core.user_token_bindings (user_id, token_id, created_at, updated_at)
                               VALUES ($1, $2, $3, $4)
                               ON CONFLICT (user_id, token_id) DO UPDATE SET
                                   updated_at = excluded.updated_at"#,
                        )
                        .bind(user_id)
                        .bind(preferred_token_id)
                        .bind(now)
                        .bind(now)
                        .execute(&mut *tx)
                        .await;

                        match result {
                            Ok(_) => {
                                if let Err(err) = tx.commit().await {
                                    let err = ProxyError::Database(err);
                                    if sleep_before_sqlite_transient_write_retry(
                                        &self.backend_time,
                                        "ensure user token binding preferred insert commit",
                                        retry_attempt,
                                        retry_deadline,
                                        &err,
                                    )
                                    .await
                                    {
                                        retry_attempt += 1;
                                        continue;
                                    }
                                    return Err(err);
                                }
                                self.cache_token_binding(preferred_token_id, Some(user_id))
                                    .await;
                                return Ok(preferred_secret);
                            }
                            Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                                tx.rollback().await.ok();
                                continue;
                            }
                            Err(sqlx::Error::Database(db_err))
                                if db_err.message().contains("database is locked") =>
                            {
                                tx.rollback().await.ok();
                                let err = ProxyError::Database(sqlx::Error::Database(db_err));
                                if sleep_before_sqlite_transient_write_retry(
                                    &self.backend_time,
                                    "ensure user token binding preferred insert",
                                    retry_attempt,
                                    retry_deadline,
                                    &err,
                                )
                                .await
                                {
                                    retry_attempt += 1;
                                    continue;
                                }
                                return Err(err);
                            }
                            Err(err) => {
                                tx.rollback().await.ok();
                                return Err(ProxyError::Database(err));
                            }
                        }
                    }
                }
            }
        }

        if let Some(existing) = self.fetch_user_token_any_status(user_id).await? {
            self.cache_token_binding(&existing.id, Some(user_id)).await;
            return Ok(existing);
        }

        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let note = note.unwrap_or("").trim().to_string();

        for _ in 0..4 {
            let mut tx = match self.pg()?.begin().await {
                Ok(tx) => tx,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "ensure user token binding begin",
                        retry_attempt,
                        retry_deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            };
            if let Some((token_id, secret)) = sqlx::query_as::<_, (String, String)>(
                r#"SELECT b.token_id, t.secret
                   FROM core.user_token_bindings b
                   JOIN core.auth_tokens t ON t.id = b.token_id
                   WHERE b.user_id = $1
                   ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
                   LIMIT 1"#,
            )
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?
            {
                tx.rollback().await.ok();
                return Ok(AuthTokenSecret {
                    id: token_id.clone(),
                    token: Self::compose_full_token(&token_id, &secret),
                });
            }

            let mut created: Option<(String, String)> = None;
            for _ in 0..8 {
                let token_id = random_string(ALPHABET, 4);
                let secret = random_string(ALPHABET, 24);

                let inserted_token = sqlx::query(
                    r#"INSERT INTO core.auth_tokens
                       (id, secret, enabled, note, group_name, total_requests, created_at, last_used_at, deleted_at)
                       VALUES ($1, $2, 1, $3, NULL, 0, $4, NULL, NULL)"#,
                )
                .bind(&token_id)
                .bind(&secret)
                .bind(&note)
                .bind(now)
                .execute(&mut *tx)
                .await;

                match inserted_token {
                    Ok(_) => {
                        created = Some((token_id, secret));
                        break;
                    }
                    Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                    Err(err) => {
                        tx.rollback().await.ok();
                        return Err(ProxyError::Database(err));
                    }
                }
            }

            let Some((token_id, secret)) = created else {
                tx.rollback().await.ok();
                return Err(ProxyError::Other(
                    "failed to create auth token for user binding".to_string(),
                ));
            };

            let inserted_binding = sqlx::query(
                r#"INSERT INTO core.user_token_bindings (user_id, token_id, created_at, updated_at)
                   VALUES ($1, $2, $3, $4)"#,
            )
            .bind(user_id)
            .bind(&token_id)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await;

            match inserted_binding {
                Ok(_) => {
                    if let Err(err) = tx.commit().await {
                        let err = ProxyError::Database(err);
                        if sleep_before_sqlite_transient_write_retry(
                            &self.backend_time,
                            "ensure user token binding commit",
                            retry_attempt,
                            retry_deadline,
                            &err,
                        )
                        .await
                        {
                            retry_attempt += 1;
                            continue;
                        }
                        return Err(err);
                    }
                    self.cache_token_binding(&token_id, Some(user_id)).await;
                    return Ok(AuthTokenSecret {
                        id: token_id.clone(),
                        token: Self::compose_full_token(&token_id, &secret),
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
                    tx.rollback().await.ok();
                    continue;
                }
                Err(sqlx::Error::Database(db_err))
                    if db_err.message().contains("database is locked") =>
                {
                    tx.rollback().await.ok();
                    let err = ProxyError::Database(sqlx::Error::Database(db_err));
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "ensure user token binding insert",
                        retry_attempt,
                        retry_deadline,
                        &err,
                    )
                    .await
                    {
                        retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
                Err(err) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Database(err));
                }
            }
        }

        Err(ProxyError::Other(
            "failed to ensure user token binding after retries".to_string(),
        ))
    }

    pub(crate) async fn fetch_user_token_any_status(
        &self,
        user_id: &str,
    ) -> Result<Option<AuthTokenSecret>, ProxyError> {
        let row = sqlx::query_as::<_, (String, String)>(
            r#"SELECT b.token_id, t.secret
               FROM core.user_token_bindings b
               JOIN core.auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = $1
               ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(self.pg()?)
        .await?;

        Ok(row.map(|(token_id, secret)| AuthTokenSecret {
            id: token_id.clone(),
            token: Self::compose_full_token(&token_id, &secret),
        }))
    }

    pub(crate) async fn get_user_token(
        &self,
        user_id: &str,
    ) -> Result<UserTokenLookup, ProxyError> {
        let row = sqlx::query_as::<_, (String, Option<String>, Option<i64>, Option<i64>)>(
            r#"SELECT b.token_id, t.secret, t.enabled, t.deleted_at
               FROM core.user_token_bindings b
               LEFT JOIN core.auth_tokens t ON t.id = b.token_id
               WHERE b.user_id = $1
               ORDER BY b.updated_at DESC, b.created_at DESC, b.token_id DESC
               LIMIT 1"#,
        )
        .bind(user_id)
        .fetch_optional(self.pg()?)
        .await?;

        let Some((token_id, maybe_secret, maybe_enabled, maybe_deleted_at)) = row else {
            return Ok(UserTokenLookup::MissingBinding);
        };
        let Some(secret) = maybe_secret else {
            return Ok(UserTokenLookup::Unavailable);
        };
        let enabled = maybe_enabled.unwrap_or(0);
        if enabled != 1 || maybe_deleted_at.is_some() {
            return Ok(UserTokenLookup::Unavailable);
        }

        Ok(UserTokenLookup::Found(AuthTokenSecret {
            id: token_id.clone(),
            token: Self::compose_full_token(&token_id, &secret),
        }))
    }

    pub(crate) async fn create_user_session(
        &self,
        user: &UserIdentity,
        session_max_age_secs: i64,
    ) -> Result<UserSession, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_";
        let now = self.backend_time.now_ts();
        let expires_at = now + session_max_age_secs.max(60);

        sqlx::query(
            "DELETE FROM core.user_sessions WHERE expires_at < $1 OR revoked_at IS NOT NULL",
        )
        .bind(now)
        .execute(self.pg()?)
        .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let inserted = sqlx::query(
                r#"INSERT INTO core.user_sessions (token, user_id, provider, created_at, expires_at, revoked_at)
                   VALUES ($1, $2, $3, $4, $5, NULL)"#,
            )
            .bind(&token)
            .bind(&user.user_id)
            .bind(&user.provider)
            .bind(now)
            .bind(expires_at)
            .execute(self.pg()?)
            .await;

            match inserted {
                Ok(_) => {
                    return Ok(UserSession {
                        token,
                        user: user.clone(),
                        expires_at,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub(crate) async fn get_user_session(
        &self,
        token: &str,
    ) -> Result<Option<UserSession>, ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query("DELETE FROM core.user_sessions WHERE expires_at < $1")
            .bind(now)
            .execute(self.pg()?)
            .await?;

        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                i64,
            ),
        >(
            r#"SELECT
                    s.token,
                    s.user_id,
                    s.provider,
                    oa.provider_user_id,
                    u.display_name,
                    u.username,
                    u.avatar_template,
                    s.expires_at
               FROM core.user_sessions s
               JOIN core.users u ON u.id = s.user_id
               LEFT JOIN core.oauth_accounts oa ON oa.user_id = u.id AND oa.provider = s.provider
               WHERE s.token = $1 AND s.revoked_at IS NULL AND s.expires_at > $2 AND u.active = 1
               LIMIT 1"#,
        )
        .bind(token)
        .bind(now)
        .fetch_optional(self.pg()?)
        .await?;

        Ok(row.map(
            |(
                token,
                user_id,
                provider,
                provider_user_id,
                display_name,
                username,
                avatar_template,
                expires_at,
            )| UserSession {
                token,
                user: UserIdentity {
                    user_id,
                    provider,
                    provider_user_id: provider_user_id.unwrap_or_default(),
                    display_name,
                    username,
                    avatar_template,
                },
                expires_at,
            },
        ))
    }

    pub(crate) async fn revoke_user_session(&self, token: &str) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            "UPDATE core.user_sessions SET revoked_at = $1 WHERE token = $2 AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(token)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn insert_token_log(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        request_kind: &TokenRequestKind,
        failure_kind: Option<&str>,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
        binding_effect_code: &str,
        binding_effect_summary: Option<&str>,
        selection_effect_code: &str,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<UserBusinessCallEventWriteDecision, ProxyError> {
        let created_at = self.backend_time.now_ts();
        let request_kind = self
            .resolve_token_log_request_kind(request_log_id, request_kind)
            .await?;
        let counts_business_quota = if request_kind.key == "mcp:session-delete-unsupported" {
            0_i64
        } else if counts_business_quota {
            1_i64
        } else {
            0_i64
        };
        let failure_kind = failure_kind
            .map(str::to_string)
            .or_else(|| classify_failure_kind(path, http_status, mcp_status, error_message, &[]));
        let key_effect_summary = key_effect_summary.map(str::to_string);
        let binding_effect_summary = binding_effect_summary.map(str::to_string);
        let selection_effect_summary = selection_effect_summary.map(str::to_string);
        let diagnostic_metadata = self
            .resolve_request_log_diagnostic_metadata(request_log_id)
            .await?;
        let request_user_id = diagnostic_metadata
            .request_user_id
            .clone()
            .or(self.resolve_request_rollup_user_id(token_id, None).await?);
        let request_log_created_at = diagnostic_metadata.created_at;
        let upstream_operation_for_business = diagnostic_metadata.upstream_operation.clone();
        sqlx::query(
            r#"
            INSERT INTO core.auth_token_logs (
                token_id, method, path, query, http_status, mcp_status,
                request_kind_key, request_kind_label, request_kind_detail,
                result_status, error_message, failure_kind, key_effect_code, key_effect_summary,
                binding_effect_code, binding_effect_summary,
                selection_effect_code, selection_effect_summary,
                gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash,
                upstream_operation, fallback_reason,
                counts_business_quota, request_log_id, created_at
                , request_user_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28)
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(result_status)
        .bind(error_message)
        .bind(failure_kind)
        .bind(key_effect_code)
        .bind(key_effect_summary)
        .bind(binding_effect_code)
        .bind(binding_effect_summary)
        .bind(selection_effect_code)
        .bind(selection_effect_summary)
        .bind(diagnostic_metadata.gateway_mode)
        .bind(diagnostic_metadata.experiment_variant)
        .bind(diagnostic_metadata.proxy_session_id)
        .bind(diagnostic_metadata.routing_subject_hash)
        .bind(diagnostic_metadata.upstream_operation)
        .bind(diagnostic_metadata.fallback_reason)
        .bind(counts_business_quota)
        .bind(request_log_id)
        .bind(created_at)
        .bind(request_user_id.as_deref())
        .execute(self.pg()?)
        .await?;
        self.request_stats_coalescer
            .enqueue_auth_token_activity(token_id, request_user_id.as_deref(), created_at)
            .await;
        Ok(build_user_business_call_event_write(
            request_user_id,
            request_log_id,
            counts_business_quota,
            upstream_operation_for_business,
            result_status,
            request_log_created_at.unwrap_or(created_at),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn insert_token_log_pending_billing(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: bool,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: i64,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
        binding_effect_code: &str,
        binding_effect_summary: Option<&str>,
        selection_effect_code: &str,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
    ) -> Result<(i64, UserBusinessCallEventWriteDecision), ProxyError> {
        let created_at = self.backend_time.now_ts();
        let request_kind = self
            .resolve_token_log_request_kind(request_log_id, request_kind)
            .await?;
        let counts_business_quota = if request_kind.key == "mcp:session-delete-unsupported" {
            0_i64
        } else if counts_business_quota {
            1_i64
        } else {
            0_i64
        };
        let business_credits = if request_kind.key == "mcp:session-delete-unsupported" {
            None
        } else {
            Some(business_credits)
        };
        let failure_kind = failure_kind
            .map(str::to_string)
            .or_else(|| classify_failure_kind(path, http_status, mcp_status, error_message, &[]));
        let key_effect_summary = key_effect_summary.map(str::to_string);
        let binding_effect_summary = binding_effect_summary.map(str::to_string);
        let selection_effect_summary = selection_effect_summary.map(str::to_string);
        let diagnostic_metadata = self
            .resolve_request_log_diagnostic_metadata(request_log_id)
            .await?;
        let request_user_id = diagnostic_metadata.request_user_id.clone().or(self
            .resolve_request_rollup_user_id(token_id, Some(billing_subject))
            .await?);
        let request_log_created_at = diagnostic_metadata.created_at;
        let upstream_operation_for_business = diagnostic_metadata.upstream_operation.clone();
        let retry_budget = Duration::from_secs(10);
        let retry_budget_ms = retry_budget.as_millis() as u64;
        let deadline = self.backend_time.instant_now() + retry_budget;
        let operation_started = Instant::now();
        let billing_subject_kind = classify_billing_subject_kind(billing_subject);
        let log_fields = SqliteContentionLogFields {
            operation: "insert_token_log_pending_billing",
            request_path: path,
            request_kind: request_kind.key.as_str(),
            billing_subject_kind,
            retry_budget_ms,
            pending_batch_counts: "dashboard=0,api_key=0,auth_token=0,account_rollup=0,request_catalog=0",
            oldest_pending_created_at: None,
            newest_pending_created_at: None,
        };
        let mut retry_attempt = 0usize;
        let log_id = loop {
            match self
                .insert_token_log_pending_billing_once(
                    token_id,
                    method,
                    path,
                    query,
                    http_status,
                    mcp_status,
                    counts_business_quota,
                    result_status,
                    error_message,
                    business_credits,
                    billing_subject,
                    &request_kind,
                    api_key_id,
                    failure_kind.as_deref(),
                    key_effect_code,
                    key_effect_summary.as_deref(),
                    binding_effect_code,
                    binding_effect_summary.as_deref(),
                    selection_effect_code,
                    selection_effect_summary.as_deref(),
                    request_log_id,
                    created_at,
                    request_user_id.as_deref(),
                    &diagnostic_metadata,
                )
                .await
            {
                Ok(log_id) => {
                    log_slow_db_operation(
                        "insert_token_log_pending_billing",
                        operation_started.elapsed(),
                        Some(
                            format!(
                                "request_path={path}, request_kind={}, billing_subject_kind={billing_subject_kind}",
                                request_kind.key
                            )
                            .as_str(),
                        ),
                    );
                    break log_id;
                }
                Err(err) => {
                    if !is_transient_sqlite_write_error(&err) {
                        log_db_operation_error(
                            "insert_token_log_pending_billing",
                            operation_started.elapsed(),
                            Some(
                                format!(
                                    "request_path={path}, request_kind={}, billing_subject_kind={billing_subject_kind}",
                                    request_kind.key
                                )
                                .as_str(),
                            ),
                            &err,
                        );
                        return Err(err);
                    }

                    let now = self.backend_time.instant_now();
                    if now >= deadline {
                        log_sqlite_transient_write_exhaustion_with_fields(
                            log_fields,
                            retry_attempt + 1,
                            operation_started.elapsed(),
                            &err,
                        );
                        log_db_operation_error(
                            "insert_token_log_pending_billing",
                            operation_started.elapsed(),
                            Some(
                                format!(
                                    "request_path={path}, request_kind={}, billing_subject_kind={billing_subject_kind}",
                                    request_kind.key
                                )
                                .as_str(),
                            ),
                            &err,
                        );
                        return Err(err);
                    }

                    let remaining = deadline.saturating_duration_since(now);
                    let backoff = sqlite_transient_write_retry_delay(retry_attempt).min(remaining);
                    log_sqlite_transient_write_retry_with_fields(
                        log_fields,
                        retry_attempt + 1,
                        backoff,
                        operation_started.elapsed(),
                        &err,
                    );
                    self.backend_time.sleep(backoff).await;
                    retry_attempt += 1;
                }
            }
        };
        self.request_stats_coalescer
            .enqueue_auth_token_activity(token_id, request_user_id.as_deref(), created_at)
            .await;
        Ok((
            log_id,
            build_user_business_call_event_write(
                request_user_id,
                request_log_id,
                counts_business_quota,
                upstream_operation_for_business,
                result_status,
                request_log_created_at.unwrap_or(created_at),
            ),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn insert_token_log_pending_billing_once(
        &self,
        token_id: &str,
        method: &Method,
        path: &str,
        query: Option<&str>,
        http_status: Option<i64>,
        mcp_status: Option<i64>,
        counts_business_quota: i64,
        result_status: &str,
        error_message: Option<&str>,
        business_credits: Option<i64>,
        billing_subject: &str,
        request_kind: &TokenRequestKind,
        api_key_id: Option<&str>,
        failure_kind: Option<&str>,
        key_effect_code: &str,
        key_effect_summary: Option<&str>,
        binding_effect_code: &str,
        binding_effect_summary: Option<&str>,
        selection_effect_code: &str,
        selection_effect_summary: Option<&str>,
        request_log_id: Option<i64>,
        created_at: i64,
        request_user_id: Option<&str>,
        diagnostic_metadata: &RequestLogDiagnosticMetadata,
    ) -> Result<i64, ProxyError> {
        let mut tx = self.pg()?.begin().await?;
        let log_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO core.auth_token_logs (
                token_id,
                method,
                path,
                query,
                http_status,
                mcp_status,
                request_kind_key,
                request_kind_label,
                request_kind_detail,
                result_status,
                error_message,
                failure_kind,
                key_effect_code,
                key_effect_summary,
                binding_effect_code,
                binding_effect_summary,
                selection_effect_code,
                selection_effect_summary,
                gateway_mode,
                experiment_variant,
                proxy_session_id,
                routing_subject_hash,
                upstream_operation,
                fallback_reason,
                counts_business_quota,
                business_credits,
                billing_subject,
                billing_state,
                api_key_id,
                request_log_id,
                created_at,
                request_user_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31, $32)
            RETURNING id
            "#,
        )
        .bind(token_id)
        .bind(method.as_str())
        .bind(path)
        .bind(query)
        .bind(http_status)
        .bind(mcp_status)
        .bind(&request_kind.key)
        .bind(&request_kind.label)
        .bind(request_kind.detail.as_deref())
        .bind(result_status)
        .bind(error_message)
        .bind(failure_kind)
        .bind(key_effect_code)
        .bind(key_effect_summary)
        .bind(binding_effect_code)
        .bind(binding_effect_summary)
        .bind(selection_effect_code)
        .bind(selection_effect_summary)
        .bind(diagnostic_metadata.gateway_mode.as_deref())
        .bind(diagnostic_metadata.experiment_variant.as_deref())
        .bind(diagnostic_metadata.proxy_session_id.as_deref())
        .bind(diagnostic_metadata.routing_subject_hash.as_deref())
        .bind(diagnostic_metadata.upstream_operation.as_deref())
        .bind(diagnostic_metadata.fallback_reason.as_deref())
        .bind(counts_business_quota)
        .bind(business_credits)
        .bind(billing_subject)
        .bind(if request_kind.key == "mcp:session-delete-unsupported" {
            BILLING_STATE_NONE
        } else {
            BILLING_STATE_PENDING
        })
        .bind(api_key_id)
        .bind(request_log_id)
        .bind(created_at)
        .bind(request_user_id)
        .fetch_one(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO core.billing_ledger (
                auth_token_log_id,
                token_id,
                billing_subject,
                billing_state,
                business_credits,
                request_user_id,
                api_key_id,
                request_log_id,
                result_status,
                created_at,
                updated_at,
                settled_at,
                error_message
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, NULL, $12)
            ON CONFLICT (auth_token_log_id) DO UPDATE SET
                token_id = excluded.token_id,
                billing_subject = excluded.billing_subject,
                billing_state = excluded.billing_state,
                business_credits = excluded.business_credits,
                request_user_id = excluded.request_user_id,
                api_key_id = excluded.api_key_id,
                request_log_id = excluded.request_log_id,
                result_status = excluded.result_status,
                created_at = excluded.created_at,
                updated_at = excluded.updated_at,
                settled_at = NULL,
                error_message = excluded.error_message
            "#,
        )
        .bind(log_id)
        .bind(token_id)
        .bind(billing_subject)
        .bind(if request_kind.key == "mcp:session-delete-unsupported" {
            BILLING_STATE_NONE
        } else {
            BILLING_STATE_PENDING
        })
        .bind(business_credits)
        .bind(request_user_id)
        .bind(api_key_id)
        .bind(request_log_id)
        .bind(result_status)
        .bind(created_at)
        .bind(created_at)
        .bind(error_message)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(log_id)
    }

    async fn resolve_request_log_diagnostic_metadata(
        &self,
        request_log_id: Option<i64>,
    ) -> Result<RequestLogDiagnosticMetadata, ProxyError> {
        let Some(request_log_id) = request_log_id else {
            return Ok(RequestLogDiagnosticMetadata::default());
        };
        if let Some(metadata) = self
            .request_log_diagnostic_handoff
            .lock()
            .await
            .take(request_log_id)
        {
            return Ok(metadata);
        }

        let row = sqlx::query_as::<_, (
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )>(
            r#"
            SELECT created_at, request_user_id, gateway_mode, experiment_variant, proxy_session_id, routing_subject_hash, upstream_operation, fallback_reason
            FROM observability.request_logs
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(request_log_id)
        .fetch_optional(self.pg()?)
        .await?;

        Ok(row
            .map(
                |(
                    created_at,
                    request_user_id,
                    gateway_mode,
                    experiment_variant,
                    proxy_session_id,
                    routing_subject_hash,
                    upstream_operation,
                    fallback_reason,
                )| RequestLogDiagnosticMetadata {
                    created_at,
                    request_user_id,
                    gateway_mode,
                    experiment_variant,
                    proxy_session_id,
                    routing_subject_hash,
                    upstream_operation,
                    fallback_reason,
                },
            )
            .unwrap_or_default())
    }

    async fn resolve_token_log_request_kind(
        &self,
        request_log_id: Option<i64>,
        fallback: &TokenRequestKind,
    ) -> Result<TokenRequestKind, ProxyError> {
        let Some(request_log_id) = request_log_id else {
            return Ok(fallback.clone());
        };

        let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
            r#"
            SELECT request_kind_key, request_kind_label, request_kind_detail
            FROM observability.request_logs
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(request_log_id)
        .fetch_optional(self.pg()?)
        .await?;

        Ok(row
            .map(|(key, label, detail)| {
                key.as_deref()
                    .and_then(|stored_key| {
                        token_request_kind_from_canonical_key(stored_key, detail.clone())
                    })
                    .unwrap_or_else(|| {
                        TokenRequestKind::new(
                            key.unwrap_or_else(|| fallback.key.clone()),
                            label.unwrap_or_else(|| fallback.label.clone()),
                            detail.or_else(|| fallback.detail.clone()),
                        )
                    })
            })
            .unwrap_or_else(|| fallback.clone()))
    }

    pub(crate) async fn list_pending_billing_log_ids(
        &self,
        billing_subject: &str,
    ) -> Result<Vec<i64>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT auth_token_log_id
            FROM core.billing_ledger
            WHERE billing_state = $1 AND billing_subject = $2 AND COALESCE(business_credits, 0) > 0
              AND auth_token_log_id > 0
            ORDER BY auth_token_log_id ASC
            "#,
        )
        .bind(BILLING_STATE_PENDING)
        .bind(billing_subject)
        .fetch_all(self.pg()?)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn list_pending_billing_subjects_for_token(
        &self,
        token_id: &str,
    ) -> Result<Vec<String>, ProxyError> {
        sqlx::query_scalar(
            r#"
            SELECT DISTINCT billing_subject
            FROM core.billing_ledger
            WHERE billing_state = $1
              AND token_id = $2
              AND billing_subject IS NOT NULL
              AND COALESCE(business_credits, 0) > 0
              AND auth_token_log_id > 0
            ORDER BY billing_subject ASC
            "#,
        )
        .bind(BILLING_STATE_PENDING)
        .bind(token_id)
        .fetch_all(self.pg()?)
        .await
        .map_err(ProxyError::from)
    }

    pub(crate) async fn apply_pending_billing_log(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        let retry_budget = Duration::from_secs(10);
        let deadline = Instant::now() + retry_budget;
        let mut retry_attempt = 0usize;
        let operation_started = Instant::now();
        let context = format!("log_id={log_id}");
        let (request_path, request_kind, billing_subject_kind) = self
            .pending_billing_log_retry_metadata(log_id)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                (
                    "/internal/pending-billing-settle".to_string(),
                    "internal:pending-billing-settle".to_string(),
                    "unknown".to_string(),
                )
            });
        let log_fields = SqliteContentionLogFields {
            operation: "apply_pending_billing_log",
            request_path: request_path.as_str(),
            request_kind: request_kind.as_str(),
            billing_subject_kind: billing_subject_kind.as_str(),
            retry_budget_ms: retry_budget.as_millis() as u64,
            pending_batch_counts: "dashboard=0,api_key=0,auth_token=0,account_rollup=0,request_catalog=0",
            oldest_pending_created_at: None,
            newest_pending_created_at: None,
        };
        loop {
            match self.apply_pending_billing_log_once(log_id).await {
                Ok(outcome) => {
                    log_slow_db_operation(
                        "apply_pending_billing_log",
                        operation_started.elapsed(),
                        Some(context.as_str()),
                    );
                    return Ok(outcome);
                }
                Err(err) => {
                    if is_transient_sqlite_write_error(&err) {
                        let now = self.backend_time.instant_now();
                        if now < deadline {
                            let remaining = deadline.saturating_duration_since(now);
                            let backoff =
                                sqlite_transient_write_retry_delay(retry_attempt).min(remaining);
                            log_sqlite_transient_write_retry_with_fields(
                                log_fields,
                                retry_attempt + 1,
                                backoff,
                                operation_started.elapsed(),
                                &err,
                            );
                            self.backend_time.sleep(backoff).await;
                            retry_attempt += 1;
                            continue;
                        }
                        log_sqlite_transient_write_exhaustion_with_fields(
                            log_fields,
                            retry_attempt + 1,
                            operation_started.elapsed(),
                            &err,
                        );
                    }
                    log_db_operation_error(
                        "apply_pending_billing_log",
                        operation_started.elapsed(),
                        Some(context.as_str()),
                        &err,
                    );
                    return Err(err);
                }
            }
        }
    }

    async fn pending_billing_log_retry_metadata(
        &self,
        log_id: i64,
    ) -> Result<Option<(String, String, String)>, ProxyError> {
        let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
            r#"
            SELECT atl.path, atl.request_kind_key, bl.billing_subject
            FROM core.billing_ledger bl
            LEFT JOIN core.auth_token_logs atl ON atl.id = bl.auth_token_log_id
            WHERE bl.auth_token_log_id = $1
            LIMIT 1
            "#,
        )
        .bind(log_id)
        .fetch_optional(self.pg()?)
        .await?;
        Ok(row.map(|(path, request_kind, billing_subject)| {
            let billing_subject_kind = billing_subject
                .as_deref()
                .map(classify_billing_subject_kind)
                .unwrap_or("unknown")
                .to_string();
            (
                path.unwrap_or_else(|| "/internal/pending-billing-settle".to_string()),
                request_kind.unwrap_or_else(|| "internal:pending-billing-settle".to_string()),
                billing_subject_kind,
            )
        }))
    }

    async fn apply_pending_billing_log_once(
        &self,
        log_id: i64,
    ) -> Result<PendingBillingSettleOutcome, ProxyError> {
        let mut tx = self.pg()?.begin().await?;
        #[cfg(test)]
        let force_claim_miss = {
            let mut forced = self.forced_pending_claim_miss_log_ids.lock().await;
            forced.remove(&log_id)
        };
        #[cfg(not(test))]
        let force_claim_miss = false;

        let claimed = if force_claim_miss {
            None
        } else {
            sqlx::query_as::<_, (i64, Option<String>, i64, Option<String>, String, Option<i64>)>(
                r#"
                SELECT
                    COALESCE(business_credits, 0),
                    billing_subject,
                    COALESCE(
                        (SELECT created_at FROM core.auth_token_logs WHERE id = billing_ledger.auth_token_log_id),
                        billing_ledger.created_at
                    ),
                    api_key_id,
                    result_status,
                    request_log_id
                FROM core.billing_ledger
                WHERE auth_token_log_id = $1 AND billing_state = $2
                LIMIT 1
                "#,
            )
            .bind(log_id)
            .bind(BILLING_STATE_PENDING)
            .fetch_optional(&mut *tx)
            .await?
        };

        let Some((credits, billing_subject, created_at, api_key_id, result_status, request_log_id)) =
            claimed
        else {
            let billing_state = sqlx::query_scalar::<_, String>(
                "SELECT billing_state FROM core.billing_ledger WHERE auth_token_log_id = $1 LIMIT 1",
            )
            .bind(log_id)
            .fetch_optional(&mut *tx)
            .await?;
            match billing_state.as_deref() {
                Some(BILLING_STATE_CHARGED) => {
                    tx.commit().await?;
                    return Ok(PendingBillingSettleOutcome::AlreadySettled);
                }
                Some(BILLING_STATE_PENDING) => {
                    tx.commit().await?;
                    return Ok(PendingBillingSettleOutcome::RetryLater);
                }
                Some(other) => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::QuotaDataMissing {
                        reason: format!(
                            "invalid billing_state for auth_token_logs.id={log_id}: {other}",
                        ),
                    });
                }
                None => {
                    tx.rollback().await.ok();
                    return Err(ProxyError::Other(format!(
                        "pending billing log not found: {log_id}",
                    )));
                }
            }
        };

        if credits <= 0 {
            let settled_at = self.backend_time.now_ts();
            let updated = sqlx::query(
                r#"
                UPDATE core.billing_ledger
                SET billing_state = $1,
                    created_at = COALESCE(
                        (SELECT created_at FROM core.auth_token_logs WHERE id = core.billing_ledger.auth_token_log_id),
                        core.billing_ledger.created_at
                    ),
                    updated_at = $2,
                    settled_at = $3,
                    error_message = NULL
                WHERE auth_token_log_id = $4 AND billing_state = $5
                "#,
            )
            .bind(BILLING_STATE_CHARGED)
            .bind(settled_at)
            .bind(settled_at)
            .bind(log_id)
            .bind(BILLING_STATE_PENDING)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() == 0 {
                tx.rollback().await.ok();
                return Ok(PendingBillingSettleOutcome::RetryLater);
            }
            sqlx::query(
                r#"
                UPDATE core.auth_token_logs
                SET billing_state = $1
                WHERE id = $2
                "#,
            )
            .bind(BILLING_STATE_CHARGED)
            .bind(log_id)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(PendingBillingSettleOutcome::Charged);
        }

        let Some(billing_subject) = billing_subject else {
            tx.rollback().await.ok();
            return Err(ProxyError::QuotaDataMissing {
                reason: format!("missing billing_subject for auth_token_logs.id={log_id}"),
            });
        };

        let charge_time = Utc
            .timestamp_opt(created_at, 0)
            .single()
            .unwrap_or_else(|| self.backend_time.now_utc());
        let charge_ts = charge_time.timestamp();
        let minute_bucket = charge_ts - (charge_ts % SECS_PER_MINUTE);
        let day_bucket = local_day_bucket_start_utc_ts(charge_ts);
        let month_start = start_of_month(charge_time).timestamp();

        let source_mutation = if let Some(request_log_id) = request_log_id {
            let source_mutation = self
                .request_stats_coalescer
                .begin_dashboard_rollup_source_mutation(charge_ts);
            sqlx::query(
                r#"
                UPDATE observability.request_logs
                SET business_credits = $1
                WHERE id = $2
                "#,
            )
            .bind(credits)
            .bind(request_log_id)
            .execute(&mut *tx)
            .await?;
            Some(source_mutation)
        } else {
            None
        };

        if let Some(user_id) = billing_subject.strip_prefix("account:") {
            sqlx::query(
                r#"
                INSERT INTO core.account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (user_id, bucket_start, granularity)
                DO UPDATE SET count = core.account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(minute_bucket)
            .bind(GRANULARITY_MINUTE)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO core.account_usage_buckets (user_id, bucket_start, granularity, count)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (user_id, bucket_start, granularity)
                DO UPDATE SET count = core.account_usage_buckets.count + excluded.count
                "#,
            )
            .bind(user_id)
            .bind(day_bucket)
            .bind(GRANULARITY_DAY)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            let (_month_start, _month_count): (i64, i64) = sqlx::query_as(
                r#"
                INSERT INTO core.account_monthly_quota (user_id, month_start, month_count)
                VALUES ($1, $2, $3)
                ON CONFLICT (user_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > core.account_monthly_quota.month_start THEN excluded.month_start
                        ELSE core.account_monthly_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > core.account_monthly_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < core.account_monthly_quota.month_start THEN core.account_monthly_quota.month_count
                        ELSE core.account_monthly_quota.month_count + excluded.month_count
                    END
                RETURNING month_start, month_count
                "#,
            )
            .bind(user_id)
            .bind(month_start)
            .bind(credits)
            .fetch_one(&mut *tx)
            .await?;

            self.record_account_business_credit_rollups(&mut tx, user_id, created_at, credits)
                .await?;

            if let Some(api_key_id) = api_key_id.as_deref() {
                self.increment_api_key_user_usage_bucket_pg(
                    &mut tx,
                    api_key_id,
                    user_id,
                    local_day_bucket_start_utc_ts(charge_ts),
                    credits,
                    result_status.as_str(),
                )
                .await?;

                if result_status == OUTCOME_SUCCESS {
                    self.refresh_user_api_key_binding_pg(&mut tx, user_id, api_key_id, created_at)
                        .await?;
                    let now = self.backend_time.now_ts();
                    sqlx::query(
                        r#"
                        INSERT INTO core.user_primary_api_key_affinity (user_id, api_key_id, created_at, updated_at)
                        VALUES ($1, $2, $3, $4)
                        ON CONFLICT (user_id) DO UPDATE SET
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
                        INSERT INTO core.token_primary_api_key_affinity (
                            token_id,
                            user_id,
                            api_key_id,
                            created_at,
                            updated_at
                        )
                        SELECT token_id, user_id, $1, $2, $3
                        FROM core.user_token_bindings
                        WHERE user_id = $4
                        ON CONFLICT (token_id) DO UPDATE SET
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
                }
            }
        } else if let Some(token_id) = billing_subject.strip_prefix("token:") {
            sqlx::query(
                r#"
                INSERT INTO core.token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (token_id, bucket_start, granularity)
                DO UPDATE SET count = core.token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(minute_bucket)
            .bind(GRANULARITY_MINUTE)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                INSERT INTO core.token_usage_buckets (token_id, bucket_start, granularity, count)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (token_id, bucket_start, granularity)
                DO UPDATE SET count = core.token_usage_buckets.count + excluded.count
                "#,
            )
            .bind(token_id)
            .bind(day_bucket)
            .bind(GRANULARITY_DAY)
            .bind(credits)
            .execute(&mut *tx)
            .await?;

            let (_month_start, _month_count): (i64, i64) = sqlx::query_as(
                r#"
                INSERT INTO core.auth_token_quota (token_id, month_start, month_count)
                VALUES ($1, $2, $3)
                ON CONFLICT (token_id) DO UPDATE SET
                    month_start = CASE
                        WHEN excluded.month_start > core.auth_token_quota.month_start THEN excluded.month_start
                        ELSE core.auth_token_quota.month_start
                    END,
                    month_count = CASE
                        WHEN excluded.month_start > core.auth_token_quota.month_start THEN excluded.month_count
                        WHEN excluded.month_start < core.auth_token_quota.month_start THEN core.auth_token_quota.month_count
                        ELSE core.auth_token_quota.month_count + excluded.month_count
                    END
                RETURNING month_start, month_count
                "#,
            )
            .bind(token_id)
            .bind(month_start)
            .bind(credits)
            .fetch_one(&mut *tx)
            .await?;

            if let Some(api_key_id) = api_key_id.as_deref()
                && result_status == OUTCOME_SUCCESS
            {
                self.refresh_token_api_key_binding_pg(&mut tx, token_id, api_key_id, created_at)
                    .await?;
                let now = self.backend_time.now_ts();
                sqlx::query(
                    r#"
                    INSERT INTO core.token_primary_api_key_affinity (
                        token_id,
                        user_id,
                        api_key_id,
                        created_at,
                        updated_at
                    )
                    VALUES ($1, NULL, $2, $3, $4)
                    ON CONFLICT (token_id) DO UPDATE SET
                        user_id = excluded.user_id,
                        api_key_id = excluded.api_key_id,
                        updated_at = excluded.updated_at
                    "#,
                )
                .bind(token_id)
                .bind(api_key_id)
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            }
        } else {
            tx.rollback().await.ok();
            return Err(ProxyError::QuotaDataMissing {
                reason: format!(
                    "invalid billing_subject for auth_token_logs.id={log_id}: {billing_subject}",
                ),
            });
        }

        let settled_at = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"
            UPDATE core.billing_ledger
            SET billing_state = $1,
                created_at = COALESCE(
                    (SELECT created_at FROM core.auth_token_logs WHERE id = core.billing_ledger.auth_token_log_id),
                    core.billing_ledger.created_at
                ),
                updated_at = $2,
                settled_at = $3,
                error_message = NULL
            WHERE auth_token_log_id = $4 AND billing_state = $5
            "#,
        )
        .bind(BILLING_STATE_CHARGED)
        .bind(settled_at)
        .bind(settled_at)
        .bind(log_id)
        .bind(BILLING_STATE_PENDING)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Ok(PendingBillingSettleOutcome::RetryLater);
        }
        sqlx::query(
            r#"
            UPDATE core.auth_token_logs
            SET billing_state = $1
            WHERE id = $2
            "#,
        )
        .bind(BILLING_STATE_CHARGED)
        .bind(log_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.request_stats_coalescer
            .enqueue_dashboard_credit_rollups_for_request_log(charge_ts, credits, request_log_id)
            .await;
        if let Some(source_mutation) = source_mutation {
            // The source row and its coalesced delta become visible as one
            // integrity boundary; see the request-log insertion path.
            source_mutation.commit().await;
        }
        Ok(PendingBillingSettleOutcome::Charged)
    }

    pub(crate) async fn annotate_pending_billing_log(
        &self,
        log_id: i64,
        message: &str,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE core.billing_ledger
            SET error_message = CASE
                WHEN error_message IS NULL OR error_message = '' THEN $1
                WHEN error_message = $2 THEN error_message
                ELSE error_message || ' | ' || $3
            END,
                updated_at = $4
            WHERE auth_token_log_id = $5
            "#,
        )
        .bind(message)
        .bind(message)
        .bind(message)
        .bind(self.backend_time.now_ts())
        .bind(log_id)
        .execute(self.pg()?)
        .await?;
        sqlx::query(
            r#"
            UPDATE core.auth_token_logs
            SET error_message = CASE
                WHEN error_message IS NULL OR error_message = '' THEN $1
                WHEN error_message = $2 THEN error_message
                ELSE error_message || ' | ' || $3
            END
            WHERE id = $4
            "#,
        )
        .bind(message)
        .bind(message)
        .bind(message)
        .bind(log_id)
        .execute(self.pg()?)
        .await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn acquire_quota_subject_lock(
        &self,
        subject: &str,
        ttl: Duration,
        wait_timeout: Duration,
    ) -> Result<QuotaSubjectDbLease, ProxyError> {
        let owner = format!(
            "{}:{}",
            std::process::id(),
            QUOTA_SUBJECT_LOCK_OWNER_SEQ.fetch_add(1, AtomicOrdering::Relaxed)
        );
        let deadline = self.backend_time.deadline_after(wait_timeout);
        let ttl_secs = ttl.as_secs().max(1) as i64;
        let mut transient_retry_attempt = 0usize;

        loop {
            let now = self.backend_time.now_ts();
            let expires_at = now + ttl_secs;
            let mut tx = match self.pg()?.begin().await {
                Ok(tx) => tx,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "quota subject lock acquire begin",
                        transient_retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        transient_retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            };

            let inserted_result: Result<sqlx::postgres::PgQueryResult, ProxyError> = async {
                sqlx::query(
                    "DELETE FROM core.quota_subject_locks WHERE subject = $1 AND expires_at <= $2",
                )
                .bind(subject)
                .bind(now)
                .execute(&mut *tx)
                .await?;

                let inserted = sqlx::query(
                    r#"
                    INSERT INTO core.quota_subject_locks (subject, owner, expires_at, updated_at)
                    VALUES ($1, $2, $3, $4)
                    ON CONFLICT DO NOTHING
                    "#,
                )
                .bind(subject)
                .bind(&owner)
                .bind(expires_at)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                Ok(inserted)
            }
            .await;

            let inserted = match inserted_result {
                Ok(inserted) => inserted,
                Err(err) => {
                    tx.rollback().await.ok();
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "quota subject lock acquire write",
                        transient_retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        transient_retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            };

            if inserted.rows_affected() == 1 {
                if let Err(err) = tx.commit().await {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "quota subject lock acquire commit",
                        transient_retry_attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        transient_retry_attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
                return Ok(QuotaSubjectDbLease {
                    subject: subject.to_string(),
                    owner,
                    ttl,
                });
            }

            tx.rollback().await.ok();
            transient_retry_attempt = 0;
            if self.backend_time.instant_now() >= deadline {
                return Err(ProxyError::Other(format!(
                    "timed out acquiring quota subject lock for {subject}",
                )));
            }
            self.backend_time.sleep(Duration::from_millis(50)).await;
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) async fn refresh_quota_subject_lock(
        &self,
        lease: &QuotaSubjectDbLease,
    ) -> Result<(), ProxyError> {
        let deadline = self.backend_time.deadline_after(lease.ttl);
        let mut attempt = 0usize;
        let rows = loop {
            let now = self.backend_time.now_ts();
            let expires_at = now + lease.ttl.as_secs().max(1) as i64;
            match sqlx::query(
                "UPDATE core.quota_subject_locks SET expires_at = $1, updated_at = $2 WHERE subject = $3 AND owner = $4",
            )
            .bind(expires_at)
            .bind(now)
            .bind(&lease.subject)
            .bind(&lease.owner)
            .execute(self.pg()?)
            .await
            {
                Ok(rows) => break rows,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "quota subject lock refresh",
                        attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        };
        if rows.rows_affected() == 0 {
            return Err(ProxyError::Other(format!(
                "quota subject lock lost for {}",
                lease.subject,
            )));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn release_quota_subject_lock(
        &self,
        lease: &QuotaSubjectDbLease,
    ) -> Result<(), ProxyError> {
        let deadline = self.backend_time.deadline_after(Duration::from_secs(2));
        let mut attempt = 0usize;
        loop {
            match sqlx::query(
                "DELETE FROM core.quota_subject_locks WHERE subject = $1 AND owner = $2",
            )
            .bind(&lease.subject)
            .bind(&lease.owner)
            .execute(self.pg()?)
            .await
            {
                Ok(_) => break,
                Err(err) => {
                    let err = ProxyError::Database(err);
                    if sleep_before_sqlite_transient_write_retry(
                        &self.backend_time,
                        "quota subject lock release",
                        attempt,
                        deadline,
                        &err,
                    )
                    .await
                    {
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}

fn build_user_business_call_event_write(
    request_user_id: Option<String>,
    request_log_id: Option<i64>,
    counts_business_quota: i64,
    upstream_operation: Option<String>,
    result_status: &str,
    created_at: i64,
) -> UserBusinessCallEventWriteDecision {
    let Some(user_id) = request_user_id else {
        return UserBusinessCallEventWriteDecision::Skipped {
            request_log_id,
            reason: UserBusinessCallEventSkipReason::MissingUserId,
        };
    };
    if counts_business_quota != 1 {
        return UserBusinessCallEventWriteDecision::Skipped {
            request_log_id,
            reason: UserBusinessCallEventSkipReason::NotBusinessQuota,
        };
    }
    if result_status == OUTCOME_QUOTA_EXHAUSTED {
        return UserBusinessCallEventWriteDecision::Skipped {
            request_log_id,
            reason: UserBusinessCallEventSkipReason::QuotaExhausted,
        };
    }
    // Keep live eligibility identical to durable backfill and server-pressure
    // reconstruction; metadata-free token logs stay excluded.
    if upstream_operation.is_none() {
        return UserBusinessCallEventWriteDecision::Skipped {
            request_log_id,
            reason: UserBusinessCallEventSkipReason::MissingUpstreamOperation,
        };
    }
    UserBusinessCallEventWriteDecision::Applied(UserBusinessCallEventWrite {
        user_id,
        request_log_id,
        created_at,
        result_status: result_status.to_string(),
    })
}

#[cfg(test)]
mod user_business_call_event_write_tests {
    use super::*;

    fn decision(
        request_user_id: Option<&str>,
        counts_business_quota: i64,
        upstream_operation: Option<&str>,
        result_status: &str,
    ) -> UserBusinessCallEventWriteDecision {
        build_user_business_call_event_write(
            request_user_id.map(str::to_string),
            Some(42),
            counts_business_quota,
            upstream_operation.map(str::to_string),
            result_status,
            1_700_200_000,
        )
    }

    #[test]
    fn skip_reasons_are_stable_and_precedence_ordered() {
        assert_eq!(
            decision(None, 1, Some("http_search"), OUTCOME_SUCCESS),
            UserBusinessCallEventWriteDecision::Skipped {
                request_log_id: Some(42),
                reason: UserBusinessCallEventSkipReason::MissingUserId,
            }
        );
        assert_eq!(
            decision(Some("user"), 0, Some("http_search"), OUTCOME_SUCCESS),
            UserBusinessCallEventWriteDecision::Skipped {
                request_log_id: Some(42),
                reason: UserBusinessCallEventSkipReason::NotBusinessQuota,
            }
        );
        assert_eq!(
            decision(
                Some("user"),
                1,
                Some("http_search"),
                OUTCOME_QUOTA_EXHAUSTED,
            ),
            UserBusinessCallEventWriteDecision::Skipped {
                request_log_id: Some(42),
                reason: UserBusinessCallEventSkipReason::QuotaExhausted,
            }
        );
        assert_eq!(
            decision(Some("user"), 1, None, OUTCOME_SUCCESS),
            UserBusinessCallEventWriteDecision::Skipped {
                request_log_id: Some(42),
                reason: UserBusinessCallEventSkipReason::MissingUpstreamOperation,
            }
        );
    }
}

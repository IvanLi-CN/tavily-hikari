use sha2::{Digest as _, Sha256};

fn admin_passkey_reset_token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.trim().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl KeyStore {
    pub async fn ensure_admin_passkey_scope(
        &self,
        scope: &AdminPasskeyScope,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"INSERT INTO admin_passkey_scopes
               (scope_id, node_id, rp_id, rp_origin, created_at, last_seen_at)
               VALUES (?, ?, ?, ?, ?, ?)
               ON CONFLICT(scope_id) DO UPDATE SET last_seen_at = excluded.last_seen_at"#,
        )
        .bind(&scope.id)
        .bind(&scope.node_id)
        .bind(&scope.rp_id)
        .bind(&scope.rp_origin)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn admin_passkey_scope_status(
        &self,
        scope: &AdminPasskeyScope,
    ) -> Result<AdminPasskeyScopeStatus, ProxyError> {
        let inactive_credential_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM admin_passkey_credentials WHERE scope_id != ? AND scope_id != ? AND revoked_at IS NULL",
        )
        .bind(&scope.id)
        .bind(LEGACY_ADMIN_PASSKEY_SCOPE_ID)
        .fetch_one(&self.pool)
        .await?;
        let legacy_credential_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM admin_passkey_credentials WHERE scope_id = ? AND revoked_at IS NULL",
        )
        .bind(LEGACY_ADMIN_PASSKEY_SCOPE_ID)
        .fetch_one(&self.pool)
        .await?;
        Ok(AdminPasskeyScopeStatus {
            inactive_credential_count,
            legacy_credential_count,
        })
    }

    pub async fn get_admin_password_settings(
        &self,
    ) -> Result<Option<AdminPasswordSettingsRecord>, ProxyError> {
        let row = sqlx::query_as::<_, (Option<String>, Option<i64>, i64, i64)>(
            r#"SELECT password_hash, disabled_at, updated_at, login_totp_required
               FROM admin_password_settings
               WHERE id = 1
               LIMIT 1"#,
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(password_hash, disabled_at, updated_at, login_totp_required)| AdminPasswordSettingsRecord {
                password_hash,
                disabled_at,
                updated_at,
                login_totp_required: login_totp_required != 0,
            },
        ))
    }

    pub async fn set_admin_password_hash(
        &self,
        password_hash: &str,
    ) -> Result<AdminPasswordSettingsRecord, ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"INSERT INTO admin_password_settings (id, password_hash, disabled_at, updated_at, login_totp_required)
               VALUES (1, ?, NULL, ?, 0)
               ON CONFLICT(id) DO UPDATE SET
                   password_hash = excluded.password_hash,
                   disabled_at = NULL,
                   updated_at = excluded.updated_at"#,
        )
        .bind(password_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        let login_totp_required = self
            .get_admin_password_settings()
            .await?
            .map(|settings| settings.login_totp_required)
            .unwrap_or(false);
        Ok(AdminPasswordSettingsRecord {
            password_hash: Some(password_hash.to_string()),
            disabled_at: None,
            updated_at: now,
            login_totp_required,
        })
    }

    pub async fn disable_admin_password_preserving_login(
        &self,
        external_admin_login_available: bool,
        runtime_passkey_login_available: bool,
        passkey_scope: Option<&AdminPasskeyScope>,
    ) -> Result<AdminPasswordSettingsRecord, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = ImmediateSqliteTransaction::begin(self.pool.acquire().await?).await?;

        let result: Result<AdminPasswordSettingsRecord, ProxyError> = async {
            if !external_admin_login_available {
                if !runtime_passkey_login_available {
                    return Err(ProxyError::LastAdminLoginMethod);
                }
                let active_passkey_count = match passkey_scope {
                    Some(scope) => sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM admin_passkey_credentials WHERE scope_id = ? AND revoked_at IS NULL",
                    )
                    .bind(&scope.id)
                    .fetch_one(&mut *conn)
                    .await?,
                    None => sqlx::query_scalar::<_, i64>(
                        "SELECT COUNT(*) FROM admin_passkey_credentials WHERE revoked_at IS NULL",
                    )
                    .fetch_one(&mut *conn)
                    .await?,
                };
                if active_passkey_count == 0 {
                    return Err(ProxyError::LastAdminLoginMethod);
                }
            }

            sqlx::query(
                r#"INSERT INTO admin_password_settings (id, password_hash, disabled_at, updated_at, login_totp_required)
                   VALUES (1, NULL, ?, ?, 0)
                   ON CONFLICT(id) DO UPDATE SET
                       password_hash = NULL,
                       disabled_at = excluded.disabled_at,
                       updated_at = excluded.updated_at"#,
            )
            .bind(now)
            .bind(now)
            .execute(&mut *conn)
            .await?;
            let login_totp_required = sqlx::query_scalar::<_, i64>(
                r#"SELECT login_totp_required
                   FROM admin_password_settings
                   WHERE id = 1
                   LIMIT 1"#,
            )
            .fetch_optional(&mut *conn)
            .await?
            .unwrap_or(0)
                != 0;
            Ok(AdminPasswordSettingsRecord {
                password_hash: None,
                disabled_at: Some(now),
                updated_at: now,
                login_totp_required,
            })
        }
        .await;

        match result {
            Ok(settings) => {
                conn.commit().await?;
                Ok(settings)
            }
            Err(err) => {
                let _ = conn.rollback().await;
                Err(err)
            }
        }
    }

    pub async fn set_admin_login_totp_required(
        &self,
        required: bool,
        passkey_scope: Option<&AdminPasskeyScope>,
    ) -> Result<AdminPasswordSettingsRecord, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = ImmediateSqliteTransaction::begin(self.pool.acquire().await?).await?;

        let result: Result<AdminPasswordSettingsRecord, ProxyError> = async {
        sqlx::query(
            r#"INSERT INTO admin_password_settings (
                   id, password_hash, disabled_at, updated_at, login_totp_required
               )
               VALUES (1, NULL, NULL, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                   login_totp_required = excluded.login_totp_required,
                   updated_at = excluded.updated_at"#,
        )
        .bind(now)
        .bind(if required { 1_i64 } else { 0_i64 })
        .execute(&mut *conn)
        .await?;
            if required {
                let mut query = sqlx::query(
                    "UPDATE admin_passkey_sessions SET revoked_at = ? WHERE revoked_at IS NULL",
                )
                .bind(now);
                if let Some(scope) = passkey_scope {
                    query = sqlx::query(
                        "UPDATE admin_passkey_sessions SET revoked_at = ? WHERE scope_id = ? AND revoked_at IS NULL",
                    )
                    .bind(now)
                    .bind(&scope.id);
                }
                query.execute(&mut *conn).await?;
            }
            let settings = sqlx::query_as::<_, (Option<String>, Option<i64>, i64, i64)>(
                r#"SELECT password_hash, disabled_at, updated_at, login_totp_required
                   FROM admin_password_settings
                   WHERE id = 1
                   LIMIT 1"#,
            )
            .fetch_optional(&mut *conn)
            .await?
            .map(|(password_hash, disabled_at, updated_at, login_totp_required)| {
                AdminPasswordSettingsRecord {
                    password_hash,
                    disabled_at,
                    updated_at,
                    login_totp_required: login_totp_required != 0,
                }
            })
            .ok_or_else(|| ProxyError::Other("admin password settings missing".to_string()))?;
            Ok(settings)
        }
        .await;

        match result {
            Ok(settings) => {
                conn.commit().await?;
                Ok(settings)
            }
            Err(err) => {
                let _ = conn.rollback().await;
                Err(err)
            }
        }
    }

    pub async fn admin_passkey_enabled(&self, scope: &AdminPasskeyScope) -> Result<bool, ProxyError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM admin_passkey_credentials WHERE scope_id = ? AND revoked_at IS NULL",
        )
        .bind(&scope.id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn create_admin_passkey_reset_token(
        &self,
        scope: &AdminPasskeyScope,
        ttl_secs: i64,
    ) -> Result<AdminPasskeyResetTokenRecord, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM admin_passkey_reset_tokens WHERE scope_id = ? AND (expires_at < ? OR consumed_at IS NOT NULL)",
        )
        .bind(&scope.id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let token_hash = admin_passkey_reset_token_hash(&token);
            let res = sqlx::query(
                r#"INSERT INTO admin_passkey_reset_tokens
                   (token_hash, scope_id, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, ?, NULL)"#,
            )
            .bind(&token_hash)
            .bind(&scope.id)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    return Ok(AdminPasskeyResetTokenRecord {
                        token: Some(token),
                        token_hash,
                        created_at: now,
                        expires_at,
                        consumed_at: None,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub async fn get_active_admin_passkey_reset_token(
        &self,
        scope: &AdminPasskeyScope,
        token: &str,
    ) -> Result<Option<AdminPasskeyResetTokenRecord>, ProxyError> {
        let now = self.backend_time.now_ts();
        let token_hash = admin_passkey_reset_token_hash(token);
        let row = sqlx::query_as::<_, (String, i64, i64, Option<i64>)>(
            r#"SELECT token_hash, created_at, expires_at, consumed_at
               FROM admin_passkey_reset_tokens
               WHERE token_hash = ? AND scope_id = ? AND consumed_at IS NULL AND expires_at >= ?
               LIMIT 1"#,
        )
        .bind(&token_hash)
        .bind(&scope.id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(token_hash, created_at, expires_at, consumed_at)| {
            AdminPasskeyResetTokenRecord {
                token: None,
                token_hash,
                created_at,
                expires_at,
                consumed_at,
            }
        }))
    }

    pub async fn consume_admin_passkey_reset_token_hash(
        &self,
        scope: &AdminPasskeyScope,
        token_hash: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            "DELETE FROM admin_passkey_reset_tokens WHERE scope_id = ? AND (expires_at < ? OR consumed_at IS NOT NULL)",
        )
        .bind(&scope.id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let updated = sqlx::query(
            r#"UPDATE admin_passkey_reset_tokens
               SET consumed_at = ?
               WHERE token_hash = ? AND scope_id = ? AND consumed_at IS NULL AND expires_at >= ?"#,
        )
        .bind(now)
        .bind(token_hash)
        .bind(&scope.id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn complete_admin_passkey_reset_registration(
        &self,
        scope: &AdminPasskeyScope,
        token_hash: &str,
        credential_id: &str,
        passkey_json: &str,
        label: Option<&str>,
        revoke_credential_ids: &[String],
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM admin_passkey_reset_tokens WHERE scope_id = ? AND (expires_at < ? OR consumed_at IS NOT NULL)",
        )
        .bind(&scope.id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let consumed = sqlx::query(
            r#"UPDATE admin_passkey_reset_tokens
               SET consumed_at = ?
               WHERE token_hash = ? AND scope_id = ? AND consumed_at IS NULL AND expires_at >= ?"#,
        )
        .bind(now)
        .bind(token_hash)
        .bind(&scope.id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        if consumed.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Ok(false);
        }

        sqlx::query(
            r#"INSERT INTO admin_passkey_credentials
               (credential_id, scope_id, passkey_json, label, created_at, updated_at, last_used_at, revoked_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL, NULL)
               ON CONFLICT(scope_id, credential_id) DO UPDATE SET
                   passkey_json = excluded.passkey_json,
                   label = excluded.label,
                   updated_at = excluded.updated_at,
                   revoked_at = NULL"#,
        )
        .bind(credential_id)
        .bind(&scope.id)
        .bind(passkey_json)
        .bind(label.map(str::trim).filter(|value| !value.is_empty()))
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        for old_credential_id in revoke_credential_ids {
            if old_credential_id == credential_id {
                continue;
            }
            sqlx::query(
                r#"UPDATE admin_passkey_credentials
                   SET revoked_at = ?, updated_at = ?
                   WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL"#,
            )
            .bind(now)
            .bind(now)
            .bind(old_credential_id)
            .bind(&scope.id)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"UPDATE admin_passkey_sessions
                   SET revoked_at = ?
                   WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL"#,
            )
            .bind(now)
            .bind(old_credential_id)
            .bind(&scope.id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(true)
    }

    pub async fn list_active_admin_passkey_credentials(
        &self,
        scope: &AdminPasskeyScope,
    ) -> Result<Vec<AdminPasskeyCredentialRecord>, ProxyError> {
        let rows = sqlx::query_as::<
            _,
            (
                String,
                String,
                Option<String>,
                i64,
                i64,
                Option<i64>,
                Option<i64>,
            ),
        >(
            r#"SELECT credential_id, passkey_json, label, created_at, updated_at, last_used_at, revoked_at
               FROM admin_passkey_credentials
               WHERE scope_id = ? AND revoked_at IS NULL
               ORDER BY created_at ASC"#,
        )
        .bind(&scope.id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    credential_id,
                    passkey_json,
                    label,
                    created_at,
                    updated_at,
                    last_used_at,
                    revoked_at,
                )| AdminPasskeyCredentialRecord {
                    credential_id,
                    passkey_json,
                    label,
                    created_at,
                    updated_at,
                    last_used_at,
                    revoked_at,
                },
            )
            .collect())
    }

    pub async fn upsert_admin_passkey_credential(
        &self,
        scope: &AdminPasskeyScope,
        credential_id: &str,
        passkey_json: &str,
        label: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"INSERT INTO admin_passkey_credentials
               (credential_id, scope_id, passkey_json, label, created_at, updated_at, last_used_at, revoked_at)
               VALUES (?, ?, ?, ?, ?, ?, NULL, NULL)
               ON CONFLICT(scope_id, credential_id) DO UPDATE SET
                   passkey_json = excluded.passkey_json,
                   label = excluded.label,
                   updated_at = excluded.updated_at,
                   revoked_at = NULL"#,
        )
        .bind(credential_id)
        .bind(&scope.id)
        .bind(passkey_json)
        .bind(label.map(str::trim).filter(|value| !value.is_empty()))
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_admin_passkey_credential_after_auth(
        &self,
        scope: &AdminPasskeyScope,
        credential_id: &str,
        passkey_json: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"UPDATE admin_passkey_credentials
               SET passkey_json = ?, updated_at = ?, last_used_at = ?
               WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL"#,
        )
        .bind(passkey_json)
        .bind(now)
        .bind(now)
        .bind(credential_id)
        .bind(&scope.id)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn update_admin_passkey_credential_label(
        &self,
        scope: &AdminPasskeyScope,
        credential_id: &str,
        label: Option<&str>,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"UPDATE admin_passkey_credentials
               SET label = ?, updated_at = ?
               WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL"#,
        )
        .bind(label.map(str::trim).filter(|value| !value.is_empty()))
        .bind(now)
        .bind(credential_id)
        .bind(&scope.id)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn revoke_admin_passkey_credential_preserving_login(
        &self,
        scope: &AdminPasskeyScope,
        credential_id: &str,
        external_admin_login_available: bool,
        runtime_password_available: bool,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut conn = ImmediateSqliteTransaction::begin(self.pool.acquire().await?).await?;

        let result: Result<bool, ProxyError> = async {
            let target_active: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM admin_passkey_credentials WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL",
            )
            .bind(credential_id)
            .bind(&scope.id)
            .fetch_one(&mut *conn)
            .await?;
            if target_active == 0 {
                return Ok(false);
            }

            if !external_admin_login_available {
                let other_active_passkey_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM admin_passkey_credentials WHERE credential_id != ? AND scope_id = ? AND revoked_at IS NULL",
                )
                .bind(credential_id)
                .bind(&scope.id)
                .fetch_one(&mut *conn)
                .await?;
                let password_row = sqlx::query_as::<_, (Option<String>, Option<i64>)>(
                    r#"SELECT password_hash, disabled_at
                       FROM admin_password_settings
                       WHERE id = 1
                       LIMIT 1"#,
                )
                .fetch_optional(&mut *conn)
                .await?;
                let password_disabled = password_row
                    .as_ref()
                    .and_then(|(_, disabled_at)| *disabled_at)
                    .is_some();
                let persisted_password_available = runtime_password_available
                    && password_row
                    .as_ref()
                    .and_then(|(password_hash, _)| password_hash.as_deref())
                    .is_some_and(|password_hash| !password_hash.trim().is_empty())
                    && !password_disabled;
                let runtime_password_available = runtime_password_available && !password_disabled;

                if other_active_passkey_count == 0
                    && !persisted_password_available
                    && !runtime_password_available
                {
                    return Err(ProxyError::LastAdminLoginMethod);
                }
            }

            let updated = sqlx::query(
                r#"UPDATE admin_passkey_credentials
                   SET revoked_at = ?, updated_at = ?
                   WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL"#,
            )
            .bind(now)
            .bind(now)
            .bind(credential_id)
            .bind(&scope.id)
            .execute(&mut *conn)
            .await?;
            Ok(updated.rows_affected() > 0)
        }
        .await;

        match result {
            Ok(revoked) => {
                conn.commit().await?;
                Ok(revoked)
            }
            Err(err) => {
                let _ = conn.rollback().await;
                Err(err)
            }
        }
    }

    pub async fn insert_admin_passkey_challenge(
        &self,
        scope: &AdminPasskeyScope,
        kind: AdminPasskeyChallengeKind,
        reset_token: Option<&str>,
        state_json: &str,
        ttl_secs: i64,
    ) -> Result<AdminPasskeyChallengeRecord, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM admin_passkey_challenges WHERE scope_id = ? AND (expires_at < ? OR consumed_at IS NOT NULL)",
        )
        .bind(&scope.id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let id = random_string(ALPHABET, 32);
            let res = sqlx::query(
                r#"INSERT INTO admin_passkey_challenges
                   (id, scope_id, kind, reset_token, state_json, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&id)
            .bind(&scope.id)
            .bind(kind.as_str())
            .bind(reset_token)
            .bind(state_json)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    return Ok(AdminPasskeyChallengeRecord {
                        id,
                        kind,
                        reset_token: reset_token.map(str::to_string),
                        state_json: state_json.to_string(),
                        created_at: now,
                        expires_at,
                        consumed_at: None,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub async fn consume_admin_passkey_challenge(
        &self,
        scope: &AdminPasskeyScope,
        id: &str,
        kind: AdminPasskeyChallengeKind,
    ) -> Result<Option<AdminPasskeyChallengeRecord>, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM admin_passkey_challenges WHERE scope_id = ? AND (expires_at < ? OR consumed_at IS NOT NULL)",
        )
        .bind(&scope.id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query_as::<
            _,
            (String, String, Option<String>, String, i64, i64, Option<i64>),
        >(
            r#"SELECT id, kind, reset_token, state_json, created_at, expires_at, consumed_at
               FROM admin_passkey_challenges
               WHERE id = ? AND scope_id = ? AND kind = ? AND consumed_at IS NULL AND expires_at >= ?
               LIMIT 1"#,
        )
        .bind(id)
        .bind(&scope.id)
        .bind(kind.as_str())
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((id, kind_raw, reset_token, state_json, created_at, expires_at, consumed_at)) =
            row
        else {
            tx.rollback().await.ok();
            return Ok(None);
        };

        let updated = sqlx::query(
            r#"UPDATE admin_passkey_challenges
               SET consumed_at = ?
               WHERE id = ? AND scope_id = ? AND consumed_at IS NULL"#,
        )
        .bind(now)
        .bind(&id)
        .bind(&scope.id)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Ok(None);
        }

        tx.commit().await?;
        Ok(Some(AdminPasskeyChallengeRecord {
            id,
            kind: AdminPasskeyChallengeKind::parse(&kind_raw).unwrap_or(kind),
            reset_token,
            state_json,
            created_at,
            expires_at,
            consumed_at,
        }))
    }

    pub async fn create_admin_passkey_session(
        &self,
        scope: &AdminPasskeyScope,
        credential_id: Option<&str>,
        ttl_secs: i64,
    ) -> Result<AdminPasskeySessionRecord, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM admin_passkey_sessions WHERE scope_id = ? AND (expires_at < ? OR revoked_at IS NOT NULL)",
        )
        .bind(&scope.id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let res = sqlx::query(
                r#"INSERT INTO admin_passkey_sessions
                   (token, scope_id, credential_id, created_at, expires_at, revoked_at)
                   VALUES (?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&token)
            .bind(&scope.id)
            .bind(credential_id)
            .bind(now)
            .bind(expires_at)
            .execute(&self.pool)
            .await;

            match res {
                Ok(_) => {
                    return Ok(AdminPasskeySessionRecord {
                        token,
                        credential_id: credential_id.map(str::to_string),
                        created_at: now,
                        expires_at,
                        revoked_at: None,
                    });
                }
                Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => continue,
                Err(err) => return Err(ProxyError::Database(err)),
            }
        }
    }

    pub async fn get_active_admin_passkey_session(
        &self,
        scope: &AdminPasskeyScope,
        token: &str,
    ) -> Result<Option<AdminPasskeySessionRecord>, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut connection = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::AdminRead)
            .await?;
        let row_result = sqlx::query_as::<_, (String, Option<String>, i64, i64, Option<i64>)>(
            r#"SELECT s.token, s.credential_id, s.created_at, s.expires_at, s.revoked_at
               FROM admin_passkey_sessions s
               LEFT JOIN admin_passkey_credentials c
                 ON s.credential_id = c.credential_id AND c.scope_id = s.scope_id
               WHERE s.token = ?
                 AND s.scope_id = ?
                 AND s.revoked_at IS NULL
                 AND s.expires_at >= ?
                 AND (s.credential_id IS NULL OR c.revoked_at IS NULL)
               LIMIT 1"#,
        )
        .bind(token)
        .bind(&scope.id)
        .bind(now)
        .fetch_optional(&mut *connection)
        .await;
        let row = connection.complete_query(row_result).await?;

        Ok(row.map(
            |(token, credential_id, created_at, expires_at, revoked_at)| {
                AdminPasskeySessionRecord {
                    token,
                    credential_id,
                    created_at,
                    expires_at,
                    revoked_at,
                }
            },
        ))
    }

    #[cfg(debug_assertions)]
    pub(crate) async fn hold_sqlite_pool_until_for_test(
        &self,
        ready: tokio::sync::oneshot::Sender<()>,
        release: std::sync::Arc<tokio::sync::Notify>,
    ) -> Result<(), ProxyError> {
        let mut connections = Vec::with_capacity(crate::SQLITE_POOL_MAX_CONNECTIONS_DEFAULT as usize);
        for _ in 0..crate::SQLITE_POOL_MAX_CONNECTIONS_DEFAULT {
            connections.push(self.pool.acquire().await?);
        }
        let _ = ready.send(());
        release.notified().await;
        drop(connections);
        Ok(())
    }

    pub async fn revoke_admin_passkey_session(
        &self,
        scope: &AdminPasskeyScope,
        token: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            "UPDATE admin_passkey_sessions SET revoked_at = ? WHERE token = ? AND scope_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(token)
        .bind(&scope.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn revoke_admin_passkey_sessions_for_credential(
        &self,
        scope: &AdminPasskeyScope,
        credential_id: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"UPDATE admin_passkey_sessions
               SET revoked_at = ?
               WHERE credential_id = ? AND scope_id = ? AND revoked_at IS NULL"#,
        )
        .bind(now)
        .bind(credential_id)
        .bind(&scope.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn revoke_all_admin_passkey_sessions(
        &self,
        scope: &AdminPasskeyScope,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            "UPDATE admin_passkey_sessions SET revoked_at = ? WHERE scope_id = ? AND revoked_at IS NULL",
        )
            .bind(now)
            .bind(&scope.id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod admin_passkey_store_tests {
    use super::*;
    use std::time::Duration;

    fn temp_db_path(name: &str) -> (tempfile::TempDir, String) {
        let temp = tempfile::tempdir().expect("temp dir");
        let db_path = temp.path().join(name).to_string_lossy().to_string();
        (temp, db_path)
    }

    fn test_scope() -> AdminPasskeyScope {
        AdminPasskeyScope::new("node-test", "admin.test.example", "https://admin.test.example")
            .expect("test passkey scope")
    }

    #[tokio::test]
    async fn challenge_is_kind_scoped_one_time_and_expires() {
        let (_temp, db_path) = temp_db_path("challenge.db");
        let (backend_time, manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");

        let challenge = store
            .insert_admin_passkey_challenge(&test_scope(),
                AdminPasskeyChallengeKind::Registration,
                Some("reset-token"),
                r#"{"state":true}"#,
                120,
            )
            .await
            .expect("insert challenge");

        assert!(
            store
                .consume_admin_passkey_challenge(&test_scope(),
                    &challenge.id,
                    AdminPasskeyChallengeKind::Authentication
                )
                .await
                .expect("wrong kind lookup")
                .is_none()
        );

        let consumed = store
            .consume_admin_passkey_challenge(&test_scope(), &challenge.id, AdminPasskeyChallengeKind::Registration)
            .await
            .expect("consume challenge")
            .expect("challenge exists");
        assert_eq!(consumed.reset_token.as_deref(), Some("reset-token"));
        assert_eq!(consumed.state_json, r#"{"state":true}"#);

        assert!(
            store
                .consume_admin_passkey_challenge(&test_scope(),
                    &challenge.id,
                    AdminPasskeyChallengeKind::Registration
                )
                .await
                .expect("second consume")
                .is_none()
        );

        let expiring = store
            .insert_admin_passkey_challenge(&test_scope(),
                AdminPasskeyChallengeKind::Authentication,
                None,
                r#"{"state":"expired"}"#,
                60,
            )
            .await
            .expect("insert expiring challenge");
        manual_time.advance_wall(Duration::from_secs(61));

        assert!(
            store
                .consume_admin_passkey_challenge(&test_scope(),
                    &expiring.id,
                    AdminPasskeyChallengeKind::Authentication
                )
                .await
                .expect("consume expired challenge")
                .is_none()
        );
    }

    #[tokio::test]
    async fn reset_token_is_hash_backed_one_time_and_expires() {
        let (_temp, db_path) = temp_db_path("reset-token.db");
        let (backend_time, manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");

        let reset = store
            .create_admin_passkey_reset_token(&test_scope(), 120)
            .await
            .expect("create reset token");
        let token = reset.token.as_deref().expect("raw token returned once");
        assert_ne!(token, reset.token_hash);
        assert!(
            store
                .get_active_admin_passkey_reset_token(&test_scope(), "wrong-token")
                .await
                .expect("wrong token lookup")
                .is_none()
        );
        let active = store
            .get_active_admin_passkey_reset_token(&test_scope(), token)
            .await
            .expect("active token lookup")
            .expect("active token exists");
        assert_eq!(active.token, None);
        assert_eq!(active.token_hash, reset.token_hash);
        assert!(
            store
                .consume_admin_passkey_reset_token_hash(&test_scope(), &active.token_hash)
                .await
                .expect("consume reset token")
        );
        assert!(
            !store
                .consume_admin_passkey_reset_token_hash(&test_scope(), &active.token_hash)
                .await
                .expect("second consume reset token")
        );

        let expiring = store
            .create_admin_passkey_reset_token(&test_scope(), 60)
            .await
            .expect("create expiring reset token");
        let expiring_token = expiring.token.as_deref().expect("expiring raw token");
        manual_time.advance_wall(Duration::from_secs(61));
        assert!(
            store
                .get_active_admin_passkey_reset_token(&test_scope(), expiring_token)
                .await
                .expect("expired token lookup")
                .is_none()
        );
    }

    #[tokio::test]
    async fn cli_reset_token_helper_uses_store_without_proxy_startup() {
        let (_temp, db_path) = temp_db_path("cli-reset-token.db");
        let reset = crate::create_admin_passkey_reset_token_for_database(&db_path, &test_scope(), 120)
            .await
            .expect("create reset token");
        let token = reset.token.as_deref().expect("raw token returned once");

        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("reopen store");
        let active = store
            .get_active_admin_passkey_reset_token(&test_scope(), token)
            .await
            .expect("active token lookup")
            .expect("active token exists");

        assert_eq!(active.token, None);
        assert_eq!(active.token_hash, reset.token_hash);
    }

    #[tokio::test]
    async fn reset_registration_consumes_token_and_rotates_credentials_atomically() {
        let (_temp, db_path) = temp_db_path("reset-registration.db");
        let (backend_time, _manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential(&test_scope(), "old-credential", r#"{"credential":"old"}"#, None)
            .await
            .expect("insert old credential");
        let old_session = store
            .create_admin_passkey_session(&test_scope(), Some("old-credential"), 120)
            .await
            .expect("create old session");
        let reset = store
            .create_admin_passkey_reset_token(&test_scope(), 120)
            .await
            .expect("create reset token");
        let token = reset.token.as_deref().expect("raw token returned");
        let active = store
            .get_active_admin_passkey_reset_token(&test_scope(), token)
            .await
            .expect("active reset token")
            .expect("reset token exists");

        assert!(
            store
                .complete_admin_passkey_reset_registration(&test_scope(),
                    &active.token_hash,
                    "new-credential",
                    r#"{"credential":"new"}"#,
                    Some("New passkey"),
                    &["old-credential".to_string()],
                )
                .await
                .expect("complete reset registration")
        );

        assert!(
            store
                .get_active_admin_passkey_reset_token(&test_scope(), token)
                .await
                .expect("consumed reset token")
                .is_none()
        );
        assert!(
            !store
                .complete_admin_passkey_reset_registration(&test_scope(),
                    &active.token_hash,
                    "another-credential",
                    r#"{"credential":"another"}"#,
                    None,
                    &[],
                )
                .await
                .expect("second reset registration attempt")
        );
        let credentials = store
            .list_active_admin_passkey_credentials(&test_scope())
            .await
            .expect("list credentials");
        assert_eq!(credentials.len(), 1);
        assert_eq!(credentials[0].credential_id, "new-credential");
        assert_eq!(credentials[0].label.as_deref(), Some("New passkey"));
        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &old_session.token)
                .await
                .expect("old session lookup")
                .is_none()
        );
    }

    #[tokio::test]
    async fn admin_passkey_totp_flag_does_not_disable_unpersisted_password() {
        let (_temp, db_path) = temp_db_path("totp-settings.db");
        let (backend_time, _manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");

        let settings = store
            .set_admin_login_totp_required(true, Some(&test_scope()))
            .await
            .expect("set totp flag");

        assert_eq!(settings.password_hash, None);
        assert_eq!(settings.disabled_at, None);
        assert!(settings.login_totp_required);
    }

    #[tokio::test]
    async fn passkey_session_must_be_active_and_can_be_revoked() {
        let (_temp, db_path) = temp_db_path("session.db");
        let (backend_time, manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert credential");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-2", r#"{"credential":2}"#, None)
            .await
            .expect("insert expiring credential");

        let session = store
            .create_admin_passkey_session(&test_scope(), Some("credential-1"), 120)
            .await
            .expect("create session");
        assert_eq!(session.credential_id.as_deref(), Some("credential-1"));
        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &session.token)
                .await
                .expect("active session")
                .is_some()
        );
        assert!(
            store
                .revoke_admin_passkey_credential_preserving_login(&test_scope(), "credential-1", true, true)
                .await
                .expect("revoke credential")
        );
        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &session.token)
                .await
                .expect("session with revoked credential")
                .is_none()
        );
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("restore credential");

        store
            .revoke_admin_passkey_session(&test_scope(), &session.token)
            .await
            .expect("revoke session");
        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &session.token)
                .await
                .expect("revoked session")
                .is_none()
        );

        let expiring = store
            .create_admin_passkey_session(&test_scope(), Some("credential-2"), 60)
            .await
            .expect("create expiring session");
        manual_time.advance_wall(Duration::from_secs(61));

        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &expiring.token)
                .await
                .expect("expired session")
                .is_none()
        );
    }

    #[tokio::test]
    async fn active_passkey_session_lookup_uses_the_bounded_admin_read_connection() {
        let (_temp, db_path) = temp_db_path("session-bounded-admin-read.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        let session = store
            .create_admin_passkey_session(&test_scope(), None, 120)
            .await
            .expect("create passkey session");

        let first = store.pool.acquire().await.expect("hold first connection");
        let second = store.pool.acquire().await.expect("hold second connection");
        let third = store.pool.acquire().await.expect("hold third connection");
        let started = std::time::Instant::now();
        let error = store
            .get_active_admin_passkey_session(&test_scope(), &session.token)
            .await
            .expect_err("pool exhaustion must bound passkey session lookup");
        assert!(matches!(
            error,
            ProxyError::Database(sqlx::Error::PoolTimedOut)
        ));
        assert!(started.elapsed() < Duration::from_millis(150));
        drop((third, second, first));

        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &session.token)
                .await
                .expect("connection must remain reusable after bounded lookup")
                .is_some()
        );
    }

    #[tokio::test]
    async fn revoke_all_admin_passkey_sessions_expires_all_active_sessions() {
        let (_temp, db_path) = temp_db_path("revoke-all-passkey-sessions.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert first credential");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-2", r#"{"credential":2}"#, None)
            .await
            .expect("insert second credential");
        let first = store
            .create_admin_passkey_session(&test_scope(), Some("credential-1"), 120)
            .await
            .expect("create first session");
        let second = store
            .create_admin_passkey_session(&test_scope(), Some("credential-2"), 120)
            .await
            .expect("create second session");

        store
            .revoke_all_admin_passkey_sessions(&test_scope())
            .await
            .expect("revoke all sessions");

        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &first.token)
                .await
                .expect("lookup first session")
                .is_none()
        );
        assert!(
            store
                .get_active_admin_passkey_session(&test_scope(), &second.token)
                .await
                .expect("lookup second session")
                .is_none()
        );
    }

    #[tokio::test]
    async fn preserving_login_rejects_password_then_final_passkey_removal() {
        let (_temp, db_path) = temp_db_path("preserve-password-passkey.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert credential");

        store
            .disable_admin_password_preserving_login(false, true, Some(&test_scope()))
            .await
            .expect("passkey keeps admin login available");
        let err = store
            .revoke_admin_passkey_credential_preserving_login(&test_scope(), "credential-1", false, true)
            .await
            .expect_err("cannot revoke final passkey after password disable");
        assert!(matches!(err, ProxyError::LastAdminLoginMethod));
    }

    #[tokio::test]
    async fn password_removal_ignores_passkey_rows_when_passkey_runtime_is_disabled() {
        let (_temp, db_path) = temp_db_path("disabled-runtime-passkey.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert stale passkey credential");

        let err = store
            .disable_admin_password_preserving_login(false, false, Some(&test_scope()))
            .await
            .expect_err("disabled runtime passkey cannot preserve login");

        assert!(matches!(err, ProxyError::LastAdminLoginMethod));
    }

    #[tokio::test]
    async fn passkey_removal_ignores_persisted_password_when_password_runtime_is_disabled() {
        let (_temp, db_path) = temp_db_path("disabled-runtime-password.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        store
            .set_admin_password_hash("stored-password-hash")
            .await
            .expect("seed stale persisted password");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert credential");

        let err = store
            .revoke_admin_passkey_credential_preserving_login(&test_scope(), "credential-1", false, false)
            .await
            .expect_err("disabled runtime password cannot preserve login");

        assert!(matches!(err, ProxyError::LastAdminLoginMethod));
        let credentials = store
            .list_active_admin_passkey_credentials(&test_scope())
            .await
            .expect("list credentials");
        assert_eq!(credentials.len(), 1);
    }

    #[tokio::test]
    async fn preserving_login_rejects_second_passkey_removal_without_password() {
        let (_temp, db_path) = temp_db_path("preserve-two-passkeys.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert first credential");
        store
            .upsert_admin_passkey_credential(&test_scope(), "credential-2", r#"{"credential":2}"#, None)
            .await
            .expect("insert second credential");

        assert!(
            store
                .revoke_admin_passkey_credential_preserving_login(&test_scope(), "credential-1", false, false)
                .await
                .expect("first passkey revoke keeps second")
        );
        let err = store
            .revoke_admin_passkey_credential_preserving_login(&test_scope(), "credential-2", false, false)
            .await
            .expect_err("cannot revoke final passkey");
        assert!(matches!(err, ProxyError::LastAdminLoginMethod));
    }

    #[tokio::test]
    async fn scope_keeps_credentials_and_ephemeral_state_isolated_and_restorable() {
        let (_temp, db_path) = temp_db_path("scope-isolation.db");
        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("create store");
        let scope_a = AdminPasskeyScope::new("node-a", "admin.example", "https://admin.example")
            .expect("scope a");
        let scope_b = AdminPasskeyScope::new("node-b", "admin.example", "https://admin.example")
            .expect("scope b");
        store.ensure_admin_passkey_scope(&scope_a).await.expect("store scope a");
        store.ensure_admin_passkey_scope(&scope_b).await.expect("store scope b");
        store
            .upsert_admin_passkey_credential(&scope_a, "credential-a", r#"{"credential":"a"}"#, None)
            .await
            .expect("insert scoped credential");
        store
            .upsert_admin_passkey_credential(&scope_b, "credential-a", r#"{"credential":"b"}"#, None)
            .await
            .expect("same credential id can be stored in another scope");
        sqlx::query(
            r#"INSERT INTO admin_passkey_credentials
               (credential_id, passkey_json, created_at, updated_at)
               VALUES ('legacy-credential', '{"credential":"legacy"}', 1, 1)"#,
        )
        .execute(&store.pool)
        .await
        .expect("insert legacy credential");

        let reset = store
            .create_admin_passkey_reset_token(&scope_a, 120)
            .await
            .expect("create scoped reset token");
        let challenge = store
            .insert_admin_passkey_challenge(
                &scope_a,
                AdminPasskeyChallengeKind::Authentication,
                None,
                r#"{"state":"a"}"#,
                120,
            )
            .await
            .expect("create scoped challenge");
        let session = store
            .create_admin_passkey_session(&scope_a, Some("credential-a"), 120)
            .await
            .expect("create scoped session");

        assert_eq!(
            store
                .list_active_admin_passkey_credentials(&scope_b)
                .await
                .expect("list scope b")[0]
                .passkey_json,
            r#"{"credential":"b"}"#
        );
        assert!(store
            .get_active_admin_passkey_reset_token(
                &scope_b,
                reset.token.as_deref().expect("reset token"),
            )
            .await
            .expect("read reset from scope b")
            .is_none());
        assert!(store
            .consume_admin_passkey_challenge(
                &scope_b,
                &challenge.id,
                AdminPasskeyChallengeKind::Authentication,
            )
            .await
            .expect("consume challenge from scope b")
            .is_none());
        assert!(store
            .get_active_admin_passkey_session(&scope_b, &session.token)
            .await
            .expect("read session from scope b")
            .is_none());

        assert_eq!(store
            .list_active_admin_passkey_credentials(&scope_a)
            .await
            .expect("restore scope a")
            .len(), 1);
        assert_eq!(
            store
                .list_active_admin_passkey_credentials(&scope_a)
                .await
                .expect("read scope a credential")[0]
                .passkey_json,
            r#"{"credential":"a"}"#
        );
        let status = store.admin_passkey_scope_status(&scope_b).await.expect("scope status");
        assert_eq!(status.inactive_credential_count, 1);
        assert_eq!(status.legacy_credential_count, 1);
    }

    #[tokio::test]
    async fn bootstrap_migrates_global_passkeys_to_non_authenticating_legacy_scope() {
        let (_temp, db_path) = temp_db_path("legacy-passkeys.db");
        let old_pool = sqlx::SqlitePool::connect(&format!("sqlite://{db_path}?mode=rwc"))
            .await
            .expect("open legacy database");
        sqlx::query(
            r#"
            CREATE TABLE admin_passkey_credentials (
                credential_id TEXT PRIMARY KEY,
                passkey_json TEXT NOT NULL,
                label TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                last_used_at INTEGER,
                revoked_at INTEGER
            )
            "#,
        )
        .execute(&old_pool)
        .await
        .expect("create legacy credentials");
        sqlx::query(
            r#"
            CREATE TABLE admin_passkey_sessions (
                token TEXT PRIMARY KEY,
                credential_id TEXT,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                revoked_at INTEGER,
                FOREIGN KEY (credential_id) REFERENCES admin_passkey_credentials(credential_id)
            )
            "#,
        )
        .execute(&old_pool)
        .await
        .expect("create legacy sessions");
        sqlx::query(
            r#"INSERT INTO admin_passkey_credentials
               (credential_id, passkey_json, created_at, updated_at)
               VALUES ('legacy-credential', '{"credential":"legacy"}', 1, 1)"#,
        )
        .execute(&old_pool)
        .await
        .expect("insert legacy credential");
        old_pool.close().await;

        let store = KeyStore::new_with_time(&db_path, BackendTime::system())
            .await
            .expect("migrate legacy database");
        let scope = test_scope();
        let scope_id: String = sqlx::query_scalar(
            "SELECT scope_id FROM admin_passkey_credentials WHERE credential_id = 'legacy-credential'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("read migrated scope");
        assert_eq!(scope_id, LEGACY_ADMIN_PASSKEY_SCOPE_ID);
        assert!(store
            .list_active_admin_passkey_credentials(&scope)
            .await
            .expect("list current scope")
            .is_empty());
        let primary_key_columns: Vec<(String, i64)> = sqlx::query_as(
            "SELECT name, pk FROM pragma_table_info('admin_passkey_credentials') WHERE pk > 0 ORDER BY pk",
        )
        .fetch_all(&store.pool)
        .await
        .expect("read primary key columns");
        assert_eq!(
            primary_key_columns,
            vec![("scope_id".to_string(), 1), ("credential_id".to_string(), 2)]
        );
    }
}

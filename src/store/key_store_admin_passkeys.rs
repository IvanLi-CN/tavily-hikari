use sha2::{Digest as _, Sha256};

fn admin_passkey_reset_token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.trim().as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl KeyStore {
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

    pub async fn disable_admin_password(
        &self,
    ) -> Result<AdminPasswordSettingsRecord, ProxyError> {
        let now = self.backend_time.now_ts();
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
        .execute(&self.pool)
        .await?;
        let login_totp_required = self
            .get_admin_password_settings()
            .await?
            .map(|settings| settings.login_totp_required)
            .unwrap_or(false);
        Ok(AdminPasswordSettingsRecord {
            password_hash: None,
            disabled_at: Some(now),
            updated_at: now,
            login_totp_required,
        })
    }

    pub async fn set_admin_login_totp_required(
        &self,
        required: bool,
    ) -> Result<AdminPasswordSettingsRecord, ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"INSERT INTO admin_password_settings (
                   id, password_hash, disabled_at, updated_at, login_totp_required
               )
               VALUES (1, NULL, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                   login_totp_required = excluded.login_totp_required,
                   updated_at = excluded.updated_at"#,
        )
        .bind(now)
        .bind(now)
        .bind(if required { 1_i64 } else { 0_i64 })
        .execute(&self.pool)
        .await?;
        self.get_admin_password_settings()
            .await?
            .ok_or_else(|| ProxyError::Other("admin password settings missing".to_string()))
    }

    pub async fn admin_passkey_enabled(&self) -> Result<bool, ProxyError> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM admin_passkey_credentials WHERE revoked_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub async fn create_admin_passkey_reset_token(
        &self,
        ttl_secs: i64,
    ) -> Result<AdminPasskeyResetTokenRecord, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM admin_passkey_reset_tokens WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let token_hash = admin_passkey_reset_token_hash(&token);
            let res = sqlx::query(
                r#"INSERT INTO admin_passkey_reset_tokens
                   (token_hash, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, NULL)"#,
            )
            .bind(&token_hash)
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
        token: &str,
    ) -> Result<Option<AdminPasskeyResetTokenRecord>, ProxyError> {
        let now = self.backend_time.now_ts();
        let token_hash = admin_passkey_reset_token_hash(token);
        let row = sqlx::query_as::<_, (String, i64, i64, Option<i64>)>(
            r#"SELECT token_hash, created_at, expires_at, consumed_at
               FROM admin_passkey_reset_tokens
               WHERE token_hash = ? AND consumed_at IS NULL AND expires_at >= ?
               LIMIT 1"#,
        )
        .bind(&token_hash)
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
        token_hash: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            "DELETE FROM admin_passkey_reset_tokens WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        let updated = sqlx::query(
            r#"UPDATE admin_passkey_reset_tokens
               SET consumed_at = ?
               WHERE token_hash = ? AND consumed_at IS NULL AND expires_at >= ?"#,
        )
        .bind(now)
        .bind(token_hash)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn list_active_admin_passkey_credentials(
        &self,
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
               WHERE revoked_at IS NULL
               ORDER BY created_at ASC"#,
        )
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
        credential_id: &str,
        passkey_json: &str,
        label: Option<&str>,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"INSERT INTO admin_passkey_credentials
               (credential_id, passkey_json, label, created_at, updated_at, last_used_at, revoked_at)
               VALUES (?, ?, ?, ?, ?, NULL, NULL)
               ON CONFLICT(credential_id) DO UPDATE SET
                   passkey_json = excluded.passkey_json,
                   label = excluded.label,
                   updated_at = excluded.updated_at,
                   revoked_at = NULL"#,
        )
        .bind(credential_id)
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
        credential_id: &str,
        passkey_json: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"UPDATE admin_passkey_credentials
               SET passkey_json = ?, updated_at = ?, last_used_at = ?
               WHERE credential_id = ? AND revoked_at IS NULL"#,
        )
        .bind(passkey_json)
        .bind(now)
        .bind(now)
        .bind(credential_id)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn update_admin_passkey_credential_label(
        &self,
        credential_id: &str,
        label: Option<&str>,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"UPDATE admin_passkey_credentials
               SET label = ?, updated_at = ?
               WHERE credential_id = ? AND revoked_at IS NULL"#,
        )
        .bind(label.map(str::trim).filter(|value| !value.is_empty()))
        .bind(now)
        .bind(credential_id)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn revoke_admin_passkey_credential(
        &self,
        credential_id: &str,
    ) -> Result<bool, ProxyError> {
        let now = self.backend_time.now_ts();
        let updated = sqlx::query(
            r#"UPDATE admin_passkey_credentials
               SET revoked_at = ?, updated_at = ?
               WHERE credential_id = ? AND revoked_at IS NULL"#,
        )
        .bind(now)
        .bind(now)
        .bind(credential_id)
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn insert_admin_passkey_challenge(
        &self,
        kind: AdminPasskeyChallengeKind,
        reset_token: Option<&str>,
        state_json: &str,
        ttl_secs: i64,
    ) -> Result<AdminPasskeyChallengeRecord, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM admin_passkey_challenges WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let id = random_string(ALPHABET, 32);
            let res = sqlx::query(
                r#"INSERT INTO admin_passkey_challenges
                   (id, kind, reset_token, state_json, created_at, expires_at, consumed_at)
                   VALUES (?, ?, ?, ?, ?, ?, NULL)"#,
            )
            .bind(&id)
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
        id: &str,
        kind: AdminPasskeyChallengeKind,
    ) -> Result<Option<AdminPasskeyChallengeRecord>, ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "DELETE FROM admin_passkey_challenges WHERE expires_at < ? OR consumed_at IS NOT NULL",
        )
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query_as::<
            _,
            (String, String, Option<String>, String, i64, i64, Option<i64>),
        >(
            r#"SELECT id, kind, reset_token, state_json, created_at, expires_at, consumed_at
               FROM admin_passkey_challenges
               WHERE id = ? AND kind = ? AND consumed_at IS NULL AND expires_at >= ?
               LIMIT 1"#,
        )
        .bind(id)
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
               WHERE id = ? AND consumed_at IS NULL"#,
        )
        .bind(now)
        .bind(&id)
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
        credential_id: Option<&str>,
        ttl_secs: i64,
    ) -> Result<AdminPasskeySessionRecord, ProxyError> {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let now = self.backend_time.now_ts();
        let expires_at = now + ttl_secs.max(60);

        sqlx::query(
            "DELETE FROM admin_passkey_sessions WHERE expires_at < ? OR revoked_at IS NOT NULL",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;

        loop {
            let token = random_string(ALPHABET, 48);
            let res = sqlx::query(
                r#"INSERT INTO admin_passkey_sessions
                   (token, credential_id, created_at, expires_at, revoked_at)
                   VALUES (?, ?, ?, ?, NULL)"#,
            )
            .bind(&token)
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
        token: &str,
    ) -> Result<Option<AdminPasskeySessionRecord>, ProxyError> {
        let now = self.backend_time.now_ts();
        let row = sqlx::query_as::<_, (String, Option<String>, i64, i64, Option<i64>)>(
            r#"SELECT s.token, s.credential_id, s.created_at, s.expires_at, s.revoked_at
               FROM admin_passkey_sessions s
               LEFT JOIN admin_passkey_credentials c
                 ON s.credential_id = c.credential_id
               WHERE s.token = ?
                 AND s.revoked_at IS NULL
                 AND s.expires_at >= ?
                 AND (s.credential_id IS NULL OR c.revoked_at IS NULL)
               LIMIT 1"#,
        )
        .bind(token)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;

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

    pub async fn revoke_admin_passkey_session(&self, token: &str) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            "UPDATE admin_passkey_sessions SET revoked_at = ? WHERE token = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(token)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn revoke_admin_passkey_sessions_for_credential(
        &self,
        credential_id: &str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        sqlx::query(
            r#"UPDATE admin_passkey_sessions
               SET revoked_at = ?
               WHERE credential_id = ? AND revoked_at IS NULL"#,
        )
        .bind(now)
        .bind(credential_id)
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

    #[tokio::test]
    async fn challenge_is_kind_scoped_one_time_and_expires() {
        let (_temp, db_path) = temp_db_path("challenge.db");
        let (backend_time, manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");

        let challenge = store
            .insert_admin_passkey_challenge(
                AdminPasskeyChallengeKind::Registration,
                Some("reset-token"),
                r#"{"state":true}"#,
                120,
            )
            .await
            .expect("insert challenge");

        assert!(
            store
                .consume_admin_passkey_challenge(
                    &challenge.id,
                    AdminPasskeyChallengeKind::Authentication
                )
                .await
                .expect("wrong kind lookup")
                .is_none()
        );

        let consumed = store
            .consume_admin_passkey_challenge(&challenge.id, AdminPasskeyChallengeKind::Registration)
            .await
            .expect("consume challenge")
            .expect("challenge exists");
        assert_eq!(consumed.reset_token.as_deref(), Some("reset-token"));
        assert_eq!(consumed.state_json, r#"{"state":true}"#);

        assert!(
            store
                .consume_admin_passkey_challenge(
                    &challenge.id,
                    AdminPasskeyChallengeKind::Registration
                )
                .await
                .expect("second consume")
                .is_none()
        );

        let expiring = store
            .insert_admin_passkey_challenge(
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
                .consume_admin_passkey_challenge(
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
            .create_admin_passkey_reset_token(120)
            .await
            .expect("create reset token");
        let token = reset.token.as_deref().expect("raw token returned once");
        assert_ne!(token, reset.token_hash);
        assert!(
            store
                .get_active_admin_passkey_reset_token("wrong-token")
                .await
                .expect("wrong token lookup")
                .is_none()
        );
        let active = store
            .get_active_admin_passkey_reset_token(token)
            .await
            .expect("active token lookup")
            .expect("active token exists");
        assert_eq!(active.token, None);
        assert_eq!(active.token_hash, reset.token_hash);
        assert!(
            store
                .consume_admin_passkey_reset_token_hash(&active.token_hash)
                .await
                .expect("consume reset token")
        );
        assert!(
            !store
                .consume_admin_passkey_reset_token_hash(&active.token_hash)
                .await
                .expect("second consume reset token")
        );

        let expiring = store
            .create_admin_passkey_reset_token(60)
            .await
            .expect("create expiring reset token");
        let expiring_token = expiring.token.as_deref().expect("expiring raw token");
        manual_time.advance_wall(Duration::from_secs(61));
        assert!(
            store
                .get_active_admin_passkey_reset_token(expiring_token)
                .await
                .expect("expired token lookup")
                .is_none()
        );
    }

    #[tokio::test]
    async fn passkey_session_must_be_active_and_can_be_revoked() {
        let (_temp, db_path) = temp_db_path("session.db");
        let (backend_time, manual_time) = BackendTime::manual_from_ts(1_700_000_000);
        let store = KeyStore::new_with_time(&db_path, backend_time)
            .await
            .expect("create store");
        store
            .upsert_admin_passkey_credential("credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("insert credential");
        store
            .upsert_admin_passkey_credential("credential-2", r#"{"credential":2}"#, None)
            .await
            .expect("insert expiring credential");

        let session = store
            .create_admin_passkey_session(Some("credential-1"), 120)
            .await
            .expect("create session");
        assert_eq!(session.credential_id.as_deref(), Some("credential-1"));
        assert!(
            store
                .get_active_admin_passkey_session(&session.token)
                .await
                .expect("active session")
                .is_some()
        );
        assert!(
            store
                .revoke_admin_passkey_credential("credential-1")
                .await
                .expect("revoke credential")
        );
        assert!(
            store
                .get_active_admin_passkey_session(&session.token)
                .await
                .expect("session with revoked credential")
                .is_none()
        );
        store
            .upsert_admin_passkey_credential("credential-1", r#"{"credential":1}"#, None)
            .await
            .expect("restore credential");

        store
            .revoke_admin_passkey_session(&session.token)
            .await
            .expect("revoke session");
        assert!(
            store
                .get_active_admin_passkey_session(&session.token)
                .await
                .expect("revoked session")
                .is_none()
        );

        let expiring = store
            .create_admin_passkey_session(Some("credential-2"), 60)
            .await
            .expect("create expiring session");
        manual_time.advance_wall(Duration::from_secs(61));

        assert!(
            store
                .get_active_admin_passkey_session(&expiring.token)
                .await
                .expect("expired session")
                .is_none()
        );
    }
}

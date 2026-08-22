#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconciliationMode {
    Compare,
    Active,
    ActivePaused,
}

impl ReconciliationMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Compare => "compare",
            Self::Active => "active",
            Self::ActivePaused => "active_paused",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "compare" => Some(Self::Compare),
            "active" => Some(Self::Active),
            "active_paused" => Some(Self::ActivePaused),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReconciliationControlState {
    pub(crate) mode: ReconciliationMode,
    pub(crate) activation_period_code: Option<String>,
    pub(crate) activation_period_start: Option<i64>,
    pub(crate) legacy_active: bool,
    pub(crate) paused_reason: Option<String>,
    pub(crate) transitioned_at: i64,
}

type ReconciliationControlStateRow = (String, Option<String>, Option<i64>, i64, Option<String>, i64);

impl KeyStore {
    pub(crate) async fn upstream_reconciliation_control_state(
        &self,
    ) -> Result<ReconciliationControlState, ProxyError> {
        let mut conn = self
            .sqlite_runtime
            .acquire_operation_connection(SqliteOperation::ScheduledJobControl)
            .await?;
        let result = sqlx::query_as::<_, ReconciliationControlStateRow>(
            r#"SELECT mode, activation_period_code, activation_period_start, legacy_active,
                      paused_reason, transitioned_at
                 FROM upstream_reconciliation_control_state
                WHERE id = 'local'"#,
        )
        .fetch_one(&mut *conn)
        .await;
        let result = conn.complete_query(result).await?;
        let (mode, activation_period_code, activation_period_start, legacy_active, paused_reason, transitioned_at) =
            result;
        let mode = ReconciliationMode::parse(&mode).ok_or_else(|| {
            ProxyError::Other("invalid persisted upstream reconciliation mode".to_string())
        })?;
        Ok(ReconciliationControlState {
            mode,
            activation_period_code,
            activation_period_start,
            legacy_active: legacy_active != 0,
            paused_reason,
            transitioned_at,
        })
    }

    pub(crate) async fn update_upstream_reconciliation_controller_for_switch(
        &self,
        previous_enabled: bool,
        enabled: bool,
    ) -> Result<(), ProxyError> {
        if previous_enabled == enabled {
            return Ok(());
        }

        let now = self.backend_time.now_ts();
        let mut tx = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
            let (mode, activation_period_code, activation_period_start, legacy_active, paused_reason, action): (
                ReconciliationMode,
                Option<String>,
                Option<i64>,
                i64,
                Option<String>,
                &str,
            ) = if enabled {
                    let current = crate::business_period_for_timestamp(now);
                    let next = crate::business_period_for_timestamp(current.ends_at.saturating_add(1));
                    (
                        ReconciliationMode::Active,
                        Some(next.code),
                        Some(next.starts_at),
                        0_i64,
                        None,
                        "active_requested",
                    )
                } else {
                    (
                        ReconciliationMode::Compare,
                        None,
                        None,
                        0_i64,
                        None,
                        "compare_requested",
                    )
                };
            sqlx::query(
                r#"INSERT INTO meta (key, value) VALUES (?, ?)
                   ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
            )
            .bind(META_KEY_UPSTREAM_PRECISE_RECONCILIATION_ENABLED_V1)
            .bind(i64::from(enabled).to_string())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"UPDATE upstream_reconciliation_control_state
                   SET mode = ?, activation_period_code = ?, activation_period_start = ?,
                       legacy_active = ?, paused_reason = ?, transitioned_at = ?
                 WHERE id = 'local'"#,
            )
            .bind(mode.as_str())
            .bind(&activation_period_code)
            .bind(activation_period_start)
            .bind(legacy_active)
            .bind(&paused_reason)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"INSERT INTO upstream_reconciliation_control_transitions
                    (mode, action, activation_period_code, transitioned_at, detail)
                   VALUES (?, ?, ?, ?, ?)"#,
            )
            .bind(mode.as_str())
            .bind(action)
            .bind(activation_period_code)
            .bind(now)
            .bind(paused_reason)
            .execute(&mut *tx)
            .await?;
            Ok::<(), ProxyError>(())
        }
        .await;
        tx.finish(result).await
    }

    pub(crate) async fn pause_upstream_reconciliation_for_integrity(
        &self,
        reason: &'static str,
    ) -> Result<(), ProxyError> {
        let now = self.backend_time.now_ts();
        let mut tx = self
            .sqlite_runtime
            .begin_scheduled_job_control()
            .await?;
        let result = async {
            sqlx::query(
                r#"INSERT INTO meta (key, value) VALUES (?, '0')
                   ON CONFLICT(key) DO UPDATE SET value = excluded.value"#,
            )
            .bind(META_KEY_UPSTREAM_PRECISE_RECONCILIATION_ENABLED_V1)
            .execute(&mut *tx)
            .await?;
            let updated = sqlx::query(
                r#"UPDATE upstream_reconciliation_control_state
                   SET mode = 'active_paused', paused_reason = ?, transitioned_at = ?
                 WHERE id = 'local' AND mode = 'active'"#,
            )
            .bind(reason)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            if updated.rows_affected() != 0 {
                sqlx::query(
                    r#"INSERT INTO upstream_reconciliation_control_transitions
                        (mode, action, activation_period_code, transitioned_at, detail)
                       VALUES ('active_paused', 'integrity_paused', NULL, ?, ?)"#,
                )
                .bind(now)
                .bind(reason)
                .execute(&mut *tx)
                .await?;
            }
            Ok::<(), ProxyError>(())
        }
        .await;
        tx.finish(result).await
    }

    pub(crate) async fn reconciliation_settlement_mode_for_period(
        &self,
        settings: &SystemSettings,
        period: &crate::BusinessPeriod,
    ) -> Result<Option<&'static str>, ProxyError> {
        if !upstream_reconciliation_shadow_ready(settings) {
            return Ok(None);
        }
        let state = self.upstream_reconciliation_control_state().await?;
        let actual = match state.mode {
            ReconciliationMode::Compare | ReconciliationMode::ActivePaused => false,
            ReconciliationMode::Active if state.legacy_active => {
                self.refresh_upstream_reconciliation_epoch().await?.0
            }
            ReconciliationMode::Active => match state.activation_period_start {
                Some(boundary) => period.starts_at >= boundary,
                None => {
                    self.pause_upstream_reconciliation_for_integrity(
                        "missing_activation_boundary",
                    )
                    .await?;
                    return Err(ProxyError::Other(
                        "active reconciliation controller is missing its activation boundary"
                            .to_string(),
                    ));
                }
            },
        };
        Ok(Some(if actual {
            RECONCILIATION_SETTLEMENT_MODE_ACTUAL
        } else {
            RECONCILIATION_SETTLEMENT_MODE_SHADOW
        }))
    }

    pub(crate) async fn reconciliation_controller_allows_representative(
        &self,
    ) -> Result<bool, ProxyError> {
        Ok(self.upstream_reconciliation_control_state().await?.mode
            != ReconciliationMode::ActivePaused)
    }
}

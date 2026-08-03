impl KeyStore {
    async fn ensure_ha_outbox_gc_state_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS ha_outbox_gc_state (
                id TEXT PRIMARY KEY CHECK (id = 'local'),
                next_channel TEXT NOT NULL DEFAULT 'control',
                pending_channel_mask INTEGER NOT NULL DEFAULT 7,
                updated_at INTEGER NOT NULL DEFAULT 0,
                low_pressure_since INTEGER,
                recovery_mode INTEGER NOT NULL DEFAULT 0,
                recovery_deadline_at INTEGER,
                last_foreground_rps INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        for (column, definition) in [
            ("last_legacy_control_seq", "INTEGER NOT NULL DEFAULT 0"),
            ("last_legacy_billing_seq", "INTEGER NOT NULL DEFAULT 0"),
            ("last_legacy_runtime_seq", "INTEGER NOT NULL DEFAULT 0"),
            ("low_pressure_since", "INTEGER"),
            ("recovery_mode", "INTEGER NOT NULL DEFAULT 0"),
            ("recovery_deadline_at", "INTEGER"),
            ("last_foreground_rps", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !self.table_column_exists("ha_outbox_gc_state", column).await? {
                sqlx::query(&format!(
                    "ALTER TABLE ha_outbox_gc_state ADD COLUMN {column} {definition}"
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query(
            r#"INSERT OR IGNORE INTO ha_outbox_gc_state
                (id, next_channel, pending_channel_mask, updated_at)
            VALUES ('local', 'control', 7, 0)"#,
        )
        .execute(&self.pool)
        .await?;
        self.ensure_ha_outbox_gc_channel_state().await
    }

    async fn ensure_ha_outbox_gc_channel_state(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS ha_outbox_gc_channel_state (
                channel TEXT PRIMARY KEY,
                last_attempt_at INTEGER,
                last_progress_at INTEGER,
                last_deleted_rows INTEGER NOT NULL DEFAULT 0,
                last_defer_reason TEXT,
                next_retry_at INTEGER,
                consecutive_no_progress INTEGER NOT NULL DEFAULT 0,
                batch_size INTEGER NOT NULL DEFAULT 250,
                last_observed_at INTEGER,
                last_high_watermark INTEGER NOT NULL DEFAULT 0,
                last_ingress_seq_delta INTEGER,
                last_net_rows_delta_estimate INTEGER,
                total_deleted_rows INTEGER NOT NULL DEFAULT 0,
                last_continuation_delay_secs INTEGER,
                debt_mode TEXT NOT NULL DEFAULT 'normal',
                oldest_deletable_age_secs INTEGER,
                deleted_rows_per_minute REAL NOT NULL DEFAULT 0,
                recovery_deadline_at INTEGER,
                slo_state TEXT NOT NULL DEFAULT 'unknown',
                foreground_rps INTEGER NOT NULL DEFAULT 0
            )"#,
        )
        .execute(&self.pool)
        .await?;
        for (column, definition) in [
            ("last_observed_at", "INTEGER"),
            ("last_high_watermark", "INTEGER NOT NULL DEFAULT 0"),
            ("last_ingress_seq_delta", "INTEGER"),
            ("last_net_rows_delta_estimate", "INTEGER"),
            ("total_deleted_rows", "INTEGER NOT NULL DEFAULT 0"),
            ("last_continuation_delay_secs", "INTEGER"),
            ("debt_mode", "TEXT NOT NULL DEFAULT 'normal'"),
            ("oldest_deletable_age_secs", "INTEGER"),
            ("deleted_rows_per_minute", "REAL NOT NULL DEFAULT 0"),
            ("recovery_deadline_at", "INTEGER"),
            ("slo_state", "TEXT NOT NULL DEFAULT 'unknown'"),
            ("foreground_rps", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !self
                .table_column_exists("ha_outbox_gc_channel_state", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE ha_outbox_gc_channel_state ADD COLUMN {column} {definition}"
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        for channel in ["control", "billing", "runtime"] {
            sqlx::query(
                "INSERT OR IGNORE INTO ha_outbox_gc_channel_state (channel) VALUES (?)",
            )
            .bind(channel)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
}

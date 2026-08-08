impl KeyStore {
    async fn ha_channel_high_watermark_on_conn(
        conn: &mut SqliteConnection,
        channel: HaSyncChannel,
    ) -> Result<i64, ProxyError> {
        let table_name = ha_channel_event_table(channel);
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let max_valid_row_seq = sqlx::query_scalar::<_, Option<i64>>(&format!(
            "SELECT MAX(seq) FROM {} WHERE resource IN ({allowed_resources})",
            quote_sqlite_identifier(table_name)
        ))
        .fetch_one(&mut *conn)
        .await?
        .unwrap_or(0);
        let has_sync_watermarks: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ha_sync_watermarks')",
        )
        .fetch_one(&mut *conn)
        .await?;
        let persisted_valid_seq: Option<i64> = if has_sync_watermarks {
            sqlx::query_scalar("SELECT watermark FROM ha_sync_watermarks WHERE name = ?")
                .bind(ha_channel_valid_watermark_name(channel))
                .fetch_optional(&mut *conn)
                .await?
        } else {
            None
        };
        if let Some(persisted_valid_seq) = persisted_valid_seq {
            return Ok(max_valid_row_seq.max(persisted_valid_seq));
        }
        let sequence_seq: Option<i64> =
            sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = ?")
                .bind(table_name)
                .fetch_optional(&mut *conn)
                .await?;
        Ok(max_valid_row_seq.max(sequence_seq.unwrap_or(0)))
    }

    async fn remember_ha_channel_valid_watermark_on_conn(
        conn: &mut SqliteConnection,
        channel: HaSyncChannel,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        let table = quote_sqlite_identifier(ha_channel_event_table(channel));
        let allowed_resources = ha_channel_allowed_resources_sql(channel);
        let max_valid_seq: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT MAX(seq) FROM {table} WHERE resource IN ({allowed_resources})"
        ))
        .fetch_one(&mut *conn)
        .await?;
        let Some(max_valid_seq) = max_valid_seq else {
            return Ok(());
        };
        let has_sync_watermarks: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ha_sync_watermarks')",
        )
        .fetch_one(&mut *conn)
        .await?;
        if !has_sync_watermarks {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO ha_sync_watermarks (
                name, source_node_id, target_node_id, watermark, updated_at, detail
            )
            VALUES (?, NULL, NULL, ?, ?, 'outbox_valid_high_watermark')
            ON CONFLICT(name) DO UPDATE SET
                watermark = MAX(ha_sync_watermarks.watermark, excluded.watermark),
                updated_at = excluded.updated_at,
                detail = excluded.detail
            "#,
        )
        .bind(ha_channel_valid_watermark_name(channel))
        .bind(max_valid_seq)
        .bind(updated_at)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    async fn ha_channel_expired_valid_watermark_on_conn(
        conn: &mut SqliteConnection,
        channel: HaSyncChannel,
    ) -> Result<Option<i64>, ProxyError> {
        let has_sync_watermarks: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ha_sync_watermarks')",
        )
        .fetch_one(&mut *conn)
        .await?;
        if !has_sync_watermarks {
            return Ok(None);
        }
        Ok(sqlx::query_scalar("SELECT watermark FROM ha_sync_watermarks WHERE name = ?")
            .bind(ha_channel_expired_valid_watermark_name(channel))
            .fetch_optional(&mut *conn)
            .await?)
    }

    async fn remember_ha_channel_expired_valid_watermark_on_conn(
        conn: &mut sqlx::SqliteConnection,
        channel: HaSyncChannel,
        max_deleted_seq: i64,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        let has_sync_watermarks: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ha_sync_watermarks')",
        )
        .fetch_one(&mut *conn)
        .await?;
        if !has_sync_watermarks {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO ha_sync_watermarks (
                name, source_node_id, target_node_id, watermark, updated_at, detail
            )
            VALUES (?, NULL, NULL, ?, ?, 'outbox_expired_valid_seq')
            ON CONFLICT(name) DO UPDATE SET
                watermark = MAX(ha_sync_watermarks.watermark, excluded.watermark),
                updated_at = excluded.updated_at,
                detail = excluded.detail
            "#,
        )
        .bind(ha_channel_expired_valid_watermark_name(channel))
        .bind(max_deleted_seq)
        .bind(updated_at)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    async fn ha_channel_expired_legacy_watermark_on_conn(
        conn: &mut sqlx::SqliteConnection,
        channel: HaSyncChannel,
    ) -> Result<Option<i64>, ProxyError> {
        let has_sync_watermarks: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ha_sync_watermarks')",
        )
        .fetch_one(&mut *conn)
        .await?;
        if !has_sync_watermarks {
            return Ok(None);
        }
        Ok(sqlx::query_scalar("SELECT watermark FROM ha_sync_watermarks WHERE name = ?")
            .bind(ha_channel_expired_legacy_watermark_name(channel))
            .fetch_optional(&mut *conn)
            .await?)
    }

    async fn remember_ha_channel_expired_legacy_watermark_on_conn(
        conn: &mut sqlx::SqliteConnection,
        channel: HaSyncChannel,
        max_deleted_seq: i64,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        let has_sync_watermarks: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'ha_sync_watermarks')",
        )
        .fetch_one(&mut *conn)
        .await?;
        if !has_sync_watermarks {
            return Ok(());
        }
        sqlx::query(
            r#"
            INSERT INTO ha_sync_watermarks (
                name, source_node_id, target_node_id, watermark, updated_at, detail
            )
            VALUES (?, NULL, NULL, ?, ?, 'outbox_expired_legacy_seq')
            ON CONFLICT(name) DO UPDATE SET
                watermark = MAX(ha_sync_watermarks.watermark, excluded.watermark),
                updated_at = excluded.updated_at,
                detail = excluded.detail
            "#,
        )
        .bind(ha_channel_expired_legacy_watermark_name(channel))
        .bind(max_deleted_seq)
        .bind(updated_at)
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}

fn ha_channel_valid_watermark_name(channel: HaSyncChannel) -> String {
    format!("local_ha_{}_valid_seq", channel.as_str())
}

fn ha_channel_expired_valid_watermark_name(channel: HaSyncChannel) -> String {
    format!("local_ha_{}_expired_valid_seq", channel.as_str())
}

fn ha_channel_expired_legacy_watermark_name(channel: HaSyncChannel) -> String {
    format!("local_ha_{}_expired_legacy_seq", channel.as_str())
}

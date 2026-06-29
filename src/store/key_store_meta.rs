const META_KEY_HA_FULL_MASTER_NODE_ID_V1: &str = "ha_full_master_node_id_v1";

impl KeyStore {
    pub(crate) async fn get_meta_string(&self, key: &str) -> Result<Option<String>, ProxyError> {
        sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn get_meta_i64(&self, key: &str) -> Result<Option<i64>, ProxyError> {
        let value = self.get_meta_string(key).await?;

        if let Some(v) = value {
            match v.parse::<i64>() {
                Ok(parsed) => Ok(Some(parsed)),
                Err(_) => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn set_meta_string(&self, key: &str, value: &str) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO meta (key, value)
            VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn set_meta_i64(&self, key: &str, value: i64) -> Result<(), ProxyError> {
        let v = value.to_string();
        self.set_meta_string(key, &v).await
    }

    pub(crate) async fn get_ha_full_master_node_id(&self) -> Result<Option<String>, ProxyError> {
        self.get_meta_string(META_KEY_HA_FULL_MASTER_NODE_ID_V1).await
    }

    pub(crate) async fn set_ha_full_master_node_id(&self, node_id: &str) -> Result<(), ProxyError> {
        self.set_meta_string(META_KEY_HA_FULL_MASTER_NODE_ID_V1, node_id)
            .await
    }
}

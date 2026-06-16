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
}

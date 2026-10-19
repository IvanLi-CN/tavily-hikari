const META_KEY_HA_FULL_MASTER_NODE_ID_V1: &str = "ha_full_master_node_id_v1";

#[cfg(debug_assertions)]
tokio::task_local! {
    static META_SELECT_COUNT: std::sync::Arc<std::sync::atomic::AtomicUsize>;
}

#[cfg(debug_assertions)]
fn record_meta_select() {
    let _ = META_SELECT_COUNT.try_with(|count| {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
}

#[cfg(debug_assertions)]
pub(crate) async fn with_meta_select_count<F, T>(
    count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    future: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    META_SELECT_COUNT.scope(count, future).await
}

impl KeyStore {
    pub(crate) async fn derive_upstream_project_id(
        &self,
        token_id: &str,
        period_code: &str,
    ) -> Result<String, ProxyError> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use ring::rand::{SecureRandom as _, SystemRandom};

        let secret = if let Some(encoded) = self
            .get_meta_string(META_KEY_UPSTREAM_PROJECT_ID_HMAC_SECRET_V1)
            .await?
        {
            URL_SAFE_NO_PAD.decode(encoded).map_err(|_| {
                ProxyError::Other("invalid upstream project id HMAC secret".to_string())
            })?
        } else {
            let mut generated = [0_u8; 32];
            SystemRandom::new()
                .fill(&mut generated)
                .map_err(|_| ProxyError::Other("failed to generate HMAC secret".to_string()))?;
            let encoded = URL_SAFE_NO_PAD.encode(generated);
            sqlx::query("INSERT OR IGNORE INTO meta (key, value) VALUES (?, ?)")
                .bind(META_KEY_UPSTREAM_PROJECT_ID_HMAC_SECRET_V1)
                .bind(encoded)
                .execute(&self.pool)
                .await?;
            let stored = self
                .get_meta_string(META_KEY_UPSTREAM_PROJECT_ID_HMAC_SECRET_V1)
                .await?
                .ok_or_else(|| ProxyError::Other("missing generated HMAC secret".to_string()))?;
            URL_SAFE_NO_PAD.decode(stored).map_err(|_| {
                ProxyError::Other("invalid upstream project id HMAC secret".to_string())
            })?
        };
        if secret.len() != 32 {
            return Err(ProxyError::Other(
                "invalid upstream project id HMAC secret length".to_string(),
            ));
        }
        Ok(derive_access_token_project_id(
            &secret,
            token_id,
            period_code,
        ))
    }

    pub(crate) async fn get_meta_string(&self, key: &str) -> Result<Option<String>, ProxyError> {
        #[cfg(debug_assertions)]
        record_meta_select();
        sqlx::query_scalar::<_, String>("SELECT value FROM meta WHERE key = ? LIMIT 1")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn get_meta_values(
        &self,
        keys: &[&str],
    ) -> Result<StdHashMap<String, String>, ProxyError> {
        if keys.is_empty() {
            return Ok(StdHashMap::new());
        }

        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT key, value FROM meta WHERE key IN (",
        );
        {
            let mut separated = query.separated(", ");
            for key in keys {
                separated.push_bind(*key);
            }
        }
        query.push(")");

        #[cfg(debug_assertions)]
        record_meta_select();
        let rows = query
            .build_query_as::<(String, String)>()
            .fetch_all(&self.pool)
            .await
            .map_err(ProxyError::Database)?;
        Ok(rows.into_iter().collect())
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

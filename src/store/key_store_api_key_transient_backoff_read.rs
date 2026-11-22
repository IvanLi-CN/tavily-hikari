impl KeyStore {
    pub(crate) async fn list_active_api_key_transient_backoffs(
        &self,
        key_ids: &[String],
        scope: &str,
        now: i64,
    ) -> Result<HashMap<String, ApiKeyTransientBackoffState>, ProxyError> {
        if key_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut builder = QueryBuilder::<sqlx::Postgres>::new(
            "SELECT key_id, cooldown_until, retry_after_secs FROM core.api_key_transient_backoffs \
             WHERE scope = ",
        );
        builder.push_bind(scope);
        builder.push(" AND cooldown_until > ");
        builder.push_bind(now);
        builder.push(" AND key_id IN (");
        {
            let mut separated = builder.separated(", ");
            for key_id in key_ids {
                separated.push_bind(key_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(self.pg()?)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(key_id, cooldown_until, retry_after_secs)| {
                (
                    key_id,
                    ApiKeyTransientBackoffState {
                        cooldown_until,
                        retry_after_secs,
                    },
                )
            })
            .collect())
    }
}

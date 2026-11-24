// Quota accounting reads.
//
// Each reader serves both engines: the SQLite body stays as the default and a PostgreSQL
// branch reads `core.token_usage_buckets`, `core.account_usage_buckets`,
// `core.auth_token_quota` and `core.account_monthly_quota`. The module is separate so the
// legacy migration host stays inside its line budget.

impl KeyStore {
    pub(crate) async fn sum_usage_buckets(
        &self,
        token_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        if self.pg_pool.is_some() {
            let sum = sqlx::query_scalar::<_, Option<i64>>(
                r#"
                SELECT SUM(count)::bigint
                FROM core.token_usage_buckets
                WHERE token_id = $1 AND granularity = $2 AND bucket_start >= $3
                "#,
            )
            .bind(token_id)
            .bind(granularity)
            .bind(bucket_start_at_least)
            .fetch_one(self.pg()?)
            .await?;
            return Ok(sum.unwrap_or(0));
        }
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM token_usage_buckets
            WHERE token_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(token_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_usage_buckets_between(
        &self,
        token_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<i64, ProxyError> {
        if self.pg_pool.is_some() {
            let sum = sqlx::query_scalar::<_, Option<i64>>(
                r#"
                SELECT SUM(count)::bigint
                FROM core.token_usage_buckets
                WHERE token_id = $1
                  AND granularity = $2
                  AND bucket_start >= $3
                  AND bucket_start < $4
                "#,
            )
            .bind(token_id)
            .bind(granularity)
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(self.pg()?)
            .await?;
            return Ok(sum.unwrap_or(0));
        }
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM token_usage_buckets
            WHERE token_id = ?
              AND granularity = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            "#,
        )
        .bind(token_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets(
        &self,
        user_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<i64, ProxyError> {
        if self.pg_pool.is_some() {
            let sum = sqlx::query_scalar::<_, Option<i64>>(
                r#"
                SELECT SUM(count)::bigint
                FROM core.account_usage_buckets
                WHERE user_id = $1 AND granularity = $2 AND bucket_start >= $3
                "#,
            )
            .bind(user_id)
            .bind(granularity)
            .bind(bucket_start_at_least)
            .fetch_one(self.pg()?)
            .await?;
            return Ok(sum.unwrap_or(0));
        }
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM account_usage_buckets
            WHERE user_id = ? AND granularity = ? AND bucket_start >= ?
            "#,
        )
        .bind(user_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets_between(
        &self,
        user_id: &str,
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<i64, ProxyError> {
        if self.pg_pool.is_some() {
            let sum = sqlx::query_scalar::<_, Option<i64>>(
                r#"
                SELECT SUM(count)::bigint
                FROM core.account_usage_buckets
                WHERE user_id = $1
                  AND granularity = $2
                  AND bucket_start >= $3
                  AND bucket_start < $4
                "#,
            )
            .bind(user_id)
            .bind(granularity)
            .bind(bucket_start_at_least)
            .bind(bucket_start_before)
            .fetch_one(self.pg()?)
            .await?;
            return Ok(sum.unwrap_or(0));
        }
        let sum = sqlx::query_scalar::<_, Option<i64>>(
            r#"
            SELECT SUM(count)
            FROM account_usage_buckets
            WHERE user_id = ?
              AND granularity = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            "#,
        )
        .bind(user_id)
        .bind(granularity)
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_one(&self.pool)
        .await?;
        Ok(sum.unwrap_or(0))
    }

    pub(crate) async fn sum_account_usage_buckets_bulk(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "SELECT user_id, SUM(count)::bigint AS total FROM core.account_usage_buckets WHERE granularity = ",
            );
            builder.push_bind(granularity);
            builder.push(" AND bucket_start >= ");
            builder.push_bind(bucket_start_at_least);
            builder.push(" AND user_id IN (");
            {
                let mut separated = builder.separated(", ");
                for user_id in user_ids {
                    separated.push_bind(user_id);
                }
            }
            builder.push(") GROUP BY user_id");
            let rows = builder
                .build_query_as::<(String, i64)>()
                .fetch_all(self.pg()?)
                .await?;
            return Ok(rows.into_iter().collect());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, SUM(count) as total FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, total) in rows {
            map.insert(user_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn sum_account_usage_buckets_bulk_between(
        &self,
        user_ids: &[String],
        granularity: &str,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "SELECT user_id, SUM(count)::bigint AS total FROM core.account_usage_buckets WHERE granularity = ",
            );
            builder.push_bind(granularity);
            builder.push(" AND bucket_start >= ");
            builder.push_bind(bucket_start_at_least);
            builder.push(" AND bucket_start < ");
            builder.push_bind(bucket_start_before);
            builder.push(" AND user_id IN (");
            {
                let mut separated = builder.separated(", ");
                for user_id in user_ids {
                    separated.push_bind(user_id);
                }
            }
            builder.push(") GROUP BY user_id");
            let rows = builder
                .build_query_as::<(String, i64)>()
                .fetch_all(self.pg()?)
                .await?;
            return Ok(rows.into_iter().collect());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, SUM(count) as total FROM account_usage_buckets WHERE granularity = ",
        );
        builder.push_bind(granularity);
        builder.push(" AND bucket_start >= ");
        builder.push_bind(bucket_start_at_least);
        builder.push(" AND bucket_start < ");
        builder.push_bind(bucket_start_before);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder
            .build_query_as::<(String, i64)>()
            .fetch_all(&self.pool)
            .await?;
        let mut map = HashMap::new();
        for (user_id, total) in rows {
            map.insert(user_id, total);
        }
        Ok(map)
    }

    pub(crate) async fn fetch_monthly_counts(
        &self,
        token_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if token_ids.is_empty() {
            return Ok(HashMap::new());
        }

        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "SELECT token_id, month_start, month_count FROM core.auth_token_quota WHERE token_id IN (",
            );
            {
                let mut separated = builder.separated(", ");
                for token_id in token_ids {
                    separated.push_bind(token_id);
                }
            }
            builder.push(")");
            let rows = builder
                .build_query_as::<(String, i64, i64)>()
                .fetch_all(self.pg()?)
                .await?;
            let mut map = HashMap::new();
            let mut stale_ids = Vec::new();
            for (token_id, stored_start, stored_count) in rows {
                if stored_start < current_month_start {
                    map.insert(token_id.clone(), 0);
                    stale_ids.push(token_id);
                } else {
                    map.insert(token_id, stored_count);
                }
            }
            for token_id in stale_ids {
                sqlx::query(
                    "UPDATE core.auth_token_quota SET month_start = $1, month_count = 0 WHERE token_id = $2",
                )
                .bind(current_month_start)
                .bind(&token_id)
                .execute(self.pg()?)
                .await?;
            }
            return Ok(map);
        }
        let mut builder = QueryBuilder::new(
            "SELECT token_id, month_start, month_count FROM auth_token_quota WHERE token_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for token_id in token_ids {
                separated.push_bind(token_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (token_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(token_id.clone(), 0);
                stale_ids.push(token_id);
            } else {
                map.insert(token_id, stored_count);
            }
        }

        for token_id in stale_ids {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(&token_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }

    pub(crate) async fn fetch_monthly_count(
        &self,
        token_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        if self.pg_pool.is_some() {
            let row = sqlx::query_as::<_, (i64, i64)>(
                "SELECT month_start, month_count FROM core.auth_token_quota WHERE token_id = $1",
            )
            .bind(token_id)
            .fetch_optional(self.pg()?)
            .await?;
            let Some((stored_start, stored_count)) = row else {
                return Ok(0);
            };
            if stored_start < current_month_start {
                sqlx::query(
                    "UPDATE core.auth_token_quota SET month_start = $1, month_count = 0 WHERE token_id = $2",
                )
                .bind(current_month_start)
                .bind(token_id)
                .execute(self.pg()?)
                .await?;
                return Ok(0);
            }
            return Ok(stored_count);
        }
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM auth_token_quota WHERE token_id = ?",
        )
        .bind(token_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE auth_token_quota SET month_start = ?, month_count = 0 WHERE token_id = ?",
            )
            .bind(current_month_start)
            .bind(token_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    pub(crate) async fn fetch_account_monthly_count(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<i64, ProxyError> {
        if self.pg_pool.is_some() {
            let row = sqlx::query_as::<_, (i64, i64)>(
                "SELECT month_start, month_count FROM core.account_monthly_quota WHERE user_id = $1",
            )
            .bind(user_id)
            .fetch_optional(self.pg()?)
            .await?;
            let Some((stored_start, stored_count)) = row else {
                return Ok(0);
            };
            if stored_start < current_month_start {
                sqlx::query(
                    "UPDATE core.account_monthly_quota SET month_start = $1, month_count = 0 WHERE user_id = $2",
                )
                .bind(current_month_start)
                .bind(user_id)
                .execute(self.pg()?)
                .await?;
                return Ok(0);
            }
            return Ok(stored_count);
        }
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT month_start, month_count FROM account_monthly_quota WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((stored_start, stored_count)) = row else {
            return Ok(0);
        };
        if stored_start < current_month_start {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
            return Ok(0);
        }
        Ok(stored_count)
    }

    pub(crate) async fn fetch_account_monthly_counts(
        &self,
        user_ids: &[String],
        current_month_start: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }

        if self.pg_pool.is_some() {
            let mut builder = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "SELECT user_id, month_start, month_count FROM core.account_monthly_quota WHERE user_id IN (",
            );
            {
                let mut separated = builder.separated(", ");
                for user_id in user_ids {
                    separated.push_bind(user_id);
                }
            }
            builder.push(")");
            let rows = builder
                .build_query_as::<(String, i64, i64)>()
                .fetch_all(self.pg()?)
                .await?;
            let mut map = HashMap::new();
            let mut stale_ids = Vec::new();
            for (user_id, stored_start, stored_count) in rows {
                if stored_start < current_month_start {
                    map.insert(user_id.clone(), 0);
                    stale_ids.push(user_id);
                } else {
                    map.insert(user_id, stored_count);
                }
            }
            for user_id in stale_ids {
                sqlx::query(
                    "UPDATE core.account_monthly_quota SET month_start = $1, month_count = 0 WHERE user_id = $2",
                )
                .bind(current_month_start)
                .bind(&user_id)
                .execute(self.pg()?)
                .await?;
            }
            return Ok(map);
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, month_start, month_count FROM account_monthly_quota WHERE user_id IN (",
        );
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(")");

        let rows = builder
            .build_query_as::<(String, i64, i64)>()
            .fetch_all(&self.pool)
            .await?;

        let mut map = HashMap::new();
        let mut stale_ids = Vec::new();
        for (user_id, stored_start, stored_count) in rows {
            if stored_start < current_month_start {
                map.insert(user_id.clone(), 0);
                stale_ids.push(user_id);
            } else {
                map.insert(user_id, stored_count);
            }
        }

        for user_id in stale_ids {
            sqlx::query(
                "UPDATE account_monthly_quota SET month_start = ?, month_count = 0 WHERE user_id = ?",
            )
            .bind(current_month_start)
            .bind(&user_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(map)
    }
}

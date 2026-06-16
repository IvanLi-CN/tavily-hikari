impl KeyStore {
    fn push_admin_token_filters<'a>(
        builder: &mut QueryBuilder<'a, Sqlite>,
        filters: &'a AdminTokenListFilters,
        search_like: Option<&'a str>,
    ) {
        builder.push("deleted_at IS NULL");

        if filters.no_group {
            builder.push(" AND TRIM(COALESCE(group_name, '')) = ''");
        } else if let Some(group) = filters.group.as_deref() {
            builder.push(" AND TRIM(COALESCE(group_name, '')) = ");
            builder.push_bind(group);
        }

        match filters.enabled {
            AdminTokenEnabledFilter::All => {}
            AdminTokenEnabledFilter::Active => {
                builder.push(" AND enabled = 1");
            }
            AdminTokenEnabledFilter::Frozen => {
                builder.push(" AND enabled = 0");
            }
        }

        match filters.owner {
            AdminTokenOwnerFilter::All => {}
            AdminTokenOwnerFilter::Bound => {
                builder.push(
                    " AND EXISTS (SELECT 1 FROM user_token_bindings b WHERE b.token_id = auth_tokens.id)",
                );
            }
            AdminTokenOwnerFilter::Unbound => {
                builder.push(
                    " AND NOT EXISTS (SELECT 1 FROM user_token_bindings b WHERE b.token_id = auth_tokens.id)",
                );
            }
        }

        if let Some(search) = search_like {
            builder.push(" AND (id LIKE ");
            builder.push_bind(search);
            builder.push(" OR COALESCE(note, '') LIKE ");
            builder.push_bind(search);
            builder.push(" OR EXISTS (SELECT 1 FROM user_token_bindings b_search JOIN users u_search ON u_search.id = b_search.user_id WHERE b_search.token_id = auth_tokens.id AND (u_search.id LIKE ");
            builder.push_bind(search);
            builder.push(" OR COALESCE(u_search.display_name, '') LIKE ");
            builder.push_bind(search);
            builder.push(" OR COALESCE(u_search.username, '') LIKE ");
            builder.push_bind(search);
            builder.push(")))");
        }
    }

    fn auth_token_from_row(
        (id, enabled, note, group_name, total_requests, created_at, last_used_at): (
            String,
            i64,
            Option<String>,
            Option<String>,
            i64,
            i64,
            Option<i64>,
        ),
    ) -> AuthToken {
        AuthToken {
            id,
            enabled: enabled == 1,
            note,
            group_name,
            total_requests,
            created_at,
            last_used_at,
            quota: None,
            quota_hourly_reset_at: None,
            quota_daily_reset_at: None,
            quota_monthly_reset_at: None,
        }
    }

    /// Paginated list of access tokens ordered by created_at desc. Returns (items, total)
    pub(crate) async fn list_access_tokens_filtered_paged(
        &self,
        page: i64,
        per_page: i64,
        filters: &AdminTokenListFilters,
    ) -> Result<(Vec<AuthToken>, i64), ProxyError> {
        self.flush_request_stats_writes().await?;
        let _permit = self
            .admin_heavy_read_semaphore
            .acquire()
            .await
            .expect("admin heavy read semaphore is never closed");
        let page = page.max(1);
        let per_page = per_page.clamp(1, 200);
        let offset = (page - 1) * per_page;
        let search_like = filters.search.as_ref().map(|value| format!("%{value}%"));

        let mut count_builder =
            QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM auth_tokens WHERE ");
        Self::push_admin_token_filters(&mut count_builder, filters, search_like.as_deref());
        let total: i64 = count_builder.build_query_scalar().fetch_one(&self.pool).await?;

        let mut rows_builder = QueryBuilder::<Sqlite>::new(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE "#,
        );
        Self::push_admin_token_filters(&mut rows_builder, filters, search_like.as_deref());
        rows_builder.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        rows_builder.push_bind(per_page);
        rows_builder.push(" OFFSET ");
        rows_builder.push_bind(offset);

        let rows = rows_builder
            .build_query_as::<(
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            )>()
            .fetch_all(&self.pool)
            .await?;
        Ok((rows.into_iter().map(Self::auth_token_from_row).collect(), total))
    }

    pub(crate) async fn list_access_tokens_for_filters(
        &self,
        filters: &AdminTokenListFilters,
    ) -> Result<Vec<AuthToken>, ProxyError> {
        self.flush_request_stats_writes().await?;
        let _permit = self
            .admin_heavy_read_semaphore
            .acquire()
            .await
            .expect("admin heavy read semaphore is never closed");
        let search_like = filters.search.as_ref().map(|value| format!("%{value}%"));
        let mut builder = QueryBuilder::<Sqlite>::new(
            r#"SELECT id, enabled, note, group_name, total_requests, created_at, last_used_at
               FROM auth_tokens
               WHERE "#,
        );
        Self::push_admin_token_filters(&mut builder, filters, search_like.as_deref());
        builder.push(" ORDER BY created_at DESC, id DESC");
        let rows = builder
            .build_query_as::<(
                String,
                i64,
                Option<String>,
                Option<String>,
                i64,
                i64,
                Option<i64>,
            )>()
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(Self::auth_token_from_row).collect())
    }

    pub(crate) async fn set_access_tokens_enabled(
        &self,
        ids: &[String],
        enabled: bool,
    ) -> Result<AdminTokenBatchMutationResult, ProxyError> {
        if ids.is_empty() {
            return Ok(AdminTokenBatchMutationResult {
                updated: 0,
                missing: Vec::new(),
            });
        }

        let existing = self.existing_access_token_ids(ids).await?;
        let missing = ids
            .iter()
            .filter(|id| !existing.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if existing.is_empty() {
            return Ok(AdminTokenBatchMutationResult {
                updated: 0,
                missing,
            });
        }

        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE auth_tokens SET enabled = ");
        builder.push_bind(if enabled { 1 } else { 0 });
        builder.push(" WHERE deleted_at IS NULL AND id IN (");
        let mut separated = builder.separated(", ");
        for id in &existing {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
        let result = builder.build().execute(&self.pool).await?;
        Ok(AdminTokenBatchMutationResult {
            updated: result.rows_affected() as i64,
            missing,
        })
    }

    pub(crate) async fn delete_access_tokens(
        &self,
        ids: &[String],
    ) -> Result<AdminTokenBatchMutationResult, ProxyError> {
        if ids.is_empty() {
            return Ok(AdminTokenBatchMutationResult {
                updated: 0,
                missing: Vec::new(),
            });
        }

        let existing = self.existing_access_token_ids(ids).await?;
        let missing = ids
            .iter()
            .filter(|id| !existing.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if existing.is_empty() {
            return Ok(AdminTokenBatchMutationResult {
                updated: 0,
                missing,
            });
        }

        let now = self.backend_time.now_ts();
        let mut builder =
            QueryBuilder::<Sqlite>::new("UPDATE auth_tokens SET enabled = 0, deleted_at = ");
        builder.push_bind(now);
        builder.push(" WHERE deleted_at IS NULL AND id IN (");
        let mut separated = builder.separated(", ");
        for id in &existing {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
        let result = builder.build().execute(&self.pool).await?;
        Ok(AdminTokenBatchMutationResult {
            updated: result.rows_affected() as i64,
            missing,
        })
    }

    async fn existing_access_token_ids(&self, ids: &[String]) -> Result<Vec<String>, ProxyError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id FROM auth_tokens WHERE deleted_at IS NULL AND id IN (",
        );
        let mut separated = builder.separated(", ");
        for id in ids {
            separated.push_bind(id);
        }
        separated.push_unseparated(")");
        let existing = builder
            .build_query_scalar::<String>()
            .fetch_all(&self.pool)
            .await?;
        Ok(ids
            .iter()
            .filter(|id| existing.iter().any(|existing_id| existing_id == *id))
            .cloned()
            .collect())
    }
}

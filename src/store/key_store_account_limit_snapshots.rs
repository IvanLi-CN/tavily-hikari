#[derive(Debug, Clone, Copy)]
pub(crate) enum AccountQuotaLimitSnapshotField {
    Hourly,
    Daily,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestRateLimitSnapshotRecord {
    pub(crate) changed_at: i64,
    pub(crate) limit_value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AccountQuotaLimitSnapshotRecord {
    pub(crate) changed_at: i64,
    pub(crate) hourly_any_limit: i64,
    pub(crate) hourly_limit: i64,
    pub(crate) daily_limit: i64,
    pub(crate) monthly_limit: i64,
}

impl AccountQuotaLimitSnapshotRecord {
    fn same_limits_as(&self, limits: &AccountQuotaLimits) -> bool {
        self.hourly_any_limit == limits.hourly_any_limit
            && self.hourly_limit == limits.hourly_limit
            && self.daily_limit == limits.daily_limit
            && self.monthly_limit == limits.monthly_limit
    }

    pub(crate) fn select(&self, field: AccountQuotaLimitSnapshotField) -> i64 {
        match field {
            AccountQuotaLimitSnapshotField::Hourly => self.hourly_limit,
            AccountQuotaLimitSnapshotField::Daily => self.daily_limit,
            AccountQuotaLimitSnapshotField::Monthly => self.monthly_limit,
        }
    }
}

impl KeyStore {
    pub(crate) async fn fetch_user_created_at(
        &self,
        user_id: &str,
    ) -> Result<Option<i64>, ProxyError> {
        sqlx::query_scalar::<_, i64>("SELECT created_at FROM users WHERE id = ? LIMIT 1")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    async fn fetch_latest_request_rate_limit_snapshot(
        &self,
    ) -> Result<Option<RequestRateLimitSnapshotRecord>, ProxyError> {
        sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT changed_at, limit_value
            FROM request_rate_limit_snapshots
            ORDER BY changed_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(|(changed_at, limit_value)| RequestRateLimitSnapshotRecord {
                changed_at,
                limit_value,
            })
        })
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn fetch_request_rate_limit_snapshots_for_window(
        &self,
        window_start: i64,
        changed_before: i64,
    ) -> Result<Vec<RequestRateLimitSnapshotRecord>, ProxyError> {
        let mut snapshots = Vec::new();

        if let Some((changed_at, limit_value)) = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT changed_at, limit_value
            FROM request_rate_limit_snapshots
            WHERE changed_at < ?
            ORDER BY changed_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(window_start)
        .fetch_optional(&self.pool)
        .await?
        {
            snapshots.push(RequestRateLimitSnapshotRecord {
                changed_at,
                limit_value,
            });
        }

        let window_rows = sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT changed_at, limit_value
            FROM request_rate_limit_snapshots
            WHERE changed_at >= ?
              AND changed_at < ?
            ORDER BY changed_at ASC, id ASC
            "#,
        )
        .bind(window_start)
        .bind(changed_before)
        .fetch_all(&self.pool)
        .await?;

        snapshots.extend(window_rows.into_iter().map(|(changed_at, limit_value)| {
            RequestRateLimitSnapshotRecord {
                changed_at,
                limit_value,
            }
        }));

        Ok(snapshots)
    }

    pub(crate) async fn record_request_rate_limit_snapshot_at(
        &self,
        limit_value: i64,
        changed_at: i64,
    ) -> Result<(), ProxyError> {
        let limit_value = limit_value.max(0);
        if let Some(latest) = self.fetch_latest_request_rate_limit_snapshot().await?
            && latest.limit_value == limit_value
        {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO request_rate_limit_snapshots (changed_at, limit_value)
            VALUES (?, ?)
            "#,
        )
        .bind(changed_at)
        .bind(limit_value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn fetch_latest_account_quota_limit_snapshot(
        &self,
        user_id: &str,
    ) -> Result<Option<AccountQuotaLimitSnapshotRecord>, ProxyError> {
        sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            SELECT changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit
            FROM account_quota_limit_snapshots
            WHERE user_id = ?
            ORDER BY changed_at DESC, id DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| {
            row.map(
                |(changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit)| {
                    AccountQuotaLimitSnapshotRecord {
                        changed_at,
                        hourly_any_limit,
                        hourly_limit,
                        daily_limit,
                        monthly_limit,
                    }
                },
            )
        })
        .map_err(ProxyError::Database)
    }

    pub(crate) async fn fetch_account_quota_limit_snapshots_for_window(
        &self,
        user_id: &str,
        window_start: i64,
        changed_before: i64,
    ) -> Result<Vec<AccountQuotaLimitSnapshotRecord>, ProxyError> {
        let mut snapshots = Vec::new();

        if let Some((changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit)) =
            sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
                r#"
                SELECT changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit
                FROM account_quota_limit_snapshots
                WHERE user_id = ?
                  AND changed_at < ?
                ORDER BY changed_at DESC, id DESC
                LIMIT 1
                "#,
            )
            .bind(user_id)
            .bind(window_start)
            .fetch_optional(&self.pool)
            .await?
        {
            snapshots.push(AccountQuotaLimitSnapshotRecord {
                changed_at,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
            });
        }

        let window_rows = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            SELECT changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit
            FROM account_quota_limit_snapshots
            WHERE user_id = ?
              AND changed_at >= ?
              AND changed_at < ?
            ORDER BY changed_at ASC, id ASC
            "#,
        )
        .bind(user_id)
        .bind(window_start)
        .bind(changed_before)
        .fetch_all(&self.pool)
        .await?;

        snapshots.extend(window_rows.into_iter().map(
            |(changed_at, hourly_any_limit, hourly_limit, daily_limit, monthly_limit)| {
                AccountQuotaLimitSnapshotRecord {
                    changed_at,
                    hourly_any_limit,
                    hourly_limit,
                    daily_limit,
                    monthly_limit,
                }
            },
        ));

        Ok(snapshots)
    }

    async fn insert_account_quota_limit_snapshot(
        &self,
        user_id: &str,
        changed_at: i64,
        limits: &AccountQuotaLimits,
    ) -> Result<(), ProxyError> {
        let limits = limits.clamped_non_negative();
        if let Some(latest) = self.fetch_latest_account_quota_limit_snapshot(user_id).await?
            && latest.same_limits_as(&limits)
        {
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO account_quota_limit_snapshots (
                user_id,
                changed_at,
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit
            )
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(changed_at)
        .bind(limits.hourly_any_limit)
        .bind(limits.hourly_limit)
        .bind(limits.daily_limit)
        .bind(limits.monthly_limit)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn record_effective_account_quota_snapshot_at(
        &self,
        user_id: &str,
        changed_at: i64,
    ) -> Result<(), ProxyError> {
        self.invalidate_account_quota_resolution(user_id).await;
        let resolution = self.resolve_account_quota_resolution(user_id).await?;
        self.insert_account_quota_limit_snapshot(user_id, changed_at, &resolution.effective)
            .await?;
        self.invalidate_account_quota_resolution(user_id).await;
        Ok(())
    }

    pub(crate) async fn record_effective_account_quota_snapshots_for_users_at(
        &self,
        user_ids: &[String],
        changed_at: i64,
    ) -> Result<(), ProxyError> {
        if user_ids.is_empty() {
            return Ok(());
        }

        let mut deduped = user_ids.to_vec();
        deduped.sort_unstable();
        deduped.dedup();

        for user_id in deduped {
            self.record_effective_account_quota_snapshot_at(&user_id, changed_at)
                .await?;
        }

        Ok(())
    }

    async fn current_account_quota_snapshot_seed(
        &self,
        user_id: &str,
    ) -> Result<Option<(i64, AccountQuotaLimits)>, ProxyError> {
        let Some(user_created_at) = self.fetch_user_created_at(user_id).await? else {
            return Ok(None);
        };

        let mut known_since = None;
        let base_limits = if let Some((
            hourly_any_limit,
            hourly_limit,
            daily_limit,
            monthly_limit,
            inherits_defaults,
            created_at,
            updated_at,
        )) = sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
            r#"
            SELECT
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
                COALESCE(inherits_defaults, 1),
                created_at,
                updated_at
            FROM account_quota_limits
            WHERE user_id = ?
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?
        {
            known_since = Some(created_at.max(updated_at));
            account_quota_limits_from_row(
                hourly_any_limit,
                hourly_limit,
                daily_limit,
                monthly_limit,
                inherits_defaults,
            )
        } else {
            default_account_quota_limits_for_created_at(
                user_created_at,
                self.account_quota_zero_base_cutover_at().await?,
            )
        };

        for (binding_created_at, binding_updated_at, tag_updated_at) in
            sqlx::query_as::<_, (i64, i64, i64)>(
                r#"
                SELECT b.created_at, b.updated_at, t.updated_at
                FROM user_tag_bindings b
                JOIN user_tags t ON t.id = b.tag_id
                WHERE b.user_id = ?
                "#,
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?
        {
            let evidence_at = binding_created_at.max(binding_updated_at).max(tag_updated_at);
            known_since = Some(match known_since {
                Some(previous) => previous.max(evidence_at),
                None => evidence_at,
            });
        }

        let effective = build_account_quota_resolution(
            base_limits,
            self.list_user_tag_bindings_for_user(user_id).await?,
        )
        .effective;
        Ok(Some((known_since.unwrap_or(user_created_at), effective)))
    }

    pub(crate) async fn backfill_account_limit_snapshot_history_v1(
        &self,
    ) -> Result<(), ProxyError> {
        if self
            .get_meta_i64(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1)
            .await?
            .is_some()
        {
            return Ok(());
        }

        let now = Utc::now().timestamp();
        if self.fetch_latest_request_rate_limit_snapshot().await?.is_none() {
            let configured_limit = self.get_meta_i64(META_KEY_REQUEST_RATE_LIMIT_V1).await?;
            let coverage_start = self
                .get_meta_i64(META_KEY_ACCOUNT_USAGE_ROLLUP_RATE5M_COVERAGE_START)
                .await?;
            let default_limit = request_rate_limit();
            let changed_at = coverage_start.unwrap_or(now);
            self.record_request_rate_limit_snapshot_at(
                configured_limit.unwrap_or(default_limit),
                changed_at,
            )
            .await?;
        }

        let user_ids = sqlx::query_scalar::<_, String>("SELECT id FROM users ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await?;
        for user_id in user_ids {
            if self
                .fetch_latest_account_quota_limit_snapshot(&user_id)
                .await?
                .is_some()
            {
                continue;
            }
            if let Some((known_since, effective)) =
                self.current_account_quota_snapshot_seed(&user_id).await?
            {
                self.insert_account_quota_limit_snapshot(&user_id, known_since, &effective)
                    .await?;
            }
        }

        self.set_meta_i64(META_KEY_ACCOUNT_LIMIT_SNAPSHOT_BACKFILL_V1, now)
            .await?;
        Ok(())
    }
}

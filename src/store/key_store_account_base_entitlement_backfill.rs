impl KeyStore {
    pub(crate) async fn backfill_account_base_entitlements_from_custom_limits_v1(
        &self,
    ) -> Result<(), ProxyError> {
        let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64)>(
            r#"
            SELECT
                aql.user_id,
                aql.business_calls_1h_limit,
                aql.daily_credits_limit,
                aql.monthly_credits_limit,
                COALESCE(aql.updated_at, aql.created_at),
                u.created_at
            FROM account_quota_limits aql
            JOIN users u ON u.id = aql.user_id
            WHERE COALESCE(aql.inherits_defaults, 1) = 0
            ORDER BY aql.user_id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.is_empty() {
            return Ok(());
        }

        let cutover_at = self.account_quota_zero_base_cutover_at().await?;
        let now = self.backend_time.now_ts();
        let mut touched_user_ids = Vec::new();
        let mut tx = self.pool.begin().await?;
        for (
            user_id,
            business_calls_1h_limit,
            daily_credits_limit,
            monthly_credits_limit,
            updated_at,
            user_created_at,
        ) in rows
        {
            let defaults = default_account_quota_limits_for_created_at(user_created_at, cutover_at);
            let business_delta = business_calls_1h_limit - defaults.business_calls_1h_limit;
            let daily_delta = daily_credits_limit - defaults.daily_credits_limit;
            let monthly_delta = monthly_credits_limit - defaults.monthly_credits_limit;
            let created_at = updated_at.max(0).max(user_created_at);
            if business_delta != 0 || daily_delta != 0 || monthly_delta != 0 {
                sqlx::query(
                    r#"
                    INSERT INTO account_entitlements (
                        user_id,
                        scope_kind,
                        month_start,
                        business_calls_1h_delta,
                        daily_credits_delta,
                        monthly_credits_delta,
                        backend_note,
                        frontend_note,
                        source_kind,
                        source_id,
                        actor_user_id,
                        actor_display_name,
                        created_at
                    )
                    VALUES (?, ?, 0, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
                    "#,
                )
                .bind(&user_id)
                .bind(ACCOUNT_ENTITLEMENT_SCOPE_BASE)
                .bind(business_delta)
                .bind(daily_delta)
                .bind(monthly_delta)
                .bind("custom base quota migration")
                .bind("Migrated custom base quota")
                .bind(ACCOUNT_ENTITLEMENT_SOURCE_KIND_ADMIN)
                .bind(format!("admin:base-backfill:{user_id}"))
                .bind("System migration")
                .bind(created_at)
                .execute(&mut *tx)
                .await?;
            }

            sqlx::query(
                r#"
                UPDATE account_quota_limits
                SET business_calls_1h_limit = ?,
                    daily_credits_limit = ?,
                    monthly_credits_limit = ?,
                    inherits_defaults = 0,
                    updated_at = ?
                WHERE user_id = ?
                "#,
            )
            .bind(defaults.business_calls_1h_limit)
            .bind(defaults.daily_credits_limit)
            .bind(defaults.monthly_credits_limit)
            .bind(now)
            .bind(&user_id)
            .execute(&mut *tx)
            .await?;
            touched_user_ids.push(user_id);
        }
        tx.commit().await?;
        self.invalidate_all_account_quota_resolutions().await;
        self.record_effective_account_quota_snapshots_for_users_at(&touched_user_ids, now)
            .await?;
        Ok(())
    }
}

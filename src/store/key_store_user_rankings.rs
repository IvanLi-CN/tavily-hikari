impl KeyStore {
    pub(crate) async fn fetch_user_rankings_snapshot(
        &self,
        generated_at: i64,
        refresh_interval_secs: i64,
    ) -> Result<UserRankingsSnapshot, ProxyError> {
        self.flush_request_stats_writes().await?;

        let last24h = self
            .fetch_user_ranking_window(generated_at.saturating_sub(SECS_PER_DAY), generated_at)
            .await?;
        let last7d = self
            .fetch_user_ranking_window(
                generated_at.saturating_sub(7 * SECS_PER_DAY),
                generated_at,
            )
            .await?;
        let last30d = self
            .fetch_user_ranking_window(
                generated_at.saturating_sub(30 * SECS_PER_DAY),
                generated_at,
            )
            .await?;

        Ok(UserRankingsSnapshot {
            generated_at,
            refresh_interval_secs,
            last24h,
            last7d,
            last30d,
        })
    }

    async fn fetch_user_ranking_window(
        &self,
        start_at: i64,
        end_at: i64,
    ) -> Result<UserRankingWindow, ProxyError> {
        let primary_success_top = self
            .fetch_user_ranking_rows(
                AccountUsageRollupMetricKind::PrimarySuccess,
                start_at,
                end_at,
            )
            .await?;
        let business_credits_top = self
            .fetch_user_ranking_rows(
                AccountUsageRollupMetricKind::BusinessCredits,
                start_at,
                end_at,
            )
            .await?;

        Ok(UserRankingWindow {
            primary_success_top,
            business_credits_top,
        })
    }

    async fn fetch_user_ranking_rows(
        &self,
        metric_kind: AccountUsageRollupMetricKind,
        start_at: i64,
        end_at: i64,
    ) -> Result<Vec<UserRankingRow>, ProxyError> {
        let bucket_kind = match metric_kind {
            AccountUsageRollupMetricKind::BusinessCredits => AccountUsageRollupBucketKind::Hour,
            AccountUsageRollupMetricKind::PrimarySuccess | AccountUsageRollupMetricKind::RequestCount => {
                AccountUsageRollupBucketKind::FiveMinute
            }
        };

        let bucket_secs = match bucket_kind {
            AccountUsageRollupBucketKind::FiveMinute => SECS_PER_FIVE_MINUTES,
            AccountUsageRollupBucketKind::Hour => SECS_PER_HOUR,
            AccountUsageRollupBucketKind::Day => SECS_PER_DAY,
            AccountUsageRollupBucketKind::UtcDay => SECS_PER_DAY,
            AccountUsageRollupBucketKind::Month => SECS_PER_DAY * 31,
        };

        let first_full_bucket_start = ((start_at + bucket_secs - 1).div_euclid(bucket_secs)) * bucket_secs;
        let final_full_bucket_end = (end_at.div_euclid(bucket_secs)) * bucket_secs;

        let mut totals = self
            .fetch_user_ranking_rollup_totals(metric_kind, bucket_kind, first_full_bucket_start, final_full_bucket_end)
            .await?;

        if start_at < first_full_bucket_start {
            self.apply_user_ranking_partial_range(
                &mut totals,
                metric_kind,
                start_at,
                first_full_bucket_start.min(end_at),
            )
            .await?;
        }

        if final_full_bucket_end < end_at && final_full_bucket_end > first_full_bucket_start {
            self.apply_user_ranking_partial_range(
                &mut totals,
                metric_kind,
                final_full_bucket_end.max(start_at),
                end_at,
            )
            .await?;
        }

        let mut rows = self.fetch_user_ranking_identities(&totals).await?;
        rows.sort_by(|left, right| {
            right
                .value
                .cmp(&left.value)
                .then_with(|| left.user.user_id.cmp(&right.user.user_id))
        });

        for (index, row) in rows.iter_mut().enumerate() {
            row.rank = (index + 1) as i64;
        }
        rows.truncate(20);
        Ok(rows)
    }

    async fn fetch_user_ranking_rollup_totals(
        &self,
        metric_kind: AccountUsageRollupMetricKind,
        bucket_kind: AccountUsageRollupBucketKind,
        bucket_start_at_least: i64,
        bucket_start_before: i64,
    ) -> Result<HashMap<String, i64>, ProxyError> {
        if bucket_start_before <= bucket_start_at_least {
            return Ok(HashMap::new());
        }

        let rows = sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT user_id, COALESCE(SUM(value), 0) AS total
            FROM account_usage_rollup_buckets
            WHERE metric_kind = ?
              AND bucket_kind = ?
              AND bucket_start >= ?
              AND bucket_start < ?
            GROUP BY user_id
            HAVING total > 0
            "#,
        )
        .bind(metric_kind.as_str())
        .bind(bucket_kind.as_str())
        .bind(bucket_start_at_least)
        .bind(bucket_start_before)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().collect())
    }

    async fn apply_user_ranking_partial_range(
        &self,
        totals: &mut HashMap<String, i64>,
        metric_kind: AccountUsageRollupMetricKind,
        start_at: i64,
        end_at: i64,
    ) -> Result<(), ProxyError> {
        if end_at <= start_at {
            return Ok(());
        }

        match metric_kind {
            AccountUsageRollupMetricKind::PrimarySuccess => {
                let rows = sqlx::query_as::<_, (String, i64)>(
                    r#"
                    SELECT
                        COALESCE(
                            atl.request_user_id,
                            CASE
                                WHEN atl.billing_subject LIKE 'account:%' THEN SUBSTR(atl.billing_subject, 9)
                                ELSE NULL
                            END,
                            b.user_id
                        ) AS user_id,
                        COUNT(*) AS total
                    FROM auth_token_logs atl
                    LEFT JOIN user_token_bindings b ON b.token_id = atl.token_id
                    WHERE atl.created_at >= ?
                      AND atl.created_at < ?
                      AND atl.result_status = ?
                      AND COALESCE(
                            atl.request_user_id,
                            CASE
                                WHEN atl.billing_subject LIKE 'account:%' THEN SUBSTR(atl.billing_subject, 9)
                                ELSE NULL
                            END,
                            b.user_id
                        ) IS NOT NULL
                      AND (
                            (atl.request_kind_key LIKE 'api:%')
                            OR atl.request_kind_key IN ('mcp:search', 'mcp:extract', 'mcp:crawl', 'mcp:map', 'mcp:research')
                            OR (atl.request_kind_key = 'mcp:batch' AND COALESCE(atl.counts_business_quota, 0) <> 0)
                      )
                    GROUP BY user_id
                    HAVING total > 0
                    "#,
                )
                .bind(start_at)
                .bind(end_at)
                .bind(OUTCOME_SUCCESS)
                .fetch_all(&self.pool)
                .await?;

                for (user_id, value) in rows {
                    *totals.entry(user_id).or_default() += value;
                }
            }
            AccountUsageRollupMetricKind::BusinessCredits => {
                let rows = sqlx::query_as::<_, (String, i64)>(
                    r#"
                    WITH charged_rows AS (
                        SELECT
                            bl.billing_subject,
                            bl.created_at,
                            COALESCE(bl.business_credits, 0) AS business_credits
                        FROM billing_ledger bl
                        WHERE bl.billing_state = ?
                          AND COALESCE(bl.business_credits, 0) > 0
                          AND bl.billing_subject LIKE 'account:%'
                          AND bl.created_at >= ?
                          AND bl.created_at < ?
                        UNION ALL
                        SELECT
                            atl.billing_subject,
                            atl.created_at,
                            COALESCE(atl.business_credits, 0) AS business_credits
                        FROM auth_token_logs atl
                        LEFT JOIN billing_ledger bl ON bl.auth_token_log_id = atl.id
                        WHERE bl.auth_token_log_id IS NULL
                          AND atl.billing_state = ?
                          AND COALESCE(atl.business_credits, 0) > 0
                          AND atl.billing_subject LIKE 'account:%'
                          AND atl.created_at >= ?
                          AND atl.created_at < ?
                    )
                    SELECT SUBSTR(billing_subject, 9) AS user_id, SUM(business_credits) AS total
                    FROM charged_rows
                    GROUP BY user_id
                    HAVING total > 0
                    "#,
                )
                .bind(BILLING_STATE_CHARGED)
                .bind(start_at)
                .bind(end_at)
                .bind(BILLING_STATE_CHARGED)
                .bind(start_at)
                .bind(end_at)
                .fetch_all(&self.pool)
                .await?;

                for (user_id, value) in rows {
                    *totals.entry(user_id).or_default() += value;
                }
            }
            AccountUsageRollupMetricKind::RequestCount => {}
        }

        Ok(())
    }

    async fn fetch_user_ranking_identities(
        &self,
        totals: &HashMap<String, i64>,
    ) -> Result<Vec<UserRankingRow>, ProxyError> {
        if totals.is_empty() {
            return Ok(Vec::new());
        }

        let mut user_ids: Vec<&String> = totals.keys().collect();
        user_ids.sort();

        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, display_name, username, avatar_template FROM users WHERE id IN (",
        );
        let mut separated = builder.separated(", ");
        for user_id in &user_ids {
            separated.push_bind(user_id.as_str());
        }
        separated.push_unseparated(")");

        let rows = builder.build().fetch_all(&self.pool).await?;
        let mut by_id: HashMap<String, UserRankingIdentity> = HashMap::new();
        for row in rows {
            let user_id: String = row.try_get("id")?;
            by_id.insert(
                user_id.clone(),
                UserRankingIdentity {
                    user_id,
                    display_name: row.try_get("display_name")?,
                    username: row.try_get("username")?,
                    avatar_template: row.try_get("avatar_template")?,
                },
            );
        }

        let mut ranking_rows = Vec::with_capacity(user_ids.len());
        for user_id in user_ids {
            let value = *totals.get(user_id).unwrap_or(&0);
            if value <= 0 {
                continue;
            }
            let user = by_id.remove(user_id.as_str()).unwrap_or(UserRankingIdentity {
                user_id: user_id.clone(),
                display_name: None,
                username: None,
                avatar_template: None,
            });
            ranking_rows.push(UserRankingRow { rank: 0, value, user });
        }

        Ok(ranking_rows)
    }
}

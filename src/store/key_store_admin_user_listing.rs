impl KeyStore {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn list_admin_users_sorted_paged(
        &self,
        page: i64,
        per_page: i64,
        query: Option<&str>,
        tag_id: Option<&str>,
        sort: AdminUserListSortField,
        direction: AdminListSortDirection,
        hour_window_start: i64,
        day_window_start: i64,
        day_window_end: i64,
        month_start: i64,
        recent_ip_since: i64,
    ) -> Result<(Vec<AdminUserIdentity>, i64), ProxyError> {
        let _permit = self
            .admin_heavy_read_semaphore
            .acquire()
            .await
            .expect("admin heavy read semaphore is never closed");
        let page = page.max(1);
        let per_page = per_page.clamp(1, 100);
        let offset = (page - 1) * per_page;
        let search = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let tag_id = tag_id.map(str::trim).filter(|value| !value.is_empty());

        let mut count_builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM users u WHERE ");
        Self::push_admin_user_filters(&mut count_builder, tag_id, search.as_deref());
        let total = count_builder
            .build_query_scalar::<i64>()
            .fetch_one(&self.pool)
            .await?;
        let monthly_broken_base_limit = self.fetch_user_blocked_key_base_limit().await?;

        let mut builder = QueryBuilder::<Sqlite>::new(
            "WITH filtered_users AS (SELECT u.id FROM users u WHERE ",
        );
        Self::push_admin_user_filters(&mut builder, tag_id, search.as_deref());
        builder.push(
            "), metric(user_id, sort_value, sort_limit, sort_total, sort_failure) AS (",
        );
        match sort {
            AdminUserListSortField::QuotaHourlyUsed => {
                builder.push(
                    "SELECT aub.user_id, COALESCE(SUM(aub.count), 0), 0, 0, 0 \
                     FROM account_usage_buckets aub \
                     JOIN filtered_users fu ON fu.id = aub.user_id \
                     WHERE aub.granularity = ",
                );
                builder.push_bind(GRANULARITY_MINUTE);
                builder.push(" AND aub.bucket_start >= ");
                builder.push_bind(hour_window_start);
                builder.push(" GROUP BY aub.user_id");
            }
            AdminUserListSortField::QuotaDailyUsed => {
                builder.push(
                    "SELECT user_id, COALESCE(SUM(total), 0), 0, 0, 0 FROM (\
                     SELECT aub.user_id, SUM(aub.count) AS total \
                     FROM account_usage_buckets aub \
                     JOIN filtered_users fu ON fu.id = aub.user_id \
                     WHERE aub.granularity = ",
                );
                builder.push_bind(GRANULARITY_DAY);
                builder.push(" AND aub.bucket_start >= ");
                builder.push_bind(day_window_start);
                builder.push(" GROUP BY aub.user_id UNION ALL SELECT aub.user_id, SUM(aub.count) AS total FROM account_usage_buckets aub JOIN filtered_users fu ON fu.id = aub.user_id WHERE aub.granularity = ");
                builder.push_bind(GRANULARITY_HOUR);
                builder.push(" AND aub.bucket_start >= ");
                builder.push_bind(day_window_start);
                builder.push(" AND aub.bucket_start < ");
                builder.push_bind(day_window_end);
                builder.push(" GROUP BY aub.user_id) GROUP BY user_id");
            }
            AdminUserListSortField::QuotaMonthlyUsed => {
                builder.push("SELECT amq.user_id, CASE WHEN amq.month_start >= ");
                builder.push_bind(month_start);
                builder.push(
                    " THEN amq.month_count ELSE 0 END, 0, 0, 0 \
                     FROM account_monthly_quota amq \
                     JOIN filtered_users fu ON fu.id = amq.user_id",
                );
            }
            AdminUserListSortField::DailySuccessRate => {
                builder.push(
                    "SELECT b.user_id, \
                     COALESCE(SUM(CASE WHEN l.result_status = ",
                );
                builder.push_bind(OUTCOME_SUCCESS);
                builder.push(" THEN 1 ELSE 0 END), 0), 0, COUNT(*), COALESCE(SUM(CASE WHEN l.result_status = ");
                builder.push_bind(OUTCOME_ERROR);
                builder.push(
                    " THEN 1 ELSE 0 END), 0) \
                     FROM filtered_users fu \
                     JOIN user_token_bindings b ON b.user_id = fu.id \
                     JOIN auth_token_logs l INDEXED BY idx_token_logs_token_time ON l.token_id = b.token_id \
                     WHERE l.created_at >= ",
                );
                builder.push_bind(day_window_start);
                builder.push(" AND l.created_at < ");
                builder.push_bind(day_window_end);
                builder.push(" AND l.result_status IN (");
                builder.push_bind(OUTCOME_SUCCESS);
                builder.push(", ");
                builder.push_bind(OUTCOME_ERROR);
                builder.push(") GROUP BY b.user_id");
            }
            AdminUserListSortField::MonthlySuccessRate => {
                builder.push(
                    "SELECT b.user_id, \
                     COALESCE(SUM(CASE WHEN l.result_status = ",
                );
                builder.push_bind(OUTCOME_SUCCESS);
                builder.push(" THEN 1 ELSE 0 END), 0), 0, COUNT(*), COALESCE(SUM(CASE WHEN l.result_status = ");
                builder.push_bind(OUTCOME_ERROR);
                builder.push(
                    " THEN 1 ELSE 0 END), 0) \
                     FROM filtered_users fu \
                     JOIN user_token_bindings b ON b.user_id = fu.id \
                     JOIN auth_token_logs l INDEXED BY idx_token_logs_token_time ON l.token_id = b.token_id \
                     WHERE l.created_at >= ",
                );
                builder.push_bind(month_start);
                builder.push(" AND l.result_status IN (");
                builder.push_bind(OUTCOME_SUCCESS);
                builder.push(", ");
                builder.push_bind(OUTCOME_ERROR);
                builder.push(") GROUP BY b.user_id");
            }
            AdminUserListSortField::MonthlyBrokenCount => {
                builder.push(
                    "SELECT skb.subject_id, COUNT(*), 0, 0, 0 \
                     FROM subject_key_breakages skb \
                     JOIN filtered_users fu ON fu.id = skb.subject_id \
                     JOIN api_keys ak ON ak.id = skb.key_id AND ak.deleted_at IS NULL \
                     LEFT JOIN api_key_quarantines aq ON aq.key_id = ak.id AND aq.cleared_at IS NULL \
                     WHERE skb.subject_kind = ",
                );
                builder.push_bind(BROKEN_KEY_SUBJECT_USER);
                builder.push(" AND skb.month_start = ");
                builder.push_bind(month_start);
                builder.push(" AND aq.reason_code IN (");
                builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
                builder.push(", ");
                builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
                builder.push(", ");
                builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
                builder.push(") AND skb.reason_code IN (");
                builder.push_bind(BLOCKED_KEY_REASON_ACCOUNT_DEACTIVATED);
                builder.push(", ");
                builder.push_bind(BLOCKED_KEY_REASON_KEY_REVOKED);
                builder.push(", ");
                builder.push_bind(BLOCKED_KEY_REASON_INVALID_API_KEY);
                builder.push(") GROUP BY skb.subject_id");
            }
            AdminUserListSortField::RecentIpCount7d => {
                builder.push(
                    "SELECT rl.request_user_id, COUNT(DISTINCT rl.client_ip), 0, 0, 0 \
                     FROM request_logs rl INDEXED BY idx_request_logs_user_ip_time \
                     JOIN filtered_users fu ON fu.id = rl.request_user_id \
                     WHERE rl.visibility = ",
                );
                builder.push_bind(REQUEST_LOG_VISIBILITY_VISIBLE);
                builder.push(" AND rl.created_at >= ");
                builder.push_bind(recent_ip_since);
                builder.push(
                    " AND rl.client_ip IS NOT NULL AND TRIM(rl.client_ip) != '' \
                     GROUP BY rl.request_user_id",
                );
            }
            AdminUserListSortField::LastActivity => {
                builder.push(
                    "SELECT b.user_id, MAX(l.created_at), 0, 0, 0 \
                     FROM filtered_users fu \
                     JOIN user_token_bindings b ON b.user_id = fu.id \
                     JOIN auth_token_logs l INDEXED BY idx_token_logs_token_time ON l.token_id = b.token_id \
                     GROUP BY b.user_id",
                );
            }
            AdminUserListSortField::LastLoginAt => {
                builder.push("SELECT NULL, NULL, NULL, NULL, NULL WHERE 0");
            }
        }
        builder.push(
            "), quota_limits AS (\
             SELECT fu.id AS user_id, \
             COALESCE((SELECT s.hourly_limit FROM account_quota_limit_snapshots s WHERE s.user_id = fu.id ORDER BY s.changed_at DESC, s.id DESC LIMIT 1), 0) AS hourly_limit, \
             COALESCE((SELECT s.daily_limit FROM account_quota_limit_snapshots s WHERE s.user_id = fu.id ORDER BY s.changed_at DESC, s.id DESC LIMIT 1), 0) AS daily_limit, \
             COALESCE((SELECT s.monthly_limit FROM account_quota_limit_snapshots s WHERE s.user_id = fu.id ORDER BY s.changed_at DESC, s.id DESC LIMIT 1), 0) AS monthly_limit \
             FROM filtered_users fu\
             ), broken_limits AS (\
             SELECT fu.id AS user_id, MAX(0, ",
        );
        builder.push_bind(monthly_broken_base_limit);
        builder.push(
            " + COALESCE(aql.monthly_blocked_key_limit_delta, aql.monthly_broken_limit - ",
        );
        builder.push_bind(USER_MONTHLY_BROKEN_LIMIT_DEFAULT);
        builder.push(
            ", 0)) AS monthly_broken_limit \
             FROM filtered_users fu \
             LEFT JOIN account_quota_limits aql ON aql.user_id = fu.id \
             GROUP BY fu.id\
             ) SELECT u.id, u.display_name, u.username, u.active, u.last_login_at, \
             COALESCE(COUNT(b.token_id), 0) AS token_count \
             FROM users u \
             JOIN filtered_users fu ON fu.id = u.id \
             LEFT JOIN user_token_bindings b ON b.user_id = u.id \
             LEFT JOIN metric m ON m.user_id = u.id \
             LEFT JOIN quota_limits ql ON ql.user_id = u.id \
             LEFT JOIN broken_limits bl ON bl.user_id = u.id \
             GROUP BY u.id, u.display_name, u.username, u.active, u.last_login_at, \
             ql.hourly_limit, ql.daily_limit, ql.monthly_limit, bl.monthly_broken_limit",
        );
        let direction_sql = Self::admin_user_sort_direction_sql(direction);
        let null_order_sql = Self::admin_user_timestamp_null_order_sql(direction);
        match sort {
            AdminUserListSortField::DailySuccessRate
            | AdminUserListSortField::MonthlySuccessRate => {
                builder.push(" ORDER BY (COALESCE(m.sort_total, 0) = 0) ASC, (CAST(COALESCE(m.sort_value, 0) AS REAL) / NULLIF(m.sort_total, 0)) ");
                builder.push(direction_sql);
                builder.push(", COALESCE(m.sort_failure, 0) ASC, u.id ASC");
            }
            AdminUserListSortField::LastActivity => {
                builder.push(" ORDER BY (m.sort_value IS NULL) ");
                builder.push(null_order_sql);
                builder.push(", m.sort_value ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
            AdminUserListSortField::LastLoginAt => {
                builder.push(" ORDER BY (u.last_login_at IS NULL) ");
                builder.push(null_order_sql);
                builder.push(", u.last_login_at ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
            AdminUserListSortField::QuotaHourlyUsed => {
                builder.push(" ORDER BY COALESCE(m.sort_value, 0) ");
                builder.push(direction_sql);
                builder.push(", COALESCE(ql.hourly_limit, 0) ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
            AdminUserListSortField::QuotaDailyUsed => {
                builder.push(" ORDER BY COALESCE(m.sort_value, 0) ");
                builder.push(direction_sql);
                builder.push(", COALESCE(ql.daily_limit, 0) ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
            AdminUserListSortField::QuotaMonthlyUsed => {
                builder.push(" ORDER BY COALESCE(m.sort_value, 0) ");
                builder.push(direction_sql);
                builder.push(", COALESCE(ql.monthly_limit, 0) ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
            AdminUserListSortField::MonthlyBrokenCount => {
                builder.push(" ORDER BY COALESCE(m.sort_value, 0) ");
                builder.push(direction_sql);
                builder.push(", COALESCE(bl.monthly_broken_limit, ");
                builder.push_bind(monthly_broken_base_limit);
                builder.push(") ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
            _ => {
                builder.push(" ORDER BY COALESCE(m.sort_value, 0) ");
                builder.push(direction_sql);
                builder.push(", u.id ASC");
            }
        }
        builder.push(" LIMIT ");
        builder.push_bind(per_page);
        builder.push(" OFFSET ");
        builder.push_bind(offset);

        let rows = builder
            .build_query_as::<(
                String,
                Option<String>,
                Option<String>,
                i64,
                Option<i64>,
                i64,
            )>()
            .fetch_all(&self.pool)
            .await?;
        let items = rows
            .into_iter()
            .map(
                |(user_id, display_name, username, active, last_login_at, token_count)| {
                    AdminUserIdentity {
                        user_id,
                        display_name,
                        username,
                        active: active == 1,
                        last_login_at,
                        token_count,
                    }
                },
            )
            .collect();
        Ok((items, total))
    }
}

impl KeyStore {
    #[allow(dead_code)]
    pub(crate) async fn daily_reconciliation_progress(
        &self,
    ) -> Result<(
        DailyReconciliationProgress,
        Vec<DailyReconciliationKeyProgress>,
    ), ProxyError> {
        let now = self.backend_time.now_ts();
        let day_window = server_local_day_window_utc(self.backend_time.now_utc().with_timezone(&Local));
        let (observed_accounts, accounts_with_settled_period, fully_terminal_accounts, observed_periods, settled_periods, degraded_periods, pending_periods) =
            sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64)>(
                r#"
                WITH windows AS (
                    SELECT
                        u.token_id,
                        u.period_code,
                        MIN(u.billing_subject) AS billing_subject,
                        MAX(CASE WHEN s.status = 'settled' THEN 1 ELSE 0 END) AS settled,
                        MAX(CASE WHEN s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded') THEN 1 ELSE 0 END) AS terminal,
                        MAX(CASE WHEN s.status IN ('degraded', 'shadow_degraded') THEN 1 ELSE 0 END) AS degraded
                    FROM upstream_reconciliation_usage u
                    LEFT JOIN upstream_reconciliation_settlements s
                      ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
                    WHERE u.period_start >= ? AND u.period_start < ?
                    GROUP BY u.token_id, u.period_code
                ), accounts AS (
                    SELECT
                        billing_subject,
                        COUNT(*) AS observed,
                        SUM(settled) AS settled,
                        SUM(terminal) AS terminal,
                        SUM(degraded) AS degraded
                    FROM windows
                    WHERE billing_subject LIKE 'account:%'
                    GROUP BY billing_subject
                )
                SELECT
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN settled > 0 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN terminal = observed THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(observed), 0),
                    COALESCE(SUM(settled), 0),
                    COALESCE(SUM(degraded), 0),
                    COALESCE(SUM(observed - terminal), 0)
                FROM accounts
                "#,
            )
            .bind(day_window.start)
            .bind(day_window.end)
            .fetch_one(&self.pool)
            .await?;
        let (research_total, research_terminal, research_pending, research_unavailable, research_pollable_pending) =
            sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
                r#"
                SELECT
                    COUNT(DISTINCT r.request_id),
                    COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                    COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END),
                    COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL AND r.poll_resolution = 'unavailable' THEN r.request_id END),
                    COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL AND r.poll_resolution = 'pollable' THEN r.request_id END)
                FROM upstream_reconciliation_research r
                JOIN upstream_reconciliation_usage u
                  ON u.token_id = r.token_id AND u.period_code = r.period_code
                WHERE u.period_start >= ? AND u.period_start < ?
                "#,
            )
            .bind(day_window.start)
            .bind(day_window.end)
            .fetch_one(&self.pool)
            .await?;
        let key_rows = sqlx::query_as::<_, (String, i64, i64, i64)>(
            r#"
            SELECT
                u.key_id,
                COUNT(DISTINCT CASE WHEN r.terminal_at IS NOT NULL THEN r.request_id END),
                COUNT(DISTINCT CASE WHEN r.terminal_at IS NULL THEN r.request_id END),
                COUNT(DISTINCT CASE
                    WHEN s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited')
                    THEN u.project_id
                END)
            FROM upstream_reconciliation_usage u
            LEFT JOIN upstream_reconciliation_research r
              ON r.token_id = u.token_id AND r.period_code = u.period_code AND r.key_id = u.key_id
            LEFT JOIN upstream_reconciliation_settlements s
              ON s.settlement_key = 'v1:' || u.token_id || ':' || u.period_code
            WHERE u.period_start >= ? AND u.period_start < ?
            GROUP BY u.key_id
            HAVING COUNT(DISTINCT r.request_id) > 0
                OR COUNT(DISTINCT CASE
                    WHEN s.settlement_key IS NULL OR s.status IN ('pending', 'waiting', 'rate_limited')
                    THEN u.project_id
                END) > 0
            ORDER BY 3 DESC, 4 DESC, u.key_id ASC
            "#,
        )
        .bind(day_window.start)
        .bind(day_window.end)
        .fetch_all(&self.pool)
        .await?;
        let backoffs = sqlx::query_as::<_, (String, i64, Option<String>)>(
            r#"
            SELECT key_id, cooldown_until, reason_code
            FROM api_key_transient_backoffs
            WHERE scope IN ('period_reconciliation', 'reconciliation_research_credentials')
              AND cooldown_until > ?
            ORDER BY cooldown_until DESC
            "#,
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(key_id, cooldown_until, reason_code)| {
            (key_id, (cooldown_until, reason_code))
        })
        .collect::<HashMap<_, _>>();
        let progress = DailyReconciliationProgress {
            observed_accounts,
            accounts_with_settled_period,
            fully_terminal_accounts,
            observed_periods,
            settled_periods,
            degraded_periods,
            pending_periods,
            research_total,
            research_terminal,
            research_pending,
            research_unavailable,
            research_pollable_pending,
        };
        let by_key = key_rows
            .into_iter()
            .map(|(key_id, terminal_research, pending_research, pending_project_ids)| {
                let cooldown = backoffs.get(&key_id);
                DailyReconciliationKeyProgress {
                    key_id_hint: key_id.chars().take(12).collect(),
                    terminal_research,
                    pending_research,
                    pending_project_ids,
                    cooldown_until: cooldown.map(|(until, _)| *until),
                    cooldown_reason: cooldown.and_then(|(_, reason)| reason.clone()),
                }
            })
            .collect();
        Ok((progress, by_key))
    }
}

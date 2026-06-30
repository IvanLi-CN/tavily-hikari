#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinuxDoCreditRechargeQuoteSnapshot {
    quote: LinuxDoCreditRechargeQuote,
}

fn parse_linuxdo_credit_recharge_quote_snapshot(
    raw: &str,
) -> Option<LinuxDoCreditRechargeQuote> {
    serde_json::from_str::<LinuxDoCreditRechargeQuoteSnapshot>(raw)
        .map(|snapshot| snapshot.quote)
        .or_else(|_| serde_json::from_str::<LinuxDoCreditRechargeQuote>(raw))
        .ok()
}

fn legacy_recharge_quote_month_start(created_at: i64, paid_at: Option<i64>) -> i64 {
    let anchor_ts = paid_at.unwrap_or(created_at);
    let local_time = Utc
        .timestamp_opt(anchor_ts, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .with_timezone(&Local);
    start_of_local_month_utc_ts(local_time)
}

impl KeyStore {
    pub(crate) async fn ensure_linuxdo_credit_recharge_schema(&self) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS linuxdo_credit_recharge_orders (
                out_trade_no TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                status TEXT NOT NULL,
                credits INTEGER NOT NULL,
                months INTEGER NOT NULL,
                money_cents INTEGER NOT NULL,
                quote_month_start INTEGER NOT NULL DEFAULT 0,
                final_money_cents INTEGER NOT NULL DEFAULT 0,
                final_hourly_delta INTEGER NOT NULL DEFAULT 0,
                final_daily_delta INTEGER NOT NULL DEFAULT 0,
                final_monthly_delta INTEGER NOT NULL DEFAULT 0,
                month_end_clamp_applied INTEGER NOT NULL DEFAULT 0,
                quote_snapshot_json TEXT,
                trade_no TEXT,
                payment_url TEXT,
                order_name TEXT NOT NULL,
                notify_payload TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                paid_at INTEGER,
                last_notify_at INTEGER,
                last_error TEXT,
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_linuxdo_credit_recharge_orders_user_time
               ON linuxdo_credit_recharge_orders(user_id, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        for (column, ty) in [
            ("quote_month_start", "INTEGER NOT NULL DEFAULT 0"),
            ("final_money_cents", "INTEGER NOT NULL DEFAULT 0"),
            ("final_hourly_delta", "INTEGER NOT NULL DEFAULT 0"),
            ("final_daily_delta", "INTEGER NOT NULL DEFAULT 0"),
            ("final_monthly_delta", "INTEGER NOT NULL DEFAULT 0"),
            ("month_end_clamp_applied", "INTEGER NOT NULL DEFAULT 0"),
            ("quote_snapshot_json", "TEXT"),
            ("refunded_at", "INTEGER"),
            ("refund_actor", "TEXT"),
            ("refund_payload", "TEXT"),
        ] {
            if !self
                .table_column_exists("linuxdo_credit_recharge_orders", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE linuxdo_credit_recharge_orders ADD COLUMN {column} {ty}"
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_linuxdo_credit_recharge_orders_status_time
               ON linuxdo_credit_recharge_orders(status, created_at DESC)"#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS linuxdo_credit_recharge_entitlements (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                out_trade_no TEXT NOT NULL,
                user_id TEXT NOT NULL,
                month_start INTEGER NOT NULL,
                credits INTEGER NOT NULL,
                hourly_delta INTEGER NOT NULL DEFAULT 0,
                daily_delta INTEGER NOT NULL DEFAULT 0,
                monthly_delta INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                UNIQUE(out_trade_no, month_start),
                FOREIGN KEY (out_trade_no) REFERENCES linuxdo_credit_recharge_orders(out_trade_no),
                FOREIGN KEY (user_id) REFERENCES users(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"CREATE INDEX IF NOT EXISTS idx_linuxdo_credit_recharge_entitlements_user_month
               ON linuxdo_credit_recharge_entitlements(user_id, month_start)"#,
        )
        .execute(&self.pool)
        .await?;
        for (column, ty) in [
            ("hourly_delta", "INTEGER NOT NULL DEFAULT 0"),
            ("daily_delta", "INTEGER NOT NULL DEFAULT 0"),
            ("monthly_delta", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !self
                .table_column_exists("linuxdo_credit_recharge_entitlements", column)
                .await?
            {
                sqlx::query(&format!(
                    "ALTER TABLE linuxdo_credit_recharge_entitlements ADD COLUMN {column} {ty}"
                ))
                .execute(&self.pool)
                .await?;
            }
        }
        self.backfill_linuxdo_credit_recharge_orders_v1().await?;
        self.backfill_linuxdo_credit_recharge_entitlements_v1().await?;
        Ok(())
    }

    async fn backfill_linuxdo_credit_recharge_orders_v1(&self) -> Result<(), ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT out_trade_no, credits, months, money_cents, status, created_at, paid_at
            FROM linuxdo_credit_recharge_orders
            WHERE quote_month_start = 0
            ORDER BY created_at ASC, out_trade_no ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let out_trade_no: String = row.try_get("out_trade_no")?;
            let credits: i64 = row.try_get("credits")?;
            let delta = linuxdo_credit_recharge_quota_delta(credits);
            let money_cents: i64 = row.try_get("money_cents")?;
            let quote_month_start = legacy_recharge_quote_month_start(
                row.try_get("created_at")?,
                row.try_get("paid_at").unwrap_or(None),
            );
            let snapshot = serde_json::json!({
                "version": 1,
                "source": "backfill",
                "requestedCredits": credits,
                "requestedMonths": row.try_get::<i64, _>("months")?,
                "quoteMonthStart": quote_month_start,
                "finalMoneyCents": money_cents,
                "finalHourlyDelta": delta.hourly_delta,
                "finalDailyDelta": delta.daily_delta,
                "finalMonthlyDelta": delta.monthly_delta,
                "monthEndClampApplied": false,
                "status": row.try_get::<String, _>("status")?,
            });
            sqlx::query(
                r#"
                UPDATE linuxdo_credit_recharge_orders
                   SET quote_month_start = ?,
                       final_money_cents = ?,
                       final_hourly_delta = ?,
                       final_daily_delta = ?,
                       final_monthly_delta = ?,
                       month_end_clamp_applied = 0,
                       quote_snapshot_json = ?,
                       updated_at = MAX(updated_at, ?)
                 WHERE out_trade_no = ?
                "#,
            )
            .bind(quote_month_start)
            .bind(money_cents)
            .bind(delta.hourly_delta)
            .bind(delta.daily_delta)
            .bind(delta.monthly_delta)
            .bind(snapshot.to_string())
            .bind(self.backend_time.now_ts())
            .bind(out_trade_no)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn backfill_linuxdo_credit_recharge_entitlements_v1(&self) -> Result<(), ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT id, credits, hourly_delta, daily_delta, monthly_delta
            FROM linuxdo_credit_recharge_entitlements
            WHERE hourly_delta = 0 AND daily_delta = 0 AND monthly_delta = 0
            ORDER BY id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let entitlement_id: i64 = row.try_get("id")?;
            let delta = linuxdo_credit_recharge_quota_delta(row.try_get("credits")?);
            sqlx::query(
                r#"
                UPDATE linuxdo_credit_recharge_entitlements
                   SET hourly_delta = ?, daily_delta = ?, monthly_delta = ?
                 WHERE id = ?
                "#,
            )
            .bind(delta.hourly_delta)
            .bind(delta.daily_delta)
            .bind(delta.monthly_delta)
            .bind(entitlement_id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    fn linuxdo_credit_recharge_order_from_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<LinuxDoCreditRechargeOrder, ProxyError> {
        Ok(LinuxDoCreditRechargeOrder {
            out_trade_no: row.try_get("out_trade_no")?,
            user_id: row.try_get("user_id")?,
            status: row.try_get("status")?,
            credits: row.try_get("credits")?,
            months: row.try_get("months")?,
            money_cents: row.try_get("money_cents")?,
            quote_month_start: row.try_get("quote_month_start").unwrap_or(0),
            final_money_cents: row.try_get("final_money_cents").unwrap_or(0),
            final_hourly_delta: row.try_get("final_hourly_delta").unwrap_or(0),
            final_daily_delta: row.try_get("final_daily_delta").unwrap_or(0),
            final_monthly_delta: row.try_get("final_monthly_delta").unwrap_or(0),
            month_end_clamp_applied: row
                .try_get::<i64, _>("month_end_clamp_applied")
                .map(|value| value != 0)
                .unwrap_or(false),
            quote_snapshot_json: row.try_get("quote_snapshot_json").unwrap_or(None),
            trade_no: row.try_get("trade_no")?,
            payment_url: row.try_get("payment_url")?,
            order_name: row.try_get("order_name")?,
            notify_payload: row.try_get("notify_payload")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            paid_at: row.try_get("paid_at")?,
            refunded_at: row.try_get("refunded_at").unwrap_or(None),
            refund_actor: row.try_get("refund_actor").unwrap_or(None),
            refund_payload: row.try_get("refund_payload").unwrap_or(None),
            last_notify_at: row.try_get("last_notify_at")?,
            last_error: row.try_get("last_error")?,
        })
    }

    fn linuxdo_credit_recharge_entitlement_from_row(
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<LinuxDoCreditRechargeEntitlement, ProxyError> {
        Ok(LinuxDoCreditRechargeEntitlement {
            id: row.try_get("id")?,
            out_trade_no: row.try_get("out_trade_no")?,
            user_id: row.try_get("user_id")?,
            month_start: row.try_get("month_start")?,
            credits: row.try_get("credits")?,
            hourly_delta: row.try_get("hourly_delta").unwrap_or(0),
            daily_delta: row.try_get("daily_delta").unwrap_or(0),
            monthly_delta: row.try_get("monthly_delta").unwrap_or(0),
            created_at: row.try_get("created_at")?,
        })
    }

    pub(crate) async fn create_linuxdo_credit_recharge_order(
        &self,
        order: &LinuxDoCreditRechargeOrder,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            INSERT INTO linuxdo_credit_recharge_orders (
                out_trade_no, user_id, status, credits, months, money_cents,
                quote_month_start, final_money_cents, final_hourly_delta, final_daily_delta,
                final_monthly_delta, month_end_clamp_applied, quote_snapshot_json,
                trade_no, payment_url, order_name, notify_payload, created_at, updated_at,
                paid_at, refunded_at, refund_actor, refund_payload, last_notify_at, last_error
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&order.out_trade_no)
        .bind(&order.user_id)
        .bind(&order.status)
        .bind(order.credits)
        .bind(order.months)
        .bind(order.money_cents)
        .bind(order.quote_month_start)
        .bind(order.final_money_cents)
        .bind(order.final_hourly_delta)
        .bind(order.final_daily_delta)
        .bind(order.final_monthly_delta)
        .bind(if order.month_end_clamp_applied { 1 } else { 0 })
        .bind(&order.quote_snapshot_json)
        .bind(&order.trade_no)
        .bind(&order.payment_url)
        .bind(&order.order_name)
        .bind(&order.notify_payload)
        .bind(order.created_at)
        .bind(order.updated_at)
        .bind(order.paid_at)
        .bind(order.refunded_at)
        .bind(&order.refund_actor)
        .bind(&order.refund_payload)
        .bind(order.last_notify_at)
        .bind(&order.last_error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn update_linuxdo_credit_recharge_order_payment_url(
        &self,
        out_trade_no: &str,
        payment_url: &str,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE linuxdo_credit_recharge_orders
               SET payment_url = ?, updated_at = ?
             WHERE out_trade_no = ?
            "#,
        )
        .bind(payment_url)
        .bind(updated_at)
        .bind(out_trade_no)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn mark_linuxdo_credit_recharge_order_failed(
        &self,
        out_trade_no: &str,
        message: &str,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE linuxdo_credit_recharge_orders
               SET status = ?, last_error = ?, updated_at = ?
             WHERE out_trade_no = ? AND status = ?
            "#,
        )
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_FAILED)
        .bind(message)
        .bind(updated_at)
        .bind(out_trade_no)
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_PENDING)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn fetch_linuxdo_credit_recharge_order(
        &self,
        out_trade_no: &str,
    ) -> Result<Option<LinuxDoCreditRechargeOrder>, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT *
            FROM linuxdo_credit_recharge_orders
            WHERE out_trade_no = ?
            LIMIT 1
            "#,
        )
        .bind(out_trade_no)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref()
            .map(Self::linuxdo_credit_recharge_order_from_row)
            .transpose()
    }

    pub(crate) async fn list_linuxdo_credit_recharge_orders_for_user(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<LinuxDoCreditRechargeOrder>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM linuxdo_credit_recharge_orders
            WHERE user_id = ?
            ORDER BY created_at DESC, out_trade_no DESC
            LIMIT ?
            "#,
        )
        .bind(user_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(Self::linuxdo_credit_recharge_order_from_row)
            .collect()
    }

    pub(crate) async fn has_linuxdo_credit_recharge_orders(&self) -> Result<bool, ProxyError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM linuxdo_credit_recharge_orders LIMIT 1",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count > 0)
    }

    pub(crate) async fn count_admin_linuxdo_credit_recharge_orders(
        &self,
        query: &LinuxDoCreditRechargeAdminListQuery,
    ) -> Result<i64, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(*) FROM linuxdo_credit_recharge_orders o LEFT JOIN users u ON u.id = o.user_id WHERE 1 = 1",
        );
        push_admin_recharge_filters(
            &mut builder,
            query.user_query.as_deref(),
            query.status.as_deref(),
            query.start_at,
            query.end_at,
        );
        builder
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn count_admin_linuxdo_credit_recharge_user_groups(
        &self,
        query: &LinuxDoCreditRechargeAdminListQuery,
    ) -> Result<i64, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT COUNT(DISTINCT o.user_id) FROM linuxdo_credit_recharge_orders o LEFT JOIN users u ON u.id = o.user_id WHERE 1 = 1",
        );
        push_admin_recharge_filters(
            &mut builder,
            query.user_query.as_deref(),
            query.status.as_deref(),
            query.start_at,
            query.end_at,
        );
        builder
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(ProxyError::Database)
    }

    pub(crate) async fn list_admin_linuxdo_credit_recharge_orders(
        &self,
        query: &LinuxDoCreditRechargeAdminListQuery,
    ) -> Result<Vec<LinuxDoCreditRechargeAdminOrder>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT o.*, u.display_name AS user_display_name, u.username AS user_username, u.avatar_template AS user_avatar_template FROM linuxdo_credit_recharge_orders o LEFT JOIN users u ON u.id = o.user_id WHERE 1 = 1",
        );
        push_admin_recharge_filters(
            &mut builder,
            query.user_query.as_deref(),
            query.status.as_deref(),
            query.start_at,
            query.end_at,
        );
        builder.push(" ORDER BY ");
        match query.sort.as_str() {
            "paidAt" => builder.push("o.paid_at"),
            "refundedAt" => builder.push("o.refunded_at"),
            "status" => builder.push("o.status"),
            _ => builder.push("o.created_at"),
        };
        if query.order.eq_ignore_ascii_case("asc") {
            builder.push(" ASC");
        } else {
            builder.push(" DESC");
        }
        builder.push(", o.out_trade_no DESC LIMIT ");
        builder.push_bind(query.per_page.clamp(1, 100));
        builder.push(" OFFSET ");
        builder.push_bind((query.page.max(1) - 1) * query.per_page.clamp(1, 100));
        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                Ok(LinuxDoCreditRechargeAdminOrder {
                    order: Self::linuxdo_credit_recharge_order_from_row(row)?,
                    user_display_name: row.try_get("user_display_name")?,
                    user_username: row.try_get("user_username")?,
                    user_avatar_template: row.try_get("user_avatar_template")?,
                })
            })
            .collect()
    }

    pub(crate) async fn list_admin_linuxdo_credit_recharge_user_groups(
        &self,
        query: &LinuxDoCreditRechargeAdminListQuery,
    ) -> Result<Vec<LinuxDoCreditRechargeAdminUserGroup>, ProxyError> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT o.user_id, u.display_name AS user_display_name, u.username AS user_username, u.avatar_template AS user_avatar_template, COUNT(*) AS order_count, SUM(CASE WHEN o.status = 'paid' THEN 1 ELSE 0 END) AS paid_order_count, SUM(CASE WHEN o.status IN ('refunded', 'refundOnly') THEN 1 ELSE 0 END) AS refunded_order_count, COALESCE(SUM(o.credits * o.months), 0) AS total_credits, COALESCE(SUM(o.final_money_cents), 0) AS total_money_cents, MAX(o.created_at) AS latest_order_created_at, MAX(o.paid_at) AS latest_paid_at, MAX(o.refunded_at) AS latest_refunded_at FROM linuxdo_credit_recharge_orders o LEFT JOIN users u ON u.id = o.user_id WHERE 1 = 1",
        );
        push_admin_recharge_filters(
            &mut builder,
            query.user_query.as_deref(),
            query.status.as_deref(),
            query.start_at,
            query.end_at,
        );
        builder.push(" GROUP BY o.user_id, u.display_name, u.username, u.avatar_template ORDER BY ");
        match query.sort.as_str() {
            "paidAt" => builder.push("latest_paid_at"),
            "refundedAt" => builder.push("latest_refunded_at"),
            "status" => builder.push("refunded_order_count"),
            _ => builder.push("latest_order_created_at"),
        };
        if query.order.eq_ignore_ascii_case("asc") {
            builder.push(" ASC");
        } else {
            builder.push(" DESC");
        }
        builder.push(" LIMIT ");
        builder.push_bind(query.per_page.clamp(1, 100));
        builder.push(" OFFSET ");
        builder.push_bind((query.page.max(1) - 1) * query.per_page.clamp(1, 100));
        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                Ok(LinuxDoCreditRechargeAdminUserGroup {
                    user_id: row.try_get("user_id")?,
                    user_display_name: row.try_get("user_display_name")?,
                    user_username: row.try_get("user_username")?,
                    user_avatar_template: row.try_get("user_avatar_template")?,
                    order_count: row.try_get("order_count")?,
                    paid_order_count: row.try_get("paid_order_count")?,
                    refunded_order_count: row.try_get("refunded_order_count")?,
                    total_credits: row.try_get("total_credits")?,
                    total_money_cents: row.try_get("total_money_cents")?,
                    latest_order_created_at: row.try_get("latest_order_created_at")?,
                    latest_paid_at: row.try_get("latest_paid_at")?,
                    latest_refunded_at: row.try_get("latest_refunded_at")?,
                })
            })
            .collect()
    }

    pub(crate) async fn apply_linuxdo_credit_recharge_payment(
        &self,
        out_trade_no: &str,
        trade_no: &str,
        notify_payload: &str,
        paid_at: i64,
    ) -> Result<LinuxDoCreditRechargeOrder, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT *
            FROM linuxdo_credit_recharge_orders
            WHERE out_trade_no = ?
            LIMIT 1
            "#,
        )
        .bind(out_trade_no)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await.ok();
            return Err(ProxyError::Other("recharge order not found".to_string()));
        };
        let order = Self::linuxdo_credit_recharge_order_from_row(&row)?;
        let paid_month_start = start_of_local_month_utc_ts(
            Utc.timestamp_opt(paid_at, 0)
                .single()
                .unwrap_or_else(Utc::now)
                .with_timezone(&Local),
        );
        if matches!(
            order.status.as_str(),
            LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING
                | LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDED
                | LINUXDO_CREDIT_RECHARGE_STATUS_REFUND_ONLY
                | LINUXDO_CREDIT_RECHARGE_STATUS_EXPIRED
        ) {
            sqlx::query(
                r#"
                UPDATE linuxdo_credit_recharge_orders
                   SET notify_payload = ?, last_notify_at = ?, updated_at = ?
                 WHERE out_trade_no = ?
                "#,
            )
            .bind(notify_payload)
            .bind(paid_at)
            .bind(paid_at)
            .bind(out_trade_no)
            .execute(&mut *tx)
            .await?;
        } else {
            if order.quote_month_start > 0
                && order.quote_month_start != paid_month_start
                && order.status == LINUXDO_CREDIT_RECHARGE_STATUS_PENDING
            {
                sqlx::query(
                    r#"
                    UPDATE linuxdo_credit_recharge_orders
                       SET status = ?, trade_no = COALESCE(NULLIF(?, ''), trade_no),
                           notify_payload = ?, paid_at = COALESCE(paid_at, ?),
                           last_notify_at = ?, updated_at = ?, last_error = ?
                     WHERE out_trade_no = ?
                    "#,
                )
                .bind(LINUXDO_CREDIT_RECHARGE_STATUS_EXPIRED)
                .bind(trade_no)
                .bind(notify_payload)
                .bind(paid_at)
                .bind(paid_at)
                .bind(paid_at)
                .bind("paid month no longer matches quote month")
                .bind(out_trade_no)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                return self
                    .fetch_linuxdo_credit_recharge_order(out_trade_no)
                    .await?
                    .ok_or_else(|| ProxyError::Other("recharge order disappeared".to_string()));
            }
            sqlx::query(
                r#"
                UPDATE linuxdo_credit_recharge_orders
                   SET status = ?,
                       trade_no = COALESCE(NULLIF(?, ''), trade_no),
                       notify_payload = ?,
                       paid_at = COALESCE(paid_at, ?),
                       last_notify_at = ?,
                       updated_at = ?,
                       last_error = NULL
                 WHERE out_trade_no = ?
                "#,
            )
            .bind(LINUXDO_CREDIT_RECHARGE_STATUS_PAID)
            .bind(trade_no)
            .bind(notify_payload)
            .bind(paid_at)
            .bind(paid_at)
            .bind(paid_at)
            .bind(out_trade_no)
            .execute(&mut *tx)
            .await?;

            let schedule_months = order
                .quote_snapshot_json
                .as_deref()
                .and_then(parse_linuxdo_credit_recharge_quote_snapshot)
                .map(|quote| quote.schedule);
            let start_month = if order.quote_month_start > 0 {
                order.quote_month_start
            } else {
                paid_month_start
            };
            for month_index in 0..order.months {
                let month_start = shift_local_month_start_utc_ts(start_month, month_index as i32);
                let month_quote = schedule_months
                    .as_ref()
                    .and_then(|months| months.get(month_index as usize));
                let monthly_delta = month_quote
                    .map(|month| month.monthly_delta)
                    .unwrap_or(order.final_monthly_delta);
                sqlx::query(
                    r#"
                    INSERT OR IGNORE INTO linuxdo_credit_recharge_entitlements (
                        out_trade_no, user_id, month_start, credits, hourly_delta, daily_delta, monthly_delta, created_at
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(out_trade_no)
                .bind(&order.user_id)
                .bind(month_start)
                .bind(order.credits)
                .bind(order.final_hourly_delta)
                .bind(order.final_daily_delta)
                .bind(monthly_delta)
                .bind(paid_at)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        self.invalidate_account_quota_resolution(&order.user_id).await;
        self.record_effective_account_quota_snapshot_at(&order.user_id, paid_at)
            .await?;
        self.fetch_linuxdo_credit_recharge_order(out_trade_no)
            .await?
            .ok_or_else(|| ProxyError::Other("recharge order disappeared".to_string()))
    }

    pub(crate) async fn refund_linuxdo_credit_recharge_order(
        &self,
        out_trade_no: &str,
        next_status: &str,
        refund_actor: &str,
        refund_payload: &str,
        refunded_at: i64,
        revoke_entitlements: bool,
    ) -> Result<LinuxDoCreditRechargeOrder, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT *
            FROM linuxdo_credit_recharge_orders
            WHERE out_trade_no = ?
            LIMIT 1
            "#,
        )
        .bind(out_trade_no)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await.ok();
            return Err(ProxyError::Other("recharge order not found".to_string()));
        };
        let order = Self::linuxdo_credit_recharge_order_from_row(&row)?;
        if order.status != LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING {
            tx.rollback().await.ok();
            return Err(ProxyError::Other(format!(
                "recharge order refund is not reserved from status {}",
                order.status
            )));
        }
        sqlx::query(
            r#"
            UPDATE linuxdo_credit_recharge_orders
               SET status = ?, refunded_at = ?, refund_actor = ?, refund_payload = ?,
                   updated_at = ?, last_error = NULL
             WHERE out_trade_no = ? AND status = ?
            "#,
        )
        .bind(next_status)
        .bind(refunded_at)
        .bind(refund_actor)
        .bind(refund_payload)
        .bind(refunded_at)
        .bind(out_trade_no)
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING)
        .execute(&mut *tx)
        .await?;
        if revoke_entitlements {
            sqlx::query("DELETE FROM linuxdo_credit_recharge_entitlements WHERE out_trade_no = ?")
                .bind(out_trade_no)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        self.invalidate_account_quota_resolution(&order.user_id).await;
        self.record_effective_account_quota_snapshot_at(&order.user_id, refunded_at)
            .await?;
        self.fetch_linuxdo_credit_recharge_order(out_trade_no)
            .await?
            .ok_or_else(|| ProxyError::Other("recharge order disappeared".to_string()))
    }

    pub(crate) async fn mark_linuxdo_credit_recharge_order_refund_external_succeeded(
        &self,
        out_trade_no: &str,
        refund_actor: &str,
        refund_payload: &str,
        updated_at: i64,
    ) -> Result<LinuxDoCreditRechargeOrder, ProxyError> {
        let result = sqlx::query(
            r#"
            UPDATE linuxdo_credit_recharge_orders
               SET refund_actor = ?, refund_payload = ?, updated_at = ?, last_error = ?
             WHERE out_trade_no = ? AND status = ?
            "#,
        )
        .bind(refund_actor)
        .bind(refund_payload)
        .bind(updated_at)
        .bind("external refund succeeded; local finalize pending")
        .bind(out_trade_no)
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(ProxyError::Other(
                "recharge order refund success marker could not be persisted".to_string(),
            ));
        }
        self.fetch_linuxdo_credit_recharge_order(out_trade_no)
            .await?
            .ok_or_else(|| ProxyError::Other("recharge order disappeared".to_string()))
    }

    pub(crate) async fn reserve_linuxdo_credit_recharge_order_refund(
        &self,
        out_trade_no: &str,
        reserved_at: i64,
    ) -> Result<LinuxDoCreditRechargeOrder, ProxyError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"
            SELECT *
            FROM linuxdo_credit_recharge_orders
            WHERE out_trade_no = ?
            LIMIT 1
            "#,
        )
        .bind(out_trade_no)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await.ok();
            return Err(ProxyError::Other("recharge order not found".to_string()));
        };
        let order = Self::linuxdo_credit_recharge_order_from_row(&row)?;
        if order.status != LINUXDO_CREDIT_RECHARGE_STATUS_PAID {
            tx.rollback().await.ok();
            return Err(ProxyError::Other(format!(
                "recharge order is not refundable from status {}",
                order.status
            )));
        }
        if order
            .trade_no
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            tx.rollback().await.ok();
            return Err(ProxyError::Other(
                "recharge order has no trade number".to_string(),
            ));
        }
        let result = sqlx::query(
            r#"
            UPDATE linuxdo_credit_recharge_orders
               SET status = ?, updated_at = ?, last_error = NULL
             WHERE out_trade_no = ? AND status = ?
            "#,
        )
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING)
        .bind(reserved_at)
        .bind(out_trade_no)
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_PAID)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() != 1 {
            tx.rollback().await.ok();
            return Err(ProxyError::Other(
                "recharge order refund is already in progress".to_string(),
            ));
        }
        tx.commit().await?;
        self.fetch_linuxdo_credit_recharge_order(out_trade_no)
            .await?
            .ok_or_else(|| ProxyError::Other("recharge order disappeared".to_string()))
    }

    pub(crate) async fn release_linuxdo_credit_recharge_order_refund_reservation(
        &self,
        out_trade_no: &str,
        message: &str,
        updated_at: i64,
    ) -> Result<(), ProxyError> {
        sqlx::query(
            r#"
            UPDATE linuxdo_credit_recharge_orders
               SET status = ?, updated_at = ?, last_error = ?
             WHERE out_trade_no = ? AND status = ?
            "#,
        )
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_PAID)
        .bind(updated_at)
        .bind(message)
        .bind(out_trade_no)
        .bind(LINUXDO_CREDIT_RECHARGE_STATUS_REFUNDING)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn sum_linuxdo_credit_recharge_entitlements_for_month(
        &self,
        user_id: &str,
        month_start: i64,
    ) -> Result<LinuxDoCreditRechargeQuotaDelta, ProxyError> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(hourly_delta), 0) AS hourly_delta,
                COALESCE(SUM(daily_delta), 0) AS daily_delta,
                COALESCE(SUM(monthly_delta), 0) AS monthly_delta
            FROM linuxdo_credit_recharge_entitlements
            WHERE user_id = ? AND month_start = ?
            "#,
        )
        .bind(user_id)
        .bind(month_start)
        .fetch_one(&self.pool)
        .await?;
        Ok(LinuxDoCreditRechargeQuotaDelta {
            hourly_delta: row.try_get("hourly_delta")?,
            daily_delta: row.try_get("daily_delta")?,
            monthly_delta: row.try_get("monthly_delta")?,
        })
    }

    pub(crate) async fn sum_current_linuxdo_credit_recharge_entitlements_for_month(
        &self,
        user_id: &str,
    ) -> Result<LinuxDoCreditRechargeQuotaDelta, ProxyError> {
        self.sum_linuxdo_credit_recharge_entitlements_for_month(
            user_id,
            start_of_local_month_utc_ts(self.backend_time.local_now()),
        )
        .await
    }

    pub(crate) async fn sum_linuxdo_credit_recharge_entitlements_for_users(
        &self,
        user_ids: &[String],
        month_start: i64,
    ) -> Result<HashMap<String, LinuxDoCreditRechargeQuotaDelta>, ProxyError> {
        if user_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut builder = QueryBuilder::new(
            "SELECT user_id, COALESCE(SUM(hourly_delta), 0) AS hourly_delta, COALESCE(SUM(daily_delta), 0) AS daily_delta, COALESCE(SUM(monthly_delta), 0) AS monthly_delta FROM linuxdo_credit_recharge_entitlements WHERE month_start = ",
        );
        builder.push_bind(month_start);
        builder.push(" AND user_id IN (");
        {
            let mut separated = builder.separated(", ");
            for user_id in user_ids {
                separated.push_bind(user_id);
            }
        }
        builder.push(") GROUP BY user_id");
        let rows = builder.build().fetch_all(&self.pool).await?;
        let mut map = HashMap::new();
        for row in rows {
            map.insert(
                row.try_get::<String, _>("user_id")?,
                LinuxDoCreditRechargeQuotaDelta {
                    hourly_delta: row.try_get("hourly_delta")?,
                    daily_delta: row.try_get("daily_delta")?,
                    monthly_delta: row.try_get("monthly_delta")?,
                },
            );
        }
        Ok(map)
    }

    pub(crate) async fn sum_current_linuxdo_credit_recharge_entitlements_for_users(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, LinuxDoCreditRechargeQuotaDelta>, ProxyError> {
        self.sum_linuxdo_credit_recharge_entitlements_for_users(
            user_ids,
            start_of_local_month_utc_ts(self.backend_time.local_now()),
        )
        .await
    }

    pub(crate) async fn linuxdo_credit_recharge_summary_for_user(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<LinuxDoCreditRechargeSummary, ProxyError> {
        let current_month_entitlement = self
            .sum_linuxdo_credit_recharge_entitlements_for_month(user_id, current_month_start)
            .await?;
        let effective_until_month_start = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT MAX(month_start)
            FROM linuxdo_credit_recharge_entitlements
            WHERE user_id = ?
            "#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(LinuxDoCreditRechargeSummary {
            current_month_start,
            current_month_entitlement_credits: current_month_entitlement.monthly_delta,
            current_month_entitlement_hourly_delta: current_month_entitlement.hourly_delta,
            current_month_entitlement_daily_delta: current_month_entitlement.daily_delta,
            current_month_entitlement_monthly_delta: current_month_entitlement.monthly_delta,
            effective_until_month_start,
        })
    }

    pub(crate) async fn list_linuxdo_credit_recharge_entitlements_for_user(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<LinuxDoCreditRechargeEntitlement>, ProxyError> {
        let rows = sqlx::query(
            r#"
            SELECT *
            FROM linuxdo_credit_recharge_entitlements
            WHERE user_id = ?
            ORDER BY month_start DESC, id DESC
            LIMIT ?
            "#,
        )
        .bind(user_id)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(Self::linuxdo_credit_recharge_entitlement_from_row)
            .collect()
    }

    pub(crate) async fn linuxdo_credit_recharge_admin_audit(
        &self,
        user_id: &str,
        current_month_start: i64,
    ) -> Result<LinuxDoCreditRechargeAdminAudit, ProxyError> {
        let summary = self
            .linuxdo_credit_recharge_summary_for_user(user_id, current_month_start)
            .await?;
        Ok(LinuxDoCreditRechargeAdminAudit {
            current_month_entitlement_credits: summary.current_month_entitlement_credits,
            effective_until_month_start: summary.effective_until_month_start,
            orders: self
                .list_linuxdo_credit_recharge_orders_for_user(user_id, 10)
                .await?,
            entitlements: self
                .list_linuxdo_credit_recharge_entitlements_for_user(user_id, 24)
                .await?,
        })
    }
}

fn push_admin_recharge_filters<'a>(
    builder: &mut QueryBuilder<'a, Sqlite>,
    user_query: Option<&str>,
    status: Option<&str>,
    start_at: Option<i64>,
    end_at: Option<i64>,
) {
    if let Some(q) = user_query.map(str::trim).filter(|q| !q.is_empty()) {
        let like = format!("%{q}%");
        builder.push(" AND (o.user_id LIKE ");
        builder.push_bind(like.clone());
        builder.push(" OR COALESCE(u.display_name, '') LIKE ");
        builder.push_bind(like.clone());
        builder.push(" OR COALESCE(u.username, '') LIKE ");
        builder.push_bind(like.clone());
        builder.push(" OR o.out_trade_no LIKE ");
        builder.push_bind(like.clone());
        builder.push(" OR COALESCE(o.trade_no, '') LIKE ");
        builder.push_bind(like);
        builder.push(")");
    }
    if let Some(status) = status.map(str::trim).filter(|s| !s.is_empty() && *s != "all") {
        builder.push(" AND o.status = ");
        builder.push_bind(status.to_string());
    }
    if let Some(start_at) = start_at {
        builder.push(" AND o.created_at >= ");
        builder.push_bind(start_at);
    }
    if let Some(end_at) = end_at {
        builder.push(" AND o.created_at <= ");
        builder.push_bind(end_at);
    }
}

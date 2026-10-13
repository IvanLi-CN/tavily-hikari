// Reconciliation terminal same-second migration, extracted verbatim from
// key_store_schema_migrations.rs (post-rebase line-budget remediation; pure
// textual move inside the store include! family - same module scope).
impl KeyStore {
    async fn apply_reconciliation_terminal_same_second_migration(
        &self,
    ) -> Result<(), ProxyError> {
        // Historical usage and settlement timestamps have only second precision. Reopen equal
        // timestamps conservatively: a duplicate verification is safer than losing a usage
        // update that happened in the same second as settlement.
        sqlx::query(
            r#"UPDATE upstream_reconciliation_work
               SET completed_generation = 0,
                   last_outcome = NULL
               WHERE completed_generation >= work_generation
                 AND EXISTS (
                     SELECT 1
                     FROM upstream_reconciliation_settlements s
                     WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                       AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                 )
                 AND EXISTS (
                     SELECT 1
                     FROM upstream_reconciliation_usage u
                     WHERE u.token_id = upstream_reconciliation_work.token_id
                       AND u.period_code = upstream_reconciliation_work.period_code
                       AND u.updated_at = COALESCE((
                           SELECT MAX(s.settled_at)
                           FROM upstream_reconciliation_settlements s
                           WHERE s.settlement_key = 'v1:' || upstream_reconciliation_work.token_id || ':' || upstream_reconciliation_work.period_code
                             AND s.status IN ('settled', 'degraded', 'shadow_settled', 'shadow_degraded')
                       ), 0)
                 )"#,
        )
        .execute(&self.pool)
        .await?;
        self.record_schema_migration(
            RECONCILIATION_TERMINAL_SAME_SECOND_VERSION,
            RECONCILIATION_TERMINAL_SAME_SECOND_NAME,
            RECONCILIATION_TERMINAL_SAME_SECOND_CHECKSUM,
        )
        .await
    }
}

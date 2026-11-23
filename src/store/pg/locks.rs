//! Central advisory-lock and row-lock registry for PostgreSQL write paths.
//!
//! The retired storage engine serialised every write in the process, so a critical
//! section only had to open a transaction. PostgreSQL runs transactions concurrently, so
//! each caller now names the invariant it needs and takes the matching lock here. Keys
//! are derived from stable strings (FNV-1a), never from a query planner decision or a
//! per-process value, so two processes that name the same invariant collide on the same
//! advisory lock.
//!
//! # Replaced critical sections
//!
//! Every previously serialised critical section, and what provides mutual exclusion now:
//!
//! | Critical section | Mutex |
//! | --- | --- |
//! | Startup schema bootstrap, versioned migrations, table-rebuild repairs | [`LockScope::StoreBootstrap`] / [`LockScope::SchemaMigration`] advisory lock plus one transaction per migration |
//! | Admin password, TOTP and passkey writes | [`RowLock::AdminPasswordSettings`], [`RowLock::AdminPasskeyCredential`] |
//! | Announcement content rebuild | [`LockScope::Announcements`] |
//! | Scheduled-job rebuild and claim | [`RowLock::ScheduledJob`] plus `INSERT ... ON CONFLICT` and `FOR UPDATE SKIP LOCKED` |
//! | Reconciliation usage, work aggregation and settlement | [`LockScope::ReconciliationWork`] per `(token_id, period_code)`, [`RowLock::ReconciliationWork`] |
//! | Reconciliation projection cursor | [`LockScope::ReconciliationProjection`], [`RowLock::ReconciliationProjectionState`] |
//! | Monthly quota rebase | [`LockScope::QuotaRebase`] per month start, [`RowLock::MetaEntry`] for the rebase marker |
//! | Request log retention GC | [`LockScope::RequestLogsGc`] for pass admission plus batch row locks with `SKIP LOCKED` |
//! | Dashboard rollup integrity and derived-bucket rebuild | [`LockScope::DashboardRollupIntegrity`], [`LockScope::ObservabilityDerivedRebuild`] |
//! | Request statistics flush | [`LockScope::RequestStatsFlush`] (single atomic upsert per bucket) |
//! | Analysis pressure rebuild | [`LockScope::AnalysisPressure`] |
//! | Token, account and meta counters | [`RowLock::TokenMonthlyQuota`], [`RowLock::AccountMonthlyQuota`], [`RowLock::MetaEntry`] |
//!
//! Row locks are single-row `FOR UPDATE` statements. Batch paths that must not block the
//! ingest writer select their work with `FOR UPDATE SKIP LOCKED` in the owning module;
//! they are still admitted one pass at a time through the scopes above.

// The registry is consumed by the PostgreSQL branches that the porting wave adds. Until
// the first `self.pg()` caller takes a scope, nothing references it and every item would
// be dead code under `-D warnings`. Drop this attribute when that call site lands.
#![allow(dead_code)]

use std::time::Duration;

use sqlx::PgConnection;
use tracing::debug;

/// FNV-1a parameters. The hash only has to be stable across processes and releases.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const FIELD_SEPARATOR: u8 = 0x1f;

/// A resolved advisory-lock key with the registry name kept for logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LockId {
    key: i64,
    name: &'static str,
}

impl LockId {
    pub(crate) fn key(&self) -> i64 {
        self.key
    }

    pub(crate) fn name(&self) -> &'static str {
        self.name
    }
}

/// One named write invariant. Scoped variants carry the identity the invariant is about,
/// so unrelated identities never contend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LockScope<'a> {
    /// Schema bootstrap and versioned migration application.
    StoreBootstrap,
    /// One migration step, including the table-rebuild repairs it performs.
    SchemaMigration,
    /// Admin login material (password, TOTP, passkey registry).
    AdminPasskey,
    /// Announcement content rebuild.
    Announcements,
    /// Scheduled-job queue rebuild.
    ScheduledJobs,
    /// Small singleton counters kept in `core.meta`.
    Meta,
    /// Admission for one bounded request-log retention GC pass.
    RequestLogsGc,
    /// Admission for dashboard rollup integrity slices.
    DashboardRollupIntegrity,
    /// Admission for derived observability bucket rebuilds.
    ObservabilityDerivedRebuild,
    /// Request statistics flush batch.
    RequestStatsFlush,
    /// Analysis pressure bucket rebuild.
    AnalysisPressure,
    /// Reconciliation projection cursor advance.
    ReconciliationProjection,
    /// One reconciliation work identity.
    ReconciliationWork {
        token_id: &'a str,
        period_code: &'a str,
    },
    /// One monthly quota rebase window.
    QuotaRebase { month_start: i64 },
}

impl LockScope<'_> {
    /// Registry name used in logs and in the table above.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::StoreBootstrap => "store:bootstrap",
            Self::SchemaMigration => "store:schema-migration",
            Self::AdminPasskey => "core:admin-passkey",
            Self::Announcements => "core:announcements",
            Self::ScheduledJobs => "core:scheduled-jobs",
            Self::Meta => "core:meta",
            Self::RequestLogsGc => "observability:request-logs-gc",
            Self::DashboardRollupIntegrity => "observability:dashboard-rollup-integrity",
            Self::ObservabilityDerivedRebuild => "observability:derived-rebuild",
            Self::RequestStatsFlush => "observability:request-stats-flush",
            Self::AnalysisPressure => "observability:analysis-pressure",
            Self::ReconciliationProjection => "core:reconciliation-projection",
            Self::ReconciliationWork { .. } => "core:reconciliation-work",
            Self::QuotaRebase { .. } => "core:quota-rebase",
        }
    }

    /// Stable advisory-lock key for this invariant.
    pub(crate) fn id(self) -> LockId {
        let mut hasher = StableHasher::new();
        hasher.feed(self.name().as_bytes());
        hasher.feed(&[FIELD_SEPARATOR]);
        match self {
            Self::ReconciliationWork {
                token_id,
                period_code,
            } => {
                hasher.feed(token_id.as_bytes());
                hasher.feed(&[FIELD_SEPARATOR]);
                hasher.feed(period_code.as_bytes());
            }
            Self::QuotaRebase { month_start } => hasher.feed_i64(month_start),
            _ => {}
        }
        LockId {
            key: hasher.finish(),
            name: self.name(),
        }
    }
}

/// Takes the transaction-scoped advisory lock for `scope`.
///
/// The lock is released when the surrounding transaction commits or rolls back; there is
/// no explicit unlock to forget.
pub(crate) async fn acquire_xact_lock(
    conn: &mut PgConnection,
    scope: LockScope<'_>,
) -> Result<LockId, sqlx::Error> {
    let id = scope.id();
    debug!(
        component = "store_locks",
        event = "advisory_lock_acquire",
        lock = id.name(),
        key = id.key(),
        mode = "exclusive",
        "acquiring transaction-scoped advisory lock"
    );
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(id.key())
        .execute(conn)
        .await?;
    Ok(id)
}

/// Takes the transaction-scoped advisory lock in shared mode.
pub(crate) async fn acquire_xact_lock_shared(
    conn: &mut PgConnection,
    scope: LockScope<'_>,
) -> Result<LockId, sqlx::Error> {
    let id = scope.id();
    debug!(
        component = "store_locks",
        event = "advisory_lock_acquire",
        lock = id.name(),
        key = id.key(),
        mode = "shared",
        "acquiring transaction-scoped advisory lock"
    );
    sqlx::query("SELECT pg_advisory_xact_lock_shared($1)")
        .bind(id.key())
        .execute(conn)
        .await?;
    Ok(id)
}

/// Attempts the transaction-scoped advisory lock without waiting.
///
/// Returns `true` when the lock was taken. Deferred maintenance uses this to yield to the
/// request path instead of queueing behind it.
pub(crate) async fn try_xact_lock(
    conn: &mut PgConnection,
    scope: LockScope<'_>,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
        .bind(scope.id().key())
        .fetch_one(conn)
        .await
}

/// Rows the ported write paths lock before a read-modify-write.
///
/// Each variant owns a compiled-in statement: a relation name never reaches this module
/// from a caller, so the whitelist cannot be widened by request input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowLock<'a> {
    /// The singleton `core.admin_password_settings` row.
    AdminPasswordSettings,
    /// One admin passkey credential row.
    AdminPasskeyCredential {
        scope_id: &'a str,
        credential_id: &'a str,
    },
    /// One `core.meta` counter row.
    MetaEntry { key: &'a str },
    /// One `core.scheduled_jobs` row.
    ScheduledJob { id: i64 },
    /// One token's monthly quota row.
    TokenMonthlyQuota { token_id: &'a str },
    /// One account's monthly quota row.
    AccountMonthlyQuota { user_id: &'a str },
    /// One reconciliation work row.
    ReconciliationWork {
        token_id: &'a str,
        period_code: &'a str,
    },
    /// The singleton reconciliation projection cursor.
    ReconciliationProjectionState,
    /// One announcement row.
    Announcement { id: &'a str },
}

impl RowLock<'_> {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::AdminPasswordSettings => "core:admin-password-settings",
            Self::AdminPasskeyCredential { .. } => "core:admin-passkey-credential",
            Self::MetaEntry { .. } => "core:meta-entry",
            Self::ScheduledJob { .. } => "core:scheduled-job",
            Self::TokenMonthlyQuota { .. } => "core:token-monthly-quota",
            Self::AccountMonthlyQuota { .. } => "core:account-monthly-quota",
            Self::ReconciliationWork { .. } => "core:reconciliation-work-row",
            Self::ReconciliationProjectionState => "core:reconciliation-projection-state",
            Self::Announcement { .. } => "core:announcement",
        }
    }
}

/// Locks the target row with `FOR UPDATE` and reports how many rows matched.
///
/// A zero result means the row does not exist; the caller decides whether that is a
/// missing-row error or an insert path.
pub(crate) async fn lock_row_for_update(
    conn: &mut PgConnection,
    target: RowLock<'_>,
) -> Result<u64, sqlx::Error> {
    let result = match target {
        RowLock::AdminPasswordSettings => {
            sqlx::query("SELECT 1 FROM core.admin_password_settings WHERE id = 1 FOR UPDATE")
                .execute(conn)
                .await?
        }
        RowLock::AdminPasskeyCredential {
            scope_id,
            credential_id,
        } => {
            sqlx::query(
                "SELECT 1 FROM core.admin_passkey_credentials \
                 WHERE scope_id = $1 AND credential_id = $2 FOR UPDATE",
            )
            .bind(scope_id)
            .bind(credential_id)
            .execute(conn)
            .await?
        }
        RowLock::MetaEntry { key } => {
            sqlx::query("SELECT 1 FROM core.meta WHERE key = $1 FOR UPDATE")
                .bind(key)
                .execute(conn)
                .await?
        }
        RowLock::ScheduledJob { id } => {
            sqlx::query("SELECT 1 FROM core.scheduled_jobs WHERE id = $1 FOR UPDATE")
                .bind(id)
                .execute(conn)
                .await?
        }
        RowLock::TokenMonthlyQuota { token_id } => {
            sqlx::query("SELECT 1 FROM core.auth_token_quota WHERE token_id = $1 FOR UPDATE")
                .bind(token_id)
                .execute(conn)
                .await?
        }
        RowLock::AccountMonthlyQuota { user_id } => {
            sqlx::query("SELECT 1 FROM core.account_monthly_quota WHERE user_id = $1 FOR UPDATE")
                .bind(user_id)
                .execute(conn)
                .await?
        }
        RowLock::ReconciliationWork {
            token_id,
            period_code,
        } => {
            sqlx::query(
                "SELECT 1 FROM core.upstream_reconciliation_work \
                 WHERE token_id = $1 AND period_code = $2 FOR UPDATE",
            )
            .bind(token_id)
            .bind(period_code)
            .execute(conn)
            .await?
        }
        RowLock::ReconciliationProjectionState => {
            sqlx::query(
                "SELECT 1 FROM core.upstream_reconciliation_projection_state \
                 WHERE id = 'local' FOR UPDATE",
            )
            .execute(conn)
            .await?
        }
        RowLock::Announcement { id } => {
            sqlx::query("SELECT 1 FROM core.announcements WHERE id = $1 FOR UPDATE")
                .bind(id)
                .execute(conn)
                .await?
        }
    };
    debug!(
        component = "store_locks",
        event = "row_lock_acquire",
        lock = target.name(),
        rows = result.rows_affected(),
        "locked row for update"
    );
    Ok(result.rows_affected())
}

/// Bounds how long the current transaction waits for a row or table lock.
///
/// This is the replacement for the per-operation writer wait budget: the value is local
/// to the transaction, so the pool connection returns to the pool with the server default.
pub(crate) async fn set_local_lock_timeout(
    conn: &mut PgConnection,
    budget: Duration,
) -> Result<(), sqlx::Error> {
    let millis = budget.as_millis().min(i64::MAX as u128);
    sqlx::query("SELECT set_config('lock_timeout', $1, true)")
        .bind(format!("{millis}ms"))
        .execute(conn)
        .await?;
    Ok(())
}

/// FNV-1a over the fed bytes, finished as the signed 64-bit advisory key.
struct StableHasher(u64);

impl StableHasher {
    fn new() -> Self {
        Self(FNV_OFFSET_BASIS)
    }

    fn feed(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn feed_i64(&mut self, value: i64) {
        self.feed(&value.to_le_bytes());
    }

    fn finish(self) -> i64 {
        self.0 as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoped_keys_are_stable_and_distinct() {
        let a = LockScope::ReconciliationWork {
            token_id: "tok-a",
            period_code: "2026-11",
        }
        .id();
        let b = LockScope::ReconciliationWork {
            token_id: "tok-a",
            period_code: "2026-11",
        }
        .id();
        let c = LockScope::ReconciliationWork {
            token_id: "tok-b",
            period_code: "2026-11",
        }
        .id();

        assert_eq!(a.key(), b.key());
        assert_ne!(a.key(), c.key());
    }

    #[test]
    fn fixed_scopes_do_not_collide_with_scoped_namespaces() {
        let bootstrap = LockScope::StoreBootstrap.id();
        let migration = LockScope::SchemaMigration.id();
        let quota = LockScope::QuotaRebase {
            month_start: 1_767_225_600,
        }
        .id();

        assert_ne!(bootstrap.key(), migration.key());
        assert_ne!(bootstrap.key(), quota.key());
        assert_eq!(bootstrap.name(), "store:bootstrap");
    }

    #[test]
    fn fields_are_separated_so_names_cannot_merge() {
        let split = LockScope::ReconciliationWork {
            token_id: "tok",
            period_code: "2026-11",
        }
        .id();
        let boundary = LockScope::ReconciliationWork {
            token_id: "tok\u{1f}2026-11",
            period_code: "",
        }
        .id();

        assert_ne!(split.key(), boundary.key());
    }
}

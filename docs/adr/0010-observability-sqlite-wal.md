---
status: accepted
---

# Use WAL for the observability SQLite sidecar

An observability-sidecar read snapshot in DELETE journal mode can block a foreground writer's commit even though the core database uses WAL. Distinct writable observability files will therefore use verified WAL initialization, retain the existing synchronous durability level and single-writer rule, and expose checkpoint results separately for each attached schema. This reduces reader/writer interference while accepting explicit ownership of sidecar WAL growth, startup failure, backup and compatibility handling.

## Consequences

- Readonly and dry-run entry points do not change journal mode or create a missing sidecar; a legacy same-file layout does not create another database.
- Core and observability checkpoint results are independent evidence. Each file is backed up through the SQLite backup API, including committed frames still in WAL; separate exports do not promise a cross-file atomic snapshot.
- A failed writable mode transition produces a bounded, explicit initialization failure. Mode reversal is a controlled storage operation, and an older binary that rejects the new migration ledger is not a valid automatic rollback target.
- The storage contract is owned by [observability-sqlite-storage](../specs/observability-sqlite-storage/SPEC.md), separately from the PRAGMA non-goal in the existing performance architecture contract.

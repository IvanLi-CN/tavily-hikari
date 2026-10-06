# ADR 0008: Bounded Online Recovery for Request Statistics

## Status

Accepted

## Context

Dashboard-integrity work and request-log GC depend on each other: source-backed local-day verification and sealing must finish before expired source rows can be deleted. The ordinary foreground-rate gate can indefinitely defer that finite recovery debt during sustained traffic, even when the database still has capacity for a bounded slice. A rate observation is a heuristic, not proof that the SQLite pool or writer is saturated.

## Decision

Aged dashboard-integrity and request-log-GC work may receive a bounded online recovery turn that bypasses only the ordinary foreground-rate heuristic. Each turn still uses the single local maintenance-bulk permit, real capacity and contention checks, bounded connection acquisition, source reads and transactions, durable claim fences, and the existing source/seal deletion guards. The exception cannot preempt actual foreground waiters or turn an uncommitted result into progress.

This is a scoped amendment to [ADR 0005](./0005-fair-sqlite-maintenance-admission.md): its ordinary rate gate and fairness rules continue to apply outside these recovery turns. Other maintenance classes receive no new rate exception. Eligibility, cadence and slice sizing must be bounded and validated against both effective recovery progress and foreground performance.

## Considered Options

- Waiting exclusively for traffic to fall below the ordinary threshold protects foreground work
  but cannot guarantee online progress for a continuously active service. Exclusive offline repair
  remains a controlled operational option rather than the automatic recovery policy.
- Raising the general rate threshold relaxes admission for unrelated work without measuring actual
  capacity. The exception instead applies only to aged recovery work and retains real safeguards.
- Increasing the connection pool does not remove SQLite's single-writer limit and is not the
  recovery mechanism selected here.

## Consequences

Verification must include recovery during sustained traffic above the ordinary rate threshold, not only catch-up after a low-load transition. Genuine capacity exhaustion may still defer a turn; evidence must identify that cause and preserve resumable work. Completion is measured against accepted progress in a fixed historical debt set, not job success counts or a moving chart alone. This policy does not authorize a production restart, maintenance outage, or manual data mutation.

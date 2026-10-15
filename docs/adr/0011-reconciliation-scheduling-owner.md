---
status: accepted
---

# Give reconciliation scheduling one background owner

Business requests synchronously persist usage, Research state and the resulting work generation, then emit a bounded scheduling hint instead of computing a global continuation before returning. One MaintenanceRuntime-owned coalescing scheduler performs the bounded global read, durably enqueues the representative and only then wakes the execution worker; durable SQLite work remains authoritative when a hint is lost. This removes history-dependent scheduling work from the billing critical section while accepting a healthy-resource coalescing window of at most one second and recovery checks through startup and the existing thirty-second watchdog.

## Consequences

- The hint is advisory, not an eventual billing queue: requests cannot succeed before their required business commit, and no task is spawned per request.
- Work arriving during an active scheduling pass remains dirty for a later pass. Enqueue failures defer with retained work; queued representatives and claim fences remain durable and unique.
- Pause, disabled background work, HA authority changes and shutdown follow the runtime owner lifecycle. SQLite contention can defer scheduling beyond the healthy-resource window; the watchdog interval is not an unconditional completion SLA.
- Research retains its independent durable drain and remote-attempt fairness. The same-account billing serialization contract is unchanged by this ownership decision.

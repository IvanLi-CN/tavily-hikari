# History

- 2026-06-02: Created the fast-track spec for request log body retention settings,
  body metadata, bounded automatic cleanup, and user debug-sharing consent.
- 2026-06-02: Implemented request body retention policy, metadata migrations, bounded historical
  body cleanup, Admin settings sliders, user debug-sharing consent, request detail cleaned-body
  display, Storybook coverage, and focused validation.

## Legacy Identity

- Legacy compatibility identity: `#owl2v`.

- 2026-10-03: Define resumable GC-blocking day recovery, stale hot-window repair, and productive
  catch-up scheduling without changing retention or billing truth.
- 2026-10-04: Make the private probe's quiet-phase rate configurable within the accepted foreground
  limit and preserve the completed high-load evidence independently of recovery duration.
- 2026-10-04: Cover new GC-blocker page preemption, unfinished source-recovery deletion protection,
  and terminal bodyless scans that make no durable cursor progress.
- 2026-10-04: Keep foreground HTTP primary-affinity cooldown selection out of maintenance admission
  while preserving global cooldown and fallback behavior during GC catch-up.
- 2026-10-05: Align admission-pressure retries with the five-minute recovery backoff and queue an
  unqueued initial hot page before a GC-blocking day.
- 2026-10-26: Increase the scheduled request-log GC slice from five to ten 100-row batches while
  retaining the 20-second runtime bound and existing admission and continuation contracts.
- 2026-10-26: Prewarm the reconciliation maintenance test fixture so parallel validation observes
  the intended admission contract instead of an unrelated lazy-pool pressure defer.

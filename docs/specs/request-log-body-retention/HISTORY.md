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

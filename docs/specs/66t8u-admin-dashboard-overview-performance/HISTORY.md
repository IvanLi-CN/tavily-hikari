# Admin Dashboard Overview History

## 2026-06-29

- Corrected the dashboard traffic trend default window from a fixed "today" frame to a rolling 24-hour hourly window.
- Aligned the dashboard overview story copy and tests with the new hourly trend semantics.

## 2026-07-03

- Hardened the shared dashboard overview snapshot singleflight so timed-out or cancelled payload
  builds release `loading` and wake waiters.
- Added a regression test that gates payload construction, verifies the build timeout surfaces as
  an error instead of an indefinite wait, then confirms a later overview request can rebuild a real
  snapshot.

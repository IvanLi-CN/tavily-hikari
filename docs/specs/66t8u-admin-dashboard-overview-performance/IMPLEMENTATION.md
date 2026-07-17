# Admin Dashboard Overview Implementation

## Status

- Status: Rolling 24-hour trend window implemented
- Last: 2026-07-17

## Coverage

- Dashboard overview now uses a rolling 24-hour hourly window for the default traffic trend chart.
- The window ends at the current visible hour bucket and leaves missing buckets blank.
- Storybook copy and hourly chart tests were updated to match the chart behavior.
- Recent alerts now derive grouped rate-limit window badges from the latest alert event's semantic
  window before falling back to legacy group metadata, so the dashboard no longer renders a stale
  `5m window` label for rolling `60m` business-call cap alerts.

## Notes

- The dashboard overview payload shape and SSE snapshot contract are unchanged.
- The recent-alert wording fix keeps the payload shape unchanged and only corrects grouped alert
  metadata precedence plus the corresponding presentation copy.

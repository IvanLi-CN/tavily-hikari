# Admin Dashboard Overview History

## 2026-08-02

- Kept the existing ten-second rebuild budget and sixty-second unchanged probe; dashboard visual
  assets are excluded from this round's PR evidence because the dashboard UI is unchanged.

## 2026-07-31

- Bounded expensive freshness probes and rebuilds to a 10-second minimum interval and added the
  post-ready partial alert index maintenance path.

## 2026-07-17

- Corrected recent-alert grouped window wording so rolling `60m` business-call cap alerts no longer
  inherit a stale `5m window` badge from legacy grouped metadata.

## 2026-06-29

- Corrected the dashboard traffic trend default window from a fixed "today" frame to a rolling 24-hour hourly window.
- Aligned the dashboard overview story copy and tests with the new hourly trend semantics.

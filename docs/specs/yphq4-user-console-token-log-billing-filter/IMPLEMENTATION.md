# Implementation

## Current Coverage

- User token logs API now accepts `billing=all|billable`, defaults to `all`, and clamps user-facing
  list size to 50.
- `countsBusinessQuota` is exposed to the user console log views so the UI can filter by request-kind
  billing semantics without inferring from charge amount or quota state.
- Token detail recent requests default to 50 rows, support `All / Quota usage` filtering, and keep
  the desktop table inside a scrollable 10-row viewport.
- Mobile token detail shows a dedicated recent-requests entry, while the full filtered list lives on
  the separate token logs route.
- Storybook coverage includes desktop token detail and mobile logs entry states, with visual evidence
  captured for the desktop 10-row scroll layout.

## Validation

- `bun test ./src/UserConsole.stories.test.ts`
- `bun run build`

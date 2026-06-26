# Implementation

## Current State

- `AdminDashboardRuntime.tsx` now keeps the shared admin shell state in the parent path and dispatches route-specific content through screen components.
- `analysis` 已成为新的 admin 父模块；`/admin/analysis/usage`、`/admin/analysis/rankings`、`/admin/analysis/pressure` 由同一稳定 route dispatch 统一承接，旧 `/admin/users/usage` 与 `/admin/rankings` 作为 alias 映射到相同页面逻辑。
- `web/src/admin/screens/UsersUsageScreen.tsx` and `web/src/admin/screens/UnboundTokenUsageScreen.tsx` hold the page-specific route bodies.
- `web/src/admin/PressureAnalysisScreen.tsx` now holds the new pressure submodule body while staying inside the same shared runtime shell contract, and the surface has been tightened to the requested three chart panels without the earlier summary card band.
- `web/src/admin/screens/shared.tsx` contains shared table / intro helpers used by both screens.
- `web/src/admin/AdminDashboardRuntime.route-switch.test.tsx` provides the local red-capable route-switch loop for the white-screen regression.
- `web/test/happydom.ts` now registers a stable local URL so history navigation in the route-switch test works deterministically.

## Verification

- `cd web && bun test src/admin/AdminDashboardRuntime.route-switch.test.tsx`
- `cd web && bun test src/admin/routes.test.ts src/admin/AdminPages.stories.test.ts`
- `cd web && bun test src/admin/AdminPages.stories.test.ts`
- `cd web && bun run build`

## Pending

- Live Chrome confirmation on the production site.
- Storybook / browser visual evidence capture for the new screen contract.
- Full spec sync once the remaining evidence artifacts are committed.

# History

## 2026-06-22

- Created the focused spec after reproducing the `/admin/users/usage` white screen and confirming the root cause was route-specific early returns before later hooks in `AdminDashboardRuntime.tsx`.
- Extracted the `user-usage` and `unbound-token-usage` route bodies into screen components and added a route-switch regression test to lock the hook-order fix.
- Aligned the story proof surface to the same screen contract so Storybook no longer needs a separate copy of the production route JSX.

## 2026-06-25

- Admin 信息架构新增 `analysis` 父模块后，把 `analysis/usage`、`analysis/rankings`、`analysis/pressure` 一并纳入同一稳定 route dispatch，避免 parent shell 再次因为新子路由扩容出现 hook-order 分叉。
- 旧 `/admin/users/usage` 与 `/admin/rankings` 被收敛为 route alias，而不是继续保留平行一级模块入口。

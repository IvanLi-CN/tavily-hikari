# 规格（Spec）总览

本目录用于管理工作项的规格与追踪，作为实现与验收的执行合同。

> Legacy compatibility: historical specs may still live under `docs/plan/**/PLAN.md`.
> New and updated specs must be maintained under `docs/specs/**/SPEC.md`.

## Index

|    ID | Title                                                  | Status           | Spec                                                       | Last       | Notes                                                                                                       |
| ----: | ------------------------------------------------------ | ---------------- | ---------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------------------- |
| bcpru | Web 响应式双设备与 Storybook 断点验收收敛              | 已完成（6/6）    | `bcpru-web-responsive-breakpoint-convergence/SPEC.md`      | 2026-03-04 | fast-track: responsive convergence, Storybook coverage, and boundary validation completed                   |
| vr67d | LinuxDo 登录复用既有 Token + 强制重登录 + 历史误建自愈 | 已完成（快车道） | `vr67d-linuxdo-token-rebind-relogin/SPEC.md`               | 2026-03-05 | hotfix: `/auth/linuxdo` use 303 to avoid POST body leak + fix upstream 405                                  |
| s2vd2 | 1:1 上游 Credits 计费（MCP + HTTP）                    | 已完成（快车道） | `s2vd2-upstream-credits-billing/SPEC.md`                   | 2026-03-07 | fast-flow 复跑完成：reserved credits 先验阻断、Research success-with-warning + minimum fallback 已同步规格  |
| 6xeyh | 多 Token 绑定保留已分配 token（不做历史回补）          | 进行中（快车道） | `6xeyh-multi-token-binding-preserve/SPEC.md`               | 2026-03-06 | PR #95；共享测试机 E2E 回归通过；review-loop 已完成并按契约保留“最新绑定优先”语义                           |
| w2t73 | 批量 Key 导入稳定性修复                                | 已完成（快车道） | `w2t73-batch-key-upsert-stability/SPEC.md`                 | 2026-03-04 | rollback+retry hardening landed; flaky case loop 50/50; PR #87 checks green; review-loop clear              |
| 8brtz | Server.rs 最小风险模块化重构                           | 已完成（快车道） | `8brtz-server-rs-modularization/SPEC.md`                   | 2026-03-04 | fast-track: modularized `src/server/**`; PR #88 checks green; review-loop converged                         |
| m4n7x | Admin URL Path 路由与模块化仪表盘重构                  | 进行中（快车道） | `m4n7x-admin-path-routing-modular-dashboard/SPEC.md`       | 2026-03-02 | admin path routes + modular shell + dashboard enrichment + future module skeletons                          |
| 45squ | 账户级配额迁移与登录后用户控制台                       | 已完成（快车道） | `45squ-account-quota-user-console/SPEC.md`                 | 2026-03-07 | PR #98；MCP probe contract fix landed with Storybook visual evidence for blocked/partial token detail state |
| 7hs2d | UserConsole Storybook 验收参数重构                     | 已完成（快车道） | `7hs2d-user-console-storybook-acceptance-controls/SPEC.md` | 2026-03-08 | PR #102 clean；Storybook acceptance controls/browser validation complete；checks green                      |
| pv69t | Admin 用户配额滑块稳定化与指数档位收敛                 | 已完成           | `pv69t-admin-user-quota-slider-stability/SPEC.md`          | 2026-03-07 | PR #99; follow-up of `45squ`; stable max + integer stages + formatted quota inputs                          |
| 3rb68 | PublicHome 未登录无令牌隐藏 Token 面板与令牌访问弹窗   | 进行中（快车道） | `3rb68-public-home-token-access-modal/SPEC.md`             | 2026-03-02 | hero 按钮统一；新增 Storybook 首屏全状态 + 页面级 token 弹窗；补充 dialog 降级兼容与首屏闪烁修复            |
| k884v | Token usage rollup 幂等修复                            | 已完成           | `k884v-token-usage-rollup-idempotency/SPEC.md`             | 2026-03-02 | normal-flow, logic-only (no historical rebuild)                                                             |
| v9k2m | Release 版本回退防护与 latest 稳定修复                 | 已完成           | `v9k2m-release-version-regression-guard/SPEC.md`           | 2026-02-28 | fix semver detection + stable monotonic guard                                                               |
| jy9mu | Admin API Keys 文本提取（tvly-dev）                    | 已完成           | `jy9mu-admin-api-keys-text-extraction/SPEC.md`             | 2026-02-28 | fast-track, PR #78                                                                                          |
| rqbqk | Web shadcn/ui 全量重构与双主题品牌化                   | 已完成           | `rqbqk-shadcn-ui-rebuild/SPEC.md`                          | 2026-02-27 | full UI migration to shadcn/ui + dual theme                                                                 |
| kgakn | Admin API Keys 校验对话框与可用入库（迁移）            | 已完成           | `kgakn-admin-api-keys-validation-dialog/SPEC.md`           | 2026-02-24 | migrated from `docs/plan/kgakn:admin-api-keys-validation-dialog/PLAN.md`                                    |
| 2fd7v | Admin API Keys 入库后自动关闭与已入库隐藏              | 已完成           | `2fd7v-admin-api-keys-import-auto-close/SPEC.md`           | 2026-02-26 | follow-up of `kgakn`                                                                                        |
| rg5ju | LinuxDo 登录入口与自动填充 Token                       | 已完成（M1-M4）  | `rg5ju-linuxdo-login-token-autofill/SPEC.md`               | 2026-02-26 | fast-track                                                                                                  |

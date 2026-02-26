# 规格（Spec）总览

本目录用于管理工作项的**规格与追踪**：记录范围、验收标准、任务清单与状态，作为交付依据；实现与验证应以对应 `SPEC.md` 为准。

> Legacy compatibility: historical entries may still point to `docs/plan/**/PLAN.md`. New entries are created under `docs/specs/**/SPEC.md`.

## 快速新增一个规格

1. 生成一个新的规格 `ID`（推荐 5 个字符 nanoId 风格）。
2. 新建目录：`docs/specs/<id>-<title>/`。
3. 在目录下创建 `SPEC.md`（必要时补充 `contracts/`）。
4. 在 Index 增加一行并更新 `Last`。

## Index（固定表格）

|    ID | Title                                | Status          | Spec                                                           | Last       | Notes       |
| ----: | ------------------------------------ | --------------- | -------------------------------------------------------------- | ---------- | ----------- |
|  0001 | request_logs 定时清理与统计口径保持  | 部分完成（5/5） | `../plan/0001:request-logs-gc/PLAN.md`                         | 2026-01-19 | Legacy plan |
| wsx8t | Caddy ForwardAuth Compose + CI       | 已完成          | `../plan/wsx8t:caddy-forwardauth-compose-ci/PLAN.md`           | 2026-01-31 | Legacy plan |
| e5g7f | Admin 登录（ForwardAuth + 内置登录） | 待实现          | `../plan/e5g7f-admin-login-modes/PLAN.md`                      | 2026-02-03 | Legacy plan |
| 2qhen | PR Label 发版（CI 后置 Release）     | 已完成          | `../plan/2qhen:pr-label-release/PLAN.md`                       | 2026-02-05 | Legacy plan |
| zsuyk | Admin API Keys 批量浮层互斥显示      | 已完成          | `../plan/zsuyk:admin-api-keys-batch-overlay-exclusive/PLAN.md` | 2026-02-18 | Legacy plan |
| 5zcj3 | Admin API Keys 分组录入与筛选        | 已完成          | `../plan/5zcj3:api-keys-grouping/PLAN.md`                      | 2026-02-19 | Legacy plan |
| 9b9w5 | Bun 工具链迁移（root + web + CI）    | 已完成          | `../plan/9b9w5:bun-migration/PLAN.md`                          | 2026-02-20 | Legacy plan |
| y5vgt | Devctl/Zellij 长驻开发服务对齐       | 已完成          | `../plan/y5vgt:devctl-zellij-dev-runtime/PLAN.md`              | 2026-02-20 | Legacy plan |
| kgakn | Admin API Keys 校验对话框与可用入库  | 已完成          | `../plan/kgakn:admin-api-keys-validation-dialog/PLAN.md`       | 2026-02-24 | Legacy plan |
| rg5ju | LinuxDo 登录入口与自动填充 Token     | 已完成（M1-M4） | `rg5ju-linuxdo-login-token-autofill/SPEC.md`                   | 2026-02-26 | fast-track  |

# request_logs 定时清理与统计口径保持（#0001）

## 状态

- Status: 部分完成（5/5）
- Created: 2026-01-19
- Last: 2026-01-19

## 背景 / 问题陈述

- `request_logs` 作为请求审计与排障数据源，会随请求量持续增长，缺少定期清理机制会带来存储与性能风险。
- 需要增加定时任务，每日固定时刻执行“清理”，并支持通过环境变量调整运行时刻与保留天数。
- 约束：不接受**统计数据**丢失（清理不能导致累计统计回退或减少）。
  - 允许删除超过保留窗口的历史 `request_logs`（不归档），但必须保证统计口径保持全历史累计。

## 目标 / 非目标

### Goals

- 增加一个定时任务：每日 07:00 运行（可通过环境变量配置为任意 `HH:mm`）。
- 默认保留至少 7 天的 `request_logs`（保留天数可通过环境变量配置；且需强制下限为 7）。
- 调整统计口径，确保 Admin 侧“累计类统计”在清理后仍保持全历史含义，不因清理而回退。
- 在 `scheduled_jobs` 中记录每次任务的开始/结束、结果与处理量，便于审计与排障。

### Non-goals

- 不在本计划内解决多实例部署下的全局互斥/分布式锁问题（若存在多实例需求，另开计划）。
- 不提供 `request_logs` 的冷归档（归档到文件/对象存储/独立 DB 等不在范围内）。
- 不在本计划内新增复杂的 UI（仅做为保持既有语义所必需的最小实现调整）。

## 范围（Scope）

### In scope

- 新增 `request_logs` 的“每日定时清理”机制（可配置运行时刻与保留天数）。
- 在主 DB 中按 retention 删除过旧 `request_logs`（不做归档；允许永久删除，统计保持全历史累计）。
- 调整统计实现，使现有 HTTP API 的统计字段在语义上保持“全历史累计”。
- 增加必要的测试与文档更新。

### Out of scope

- 细粒度的按 token/group/key 的差异化 retention 策略。
- 对被删除日志提供查询/回放能力（如需另开计划）。

## 需求（Requirements）

### MUST

- 定时任务
  - 每日运行一次，默认运行时刻为 07:00（服务器本地时区）。
  - 支持环境变量配置运行时刻，格式为 `HH:mm`（24 小时制），非法值必须回退到默认值并记录告警日志。
- 保留策略
  - 默认保留天数为 7 天。
  - 支持环境变量配置保留天数；配置值必须强制下限为 7（小于 7 按下限处理或回退默认值，见契约）。
  - 保留窗口按“自然日边界”定义（以服务器本地时区的 00:00 为界）。
- Rollup / 桶（用于统计不丢）
  - 写入 `request_logs` 时，必须同步写入“按 API key 的统计桶”，用于支撑全历史累计与趋势分析。
  - 统计桶的写入必须与 `request_logs` 写入保持原子性（同一事务），避免出现“日志写了但桶没写”导致后续清理后统计缺口。
  - 统计桶至少支持按 `result_status` 维度拆分（success / error / quota_exhausted），并可聚合得到全历史累计。
- 统计口径
  - Admin 侧既有统计字段（如 API key 的累计成功/失败/总请求数）在清理后必须仍表示“全历史累计”。
  - 统计数据源改为 rollup 桶后，必须定义一次性回填策略，并可在迁移后校验与原始数据一致。
- 可观测性
  - 每次任务执行需要在 `scheduled_jobs` 留痕：开始/结束、状态、处理行数、耗时或关键指标。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                           | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）   | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                              |
| ------------------------------------------------------ | ------------ | ------------- | -------------- | -------------------------- | --------------- | ------------------- | ------------------------------------------ |
| `REQUEST_LOGS_GC_AT` / `REQUEST_LOGS_RETENTION_DAYS`   | Config       | internal      | New            | `./contracts/config.md`    | backend         | ops                 | 运行时刻与保留天数                         |
| Request log retention / rollup buckets                 | DB           | internal      | Modify         | `./contracts/db.md`        | backend         | backend             | 引入 rollup 桶与回填（保持全历史累计语义） |
| `/api/summary` / `/api/keys` / `/api/keys/:id/metrics` | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | backend         | web                 | 统计语义保持全历史累计                     |
| `/api/logs` / `/api/keys/:id/logs`                     | HTTP API     | internal      | Modify         | `./contracts/http-apis.md` | backend         | web                 | 清理后日志可见范围                         |

### 契约文档（按 Kind 拆分）

创建 `docs/plan/0001:request-logs-gc/contracts/`，并只链接本计划需要的契约文件：

- [contracts/README.md](./contracts/README.md)
- [contracts/config.md](./contracts/config.md)
- [contracts/http-apis.md](./contracts/http-apis.md)
- [contracts/db.md](./contracts/db.md)

## 验收标准（Acceptance Criteria）

- Given 服务运行且启用默认配置
  When 到达当日 07:00
  Then 触发一次 `request_logs` 定时任务，并在 `scheduled_jobs` 中记录成功/失败与处理量。

- Given `REQUEST_LOGS_GC_AT=23:30`
  When 服务运行
  Then 定时任务在每日 23:30 触发；当配置为非法值（如 `7:00`、`25:00`、空字符串）时回退到默认值并记录告警。

- Given 默认保留 7 天且已有超过 7 天的历史请求日志
  When 定时任务执行
  Then 超过阈值（自然日边界）的历史日志被从主 DB 中删除；统计口径不受影响（累计语义仍为全历史累计）。

- Given 清理已发生
  When Admin 查询统计接口（`/api/summary`、`/api/keys`、`/api/keys/:id/metrics`）
  Then 返回的累计统计语义仍为“全历史累计”，不会因清理而回退或减少。

- Given 定时任务执行失败
  When 任务结束
  Then `scheduled_jobs` 记录失败原因，并保证不会产生“统计回退或不一致”（任务失败不得影响累计统计正确性）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests:
  - `HH:mm` 解析与默认回退行为（含边界：`00:00`、`23:59`、空/非法格式）。
  - retention days 的解析、默认值与下限策略。
- Integration tests:
  - 基于临时 SQLite DB 构造 `request_logs` 历史数据，验证 retention 删除的幂等性（重复运行不会误删未到期数据）。
  - 验证统计接口在清理后仍与清理前一致（全历史累计）。

### Quality checks

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

## 文档更新（Docs to Update）

- `docs/plan/README.md`: 新增计划索引（本计划完成后更新状态与 Notes）。
- `docs/plan/0001:request-logs-gc/contracts/*`: 作为接口契约的长期依据（实现阶段需保持一致）。

## 实现里程碑（Milestones）

- [x] M1: 定义并落地按 API key 的 rollup 桶（按日）与回填/校验策略
- [x] M2: 增加每日定时任务（支持 `HH:mm` 配置）并记录 `scheduled_jobs`
- [x] M3: 实现 `request_logs` retention（默认 7 天、可配置且强制下限）并确保幂等/可恢复
- [x] M4: 切换统计实现，保证对外字段与“全历史累计”语义不变
- [x] M5: 增加测试与运行手册/说明（含失败处理与排障路径）

## 方案概述（Approach, high-level）

为满足“清理 + 统计不丢 + 控制在线数据集规模”，建议采用“热数据 retention + rollup 桶”的组合（尽量少表）：

- 热数据：主 SQLite DB 中仅保留最近 N 天的 `request_logs`，用于 UI 展示最近请求、快速排障。
- 统计：把“全历史累计”统计从“扫描 `request_logs`”迁移为“对 `request_logs` 的 rollup 桶”，并提供一次性回填与一致性校验。

关于“从 `auth_token_logs` 计算统计”的可行性结论：

- `auth_token_logs` 当前不包含 `api_key_id`，无法直接支撑“按 API key 的全历史累计统计”。
- 若坚持从 `auth_token_logs` 作为统计源，需要在后续实现中增加可关联到 `api_key_id` 的字段/映射（属于 DB + 写入路径变更），并补齐历史回填策略。

## 风险与开放问题（Risks & Open Questions）

- 风险：
  - 定时任务对时区与 DST 的处理不明确，可能导致触发偏移或重复触发。
  - 统计从扫描日志迁移为 rollup 桶后，需要严格的一致性校验与回填策略，避免出现口径漂移。

## 开放问题（需要主人决策）

None

## 假设（Assumptions，需要主人确认）

- 定时任务采用服务器本地时区（`Local`）。
- 不提供归档；被 retention 命中的 `request_logs` 将被永久删除，但累计统计保持全历史语义。

## 变更记录（Change log）

- 2026-01-19: 创建计划。
- 2026-01-19: 完成实现：新增 `api_key_usage_buckets` 作为统计 rollup 桶；写入 `request_logs` 时同事务更新桶；新增 `request_logs_gc` 每日任务（`REQUEST_LOGS_GC_AT`，默认 07:00）按本地自然日边界清理 `request_logs`；统计接口改从桶聚合以保持全历史累计语义。

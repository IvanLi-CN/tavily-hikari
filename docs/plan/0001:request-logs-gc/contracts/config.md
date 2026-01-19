# Config（环境变量）

## Request logs GC schedule

- Name: `REQUEST_LOGS_GC_AT`
- Scope: internal
- Change: New
- Type: `HH:mm`（24 小时制）
- Default: `07:00`
- Validation:
  - 必须匹配 `^\\d{2}:\\d{2}$`
  - `00 <= HH <= 23`
  - `00 <= mm <= 59`
  - 非法值：回退到默认值，并输出告警日志（warning）
- Semantics:
  - 每日触发一次“request_logs 清理任务”
  - 时区：服务器本地时区（`Local`）

## Request logs retention days

- Name: `REQUEST_LOGS_RETENTION_DAYS`
- Scope: internal
- Change: New
- Type: integer（天）
- Default: `7`
- Validation:
  - 解析失败或小于下限：按下限处理或回退默认值（以 `PLAN.md` 决策为准）
  - 强制下限：`>= 7`
- Semantics:
  - 在线热数据集（primary DB）中保留最近 N 天的 `request_logs`（按自然日边界计算）
  - “自然日边界”：以服务器本地时区的 00:00 为界；阈值为“今天 00:00 - (N-1) 天”
  - 超过阈值的记录将从热数据集中永久删除（不归档）；累计统计保持全历史语义

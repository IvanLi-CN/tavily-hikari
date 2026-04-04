# HTTP APIs

## User-facing daily window parameters

适用接口：

- `GET /api/user/dashboard`
- `GET /api/user/tokens`
- `GET /api/user/tokens/:id`
- `GET /api/public/metrics`
- `GET /api/token/metrics`
- `GET /api/public/events`
- `GET /api/tavily/usage`

### Query

- `today_start=<ISO8601 datetime with offset>`
- `today_end=<ISO8601 datetime with offset>`

### Rules

- 两个参数必须成对出现；若都缺失，则回退到服务器时区今日窗口。
- 必须是可解析的 RFC3339 / ISO8601 时间，且必须包含显式 offset 或 `Z`。
- `today_start` 与 `today_end` 都必须对齐各自时区的本地午夜。
- `today_end` 必须晚于 `today_start`，且仅允许一个自然日窗口。
- 非法参数返回 `400`。

### Semantics

- `dailySuccess` / `dailyFailure`：按显式窗口聚合；回退时按服务器时区今日窗口。
- `monthlySuccess`：始终按 UTC 当前月聚合。
- `quotaDailyUsed` / `quotaDailyLimit`：服务器时区自然日。
- `quotaMonthlyUsed` / `quotaMonthlyLimit`：UTC 月。

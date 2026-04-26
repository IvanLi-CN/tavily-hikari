# 低余额 432 Key 本月降级（#726a7）

## 状态

- Status: 进行中（快车道）

## 背景

线上运行数据表明，少量上游 Key 在剩余额度很低时会反复返回 Tavily `432`
usage-limit 响应，又因偶发小请求成功被自动恢复到 `active`。这会把接近耗尽的 Key
重新丢回本月正常池，造成请求抖动与重复失败。

## Goals

- 当 Key 返回上游 `432`，且最新同步的 `quota_remaining <= LOW_QUOTA_DEPLETION_THRESHOLD`
  时，记录该 Key 在当前 UTC 月的低余额耗尽状态。
- 本月低余额耗尽 Key 不进入正常 `active` / 普通 `exhausted` 候选池；调度顺序固定为
  `active -> regular exhausted -> low-quota depleted exhausted`。
- 保留低余额耗尽 Key 的最终兜底能力；只有当没有其它 active 或普通 exhausted Key
  可用时才会尝试。
- 本月低余额耗尽 Key 即使后续请求成功，也不得触发自动 `auto_restore_active`。
- 默认阈值为 `15` credits，可通过 `LOW_QUOTA_DEPLETION_THRESHOLD` 配置；非法值回退默认值并记录启动警告。

## Non-goals

- 不新增用户可见 Key status，不改 admin UI。
- 不改变下游 token/account quota 计费、request log 分类或业务 credits 口径。
- 不重写历史日志，也不对历史 432 自动回填低余额耗尽记录。

## 数据契约

- 新增 `api_key_low_quota_depletions`：
  - `key_id TEXT NOT NULL`
  - `month_start INTEGER NOT NULL`
  - `threshold INTEGER NOT NULL`
  - `quota_remaining INTEGER NOT NULL`
  - `created_at INTEGER NOT NULL`
  - primary key: `(key_id, month_start)`
- `month_start` 使用既有 UTC 月起点口径，与 `reset_monthly()` 一致。
- 重复命中同一 Key / 月份时保持一条记录，并保留更低的 `quota_remaining`。

## 验收标准

- `quota_remaining = 15` 且上游返回 `432` 时写入当前月低余额耗尽记录。
- `quota_remaining = 16` 且上游返回 `432` 时不写入低余额耗尽记录。
- 选择顺序优先 active，其次普通 exhausted，最后才是本月低余额耗尽 exhausted。
- 本月低余额耗尽 Key 成功请求不会自动恢复 active。
- 旧月份低余额耗尽记录不影响当前月恢复与选择。

## 质量门槛

- `cargo fmt --all`
- `cargo test`
- `cargo clippy -- -D warnings`

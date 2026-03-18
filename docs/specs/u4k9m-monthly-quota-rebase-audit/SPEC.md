# u4k9m · 月配额重基线与共享测试机积分验真

## Summary

- 线上 `101` 的 `ai-tavily-hikari` 已确认健康，最近 24 小时 `auth_token_logs` 中 `billing_state='charged'` 的 billable 记录没有 pending / retry 悬挂。
- 当前账本异常仅出现在月窗口：小时/日窗口与 `charged credits` 对齐，但 `auth_token_quota` / `account_monthly_quota` 仍保留 credits cutover 前的旧请求口径残留。
- 本工作项将新增一个一次性、幂等的“当前 UTC 月月配额重基线”迁移，以 `auth_token_logs.billing_state='charged' AND business_credits>0` 为唯一真相源，重写本月 token/account 月配额。
- 同时新增一个确定性的账本审计入口，用于比较 hour/day/month 三窗口上的 charged ledger 与 quota-table diff，并在共享测试机复制的线上数据库上复现和验证修复结果。
- 最终需要在 `codex-testbox` 的隔离环境中执行 1 次真实扣费探针，确认 `Δupstream usage = Δcharged credits = Δaccount quota windows`。

## Functional/Behavior Spec

### Monthly quota rebase

- 新增一条新的 DB meta gate，确保“当前 UTC 月月配额重基线”默认只自动运行一次。
- 重基线只修改：
  - `auth_token_quota.month_count`
  - `account_monthly_quota.month_count`
- 重基线绝不修改：
  - `token_usage_buckets`
  - `account_usage_buckets`
  - `auth_token_logs`
  - 任何小时/日窗口的聚合 bucket
- 真相源固定为：
  - `auth_token_logs.billing_state = 'charged'`
  - `COALESCE(auth_token_logs.business_credits, 0) > 0`
  - `created_at >= current UTC month_start`
- 聚合键固定为 `billing_subject`：
  - `account:<user_id>` 汇总后写入 `account_monthly_quota`
  - `token:<token_id>` 汇总后写入 `auth_token_quota`
- 对本月无 charged rows 的主体，本月 `month_count` 直接归零。
- 对已经绑定到账户的 token，重基线后允许其 token-level 月账变为 `0`；真实可见月账应由 `snapshot_many()` 与用户控制台接口继续从账户主体读取。
- 该迁移不尝试恢复 `charged ledger` 之外、当前无法证明的本月早期历史 credits。

### Ledger audit

- 新增一个可重复执行的账本审计入口，输出每个 `billing_subject` 在当前 UTC hour/day/month 三窗口上的：
  - charged ledger totals
  - quota table / bucket totals
  - diff
- 审计只检查：
  - `token:*`
  - `account:*`
- 审计失败条件：
  - 任意主体的 hour/day/month 任一窗口 diff 非 `0`
- 审计输出必须足够稳定，便于共享测试机脚本在“修复前 / 修复后”两次运行中直接比较。

### Shared testbox validation

- 线上数据库只能以只读快照方式从 `101` 导出到 `codex-testbox` 的 run 目录中。
- `codex-testbox` 上的 Docker/Compose 测试必须遵守：
  - 仅写入 `/srv/codex/**`
  - 唯一 `COMPOSE_PROJECT`
  - 只清理当前 run 产生的资源
  - LXC caps override 由 `.codex.caps-compat.yaml` 提供
- 测试机实例必须运行当前分支实现，而不是线上 `latest` 镜像。
- 测试机实例使用复制的线上数据库启动，先复现 month-only diff，再执行重基线并复查 diff 归零。

### Live probe verification

- 真实扣费探针默认使用 owner-owned token `ZjvC`，其绑定账户为 `jTl8MDlzuejK`。
- 探针执行路径使用现有 token API 测试功能，不在生产站点做写操作。
- 探针前后必须同步/读取命中的上游 `api_key_id` usage，并核对：
  - `Δupstream key usage`
  - `Σ新增 charged business_credits`
  - `Δaccount:jTl8MDlzuejK.hour`
  - `Δaccount:jTl8MDlzuejK.day`
  - `Δaccount:jTl8MDlzuejK.month`
- 对 `ZjvC`，探针后 token-level 原始月账仍应保持 `0`。

## Acceptance

- 复制的线上数据库在修复前能稳定复现：
  - hour diff 数量为 `0`
  - day diff 数量为 `0`
  - month-only diff 数量大于 `0`
  - 包含 `token:ZjvC` 与 `account:jTl8MDlzuejK` 样本
- 月配额重基线后，同一份数据库上的所有 `token:*` / `account:*` 主体在 hour/day/month 三窗口 diff 全部归零。
- 修复前后 `auth_token_logs` 中 charged rows 数量与 charged credits 总量不发生变化。
- live probe 后，新增 charged rows 的 credits 总和，精确等于被命中上游 key 的 usage 差值，并与账户 hour/day/month 增量一致。
- `ZjvC` 在 live probe 后仍只增长到账户月账，不重新污染 token-level 月账。

## Verification

- `cargo fmt --all`
- `cargo test monthly_quota_ -- --nocapture`
- `cargo test billing_ledger_ -- --nocapture`
- `cargo test bound_token_ -- --nocapture`
- `cargo clippy -- -D warnings`
- `codex-testbox` 隔离实例：
  - 修复前运行账本审计并记录 month-only diff
  - 应用重基线后再次运行审计并确认 diff 归零
  - 执行 1 次真实扣费探针并保存前后 usage / quota 证据

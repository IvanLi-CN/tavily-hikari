## account_quota_limits

- 不新增表或列。
- 新行为仅影响“新插入行”的默认值：
  - 新用户首次通过 `ensure_account_quota_limits*` 落库时，写入
    - `hourly_any_limit = 0`
    - `hourly_limit = 0`
    - `daily_limit = 0`
    - `monthly_limit = 0`
    - `inherits_defaults = 0`
- 已存在的 `account_quota_limits` 行不做自动迁移。
- 启动期 `sync_account_quota_limits_with_defaults()` 仍只同步历史 `inherits_defaults = 1` 行，且继续使用旧 token/env 默认 tuple，而不是零基线。

## user_tags / user_tag_bindings

- 无 schema 变化。
- `linuxdo_l*` 系统标签的默认 delta 继续沿用旧 token/env 默认额度映射。

## auth_tokens / token quota

- 无 schema 变化。
- token 级默认额度与相关 env 变量语义保持不变。

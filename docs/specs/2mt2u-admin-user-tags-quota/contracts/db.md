# 数据库（DB）

## 用户标签与额度聚合模型

- 范围（Scope）: internal
- 变更（Change）: Extend
- 影响表（Affected tables）:
  - `user_tags`
  - `user_tag_bindings`
  - `account_quota_limits`
  - `oauth_accounts`

### Schema delta（结构变更）

- `user_tags`
  - `id TEXT PRIMARY KEY`
  - `name TEXT NOT NULL UNIQUE`
  - `display_name TEXT NOT NULL`
  - `icon TEXT`
  - `system_key TEXT UNIQUE`
  - `effect_kind TEXT NOT NULL DEFAULT 'quota_delta'` (`quota_delta` | `block_all`)
  - `hourly_any_delta INTEGER NOT NULL DEFAULT 0`
  - `hourly_delta INTEGER NOT NULL DEFAULT 0`
  - `daily_delta INTEGER NOT NULL DEFAULT 0`
  - `monthly_delta INTEGER NOT NULL DEFAULT 0`
  - `created_at INTEGER NOT NULL`
  - `updated_at INTEGER NOT NULL`

- `user_tag_bindings`
  - `user_id TEXT NOT NULL`
  - `tag_id TEXT NOT NULL`
  - `source TEXT NOT NULL` (`manual` | `system_linuxdo`)
  - `created_at INTEGER NOT NULL`
  - `updated_at INTEGER NOT NULL`
  - `PRIMARY KEY(user_id, tag_id)`
  - `FOREIGN KEY(user_id) REFERENCES users(id)`
  - `FOREIGN KEY(tag_id) REFERENCES user_tags(id)`

- `account_quota_limits`
  - 新增 `inherits_defaults INTEGER NOT NULL DEFAULT 1`
  - 语义：记录“基线额度是否继续跟随 env 默认值”

### Constraints / indexes

- `user_tags.name` 全局唯一。
- `user_tags.system_key` 对系统标签唯一；custom tag 为 `NULL`。
- `idx_user_tag_bindings_user_updated` on `user_tag_bindings(user_id, updated_at DESC)`。
- `idx_user_tag_bindings_tag_user` on `user_tag_bindings(tag_id, user_id)`。

### Migration notes（迁移说明）

- 启动时创建新表，并为 `account_quota_limits` 增列 `inherits_defaults`。
- 一次性 `inherits_defaults` 回填只会把“当前仍等于 env 默认 tuple”的历史行保留为默认跟随；其他 legacy tuple 保守视为自定义基线，避免升级时覆盖管理员手工额度。
- 初始化时 seed 5 个 LinuxDo 系统标签；重复启动必须幂等。
- LinuxDo 系统标签默认 delta 直接镜像旧 token 默认额度，自动同步绑定后会按普通 tag delta 参与有效额度叠加。
- 启动时对现有 LinuxDo 用户做一次回填：将 `trust_level` 映射为单一系统标签绑定。
- 之后每次 LinuxDo 登录时按最新 `trust_level` 更新系统绑定；`trust_level` 缺失/越界不自动删除旧绑定。

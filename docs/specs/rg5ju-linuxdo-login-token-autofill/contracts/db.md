# 数据库（DB）

## LinuxDo 用户认证与绑定模型

- 范围（Scope）: internal
- 变更（Change）: New
- 影响表（Affected tables）:
  - `users`
  - `oauth_accounts`
  - `user_sessions`
  - `user_token_bindings`
  - `oauth_login_states`

### Schema delta（结构变更）

- `users`
  - `id TEXT PRIMARY KEY`
  - `display_name TEXT`
  - `username TEXT`
  - `avatar_template TEXT`
  - `active INTEGER NOT NULL DEFAULT 1`
  - `created_at INTEGER NOT NULL`
  - `updated_at INTEGER NOT NULL`
  - `last_login_at INTEGER`

- `oauth_accounts`
  - `id INTEGER PRIMARY KEY AUTOINCREMENT`
  - `provider TEXT NOT NULL`
  - `provider_user_id TEXT NOT NULL`
  - `user_id TEXT NOT NULL`
  - `username TEXT`
  - `name TEXT`
  - `avatar_template TEXT`
  - `active INTEGER NOT NULL DEFAULT 1`
  - `trust_level INTEGER`
  - `raw_payload TEXT`
  - `created_at INTEGER NOT NULL`
  - `updated_at INTEGER NOT NULL`
  - `UNIQUE(provider, provider_user_id)`
  - `FOREIGN KEY(user_id) REFERENCES users(id)`

- `user_sessions`
  - `token TEXT PRIMARY KEY`
  - `user_id TEXT NOT NULL`
  - `provider TEXT NOT NULL`
  - `created_at INTEGER NOT NULL`
  - `expires_at INTEGER NOT NULL`
  - `revoked_at INTEGER`
  - `FOREIGN KEY(user_id) REFERENCES users(id)`

- `user_token_bindings`
  - `user_id TEXT PRIMARY KEY`
  - `token_id TEXT NOT NULL UNIQUE`
  - `created_at INTEGER NOT NULL`
  - `updated_at INTEGER NOT NULL`
  - `FOREIGN KEY(user_id) REFERENCES users(id)`
  - `FOREIGN KEY(token_id) REFERENCES auth_tokens(id)`

- `oauth_login_states`
  - `state TEXT PRIMARY KEY`
  - `provider TEXT NOT NULL`
  - `redirect_to TEXT`
  - `created_at INTEGER NOT NULL`
  - `expires_at INTEGER NOT NULL`
  - `consumed_at INTEGER`

### Constraints / indexes

- `idx_oauth_accounts_user` on `oauth_accounts(user_id)`
- `idx_user_sessions_user` on `user_sessions(user_id, expires_at DESC)`
- `idx_oauth_login_states_expire` on `oauth_login_states(expires_at)`

### Migration notes（迁移说明）

- 向后兼容窗口：新增表，不改动历史表结构。
- 发布步骤：应用启动时通过 `CREATE TABLE IF NOT EXISTS` + 索引创建完成初始化。
- 回滚策略：回滚到旧版本后新表将保留但不被读取，不影响旧路径。
- 回填策略：无需历史数据回填；首次登录按需创建用户与绑定。

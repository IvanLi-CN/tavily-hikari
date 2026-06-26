# HTTP APIs

## POST /auth/linuxdo

- Scope: external
- Change: New
- Auth: none
- Content-Type: `application/x-www-form-urlencoded`

### Form fields

- `token`（optional）: 完整 token，格式 `th-<id>-<secret>`。

### Response

- `303` 重定向到 LinuxDo authorize endpoint。
- 行为：
  - 当 `token` 合法且可解析时，服务端在 oauth state 中写入 `bind_token_id`。
  - 当 `token` 缺失/非法/不可用时，不写入 `bind_token_id`，继续普通登录流程。
  - 必须使用 `303 See Other`，避免浏览器在跟随重定向时保留原始 POST body（可能导致上游 authorize 端点 GET-only 返回 405，并有泄露表单字段风险）。

### Error

- `404` OAuth 未启用
- `500` 生成 oauth state 失败

## GET /auth/linuxdo

- Scope: external
- Change: Keep (兼容)
- Auth: none

### Response

- 行为保持兼容：发起 OAuth 登录，但不带候选 token。

## POST /auth/linuxdo/finalize

- Scope: external
- Change: Modify
- Auth: none

### Body

- `code` (required)
- `state` (required)

### Behavior changes

- 回调消费 oauth state 时同时读取 `bind_token_id`。
- 绑定优先级：
  1. 优先尝试绑定 `bind_token_id`（仅当 token 可用且不被他人绑定）；
  2. 失败则回退原有 `ensure_user_token_binding` 逻辑。
- 历史误建修复：若用户当前误绑了其他 token，可切回候选 token；被替换 token 保持 active + unbound。

### Error

- 与 finalize 既有行为一致（`400/403/502/5xx` 语义不变）。

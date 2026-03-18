# DB

## `forward_proxy_settings`

新增列：

- `egress_socks5_enabled INTEGER NOT NULL DEFAULT 0`
- `egress_socks5_url TEXT NOT NULL DEFAULT ''`

读取与写入规则：

- `egress_socks5_enabled=0` 时，`egress_socks5_url` 允许保留最近一次输入值。
- 启用全局 SOCKS5 前，必须先通过运行时校验；校验失败不得把 `egress_socks5_enabled` 写为 `1`。

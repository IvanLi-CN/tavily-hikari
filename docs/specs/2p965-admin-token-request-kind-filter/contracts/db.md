# DB

## auth_token_logs

新增列：

- `request_kind_key TEXT`
- `request_kind_label TEXT`
- `request_kind_detail TEXT`

## Write rules

- 新写入 token 日志必须在 insert 时同步落盘三列。
- `request_kind_key` / `request_kind_label` 为筛选与展示主字段。
- `request_kind_detail` 为 mixed batch 等补充说明字段，可为空。

## Read rules

- 若新列为 `NULL`，读取侧必须基于已有 `method/path/query` 做 best-effort fallback。
- legacy `/mcp` 且无法恢复 JSON-RPC method 时，至少回退为 `mcp:raw:/mcp` / `MCP | /mcp`。

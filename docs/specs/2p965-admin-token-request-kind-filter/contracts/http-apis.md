# HTTP APIs

## GET /api/tokens/:id/logs/page

- Purpose: 返回 token detail 的请求记录分页结果，并支持按 request type 多选筛选。
- Query:
  - `page`: `integer >= 1`
  - `per_page`: `integer 1..200`
  - `since`: `ISO-8601 datetime`
  - `until`: `ISO-8601 datetime`
  - `request_kind`: 可重复 query 参数；每个值为稳定 key，例如 `api:search`、`mcp:search`
- Response:

```json
{
  "items": [
    {
      "id": 42,
      "method": "POST",
      "path": "/mcp",
      "query": null,
      "http_status": 200,
      "mcp_status": 0,
      "business_credits": 2,
      "request_kind_key": "mcp:search",
      "request_kind_label": "MCP | search",
      "request_kind_detail": null,
      "result_status": "success",
      "error_message": null,
      "created_at": 1773280557
    }
  ],
  "page": 1,
  "per_page": 20,
  "total": 3,
  "request_kind_options": [
    { "key": "api:search", "label": "API | search" },
    { "key": "mcp:search", "label": "MCP | search" },
    { "key": "mcp:tools/list", "label": "MCP | tools/list" }
  ]
}
```

## Contract rules

- `request_kind` 为 exact-match 多选；多个值之间按 OR 语义匹配。
- `request_kind_options` 基于当前 `token_id + since + until` 计算，不因当前 `request_kind` 过滤而丢失其它可选项。
- `request_kind_key` 为稳定机器值；`request_kind_label` 为 UI 直接展示值。
- `request_kind_detail` 仅在主 label 不足以表达批次内容时返回，例如 mixed MCP batch；其余场景允许为 `null`。

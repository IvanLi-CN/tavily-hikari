# Admin：API Keys 校验对话框与可用入库（kgakn）

## 背景

当前 `/admin` → API Keys 的“添加/批量导入”会直接入库，然后弹出 “Batch Import Report”。这会导致：

- 无法在入库前确认 key 是否可用；
- 用户无法看到逐条校验进度与失败原因；
- 无法一键重试失败项；也无法对 “remaining=0” 的 key 做明确处理。

## 目标

- 新增 API keys 后弹出“Key 校验”对话框：
  - 展示检测进度、摘要统计、逐条 key 列表（含状态、quota、重试按钮）。
  - 鼠标悬浮/点击（移动端 focus）可查看详细错误原因气泡。
- 校验完成后：由用户手动点击 “Import N valid keys” 才入库。
- 支持一键批量重试失败项 + 单条重试。
- `/usage` 校验成功但 `remaining=0`：视为“有效但已耗尽”，仍入库并将该 key 在 DB 中标记为 `exhausted`。

## 非目标

- 不引入新的前端测试框架（保持现状：只做 `bun run build` 验证）。
- 不改变现有 `/api/keys/batch` 的默认行为（新增字段均为可选，旧客户端不受影响）。
- 不做后台常驻校验任务/SSE 推送（先用前端分批调用实现进度）。

## 范围（In / Out）

### In scope

- Backend
  - 新增 `POST /api/keys/validate`：对输入 key 调用 `/usage` 校验并返回逐条结果（不入库）。
  - 扩展 `POST /api/keys/batch`：新增可选 `exhausted_api_keys`，入库后按需标记 `exhausted`。
- Frontend
  - `/admin` 的 Add Key 流程改为：弹窗校验 → 手动确认入库 → 刷新数据。
  - 支持 “Retry failed / Retry one”。
  - 新增错误气泡样式（复用现有 bubble 交互模式）。

### Out of scope

- admin-only API 的权限体系改造（沿用现有 is_admin_request 逻辑）。
- quota/usage 的后台同步策略调整（沿用现状）。

## 验收标准（Given / When / Then）

### 校验对话框与进度

- Given `/admin` 页面已加载，用户在 API Keys 录入框粘贴多行 keys
  - When 点击 Add Key
  - Then 立即弹出校验对话框，列表状态从 `pending` 逐步变为 `valid / exhausted / invalid / error / duplicate`
  - And 进度条显示 `checked / total_to_check` 并随校验推进更新

### 错误详情气泡

- Given 校验结果存在失败项
  - When 鼠标悬浮/点击失败项的结果（移动端 focus）
  - Then 显示包含详细错误信息的气泡（多行可读，长度可控）

### 手动确认入库 + exhausted 标记

- Given 校验完成且存在可用项（valid / valid_exhausted）
  - When 点击 “Import N valid keys”
  - Then 仅把可用项入库
  - And 对于 `remaining=0` 的可用项，入库后 key 状态为 `exhausted`
  - And 页面刷新后 keys 表格能显示该 key 为 `exhausted`

### 重试

- Given 校验完成且存在失败项
  - When 点击 “Retry failed”
  - Then 仅对失败项再次发起校验并更新其结果

## 测试计划

- Backend：`cargo test`
  - 覆盖 `/api/keys/validate`：ok / ok_exhausted / 401 / duplicate_in_input 等路径（使用 mock usage server）。
  - 覆盖 `/api/keys/batch` + `exhausted_api_keys`：入库后 key 状态被标记为 exhausted。
- Frontend：`cd web && bun run build`
- 手动：按验收标准在 `/admin` 页面回归一次交互流程。

## 实现里程碑

- [ ] Backend：实现 `/api/keys/validate`（含状态映射与 detail 截断）
- [ ] Backend：扩展 `/api/keys/batch` 支持 `exhausted_api_keys` 并标记 exhausted
- [ ] Frontend：新增校验对话框 + 进度/摘要/列表/错误气泡
- [ ] Frontend：实现单条/批量重试与手动入库
- [ ] 验证：cargo test + web build + 手动回归

## 风险与备注

- 注意避免在测试中请求真实 Tavily 生产端点：所有自动化测试必须使用本地 mock usage server。

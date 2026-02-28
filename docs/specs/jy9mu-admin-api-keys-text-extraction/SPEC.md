# Admin：API Keys 文本提取（tvly-dev）（#jy9mu）

## 状态

- Status: 已完成
- Created: 2026-02-28
- Last: 2026-02-28

## 背景 / 问题陈述

- 目前 Admin 端批量录入 API key 依赖“每行就是 key”的输入形式。
- 迁移场景中常见 `邮箱 => key` 或带附加文本的行，直接粘贴会把整行送去校验，导致噪音与手工清洗成本。

## 目标 / 非目标

### Goals

- 在前端批量录入入口支持“按行提取 key”：每行提取首个 `tvly-dev-[A-Za-z0-9_-]+`。
- 提取后沿用现有校验与入库链路，不改后端协议。
- 未匹配行直接忽略，不新增单独统计字段。

### Non-goals

- 不修改 `/api/keys/batch` 与 `/api/keys/validate` 的请求/响应结构。
- 不支持 `tvly-dev-*` 以外前缀的自动提取。
- 不引入后端提取逻辑。

## 范围（Scope）

### In scope

- `web/src/lib/api-key-extract.ts`：
  - 新增按行提取函数：`extractTvlyDevApiKeyFromLine`、`extractTvlyDevApiKeysFromText`
- `web/src/AdminDashboard.tsx`：
  - `keysBatchParsed` 改为基于提取结果计数
  - `handleAddKey` 改为提交提取后的 key 列表
- `web/src/i18n.tsx`：
  - 更新中英文 placeholder/hint/validation hint 文案，说明支持文本提取与忽略未匹配行

### Out of scope

- `src/server.rs` 与后端 API 行为无改动。
- 不新增数据库变更与迁移。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                                  | 类型（Kind）  | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                            |
| --------------------------------------------- | ------------- | ------------- | -------------- | ------------------------ | --------------- | ------------------- | ---------------------------------------- |
| `extractTvlyDevApiKeyFromLine`                | TypeScript fn | internal      | Add            | None                     | frontend        | AdminDashboard      | 返回每行首个 `tvly-dev-*` 或 `null`      |
| `extractTvlyDevApiKeysFromText`               | TypeScript fn | internal      | Add            | None                     | frontend        | AdminDashboard      | 按行提取并忽略未匹配行                   |
| `keysBatchParsed` 计算语义                    | React state   | internal      | Modify         | None                     | frontend        | AdminDashboard      | 从“非空行”改为“已提取 key 数”            |
| `POST /api/keys/validate` & `/api/keys/batch` | HTTP API      | external      | None           | Existing                 | backend         | admin web           | 前端入参变为提取后 key；协议保持完全兼容 |

## 验收标准（Acceptance Criteria）

- Given 粘贴 `name@example.com => tvly-dev-xxx`
  When 点击“添加密钥”
  Then 校验与入库链路仅接收 `tvly-dev-xxx`，不包含邮箱与箭头文本。
- Given 同一批次混入未匹配文本行
  When 点击“添加密钥”
  Then 未匹配行被忽略，不阻断流程，不额外显示未匹配计数。
- Given 同一 key 在提取结果中重复
  When 进入校验弹窗
  Then 仍按现有语义计入 `duplicate_in_input`。
- Given 仅有未匹配文本（无可提取 key）
  When 查看“添加密钥”按钮状态
  Then 按钮不可点击（提取后 key 数为 0）。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Frontend build: `cd web && bun run build`

### Manual check

- `/admin` 手工粘贴“邮箱 => key”样例并确认计数、校验列表与入库行为符合预期。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增前端提取函数并覆盖 `tvly-dev-*` 规则
- [x] M2: `AdminDashboard` 批量逻辑接入提取函数
- [x] M3: 中英文提示文案更新
- [x] M4: 前端构建与手工验证通过

## 风险 / 开放问题 / 假设

- 风险：规则目前限制为 `tvly-dev-*`，若后续 key 前缀扩展需要再调整提取策略。
- 假设：当前业务 key 前缀在本场景固定为 `tvly-dev-*`。

## 变更记录（Change log）

- 2026-02-28: 新建规格，定义前端文本提取方案与验收标准。
- 2026-02-28: 实现完成，前端支持逐行提取 `tvly-dev-*`，并完成构建与 `/admin` 交互验证。
- 2026-02-28: review-loop 对齐后确认 `tvly-dev-*` 限制为既定产品约束，未扩展到其他前缀。

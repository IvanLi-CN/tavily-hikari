# Linked Worktree Bootstrap Contract（#w7tbk）

## 状态

- Status: 已实现（待审查）
- Created: 2026-07-15
- Last: 2026-07-15

## 背景 / 问题陈述

- `tavily-hikari` 之前没有正式的 linked worktree bootstrap contract。新的 linked checkout 经常缺失 shared hooks、root / `web` / `docs-site` 依赖，以及本地 `.env` 文件。
- 现有 `scripts/start-frontend-dev.sh` 只会在启动前端时懒装 `web/node_modules`，无法覆盖 root 工具链、docs-site、shared hooks、`cargo fetch --locked` 或 primary worktree 到 linked worktree 的 env 同步。
- 仓库当前没有 `.env.example` 一类模板真相源，真实可复用的本地配置只能来自 primary worktree 自身。
- 本题目标是把“首次 linked checkout 自动恢复 + 显式 strict repair 入口”写成 repo current truth，而不是继续依赖人工记忆和零散启动脚本。

## Goals

- 建立 shared `post-checkout` hook 驱动的首次 linked checkout 自动恢复合同。
- 固定 env 同步语义为 primary worktree `copy-missing`：只复制缺失的 root `.env` / `.env.*` 常规文件，目标已存在文件绝不覆盖。
- 固定自动恢复范围为 shared hooks、root / `web` / `docs-site` Bun 依赖、以及 `cargo fetch --locked` 预热。
- 提供稳定的显式 repair 入口：`scripts/worktree-setup.sh` 与 root `package.json` `worktree:setup`。
- 用真实 linked worktree smoke test + CI job 锁住合同。

## Non-goals

- 不复制或恢复 `*.db`、`web/dist`、`web/storybook-static`、`downloads/`、浏览器 cache、Playwright 安装态或其他运行期产物。
- 不把“每次 checkout 都重新装依赖”做成默认行为；默认只做“首次自动 + 显式重跑”。
- 不引入 `.env.example` fallback、额外的 quality-gates 配置文件、或新的 worktree 端口租约机制。
- 不把这次合同升级成 `tavreg-hikari` 式的全运行态修复器。

## 范围（Scope）

### In scope

- `scripts/install-hooks.sh`
- `scripts/worktree-bootstrap.sh`
- `scripts/worktree-setup.sh`
- `scripts/test-worktree-bootstrap.sh`
- `package.json`
- `.github/workflows/ci.yml`
- `README.md`
- `README.zh-CN.md`
- `AGENTS.md`
- `docs/specs/README.md`

### Out of scope

- `scripts/start-backend-dev.sh`、`scripts/start-frontend-dev.sh` 的运行时语义本身不在本轮重写范围内。
- 任何数据库、静态构建产物、下载目录或浏览器运行期缓存不纳入恢复面。

## 合同（Contract）

### Trigger contract

- shared `post-checkout` hook 安装在 git common hooks 目录，由 `scripts/install-hooks.sh` 负责落盘。
- 自动 bootstrap 只对 linked worktree 生效；primary worktree 默认 no-op。
- linked worktree 首次自动恢复以 `.tmp/worktree-bootstrap.v1.done` 为本地 marker；后续普通 checkout 默认不再重复做重恢复。
- shared hook 必须对历史 revision 缺失 `scripts/worktree-bootstrap.sh` 的情况安全 no-op。

### Hook contract

- `scripts/install-hooks.sh` 会：
  - 安装 repo-managed shared `post-checkout` wrapper。
  - 若 common hooks 目录里已有 unmanaged `post-checkout`，将其保留为 `post-checkout.local` 并在 wrapper 末尾继续链式执行。
  - 若 `lefthook` 二进制存在于 `PATH`，补齐当前仓已有 pre-commit / commit-msg hooks；若缺失则只 warning，不阻断安装。

### Env contract

- env 来源固定为 primary worktree 根目录。
- 只复制 `.env` 与 `.env.*` 常规文件；模板类文件（如 `.env.example` / `.env.sample` / `.env.template` / `.env.dist`）不参与复制。
- 目标 worktree 已存在的 env 文件一律保留，不做覆盖。
- 若 primary worktree 没有可复制 env 文件，只 warning，不视为失败。

### Restore contract

- 自动路径会：
  - 复制缺失 env 文件。
  - 安装缺失的 root / `web` / `docs-site` Bun 依赖。
  - 执行一次 `cargo fetch --locked`。
- 显式 `scripts/worktree-setup.sh` / `bun run worktree:setup` 会：
  - 先重新安装 shared hooks。
  - 再以 `--manual --force --strict` 重跑 linked-worktree bootstrap。

### Failure semantics

- 自动路径固定为 `best-effort + warning`，不得阻断 checkout。
- 缺失 `lefthook`、`bun`、`cargo` 或 source env 文件时，只打印 warning。
- 显式 strict repair 只有在工具已存在但实际恢复命令失败时才返回非零退出码。

## 验收标准（Acceptance Criteria）

- Given 新的 linked worktree 首次 checkout
  When shared `post-checkout` hook 触发
  Then linked worktree 会获得缺失的 root `.env` / `.env.*`、缺失的 root / `web` / `docs-site` Bun 依赖、`cargo fetch --locked` 预热，以及 worktree-local marker。

- Given 同一个 linked worktree 已经写入 marker
  When 发生普通 checkout
  Then 自动 bootstrap 不会重复做依赖恢复或 `cargo fetch --locked`。

- Given 运行 `bun run worktree:setup`
  When 当前目录是 linked worktree
  Then shared hooks 会先被刷新，随后 bootstrap 会以 `force + strict` 重跑。

- Given 目标 linked worktree 已存在 `.env` 或 `.env.*`
  When 自动或手工 bootstrap 执行
  Then 这些目标 env 文件不会被 primary worktree 覆盖。

- Given 当前 revision 缺少 `scripts/worktree-bootstrap.sh`
  When shared `post-checkout` hook 在该 revision 触发
  Then hook 会安全 no-op，不报错阻断 checkout。

- Given 本机缺少 `lefthook`、`bun` 或 `cargo`
  When 自动 bootstrap 或 hook 安装运行
  Then 命令以 warning 方式降级，不把 linked checkout 直接打断。

## 测试与证据

- `bash scripts/test-worktree-bootstrap.sh`
- `.github/workflows/ci.yml` `Worktree Bootstrap Smoke`

## 变更记录（Change log）

- 2026-07-15: 创建 worktree bootstrap topic spec，冻结 shared `post-checkout`、primary `copy-missing` env、auto `best-effort`、manual strict repair，以及 DB/dist/runtime exclusion contract。
- 2026-07-15: 实现 `scripts/install-hooks.sh`、`scripts/worktree-bootstrap.sh`、`scripts/worktree-setup.sh`、`scripts/test-worktree-bootstrap.sh`、root package scripts、CI smoke、README/AGENTS current truth。

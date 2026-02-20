# Devctl/Zellij 长驻开发服务对齐

## 背景

当前仓库的开发服务长驻方式是 `nohup + PID 文件`（`logs/*.pid` + `logs/*.log`）。该方式可以工作，但不符合我们在 Codex 环境下的“最新方案”：使用 `~/.codex/bin/devctl` 将服务托管到 Zellij session 中，以跨 turn 持久运行，并统一落盘日志到 `<workspace>/.codex/logs/*.log`。

## 目标（Goals）

- 为 `scripts/start-backend-dev.sh` 与 `scripts/start-frontend-dev.sh` 增加 `FOREGROUND=1` 前台模式：
  - 不使用 `nohup`
  - 不后台化（不 `&`）
  - 不重定向到 `logs/*.log`（stdout/stderr 交给 `devctl` 捕获）
- 更新 `AGENTS.md`：将“长驻 dev server”的标准做法切换到 `devctl up ...`（符合 `$zellij-service-manager`）。
- 更新 `.gitignore`：忽略 `.codex/`，避免 devctl 日志造成 untracked 噪音。

## 非目标（Non-goals）

- 不改变默认端口策略（backend 58087 / frontend 55173）。
- 不引入新的运行时依赖，不改变 Rust/TS 产品逻辑。
- 不做“强制阻断 production Tavily upstream”的行为变更（如需另起安全 PR）。

## 范围（Scope）

### In

- `scripts/start-backend-dev.sh`
- `scripts/start-frontend-dev.sh`
- `AGENTS.md`
- `.gitignore`

### Out

- 任何与功能无关的重构（例如重写启动脚本结构、迁移日志目录约定等）。

## 验收标准（Acceptance Criteria）

1. `devctl up api -- env FOREGROUND=1 scripts/start-backend-dev.sh` 后：
   - `devctl status api` 显示 service session 存在
   - `curl -sSf http://127.0.0.1:58087/health` 返回 200
2. `devctl up web -- env FOREGROUND=1 scripts/start-frontend-dev.sh` 后：
   - `devctl status web` 显示 service session 存在
   - `curl -sSf http://127.0.0.1:55173/` 有响应（或 Playwright 可打开页面）
3. `devctl down api/web` 后：
   - 相应 `devctl status` 显示不存在
   - 端口不再被占用
4. `.gitignore` 生效：`.codex/logs/*` 不会出现在 `git status` 的 untracked 列表中。

## 测试策略（Testing）

- Static：`bash -n scripts/start-backend-dev.sh scripts/start-frontend-dev.sh`
- Runtime smoke（不需要产生生产 upstream 流量）：
  - `~/.codex/bin/devctl up api -- env FOREGROUND=1 scripts/start-backend-dev.sh`
  - `curl -sSf http://127.0.0.1:58087/health`
  - `~/.codex/bin/devctl logs api -n 200`
  - `~/.codex/bin/devctl up web -- env FOREGROUND=1 scripts/start-frontend-dev.sh`
  - `curl -sSf http://127.0.0.1:55173/`
  - `~/.codex/bin/devctl logs web -n 200`

## 里程碑（Milestones）

1. 脚本支持 `FOREGROUND=1` 前台模式（backend + frontend）
2. 文档对齐：`AGENTS.md` 写明 devctl 命令与规则
3. 本地最小验证：devctl up/status/logs/down + health 检查

## 交付（Delivery）

- PR #69

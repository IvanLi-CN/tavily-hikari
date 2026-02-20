# Bun 工具链迁移（root + web + CI）

## 背景

当前仓库前端与 CI 使用 Node + npm（存在 `package-lock.json` / `web/package-lock.json`，并在 workflows 中 `setup-node` + `npm ci`）。
为统一开发体验与加速依赖安装，希望迁移到 Bun。

## 目标

- root + `web/` 使用 Bun 安装依赖并执行脚本（`bun install` / `bun run` / `bunx`）。
- CI / release workflows 使用 Bun（不再依赖 `actions/setup-node` + `npm ci`）。
- 固定 bun 版本为 `1.3.9`（通过 `.bun-version`）。

## 非目标

- 不改 Rust 业务逻辑与功能行为。
- 不做与工具链无关的 UI/功能扩展。

## 范围（In scope）

- 新增 `.bun-version`。
- root + `web/`：删除 `package-lock.json`，生成并提交 Bun lockfile（以 bun 实际产出为准，仅保留一种格式）。
- 更新 `/scripts/start-frontend-dev.sh`：`bun install` + `bun run dev`。
- 更新 `/lefthook.yml`：`npx` → `bunx`（dprint / commitlint）。
- 更新 GitHub Actions：
  - `.github/workflows/ci.yml`
  - `.github/workflows/release.yml`
  - `oven-sh/setup-bun@v2` + `bun install` + `bun run build`
- 更新 README：开发/构建命令从 npm 改为 bun。

## 验收标准（Acceptance Criteria）

1. root：`bun install --frozen-lockfile` 成功。
2. `web/`：`bun install --frozen-lockfile` 成功。
3. `web/`：`bun run build` 成功，并生成 `web/dist/version.json`。
4. `scripts/start-frontend-dev.sh` 可启动（不要求长驻验证）。
5. CI/release workflow 不再依赖 `actions/setup-node`，改用 `oven-sh/setup-bun@v2`。

## 测试策略（Testing）

- 本地最小验证：
  - root：`bun install --frozen-lockfile`
  - `web/`：`bun install --frozen-lockfile && bun run build`
  - `web/dist/version.json` 存在性检查

## 里程碑（Milestones）

1. lockfiles + package.json scripts 完成
2. hooks + dev scripts 完成
3. CI + release workflows 完成
4. README 更新 + 本地 build 验证通过

## 风险

- 若 Bun runtime 对 Vite/TS 存在兼容性问题：优先修复；若无法在合理时间内修复，则需要重新冻结口径并回退到“bun 仅安装、Node 运行”。本计划不做隐性回退。

# Implementation：Release `amd64` smoke 启动加固与 `v0.36.7` 回填（#stxfn）

## 当前状态

- 状态：部分完成（3/4）
- 最近更新：2026-06-15

## 已落地实现

- release smoke orchestration 继续由 `.github/scripts/release-mcp-billing-smoke.sh` 负责，保留动态端口、统一 cleanup、mock/proxy readiness 与诊断输出。
- SQLite 账单 / 日志断言已抽离到 `.github/scripts/release_smoke_sqlite_check.py`，不再把 sidecar-aware 查询逻辑硬编码在 shell heredoc 里。
- 新 helper 复用了当前 sidecar 命名合同：`tavily_proxy.db -> tavily_proxy-observability.db`，优先读取 `observability.request_logs`，若 sidecar 缺失或当前布局仍是 legacy 单库，则回退到 `main.request_logs`。
- `release.yml` 的 backfill helper restore 现在同时覆盖 smoke shell 脚本与 SQLite 校验 helper，避免 checkout 旧 SHA 时只恢复一半辅助文件。
- 新增 `tests/test_release_smoke_sqlite_check.py`，覆盖 sibling sidecar、legacy 单库、双表并存优先级与缺表报错。

## 已完成验证

- `python3 -m py_compile .github/scripts/release_smoke_sqlite_check.py tests/test_release_smoke_sqlite_check.py`
- `bash -n .github/scripts/release-mcp-billing-smoke.sh`
- `python3 -m unittest tests/test_release_smoke_sqlite_check.py`

## 剩余缺口

- 仍需在当前修复分支完成一次本地完整 release smoke rehearsal，证明 helper 与真实容器启动链路一起工作。
- 仍需在 PR 合入后通过目标 SHA 的真实 `Release` workflow 回填或 rerun 关闭最终线上证据。

## 相关文件

- `.github/scripts/release-mcp-billing-smoke.sh`
- `.github/scripts/release_smoke_sqlite_check.py`
- `.github/workflows/release.yml`
- `tests/test_release_smoke_sqlite_check.py`

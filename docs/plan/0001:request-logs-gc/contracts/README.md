# 接口契约（Contracts）

本目录用于存放本计划的**接口契约**。为避免形状混杂，契约必须按 `Kind` **拆分成不同文件**（不要把 HTTP/RPC/Event/CLI/DB/File 等混在一个文件里）。

编写约定：

- 只保留本计划实际用到的契约文件（不用的不要创建/应删除）。
- `../PLAN.md` 是唯一的“接口清单（Inventory）”：每条接口都必须在那张表里出现。
- 在 `../PLAN.md` 的 `Contract Doc` 列，填入对应契约文件的相对路径（例如 `./contracts/http-apis.md`）。
- 修改既有接口时，契约里必须写清楚：
  - 变化点（旧 → 新）
  - 向后兼容期望
  - 迁移 / rollout 方案（feature flag、弃用周期、双写/回填等）

推荐文件（按需选择，不需要的不要创建）：

- `http-apis.md`
- `rpc.md`
- `events.md`
- `cli.md`
- `db.md`
- `config.md`

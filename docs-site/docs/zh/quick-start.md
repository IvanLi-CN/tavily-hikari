# 快速开始

## 前置依赖

- Rust `1.91+`
- Bun `1.3.10` 或与仓库锁定版本一致的更新版本
- 本地 Rust 构建所需的 SQLite 运行时依赖

## 本地后端 + 前端

```bash
# 后端
cargo run -- --bind 127.0.0.1 --port 58087

# 前端 dev server
cd web
bun install --frozen-lockfile
bun run --bun dev -- --host 127.0.0.1 --port 55173
```

后端健康检查地址：`http://127.0.0.1:58087/health`\
前端控制台地址：`http://127.0.0.1:55173`

## 注入 Tavily key

```bash
curl -X POST http://127.0.0.1:58087/api/keys \
  -H "X-Forwarded-User: admin@example.com" \
  -H "X-Forwarded-Admin: true" \
  -H "Content-Type: application/json" \
  -d '{"api_key":"key_a"}'
```

本地开发通常结合 ForwardAuth 模拟头，或直接启用 `DEV_OPEN_ADMIN=true`。

## Docker

```bash
docker run --rm \
  -p 8787:8787 \
  -v "$(pwd)/data:/srv/app/data" \
  ghcr.io/ivanli-cn/tavily-hikari:latest
```

镜像内已包含 `web/dist`，SQLite 数据默认写入 `/srv/app/data/tavily_proxy.db`。

## Docker Compose

```bash
docker compose up -d
```

仓库自带的 `docker-compose.yml` 会暴露 `8787` 并挂载持久化 volume。若你需要一个带
ForwardAuth 的入口样例，可直接参考仓库中的
[examples/forwardauth-caddy](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)。

## 可选的本地验收面

```bash
# Storybook
cd web
bun install --frozen-lockfile
bun run storybook

# docs-site
cd docs-site
bun install --frozen-lockfile
bun run dev
```

- Storybook 默认地址：`http://127.0.0.1:56006`
- docs-site 默认地址：`http://127.0.0.1:56007`

这两个本地服务会互相回链，行为与最终 GitHub Pages 发布面保持一致。

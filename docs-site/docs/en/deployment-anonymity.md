# Deployment & Anonymity

## Recommended deployment shape

For production, Tavily Hikari is typically deployed behind a gateway that terminates TLS and
injects trusted identity headers for admin traffic.

Recommended surfaces:

- public homepage and user console
- `/admin` for operators
- `/api/tavily/*` for downstream HTTP clients
- `/mcp` for proxied MCP traffic

## High-anonymity forwarding

Tavily Hikari can strip or rewrite sensitive request headers when proxying upstream traffic.

Operational guidance:

- only forward the traffic classes that must reach Tavily
- sanitize `X-Forwarded-*`, `Origin`, and `Referer` according to your anonymity policy
- keep audit logging enabled so dropped and forwarded headers remain observable

For the design background, see the repository document
[`docs/high-anonymity-proxy.md`](https://github.com/IvanLi-CN/tavily-hikari/blob/main/docs/high-anonymity-proxy.md).

## ForwardAuth example

The repository includes a Caddy-based example:

- [examples/forwardauth-caddy](https://github.com/IvanLi-CN/tavily-hikari/tree/main/examples/forwardauth-caddy)

Use that example when you need a concrete baseline for admin auth and reverse-proxy routing.

## Built-in admin caveats

If you use the built-in admin login:

- prefer hashed passwords over plaintext env vars
- keep TLS termination trustworthy so the session cookie can be marked `Secure`
- treat it as a self-hosted convenience mode, not the default zero-trust production setup

## Release surface

The main release artifact is a container image published to:

`ghcr.io/ivanli-cn/tavily-hikari:<tag>`

That image includes the compiled frontend bundle. The public docs-site and Storybook are published
separately through GitHub Pages.

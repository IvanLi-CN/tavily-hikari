# CLI / Env

## New options

- `--xray-binary` / `XRAY_BINARY`
  - default: `xray`
  - purpose: executable used to turn share-link nodes into local socks5 routes

- `--xray-runtime-dir` / `XRAY_RUNTIME_DIR`
  - default: `data/xray-runtime`
  - purpose: directory for generated Xray configs

## Notes

- If `xray` is missing, the service must keep running.
- Only share-link nodes are marked unavailable; direct/manual native proxy nodes still work.

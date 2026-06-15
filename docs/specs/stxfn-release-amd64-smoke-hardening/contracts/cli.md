# Release MCP billing smoke script contract

## Command

- Path: `.github/scripts/release-mcp-billing-smoke.sh`
- Shell: `bash`

## Inputs

The script is invoked from the release workflow and reads configuration from environment variables.

| Name                   | Required | Description                                                                             |
| ---------------------- | -------- | --------------------------------------------------------------------------------------- |
| `LOCAL_SMOKE_IMAGE`    | yes      | Docker image tag to run for the release smoke proxy                                     |
| `RUNNER_TEMP`          | no       | Base temp directory for logs/data; defaults to a script-created temp dir when absent    |
| `SMOKE_MOCK_PORT`      | no       | Force the host port for `mock_tavily`; when absent the script allocates a free port     |
| `SMOKE_PROXY_PORT`     | no       | Force the host port for the release proxy; when absent the script allocates a free port |
| `SMOKE_DATA_DIR`       | no       | Override the persistent data dir mounted into the smoke container                       |
| `SMOKE_CONTAINER_NAME` | no       | Override the Docker container name used for the smoke proxy                             |
| `SMOKE_MOCK_BIN`       | no       | Override the `mock_tavily` binary path; defaults to `./target/debug/mock_tavily`        |

## Behavior

- Start `mock_tavily`, wait for `/admin/state`, and pre-seed a test key.
- Start the release image container, wait for `/health`, then execute the existing MCP billing smoke flow.
- Delegate SQLite billing/log assertions to `.github/scripts/release_smoke_sqlite_check.py`.
- The SQLite helper must resolve `request_logs` against the current sidecar layout: prefer the sibling `*-observability.db` attachment and `observability.request_logs`, but fall back to `main.request_logs` for legacy single-DB or backfill-compatible trees.
- On any failure, dump mock log, port state, Docker state/logs, and data-dir context before exiting non-zero.
- Always clean up the mock process and smoke container on exit.

## Exit codes

- `0`: smoke completed successfully.
- Non-zero: startup, transport, business assertion, or diagnostic failure.

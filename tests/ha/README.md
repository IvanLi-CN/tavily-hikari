# HA Test Harness

The repository-supported deployment profile is temporarily single node. This Compose smoke
contract verifies that the node runs as `full_master` with full writes enabled and without a
configured backup node or sync source.

The Rust test suite retains the `active_standby` state machine and protocol tests as dormant
compatibility coverage. This Compose profile does not deploy or exercise a second node.

## Compose Layout

- `docker-compose.yml` runs one `node-a` service and the local `upstream-mock` service.
- `node-a` uses `HA_MODE=single`, `NODE_ID=single`, and no HA peer or pull-sync configuration.
- Runtime data is mounted from `${HA_RUNTIME_DIR}/node-a`.

## Local Usage

Prepare a runtime directory and start the smoke services:

```bash
export HA_RUNTIME_DIR="$PWD/.tmp/ha-runtime"
mkdir -p "$HA_RUNTIME_DIR/node-a"

docker compose \
  -f tests/ha/docker-compose.yml \
  up -d --build
```

Run the assertions from the host or another container that can reach `node-a`:

```bash
NODE_URL=http://127.0.0.1:8787 \
  python3 tests/ha/scripts/run_single_node_acceptance.py
```

The shared-testbox entrypoint runs the same contract with isolated Docker networking:

```bash
scripts/run-ha-testbox-suite.sh
```

## Acceptance Contract

The acceptance script checks both `/api/admin/ha/status` and `/api/ha/status` for:

- `mode=single`
- `role=full_master`
- `allowsFullWrites=true`
- `peerCount=0`
- `syncDisabledReason=null`
- `peerNodes=[]`

The testbox runner stores `single_node.json`, `single_node.log`, and `ha-suite-summary.json` in
the run artifact directory.

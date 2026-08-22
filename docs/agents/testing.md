# Backend Test Execution

Use the narrowest command that proves the changed behavior. Run a single Rust target and exact
test for a focused change; use `scripts/ci_backend_tests.py run-shard --id <shard>` when a change
crosses one manifest shard.

`run-shard` and `run-all` default to low resource limits: two Cargo jobs, one filtered process
worker, and two filtered test threads. Use `--diagnostic` for a fixed `1/1/1` execution when
investigating interference. Keep `benchmark` for measurement; it is not the development default.

Treat `run-all`, release builds, and Compose checks as heavy targets. Select an execution target
explicitly before starting them. If it is unavailable, report the missing validation and wait for
the selected target or CI; do not automatically move a heavy command to a different machine.

The backend runner enforces manifest coverage before CI fan-out. Do not bypass it by editing a
test command, skipping a shard, or changing a selector to hide an unmatched test. Test upstreams
must stay stubbed or sandboxed; production Tavily endpoints require explicit approval.

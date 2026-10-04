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

## GitHub Actions Performance Recovery A/B

`.github/workflows/performance-recovery.yml` is a manual-only A/B comparison gate on the GitHub-hosted
`ubuntu-24.04` runner. It creates a disposable SQLite/core-sidecar fixture from the candidate build,
archives the baseline and candidate source revisions locally, and runs the existing isolated
baseline/candidate Docker comparison with only the local/mock upstream. It uses only managed
GitHub-hosted compute and does not require SSH, private network hosts, or production database
snapshots.

It requires `confirm=yes` and a duration from 600 to 1800 seconds per variant. It uploads sanitized
logs and comparison output for 14 days. A passing run proves the recovery behavior against the
GitHub-hosted fixture; it is not a claim about a production snapshot or private-machine behavior.

From the GitHub UI, dispatch `Performance Recovery A/B` from the candidate ref. The equivalent CLI
form is:

```bash
gh workflow run performance-recovery.yml \
  --ref main \
  -f confirm=yes \
  -f baseline_ref=1d6d93cbf4de6e673d75811fadd21f45b9a40482 \
  -f candidate_ref=<candidate-sha> \
  -f duration_secs=600
```

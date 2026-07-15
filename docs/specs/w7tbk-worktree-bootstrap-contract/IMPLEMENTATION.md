# Implementation

## Current Status

- Implementation: completed
- Lifecycle: active
- Catalog note: linked-worktree bootstrap contract is now repo-local and CI-backed

## Current Coverage

- Root `package.json`
  - `prepare` now runs `scripts/install-hooks.sh`.
  - `hooks:install`, `worktree:setup`, and `test:worktree-bootstrap` provide explicit operator surfaces.
- `scripts/install-hooks.sh`
  - installs a shared `post-checkout` wrapper into the git common hooks directory
  - preserves an existing unmanaged `post-checkout` as `post-checkout.local`
  - refreshes `lefthook` commit hooks when the binary exists on `PATH`
- `scripts/worktree-bootstrap.sh`
  - detects primary vs linked worktree through git dir vs common dir
  - uses `.tmp/worktree-bootstrap.v1.done` as the worktree-local completion marker
  - copies missing root `.env` / `.env.*` files from the primary worktree without overwriting existing targets
  - restores missing root / `web` / `docs-site` Bun dependencies
  - runs `cargo fetch --locked`
  - keeps automatic checkout-triggered bootstrap best-effort and non-blocking
- `scripts/worktree-setup.sh`
  - reinstalls hooks
  - reruns linked-worktree bootstrap with `--manual --force --strict`
- `scripts/test-worktree-bootstrap.sh`
  - exercises a real temporary linked worktree
  - verifies first auto bootstrap, repeated checkout no-op, manual strict rerun, primary no-op, historical revision safe skip, env no-overwrite, missing-tools warnings, and preserved custom `post-checkout` chaining
- `.github/workflows/ci.yml`
  - ships an independent `Worktree Bootstrap Smoke` job that runs the smoke script without real dependency downloads

## Validation

- `bash scripts/test-worktree-bootstrap.sh`

## References

- `./SPEC.md`
- `./HISTORY.md`

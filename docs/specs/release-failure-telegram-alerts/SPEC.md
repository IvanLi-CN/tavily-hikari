# Release failure notification via Oidrune

## Context and Scope

The repository reports failed `Release` and releasable post-merge `CI Pipeline` runs through the
repo-local notifier. The notification handoff is moving from the shared Telegram workflow to
Oidrune's OIDC-authenticated reusable workflow.

### In scope

- `.github/workflows/notify-release-failure.yml`
- `.github/scripts/release_failure_notifier.py`
- `tests/test_release_failure_notifier.py`
- `tests/test_notify_release_workflow.py`
- This topic's Spec, Implementation, and History documents

### Out of scope

- Changes to Oidrune gateway configuration or control-plane resources
- Production Tavily traffic
- Automatic dispatch of the manual smoke path during local or CI validation
- Changes to ordinary pull-request CI or unrelated workflows

## Requirements

- `REQ-001`: The notifier workflow MUST call
  `IvanLi-CN/oidrune/.github/workflows/notify.yml` at the trusted full commit SHA for the current
  latest formal Oidrune release, and MUST NOT use a moving ref such as `main`.
- `REQ-002`: Calls MUST use Oidrune's default gateway by omitting both `gateway_url` and
  `oidc_audience` inputs.
- `REQ-003`: Each Oidrune caller job MUST grant `id-token: write` and MUST NOT forward the legacy
  `SHOUTRRR_URL` Telegram secret.
- `REQ-004`: The workflow MUST preserve the `Release` and `main` `CI Pipeline` `workflow_run`
  filters, release-intent suppression, one-time transient Docker failed-job rerun, post-rerun
  alert behavior, and the manual `workflow_dispatch` smoke path.
- `REQ-005`: The caller MUST provide a complete summary containing the project name, status,
  result, target SHA, run URL, and a smoke/failure title; the summary MUST include the resolved
  details produced by the repo-local notifier and MUST NOT depend on Oidrune to add metadata.
- `REQ-006`: Local workflow contract tests MUST cover the pinned reusable workflow target,
  permission and secret boundaries, summary fields, event filters, and preserved notification
  semantics.

## Verification

- `VER-001` covers: `REQ-001`, `REQ-002`, `REQ-003`, `REQ-004`, and `REQ-005`: run
  `actionlint .github/workflows/notify-release-failure.yml` and inspect the workflow contract
  assertions in `tests/test_notify_release_workflow.py`.
- `VER-002` covers: `REQ-004` and `REQ-005`: run
  `python3 -m unittest tests/test_release_failure_notifier.py` against stubbed GitHub API data;
  do not dispatch the real smoke notification.
- `VER-003` covers: `REQ-006`: run
  `python3 -m unittest tests/test_notify_release_workflow.py tests/test_release_failure_notifier.py`.
- `VER-004` covers: `REQ-006`: run the Spec contract check and Spec drift check against the
  current base and the frozen ADR relationship.

## Related ADRs

- None

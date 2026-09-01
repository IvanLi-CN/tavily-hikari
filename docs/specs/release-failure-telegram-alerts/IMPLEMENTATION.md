# Release failure notification via Oidrune implementation

## Status

- Status: implemented and ready for delivery verification.
- The notification handoff targets Oidrune `v0.1.14` at
  `e48822f99c6402a753ed86557ea029754cbab20b`.

## Implementation Coverage

- `notify-release-failure.yml` keeps the `Release` and `CI Pipeline` `workflow_run` filters,
  main-branch release-intent suppression, transient Docker failed-job rerun, and manual smoke
  dispatch path.
- Both reusable-workflow caller jobs grant `id-token: write`, omit gateway overrides so Oidrune's
  default gateway is selected, and no longer pass the legacy Telegram secret.
- `release_failure_notifier.py` emits a caller-owned summary with the project, failure status and
  result, title, resolved target SHA, run URL, ref, attempt, actor, event, and triage details.
- Workflow and Python contract tests cover the pinned target, permission boundary, summary fields,
  preserved filters, and first-attempt/post-rerun behavior without sending a real notification.

## Verification Coverage

- `actionlint` covers the reusable-workflow call shape and GitHub Actions expression syntax.
- Stubbed Python unit tests cover notification classification, summary generation, target SHA
  resolution, release-intent gating, and transient Docker self-healing.
- Spec contract and drift checks cover the canonical requirements and ADR relationship.

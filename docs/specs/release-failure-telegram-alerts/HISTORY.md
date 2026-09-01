# Release failure notification via Oidrune history

## Legacy Identity

- Legacy compatibility identity: `#jmdsq`.

## Lifecycle

- The original topic introduced repo-local triage for failed `Release` and releasable post-merge
  `CI Pipeline` runs, including target-SHA markers, release-intent suppression, the manual smoke
  path, and one-time transient Docker failed-job reruns.
- The notification consumer moved from
  `IvanLi-CN/github-workflows/.github/workflows/release-failure-telegram.yml` to
  `IvanLi-CN/oidrune/.github/workflows/notify.yml` pinned to the trusted `v0.1.14` commit.
- The caller now owns the complete notification summary and uses OIDC permissions with Oidrune's
  default gateway. The legacy Telegram secret forwarding contract is retired for this workflow.

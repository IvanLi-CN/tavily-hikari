# Monthly Quota Rebase Audit Results

## Testbox Run

- Remote workspace: `/srv/codex/workspaces/ivan/tavily-hikari__e179f1ce`
- Run directory: `/srv/codex/workspaces/ivan/tavily-hikari__e179f1ce/runs/20260318t143153-u4k9m`
- Live DB source: `101:/srv/app/data/tavily_proxy.db` copied read-only into the run directory
- Evidence files:
  - `evidence/audit-before.json`
  - `evidence/rebase-report.json`
  - `evidence/audit-after.json`
  - `evidence/probe-before-actual.json`
  - `evidence/probe-after-actual.json`
  - `evidence/probe-log-delta.json`
  - `evidence/probe-before-unique.json`
  - `evidence/probe-unique-after.json`

## Rebase Verification

- Baseline audit on the copied live DB reproduced the production mismatch shape:
  - `mismatched_subjects=35`
  - `hour_only=0`
  - `day_only=0`
  - `month_only=35`
- Sample mismatches matched the production investigation:
  - `token:ZjvC` monthly quota residue remained on the raw token subject
  - `account:jTl8MDlzuejK` monthly quota was higher than charged ledger credits
- Running `monthly_quota_rebase` on the copied DB produced:
  - `rebased_subjects=290`
  - `rebased_tokens=5`
  - `rebased_accounts=285`
  - `charged_rows=2753`
  - `charged_credits=3604`
- Post-rebase audit returned `mismatched_subjects=0`.
- Charged ledger rows and charged credit totals stayed unchanged before and after the rebase.

## Live Probe Verification

- `ZjvC` is bound to `account:jTl8MDlzuejK`.
- On the copied DB after rebase, the raw token monthly row stayed at `0`, while the bound account row carried the live monthly usage.
- A non-`DEV_OPEN_ADMIN` isolated instance was started on the copied DB to avoid `token:dev` fallback during probe traffic.
- Real token-authenticated `/api/tavily/search` probes produced charged rows:
  - `509023` => `api_key_id=CBoX`, `billing_subject=account:jTl8MDlzuejK`, `business_credits=1`
  - `509024` => `api_key_id=CBoX`, `billing_subject=account:jTl8MDlzuejK`, `business_credits=1`
- For the validated browser probe:
  - account hourly usage `2 -> 3`
  - account daily usage `4 -> 5`
  - account monthly usage `7 -> 8`
  - raw `auth_token_quota` for `ZjvC` stayed `0`

## Upstream Usage Finding

- Tavily direct `/search` responses with `include_usage=true` returned `usage.credits=1`.
- The same upstream key's direct `/usage` response stayed at `usage=92`, `search_usage=7`, `plan_usage=92` across:
  - direct Tavily search calls
  - proxy-mediated real token calls
  - repeated manual `sync-usage` operations
- This means the copied-environment probe confirmed local token/account/month/hour/day billing consistency against upstream per-response credits, but Tavily `/usage` did not expose an immediate delta that could be matched 1:1 in the same validation window.

## Follow-up Fix

- `DEV_OPEN_ADMIN` no longer overrides an explicit bearer token on `/api/tavily/*` and `/mcp`.
- When a real token is present, billing and quota checks now stay on the real token/account subject even if admin shortcut mode is enabled.
- Regression coverage was added for both HTTP and MCP paths under `DEV_OPEN_ADMIN=true`.

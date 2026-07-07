# History

## 2026-07-07

- Created the `tv4hc` topic spec for the Tavily Hikari CLI + Agent Skills release.
- Locked distribution to GitHub Release assets instead of PyPI/npm/Homebrew.
- Locked Agent Skills distribution to the repository root `skills/` package.
- Locked UI entrypoint to the existing client guide as a `CLI + Skills` tab.
- Confirmed tests must use fake or local Hikari paths and must not hit Tavily production upstream.
- Review loop tightened wrapper token validation to match backend token lengths and fixed installer
  custom `--config-dir` persistence through a generated launcher.
- Review proof fixed the default installer path so omitting `--with-skills` exits successfully
  instead of inheriting a failed Bash test status under `set -e`.

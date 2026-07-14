# Implementation

## Current Implementation

- `scripts/tvly-hikari` is a Bash wrapper around the official `tvly` executable.
- The wrapper validates Hikari token shape against the backend contract: `th-<4 chars>-<12 or 24 chars>`.
- `scripts/install-tvly-hikari.sh` installs the wrapper from GitHub Release assets or a local test
  source, configures Hikari origin/token, and optionally installs Agent Skills.
- The installer writes a `tvly-hikari` launcher plus a hidden real wrapper so custom `--config-dir`
  remains the default for later invocations while `TAVILY_HIKARI_CONFIG_DIR` and
  `TAVILY_HIKARI_CONFIG_FILE` can still override it.
- `.github/workflows/release.yml` validates the scripts and uploads both CLI assets during stable
  and rc releases.
- `skills/` contains the Hikari-specific Agent Skills package.
- All seven Hikari Skills declare valid YAML `name` and `description` frontmatter so the
  `npx skills` package and native client loaders can discover them.
- Public installation guidance and the installer's `--with-skills` path use the user-level command
  `npx skills add https://github.com/IvanLi-CN/tavily-hikari --global` without project scope or
  an explicit agent selection.
- User-facing guide data now includes `hikariCli`, surfaced as `CLI + Skills` in desktop tabs and
  mobile dropdowns.
- Guide code blocks include copy controls using the existing clipboard helper.
- Storybook includes `Setup Guide CLI + Skills` and its mobile variant for the current state.

## Verification Notes

- CLI tests use a fake `tvly` executable to capture injected environment variables.
- Installer tests use a local wrapper source and fake `npx` so no external skills install is
  performed.
- Installer tests assert that a custom `--config-dir` remains readable from the installed launcher.
- UI guide tests assert generated installer and skills commands.
- The optional `RUN_NPX_SKILLS_INTEGRATION=1` test installs the local package into isolated user
  homes for Codex, OpenCode, and Claude Code, then verifies all seven skills through
  `npx skills list --global`.
- Visual evidence is captured from mock-only Storybook canvas. Chrome Control browser discovery was
  unavailable in this environment, so the approved escalation path used `agent-browser`; the mobile
  proof uses the existing 390 px mobile Storybook state and confirms the Skills code sample is
  horizontally scrollable before capturing its `--global` tail.

## References

- Spec: `docs/specs/tv4hc-hikari-cli-agent-skills-release/SPEC.md`
- Skills package: `skills/README.md`

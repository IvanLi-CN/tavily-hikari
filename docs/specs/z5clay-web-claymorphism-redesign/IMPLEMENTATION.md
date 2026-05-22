# Implementation

## Current Plan

- Add project design context in `PRODUCT.md` and `DESIGN.md`.
- Add a final clay override stylesheet imported after existing light/dark/page styles.
- Update Tailwind tokens and shared UI wrappers for clay shadows, radii, input depth, and button feedback.
- Update Storybook defaults and stories as needed for stable visual review.

## Validation

- `cd web && bun run build` passes.
- `cd web && bun test` passes.
- `cd web && bun run build-storybook` passes.
- `git diff --check` passes.
- Storybook canvas evidence captured from `design-system-claymorphism--overview`, `admin-pages--dashboard`, `public-publichomeherocard--logged-out-no-token`, and `public-pages-registrationpaused--default`.
- Codex review finding for dark-mode clay token inheritance was fixed; follow-up review reported no actionable correctness, build, or regression findings.

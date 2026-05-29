# 用户控制台公告实现状态（#aa7yu）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: implemented
- Lifecycle: active
- Catalog note: 管理员控制台公告模块和用户控制台公告展示。

## Coverage / rollout summary

- Backend: SQLite announcement persistence, admin CRUD lifecycle APIs, archive edit/republish ID handling, ticker-only empty body acceptance, modal body enforcement, and user active/history APIs are implemented.
- Frontend: admin announcement management split into list and create/edit views, shared module surface without duplicate page headers, title-row create action without an announcement-module refresh toolbar, content-fit single-line desktop table actions, Milkdown-powered Markdown authoring with Markdown/split/WYSIWYG modes, ticker-only optional body validation/copy, save-and-publish ergonomics, list-page preview that reuses the user-console modal/ticker display, Markdown body rendering, user console modal/ticker title/detail/history UI, bodyful ticker detail acknowledgement/close dismissal, bundled offline ticker icon data, local close memory, and i18n copy are implemented.
- Storybook: admin default/empty/create/mobile-create/mobile/list-preview coverage and user console active/history announcement states are implemented; the user console announcement story asserts ticker body text stays out of the ticker strip, a bodyful ticker opens details from the title/action and dismisses after detail acknowledgement or dialog close, a bodyless ticker can be directly dismissed, Storybook uses a lightweight editor stub for static coverage while the app loads Milkdown on demand, and the create story asserts all editor modes remain available without an editor-side user preview.
- Visual evidence: stored in `./assets/` and referenced from `./SPEC.md`.

## Remaining Gaps

- No known implementation gaps.

## Related Changes

- `src/models.rs`
- `src/store/key_store_announcements.rs`
- `src/server/handlers/admin_resources/announcements.rs`
- `src/tavily_proxy/proxy_announcements.rs`
- `web/src/admin/AnnouncementsModule.tsx`
- `web/src/components/MarkdownEditor.tsx`
- `web/src/lib/icon-data.ts`
- `web/src/user-console/Announcements.tsx`
- `web/scripts/generate-icon-data.mjs`
- `web/src/UserConsole.stories.tsx`
- `web/src/admin/AnnouncementsModule.stories.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`

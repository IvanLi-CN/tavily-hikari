# 用户控制台公告实现状态（#aa7yu）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: implemented
- Lifecycle: active
- Catalog note: 管理员控制台公告模块和用户控制台公告展示。

## Coverage / rollout summary

- Backend: SQLite announcement persistence, admin CRUD lifecycle APIs, archive edit/republish ID handling, and user active/history APIs are implemented.
- Frontend: admin announcement management split into list and create/edit views, Milkdown-powered Markdown authoring with Markdown/split/WYSIWYG modes, save-and-publish ergonomics, Milkdown-rendered user-side preview, Markdown body rendering, user console modal/ticker/history UI, local close memory, and i18n copy are implemented.
- Storybook: admin default/empty/create/mobile-create/mobile coverage and user console active/history announcement states are implemented; Storybook uses a lightweight editor stub for static coverage while the app loads Milkdown on demand, and the create story asserts all editor modes remain available.
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
- `web/src/user-console/Announcements.tsx`
- `web/src/UserConsole.stories.tsx`
- `web/src/admin/AnnouncementsModule.stories.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`

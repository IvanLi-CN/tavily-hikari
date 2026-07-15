# 用户控制台公告实现状态（#aa7yu）

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## Current Status

- Implementation: implemented
- Lifecycle: active
- Catalog note: 管理员控制台公告模块和用户控制台公告展示。

## Coverage / rollout summary

- Backend: SQLite 公告持久化已收敛到 `content-only`；管理端公告 CRUD 生命周期 API、已发布/已归档编辑换新 ID、旧 `title/body` 数据回填迁移、ATX/Setext 首标题识别、modal 标题/正文校验，以及用户 active/history API 均已实现。
- Frontend: 管理端公告管理已收敛到单一“内容”字段并移除独立标题输入；Milkdown Markdown/split/WYSIWYG 编辑、内容驱动的列表预览、列表页复用用户端 modal/ticker 预览、标题链接渲染、无标题横幅直出内容、标题+正文横幅独立详情按钮、标题-only 横幅直接关闭、历史列表无伪标题与正文去重、浏览器本地关闭记忆和 i18n 文案均已实现。
- Storybook: admin default/empty/create/mobile-create/mobile/list-preview 覆盖与 user console active/history/titled-ticker/untitled-ticker 覆盖已实现；故事断言覆盖标题去重、独立详情按钮、标题-only 横幅直接关闭、无标题横幅内容直出与链接可点击。
- Visual evidence: stored in `./assets/` and referenced from `./SPEC.md`.

## Remaining Gaps

- No known implementation gaps.

## Related Changes

- `src/models.rs`
- `src/store/key_store_announcements.rs`
- `src/server/handlers/admin_resources/announcements.rs`
- `src/server/dto.rs`
- `src/tavily_proxy/proxy_announcements.rs`
- `web/src/admin/AnnouncementsModule.tsx`
- `web/src/api/announcements.ts`
- `web/src/components/MarkdownEditor.tsx`
- `web/src/lib/announcementContent.ts`
- `web/src/user-console/Announcements.tsx`
- `web/src/UserConsole.stories.tsx`
- `web/src/admin/AnnouncementsModule.stories.tsx`

## References

- `./SPEC.md`
- `./HISTORY.md`

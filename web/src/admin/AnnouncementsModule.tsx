import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

import {
  archiveAnnouncement,
  createAnnouncement,
  fetchAnnouncements,
  publishAnnouncement,
  updateAnnouncement,
  type Announcement,
  type AnnouncementDisplayKind,
  type AnnouncementMutationPayload,
  type AnnouncementStatus,
} from '../api'
import AdminModuleSurface from './AdminModuleSurface'
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import MarkdownContent from '../components/MarkdownContent'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import SegmentedTabs, { type SegmentedTabsOption } from '../components/ui/SegmentedTabs'
import { Button } from '../components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
import UserConsoleAnnouncements from '../user-console/Announcements'
import { EN as USER_CONSOLE_EN, ZH as USER_CONSOLE_ZH } from '../user-console/text'
import { Icon } from '../lib/icons'
import { parseAnnouncementContent } from '../lib/announcementContent'
import type { Language } from '../i18n'
import { announcementCreatePath, announcementEditPath, announcementListPath } from './routes'

export type AnnouncementRouteMode =
  | { kind: 'list' }
  | { kind: 'create' }
  | { kind: 'edit'; id: string }

interface AnnouncementsModuleProps {
  language: Language
  refreshToken?: number
  initialMode?: 'list' | 'create'
  routeMode?: AnnouncementRouteMode
  onNavigate?: (path: string) => void
  headerActionSlotId?: string
  showListCreateAction?: boolean
}

interface AnnouncementDraft {
  content: string
  displayKind: AnnouncementDisplayKind
}

type AnnouncementEditorMode =
  | { kind: 'create' }
  | { kind: 'edit'; id: string; status: AnnouncementStatus }

type AnnouncementSubmitAction = 'draft' | 'publish'
type AnnouncementBodyMode = 'markdown' | 'split' | 'wysiwyg'

type AnnouncementCopy = ReturnType<typeof copy>

const EMPTY_DRAFT: AnnouncementDraft = {
  content: '',
  displayKind: 'modal',
}

const MarkdownEditor = lazy(() => import('../components/MarkdownEditor'))

function copy(language: Language) {
  return language === 'zh'
    ? {
        refresh: '刷新',
        refreshing: '刷新中…',
        loading: '正在加载公告…',
        error: '公告加载失败。',
        empty: '还没有公告。',
        newAnnouncement: '新建公告',
        listTitle: '公告列表',
        listDescription: '查看公告状态，并对已有公告执行发布、归档或编辑。',
        backToList: '返回列表',
        formTitleNew: '新建公告',
        formTitleEdit: '编辑公告',
        formDescriptionNew: '使用 Markdown 编写公告内容。首个 Header 会作为标题，选择展示方式后可保存草稿或直接发布。',
        formDescriptionEdit: '保存修改会更新草稿；发布后会让用户控制台显示最新内容。',
        formDescriptionPublished: '发布新版本会归档旧公告，并生成新公告 ID 重新提醒用户。',
        formDescriptionArchived: '编辑归档公告会保留历史记录，并生成新的草稿或发布项。',
        publishImpact: '发布后会立即对用户可见；归档后不再作为当前公告展示。',
        bodyLabel: '内容',
        bodyPlaceholder: '使用 Markdown 编写公告内容。首个 Header 会作为标题。',
        bodyOptionalPlaceholder: '可选：以 Markdown 补充点击后展示的公告详情；如果没有 Header，横幅会直接显示内容。',
        bodyA11yHint: '内容支持 Markdown。首个 Header 会作为标题，并在展示时从正文中去重。',
        bodyModeLabel: '内容编辑模式',
        bodyModeMarkdown: 'Markdown',
        bodyModeSplit: '左右对比',
        bodyModeWysiwyg: '所见即所得',
        bodyModeRenderLabel: 'Milkdown 只读渲染',
        bodyModeRenderEmpty: '内容为空。',
        displayLabel: '展示方式',
        modal: '弹窗',
        ticker: '横幅',
        saveDraft: '保存草稿',
        saveChanges: '保存修改',
        saveAndPublish: '保存并发布',
        saveAndPublishVersion: '保存并发布新版本',
        saving: '保存中…',
        publishing: '发布中…',
        cancel: '取消',
        status: {
          draft: '草稿',
          published: '发布中',
          archived: '已归档',
        },
        table: {
          announcement: '公告',
          display: '展示',
          status: '状态',
          updated: '更新时间',
          actions: '操作',
        },
        actions: {
          preview: '预览',
          edit: '编辑',
          publish: '发布',
          archive: '归档',
        },
        actionBusy: '处理中…',
        validationContent: '内容不能为空。',
        validationModalTitle: '弹窗公告内容必须以 Markdown 标题开头。',
        validationModalBody: '弹窗公告标题后必须提供正文内容。',
        notFound: '公告不存在或已被删除。',
        saved: '公告已保存。',
        savedAndPublished: '公告已保存并发布。',
        published: '公告已发布。',
        archived: '公告已归档。',
      }
    : {
        refresh: 'Refresh',
        refreshing: 'Refreshing…',
        loading: 'Loading announcements…',
        error: 'Failed to load announcements.',
        empty: 'No announcements yet.',
        newAnnouncement: 'New announcement',
        listTitle: 'Announcement list',
        listDescription: 'Review announcement status, then publish, archive, or edit existing items.',
        backToList: 'Back to list',
        formTitleNew: 'New announcement',
        formTitleEdit: 'Edit announcement',
        formDescriptionNew: 'Write announcement content in Markdown. The first header becomes the title, then save a draft or publish directly.',
        formDescriptionEdit: 'Saving updates the draft; publishing makes the latest content visible in the user console.',
        formDescriptionPublished: 'Publishing a new version archives the old item and creates a new ID so users are reminded again.',
        formDescriptionArchived: 'Editing an archived announcement keeps the history entry and creates a new draft or published item.',
        publishImpact: 'Publishing makes it visible immediately; archiving removes it from current announcements.',
        bodyLabel: 'Content',
        bodyPlaceholder: 'Write the announcement in Markdown. The first header becomes the title.',
        bodyOptionalPlaceholder: 'Optional: add Markdown details shown after opening the ticker. Without a header, the ticker shows the content directly.',
        bodyA11yHint: 'Content supports Markdown. The first header becomes the title and is removed from the rendered body.',
        bodyModeLabel: 'Content editor mode',
        bodyModeMarkdown: 'Markdown',
        bodyModeSplit: 'Split',
        bodyModeWysiwyg: 'WYSIWYG',
        bodyModeRenderLabel: 'Milkdown read-only render',
        bodyModeRenderEmpty: 'The content is empty.',
        displayLabel: 'Display',
        modal: 'Modal',
        ticker: 'Ticker',
        saveDraft: 'Save draft',
        saveChanges: 'Save changes',
        saveAndPublish: 'Save and publish',
        saveAndPublishVersion: 'Save and publish version',
        saving: 'Saving…',
        publishing: 'Publishing…',
        cancel: 'Cancel',
        status: {
          draft: 'Draft',
          published: 'Published',
          archived: 'Archived',
        },
        table: {
          announcement: 'Announcement',
          display: 'Display',
          status: 'Status',
          updated: 'Updated',
          actions: 'Actions',
        },
        actions: {
          preview: 'Preview',
          edit: 'Edit',
          publish: 'Publish',
          archive: 'Archive',
        },
        actionBusy: 'Working…',
        validationContent: 'Content is required.',
        validationModalTitle: 'Modal announcements must start with a Markdown title.',
        validationModalBody: 'Modal announcements need body content after the title.',
        notFound: 'Announcement was not found or has been deleted.',
        saved: 'Announcement saved.',
        savedAndPublished: 'Announcement saved and published.',
        published: 'Announcement published.',
        archived: 'Announcement archived.',
      }
}

function statusTone(status: AnnouncementStatus): StatusTone {
  switch (status) {
    case 'published':
      return 'success'
    case 'draft':
      return 'info'
    case 'archived':
      return 'neutral'
    default:
      return 'neutral'
  }
}

function displayLabel(displayKind: AnnouncementDisplayKind, strings: AnnouncementCopy): string {
  return displayKind === 'ticker' ? strings.ticker : strings.modal
}

function formatTimestamp(value: number | null, language: Language): string {
  if (!value) return '-'
  try {
    return new Intl.DateTimeFormat(language === 'zh' ? 'zh-CN' : 'en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value * 1000))
  } catch {
    return '-'
  }
}

function toDraft(item: Announcement): AnnouncementDraft {
  return {
    content: item.content,
    displayKind: item.displayKind,
  }
}

function toPayload(draft: AnnouncementDraft): AnnouncementMutationPayload {
  return {
    content: draft.content.trim(),
    displayKind: draft.displayKind,
  }
}

export function validateAnnouncementContentInput(
  content: string,
  displayKind: AnnouncementDisplayKind,
): 'content' | 'modal_title' | 'modal_body' | null {
  const normalized = content.trim()
  if (!normalized) return 'content'
  if (displayKind !== 'modal') return null
  const parsed = parseAnnouncementContent(normalized)
  if (!parsed.hasTitle) return 'modal_title'
  if (!parsed.hasBody) return 'modal_body'
  return null
}

function editorDescription(mode: AnnouncementEditorMode, strings: AnnouncementCopy): string {
  if (mode.kind === 'create') return strings.formDescriptionNew
  if (mode.status === 'published') return strings.formDescriptionPublished
  if (mode.status === 'archived') return strings.formDescriptionArchived
  return strings.formDescriptionEdit
}

export function estimateAnnouncementContentRows(content: string): number {
  const visualLines = content.split('\n').reduce((count, line) => (
    count + Math.max(1, Math.ceil(line.length / 72))
  ), 0)
  return Math.min(18, Math.max(6, visualLines + 2))
}

function AnnouncementBodyEditor({
  mode,
  draft,
  rows,
  strings,
  saving,
  onChangeDraft,
}: {
  mode: AnnouncementBodyMode
  draft: AnnouncementDraft
  rows: number
  strings: AnnouncementCopy
  saving: boolean
  onChangeDraft: (draft: AnnouncementDraft) => void
}): JSX.Element {
  const textarea = (
    <TextareaFallback
      id="announcement-body-editor"
      name="announcement-body"
      ariaLabelledBy="announcement-body-editor-label"
      ariaDescribedBy="announcement-body-editor-hint"
      value={draft.content}
      placeholder={strings.bodyPlaceholder}
      rows={rows}
      disabled={saving}
      onChange={(content) => onChangeDraft({ ...draft, content })}
    />
  )

  if (mode === 'markdown') {
    return textarea
  }

  if (mode === 'split') {
    return (
      <div className="announcements-body-split">
        {textarea}
        <MilkdownPreviewContent
          value={draft.content || strings.bodyModeRenderEmpty}
          label={strings.bodyModeRenderLabel}
          className="announcements-body-milkdown-preview"
        />
      </div>
    )
  }

  return (
    <LazyMarkdownEditor
      id="announcement-body-editor"
      name="announcement-body"
      ariaLabelledBy="announcement-body-editor-label"
      ariaDescribedBy="announcement-body-editor-hint"
      value={draft.content}
      placeholder={strings.bodyPlaceholder}
      disabled={saving}
      onChange={(content) => onChangeDraft({ ...draft, content })}
      fallback={textarea}
    />
  )
}

function MilkdownPreviewContent({
  value,
  label,
  className,
}: {
  value: string
  label: string
  className?: string
}): JSX.Element {
  return (
    <LazyMarkdownEditor
      value={value}
      placeholder=""
      ariaLabel={label}
      readOnly
      className={[
        'announcements-milkdown-preview',
        className ?? '',
      ].filter(Boolean).join(' ')}
      onChange={() => {}}
      fallback={(
        <textarea
          className="textarea announcements-body-fallback announcements-body-fallback--readonly"
          value={value}
          aria-label={label}
          rows={5}
          readOnly
        />
      )}
    />
  )
}

function AnnouncementEditorPanel({
  mode,
  draft,
  submittingAction,
  strings,
  onBack,
  onChangeDraft,
  onSubmit,
}: {
  mode: AnnouncementEditorMode
  draft: AnnouncementDraft
  submittingAction: AnnouncementSubmitAction | null
  strings: AnnouncementCopy
  onBack: () => void
  onChangeDraft: (draft: AnnouncementDraft) => void
  onSubmit: (action: AnnouncementSubmitAction) => void
}): JSX.Element {
  const saving = submittingAction != null
  const isPublishedEdit = mode.kind === 'edit' && mode.status === 'published'
  const [bodyMode, setBodyMode] = useState<AnnouncementBodyMode>('split')
  const bodyRows = estimateAnnouncementContentRows(draft.content)
  const bodyModeOptions: ReadonlyArray<SegmentedTabsOption<AnnouncementBodyMode>> = [
    { value: 'markdown', label: strings.bodyModeMarkdown },
    { value: 'split', label: strings.bodyModeSplit },
    { value: 'wysiwyg', label: strings.bodyModeWysiwyg },
  ]

  return (
    <form
      className="announcements-editor"
      aria-label={mode.kind === 'edit' ? strings.formTitleEdit : strings.formTitleNew}
      onSubmit={(event) => {
        event.preventDefault()
        onSubmit('draft')
      }}
    >
      <div className="announcements-editor-header">
        <div>
          <h3>{mode.kind === 'edit' ? strings.formTitleEdit : strings.formTitleNew}</h3>
          <p>{editorDescription(mode, strings)}</p>
        </div>
        <div className="announcements-editor-actions">
          <Button type="button" variant="outline" size="sm" onClick={onBack} disabled={saving}>
            <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
            <span>{strings.backToList}</span>
          </Button>
          <Button type="submit" variant="secondary" size="sm" disabled={saving}>
            {submittingAction === 'draft'
              ? strings.saving
              : mode.kind === 'edit' ? strings.saveChanges : strings.saveDraft}
          </Button>
          <Button
            type="button"
            size="sm"
            className="announcements-publish-action"
            title={strings.publishImpact}
            onClick={() => onSubmit('publish')}
            disabled={saving}
          >
            {submittingAction === 'publish'
              ? strings.publishing
              : isPublishedEdit ? strings.saveAndPublishVersion : strings.saveAndPublish}
          </Button>
        </div>
      </div>
      <label className="announcements-field">
        <span>{strings.displayLabel}</span>
        <Select
          name="announcement-display-kind"
          value={draft.displayKind}
          onValueChange={(value) => {
            onChangeDraft({
              ...draft,
              displayKind: value === 'ticker' ? 'ticker' : 'modal',
            })
          }}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="modal">{strings.modal}</SelectItem>
            <SelectItem value="ticker">{strings.ticker}</SelectItem>
          </SelectContent>
        </Select>
      </label>
      <div className="announcements-field">
        <div className="announcements-body-heading">
          <span id="announcement-body-editor-label">{strings.bodyLabel}</span>
          <SegmentedTabs<AnnouncementBodyMode>
            value={bodyMode}
            onChange={setBodyMode}
            options={bodyModeOptions}
            ariaLabel={strings.bodyModeLabel}
            className="announcements-body-mode-tabs"
            disabled={saving}
          />
        </div>
        <span id="announcement-body-editor-hint" className="sr-only">{strings.bodyA11yHint}</span>
        <AnnouncementBodyEditor
          mode={bodyMode}
          draft={draft}
          rows={bodyRows}
          strings={{
            ...strings,
            bodyPlaceholder: draft.displayKind === 'modal' ? strings.bodyPlaceholder : strings.bodyOptionalPlaceholder,
          }}
          saving={saving}
          onChangeDraft={onChangeDraft}
        />
      </div>
    </form>
  )
}

function AnnouncementListPrimaryCopy({
  item,
}: {
  item: Announcement
}): JSX.Element {
  const parsed = parseAnnouncementContent(item.content)
  if (!parsed.titleMarkdown) {
    return <span className="announcements-summary-text">{parsed.summary}</span>
  }

  return (
    <>
      <MarkdownContent content={parsed.titleMarkdown} inline className="announcements-title-markdown" />
      {parsed.bodyMarkdown ? <MarkdownContent content={parsed.bodyMarkdown} compact /> : null}
    </>
  )
}

function TextareaFallback({
  id,
  name,
  value,
  placeholder,
  ariaLabelledBy,
  ariaDescribedBy,
  rows,
  disabled,
  onChange,
}: {
  id: string
  name: string
  value: string
  placeholder: string
  ariaLabelledBy: string
  ariaDescribedBy: string
  rows: number
  disabled: boolean
  onChange: (value: string) => void
}): JSX.Element {
  return (
    <textarea
      id={id}
      name={name}
      className="textarea announcements-body-fallback"
      value={value}
      aria-labelledby={ariaLabelledBy}
      aria-describedby={ariaDescribedBy}
      placeholder={placeholder}
      rows={rows}
      maxLength={4200}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    />
  )
}

interface LazyMarkdownEditorProps {
  id?: string
  name?: string
  value: string
  placeholder: string
  ariaLabel?: string
  ariaLabelledBy?: string
  ariaDescribedBy?: string
  disabled?: boolean
  readOnly?: boolean
  className?: string
  onChange: (value: string) => void
  fallback: JSX.Element
}

function LazyMarkdownEditor({
  fallback,
  ...editorProps
}: LazyMarkdownEditorProps): JSX.Element {
  return (
    <Suspense fallback={fallback}>
      <MarkdownEditor {...editorProps} />
    </Suspense>
  )
}

function AnnouncementsListPanel({
  items,
  loading,
  error,
  busyId,
  strings,
  language,
  showCreateAction,
  onCreate,
  onEdit,
  onPreview,
  onAct,
}: {
  items: Announcement[]
  loading: boolean
  error: string | null
  busyId: string | null
  strings: AnnouncementCopy
  language: Language
  showCreateAction: boolean
  onCreate: () => void
  onEdit: (item: Announcement) => void
  onPreview: (item: Announcement) => void
  onAct: (id: string, action: 'publish' | 'archive') => void
}): JSX.Element {
  return (
    <div className="announcements-list">
      <div className="announcements-list-header">
        <div>
          <h3>{strings.listTitle}</h3>
          <p>{strings.listDescription}</p>
        </div>
        {showCreateAction ? (
          <Button type="button" size="sm" onClick={onCreate}>
            <Icon icon="mdi:plus" width={16} height={16} aria-hidden="true" />
            <span>{strings.newAnnouncement}</span>
          </Button>
        ) : null}
      </div>
      <AdminLoadingRegion
        loadState={loading ? 'initial_loading' : error ? 'error' : 'ready'}
        loadingLabel={strings.loading}
        errorLabel={error ?? strings.error}
        minHeight={260}
      >
        {items.length === 0 ? (
          <div className="empty-state alert announcements-empty-state">
            <span>{strings.empty}</span>
            {showCreateAction ? (
              <Button type="button" size="sm" onClick={onCreate}>
                <Icon icon="mdi:plus" width={16} height={16} aria-hidden="true" />
                <span>{strings.newAnnouncement}</span>
              </Button>
            ) : null}
          </div>
        ) : (
          <>
            <div className="table-wrapper announcements-table-wrapper admin-responsive-up">
              <table className="jobs-table announcements-table">
                <colgroup>
                  <col className="announcements-col-title" />
                  <col className="announcements-col-display" />
                  <col className="announcements-col-status" />
                  <col className="announcements-col-updated" />
                  <col className="announcements-col-actions" />
                </colgroup>
                <thead>
                  <tr>
                    <th>{strings.table.announcement}</th>
                    <th>{strings.table.display}</th>
                    <th>{strings.table.status}</th>
                    <th>{strings.table.updated}</th>
                    <th>{strings.table.actions}</th>
                  </tr>
                </thead>
                <tbody>
                  {items.map((item) => (
                    <tr key={item.id}>
                      <td>
                        <div className="announcements-title-cell">
                          <AnnouncementListPrimaryCopy item={item} />
                        </div>
                      </td>
                      <td>{displayLabel(item.displayKind, strings)}</td>
                      <td>
                        <StatusBadge tone={statusTone(item.status)}>
                          {strings.status[item.status]}
                        </StatusBadge>
                      </td>
                      <td>{formatTimestamp(item.updatedAt, language)}</td>
                      <td>
                        <div className="table-actions announcements-actions">
                          <Button type="button" variant="outline" size="xs" onClick={() => onPreview(item)}>
                            {strings.actions.preview}
                          </Button>
                          <Button type="button" variant="outline" size="xs" onClick={() => onEdit(item)}>
                            {strings.actions.edit}
                          </Button>
                          {item.status !== 'published' ? (
                            <Button
                              type="button"
                              size="xs"
                              onClick={() => onAct(item.id, 'publish')}
                              disabled={busyId === item.id}
                            >
                              {busyId === item.id ? strings.actionBusy : strings.actions.publish}
                            </Button>
                          ) : null}
                          {item.status !== 'archived' ? (
                            <Button
                              type="button"
                              variant="outline"
                              size="xs"
                              onClick={() => onAct(item.id, 'archive')}
                              disabled={busyId === item.id}
                            >
                              {busyId === item.id ? strings.actionBusy : strings.actions.archive}
                            </Button>
                          ) : null}
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="admin-mobile-list admin-responsive-down">
              {items.map((item) => {
                const parsed = parseAnnouncementContent(item.content)
                return (
                  <article key={item.id} className="admin-mobile-card announcements-mobile-card">
                    <header className="announcements-mobile-header">
                      {parsed.titleMarkdown ? (
                        <MarkdownContent
                          content={parsed.titleMarkdown}
                          inline
                          className="announcements-title-markdown"
                        />
                      ) : (
                        <strong className="announcements-summary-text">{parsed.summary}</strong>
                      )}
                      <StatusBadge tone={statusTone(item.status)}>
                        {strings.status[item.status]}
                      </StatusBadge>
                    </header>
                    {parsed.titleMarkdown && parsed.bodyMarkdown ? (
                      <MarkdownContent
                        content={parsed.bodyMarkdown}
                        className="announcements-mobile-body"
                      />
                    ) : null}
                    <div className="admin-mobile-kv">
                      <span>{strings.table.display}</span>
                      <strong>{displayLabel(item.displayKind, strings)}</strong>
                    </div>
                    <div className="admin-mobile-kv">
                      <span>{strings.table.updated}</span>
                      <strong>{formatTimestamp(item.updatedAt, language)}</strong>
                    </div>
                    <div className="table-actions announcements-mobile-actions">
                      <Button type="button" variant="outline" size="xs" onClick={() => onPreview(item)}>
                        {strings.actions.preview}
                      </Button>
                      <Button type="button" variant="outline" size="xs" onClick={() => onEdit(item)}>
                        {strings.actions.edit}
                      </Button>
                      {item.status !== 'published' ? (
                        <Button
                          type="button"
                          size="xs"
                          onClick={() => onAct(item.id, 'publish')}
                          disabled={busyId === item.id}
                        >
                          {busyId === item.id ? strings.actionBusy : strings.actions.publish}
                        </Button>
                      ) : null}
                      {item.status !== 'archived' ? (
                        <Button
                          type="button"
                          variant="outline"
                          size="xs"
                          onClick={() => onAct(item.id, 'archive')}
                          disabled={busyId === item.id}
                        >
                          {busyId === item.id ? strings.actionBusy : strings.actions.archive}
                        </Button>
                      ) : null}
                    </div>
                  </article>
                )
              })}
            </div>
          </>
        )}
      </AdminLoadingRegion>
    </div>
  )
}
function AnnouncementUserPreview({
  item,
  language,
  onClose,
}: {
  item: Announcement | null
  language: Language
  onClose: () => void
}): JSX.Element | null {
  if (!item) return null

  return (
    <div className="announcements-user-preview">
      <UserConsoleAnnouncements
        language={language}
        text={language === 'zh' ? USER_CONSOLE_ZH : USER_CONSOLE_EN}
        activeAnnouncements={[item]}
        historyAnnouncements={[]}
        closedRecords={{}}
        historyOpen={false}
        onHistoryOpenChange={() => {}}
        onCloseAnnouncement={onClose}
      />
    </div>
  )
}

export default function AnnouncementsModule({
  language,
  refreshToken = 0,
  initialMode = 'list',
  routeMode,
  onNavigate,
  headerActionSlotId,
  showListCreateAction = true,
}: AnnouncementsModuleProps): JSX.Element {
  const strings = useMemo(() => copy(language), [language])
  const [uncontrolledRouteMode, setUncontrolledRouteMode] = useState<AnnouncementRouteMode>(
    () => initialMode === 'create' ? { kind: 'create' } : { kind: 'list' },
  )
  const currentRouteMode = routeMode ?? uncontrolledRouteMode
  const [items, setItems] = useState<Announcement[]>([])
  const [loading, setLoading] = useState(true)
  const [, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [draft, setDraft] = useState<AnnouncementDraft>(EMPTY_DRAFT)
  const loadedEditorKeyRef = useRef<string | null>(null)
  const [submittingAction, setSubmittingAction] = useState<AnnouncementSubmitAction | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)
  const [previewItem, setPreviewItem] = useState<Announcement | null>(null)
  const editorItem = currentRouteMode.kind === 'edit'
    ? items.find((item) => item.id === currentRouteMode.id) ?? null
    : null
  const editorMode: AnnouncementEditorMode | null = currentRouteMode.kind === 'create'
    ? { kind: 'create' }
    : currentRouteMode.kind === 'edit' && editorItem
      ? { kind: 'edit', id: editorItem.id, status: editorItem.status }
      : null
  const isEditorRoute = currentRouteMode.kind !== 'list'

  const navigateAnnouncements = useCallback((nextMode: AnnouncementRouteMode) => {
    if (onNavigate) {
      if (nextMode.kind === 'create') {
        onNavigate(announcementCreatePath())
      } else if (nextMode.kind === 'edit') {
        onNavigate(announcementEditPath(nextMode.id))
      } else {
        onNavigate(announcementListPath())
      }
      return
    }
    setUncontrolledRouteMode(nextMode)
  }, [onNavigate])

  const load = useCallback(async (signal?: AbortSignal, mode: 'initial' | 'refresh' = 'refresh') => {
    if (mode === 'initial') {
      setLoading(true)
    } else {
      setRefreshing(true)
    }
    setError(null)
    try {
      const response = await fetchAnnouncements(signal)
      setItems(response.items)
    } catch (err) {
      if (signal?.aborted) return
      setError(err instanceof Error ? err.message : strings.error)
    } finally {
      if (!signal?.aborted) {
        setLoading(false)
        setRefreshing(false)
      }
    }
  }, [strings.error])

  useEffect(() => {
    const controller = new AbortController()
    void load(controller.signal, 'initial')
    return () => controller.abort()
  }, [load, refreshToken])

  useEffect(() => {
    if (currentRouteMode.kind === 'list') {
      loadedEditorKeyRef.current = null
      setDraft(EMPTY_DRAFT)
      return
    }
    if (currentRouteMode.kind === 'create') {
      if (loadedEditorKeyRef.current !== 'create') {
        loadedEditorKeyRef.current = 'create'
        setDraft(EMPTY_DRAFT)
      }
      return
    }

    const editorKey = `edit:${currentRouteMode.id}`
    if (loadedEditorKeyRef.current === editorKey) return
    if (editorItem) {
      loadedEditorKeyRef.current = editorKey
      setDraft(toDraft(editorItem))
      return
    }
    if (!loading) {
      setError(strings.notFound)
    }
  }, [currentRouteMode, editorItem, loading, strings.notFound])

  const startCreate = useCallback(() => {
    navigateAnnouncements({ kind: 'create' })
    setPreviewItem(null)
    setMessage(null)
    setError(null)
  }, [navigateAnnouncements])

  const startEdit = (item: Announcement) => {
    loadedEditorKeyRef.current = `edit:${item.id}`
    setDraft(toDraft(item))
    navigateAnnouncements({ kind: 'edit', id: item.id })
    setPreviewItem(null)
    setMessage(null)
    setError(null)
  }

  const closeEditor = () => {
    navigateAnnouncements({ kind: 'list' })
    setDraft(EMPTY_DRAFT)
    loadedEditorKeyRef.current = null
  }

  const submit = async (action: AnnouncementSubmitAction) => {
    const payload = toPayload(draft)
    const validationError = validateAnnouncementContentInput(payload.content, payload.displayKind)
    if (validationError === 'content') {
      setError(strings.validationContent)
      return
    }
    if (validationError === 'modal_title') {
      setError(strings.validationModalTitle)
      return
    }
    if (validationError === 'modal_body') {
      setError(strings.validationModalBody)
      return
    }
    setSubmittingAction(action)
    setError(null)
    try {
      let saved: Announcement
      if (editorMode?.kind === 'edit') {
        saved = await updateAnnouncement(editorMode.id, payload)
      } else {
        saved = await createAnnouncement(payload)
      }
      if (action === 'publish') {
        await publishAnnouncement(saved.id)
      }
      closeEditor()
      setMessage(action === 'publish' ? strings.savedAndPublished : strings.saved)
      await load(undefined, 'refresh')
    } catch (err) {
      setError(err instanceof Error ? err.message : strings.error)
    } finally {
      setSubmittingAction(null)
    }
  }

  const act = async (id: string, action: 'publish' | 'archive') => {
    setBusyId(id)
    setPreviewItem(null)
    setError(null)
    try {
      if (action === 'publish') {
        await publishAnnouncement(id)
        setMessage(strings.published)
      } else {
        await archiveAnnouncement(id)
        setMessage(strings.archived)
      }
      await load(undefined, 'refresh')
    } catch (err) {
      setError(err instanceof Error ? err.message : strings.error)
    } finally {
      setBusyId(null)
    }
  }

  const headerActionHost = headerActionSlotId && typeof document !== 'undefined'
    ? document.getElementById(headerActionSlotId)
    : null
  const headerAction = headerActionHost && !isEditorRoute
    ? createPortal(
      <Button type="button" size="sm" onClick={startCreate}>
        <Icon icon="mdi:plus" width={16} height={16} aria-hidden="true" />
        <span>{strings.newAnnouncement}</span>
      </Button>,
      headerActionHost,
    )
    : null

  return (
    <AdminModuleSurface className="announcements-module">
      {headerAction}
      {message ? <div className="announcements-message">{message}</div> : null}
      {error && !loading ? <div className="announcements-error">{error}</div> : null}

      {isEditorRoute ? (
        editorMode ? (
          <AnnouncementEditorPanel
            mode={editorMode}
            draft={draft}
            submittingAction={submittingAction}
            strings={strings}
            onBack={closeEditor}
            onChangeDraft={setDraft}
            onSubmit={(action) => void submit(action)}
          />
        ) : (
          <AdminLoadingRegion
            loadState={loading ? 'initial_loading' : 'error'}
            loadingLabel={strings.loading}
            errorLabel={strings.notFound}
            minHeight={240}
          />
        )
      ) : (
        <>
          <AnnouncementUserPreview
            item={previewItem}
            language={language}
            onClose={() => setPreviewItem(null)}
          />
          <AnnouncementsListPanel
            items={items}
            loading={loading}
            error={error}
            busyId={busyId}
            strings={strings}
            language={language}
            showCreateAction={showListCreateAction}
            onCreate={startCreate}
            onEdit={startEdit}
            onPreview={setPreviewItem}
            onAct={(id, action) => void act(id, action)}
          />
        </>
      )}
    </AdminModuleSurface>
  )
}

import { Suspense, lazy, useCallback, useEffect, useMemo, useState } from 'react'

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
import AdminLoadingRegion from '../components/AdminLoadingRegion'
import MarkdownContent from '../components/MarkdownContent'
import { StatusBadge, type StatusTone } from '../components/StatusBadge'
import SegmentedTabs, { type SegmentedTabsOption } from '../components/ui/SegmentedTabs'
import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../components/ui/select'
import { Icon } from '../lib/icons'
import type { Language } from '../i18n'

interface AnnouncementsModuleProps {
  language: Language
  refreshToken?: number
  initialMode?: 'list' | 'create'
}

interface AnnouncementDraft {
  title: string
  body: string
  displayKind: AnnouncementDisplayKind
}

type AnnouncementEditorMode =
  | { kind: 'create' }
  | { kind: 'edit'; id: string; status: AnnouncementStatus }

type AnnouncementSubmitAction = 'draft' | 'publish'
type AnnouncementBodyMode = 'markdown' | 'split' | 'wysiwyg'

type AnnouncementCopy = ReturnType<typeof copy>

const EMPTY_DRAFT: AnnouncementDraft = {
  title: '',
  body: '',
  displayKind: 'modal',
}

const MarkdownEditor = lazy(() => import('../components/MarkdownEditor'))

function copy(language: Language) {
  return language === 'zh'
    ? {
        title: '公告',
        description: '发布用户控制台公告。弹窗用于强提醒，滚动公告用于低打扰提示。',
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
        formDescriptionNew: '编写公告正文，选择展示方式后可保存草稿或直接发布。',
        formDescriptionEdit: '保存修改会更新草稿；发布会让用户控制台显示最新内容。',
        formDescriptionPublished: '保存已发布公告会归档旧公告，并生成新公告 ID 重新提醒用户。',
        formDescriptionArchived: '编辑归档公告会保留历史记录，并生成新的草稿或发布项。',
        titleLabel: '标题',
        titlePlaceholder: '例如：维护窗口通知',
        bodyLabel: '正文',
        bodyPlaceholder: '写给用户看的公告内容。',
        bodyA11yHint: '正文支持 Markdown，保存时保留 Markdown 原文。',
        bodyModeLabel: '正文编辑模式',
        bodyModeMarkdown: 'Markdown',
        bodyModeSplit: '左右对比',
        bodyModeWysiwyg: '所见即所得',
        bodyModePreviewLabel: 'Milkdown 只读预览',
        displayLabel: '展示方式',
        modal: '弹窗',
        ticker: '滚动',
        previewTitle: '用户侧预览',
        previewEmpty: '填写标题和正文后显示预览。',
        previewAcknowledge: '知道了',
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
          edit: '编辑',
          publish: '发布',
          archive: '归档',
        },
        actionBusy: '处理中…',
        validation: '标题和正文不能为空。',
        saved: '公告已保存。',
        savedAndPublished: '公告已保存并发布。',
        published: '公告已发布。',
        archived: '公告已归档。',
      }
    : {
        title: 'Announcements',
        description: 'Publish user-console notices. Modal announcements are high-attention, tickers are lower-interruption.',
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
        formDescriptionNew: 'Write the announcement body, choose a display mode, then save a draft or publish directly.',
        formDescriptionEdit: 'Saving updates the draft; publishing makes the latest content visible in the user console.',
        formDescriptionPublished: 'Saving a published announcement archives the old item and creates a new ID so users are reminded again.',
        formDescriptionArchived: 'Editing an archived announcement keeps the history entry and creates a new draft or published item.',
        titleLabel: 'Title',
        titlePlaceholder: 'For example: maintenance window',
        bodyLabel: 'Body',
        bodyPlaceholder: 'Write the user-facing announcement body.',
        bodyA11yHint: 'The body supports Markdown and is saved as the original Markdown text.',
        bodyModeLabel: 'Body editor mode',
        bodyModeMarkdown: 'Markdown',
        bodyModeSplit: 'Split',
        bodyModeWysiwyg: 'WYSIWYG',
        bodyModePreviewLabel: 'Milkdown read-only preview',
        displayLabel: 'Display',
        modal: 'Modal',
        ticker: 'Ticker',
        previewTitle: 'User preview',
        previewEmpty: 'Add a title and body to show the preview.',
        previewAcknowledge: 'Got it',
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
          edit: 'Edit',
          publish: 'Publish',
          archive: 'Archive',
        },
        actionBusy: 'Working…',
        validation: 'Title and body are required.',
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
    title: item.title,
    body: item.body,
    displayKind: item.displayKind,
  }
}

function toPayload(draft: AnnouncementDraft): AnnouncementMutationPayload {
  return {
    title: draft.title.trim(),
    body: draft.body.trim(),
    displayKind: draft.displayKind,
  }
}

function editorDescription(mode: AnnouncementEditorMode, strings: AnnouncementCopy): string {
  if (mode.kind === 'create') return strings.formDescriptionNew
  if (mode.status === 'published') return strings.formDescriptionPublished
  if (mode.status === 'archived') return strings.formDescriptionArchived
  return strings.formDescriptionEdit
}

function AnnouncementDisplayPreview({
  draft,
  strings,
}: {
  draft: AnnouncementDraft
  strings: AnnouncementCopy
}): JSX.Element {
  const title = draft.title.trim()
  const body = draft.body.trim()
  const hasContent = title.length > 0 || body.length > 0
  const previewTitle = title || strings.titlePlaceholder
  const previewBody = body || strings.previewEmpty

  return (
    <section className="announcements-preview" aria-labelledby="announcement-preview-label">
      <div className="announcements-preview-header">
        <span id="announcement-preview-label">{strings.previewTitle}</span>
        <StatusBadge tone="neutral">
          {displayLabel(draft.displayKind, strings)}
        </StatusBadge>
      </div>
      {draft.displayKind === 'ticker' ? (
        <div className="announcements-preview-ticker" aria-label={strings.ticker}>
          <div className="announcements-preview-icon" aria-hidden="true">
            <Icon icon="mdi:bullhorn-outline" width={18} height={18} />
          </div>
          <div className="announcements-preview-copy">
            <strong>{previewTitle}</strong>
            <MilkdownPreviewContent
              value={previewBody}
              label={strings.previewTitle}
              compact={!hasContent}
            />
          </div>
        </div>
      ) : (
        <div className="announcements-preview-modal" aria-label={strings.modal}>
          <div className="announcements-preview-modal-header">
            <Icon icon="mdi:bullhorn-outline" width={18} height={18} aria-hidden="true" />
            <strong>{previewTitle}</strong>
          </div>
          <MilkdownPreviewContent
            value={previewBody}
            label={strings.previewTitle}
            className="announcements-preview-body"
          />
          <div className="announcements-preview-modal-actions">
            <Button type="button" size="xs" disabled>
              {strings.previewAcknowledge}
            </Button>
          </div>
        </div>
      )}
    </section>
  )
}

function AnnouncementBodyEditor({
  mode,
  draft,
  strings,
  saving,
  onChangeDraft,
}: {
  mode: AnnouncementBodyMode
  draft: AnnouncementDraft
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
      value={draft.body}
      placeholder={strings.bodyPlaceholder}
      disabled={saving}
      onChange={(body) => onChangeDraft({ ...draft, body })}
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
          value={draft.body || strings.previewEmpty}
          label={strings.bodyModePreviewLabel}
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
      value={draft.body}
      placeholder={strings.bodyPlaceholder}
      disabled={saving}
      onChange={(body) => onChangeDraft({ ...draft, body })}
      fallback={textarea}
    />
  )
}

function MilkdownPreviewContent({
  value,
  label,
  compact = false,
  className,
}: {
  value: string
  label: string
  compact?: boolean
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
        compact ? 'announcements-milkdown-preview--compact' : '',
        className ?? '',
      ].filter(Boolean).join(' ')}
      onChange={() => {}}
      fallback={(
        <textarea
          className="textarea announcements-body-fallback announcements-body-fallback--readonly"
          value={value}
          aria-label={label}
          rows={compact ? 2 : 5}
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
        <Button type="button" variant="outline" size="sm" onClick={onBack} disabled={saving}>
          <Icon icon="mdi:arrow-left" width={16} height={16} aria-hidden="true" />
          <span>{strings.backToList}</span>
        </Button>
      </div>
      <label className="announcements-field">
        <span>{strings.titleLabel}</span>
        <Input
          id="announcement-title"
          name="announcement-title"
          value={draft.title}
          placeholder={strings.titlePlaceholder}
          onChange={(event) => onChangeDraft({ ...draft, title: event.target.value })}
          maxLength={120}
        />
      </label>
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
      <div className="announcements-editor-body-grid">
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
            strings={strings}
            saving={saving}
            onChangeDraft={onChangeDraft}
          />
        </div>
        <AnnouncementDisplayPreview draft={draft} strings={strings} />
      </div>
      <div className="announcements-editor-actions">
        <Button type="button" variant="outline" onClick={onBack} disabled={saving}>
          {strings.cancel}
        </Button>
        <Button type="submit" variant="secondary" disabled={saving}>
          {submittingAction === 'draft'
            ? strings.saving
            : mode.kind === 'edit' ? strings.saveChanges : strings.saveDraft}
        </Button>
        <Button
          type="button"
          onClick={() => onSubmit('publish')}
          disabled={saving}
        >
          {submittingAction === 'publish'
            ? strings.publishing
            : isPublishedEdit ? strings.saveAndPublishVersion : strings.saveAndPublish}
        </Button>
      </div>
    </form>
  )
}

function TextareaFallback({
  id,
  name,
  value,
  placeholder,
  ariaLabelledBy,
  ariaDescribedBy,
  disabled,
  onChange,
}: {
  id: string
  name: string
  value: string
  placeholder: string
  ariaLabelledBy: string
  ariaDescribedBy: string
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
      rows={7}
      maxLength={4000}
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
  onCreate,
  onEdit,
  onAct,
}: {
  items: Announcement[]
  loading: boolean
  error: string | null
  busyId: string | null
  strings: AnnouncementCopy
  language: Language
  onCreate: () => void
  onEdit: (item: Announcement) => void
  onAct: (id: string, action: 'publish' | 'archive') => void
}): JSX.Element {
  return (
    <div className="announcements-list">
      <div className="announcements-list-header">
        <div>
          <h3>{strings.listTitle}</h3>
          <p>{strings.listDescription}</p>
        </div>
        <Button type="button" size="sm" onClick={onCreate}>
          <Icon icon="mdi:plus" width={16} height={16} aria-hidden="true" />
          <span>{strings.newAnnouncement}</span>
        </Button>
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
            <Button type="button" size="sm" onClick={onCreate}>
              <Icon icon="mdi:plus" width={16} height={16} aria-hidden="true" />
              <span>{strings.newAnnouncement}</span>
            </Button>
          </div>
        ) : (
          <>
            <div className="table-wrapper announcements-table-wrapper admin-responsive-up">
              <table className="jobs-table announcements-table">
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
                          <strong>{item.title}</strong>
                          <MarkdownContent content={item.body} compact />
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
              {items.map((item) => (
                <article key={item.id} className="admin-mobile-card announcements-mobile-card">
                  <header className="announcements-mobile-header">
                    <strong>{item.title}</strong>
                    <StatusBadge tone={statusTone(item.status)}>
                      {strings.status[item.status]}
                    </StatusBadge>
                  </header>
                  <MarkdownContent
                    content={item.body}
                    className="announcements-mobile-body"
                  />
                  <div className="admin-mobile-kv">
                    <span>{strings.table.display}</span>
                    <strong>{displayLabel(item.displayKind, strings)}</strong>
                  </div>
                  <div className="admin-mobile-kv">
                    <span>{strings.table.updated}</span>
                    <strong>{formatTimestamp(item.updatedAt, language)}</strong>
                  </div>
                  <div className="table-actions announcements-mobile-actions">
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
              ))}
            </div>
          </>
        )}
      </AdminLoadingRegion>
    </div>
  )
}

export default function AnnouncementsModule({
  language,
  refreshToken = 0,
  initialMode = 'list',
}: AnnouncementsModuleProps): JSX.Element {
  const strings = useMemo(() => copy(language), [language])
  const [items, setItems] = useState<Announcement[]>([])
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [draft, setDraft] = useState<AnnouncementDraft>(EMPTY_DRAFT)
  const [editorMode, setEditorMode] = useState<AnnouncementEditorMode | null>(
    () => initialMode === 'create' ? { kind: 'create' } : null,
  )
  const [submittingAction, setSubmittingAction] = useState<AnnouncementSubmitAction | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

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

  const startCreate = () => {
    setEditorMode({ kind: 'create' })
    setDraft(EMPTY_DRAFT)
    setMessage(null)
    setError(null)
  }

  const startEdit = (item: Announcement) => {
    setEditorMode({ kind: 'edit', id: item.id, status: item.status })
    setDraft(toDraft(item))
    setMessage(null)
    setError(null)
  }

  const closeEditor = () => {
    setEditorMode(null)
    setDraft(EMPTY_DRAFT)
  }

  const submit = async (action: AnnouncementSubmitAction) => {
    const payload = toPayload(draft)
    if (!payload.title || !payload.body) {
      setError(strings.validation)
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

  return (
    <section className="surface panel announcements-module">
      <div className="panel-header announcements-module-header">
        <div>
          <h2>{strings.title}</h2>
          <p className="panel-description">{strings.description}</p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => void load(undefined, 'refresh')}
          disabled={loading || refreshing}
        >
          <Icon
            icon={refreshing ? 'mdi:loading' : 'mdi:refresh'}
            width={16}
            height={16}
            className={refreshing ? 'icon-spin' : undefined}
            aria-hidden="true"
          />
          <span>{refreshing ? strings.refreshing : strings.refresh}</span>
        </Button>
      </div>

      {message ? <div className="announcements-message">{message}</div> : null}
      {error && !loading ? <div className="announcements-error">{error}</div> : null}

      {editorMode ? (
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
        <AnnouncementsListPanel
          items={items}
          loading={loading}
          error={error}
          busyId={busyId}
          strings={strings}
          language={language}
          onCreate={startCreate}
          onEdit={startEdit}
          onAct={(id, action) => void act(id, action)}
        />
      )}
    </section>
  )
}

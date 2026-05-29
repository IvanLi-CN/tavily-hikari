import { Button } from '../components/ui/button'
import QuotaRangeField from '../components/QuotaRangeField'
import { StatusBadge } from '../components/StatusBadge'
import type { AdminUserDetail, AdminUserQuotaBreakdownEntry } from '../api'
import type { AdminTranslations } from '../i18n'
import type { AdminRechargeTranslations } from '../i18n/adminRechargeTranslationTypes'
import {
  buildQuotaSliderTrack,
  clampQuotaSliderStageIndex,
  createQuotaSliderSeed,
  formatQuotaDraftInput,
  getQuotaSliderStagePosition,
  getQuotaSliderStageValue,
  parseQuotaDraftValue,
  type QuotaSliderField,
  type QuotaSliderSeed,
} from './quotaSlider'
import { UserDetailQuotaBreakdown } from './UserDetailQuotaBreakdown'
import { UserRechargeQuotaCalendar } from './UserRechargeQuotaCalendar'

type QuotaDraft = Record<QuotaSliderField, string>
type QuotaSnapshot = Record<QuotaSliderField, QuotaSliderSeed>

interface AdminUserDetailQuotaWorkspaceProps {
  detail: AdminUserDetail
  usersStrings: AdminTranslations['users']
  rechargeStrings: AdminRechargeTranslations['userDetail']
  language: 'en' | 'zh'
  quotaDraft: QuotaDraft | null
  quotaSnapshot: QuotaSnapshot | null
  quotaSavedAt: number | null
  savingQuota: boolean
  hasBlockAllTag: boolean
  formatNumber: (value: number) => string
  formatQuotaLimitValue: (value: number) => string
  formatSignedQuotaDelta: (value: number) => string
  formatSaveTime: (date: Date) => string
  onQuotaDraftChange: (field: QuotaSliderField, value: string) => void
  onSaveQuota: () => void
}

export function AdminUserDetailQuotaWorkspace({
  detail,
  usersStrings,
  rechargeStrings,
  language,
  quotaDraft,
  quotaSnapshot,
  quotaSavedAt,
  savingQuota,
  hasBlockAllTag,
  formatNumber,
  formatQuotaLimitValue,
  formatSignedQuotaDelta,
  formatSaveTime,
  onQuotaDraftChange,
  onSaveQuota,
}: AdminUserDetailQuotaWorkspaceProps): JSX.Element {
  const quotaFields = [
    {
      field: 'hourlyLimit',
      label: usersStrings.quota.hourly,
      used: detail.quotaHourlyUsed,
      currentLimit: detail.quotaBase.hourlyLimit,
    },
    {
      field: 'dailyLimit',
      label: usersStrings.quota.daily,
      used: detail.quotaDailyUsed,
      currentLimit: detail.quotaBase.dailyLimit,
    },
    {
      field: 'monthlyLimit',
      label: usersStrings.quota.monthly,
      used: detail.quotaMonthlyUsed,
      currentLimit: detail.quotaBase.monthlyLimit,
    },
  ] as const
  const quotaDirty = quotaDraft
    ? quotaFields.some((item) => {
        const snapshot = quotaSnapshot?.[item.field] ?? createQuotaSliderSeed(item.field, item.used, item.currentLimit)
        const draftValue = quotaDraft[item.field] ?? String(snapshot.initialLimit)
        return parseQuotaDraftValue(draftValue, snapshot.initialLimit) !== snapshot.initialLimit
      })
    : false
  const breakdownEntries = detail.quotaBreakdown.length > 0
    ? detail.quotaBreakdown
    : buildFallbackQuotaBreakdown(detail, rechargeStrings.rechargeColumn)

  return (
    <section className="surface panel user-detail-quota-workspace" id="user-detail-quota">
      <div className="panel-header" style={{ gap: 12, flexWrap: 'wrap' }}>
        <div>
          <h2>{usersStrings.quota.title}</h2>
          <p className="panel-description">{usersStrings.quota.description}</p>
        </div>
        <div className="user-detail-quota-status-row">
          <StatusBadge tone={detail.quotaBase.inheritsDefaults ? 'info' : 'neutral'}>
            {detail.quotaBase.inheritsDefaults ? usersStrings.quota.inheritsDefaults : usersStrings.quota.customized}
          </StatusBadge>
          {quotaDirty && <StatusBadge tone="warning">{usersStrings.quota.unsaved}</StatusBadge>}
        </div>
      </div>

      {hasBlockAllTag && (
        <div className="alert alert-warning" role="status">
          {usersStrings.effectiveQuota.blockAllNotice}
        </div>
      )}

      <div className="user-detail-quota-editor">
        <div className="quota-grid user-detail-quota-grid">
          {quotaFields.map((item) => {
            const sliderSeed = quotaSnapshot?.[item.field] ?? createQuotaSliderSeed(item.field, item.used, item.currentLimit)
            const draftValue = quotaDraft?.[item.field] ?? String(sliderSeed.initialLimit)
            const parsedDraft = parseQuotaDraftValue(draftValue, sliderSeed.initialLimit)
            return (
              <QuotaRangeField
                key={item.field}
                label={item.label}
                sliderName={`${item.field}-slider`}
                sliderMin={0}
                sliderMax={Math.max(0, sliderSeed.stages.length - 1)}
                sliderValue={getQuotaSliderStagePosition(sliderSeed.stages, parsedDraft)}
                sliderAriaLabel={item.label}
                sliderStyle={{ background: buildQuotaSliderTrack(sliderSeed.stages, sliderSeed.used, parsedDraft) }}
                onSliderChange={(nextValue) => {
                  const nextIndex = clampQuotaSliderStageIndex(sliderSeed.stages, nextValue)
                  onQuotaDraftChange(item.field, String(getQuotaSliderStageValue(sliderSeed.stages, nextIndex)))
                }}
                helperText={<>{formatNumber(sliderSeed.used)} / {formatNumber(parsedDraft)}</>}
                inputName={item.field}
                inputValue={formatQuotaDraftInput(draftValue)}
                inputAriaLabel={`${item.label} input`}
                onInputChange={(nextValue) => onQuotaDraftChange(item.field, nextValue)}
              />
            )
          })}
        </div>
        <div className={`user-detail-quota-savebar${quotaDirty ? ' user-detail-quota-savebar--dirty' : ''}`}>
          <span>
            {quotaDirty
              ? usersStrings.quota.unsaved
              : quotaSavedAt
                ? usersStrings.quota.savedAt.replace('{time}', formatSaveTime(new Date(quotaSavedAt)))
                : usersStrings.quota.hint}
          </span>
          <Button type="button" variant={quotaDirty ? 'default' : 'outline'} onClick={onSaveQuota} disabled={savingQuota || !quotaDirty}>
            {savingQuota ? usersStrings.quota.saving : usersStrings.quota.save}
          </Button>
        </div>
      </div>

      <div className="user-detail-quota-breakdown">
        <div className="user-detail-subsection-heading">
          <h3>{usersStrings.effectiveQuota.title}</h3>
          <p className="panel-description">{usersStrings.effectiveQuota.description}</p>
        </div>
        <UserDetailQuotaBreakdown
          entries={breakdownEntries}
          usersStrings={usersStrings}
          formatQuotaLimitValue={formatQuotaLimitValue}
          formatSignedQuotaDelta={formatSignedQuotaDelta}
        />
      </div>

      <UserRechargeQuotaCalendar
        detail={detail}
        strings={rechargeStrings}
        language={language}
        formatNumber={formatNumber}
        embedded
      />
    </section>
  )
}

function buildFallbackQuotaBreakdown(
  detail: AdminUserDetail,
  rechargeLabel: string,
): AdminUserQuotaBreakdownEntry[] {
  const rows: AdminUserQuotaBreakdownEntry[] = [
    {
      kind: 'base',
      label: 'base',
      tagId: null,
      tagName: null,
      source: null,
      effectKind: 'base',
      hourlyAnyDelta: detail.quotaBase.hourlyAnyLimit,
      hourlyDelta: detail.quotaBase.hourlyLimit,
      dailyDelta: detail.quotaBase.dailyLimit,
      monthlyDelta: detail.quotaBase.monthlyLimit,
    },
  ]
  const rechargeCredits = detail.recharge?.currentMonthEntitlementCredits ?? 0
  if (rechargeCredits > 0) {
    rows.push({
      kind: 'recharge',
      label: rechargeLabel,
      tagId: null,
      tagName: null,
      source: 'system_linuxdo',
      effectKind: 'quota_delta',
      hourlyAnyDelta: rechargeCredits,
      hourlyDelta: rechargeCredits,
      dailyDelta: rechargeCredits,
      monthlyDelta: rechargeCredits,
    })
  }
  rows.push({
    kind: 'effective',
    label: 'effective',
    tagId: null,
    tagName: null,
    source: null,
    effectKind: 'effective',
    hourlyAnyDelta: detail.effectiveQuota.hourlyAnyLimit,
    hourlyDelta: detail.effectiveQuota.hourlyLimit,
    dailyDelta: detail.effectiveQuota.dailyLimit,
    monthlyDelta: detail.effectiveQuota.monthlyLimit,
  })
  return rows
}

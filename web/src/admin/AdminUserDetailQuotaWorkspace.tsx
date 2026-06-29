import { Button } from '../components/ui/button'
import { UsageMetricLabel } from '../components/UsageMetricLabel'
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

function buildQuotaBreakdownEntry(
  entry: Pick<
    AdminUserQuotaBreakdownEntry,
    'kind' | 'label' | 'tagId' | 'tagName' | 'source' | 'effectKind'
  > & {
    businessCalls1hDelta: number
    dailyCreditsDelta: number
    monthlyCreditsDelta: number
  },
): AdminUserQuotaBreakdownEntry {
  return {
    ...entry,
    businessCalls1hDelta: entry.businessCalls1hDelta,
    dailyCreditsDelta: entry.dailyCreditsDelta,
    monthlyCreditsDelta: entry.monthlyCreditsDelta,
  }
}

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
      field: 'businessCalls1hLimit',
      label: (
        <UsageMetricLabel label={usersStrings.quota.hourly} kind="businessCalls1h" language={language} />
      ),
      ariaLabel: usersStrings.quota.hourly,
      used: detail.businessCalls1h.totalCount,
      currentLimit: detail.quotaBase.businessCalls1hLimit,
    },
    {
      field: 'dailyCreditsLimit',
      label: <UsageMetricLabel label={usersStrings.quota.daily} kind="dailyCredits" language={language} />,
      ariaLabel: usersStrings.quota.daily,
      used: detail.dailyCreditsUsed,
      currentLimit: detail.quotaBase.dailyCreditsLimit,
    },
    {
      field: 'monthlyCreditsLimit',
      label: <UsageMetricLabel label={usersStrings.quota.monthly} kind="monthlyCredits" language={language} />,
      ariaLabel: usersStrings.quota.monthly,
      used: detail.monthlyCreditsUsed,
      currentLimit: detail.quotaBase.monthlyCreditsLimit,
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
                sliderAriaLabel={item.ariaLabel}
                sliderStyle={{ background: buildQuotaSliderTrack(sliderSeed.stages, sliderSeed.used, parsedDraft) }}
                onSliderChange={(nextValue) => {
                  const nextIndex = clampQuotaSliderStageIndex(sliderSeed.stages, nextValue)
                  onQuotaDraftChange(item.field, String(getQuotaSliderStageValue(sliderSeed.stages, nextIndex)))
                }}
                helperText={<>{formatNumber(sliderSeed.used)} / {formatNumber(parsedDraft)}</>}
                inputName={item.field}
                inputValue={formatQuotaDraftInput(draftValue)}
                inputAriaLabel={`${item.ariaLabel} input`}
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
          language={language}
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
    buildQuotaBreakdownEntry({
      kind: 'base',
      label: 'base',
      tagId: null,
      tagName: null,
      source: null,
      effectKind: 'base',
      businessCalls1hDelta: detail.quotaBase.businessCalls1hLimit,
      dailyCreditsDelta: detail.quotaBase.dailyCreditsLimit,
      monthlyCreditsDelta: detail.quotaBase.monthlyCreditsLimit,
    }),
  ]
  const rechargeMonthlyDelta = detail.recharge?.currentMonthEntitlementMonthlyDelta ?? 0
  if (rechargeMonthlyDelta > 0) {
    rows.push(buildQuotaBreakdownEntry({
      kind: 'recharge',
      label: rechargeLabel,
      tagId: null,
      tagName: null,
      source: 'system_linuxdo',
      effectKind: 'quota_delta',
      businessCalls1hDelta: detail.recharge?.currentMonthEntitlementHourlyDelta ?? 0,
      dailyCreditsDelta: detail.recharge?.currentMonthEntitlementDailyDelta ?? 0,
      monthlyCreditsDelta: rechargeMonthlyDelta,
    }))
  }
  rows.push(buildQuotaBreakdownEntry({
    kind: 'effective',
    label: 'effective',
    tagId: null,
    tagName: null,
    source: null,
    effectKind: 'effective',
    businessCalls1hDelta: detail.effectiveQuota.businessCalls1hLimit,
    dailyCreditsDelta: detail.effectiveQuota.dailyCreditsLimit,
    monthlyCreditsDelta: detail.effectiveQuota.monthlyCreditsLimit,
  }))
  return rows
}

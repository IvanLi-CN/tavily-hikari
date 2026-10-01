import type { Language } from './types'
import type { AdminProvidersTranslations } from './adminProviderTranslationTypes'

/**
 * `admin.providers.*` copy for both catalogs (plan todo 22). Kept in a
 * dedicated module so the shared runtime catalogs stay within their source
 * line budgets; `types.ts` references {@link AdminProvidersTranslations} and
 * `en.ts`/`zh.ts` spread the matching record in one line each.
 *
 * The keys are exactly the stable registration points declared by todo 21
 * (`admin.providers.<id>`, `.validateProbe`, `.quotaNote`). Brand labels stay
 * identical across languages (proper nouns); all prose is localized.
 */
export const adminProviderTranslations: Record<Language, AdminProvidersTranslations> = {
  en: {
    tavily: {
      label: 'Tavily',
      validateProbe: 'Probe method: GET /usage (legacy credit probe; no extra cost).',
      quotaNote: 'Tavily Usage for this key',
    },
    serpapi: {
      label: 'SerpApi',
      validateProbe:
        'Probe method: GET /account (free, uncounted) — reads plan searches left, monthly searches, and renewal date.',
      quotaNote: 'SerpApi plan quota synced from /account (free, uncounted).',
      planFields: {
        planSearchesLeft: 'Plan searches left',
        totalSearchesLeft: 'Total searches left',
        searchesPerMonth: 'Searches / month',
        planRenewalDate: 'Plan renewal date',
        accountStatus: 'Account status',
      },
    },
    exa: {
      label: 'Exa',
      validateProbe:
        'Probe method: POST /contents with an empty body (401 = invalid key; unbilled 400/422 = valid key).',
      quotaNote: 'Exa has no upstream balance API; usage is counted locally per request.',
    },
    brave: {
      label: 'Brave',
      validateProbe:
        'Probe method: GET /res/v1/web/search without q (422 VALIDATION = valid; SUBSCRIPTION_TOKEN_INVALID = invalid; never billable).',
      quotaNote: 'Brave remaining quota is harvested from upstream response headers (passive snapshot).',
    },
    shared: {
      tableHeader: 'Provider',
      filterLabel: 'Provider',
      filterAll: 'All providers',
      selectLabel: 'Provider',
      selectBatchHint: 'One provider applies to the whole batch.',
      quotaUsed: 'Used',
      quotaRemaining: 'Remaining',
      quotaRenewalDate: 'Renews',
      quotaSynced: 'Synced',
      probeMethodLabel: 'Probe method',
      reasonLabel: 'Probe reason',
      exaVerdictValid: 'Key authenticated (unbilled validation error)',
      exaVerdictInvalid: 'Key rejected (401 unauthorized)',
      syncUnsupportedHint: 'Exa has no upstream balance API, so usage cannot be synced.',
      syncPassiveNotice: 'Returned the last harvested snapshot; no live upstream call was made.',
      syncErrorUnsupported: 'Usage sync is not supported for this provider.',
      reasonCodes: {
        VALIDATION: 'Validation response',
        SUBSCRIPTION_TOKEN_INVALID: 'Subscription token invalid',
        QUOTA_LIMITED: 'Quota limited',
        RATE_LIMITED: 'Rate limited',
        UNAUTHORIZED: 'Unauthorized',
        UNBILLED_VALIDATION_ERROR: 'Unbilled validation error',
      },
    },
  },
  zh: {
    tavily: {
      label: 'Tavily',
      validateProbe: '探针方式：GET /usage（沿用旧版额度探针，不产生额外费用）。',
      quotaNote: '此密钥的 Tavily 用量',
    },
    serpapi: {
      label: 'SerpApi',
      validateProbe: '探针方式：GET /account（免费、不计费）——读取套餐剩余搜索、每月搜索次数与续费日期。',
      quotaNote: 'SerpApi 套餐额度，来自 /account 同步（免费、不计费）。',
      planFields: {
        planSearchesLeft: '套餐剩余搜索',
        totalSearchesLeft: '剩余搜索总数',
        searchesPerMonth: '每月搜索次数',
        planRenewalDate: '套餐续费日期',
        accountStatus: '账户状态',
      },
    },
    exa: {
      label: 'Exa',
      validateProbe: '探针方式：向 /contents 发送空请求体（401 表示密钥无效；不计费的 400/422 表示密钥有效）。',
      quotaNote: 'Exa 没有上游余额接口，用量按请求在本地统计。',
    },
    brave: {
      label: 'Brave',
      validateProbe:
        '探针方式：GET /res/v1/web/search 且不带 q（422 VALIDATION 表示有效；SUBSCRIPTION_TOKEN_INVALID 表示无效；绝不产生费用）。',
      quotaNote: 'Brave 剩余额度来自上游响应头采集（被动快照）。',
    },
    shared: {
      tableHeader: '提供商',
      filterLabel: '提供商',
      filterAll: '全部提供商',
      selectLabel: '提供商',
      selectBatchHint: '同一批次只会应用同一个提供商。',
      quotaUsed: '已用',
      quotaRemaining: '剩余',
      quotaRenewalDate: '续费',
      quotaSynced: '同步时间',
      probeMethodLabel: '探针方式',
      reasonLabel: '探针原因',
      exaVerdictValid: '密钥已通过认证（不计费的校验错误）',
      exaVerdictInvalid: '密钥被拒绝（401 未授权）',
      syncUnsupportedHint: 'Exa 没有上游余额接口，无法同步用量。',
      syncPassiveNotice: '已返回最近一次采集的快照，未发起实时上游请求。',
      syncErrorUnsupported: '该提供商不支持用量同步。',
      reasonCodes: {
        VALIDATION: '校验响应',
        SUBSCRIPTION_TOKEN_INVALID: '订阅令牌无效',
        QUOTA_LIMITED: '额度受限',
        RATE_LIMITED: '触发限流',
        UNAUTHORIZED: '未授权',
        UNBILLED_VALIDATION_ERROR: '不计费的校验错误',
      },
    },
  },
}
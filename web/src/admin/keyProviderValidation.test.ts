import { describe, expect, it } from 'bun:test'

import { EN } from '../i18n/translations/en'
import { ZH } from '../i18n/translations/zh'
import {
  exaProbeVerdict,
  providerProbeCopy,
  providerQuotaCopy,
  reasonCodeLabel,
  serpApiPlanEntries,
} from './keyProviderValidation'

const SERPAPI_DETAIL =
  'plan_searches_left=120; total_searches_left=95; searches_per_month=500; plan_renewal_date=2026-04-01; account_status=Active'

describe('provider copy resolution (plan todo 22)', () => {
  it('resolves the probe method for every provider in en and zh', () => {
    for (const provider of ['tavily', 'serpapi', 'exa', 'brave'] as const) {
      expect(providerProbeCopy(EN.admin.providers, provider)).toBe(EN.admin.providers[provider].validateProbe)
      expect(providerProbeCopy(ZH.admin.providers, provider)).toBe(ZH.admin.providers[provider].validateProbe)
      expect(providerProbeCopy(ZH.admin.providers, provider)).not.toBe(providerProbeCopy(EN.admin.providers, provider))
    }
  })

  it('resolves the quota note for every provider in en and zh', () => {
    expect(providerQuotaCopy(EN.admin.providers, 'serpapi')).toContain('/account')
    expect(providerQuotaCopy(ZH.admin.providers, 'brave')).toContain('被动快照')
    expect(providerQuotaCopy(EN.admin.providers, 'exa')).toContain('no upstream balance API')
  })
})

describe('serpApiPlanEntries (plan todo 22)', () => {
  it('maps the five plan fields with localized labels and null -> em dash', () => {
    const entries = serpApiPlanEntries(SERPAPI_DETAIL, EN.admin.providers.serpapi.planFields)
    expect(entries.map((entry) => entry.id)).toEqual([
      'plan_searches_left',
      'total_searches_left',
      'searches_per_month',
      'plan_renewal_date',
      'account_status',
    ])
    expect(entries[0]).toMatchObject({ label: 'Plan searches left', value: '120' })
    expect(entries[4]).toMatchObject({ label: 'Account status', value: 'Active' })
  })

  it('renders null plan fields as em dashes and ignores non-plan detail', () => {
    const entries = serpApiPlanEntries(
      'plan_searches_left=null; total_searches_left=; searches_per_month=500; plan_renewal_date=null; account_status=null',
      EN.admin.providers.serpapi.planFields,
    )
    expect(entries[0]?.value).toBe('—')
    expect(entries[1]?.value).toBe('—')
    expect(entries[2]?.value).toBe('500')
    expect(serpApiPlanEntries('provider probe failed with 500', EN.admin.providers.serpapi.planFields)).toEqual([])
  })
})

describe('reason codes and Exa verdicts (plan todo 22)', () => {
  it('localizes known probe reason codes and ignores unknown ones', () => {
    expect(reasonCodeLabel(EN.admin.providers.shared, 'SUBSCRIPTION_TOKEN_INVALID')).toBe(
      'Subscription token invalid',
    )
    expect(reasonCodeLabel(ZH.admin.providers.shared, 'SUBSCRIPTION_TOKEN_INVALID')).toBe('订阅令牌无效')
    expect(reasonCodeLabel(EN.admin.providers.shared, 'SOMETHING_ELSE')).toBeNull()
  })

  it('maps Exa 401-invalid versus 400/422-valid wording', () => {
    const shared = EN.admin.providers.shared
    expect(exaProbeVerdict(shared, 'UNBILLED_VALIDATION_ERROR', 'ok')).toBe('Key authenticated (unbilled validation error)')
    expect(exaProbeVerdict(shared, 'UNAUTHORIZED', 'unauthorized')).toBe('Key rejected (401 unauthorized)')
    expect(exaProbeVerdict(shared, null, 'invalid')).toBe('Key rejected (401 unauthorized)')
    expect(exaProbeVerdict(shared, null, 'ok')).toBeNull()
  })
})
export type QuotaSliderField = 'hourlyAnyLimit' | 'hourlyLimit' | 'dailyLimit' | 'monthlyLimit'

export interface QuotaSliderSeed {
  field: QuotaSliderField
  used: number
  initialLimit: number
  stableMax: number
  stages: number[]
}

const QUOTA_SLIDER_DEFAULT_BASELINES: Readonly<Record<QuotaSliderField, number>> = {
  hourlyAnyLimit: 1_000,
  hourlyLimit: 1_000,
  dailyLimit: 10_000,
  monthlyLimit: 100_000,
}

const QUOTA_LINEAR_STAGE_VALUES = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100] as const
const QUOTA_EXP_STAGE_VALUES = [100, 120, 150, 200, 250, 300, 400, 500, 600, 800, 1_000] as const

const quotaDraftFormatter = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 0,
})

function coerceQuotaInteger(value: number, minimum: number): number {
  if (!Number.isFinite(value)) return minimum
  return Math.max(minimum, Math.trunc(value))
}

export function getQuotaSliderDefaultBaseline(field: QuotaSliderField): number {
  return QUOTA_SLIDER_DEFAULT_BASELINES[field]
}

function trimQuotaDraftSeparators(value: string | undefined): string {
  return (value ?? '').replace(/[\s,_']/g, '').trim()
}

export function normalizeQuotaDraftInput(value: string | undefined): string | null {
  const trimmed = trimQuotaDraftSeparators(value)
  if (!trimmed) return ''
  if (!/^\d{1,3}(,?\d{3})*$|^\d+$/.test(trimmed)) return null

  const digitsOnly = trimmed.replace(/,/g, '')
  const normalized = digitsOnly.replace(/^0+(?=\d)/, '')
  return normalized || '0'
}

export function formatQuotaDraftInput(value: string | undefined): string {
  const normalized = normalizeQuotaDraftInput(value)
  if (!normalized) return ''
  const parsed = Number.parseInt(normalized, 10)
  if (!Number.isFinite(parsed)) return normalized
  return quotaDraftFormatter.format(parsed)
}

export function parseQuotaDraftValue(value: string | undefined, fallback: number): number {
  const normalized = normalizeQuotaDraftInput(value)
  const parsed = Number.parseInt(normalized ?? '', 10)
  if (!Number.isFinite(parsed)) return coerceQuotaInteger(fallback, 1)
  return coerceQuotaInteger(parsed, 1)
}

export function resolveQuotaSliderStableMax(field: QuotaSliderField, initialLimit: number, used: number): number {
  return Math.max(
    1,
    getQuotaSliderDefaultBaseline(field),
    coerceQuotaInteger(initialLimit, 1),
    coerceQuotaInteger(used, 0),
  )
}

export function buildQuotaSliderStages(stableMax: number, extras: number[] = []): number[] {
  const resolvedMax = coerceQuotaInteger(stableMax, 1)
  const stages = new Set<number>()

  for (const value of QUOTA_LINEAR_STAGE_VALUES) {
    if (value <= resolvedMax) {
      stages.add(value)
    }
  }

  for (let scale = 1; scale <= 10 ** 9; scale *= 10) {
    const firstValue = QUOTA_EXP_STAGE_VALUES[0] * scale
    if (firstValue > resolvedMax) break
    for (const baseValue of QUOTA_EXP_STAGE_VALUES) {
      const value = baseValue * scale
      if (value <= resolvedMax) {
        stages.add(value)
      }
    }
  }

  for (const extra of extras) {
    if (!Number.isFinite(extra)) continue
    const value = Math.trunc(extra)
    if (value < 1 || value > resolvedMax) continue
    stages.add(value)
  }

  stages.add(resolvedMax)

  return [...stages].sort((left, right) => left - right)
}

export function createQuotaSliderSeed(
  field: QuotaSliderField,
  used: number,
  initialLimit: number,
): QuotaSliderSeed {
  const resolvedUsed = coerceQuotaInteger(used, 0)
  const resolvedInitialLimit = coerceQuotaInteger(initialLimit, 1)
  const stableMax = resolveQuotaSliderStableMax(field, resolvedInitialLimit, resolvedUsed)
  return {
    field,
    used: resolvedUsed,
    initialLimit: resolvedInitialLimit,
    stableMax,
    stages: buildQuotaSliderStages(stableMax, [resolvedInitialLimit, resolvedUsed]),
  }
}

export function findNearestQuotaSliderStageIndex(stages: readonly number[], value: number): number {
  if (stages.length === 0) return 0

  const resolvedValue = coerceQuotaInteger(value, 1)
  let bestIndex = 0
  let bestDistance = Number.POSITIVE_INFINITY

  for (const [index, stage] of stages.entries()) {
    const distance = Math.abs(stage - resolvedValue)
    if (distance < bestDistance || (distance === bestDistance && stage > stages[bestIndex])) {
      bestIndex = index
      bestDistance = distance
    }
  }

  return bestIndex
}

export function getQuotaSliderStagePosition(stages: readonly number[], value: number): number {
  if (stages.length <= 1) return 0

  const resolvedValue = coerceQuotaInteger(value, 0)
  if (resolvedValue <= stages[0]) return 0

  for (let index = 0; index < stages.length - 1; index += 1) {
    const left = stages[index] ?? 0
    const right = stages[index + 1] ?? left

    if (resolvedValue <= right) {
      if (right <= left) return index + 1
      return index + (resolvedValue - left) / (right - left)
    }
  }

  return stages.length - 1
}

export function clampQuotaSliderStageIndex(stages: readonly number[], index: number): number {
  if (stages.length === 0) return 0
  if (!Number.isFinite(index)) return 0
  return Math.min(stages.length - 1, Math.max(0, Math.round(index)))
}

export function getQuotaSliderStageValue(stages: readonly number[], index: number): number {
  if (stages.length === 0) return 1
  const resolvedIndex = clampQuotaSliderStageIndex(stages, index)
  return stages[resolvedIndex] ?? stages[stages.length - 1] ?? 1
}

function toQuotaRatioPercent(stages: readonly number[], value: number): number {
  if (stages.length <= 1) return coerceQuotaInteger(value, 0) > 0 ? 100 : 0
  return Math.min(100, Math.max(0, (getQuotaSliderStagePosition(stages, value) / (stages.length - 1)) * 100))
}

export function buildQuotaSliderTrack(stages: readonly number[], used: number, draftLimit: number): string {
  const usedRatio = toQuotaRatioPercent(stages, used)
  const draftRatio = toQuotaRatioPercent(stages, draftLimit)
  const start = Math.min(usedRatio, draftRatio)
  const end = Math.max(usedRatio, draftRatio)
  return `linear-gradient(to right, hsl(var(--warning) / 0.34) 0% ${start}%, hsl(var(--primary) / 0.44) ${start}% ${end}%, hsl(var(--muted) / 0.5) ${end}% 100%)`
}

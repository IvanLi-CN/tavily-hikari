import { describe, expect, it } from 'bun:test'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import path from 'node:path'

const PROJECT_ROOT = path.resolve(import.meta.dir, '..')
const SOURCE_ROOT = path.join(PROJECT_ROOT, 'src')
const IGNORE_DIRS = new Set(['node_modules', 'dist', 'storybook-static', '.git', '.turbo'])
const SOURCE_EXTENSIONS = new Set(['.js', '.jsx', '.ts', '.tsx', '.css'])
const STORY_PATTERN = /\.stories\.[jt]sx?$/

const DEFAULT_LIMITS = {
  runtime: 1500,
  story: 1800,
  css: 2200,
} as const

const EXCEPTIONS = new Map<string, { max: number; reason: string }>([
  [
    'src/admin/AdminDashboardRuntime.tsx',
    {
      max: 13480,
      reason: 'Legacy admin dashboard runtime remains as a compatibility shell while HA source settings, active-user list filtering, and the admin rankings live-status wiring finish converging before a larger extraction pass.',
    },
  ],
  [
    'src/admin/storySupport/AdminPagesStoryRuntime.tsx',
    {
      max: 7640,
      reason:
        'Storybook proof runtime remains centralized temporarily while active-user admin states, rankings shell proof, and system-settings proof data stay on the shared Admin/Pages proof shell.',
    },
  ],
  [
    'src/api/runtime.ts',
    {
      max: 3775,
      reason:
        'API barrel still carries HA source settings, planned cutover node-detail contracts, admin settings, auth-token retention contracts, grouped-alert dashboard summary contracts, user-list contracts, source dialog failure normalization, and user-console overview APIs until the proxy API surface is split out.',
    },
  ],
  [
    'src/api/demo.ts',
    {
      max: 2294,
      reason:
        'Demo API fixtures now cover user-console overview snapshots, alerts center mother-child aggregation states, request-record drawers, SSE proof states, auth-token retention settings, and recharge availability evidence on the shared demo shell.',
    },
  ],
  [
    'src/styles/admin.css',
    {
      max: 2260,
      reason:
        'The shared admin shell stylesheet now also carries the alerts-center layout refinements while the broader admin surface remains consolidated on the same runtime stylesheet.',
    },
  ],
  [
    'src/styles/public.css',
    {
      max: 2345,
      reason:
        'The shared public/runtime stylesheet now also carries the grouped-alert dashboard summary queue, review controls, and supporting shell refinements while those sections remain on the consolidated public surface.',
    },
  ],
  [
    'src/api.test.ts',
    {
      max: 1700,
      reason:
        'Shared API contract coverage now includes auth-token retention settings, the user-console overview snapshot, events surface, and the expanded admin rankings endpoint until the largest runtime suites are split out.',
    },
  ],
  [
    'src/admin/SystemSettingsModule.tsx',
    {
      max: 1573,
      reason:
        'System settings currently keeps the active-user default control, auth-token retention control, and supporting copy in the existing module pending a broader settings split.',
    },
  ],
  [
    'src/i18n/translations/en.ts',
    {
      max: 1595,
      reason:
        'Admin jobs maintenance copy, the expanded admin rankings grouping/dimension strings, the dashboard grouped-alert summary copy, and the auth-token retention settings copy are still stored in the shared English runtime catalog.',
    },
  ],
  [
    'src/i18n/translations/zh.ts',
    {
      max: 1595,
      reason:
        'Admin jobs maintenance copy, the expanded admin rankings grouping/dimension strings, the dashboard grouped-alert summary copy, and the auth-token retention settings copy are still stored in the shared Chinese runtime catalog.',
    },
  ],
  [
    'src/i18n/types.ts',
    {
      max: 1775,
      reason:
        'HA source settings mode-specific failure copy, planned-cutover and node-detail strings, admin jobs maintenance strings, the expanded admin rankings contract, grouped-alert dashboard summary strings, and auth-token retention settings copy remain in the shared catalog contract.',
    },
  ],
  [
    'src/user-console/runtime.tsx',
    {
      max: 3169,
      reason: 'User console runtime still carries the route-level shell while the new landing overview orchestration finishes splitting into dedicated hooks and sections.',
    },
  ],
  [
    'src/UserConsole.stories.tsx',
    {
      max: 2200,
      reason:
        'Console Storybook proof now also keeps the month-end recharge quote, clamp, and expired-order evidence on the same stable owner-facing story surface.',
    },
  ],
  [
    'src/admin/AdminRechargeRecordsModule.tsx',
    {
      max: 640,
      reason:
        'Recharge records module now carries the final amount column, expired state, and clamp marker proof alongside the existing refund workflow until the admin recharge surface is split further.',
    },
  ],
  [
    'src/admin/ForwardProxySettingsModule.tsx',
    {
      max: 3050,
      reason: 'Forward proxy settings now carries the node-pool and error-statistics surfaces; extraction remains a follow-up.',
    },
  ],
  [
    'src/components/AdminRecentRequestsPanel.tsx',
    {
      max: 1600,
      reason: 'Admin recent-requests panel is an existing shared surface that still needs a dedicated follow-up split.',
    },
  ],
  [
    'src/pages/TokenDetail.tsx',
    {
      max: 1700,
      reason: 'Token detail page remains on a temporary allowance until the route-level drill-down is decomposed separately.',
    },
  ],
])

function walk(dir: string, out: string[]): void {
  for (const entry of readdirSync(dir)) {
    if (IGNORE_DIRS.has(entry)) continue
    const fullPath = path.join(dir, entry)
    const stat = statSync(fullPath)
    if (stat.isDirectory()) {
      walk(fullPath, out)
      continue
    }
    if (SOURCE_EXTENSIONS.has(path.extname(entry))) {
      out.push(fullPath)
    }
  }
}

function relativeFile(file: string): string {
  return path.relative(PROJECT_ROOT, file).split(path.sep).join('/')
}

function countLines(file: string): number {
  const lines = readFileSync(file, 'utf8').split(/\r?\n/)
  if (lines.at(-1) === '') {
    lines.pop()
  }
  return lines.length
}

function resolveBudget(file: string): { max: number; category: string; reason?: string } {
  const relative = relativeFile(file)
  const exception = EXCEPTIONS.get(relative)
  if (exception) {
    return { max: exception.max, category: 'exception', reason: exception.reason }
  }
  if (path.extname(file) === '.css') {
    return { max: DEFAULT_LIMITS.css, category: 'css' }
  }
  if (STORY_PATTERN.test(file)) {
    return { max: DEFAULT_LIMITS.story, category: 'story' }
  }
  return { max: DEFAULT_LIMITS.runtime, category: 'runtime' }
}

describe('frontend source line budgets', () => {
  it('keeps source files within the configured line budgets', () => {
    const files: string[] = []
    walk(SOURCE_ROOT, files)
    files.sort()

    const overBudget = files.flatMap((file) => {
      const relative = relativeFile(file)
      const lines = countLines(file)
      const budget = resolveBudget(file)
      if (lines <= budget.max) {
        return []
      }
      const reason = budget.reason ? ` | reason: ${budget.reason}` : ''
      return [`${relative}: ${lines} lines > ${budget.max} (${budget.category})${reason}`]
    })

    expect(overBudget).toEqual([])
  })
})

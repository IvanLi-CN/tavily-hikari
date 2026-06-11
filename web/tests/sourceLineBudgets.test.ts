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
      max: 13193,
      reason: 'Legacy admin dashboard runtime remains as a compatibility shell while active-user list filtering lands before a larger extraction pass.',
    },
  ],
  [
    'src/admin/storySupport/AdminPagesStoryRuntime.tsx',
    {
      max: 7409,
      reason: 'Storybook proof runtime remains centralized temporarily while active-user admin states stay on the shared Admin/Pages proof shell.',
    },
  ],
  [
    'src/api/runtime.ts',
    {
      max: 3330,
      reason: 'API barrel still carries admin settings and user-list contracts until the proxy API surface is split out.',
    },
  ],
  [
    'src/api/demo.ts',
    {
      max: 1610,
      reason: 'Demo API fixtures now cover active-user default filtering states for real-shell admin evidence.',
    },
  ],
  [
    'src/admin/SystemSettingsModule.tsx',
    {
      max: 1560,
      reason: 'System settings currently keeps the active-user default control and supporting copy in the existing module pending a broader settings split.',
    },
  ],
  [
    'src/i18n/translations/en.ts',
    {
      max: 1528,
      reason: 'Admin jobs maintenance copy is still stored in the shared English runtime catalog.',
    },
  ],
  [
    'src/i18n/translations/zh.ts',
    {
      max: 1527,
      reason: 'Admin jobs maintenance copy is still stored in the shared Chinese runtime catalog.',
    },
  ],
  [
    'src/i18n/types.ts',
    {
      max: 1522,
      reason: 'Admin jobs maintenance translation types remain in the shared catalog contract.',
    },
  ],
  [
    'src/user-console/runtime.tsx',
    {
      max: 3100,
      reason: 'User console runtime still carries the route-level shell; token reset dialogs/actions have been split out as the next extraction step.',
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

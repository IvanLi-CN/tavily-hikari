import { describe, expect, it } from 'bun:test'
import { readdirSync, readFileSync } from 'node:fs'
import { extname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const SOURCE_ROOT = fileURLToPath(new URL('.', import.meta.url))
const ALLOWED_EXTENSIONS = new Set(['.ts', '.tsx', '.js', '.jsx', '.css'])
const EXCLUDED_FILE_RE = /\.(stories|test)\.[jt]sx?$/

const legacyPatterns: Array<{ label: string; regex: RegExp; extensions?: Set<string> }> = [
  { label: 'data-tip attribute', regex: /\bdata-tip\s*=/ },
  {
    label: 'legacy .tooltip selector',
    regex: /\.tooltip\b/,
    extensions: new Set(['.css']),
  },
  { label: 'legacy dropdown-content class', regex: /\bdropdown-content\b/ },
]

function collectSourceFiles(dir: string): string[] {
  const entries = readdirSync(dir, { withFileTypes: true })
  const files: string[] = []

  for (const entry of entries) {
    const nextPath = join(dir, entry.name)
    if (entry.isDirectory()) {
      files.push(...collectSourceFiles(nextPath))
      continue
    }

    if (!entry.isFile()) continue
    if (EXCLUDED_FILE_RE.test(entry.name)) continue
    if (!ALLOWED_EXTENSIONS.has(extname(entry.name))) continue
    files.push(nextPath)
  }

  return files
}

describe('overlay legacy pattern guard', () => {
  it('forbids non-portal tooltip and DaisyUI dropdown regressions in production source files', () => {
    const findings: string[] = []

    for (const filePath of collectSourceFiles(SOURCE_ROOT)) {
      const extension = extname(filePath)
      const content = readFileSync(filePath, 'utf8')
      for (const pattern of legacyPatterns) {
        if (pattern.extensions && !pattern.extensions.has(extension)) continue

        const match = content.match(pattern.regex)
        if (!match || match.index == null) continue

        const line = content.slice(0, match.index).split('\n').length
        findings.push(`${relative(SOURCE_ROOT, filePath)}:${line} ${pattern.label}`)
      }
    }

    expect(findings).toEqual([])
  })
})

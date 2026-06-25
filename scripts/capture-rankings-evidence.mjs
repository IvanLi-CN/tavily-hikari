#!/usr/bin/env node

import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { chromium } from '/Users/ivan/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '..')
const chromeExecutable = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const evidenceDir = path.join(repoRoot, 'docs/specs/p7n4k-admin-user-rankings/assets')
const liveBaseUrl = process.env.LIVE_BASE_URL ?? 'http://127.0.0.1:55174'
const rankingsUrl = `${liveBaseUrl.replace(/\/$/, '')}/admin/rankings?demo=true`
const themeStorageKey = 'tavily-hikari-theme-mode'

async function ensureDir(dir) {
  await fs.mkdir(dir, { recursive: true })
}

async function capture({
  url,
  viewport,
  output,
  waitForText,
  waitForSelector,
  settleMs = 1500,
  fullPage = false,
  themeMode = 'light',
  afterLoad,
}) {
  const browser = await chromium.launch({
    executablePath: chromeExecutable,
    headless: true,
  })

  try {
    const page = await browser.newPage({ viewport })
    await page.addInitScript(
      ({ storageKey, mode }) => {
        window.localStorage.setItem(storageKey, mode)
      },
      { storageKey: themeStorageKey, mode: themeMode },
    )
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 })
    if (waitForSelector) {
      await page.locator(waitForSelector).first().waitFor({ timeout: 15000 })
    } else if (waitForText) {
      await page.getByText(waitForText).first().waitFor({ timeout: 15000 })
    }
    if (afterLoad) {
      await afterLoad(page)
    }
    await page.waitForTimeout(settleMs)
    await page.screenshot({ path: output, fullPage })
  } finally {
    await browser.close()
  }
}

await ensureDir(evidenceDir)

await capture({
  url: rankingsUrl,
  viewport: { width: 1440, height: 1200 },
  output: path.join(evidenceDir, 'web-demo-rankings-desktop.png'),
  waitForSelector: '.admin-ranking-chart-shell',
  fullPage: false,
})

await capture({
  url: rankingsUrl,
  viewport: { width: 390, height: 1600 },
  output: path.join(evidenceDir, 'web-demo-rankings-mobile.png'),
  waitForSelector: '.admin-ranking-chart-shell',
  settleMs: 2000,
  fullPage: true,
})

await capture({
  url: rankingsUrl,
  viewport: { width: 1440, height: 1600 },
  output: path.join(evidenceDir, 'web-demo-rankings-dark-desktop.png'),
  waitForSelector: '.admin-ranking-chart-shell',
  fullPage: false,
  themeMode: 'dark',
})

await capture({
  url: `${rankingsUrl}&tab=businessCredits`,
  viewport: { width: 1440, height: 1200 },
  output: path.join(evidenceDir, 'web-demo-rankings-business-credits-highlight.png'),
  waitForSelector: '.admin-ranking-chart-shell',
  settleMs: 1200,
  fullPage: false,
  afterLoad: async (page) => {
    await page.getByRole('radio', { name: '积分' }).waitFor({ timeout: 15000 })
    const firstRow = page.getByRole('button', { name: /^1\. Alice Chen/ }).first()
    await firstRow.hover()
    await page.waitForTimeout(250)
  },
})

console.log(JSON.stringify({
  webDemoDesktop: path.join(evidenceDir, 'web-demo-rankings-desktop.png'),
  webDemoMobile: path.join(evidenceDir, 'web-demo-rankings-mobile.png'),
  webDemoDarkDesktop: path.join(evidenceDir, 'web-demo-rankings-dark-desktop.png'),
  webDemoBusinessCreditsHighlight: path.join(evidenceDir, 'web-demo-rankings-business-credits-highlight.png'),
}, null, 2))

#!/usr/bin/env node

import fs from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { chromium } from '/Users/ivan/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '..')
const chromeExecutable = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const evidenceDir = path.join(repoRoot, 'docs/specs/p7n4k-admin-user-rankings/assets')

async function ensureDir(dir) {
  await fs.mkdir(dir, { recursive: true })
}

async function capture({ url, viewport, output, waitForText, settleMs = 1500 }) {
  const browser = await chromium.launch({
    executablePath: chromeExecutable,
    headless: true,
  })

  try {
    const page = await browser.newPage({ viewport })
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 })
    if (waitForText) {
      await page.getByRole('heading', { name: waitForText }).first().waitFor({ timeout: 15000 })
    }
    await page.waitForTimeout(settleMs)
    await page.screenshot({ path: output, fullPage: true })
  } finally {
    await browser.close()
  }
}

await ensureDir(evidenceDir)

await capture({
  url: 'http://127.0.0.1:56006/iframe.html?id=admin-pages-userrankings--default&viewMode=story',
  viewport: { width: 1440, height: 1600 },
  output: path.join(evidenceDir, 'storybook-rankings-desktop-raw.png'),
  waitForText: '用户排行',
})

await capture({
  url: 'http://127.0.0.1:56006/iframe.html?id=admin-pages-userrankings--default&viewMode=story&globals=viewport:mobile1',
  viewport: { width: 430, height: 1800 },
  output: path.join(evidenceDir, 'storybook-rankings-mobile-raw.png'),
  waitForText: '用户排行',
})

await capture({
  url: 'http://127.0.0.1:58087/admin/rankings',
  viewport: { width: 1440, height: 1600 },
  output: path.join(evidenceDir, 'live-rankings-desktop-raw.png'),
  waitForText: '用户排行',
})

await capture({
  url: 'http://127.0.0.1:58087/admin/rankings',
  viewport: { width: 430, height: 1800 },
  output: path.join(evidenceDir, 'live-rankings-mobile-raw.png'),
  waitForText: '用户排行',
})

console.log(JSON.stringify({
  storybookDesktop: path.join(evidenceDir, 'storybook-rankings-desktop-raw.png'),
  storybookMobile: path.join(evidenceDir, 'storybook-rankings-mobile-raw.png'),
  liveDesktop: path.join(evidenceDir, 'live-rankings-desktop-raw.png'),
  liveMobile: path.join(evidenceDir, 'live-rankings-mobile-raw.png'),
}, null, 2))

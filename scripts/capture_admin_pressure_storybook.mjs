import { chromium } from 'playwright-core'

const captures = [
  {
    name: 'pressure-desktop.png',
    storyPath: '/iframe.html?id=admin-pages--pressure&viewMode=story',
    viewport: { width: 1440, height: 2200 },
  },
  {
    name: 'pressure-mobile.png',
    storyPath: '/iframe.html?id=admin-pages--pressure-mobile&viewMode=story',
    viewport: { width: 390, height: 2600 },
  },
]

const baseUrl = process.env.STORYBOOK_BASE_URL ?? 'http://127.0.0.1:54590'
const outputDir =
  process.env.OUTPUT_DIR ??
  'docs/specs/4q9xk-admin-route-hook-order-screen-split/assets/analysis-pressure'

const browser = await chromium.launch({ headless: true })

try {
  for (const capture of captures) {
    const page = await browser.newPage({ viewport: capture.viewport, deviceScaleFactor: 2 })
    await page.goto(`${baseUrl}${capture.storyPath}`, { waitUntil: 'networkidle', timeout: 30000 })
    await page.locator('#storybook-root').waitFor({ state: 'attached', timeout: 30000 })
    await page.screenshot({
      path: `${outputDir}/${capture.name}`,
      fullPage: true,
    })
    await page.close()
  }
} finally {
  await browser.close()
}

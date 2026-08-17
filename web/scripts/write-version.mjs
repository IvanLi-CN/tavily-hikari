import pkg from '../package.json'
import { resolve } from 'node:path'

const version = Bun.env.VITE_APP_VERSION?.trim() || pkg.version || '0.0.0'
const outputDir = resolve(import.meta.dir, '..', Bun.env.WEB_DIST_DIR || 'dist')
const outputPath = resolve(outputDir, 'version.json')
const htmlFiles = [
  'index.html',
  'admin.html',
  'console.html',
  'login.html',
  'registration-paused.html',
]
const markerPattern = /<meta\s+name="tavily-hikari-build-version"\s+content="__TAVILY_HIKARI_BUILD_VERSION__"\s*\/?\s*>/

function escapeHtmlAttribute(value) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll("'", '&#39;')
}

try {
  await Bun.write(outputPath, `${JSON.stringify({ version }, null, 2)}\n`)
  const escapedVersion = escapeHtmlAttribute(version)
  const replacement = `<meta name="tavily-hikari-build-version" content="${escapedVersion}" />`

  for (const fileName of htmlFiles) {
    const htmlPath = resolve(outputDir, fileName)
    const html = await Bun.file(htmlPath).text()
    const matches = html.match(markerPattern) || []
    if (matches.length !== 1) {
      throw new Error(`expected one version marker in ${htmlPath}, found ${matches.length}`)
    }
    await Bun.write(htmlPath, html.replace(markerPattern, replacement))
  }

  console.log(`[version] wrote ${version} to ${outputPath}`)
} catch (err) {
  console.error('[version] failed to write version.json:', err)
  throw err
}

import pkg from '../package.json'

const version = Bun.env.VITE_APP_VERSION || pkg.version || '0.0.0'
const outputPath = new URL('../dist/version.json', import.meta.url)

try {
  await Bun.write(outputPath, `${JSON.stringify({ version }, null, 2)}\n`)
  console.log(`[version] wrote ${version} to dist/version.json`)
} catch (err) {
  console.error('[version] failed to write version.json:', err)
  throw err
}

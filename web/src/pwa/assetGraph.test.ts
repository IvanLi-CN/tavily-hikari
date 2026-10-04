import { expect, test } from 'bun:test'

import { Window } from 'happy-dom'
import { createHash } from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const REQUIRED_ICON_SIZES = ['64x64', '96x96', '128x128', '144x144', '152x152', '167x167', '180x180', '192x192', '256x256', '384x384', '512x512', '1024x1024']
const HASHED_ICON_PATTERN = /^pwa\/(?:public|admin)(?:-maskable-\d+|-\d+)-[a-f0-9]{12}\.png$/

type IconGraph = {
  any: Record<string, string>
  maskable: Record<string, string>
}

type PwaGraph = {
  manifest: string
  serviceWorker: string
  files: string[]
  precacheFiles: string[]
  icons: IconGraph
}

type AssetGraph = {
  public: PwaGraph
  admin: PwaGraph
}

function iconPaths(graph: IconGraph): string[] {
  return [...Object.values(graph.any), ...Object.values(graph.maskable)]
}

function assertHashedIcon(distDir: string, relativePath: string): void {
  expect(relativePath).toMatch(HASHED_ICON_PATTERN)
  const content = fs.readFileSync(path.join(distDir, relativePath))
  const digest = createHash('sha256').update(content).digest('hex').slice(0, 12)
  expect(relativePath.endsWith(`-${digest}.png`)).toBe(true)
}

function readAssetGraph(): { distDir: string; graph: AssetGraph } | null {
  const distDir = path.resolve(import.meta.dir, '../../dist')
  const graphPath = path.join(distDir, 'pwa/asset-graphs.json')
  if (!fs.existsSync(graphPath)) return null
  return {
    distDir,
    graph: JSON.parse(fs.readFileSync(graphPath, 'utf8')) as AssetGraph,
  }
}

test('built asset graph keeps public and admin identities separated', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }
  const { distDir, graph } = build

  expect(graph.public.files).toContain('index.html')
  expect(graph.public.files).toContain('console.html')
  expect(graph.public.files).toContain('login.html')
  expect(graph.public.files).not.toContain('admin.html')
  expect(graph.admin.files).toContain('admin.html')
  expect(graph.admin.files).not.toContain('index.html')

  for (const identity of [graph.public, graph.admin]) {
    expect(identity.precacheFiles).toEqual(identity.files)
    expect(identity.precacheFiles).not.toContain(identity.manifest)
    for (const iconPath of iconPaths(identity.icons)) {
      assertHashedIcon(distDir, iconPath)
      expect(identity.precacheFiles).not.toContain(iconPath)
    }
  }

  const publicIcons = iconPaths(graph.public.icons)
  const adminIcons = iconPaths(graph.admin.icons)
  expect(new Set([...publicIcons, ...adminIcons]).size).toBe(publicIcons.length + adminIcons.length)
  expect(graph.public.precacheFiles).not.toContain(graph.admin.manifest)
  expect(graph.admin.precacheFiles).not.toContain(graph.public.manifest)
})

test('built manifests expose stable identities, full icon coverage, and maskable entries', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const [identity, expected] of [
    ['public', { graph: build.graph.public, id: '/', scope: '/', startUrl: '/', name: 'Tavily Hikari' }],
    ['admin', { graph: build.graph.admin, id: '/admin/', scope: '/admin/', startUrl: '/admin/', name: 'Tavily Hikari Admin' }],
  ] as const) {
    const manifestPath = path.join(build.distDir, expected.graph.manifest)
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as {
      id: string
      name: string
      start_url: string
      scope: string
      icons: Array<{ src: string; sizes: string; purpose?: string }>
    }
    expect(manifest).toMatchObject({
      id: expected.id,
      name: expected.name,
      start_url: expected.startUrl,
      scope: expected.scope,
    })
    expect(expected.graph.manifest).toBe(identity === 'public' ? 'manifest.webmanifest' : 'manifest-admin.webmanifest')

    const manifestIconPaths = manifest.icons.map((icon) => icon.src.slice(1)).sort()
    expect(manifestIconPaths).toEqual(iconPaths(expected.graph.icons).sort())
    for (const iconPath of manifestIconPaths) {
      assertHashedIcon(build.distDir, iconPath)
    }

    const sizes = manifest.icons.map((icon) => icon.sizes)
    for (const size of REQUIRED_ICON_SIZES) {
      expect(sizes).toContain(size)
    }
    const maskableSizes = manifest.icons
      .filter((icon) => icon.purpose === 'maskable')
      .map((icon) => icon.sizes)
    expect(maskableSizes).toContain('192x192')
    expect(maskableSizes).toContain('512x512')
  }
})

test('built HTML points to the matching manifest without a legacy touch-icon override', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const [htmlFile, manifestPath] of [
    ['index.html', 'manifest.webmanifest'],
    ['console.html', 'manifest.webmanifest'],
    ['login.html', 'manifest.webmanifest'],
    ['registration-paused.html', 'manifest.webmanifest'],
    ['admin.html', 'manifest-admin.webmanifest'],
  ]) {
    const html = fs.readFileSync(path.join(build.distDir, htmlFile), 'utf8')
    const parserWindow = new Window()
    const document = new parserWindow.DOMParser().parseFromString(html, 'text/html')
    const linkElements = Array.from(document.getElementsByTagName('link'))
    const manifestLinks = linkElements.filter((link) =>
      (link.getAttribute('rel') ?? '').split(/\s+/).includes('manifest'),
    )
    expect(manifestLinks.length).toBe(1)
    expect(manifestLinks[0]?.getAttribute('href')).toBe(`/${manifestPath}`)
    expect(
      linkElements.filter((link) =>
        (link.getAttribute('rel') ?? '').split(/\s+/).includes('apple-touch-icon'),
      ).length,
    ).toBe(0)
  }
  expect(fs.existsSync(path.join(build.distDir, 'assets/apple-touch-icon.png'))).toBe(false)
})

test('built service workers precache only the application shell and wait for explicit activation', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const identity of [build.graph.public, build.graph.admin]) {
    const serviceWorkerPath = path.join(build.distDir, identity.serviceWorker)
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    expect(source).toContain("ACTIVATE_UPDATE_MESSAGE = 'TAVILY_HIKARI_ACTIVATE_UPDATE'")
    expect(source).toContain('event.data.type === ACTIVATE_UPDATE_MESSAGE')
    expect(source).toContain('event.waitUntil(self.skipWaiting())')
    expect(source).not.toContain('await self.skipWaiting();')
    expect(source).not.toContain('self.clients.claim()')
    expect(source).toContain("cache.addAll(PRECACHE_URLS.map((url) => new Request(new URL(url, self.location.origin), { cache: 'reload' }))")
    expect(source).not.toContain(`/${identity.manifest}`)
    for (const precachedFile of identity.precacheFiles) {
      expect(source).toContain(`/${precachedFile}`)
    }
    for (const iconPath of iconPaths(identity.icons)) {
      expect(source).not.toContain(`/${iconPath}`)
    }
  }
})

test('built service workers keep manifests and icons out of installation precache', async () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  type WorkerEvent = {
    request: Request
    respondWith: (response: Promise<Response>) => void
    waitUntil: (promise: Promise<unknown>) => void
  }

  for (const identity of [build.graph.public, build.graph.admin]) {
    const source = fs.readFileSync(path.join(build.distDir, identity.serviceWorker), 'utf8')
    const listeners = new Map<string, (event: WorkerEvent) => void>()
    const requests: Request[] = []
    let installPromise: Promise<unknown> | null = null

    const workerSelf = {
      location: { origin: 'https://hikari.test' },
      addEventListener(type: string, listener: (event: WorkerEvent) => void) {
        listeners.set(type, listener)
      },
    }
    const workerCaches = {
      open: async () => ({
        addAll: async (entries: Request[]) => {
          requests.push(...entries)
        },
      }),
    }
    const requestOptions: RequestInit[] = []
    const requestConstructor = globalThis.Request
    const workerRequest = function(input: RequestInfo | URL, init?: RequestInit): Request {
      requestOptions.push(init ?? {})
      return new requestConstructor(input, init)
    }

    new Function('self', 'caches', 'Request', 'Response', 'fetch', source)(
      workerSelf,
      workerCaches,
      workerRequest,
      Response,
      globalThis.fetch,
    )
    const installListener = listeners.get('install')
    expect(installListener).toBeDefined()
    installListener?.({
      request: new requestConstructor('https://hikari.test/'),
      respondWith: () => undefined,
      waitUntil: (promise) => {
        installPromise = promise
      },
    })
    expect(installPromise).not.toBeNull()
    await installPromise

    expect(requests.length).toBeGreaterThan(0)
    expect(requests.length).toBe(identity.precacheFiles.length)
    expect(requestOptions).toHaveLength(requests.length)
    expect(requestOptions.every((options) => options.cache === 'reload')).toBe(true)
    const requestPaths = requests.map((request) => new URL(request.url).pathname)
    expect(requestPaths).toEqual(identity.precacheFiles.map((fileName) => `/${fileName}`))
    expect(requestPaths).not.toContain(`/${identity.manifest}`)
    for (const iconPath of iconPaths(identity.icons)) {
      expect(requestPaths).not.toContain(`/${iconPath}`)
    }
  }
})

test('built service workers leave manifests and icons on the network path', async () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  type FetchWorkerEvent = {
    request: Request
    respondWith: (response: Promise<Response>) => void
  }

  for (const identity of [build.graph.public, build.graph.admin]) {
    const source = fs.readFileSync(path.join(build.distDir, identity.serviceWorker), 'utf8')
    const listeners = new Map<string, (event: FetchWorkerEvent) => void>()
    let cacheMatchCalls = 0
    const networkRequests: Request[] = []
    const cache = {
      match: async () => {
        cacheMatchCalls += 1
        return new Response('stale-cache')
      },
      put: async () => undefined,
    }
    const workerSelf = {
      location: { origin: 'https://hikari.test' },
      addEventListener(type: string, listener: (event: FetchWorkerEvent) => void) {
        listeners.set(type, listener)
      },
    }
    const workerCaches = {
      open: async () => cache,
    }
    const workerFetch = async (request: Request) => {
      networkRequests.push(request)
      return new Response('network-response')
    }

    new Function('self', 'caches', 'Request', 'Response', 'fetch', source)(
      workerSelf,
      workerCaches,
      Request,
      Response,
      workerFetch,
    )
    const fetchListener = listeners.get('fetch')
    expect(fetchListener).toBeDefined()

    for (const requestPath of [`/${identity.manifest}`, `/${iconPaths(identity.icons)[0]}`]) {
      let responsePromise: Promise<Response> | null = null
      fetchListener?.({
        request: new Request(`https://hikari.test${requestPath}`),
        respondWith: (response) => {
          responsePromise = response
        },
      })
      expect(responsePromise).not.toBeNull()
      expect(await responsePromise?.then((response) => response.text())).toBe('network-response')
    }

    expect(cacheMatchCalls).toBe(0)
    expect(networkRequests.map((request) => new URL(request.url).pathname)).toEqual([
      `/${identity.manifest}`,
      `/${iconPaths(identity.icons)[0]}`,
    ])
  }
})

test('built service workers carry the release version in their identity', () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  const packagePath = path.resolve(import.meta.dir, '../../package.json')
  const packageVersion = (JSON.parse(fs.readFileSync(packagePath, 'utf8')) as { version: string }).version
  const version = process.env.VITE_APP_VERSION?.trim() || packageVersion
  expect(fs.existsSync(path.join(build.distDir, 'version.json'))).toBe(false)

  const applicationScripts = [...build.graph.public.files, ...build.graph.admin.files]
    .filter((file) => file.endsWith('.js'))
    .map((file) => fs.readFileSync(path.join(build.distDir, file), 'utf8'))
  expect(applicationScripts.some((source) => source.includes(JSON.stringify(version)))).toBe(true)

  for (const identity of [build.graph.public, build.graph.admin]) {
    const serviceWorkerPath = path.join(build.distDir, identity.serviceWorker)
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    expect(source).toContain(`const BUILD_VERSION = ${JSON.stringify(version)};`)
  }
})

test('built service workers leave network-only requests to the browser and contain runtime fetch failures', async () => {
  const build = readAssetGraph()
  if (!build) {
    expect(true).toBe(true)
    return
  }

  for (const identity of [build.graph.public, build.graph.admin]) {
    const serviceWorkerPath = path.join(build.distDir, identity.serviceWorker)
    const source = fs.readFileSync(serviceWorkerPath, 'utf8')
    const listeners = new Map<string, (event: { request: Request; respondWith: (response: Promise<Response>) => void }) => void>()
    const originalSelf = globalThis.self
    const originalFetch = globalThis.fetch

    Object.assign(globalThis, {
      self: {
        location: { origin: 'https://hikari.test' },
        addEventListener(type: string, listener: (event: { request: Request; respondWith: (response: Promise<Response>) => void }) => void) {
          listeners.set(type, listener)
        },
      },
      fetch: () => Promise.reject(new TypeError('network unavailable')),
    })

    try {
      new Function(source)()
      const fetchListener = listeners.get('fetch')
      expect(fetchListener).toBeDefined()

      let networkOnlyResponse: Promise<Response> | null = null
      fetchListener?.({
        request: new Request('https://hikari.test/mcp/console/state?refresh=true'),
        respondWith: (response) => {
          networkOnlyResponse = response
        },
      })
      expect(networkOnlyResponse).toBeNull()

      let runtimeResponse: Promise<Response> | null = null
      fetchListener?.({
        request: new Request('https://hikari.test/assets/lazy-chunk.js'),
        respondWith: (response) => {
          runtimeResponse = response
        },
      })
      expect(runtimeResponse).not.toBeNull()
      const response = await runtimeResponse
      expect(response.status).toBe(503)
    } finally {
      Object.assign(globalThis, { self: originalSelf, fetch: originalFetch })
    }
  }
})

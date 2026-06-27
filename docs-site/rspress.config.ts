import { defineConfig } from 'rspress/config'

function normalizeBase(base: string | undefined): string {
  const raw = (base ?? '/').trim()
  if (!raw || raw === '/') return '/'
  const withLeading = raw.startsWith('/') ? raw : `/${raw}`
  return withLeading.endsWith('/') ? withLeading : `${withLeading}/`
}

function assetWithBase(assetPath: string): string {
  const trimmed = assetPath.replace(/^\/+/, '')
  return docsBase === '/' ? `/${trimmed}` : `${docsBase}${trimmed}`
}

const docsBase = normalizeBase(process.env.DOCS_BASE)
const localStorybookDevOrigin = process.env.VITE_STORYBOOK_DEV_ORIGIN?.trim() ?? ''

export default defineConfig({
  root: 'docs',
  base: docsBase,
  icon: 'docs/public/favicon.svg',
  logo: {
    light: assetWithBase('/assets/relay-mesh-lockup-light.png'),
    dark: assetWithBase('/assets/relay-mesh-lockup-dark.png'),
  },
  logoText: 'Tavily Hikari Docs',
  lang: 'en',
  head: [
    ['meta', { name: 'theme-color', content: '#7c3aed' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: assetWithBase('/assets/relay-mesh-mark-light.svg'), media: '(prefers-color-scheme: light)' }],
    ['link', { rel: 'icon', type: 'image/svg+xml', href: assetWithBase('/assets/relay-mesh-mark-dark.svg'), media: '(prefers-color-scheme: dark)' }],
    ['link', { rel: 'icon', type: 'image/png', sizes: '32x32', href: assetWithBase('/assets/favicon-32x32.png') }],
    ['link', { rel: 'icon', type: 'image/png', sizes: '16x16', href: assetWithBase('/assets/favicon-16x16.png') }],
    ['link', { rel: 'apple-touch-icon', href: assetWithBase('/assets/apple-touch-icon.png') }],
  ],
  locales: [
    {
      lang: 'en',
      label: 'English',
      title: 'Tavily Hikari Docs',
      description: 'Product, deployment, API, and operator guidance for Tavily Hikari.',
    },
    {
      lang: 'zh',
      label: '简体中文',
      title: 'Tavily Hikari 文档',
      description: 'Tavily Hikari 的产品、部署、API 与运维文档。',
    },
  ],
  builderConfig: {
    source: {
      define: {
        'process.env.RSPRESS_STORYBOOK_DEV_ORIGIN': JSON.stringify(localStorybookDevOrigin),
        'process.env.RSPRESS_DOCS_BASE': JSON.stringify(docsBase),
      },
    },
  },
  themeConfig: {
    search: true,
    localeRedirect: 'never',
    locales: [
      {
        lang: 'en',
        label: 'English',
        title: 'Tavily Hikari Docs',
        description: 'Product, deployment, API, and operator guidance for Tavily Hikari.',
        logoText: 'Tavily Hikari Docs',
        nav: [
          { text: 'Home', link: '/' },
          { text: 'Quick Start', link: '/quick-start' },
          { text: 'Deployment', link: '/deployment-anonymity' },
          { text: 'Storybook', link: '/storybook.html' },
          { text: 'GitHub', link: 'https://github.com/IvanLi-CN/tavily-hikari', position: 'right' },
        ],
        sidebar: {
          '/': [
            {
              text: 'Documentation',
              items: [
                { text: 'Home', link: '/' },
                { text: 'Quick Start', link: '/quick-start' },
                { text: 'Configuration & Access', link: '/configuration-access' },
                { text: 'HTTP API Guide', link: '/http-api-guide' },
                { text: 'Deployment & Anonymity', link: '/deployment-anonymity' },
                { text: 'FAQ & Troubleshooting', link: '/faq' },
                { text: 'Development', link: '/development' },
              ],
            },
          ],
        },
      },
      {
        lang: 'zh',
        label: '简体中文',
        title: 'Tavily Hikari 文档',
        description: 'Tavily Hikari 的产品、部署、API 与运维文档。',
        logoText: 'Tavily Hikari 文档',
        nav: [
          { text: '首页', link: '/zh/' },
          { text: '快速开始', link: '/zh/quick-start' },
          { text: '部署', link: '/zh/deployment-anonymity' },
          { text: 'Storybook', link: '/zh/storybook.html' },
          { text: 'GitHub', link: 'https://github.com/IvanLi-CN/tavily-hikari', position: 'right' },
        ],
        sidebar: {
          '/zh/': [
            {
              text: '文档',
              items: [
                { text: '首页', link: '/zh/' },
                { text: '快速开始', link: '/zh/quick-start' },
                { text: '配置与访问', link: '/zh/configuration-access' },
                { text: 'HTTP API 指南', link: '/zh/http-api-guide' },
                { text: '部署与高匿名', link: '/zh/deployment-anonymity' },
                { text: 'FAQ 与排障', link: '/zh/faq' },
                { text: '开发', link: '/zh/development' },
              ],
            },
          ],
        },
      },
    ],
  },
})

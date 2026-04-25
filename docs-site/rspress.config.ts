import { defineConfig } from 'rspress/config'

function normalizeBase(base: string | undefined): string {
  const raw = (base ?? '/').trim()
  if (!raw || raw === '/') return '/'
  const withLeading = raw.startsWith('/') ? raw : `/${raw}`
  return withLeading.endsWith('/') ? withLeading : `${withLeading}/`
}

const docsBase = normalizeBase(process.env.DOCS_BASE)
const localStorybookDevOrigin = process.env.VITE_STORYBOOK_DEV_ORIGIN?.trim() ?? ''

export default defineConfig({
  root: 'docs',
  base: docsBase,
  lang: 'en',
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

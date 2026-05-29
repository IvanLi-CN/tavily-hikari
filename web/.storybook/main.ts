import path from 'node:path'
import { fileURLToPath } from 'node:url'
import type { StorybookConfig } from '@storybook/react-vite'

const dirname = path.dirname(fileURLToPath(import.meta.url))

const config: StorybookConfig = {
  stories: ['../src/**/*.stories.@(ts|tsx)'],
  addons: ['@storybook/addon-docs'],
  framework: {
    name: '@storybook/react-vite',
    options: {},
  },
  staticDirs: ['../public'],
  core: {
    // Avoid outbound calls during local/CI runs.
    disableTelemetry: true,
  },
  viteFinal(config) {
    config.resolve ??= {}
    const markdownEditorAlias = {
      find: '../components/MarkdownEditor',
      replacement: path.resolve(dirname, '../src/components/MarkdownEditor.storybook.tsx'),
    }
    config.resolve.alias = Array.isArray(config.resolve.alias)
      ? [markdownEditorAlias, ...config.resolve.alias]
      : [markdownEditorAlias, ...Object.entries(config.resolve.alias ?? {}).map(([find, replacement]) => ({ find, replacement }))]
    return config
  },
}

export default config

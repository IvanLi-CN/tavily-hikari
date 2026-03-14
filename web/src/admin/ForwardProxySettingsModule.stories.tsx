import type { Meta, StoryObj } from '@storybook/react-vite'

import ForwardProxySettingsModule from './ForwardProxySettingsModule'
import {
  forwardProxyStorySavedAt,
  forwardProxyStorySettings,
  forwardProxyStoryStats,
} from './forwardProxyStoryData'
import { LanguageProvider, useTranslate } from '../i18n'

function StoryCanvas(): JSX.Element {
  const strings = useTranslate().admin.proxySettings

  return (
    <div
      style={{
        minHeight: '100vh',
        padding: 24,
        color: 'hsl(var(--foreground))',
        background: [
          'radial-gradient(1000px 520px at 6% -8%, hsl(var(--primary) / 0.14), transparent 62%)',
          'radial-gradient(900px 460px at 95% -14%, hsl(var(--accent) / 0.12), transparent 64%)',
          'linear-gradient(180deg, hsl(var(--background)) 0%, hsl(var(--background)) 62%, hsl(var(--muted) / 0.58) 100%)',
          'hsl(var(--background))',
        ].join(', '),
      }}
    >
      <ForwardProxySettingsModule
        strings={strings}
        settingsLoadState="ready"
        statsLoadState="ready"
        settingsError={null}
        statsError={null}
        saveError={null}
        saving={false}
        savedAt={forwardProxyStorySavedAt}
        settings={forwardProxyStorySettings}
        stats={forwardProxyStoryStats}
        onPersistDraft={async () => {}}
        onValidateCandidates={async () => []}
        onRefresh={() => {}}
      />
    </div>
  )
}

const meta = {
  title: 'Admin/ForwardProxySettingsModule',
  parameters: {
    layout: 'fullscreen',
  },
  decorators: [
    (Story) => (
      <LanguageProvider>
        <Story />
      </LanguageProvider>
    ),
  ],
} satisfies Meta<typeof ForwardProxySettingsModule>

export default meta

type Story = StoryObj<typeof meta>

export const Default: Story = {
  render: () => <StoryCanvas />,
}

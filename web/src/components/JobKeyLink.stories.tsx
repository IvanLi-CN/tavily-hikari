import type { Meta, StoryObj } from '@storybook/react-vite'
import type { ReactNode } from 'react'

import { keyDetailPath } from '../admin/routes'
import { LanguageProvider, useTranslate } from '../i18n'

import JobKeyLink from './JobKeyLink'

function StoryShell({ children }: { children: ReactNode }): JSX.Element {
  return (
    <LanguageProvider>
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
        {children}
      </div>
    </LanguageProvider>
  )
}

function JobKeyLinkShowcaseCanvas(): JSX.Element {
  const { admin } = useTranslate()
  const detailPath = keyDetailPath('7QZ5')

  return (
    <div
      style={{
        display: 'grid',
        gap: 20,
        maxWidth: 960,
        margin: '0 auto',
      }}
    >
      <style>{`
        .storybook-jobs-key-showcase .jobs-key-tooltip[data-tip]::after {
          opacity: 1;
        }
      `}</style>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Desktop key link with group bubble</h2>
            <p className="panel-description">
              This mirrors the desktop jobs table state: the key links to <code>{detailPath}</code> and shows the
              group bubble.
            </p>
          </div>
        </div>
        <div className="storybook-jobs-key-showcase" style={{ display: 'grid', gap: 12 }}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              gap: 12,
              padding: '16px 18px',
              borderRadius: 24,
              border: '1px solid hsl(var(--border))',
              background: 'hsl(var(--card) / 0.92)',
            }}
          >
            <div style={{ display: 'grid', gap: 4 }}>
              <strong>Quota sync / row key cell</strong>
              <span className="panel-description">Bubble content should read the key group on desktop hover/focus.</span>
            </div>
            <JobKeyLink
              keyId="7QZ5"
              keyGroup="ops"
              ungroupedLabel={admin.keys.groups.ungrouped}
              detailLabel={admin.keys.actions.details}
            />
          </div>

          <div
            style={{
              display: 'grid',
              gap: 8,
              padding: '16px 18px',
              borderRadius: 24,
              border: '1px dashed hsl(var(--accent) / 0.45)',
              background: 'hsl(var(--accent) / 0.08)',
            }}
          >
            <strong>Expanded details panel key field</strong>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
              <span className="panel-description">Destination:</span>
              <code>{detailPath}</code>
              <JobKeyLink
                keyId="7QZ5"
                keyGroup="ops"
                ungroupedLabel={admin.keys.groups.ungrouped}
                detailLabel={admin.keys.actions.details}
              />
            </div>
          </div>
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Mobile keeps navigation without the bubble</h2>
            <p className="panel-description">
              The mobile jobs card still links to the same key details page, but does not render the group bubble.
            </p>
          </div>
        </div>
        <div
          style={{
            maxWidth: 360,
            padding: 16,
            borderRadius: 24,
            border: '1px solid hsl(var(--border))',
            background: 'hsl(var(--card) / 0.94)',
            display: 'grid',
            gap: 10,
          }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
            <span className="panel-description">{admin.jobs.table.key}</span>
            <JobKeyLink
              keyId="7QZ5"
              keyGroup="ops"
              ungroupedLabel={admin.keys.groups.ungrouped}
              detailLabel={admin.keys.actions.details}
              showBubble={false}
            />
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
            <span className="panel-description">Bubble</span>
            <strong>No</strong>
          </div>
        </div>
      </section>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Jobs without a key stay inert</h2>
            <p className="panel-description">Rows with no key keep the original dash placeholder and do not become links.</p>
          </div>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <span className="panel-description">{admin.jobs.table.key}</span>
          <JobKeyLink
            keyId={null}
            keyGroup={null}
            ungroupedLabel={admin.keys.groups.ungrouped}
            detailLabel={admin.keys.actions.details}
          />
        </div>
      </section>
    </div>
  )
}

function JobKeyLinkBubbleProofCanvas(): JSX.Element {
  const { admin } = useTranslate()

  return (
    <div
      style={{
        display: 'grid',
        gap: 20,
        maxWidth: 720,
        margin: '48px auto 0',
      }}
    >
      <style>{`
        .storybook-jobs-key-bubble-proof .jobs-key-tooltip[data-tip]::after {
          opacity: 1;
        }
      `}</style>

      <section className="surface panel">
        <div className="panel-header">
          <div>
            <h2>Desktop group bubble proof</h2>
            <p className="panel-description">
              Storybook-only proof view. The hover/focus state is forced visible so the implemented bubble can be seen
              in a static screenshot.
            </p>
          </div>
        </div>

        <div
          className="storybook-jobs-key-bubble-proof"
          style={{
            display: 'grid',
            placeItems: 'center',
            minHeight: 280,
            padding: 24,
            borderRadius: 24,
            border: '1px dashed hsl(var(--accent) / 0.45)',
            background: 'hsl(var(--accent) / 0.08)',
            overflow: 'visible',
          }}
        >
          <div
            style={{
              transform: 'scale(2.2)',
              transformOrigin: 'center center',
              paddingTop: 36,
            }}
          >
            <JobKeyLink
              keyId="7QZ5"
              keyGroup="ops"
              ungroupedLabel={admin.keys.groups.ungrouped}
              detailLabel={admin.keys.actions.details}
            />
          </div>
        </div>
      </section>
    </div>
  )
}

const meta = {
  title: 'Admin/Components/JobKeyLink',
  parameters: {
    layout: 'fullscreen',
    viewport: { defaultViewport: '1440-device-desktop' },
  },
  decorators: [
    (Story) => (
      <StoryShell>
        <Story />
      </StoryShell>
    ),
  ],
} satisfies Meta

export default meta

type Story = StoryObj<typeof meta>

export const Showcase: Story = {
  render: () => <JobKeyLinkShowcaseCanvas />,
}

export const BubbleProof: Story = {
  render: () => <JobKeyLinkBubbleProofCanvas />,
}

import type { Meta, StoryObj } from '@storybook/react-vite'

import OAuthCallbackPanel from './OAuthCallbackPanel'
import UserConsoleHeader from '../components/UserConsoleHeader'
import UserConsoleFooter from '../components/UserConsoleFooter'
import {
  resolveOAuthCallbackPanelModel,
  type OAuthCallbackPanelModel,
  type OAuthCallbackScreenState,
} from './oauthCallback'
import { ZH } from './text'

interface OAuthCallbackScenario {
  id: OAuthCallbackScreenState
  title: string
  description: string
  detail?: string
}

interface SingleStateFrameProps {
  model: OAuthCallbackPanelModel
}

const providerLabel = ZH.header.providers.linuxdo

const scenarios: OAuthCallbackScenario[] = [
  {
    id: 'connecting',
    title: 'Connecting',
    description: 'Fresh callback handoff while the frontend is exchanging code/state for a local session.',
  },
  {
    id: 'providerDenied',
    title: 'Provider Denied',
    description: 'LinuxDo returned `error=access_denied`, so the page must stay in-place with restart CTA.',
    detail: 'access_denied',
  },
  {
    id: 'invalidState',
    title: 'Invalid State',
    description: 'The callback was returned, but the server rejected the state as expired or already used.',
  },
  {
    id: 'timeout',
    title: 'Timeout',
    description: 'The finalize request crossed the frontend timeout budget and must not reuse the old authorization code.',
  },
  {
    id: 'upstreamFailure',
    title: 'Upstream Failure',
    description: 'The backend could not finish the provider token/userinfo exchange and surfaces a friendly retry path.',
    detail: 'userinfo upstream status 502: bad gateway',
  },
  {
    id: 'success',
    title: 'Success Handoff',
    description: 'The local session is ready and the page is about to jump into `/console`.',
  },
]

function buildModel(state: OAuthCallbackScenario): OAuthCallbackPanelModel {
  return resolveOAuthCallbackPanelModel({
    state: state.id,
    providerLabel,
    text: ZH.oauthCallback,
    detail: state.detail,
  })
}

function ScenarioCard({ scenario }: { scenario: OAuthCallbackScenario }): JSX.Element {
  return (
    <article
      style={{
        display: 'grid',
        gap: 16,
        minWidth: 0,
      }}
    >
      <div style={{ display: 'grid', gap: 6 }}>
        <div
          style={{
            fontSize: '0.78rem',
            fontWeight: 800,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'rgba(71, 85, 105, 0.92)',
          }}
        >
          {scenario.title}
        </div>
        <div style={{ fontSize: '0.94rem', lineHeight: 1.6, color: 'rgba(71, 85, 105, 0.92)' }}>
          {scenario.description}
        </div>
      </div>
      <OAuthCallbackPanel
        model={buildModel(scenario)}
        onRestart={() => undefined}
        onHome={() => undefined}
      />
    </article>
  )
}

function SingleStateFrame({ model }: SingleStateFrameProps): JSX.Element {
  return (
    <main
      className="app-shell public-home user-console-shell"
      style={{
        width: '100%',
        maxWidth: 'none',
      }}
    >
      <UserConsoleHeader
        title={ZH.title}
        subtitle={ZH.subtitle}
        eyebrow={ZH.header.eyebrow}
        currentViewLabel={ZH.header.currentView}
        currentViewTitle={ZH.header.views.oauthCallback}
        currentViewDescription={model.description}
        sessionLabel={ZH.header.session}
        sessionDisplayName={null}
        sessionProviderLabel={null}
        sessionAvatarUrl={null}
        adminLabel={ZH.header.adminLabel}
        isAdmin={false}
        adminHref={null}
        adminActionLabel={null}
        adminMenuLabel={null}
        announcementsLabel={null}
        announcementCount={0}
        onOpenAnnouncements={undefined}
        logoutVisible={false}
        isLoggingOut={false}
        logoutLabel={ZH.header.logout}
        loggingOutLabel={ZH.header.loggingOut}
        onLogout={() => undefined}
      />
      <div className="oauth-callback-stage">
        <OAuthCallbackPanel
          model={model}
          onRestart={() => undefined}
          onHome={() => undefined}
        />
      </div>
      <UserConsoleFooter
        strings={ZH.footer}
        versionState={{ status: 'ready', value: { backend: '0.2.0-dev', frontend: '0.2.0-dev' } }}
      />
    </main>
  )
}

function OAuthCallbackGallery(): JSX.Element {
  return (
    <div
      style={{
        display: 'grid',
        gap: 24,
        padding: 28,
        borderRadius: 28,
        background:
          'radial-gradient(circle at top, rgba(14, 165, 233, 0.12), transparent 28%), linear-gradient(180deg, rgba(248, 250, 252, 1), rgba(241, 245, 249, 1))',
      }}
    >
      <section style={{ display: 'grid', gap: 8, maxWidth: 760 }}>
        <div
          style={{
            fontSize: '0.78rem',
            fontWeight: 800,
            letterSpacing: '0.1em',
            textTransform: 'uppercase',
            color: 'rgba(100, 116, 139, 0.94)',
          }}
        >
          User Console Fragment
        </div>
        <h2 style={{ margin: 0, fontSize: '1.9rem', lineHeight: 1.08, color: '#0f172a' }}>
          OAuth Callback State Gallery
        </h2>
        <p style={{ margin: 0, fontSize: '1rem', lineHeight: 1.65, color: 'rgba(51, 65, 85, 0.92)' }}>
          Dedicated callback proof surface for the frontend handoff states. Review connecting, timeout, provider-denied,
          invalid-state, upstream-failure, and success handoff without relying on live OAuth traffic.
        </p>
      </section>
      <div
        style={{
          display: 'grid',
          gap: 18,
          gridTemplateColumns: 'repeat(auto-fit, minmax(320px, 1fr))',
          alignItems: 'start',
        }}
      >
        {scenarios.map((scenario) => (
          <ScenarioCard key={scenario.title} scenario={scenario} />
        ))}
      </div>
    </div>
  )
}

const meta = {
  title: 'User Console/Fragments/OAuth Callback',
  component: OAuthCallbackPanel,
  tags: ['autodocs'],
  args: {
    model: buildModel(scenarios[0]),
    onRestart: () => undefined,
    onHome: () => undefined,
  },
  parameters: {
    layout: 'fullscreen',
    controls: { disable: true },
    docs: {
      description: {
        component:
          'Standalone user-console OAuth callback fragment. Use this gallery to review the callback handoff states without needing a live LinuxDo provider round-trip.',
      },
    },
  },
} satisfies Meta<typeof OAuthCallbackPanel>

export default meta

type Story = StoryObj<typeof meta>

export const Gallery: Story = {
  name: 'State Gallery',
  args: {},
  render: () => <OAuthCallbackGallery />,
}

export const TimeoutRecovery: Story = {
  name: 'Timeout Recovery',
  args: {
    model: buildModel(scenarios[3]),
  },
}

export const SuccessHandoff: Story = {
  name: 'Success Handoff',
  args: {
    model: buildModel(scenarios[5]),
  },
}

export const DesktopTimeout: Story = {
  name: 'Desktop Timeout',
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: {
      defaultViewport: 'desktop',
    },
  },
  args: {
    model: buildModel(scenarios[3]),
  },
  render: (args) => <SingleStateFrame model={args.model} />,
}

export const DesktopConnecting: Story = {
  name: 'Desktop Connecting',
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: {
      defaultViewport: 'desktop',
    },
  },
  args: {
    model: buildModel(scenarios[0]),
  },
  render: (args) => <SingleStateFrame model={args.model} />,
}

export const MobileTimeout: Story = {
  name: 'Mobile Timeout',
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: {
      defaultViewport: 'mobile1',
    },
  },
  args: {
    model: buildModel(scenarios[3]),
  },
  render: (args) => <SingleStateFrame model={args.model} />,
}

export const DesktopSuccess: Story = {
  name: 'Desktop Success',
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: {
      defaultViewport: 'desktop',
    },
  },
  args: {
    model: buildModel(scenarios[5]),
  },
  render: (args) => <SingleStateFrame model={args.model} />,
}

export const MobileSuccess: Story = {
  name: 'Mobile Success',
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    viewport: {
      defaultViewport: 'mobile1',
    },
  },
  args: {
    model: buildModel(scenarios[5]),
  },
  render: (args) => <SingleStateFrame model={args.model} />,
}

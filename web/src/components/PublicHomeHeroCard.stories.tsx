import type { Meta, StoryObj } from '@storybook/react-vite'

import { useTranslate } from '../i18n'
import LanguageSwitcher from './LanguageSwitcher'
import PublicHomeHeroCard, { type PublicHomeHeroCardProps } from './PublicHomeHeroCard'
import ThemeToggle from './ThemeToggle'

type HeroStoryArgs = Omit<
  PublicHomeHeroCardProps,
  'publicStrings' | 'topControls' | 'linuxDoHref' | 'onTokenAccessClick' | 'onAdminActionClick'
>

const ADMIN_LABEL = '__ADMIN_LABEL__'
const LOGIN_LABEL = '__LOGIN_LABEL__'

function HeroStory(args: HeroStoryArgs): JSX.Element {
  const strings = useTranslate().public
  const resolvedAdminLabel = (() => {
    if (args.adminActionLabel === ADMIN_LABEL) return strings.adminButton
    if (args.adminActionLabel === LOGIN_LABEL) return strings.adminLoginButton
    return args.adminActionLabel
  })()

  return (
    <div style={{ maxWidth: 1120, margin: '0 auto' }}>
      <PublicHomeHeroCard
        {...args}
        adminActionLabel={resolvedAdminLabel}
        publicStrings={strings}
        topControls={(
          <>
            <ThemeToggle />
            <LanguageSwitcher />
          </>
        )}
      />
    </div>
  )
}

const baseArgs: HeroStoryArgs = {
  metricsLoading: false,
  summaryLoading: false,
  error: null,
  metrics: {
    monthlySuccess: 1240,
    dailySuccess: 87,
    freshness: {
      state: 'fresh',
      source: 'live',
      generatedAt: 1_782_000_000,
      reason: 'up_to_date',
    },
  },
  freshness: {
    state: 'fresh',
    source: 'live',
    generatedAt: 1_782_000_000,
    reason: 'up_to_date',
  },
  availableKeys: 7,
  totalKeys: 12,
  showAuthStatusLoading: false,
  showAuthStatusUnavailable: false,
  showLinuxDoLogin: false,
  showRegistrationPausedNotice: false,
  showTokenAccessButton: false,
  showAdminAction: false,
  adminActionLabel: LOGIN_LABEL,
}

const meta = {
  title: 'Public/PublicHomeHeroCard',
  parameters: {
    layout: 'padded',
  },
  render: (args) => <HeroStory {...args} />,
} satisfies Meta<HeroStoryArgs>

export default meta

type Story = StoryObj<typeof meta>

export const AuthStatusCheckingSlowStats: Story = {
  args: {
    ...baseArgs,
    metrics: null,
    freshness: null,
    availableKeys: null,
    totalKeys: null,
    metricsLoading: true,
    summaryLoading: true,
    showAuthStatusLoading: true,
    showTokenAccessButton: false,
    showAdminAction: false,
  },
}

export const AuthStatusUnavailable: Story = {
  args: {
    ...baseArgs,
    showAuthStatusUnavailable: true,
    showTokenAccessButton: true,
    showAdminAction: false,
  },
}

export const MetricsStaleFallback: Story = {
  args: {
    ...baseArgs,
    metrics: {
      monthlySuccess: 1234,
      dailySuccess: 82,
      freshness: {
        state: 'stale',
        source: 'last_good',
        generatedAt: 1_782_000_120,
        reason: 'sqlite_contention',
      },
    },
    freshness: {
      state: 'stale',
      source: 'last_good',
      generatedAt: 1_782_000_120,
      reason: 'sqlite_contention',
    },
  },
}

export const MetricsColdStartDegraded: Story = {
  args: {
    ...baseArgs,
    metrics: {
      monthlySuccess: 0,
      dailySuccess: 0,
      freshness: {
        state: 'degraded',
        source: 'cold_start_fallback',
        generatedAt: 1_782_000_180,
        reason: 'cold_start_no_cache',
      },
    },
    freshness: {
      state: 'degraded',
      source: 'cold_start_fallback',
      generatedAt: 1_782_000_180,
      reason: 'cold_start_no_cache',
    },
  },
}

export const FreshnessGallery: Story = {
  args: {
    ...baseArgs,
  },
  parameters: {
    viewport: { defaultViewport: 'desktop' },
  },
  render: () => (
    <div style={{ display: 'grid', gap: 24 }}>
      <section style={{ display: 'grid', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 18 }}>Fresh</h3>
        <HeroStory {...baseArgs} />
      </section>
      <section style={{ display: 'grid', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 18 }}>Stale last-good fallback</h3>
        <HeroStory {...(MetricsStaleFallback.args as HeroStoryArgs)} />
      </section>
      <section style={{ display: 'grid', gap: 12 }}>
        <h3 style={{ margin: 0, fontSize: 18 }}>Degraded cold start</h3>
        <HeroStory {...(MetricsColdStartDegraded.args as HeroStoryArgs)} />
      </section>
    </div>
  ),
}

export const LoggedOutNoToken: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showTokenAccessButton: true,
    showAdminAction: false,
  },
}

export const LoggedOutRegistrationPaused: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showRegistrationPausedNotice: true,
    showTokenAccessButton: true,
    showAdminAction: false,
  },
}

export const LoggedOutWithToken: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showTokenAccessButton: false,
    showAdminAction: false,
  },
}

export const LoggedInNoPrivilege: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: false,
    showTokenAccessButton: false,
    showAdminAction: false,
  },
}

export const LoggedInBuiltinAuth: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: false,
    showTokenAccessButton: false,
    showAdminAction: true,
    adminActionLabel: LOGIN_LABEL,
  },
}

export const LoggedInAdmin: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: false,
    showTokenAccessButton: false,
    showAdminAction: true,
    adminActionLabel: ADMIN_LABEL,
  },
}

export const LoggedOutNoTokenWithBuiltinAuth: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showTokenAccessButton: true,
    showAdminAction: true,
    adminActionLabel: LOGIN_LABEL,
  },
}

export const LoggedOutWithTokenBuiltinAuth: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showTokenAccessButton: false,
    showAdminAction: true,
    adminActionLabel: LOGIN_LABEL,
  },
}

export const LoggedOutNoTokenAdmin: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showTokenAccessButton: true,
    showAdminAction: true,
    adminActionLabel: ADMIN_LABEL,
  },
}

export const LoggedOutWithTokenAdmin: Story = {
  args: {
    ...baseArgs,
    showLinuxDoLogin: true,
    showTokenAccessButton: false,
    showAdminAction: true,
    adminActionLabel: ADMIN_LABEL,
  },
}

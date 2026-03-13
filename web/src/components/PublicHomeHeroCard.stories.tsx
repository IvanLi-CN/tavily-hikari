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
  loading: false,
  error: null,
  metrics: {
    monthlySuccess: 1240,
    dailySuccess: 87,
  },
  availableKeys: 7,
  totalKeys: 12,
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

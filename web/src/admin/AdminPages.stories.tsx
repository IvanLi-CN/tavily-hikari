import type { Meta } from '@storybook/react-vite'

import { LanguageProvider } from '../i18n'
import * as RuntimeStories from './storySupport/AdminPagesStoryRuntime'

const meta = {
  title: 'Admin/Pages',
  tags: ['autodocs'],
  parameters: {
    docs: {
      description: {
        component: [
          'Route-level admin review surface covering dashboard, keys, tokens, users, jobs, system settings, and forward proxy settings.',
          '',
          'Public docs: [Configuration & Access](../configuration-access.html) · [Deployment & Anonymity](../deployment-anonymity.html) · [Storybook Guide](../storybook-guide.html)',
        ].join('\n'),
      },
    },
    layout: 'fullscreen',
  },
  decorators: [
    (Story) => (
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
          <Story />
        </div>
      </LanguageProvider>
    ),
  ],
} satisfies Meta

export default meta

export const Dashboard = { ...RuntimeStories.Dashboard }
export const DashboardStacked = { ...RuntimeStories.DashboardStacked }
export const Tokens = { ...RuntimeStories.Tokens }
export const Keys = { ...RuntimeStories.Keys }
export const KeysSelected = { ...RuntimeStories.KeysSelected }
export const KeysSyncUsageInProgress = { ...RuntimeStories.KeysSyncUsageInProgress }
export const KeysSelectionRetainedAfterSync = { ...RuntimeStories.KeysSelectionRetainedAfterSync }
export const KeysRegistrationFilters = { ...RuntimeStories.KeysRegistrationFilters }
export const KeysTemporaryIsolationFilter = { ...RuntimeStories.KeysTemporaryIsolationFilter }
export const Requests = { ...RuntimeStories.Requests }
export const RequestsResultFilterOpen = { ...RuntimeStories.RequestsResultFilterOpen }
export const KeyDetailRecentRequests = { ...RuntimeStories.KeyDetailRecentRequests }
export const TokenDetailRecentRequests = { ...RuntimeStories.TokenDetailRecentRequests }
export const RequestsTokenDrawerDesktop = { ...RuntimeStories.RequestsTokenDrawerDesktop }
export const Jobs = { ...RuntimeStories.Jobs }
export const Users = { ...RuntimeStories.Users }
export const UsersUsage = { ...RuntimeStories.UsersUsage }
export const UsersUsageStacked = { ...RuntimeStories.UsersUsageStacked }
export const UsersUsageBreakageDrawerProof = { ...RuntimeStories.UsersUsageBreakageDrawerProof }
export const UnboundTokenUsage = { ...RuntimeStories.UnboundTokenUsage }
export const UnboundTokenUsageMonthlyBrokenSortProof = { ...RuntimeStories.UnboundTokenUsageMonthlyBrokenSortProof }
export const UnboundTokenUsageBreakageDrawerProof = { ...RuntimeStories.UnboundTokenUsageBreakageDrawerProof }
export const UnboundTokenUsageMobile = { ...RuntimeStories.UnboundTokenUsageMobile }
export const UnboundTokenUsageStacked = { ...RuntimeStories.UnboundTokenUsageStacked }
export const UnboundTokenUsageEmpty = { ...RuntimeStories.UnboundTokenUsageEmpty }
export const UnboundTokenUsageError = { ...RuntimeStories.UnboundTokenUsageError }
export const UnboundTokenUsageTokenDetailTrigger = { ...RuntimeStories.UnboundTokenUsageTokenDetailTrigger }
export const UsersUsageTooltipProof = { ...RuntimeStories.UsersUsageTooltipProof }
export const MonthlyBrokenDrawerEmpty = { ...RuntimeStories.MonthlyBrokenDrawerEmpty }
export const MonthlyBrokenDrawerSingleRow = { ...RuntimeStories.MonthlyBrokenDrawerSingleRow }
export const MonthlyBrokenDrawerLongContent = { ...RuntimeStories.MonthlyBrokenDrawerLongContent }
export const MonthlyBrokenDrawerOverflow = { ...RuntimeStories.MonthlyBrokenDrawerOverflow }
export const MonthlyBrokenDrawerMobile = { ...RuntimeStories.MonthlyBrokenDrawerMobile }
export const UserTags = { ...RuntimeStories.UserTags }
export const UserTagNew = { ...RuntimeStories.UserTagNew }
export const UserTagEdit = { ...RuntimeStories.UserTagEdit }
export const UserDetail = { ...RuntimeStories.UserDetail }
export const UserDetailCompact = { ...RuntimeStories.UserDetailCompact }
export const UserDetailSharedUsageTooltip = { ...RuntimeStories.UserDetailSharedUsageTooltip }
export const UserDetailMonthlyGap = { ...RuntimeStories.UserDetailMonthlyGap }
export const Alerts = { ...RuntimeStories.Alerts }
export const SystemSettings = { ...RuntimeStories.SystemSettings }
export const ProxySettings = { ...RuntimeStories.ProxySettings }

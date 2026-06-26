import type { Meta } from '@storybook/react-vite'

import * as HaStories from './storySupport/AdminPagesHaStories'
import * as AlertsStories from './storySupport/AdminPagesAlertsStories'
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
} satisfies Meta

export default meta

export const Dashboard = { ...RuntimeStories.Dashboard }
export const DashboardDark = {
  ...RuntimeStories.Dashboard,
  globals: {
    language: 'zh',
    themeMode: 'dark',
  },
  parameters: {
    ...RuntimeStories.Dashboard.parameters,
    viewport: { defaultViewport: '1440-device-desktop' },
    docs: {
      description: {
        story:
          'Dark-theme admin dashboard proof for the repaired low-light clay shell, sidebar, dense cards, charts, and loading regions.',
      },
    },
  },
}
export const DashboardStacked = { ...RuntimeStories.DashboardStacked }
export const Tokens = { ...RuntimeStories.Tokens }
export const Rankings = { ...RuntimeStories.Rankings }
export const RankingsDimension = { ...RuntimeStories.RankingsDimension }
export const RankingsEmpty = { ...RuntimeStories.RankingsEmpty }
export const RankingsLoading = { ...RuntimeStories.RankingsLoading }
export const RankingsMobile = { ...RuntimeStories.RankingsMobile }
export const Pressure = { ...RuntimeStories.Pressure }
export const PressureMobile = { ...RuntimeStories.PressureMobile }
export const Keys = { ...RuntimeStories.Keys }
export const KeysSelected = { ...RuntimeStories.KeysSelected }
export const KeysSyncUsageInProgress = { ...RuntimeStories.KeysSyncUsageInProgress }
export const KeysSelectionRetainedAfterSync = { ...RuntimeStories.KeysSelectionRetainedAfterSync }
export const KeysRegistrationFilters = { ...RuntimeStories.KeysRegistrationFilters }
export const KeysTemporaryIsolationFilter = { ...RuntimeStories.KeysTemporaryIsolationFilter }
export const Requests = { ...RuntimeStories.Requests }
export const RequestsResultFilterOpen = { ...RuntimeStories.RequestsResultFilterOpen }
export const Alerts = { ...AlertsStories.Alerts }
export const AlertsMobile = { ...AlertsStories.AlertsMobile }
export const KeyDetailRecentRequests = { ...RuntimeStories.KeyDetailRecentRequests }
export const TokenDetailRecentRequests = { ...RuntimeStories.TokenDetailRecentRequests }
export const RequestsTokenDrawerDesktop = { ...RuntimeStories.RequestsTokenDrawerDesktop }
export const Jobs = { ...RuntimeStories.Jobs }
export const Users = { ...RuntimeStories.Users }
export const UsersActiveOnlyDefault = { ...RuntimeStories.UsersActiveOnlyDefault }
export const UsersActiveOnlySearchAll = { ...RuntimeStories.UsersActiveOnlySearchAll }
export const UsersUsage = { ...RuntimeStories.UsersUsage }
export const UsersUsageActiveOnlyDefault = { ...RuntimeStories.UsersUsageActiveOnlyDefault }
export const UsersUsageActiveOnlySearchAll = { ...RuntimeStories.UsersUsageActiveOnlySearchAll }
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
export const UserDetailSingleTokenGuard = { ...RuntimeStories.UserDetailSingleTokenGuard }
export const UserDetailCompact = { ...RuntimeStories.UserDetailCompact }
export const UserDetailSharedUsageTooltip = { ...RuntimeStories.UserDetailSharedUsageTooltip }
export const UserDetailMonthlyGap = { ...RuntimeStories.UserDetailMonthlyGap }
export const UserDetailBusinessCalls1h = { ...RuntimeStories.UserDetailBusinessCalls1h }
export const UserDetailIpUsage = { ...RuntimeStories.UserDetailIpUsage }
export const Announcements = { ...RuntimeStories.Announcements }
export const SystemSettings = { ...RuntimeStories.SystemSettings }
export const SystemSettingsHa = { ...HaStories.SystemSettingsHa }
export const DashboardHaAttention = { ...HaStories.DashboardHaAttention }
export const SystemSettingsHaMobile = { ...HaStories.SystemSettingsHaMobile }
export const DashboardHaAttentionMobile = { ...HaStories.DashboardHaAttentionMobile }
export const ProxySettings = { ...RuntimeStories.ProxySettings }

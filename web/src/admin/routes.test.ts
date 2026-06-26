import { describe, expect, it } from 'bun:test'

import {
  analysisPath,
  announcementCreatePath,
  announcementEditPath,
  announcementListPath,
  alertsPath,
  buildAdminKeysPath,
  buildAdminUsersOverviewPath,
  buildAdminUsersPath,
  getRankingsTabFromSearch,
  getAlertsViewFromSearch,
  isAdminUsersOverviewSortField,
  isSameAdminRoute,
  keyDetailPath,
  parseAdminPath,
  rankingsPath,
  systemSettingsHaPath,
  systemSettingsHaNodePath,
  tokenDetailPath,
  unboundTokenUsagePath,
  userDetailPath,
  userTagCreatePath,
  userTagEditPath,
  userTagsPath,
  userUsagePath,
} from './routes'

describe('admin user tag routes', () => {
  it('parses the user tag index before user detail fallback', () => {
    expect(parseAdminPath('/admin/users/tags')).toEqual({ name: 'user-tags' })
  })

  it('parses the dedicated user usage page as an analysis usage alias before user detail fallback', () => {
    expect(parseAdminPath('/admin/users/usage')).toEqual({ name: 'module', module: 'analysis', analysisView: 'usage' })
  })

  it('parses the unbound token usage page before token detail fallback', () => {
    expect(parseAdminPath('/admin/tokens/leaderboard')).toEqual({ name: 'unbound-token-usage' })
  })

  it('parses the analysis routes and rankings alias', () => {
    expect(parseAdminPath('/admin/analysis')).toEqual({ name: 'module', module: 'analysis', analysisView: 'rankings' })
    expect(parseAdminPath('/admin/analysis/rankings')).toEqual({ name: 'module', module: 'analysis', analysisView: 'rankings' })
    expect(parseAdminPath('/admin/analysis/pressure')).toEqual({ name: 'module', module: 'analysis', analysisView: 'pressure' })
    expect(parseAdminPath('/admin/rankings')).toEqual({ name: 'module', module: 'analysis', analysisView: 'rankings' })
  })

  it('builds and parses stable rankings tab paths', () => {
    expect(rankingsPath()).toBe('/admin/rankings?tab=last24h')
    expect(rankingsPath('uniqueIp')).toBe('/admin/rankings?tab=uniqueIp')
    expect(getRankingsTabFromSearch('')).toBe('last24h')
    expect(getRankingsTabFromSearch('?tab=businessCredits')).toBe('businessCredits')
    expect(getRankingsTabFromSearch('?tab=invalid')).toBe('last24h')
  })

  it('parses the system settings module route', () => {
    expect(parseAdminPath('/admin/system-settings')).toEqual({
      name: 'module',
      module: 'system-settings',
      systemSettingsView: 'general',
    })
    expect(parseAdminPath('/admin/settings')).toEqual({
      name: 'module',
      module: 'system-settings',
      systemSettingsView: 'general',
    })
  })

  it('parses the system settings HA subpage route', () => {
    expect(parseAdminPath('/admin/system-settings/ha')).toEqual({
      name: 'module',
      module: 'system-settings',
      systemSettingsView: 'ha',
    })
  })

  it('parses the HA node detail route', () => {
    expect(parseAdminPath('/admin/system-settings/ha/nodes/demo-standby')).toEqual({
      name: 'ha-node',
      nodeId: 'demo-standby',
    })
  })

  it('parses dedicated announcement editor routes before module fallback', () => {
    expect(parseAdminPath('/admin/announcements')).toEqual({ name: 'module', module: 'announcements' })
    expect(parseAdminPath('/admin/announcements/new')).toEqual({ name: 'announcement-editor', mode: 'create' })
    expect(parseAdminPath('/admin/announcements/ann%2042/edit')).toEqual({
      name: 'announcement-editor',
      mode: 'edit',
      id: 'ann 42',
    })
  })

  it('parses the user tag create page', () => {
    expect(parseAdminPath('/admin/users/tags/new')).toEqual({ name: 'user-tag-editor', mode: 'create' })
  })

  it('parses the user tag edit page without colliding with user detail routes', () => {
    expect(parseAdminPath('/admin/users/tags/linuxdo_l2')).toEqual({
      name: 'user-tag-editor',
      mode: 'edit',
      id: 'linuxdo_l2',
    })
    expect(parseAdminPath('/admin/users/usr_alice')).toEqual({ name: 'user', id: 'usr_alice' })
  })

  it('builds stable user tag management paths', () => {
    expect(alertsPath()).toBe('/admin/alerts?view=groups')
    expect(alertsPath({ view: 'events' })).toBe('/admin/alerts?view=events')
    expect(getAlertsViewFromSearch('')).toBe('groups')
    expect(getAlertsViewFromSearch('?view=events')).toBe('events')
    expect(userTagsPath()).toBe('/admin/users/tags')
    expect(analysisPath()).toBe('/admin/analysis/rankings')
    expect(analysisPath('pressure')).toBe('/admin/analysis/pressure')
    expect(userUsagePath()).toBe('/admin/analysis/usage')
    expect(unboundTokenUsagePath()).toBe('/admin/tokens/leaderboard')
    expect(unboundTokenUsagePath('ops', 2, 'quotaMonthlyUsed', 'asc')).toBe(
      '/admin/tokens/leaderboard?q=ops&page=2&sort=quotaMonthlyUsed&order=asc',
    )
    expect(tokenDetailPath('tok 42')).toBe('/admin/tokens/tok%2042')
    expect(tokenDetailPath('tok 42', 'ops', 2, 'quotaMonthlyUsed', 'asc', 'unbound-usage')).toBe(
      '/admin/tokens/tok%2042?q=ops&page=2&sort=quotaMonthlyUsed&order=asc&view=unbound-usage',
    )
    expect(tokenDetailPath('tok 42', undefined, undefined, undefined, undefined, 'tokens', {
      query: 'legacy',
      group: 'ops',
      owner: 'bound',
      enabled: 'frozen',
      quotaState: 'day',
      page: 3,
      perPage: 50,
    })).toBe('/admin/tokens/tok%2042?q=legacy&group=ops&owner=bound&enabled=frozen&quota_state=day&page=3&perPage=50')
    expect(userTagCreatePath()).toBe('/admin/users/tags/new')
    expect(userTagEditPath('linuxdo l2')).toBe('/admin/users/tags/linuxdo%20l2')
    expect(announcementListPath()).toBe('/admin/announcements')
    expect(announcementCreatePath()).toBe('/admin/announcements/new')
    expect(announcementEditPath('ann 42')).toBe('/admin/announcements/ann%2042/edit')
    expect(systemSettingsHaPath()).toBe('/admin/system-settings/ha')
    expect(systemSettingsHaNodePath('demo standby')).toBe('/admin/system-settings/ha/nodes/demo%20standby')
  })

  it('preserves full users list context when building cross-page routes', () => {
    expect(buildAdminUsersPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userDetailPath('usr_alice', 'L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/usr_alice?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userTagsPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/tags?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userUsagePath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/analysis/usage?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userTagCreatePath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/tags/new?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userTagEditPath('linuxdo l2', 'L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users/tags/linuxdo%20l2?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc',
    )
    expect(userDetailPath('usr_alice')).toBe('/admin/users/usr_alice')
  })

  it('drops usage-only sort state when building an overview return path', () => {
    expect(isAdminUsersOverviewSortField('recentIpCount7d')).toBe(true)
    expect(isAdminUsersOverviewSortField('monthlySuccessRate')).toBe(false)
    expect(buildAdminUsersOverviewPath('L2', 'linuxdo_l2', 3, 'recentIpCount7d', 'asc')).toBe(
      '/admin/users?q=L2&tagId=linuxdo_l2&page=3&sort=recentIpCount7d&order=asc',
    )
    expect(buildAdminUsersOverviewPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc')).toBe(
      '/admin/users?q=L2&tagId=linuxdo_l2&page=3',
    )
  })

  it('marks detail and tag routes that were opened from the usage view', () => {
    expect(userDetailPath('usr_alice', 'L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc', 'usage')).toBe(
      '/admin/users/usr_alice?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc&view=usage',
    )
    expect(userTagsPath('L2', 'linuxdo_l2', 3, 'monthlySuccessRate', 'asc', 'usage')).toBe(
      '/admin/users/tags?q=L2&tagId=linuxdo_l2&page=3&sort=monthlySuccessRate&order=asc&view=usage',
    )
  })

  it('builds stable key list paths with pagination and repeated filters', () => {
    expect(buildAdminKeysPath()).toBe('/admin/keys')
    expect(
      buildAdminKeysPath({
        page: 2,
        perPage: 50,
        groups: ['ops', '', 'ops'],
        statuses: ['active', 'Quarantined', 'active'],
        registrationIp: '8.8.8.8',
        regions: ['US', 'US', 'US Westfield (MA)'],
      }),
    ).toBe(
      '/admin/keys?page=2&perPage=50&group=ops&group=&status=active&status=quarantined&registrationIp=8.8.8.8&region=US&region=US+Westfield+%28MA%29',
    )
    expect(
      keyDetailPath('key 42', {
        page: 3,
        perPage: 100,
        groups: ['ops'],
        statuses: ['disabled'],
        registrationIp: '8.8.4.4',
        regions: ['US Westfield (MA)'],
      }),
    ).toBe(
      '/admin/keys/key%2042?page=3&perPage=100&group=ops&status=disabled&registrationIp=8.8.4.4&region=US+Westfield+%28MA%29',
    )
  })

  it('compares user tag editor routes by mode and id', () => {
    expect(
      isSameAdminRoute(
        { name: 'user-tag-editor', mode: 'create' },
        { name: 'user-tag-editor', mode: 'create' },
      ),
    ).toBe(true)
    expect(
      isSameAdminRoute(
        { name: 'user-tag-editor', mode: 'edit', id: 'tag-a' },
        { name: 'user-tag-editor', mode: 'edit', id: 'tag-b' },
      ),
    ).toBe(false)
  })

  it('compares analysis routes by logical subview', () => {
    expect(
      isSameAdminRoute(
        { name: 'module', module: 'analysis', analysisView: 'usage' },
        { name: 'module', module: 'analysis', analysisView: 'usage' },
      ),
    ).toBe(true)
    expect(
      isSameAdminRoute(
        { name: 'module', module: 'analysis', analysisView: 'usage' },
        { name: 'module', module: 'analysis', analysisView: 'pressure' },
      ),
    ).toBe(false)
  })

  it('compares HA node routes by node id', () => {
    expect(isSameAdminRoute({ name: 'ha-node', nodeId: 'node-a' }, { name: 'ha-node', nodeId: 'node-a' })).toBe(true)
    expect(isSameAdminRoute({ name: 'ha-node', nodeId: 'node-a' }, { name: 'ha-node', nodeId: 'node-b' })).toBe(false)
  })
})

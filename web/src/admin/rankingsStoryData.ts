import type { AdminUserRankingRow, AdminUserRankingsSnapshot } from '../api/adminRankings'
import { buildRankingMockAvatarDataUrl } from './rankingAvatar'

function withMockAvatars(entries: Array<{ name: string; username: string | null; value: number }>) {
  return entries.map((entry) => {
    const userId = `usr_${entry.name.toLowerCase().replace(/[^a-z0-9]+/g, '_')}`
    return {
      ...entry,
      avatarUrl: buildRankingMockAvatarDataUrl(
        {
          userId,
          displayName: entry.name,
          username: entry.username,
          avatarUrl: null,
        },
        'User',
      ),
    }
  })
}

function buildRows(
  entries: Array<{ name: string; username: string | null; value: number; avatarUrl?: string | null }>,
): AdminUserRankingRow[] {
  return entries.map((entry, index) => ({
    rank: index + 1,
    value: entry.value,
    user: {
      userId: `usr_${entry.name.toLowerCase().replace(/[^a-z0-9]+/g, '_')}`,
      displayName: entry.name,
      username: entry.username,
      avatarUrl: entry.avatarUrl ?? null,
    },
  }))
}

const last24hPrimary = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 184 },
  { name: 'Bob Lin', username: 'bobby', value: 176 },
  { name: 'Carol Xu', username: 'carol', value: 169 },
  { name: 'Dan Wu', username: 'dan', value: 161 },
  { name: 'Erin Zhou', username: 'erin', value: 154 },
  { name: 'Frank He', username: 'frank', value: 149 },
  { name: 'Grace Li', username: 'grace', value: 143 },
  { name: 'Hana Su', username: 'hana', value: 137 },
  { name: 'Ivan Qiao', username: 'ivan', value: 133 },
  { name: 'Judy Gao', username: 'judy', value: 128 },
  { name: 'Kevin Sun', username: 'kevin', value: 123 },
  { name: 'Luna Tang', username: 'luna', value: 118 },
  { name: 'Milo Shen', username: 'milo', value: 114 },
  { name: 'Nora Ye', username: 'nora', value: 109 },
  { name: 'Owen Qi', username: 'owen', value: 104 },
  { name: 'Piper Lu', username: 'piper', value: 98 },
  { name: 'Quinn Ma', username: 'quinn', value: 92 },
  { name: 'Ryan Ji', username: 'ryan', value: 87 },
  { name: 'Sara Hu', username: 'sara', value: 81 },
  { name: 'Tina Fan', username: 'tina', value: 76 },
]))

const last24hCredits = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 628 },
  { name: 'Dan Wu', username: 'dan', value: 597 },
  { name: 'Erin Zhou', username: 'erin', value: 566 },
  { name: 'Frank He', username: 'frank', value: 542 },
  { name: 'Grace Li', username: 'grace', value: 519 },
  { name: 'Bob Lin', username: 'bobby', value: 504 },
  { name: 'Hana Su', username: 'hana', value: 488 },
  { name: 'Ivan Qiao', username: 'ivan', value: 472 },
  { name: 'Judy Gao', username: 'judy', value: 455 },
  { name: 'Kevin Sun', username: 'kevin', value: 439 },
  { name: 'Luna Tang', username: 'luna', value: 425 },
  { name: 'Milo Shen', username: 'milo', value: 409 },
  { name: 'Nora Ye', username: 'nora', value: 394 },
  { name: 'Owen Qi', username: 'owen', value: 381 },
  { name: 'Piper Lu', username: 'piper', value: 366 },
  { name: 'Quinn Ma', username: 'quinn', value: 351 },
  { name: 'Ryan Ji', username: 'ryan', value: 336 },
  { name: 'Sara Hu', username: 'sara', value: 321 },
  { name: 'Tina Fan', username: 'tina', value: 308 },
  { name: 'Uma Ren', username: 'uma', value: 294 },
]))

const last24hUniqueIp = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 42 },
  { name: 'Bob Lin', username: 'bobby', value: 39 },
  { name: 'Dan Wu', username: 'dan', value: 36 },
  { name: 'Erin Zhou', username: 'erin', value: 33 },
  { name: 'Carol Xu', username: 'carol', value: 31 },
  { name: 'Frank He', username: 'frank', value: 29 },
  { name: 'Grace Li', username: 'grace', value: 28 },
  { name: 'Hana Su', username: 'hana', value: 26 },
  { name: 'Ivan Qiao', username: 'ivan', value: 24 },
  { name: 'Judy Gao', username: 'judy', value: 23 },
  { name: 'Kevin Sun', username: 'kevin', value: 22 },
  { name: 'Luna Tang', username: 'luna', value: 20 },
  { name: 'Milo Shen', username: 'milo', value: 18 },
  { name: 'Nora Ye', username: 'nora', value: 17 },
  { name: 'Owen Qi', username: 'owen', value: 16 },
  { name: 'Piper Lu', username: 'piper', value: 15 },
  { name: 'Quinn Ma', username: 'quinn', value: 14 },
  { name: 'Ryan Ji', username: 'ryan', value: 13 },
  { name: 'Sara Hu', username: 'sara', value: 12 },
  { name: 'Tina Fan', username: 'tina', value: 11 },
]))

const last7dPrimary = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 1182 },
  { name: 'Bob Lin', username: 'bobby', value: 1131 },
  { name: 'Frank He', username: 'frank', value: 1094 },
  { name: 'Dan Wu', username: 'dan', value: 1057 },
  { name: 'Erin Zhou', username: 'erin', value: 1012 },
  { name: 'Grace Li', username: 'grace', value: 978 },
  { name: 'Hana Su', username: 'hana', value: 944 },
  { name: 'Ivan Qiao', username: 'ivan', value: 918 },
  { name: 'Judy Gao', username: 'judy', value: 891 },
  { name: 'Kevin Sun', username: 'kevin', value: 866 },
  { name: 'Luna Tang', username: 'luna', value: 842 },
  { name: 'Milo Shen', username: 'milo', value: 817 },
  { name: 'Nora Ye', username: 'nora', value: 791 },
  { name: 'Owen Qi', username: 'owen', value: 766 },
  { name: 'Piper Lu', username: 'piper', value: 744 },
  { name: 'Quinn Ma', username: 'quinn', value: 718 },
  { name: 'Ryan Ji', username: 'ryan', value: 693 },
  { name: 'Sara Hu', username: 'sara', value: 667 },
  { name: 'Tina Fan', username: 'tina', value: 645 },
  { name: 'Uma Ren', username: 'uma', value: 621 },
]))

const last7dCredits = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 4120 },
  { name: 'Dan Wu', username: 'dan', value: 3980 },
  { name: 'Erin Zhou', username: 'erin', value: 3815 },
  { name: 'Frank He', username: 'frank', value: 3662 },
  { name: 'Grace Li', username: 'grace', value: 3519 },
  { name: 'Bob Lin', username: 'bobby', value: 3380 },
  { name: 'Hana Su', username: 'hana', value: 3248 },
  { name: 'Ivan Qiao', username: 'ivan', value: 3111 },
  { name: 'Judy Gao', username: 'judy', value: 2994 },
  { name: 'Kevin Sun', username: 'kevin', value: 2870 },
  { name: 'Luna Tang', username: 'luna', value: 2748 },
  { name: 'Milo Shen', username: 'milo', value: 2615 },
  { name: 'Nora Ye', username: 'nora', value: 2491 },
  { name: 'Owen Qi', username: 'owen', value: 2384 },
  { name: 'Piper Lu', username: 'piper', value: 2277 },
  { name: 'Quinn Ma', username: 'quinn', value: 2168 },
  { name: 'Ryan Ji', username: 'ryan', value: 2059 },
  { name: 'Sara Hu', username: 'sara', value: 1944 },
  { name: 'Tina Fan', username: 'tina', value: 1825 },
  { name: 'Uma Ren', username: 'uma', value: 1708 },
]))

const last7dUniqueIp = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 161 },
  { name: 'Bob Lin', username: 'bobby', value: 154 },
  { name: 'Dan Wu', username: 'dan', value: 148 },
  { name: 'Frank He', username: 'frank', value: 143 },
  { name: 'Erin Zhou', username: 'erin', value: 139 },
  { name: 'Grace Li', username: 'grace', value: 132 },
  { name: 'Hana Su', username: 'hana', value: 128 },
  { name: 'Ivan Qiao', username: 'ivan', value: 121 },
  { name: 'Carol Xu', username: 'carol', value: 117 },
  { name: 'Judy Gao', username: 'judy', value: 114 },
  { name: 'Kevin Sun', username: 'kevin', value: 109 },
  { name: 'Luna Tang', username: 'luna', value: 103 },
  { name: 'Milo Shen', username: 'milo', value: 98 },
  { name: 'Nora Ye', username: 'nora', value: 93 },
  { name: 'Owen Qi', username: 'owen', value: 88 },
  { name: 'Piper Lu', username: 'piper', value: 83 },
  { name: 'Quinn Ma', username: 'quinn', value: 78 },
  { name: 'Ryan Ji', username: 'ryan', value: 73 },
  { name: 'Sara Hu', username: 'sara', value: 69 },
  { name: 'Tina Fan', username: 'tina', value: 64 },
]))

const last30dPrimary = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 4850 },
  { name: 'Bob Lin', username: 'bobby', value: 4728 },
  { name: 'Erin Zhou', username: 'erin', value: 4589 },
  { name: 'Dan Wu', username: 'dan', value: 4463 },
  { name: 'Frank He', username: 'frank', value: 4321 },
  { name: 'Grace Li', username: 'grace', value: 4190 },
  { name: 'Hana Su', username: 'hana', value: 4068 },
  { name: 'Ivan Qiao', username: 'ivan', value: 3944 },
  { name: 'Judy Gao', username: 'judy', value: 3822 },
  { name: 'Kevin Sun', username: 'kevin', value: 3711 },
  { name: 'Luna Tang', username: 'luna', value: 3596 },
  { name: 'Milo Shen', username: 'milo', value: 3474 },
  { name: 'Nora Ye', username: 'nora', value: 3361 },
  { name: 'Owen Qi', username: 'owen', value: 3250 },
  { name: 'Piper Lu', username: 'piper', value: 3132 },
  { name: 'Quinn Ma', username: 'quinn', value: 3018 },
  { name: 'Ryan Ji', username: 'ryan', value: 2897 },
  { name: 'Sara Hu', username: 'sara', value: 2781 },
  { name: 'Tina Fan', username: 'tina', value: 2668 },
  { name: 'Uma Ren', username: 'uma', value: 2550 },
]))

const last30dCredits = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 12680 },
  { name: 'Dan Wu', username: 'dan', value: 12140 },
  { name: 'Hana Su', username: 'hana', value: 11675 },
  { name: 'Erin Zhou', username: 'erin', value: 11294 },
  { name: 'Frank He', username: 'frank', value: 10928 },
  { name: 'Bob Lin', username: 'bobby', value: 10584 },
  { name: 'Grace Li', username: 'grace', value: 10192 },
  { name: 'Ivan Qiao', username: 'ivan', value: 9811 },
  { name: 'Judy Gao', username: 'judy', value: 9448 },
  { name: 'Kevin Sun', username: 'kevin', value: 9083 },
  { name: 'Luna Tang', username: 'luna', value: 8714 },
  { name: 'Milo Shen', username: 'milo', value: 8379 },
  { name: 'Nora Ye', username: 'nora', value: 8012 },
  { name: 'Owen Qi', username: 'owen', value: 7684 },
  { name: 'Piper Lu', username: 'piper', value: 7336 },
  { name: 'Quinn Ma', username: 'quinn', value: 6995 },
  { name: 'Ryan Ji', username: 'ryan', value: 6644 },
  { name: 'Sara Hu', username: 'sara', value: 6321 },
  { name: 'Tina Fan', username: 'tina', value: 5982 },
  { name: 'Uma Ren', username: 'uma', value: 5660 },
]))

const last30dUniqueIp = buildRows(withMockAvatars([
  { name: 'Alice Chen', username: 'alice', value: 522 },
  { name: 'Bob Lin', username: 'bobby', value: 504 },
  { name: 'Dan Wu', username: 'dan', value: 489 },
  { name: 'Erin Zhou', username: 'erin', value: 476 },
  { name: 'Frank He', username: 'frank', value: 461 },
  { name: 'Grace Li', username: 'grace', value: 447 },
  { name: 'Hana Su', username: 'hana', value: 432 },
  { name: 'Ivan Qiao', username: 'ivan', value: 419 },
  { name: 'Judy Gao', username: 'judy', value: 403 },
  { name: 'Carol Xu', username: 'carol', value: 392 },
  { name: 'Kevin Sun', username: 'kevin', value: 381 },
  { name: 'Luna Tang', username: 'luna', value: 367 },
  { name: 'Milo Shen', username: 'milo', value: 352 },
  { name: 'Nora Ye', username: 'nora', value: 338 },
  { name: 'Owen Qi', username: 'owen', value: 324 },
  { name: 'Piper Lu', username: 'piper', value: 309 },
  { name: 'Quinn Ma', username: 'quinn', value: 296 },
  { name: 'Ryan Ji', username: 'ryan', value: 281 },
  { name: 'Sara Hu', username: 'sara', value: 267 },
  { name: 'Tina Fan', username: 'tina', value: 252 },
]))

export const rankingsStorySnapshot: AdminUserRankingsSnapshot = {
  generatedAt: 1_781_763_600,
  refreshIntervalSecs: 10,
  last24h: {
    primarySuccessTop: last24hPrimary,
    businessCreditsTop: last24hCredits,
    uniqueIpTop: last24hUniqueIp,
  },
  last7d: {
    primarySuccessTop: last7dPrimary,
    businessCreditsTop: last7dCredits,
    uniqueIpTop: last7dUniqueIp,
  },
  last30d: {
    primarySuccessTop: last30dPrimary,
    businessCreditsTop: last30dCredits,
    uniqueIpTop: last30dUniqueIp,
  },
}

export const rankingsStoryEmptySnapshot: AdminUserRankingsSnapshot = {
  generatedAt: rankingsStorySnapshot.generatedAt,
  refreshIntervalSecs: rankingsStorySnapshot.refreshIntervalSecs,
  last24h: {
    primarySuccessTop: [],
    businessCreditsTop: [],
    uniqueIpTop: [],
  },
  last7d: {
    primarySuccessTop: [],
    businessCreditsTop: [],
    uniqueIpTop: [],
  },
  last30d: {
    primarySuccessTop: [],
    businessCreditsTop: [],
    uniqueIpTop: [],
  },
}

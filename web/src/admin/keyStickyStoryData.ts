import type { StickyNode, StickyUserDailyBucket, StickyUserRow } from '../api'

import { forwardProxyStoryStats } from './forwardProxyStoryData'

function createDailyBuckets(values: Array<[number, number]>): StickyUserDailyBucket[] {
  const rangeStartMs = Date.parse('2026-03-10T00:00:00Z')
  const bucketMs = 24 * 60 * 60 * 1000

  return Array.from({ length: 7 }, (_, index) => {
    const [successCredits, failureCredits] = values[index] ?? [0, 0]
    const bucketStartMs = rangeStartMs + index * bucketMs
    return {
      bucketStart: Math.floor(bucketStartMs / 1000),
      bucketEnd: Math.floor((bucketStartMs + bucketMs) / 1000),
      successCredits,
      failureCredits,
    }
  })
}

const stickyUserBuckets = {
  ivy: createDailyBuckets([
    [48, 4],
    [52, 2],
    [67, 5],
    [74, 4],
    [69, 3],
    [81, 6],
    [93, 5],
  ]),
  max: createDailyBuckets([
    [18, 0],
    [24, 1],
    [16, 2],
    [21, 1],
    [28, 2],
    [30, 3],
    [34, 2],
  ]),
  noah: createDailyBuckets([
    [0, 0],
    [7, 0],
    [9, 1],
    [11, 2],
    [13, 1],
    [15, 2],
    [12, 4],
  ]),
}

export const stickyUsersStoryData: StickyUserRow[] = [
  {
    user: {
      userId: 'usr_ivy',
      displayName: 'Ivy Chen',
      username: 'ivy',
      active: true,
      lastLoginAt: 1773378000,
      tokenCount: 3,
    },
    lastSuccessAt: 1773455100,
    windows: {
      yesterday: { successCredits: 81, failureCredits: 6 },
      today: { successCredits: 93, failureCredits: 5 },
      month: { successCredits: 484, failureCredits: 29 },
    },
    dailyBuckets: stickyUserBuckets.ivy,
  },
  {
    user: {
      userId: 'usr_max',
      displayName: 'Max Rivera',
      username: 'maxr',
      active: true,
      lastLoginAt: 1773452000,
      tokenCount: 2,
    },
    lastSuccessAt: 1773451200,
    windows: {
      yesterday: { successCredits: 30, failureCredits: 3 },
      today: { successCredits: 34, failureCredits: 2 },
      month: { successCredits: 171, failureCredits: 11 },
    },
    dailyBuckets: stickyUserBuckets.max,
  },
  {
    user: {
      userId: 'usr_noah',
      displayName: null,
      username: 'noah',
      active: false,
      lastLoginAt: 1773291600,
      tokenCount: 1,
    },
    lastSuccessAt: 1773448200,
    windows: {
      yesterday: { successCredits: 15, failureCredits: 2 },
      today: { successCredits: 12, failureCredits: 4 },
      month: { successCredits: 67, failureCredits: 10 },
    },
    dailyBuckets: stickyUserBuckets.noah,
  },
]

export const stickyUsersEmptyStoryData: StickyUserRow[] = []

export const stickyNodesStoryData: StickyNode[] = [
  {
    ...forwardProxyStoryStats.nodes[0],
    role: 'primary',
  },
  {
    ...forwardProxyStoryStats.nodes[1],
    role: 'secondary',
  },
]

export const stickyNodesReviewStoryData: StickyNode[] = [
  {
    ...forwardProxyStoryStats.nodes[0],
    key: 'sg-02-primary',
    displayName: '🇸🇬 新加坡 02 | 电信联通推荐',
    primaryAssignmentCount: 7,
    secondaryAssignmentCount: 2,
    stats: {
      ...forwardProxyStoryStats.nodes[0].stats,
      oneDay: {
        attempts: 1_433,
        successRate: 0.98,
        avgLatencyMs: 3_825,
      },
    },
    role: 'primary',
  },
  {
    ...forwardProxyStoryStats.nodes[1],
    key: 'us-sj-02-secondary',
    displayName: '🇺🇸 美国圣何塞 02 | 0.1 倍',
    primaryAssignmentCount: 1,
    secondaryAssignmentCount: 6,
    stats: {
      ...forwardProxyStoryStats.nodes[1].stats,
      oneDay: {
        attempts: 1_642,
        successRate: 0.98,
        avgLatencyMs: 2_489,
      },
    },
    role: 'secondary',
  },
]

export const stickyUsersStoryTotal = 27
export const stickyUsersStoryPerPage = 20

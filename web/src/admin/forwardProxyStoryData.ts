import type {
  ForwardProxyActivityBucket,
  ForwardProxySettings,
  ForwardProxyStatsResponse,
  ForwardProxyWeightBucket,
} from '../api'

const STORY_TIME_MS = Date.parse('2026-03-13T02:55:00Z')
const STORY_RANGE_START = '2026-03-12T00:00:00Z'
const STORY_RANGE_END = '2026-03-13T00:00:00Z'
const STORY_BUCKET_SECONDS = 3600
const STORY_BUCKET_MS = STORY_BUCKET_SECONDS * 1000
const STORY_BUCKET_COUNT = 24

export const forwardProxyStorySavedAt = STORY_TIME_MS

function createBucketTime(index: number): { bucketStart: string; bucketEnd: string } {
  const bucketStartMs = Date.parse(STORY_RANGE_START) + index * STORY_BUCKET_MS
  return {
    bucketStart: new Date(bucketStartMs).toISOString(),
    bucketEnd: new Date(bucketStartMs + STORY_BUCKET_MS).toISOString(),
  }
}

function buildActivityBuckets(series: Array<[number, number]>): ForwardProxyActivityBucket[] {
  return Array.from({ length: STORY_BUCKET_COUNT }, (_, index) => {
    const [successCount, failureCount] = series[index] ?? [0, 0]
    return {
      ...createBucketTime(index),
      successCount,
      failureCount,
    }
  })
}

function buildWeightBuckets(values: number[]): ForwardProxyWeightBucket[] {
  return Array.from({ length: STORY_BUCKET_COUNT }, (_, index) => {
    const lastWeight = values[index] ?? values[Math.max(0, values.length - 1)] ?? 0
    return {
      ...createBucketTime(index),
      sampleCount: 1,
      minWeight: lastWeight,
      maxWeight: lastWeight,
      avgWeight: lastWeight,
      lastWeight,
    }
  })
}

const TOKYO_ACTIVITY = buildActivityBuckets([
  [14, 1],
  [16, 0],
  [18, 1],
  [17, 0],
  [18, 0],
  [19, 1],
  [20, 0],
  [21, 0],
  [19, 1],
  [17, 0],
  [16, 0],
  [14, 1],
  [15, 0],
  [17, 0],
  [18, 0],
  [21, 0],
  [23, 0],
  [22, 1],
  [21, 0],
  [19, 0],
  [18, 0],
  [17, 1],
  [16, 0],
  [15, 0],
])

const TOKYO_WEIGHTS = buildWeightBuckets([
  0.82, 0.83, 0.84, 0.85, 0.84, 0.86, 0.88, 0.89, 0.88, 0.87, 0.86, 0.85,
  0.86, 0.88, 0.89, 0.9, 0.92, 0.93, 0.92, 0.91, 0.9, 0.89, 0.9, 0.91,
])

const FRANKFURT_ACTIVITY = buildActivityBuckets([
  [1, 4],
  [2, 5],
  [3, 4],
  [2, 4],
  [1, 5],
  [2, 3],
  [3, 2],
  [4, 2],
  [5, 1],
  [4, 2],
  [3, 3],
  [2, 4],
  [1, 5],
  [2, 4],
  [3, 3],
  [4, 2],
  [5, 1],
  [4, 1],
  [3, 2],
  [2, 3],
  [1, 4],
  [2, 5],
  [3, 4],
  [4, 3],
])

const FRANKFURT_WEIGHTS = buildWeightBuckets([
  0.62, 0.58, 0.54, 0.5, 0.46, 0.42, 0.38, 0.34, 0.3, 0.28, 0.31, 0.35,
  0.39, 0.43, 0.46, 0.49, 0.52, 0.55, 0.51, 0.47, 0.43, 0.4, 0.38, 0.37,
])

const DIRECT_ACTIVITY = buildActivityBuckets([
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [1, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [0, 0],
  [1, 0],
  [0, 0],
])

const DIRECT_WEIGHTS = buildWeightBuckets(Array.from({ length: STORY_BUCKET_COUNT }, () => 1))

export const forwardProxyStorySettings: ForwardProxySettings = {
  proxyUrls: ['http://127.0.0.1:8080', 'socks5h://127.0.0.1:1080', 'ss://demo-node'],
  subscriptionUrls: ['https://example.com/subscription.base64', 'https://mirror.example.com/proxy-feed.txt'],
  subscriptionUpdateIntervalSecs: 3600,
  insertDirect: true,
  nodes: [
    {
      key: 'node-tokyo-a',
      source: 'subscription',
      displayName: 'Tokyo-A',
      endpointUrl: 'socks5h://127.0.0.1:30001',
      resolvedIps: ['18.183.246.69'],
      resolvedRegions: ['JP Tokyo (13)'],
      weight: 0.91,
      available: true,
      lastError: null,
      penalized: false,
      primaryAssignmentCount: 3,
      secondaryAssignmentCount: 1,
      stats: {
        oneMinute: { attempts: 5, successRate: 1, avgLatencyMs: 121 },
        fifteenMinutes: { attempts: 22, successRate: 0.95, avgLatencyMs: 132 },
        oneHour: { attempts: 96, successRate: 0.94, avgLatencyMs: 141 },
        oneDay: { attempts: 1180, successRate: 0.93, avgLatencyMs: 155 },
        sevenDays: { attempts: 7420, successRate: 0.91, avgLatencyMs: 163 },
      },
    },
    {
      key: 'node-frankfurt-b',
      source: 'manual',
      displayName: 'Frankfurt-B',
      endpointUrl: 'http://127.0.0.1:8080',
      resolvedIps: ['1.1.1.1'],
      resolvedRegions: ['US Westfield (MA)'],
      weight: 0.37,
      available: false,
      lastError: 'proxy_timeout',
      penalized: true,
      primaryAssignmentCount: 1,
      secondaryAssignmentCount: 2,
      stats: {
        oneMinute: { attempts: 1, successRate: 0, avgLatencyMs: 820 },
        fifteenMinutes: { attempts: 8, successRate: 0.5, avgLatencyMs: 466 },
        oneHour: { attempts: 31, successRate: 0.58, avgLatencyMs: 338 },
        oneDay: { attempts: 202, successRate: 0.61, avgLatencyMs: 305 },
        sevenDays: { attempts: 1390, successRate: 0.67, avgLatencyMs: 284 },
      },
    },
    {
      key: 'direct',
      source: 'direct',
      displayName: 'Direct',
      endpointUrl: null,
      resolvedIps: [],
      resolvedRegions: [],
      weight: 1,
      available: true,
      lastError: null,
      penalized: false,
      primaryAssignmentCount: 0,
      secondaryAssignmentCount: 2,
      stats: {
        oneMinute: { attempts: 0, successRate: null, avgLatencyMs: null },
        fifteenMinutes: { attempts: 0, successRate: null, avgLatencyMs: null },
        oneHour: { attempts: 4, successRate: 1, avgLatencyMs: 210 },
        oneDay: { attempts: 10, successRate: 1, avgLatencyMs: 205 },
        sevenDays: { attempts: 12, successRate: 1, avgLatencyMs: 207 },
      },
    },
  ],
}

export const forwardProxyStoryStats: ForwardProxyStatsResponse = {
  rangeStart: STORY_RANGE_START,
  rangeEnd: STORY_RANGE_END,
  bucketSeconds: STORY_BUCKET_SECONDS,
  nodes: [
    {
      key: 'node-tokyo-a',
      source: 'subscription',
      displayName: 'Tokyo-A',
      endpointUrl: 'socks5h://127.0.0.1:30001',
      resolvedIps: ['18.183.246.69'],
      resolvedRegions: ['JP Tokyo (13)'],
      weight: 0.91,
      available: true,
      lastError: null,
      penalized: false,
      primaryAssignmentCount: 3,
      secondaryAssignmentCount: 1,
      stats: {
        oneMinute: { attempts: 5, successRate: 1, avgLatencyMs: 121 },
        fifteenMinutes: { attempts: 22, successRate: 0.95, avgLatencyMs: 132 },
        oneHour: { attempts: 96, successRate: 0.94, avgLatencyMs: 141 },
        oneDay: { attempts: 1180, successRate: 0.93, avgLatencyMs: 155 },
        sevenDays: { attempts: 7420, successRate: 0.91, avgLatencyMs: 163 },
      },
      last24h: TOKYO_ACTIVITY,
      weight24h: TOKYO_WEIGHTS,
    },
    {
      key: 'node-frankfurt-b',
      source: 'manual',
      displayName: 'Frankfurt-B',
      endpointUrl: 'http://127.0.0.1:8080',
      resolvedIps: ['1.1.1.1'],
      resolvedRegions: ['US Westfield (MA)'],
      weight: 0.37,
      available: false,
      lastError: 'proxy_timeout',
      penalized: true,
      primaryAssignmentCount: 1,
      secondaryAssignmentCount: 2,
      stats: {
        oneMinute: { attempts: 1, successRate: 0, avgLatencyMs: 820 },
        fifteenMinutes: { attempts: 8, successRate: 0.5, avgLatencyMs: 466 },
        oneHour: { attempts: 31, successRate: 0.58, avgLatencyMs: 338 },
        oneDay: { attempts: 202, successRate: 0.61, avgLatencyMs: 305 },
        sevenDays: { attempts: 1390, successRate: 0.67, avgLatencyMs: 284 },
      },
      last24h: FRANKFURT_ACTIVITY,
      weight24h: FRANKFURT_WEIGHTS,
    },
    {
      key: 'direct',
      source: 'direct',
      displayName: 'Direct',
      endpointUrl: null,
      resolvedIps: [],
      resolvedRegions: [],
      weight: 1,
      available: true,
      lastError: null,
      penalized: false,
      primaryAssignmentCount: 0,
      secondaryAssignmentCount: 2,
      stats: {
        oneMinute: { attempts: 0, successRate: null, avgLatencyMs: null },
        fifteenMinutes: { attempts: 0, successRate: null, avgLatencyMs: null },
        oneHour: { attempts: 4, successRate: 1, avgLatencyMs: 210 },
        oneDay: { attempts: 10, successRate: 1, avgLatencyMs: 205 },
        sevenDays: { attempts: 12, successRate: 1, avgLatencyMs: 207 },
      },
      last24h: DIRECT_ACTIVITY,
      weight24h: DIRECT_WEIGHTS,
    },
  ],
}

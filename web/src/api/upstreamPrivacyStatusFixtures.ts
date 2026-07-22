import type { UpstreamPrivacyStatus } from './systemSettingsTypes'

type ActivityDiagnostics = Pick<
  UpstreamPrivacyStatus,
  'retryBuckets' | 'currentPeriodBoundUsersByKey' | 'currentPeriodPendingProjectIdsByKey'
>

const demoActivityDiagnostics: ActivityDiagnostics = {
  retryBuckets: { upstream429: 1, localUsageRateLimit: 1, other: 0 },
  currentPeriodBoundUsersByKey: [
    { keyIdHint: 'key-primary', count: 12 },
    { keyIdHint: 'key-backup', count: 5 },
  ],
  currentPeriodPendingProjectIdsByKey: [
    { keyIdHint: 'key-primary', count: 28 },
    { keyIdHint: 'key-backup', count: 9 },
    { keyIdHint: 'key-cold', count: 3 },
  ],
}

const storyActivityDiagnostics: ActivityDiagnostics = {
  retryBuckets: { upstream429: 4, localUsageRateLimit: 2, other: 1 },
  currentPeriodBoundUsersByKey: [
    { keyIdHint: 'key-primary', count: 19 },
    { keyIdHint: 'key-backup', count: 8 },
    { keyIdHint: 'key-eu-west', count: 4 },
  ],
  currentPeriodPendingProjectIdsByKey: [
    { keyIdHint: 'key-primary', count: 48 },
    { keyIdHint: 'key-backup', count: 17 },
    { keyIdHint: 'key-eu-west', count: 7 },
    { keyIdHint: 'key-cold', count: 3 },
  ],
}

function createUpstreamPrivacyStatus(diagnostics: ActivityDiagnostics): UpstreamPrivacyStatus {
  return {
    phase: 'compare',
    configuredProjectIdMode: 'accessToken',
    effectiveProjectIdMode: 'accessToken',
    fixedProjectIdConfigured: false,
    configuredMcpUserAgent: '',
    effectiveMcpUserAgent: null,
    upstreamPreciseReconciliationEnabled: true,
    httpAllowedHeaders: ['accept', 'accept-encoding', 'content-type', 'x-project-id (policy injected)'],
    controlMcpAllowedHeaders: ['accept', 'cache-control', 'mcp-protocol-version', 'mcp-session-id', 'user-agent (configured only)'],
    gates: [
      { key: 'accessTokenMode', ready: true, detail: 'AccessToken' },
      { key: 'apiRebalance', ready: true, detail: 'enabled' },
      { key: 'mcpRebalance', ready: true, detail: 'enabled' },
      { key: 'controlSessionsDrained', ready: false, detail: '2' },
    ],
    completedGates: 3,
    totalGates: 4,
    activeUpstreamMcpSessions: 2,
    currentPeriodCode: '2026-07-14/S2',
    currentPeriodEndsAt: 1_783_994_400,
    nextEpochAt: null,
    pendingResearch: 1,
    queuedSettlements: 2,
    degradedSettlements: 0,
    lastReconciliationRunAt: 1_783_958_250,
    lastShadowAdjustmentAt: 1_783_958_100,
    lastReconciliationEnqueueErrorAt: 1_783_957_900,
    ...diagnostics,
    recentAdjustments: [
      {
        settlementKey: 'v1:tok_demo:2026-07-14/S1',
        tokenIdHint: 'tok_demo',
        billingSubjectKind: 'token',
        periodCode: '2026-07-14/S1',
        deltaCredits: -3,
        degradedReason: null,
        createdAt: 1_783_958_100,
      },
    ],
    generatedAt: 1_783_958_400,
  }
}

export function createDemoUpstreamPrivacyStatus(): UpstreamPrivacyStatus {
  return createUpstreamPrivacyStatus(demoActivityDiagnostics)
}

export const storyUpstreamPrivacyStatus = createUpstreamPrivacyStatus(storyActivityDiagnostics)

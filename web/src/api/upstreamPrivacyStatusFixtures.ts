import type { UpstreamPrivacyStatus } from './systemSettingsTypes'

type ActivityDiagnostics = Pick<
  UpstreamPrivacyStatus,
  | 'retryBuckets'
  | 'currentPeriodBoundUsersByKey'
  | 'currentPeriodPendingProjectIdsByKey'
  | 'dailyReconciliationProgress'
  | 'dailyReconciliationByKey'
>

const demoActivityDiagnostics: ActivityDiagnostics = {
  retryBuckets: { upstream429: 1, localUsageRateLimit: 1, missingEligibleUpstreamKey: 0, other: 0 },
  currentPeriodBoundUsersByKey: [
    { keyIdHint: 'key-primary', count: 12 },
    { keyIdHint: 'key-backup', count: 5 },
  ],
  currentPeriodPendingProjectIdsByKey: [
    { keyIdHint: 'key-primary', count: 28 },
    { keyIdHint: 'key-backup', count: 9 },
    { keyIdHint: 'key-cold', count: 3 },
  ],
  dailyReconciliationProgress: {
    observedAccounts: 12,
    accountsWithSettledPeriod: 5,
    fullyTerminalAccounts: 7,
    observedPeriods: 21,
    settledPeriods: 8,
    degradedPeriods: 1,
    pendingPeriods: 12,
    researchTotal: 18,
    researchTerminal: 7,
    researchPending: 11,
  },
  dailyReconciliationByKey: [
    { keyIdHint: 'key-primary', terminalResearch: 5, pendingResearch: 8, pendingProjectIds: 28, cooldownUntil: null, cooldownReason: null },
    { keyIdHint: 'key-backup', terminalResearch: 2, pendingResearch: 3, pendingProjectIds: 9, cooldownUntil: 1_783_960_000, cooldownReason: 'upstream429' },
  ],
}

const storyActivityDiagnostics: ActivityDiagnostics = {
  retryBuckets: { upstream429: 4, localUsageRateLimit: 2, missingEligibleUpstreamKey: 3, other: 1 },
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
  dailyReconciliationProgress: {
    observedAccounts: 31,
    accountsWithSettledPeriod: 8,
    fullyTerminalAccounts: 10,
    observedPeriods: 55,
    settledPeriods: 11,
    degradedPeriods: 3,
    pendingPeriods: 41,
    researchTotal: 49,
    researchTerminal: 13,
    researchPending: 36,
  },
  dailyReconciliationByKey: [
    { keyIdHint: 'key-primary', terminalResearch: 9, pendingResearch: 21, pendingProjectIds: 48, cooldownUntil: 1_783_960_000, cooldownReason: 'upstream429' },
    { keyIdHint: 'key-backup', terminalResearch: 4, pendingResearch: 9, pendingProjectIds: 17, cooldownUntil: null, cooldownReason: null },
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
    degradedSettlementsCapped: false,
    lastReconciliationRunAt: 1_783_958_250,
    lastShadowAdjustmentAt: 1_783_958_100,
    lastReconciliationEnqueueErrorAt: 1_783_957_900,
    lastResearchSweepAt: 1_783_958_320,
    lastResearchTerminalAt: 1_783_958_300,
    reconciliationLastDurationMs: 19_842,
    reconciliationLastAttempted: 20,
    reconciliationLastSettled: 0,
    reconciliationLastNoAdjustment: 0,
    reconciliationLastUpstream429: 16,
    reconciliationLastBudgetExhausted: true,
    reconciliationObservation: {
      observedAt: 1_783_958_320,
      coverage: 'bounded',
      queueEstimate: null,
      hasEligible: true,
      oldestCandidateAgeSecs: 3_600,
    },
    reconciliationLocalBackoff: {
      pressureStreak: 0,
      level: 0,
      availableAt: null,
      lastRecoveredAt: null,
    },
    reconciliationRunObservation: {
      mode: 'compare',
      projectionState: 'projecting',
      projectionScannedRows: 1_250,
      projectionBatchSize: 50,
      projectionTransactionP95Ms: 42,
      cursorAdvanced: true,
      hydrateMs: 84,
      firstRemoteMs: 312,
      remoteMs: 640,
      finalizationMs: 38,
      researchMs: 0,
      settled: 0,
      noAdjustment: 0,
      observed: 1,
      upstream429: 0,
      transportFailure: 0,
      semanticFailure: 0,
      localPressure: 0,
      partialKeyObservations: 2,
      multiKeyPending: 1,
      remoteAttemptBudgetDefers: 1,
      resumedRuns: 1,
      terminalRuns: 1,
      lastTransportKind: null,
      continuationReason: 'observed',
      nextRetryAt: null,
      observedAt: 1_783_958_320,
    },
    reconciliationResearchProgressWindow: {
      windowStartedAt: 1_783_957_720,
      windowEndedAt: 1_783_958_320,
      windowSeconds: 600,
      terminalDelta: 2,
      pendingDelta: -1,
      unavailableDelta: 0,
      pollablePendingDelta: -1,
      terminalRatePositive: true,
      pollResolutionRatePositive: true,
      pendingNonGrowing: true,
      complete: true,
    },
    reconciliationResearchPollDiagnostics: {
      unavailable: 0,
      pollablePending: 11,
      credentialsCoolingKeys: 0,
      earliestCredentialsRetryAt: null,
      lastPollOutcome: 'pending',
      lastPollObservedAt: 1_783_958_320,
    },
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

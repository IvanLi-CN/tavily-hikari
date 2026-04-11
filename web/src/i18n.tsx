import { createContext, ReactNode, useContext, useMemo, useState } from 'react'

export type Language = 'en' | 'zh'

const LANGUAGE_STORAGE_KEY = 'tavily-hikari-language'
const DEFAULT_LANGUAGE: Language = 'en'

interface LanguageContextValue {
  language: Language
  setLanguage: (language: Language) => void
}

interface PublicTranslations {
  updateBanner: {
    title: string
    description: (current: string, latest: string) => string
    refresh: string
    dismiss: string
  }
  heroTitle: string
  heroTagline: string
  heroDescription: string
  metrics: {
    monthly: { title: string; subtitle: string }
    daily: { title: string; subtitle: string }
    pool: { title: string; subtitle: string }
  }
  adminButton: string
  adminLoginButton: string
  linuxDoLogin: {
    button: string
    logoAlt: string
  }
  registrationPaused: {
    badge: string
    title: string
    description: string
    returnHome: string
    continueHint: string
  }
  registrationPausedNotice: {
    title: string
    description: string
  }
  adminLogin: {
    title: string
    description: string
    password: {
      label: string
      placeholder: string
    }
    submit: {
      label: string
      loading: string
    }
    backHome: string
    hints: {
      checking: string
      disabled: string
    }
    errors: {
      invalid: string
      disabled: string
      generic: string
    }
  }
  accessPanel: {
    title: string
    stats: {
      dailySuccess: string
      dailyFailure: string
      monthlySuccess: string
      hourlyLimit: string
      dailyLimit: string
      monthlyLimit: string
    }
  }
  accessToken: {
    label: string
    placeholder: string
    toggle: {
      show: string
      hide: string
      iconAlt: string
    }
  }
  copyToken: {
    iconAlt: string
    copy: string
    copied: string
    error: string
  }
  tokenAccess: {
    button: string
    dialog: {
      title: string
      description: string
      actions: {
        cancel: string
        confirm: string
      }
      loginHint: string
    }
  }
  guide: {
    title: string
    dataSourceLabel: string
    tokenVisibility: {
      show: string
      hide: string
    }
    tabs: Record<string, string>
  }
  footer: {
    version: string
  }
  errors: {
    metrics: string
    summary: string
  }
  logs: {
    title: string
    description: string
    empty: {
      noToken: string
      hint: string
      loading: string
      none: string
    }
    table: {
      time: string
      httpStatus: string
      mcpStatus: string
      result: string
    }
    toggles: {
      show: string
      hide: string
    }
  }
  cherryMock: {
    title: string
    windowTitle: string
    sidebar: {
      modelService: string
      defaultModel: string
      generalSettings: string
      displaySettings: string
      dataSettings: string
      mcp: string
      notes: string
      webSearch: string
      memory: string
      apiServer: string
      docProcessing: string
      quickPhrases: string
      shortcuts: string
    }
    providerCard: {
      title: string
      subtitle: string
      providerValue: string
    }
    tavilyCard: {
      title: string
      apiKeyLabel: string
      apiKeyPlaceholder: string
      apiKeyHint: string
      testButtonLabel: string
      apiUrlLabel: string
      apiUrlHint: string
    }
    generalCard: {
      title: string
      includeDateLabel: string
      resultsCountLabel: string
    }
  }
}

interface AdminTranslationsShape {
  header: {
    title: string
    subtitle: string
    updatedPrefix: string
    refreshNow: string
    refreshing: string
    returnToConsole: string
  }
  loadingStates: {
    switching: string
    refreshing: string
    error: string
  }
  nav: {
    dashboard: string
    usage: string
    tokens: string
    keys: string
    requests: string
    jobs: string
    users: string
    alerts: string
    systemSettings: string
    proxySettings: string
  }
  dashboard: {
    title: string
    description: string
    loading: string
    summaryUnavailable: string
    statusUnavailable: string
    todayTitle: string
    todayDescription: string
    monthTitle: string
    monthDescription: string
    currentStatusTitle: string
    currentStatusDescription: string
    deltaFromYesterday: string
    deltaNoBaseline: string
    percentagePointUnit: string
    asOfNow: string
    currentSnapshot: string
    todayShare: string
    todayAdded: string
    monthToDate: string
    monthAdded: string
    monthShare: string
    quotaChargeTitle: string
    quotaChargeLocalEstimate: string
    quotaChargeUpstreamActual: string
    quotaChargeDelta: string
    quotaChargeSampledKeys: string
    quotaChargeStaleKeys: string
    quotaChargeLatestSync: string
    quotaChargeNoSync: string
    valuableTag: string
    otherTag: string
    unknownTag: string
    upstreamExhaustedLabel: string
    trendsTitle: string
    trendsDescription: string
    requestTrend: string
    errorTrend: string
    chartModeResults: string
    chartModeTypes: string
    chartModeResultsDelta: string
    chartModeTypesDelta: string
    chartVisibleSeries: string
    chartDeltaSeries: string
    chartSelectionAll: string
    chartEmpty: string
    chartUtcWindow: string
    chartResultSecondarySuccess: string
    chartResultPrimarySuccess: string
    chartResultSecondaryFailure: string
    chartResultPrimaryFailure429: string
    chartResultPrimaryFailureOther: string
    chartResultUnknown: string
    chartTypeMcpNonBillable: string
    chartTypeMcpBillable: string
    chartTypeApiNonBillable: string
    chartTypeApiBillable: string
    riskTitle: string
    riskDescription: string
    riskEmpty: string
    actionsTitle: string
    actionsDescription: string
    recentRequests: string
    recentJobs: string
    openModule: string
    openToken: string
    openKey: string
    disabledTokenRisk: string
    exhaustedKeyRisk: string
    failedJobRisk: string
    tokenCoverageTruncated: string
    tokenCoverageError: string
  }
  modules: {
    comingSoon: string
    users: {
      title: string
      description: string
      sections: {
        list: string
        roles: string
        status: string
      }
    }
    alerts: {
      title: string
      description: string
      sections: {
        rules: string
        thresholds: string
        channels: string
      }
    }
    proxySettings: {
      title: string
      description: string
      sections: {
        upstream: string
        routing: string
        rateLimit: string
      }
    }
  }
  proxySettings: {
    title: string
    description: string
    actions: {
      refresh: string
      save: string
      saving: string
      validateSubscriptions: string
      validatingSubscriptions: string
      validateManual: string
      validatingManual: string
    }
    summary: {
      configuredNodes: string
      configuredNodesHint: string
      readyNodes: string
      readyNodesHint: string
      penalizedNodes: string
      penalizedNodesHint: string
      subscriptions: string
      subscriptionsHint: string
      manualNodes: string
      manualNodesHint: string
      assignmentSpread: string
      assignmentSpreadHint: string
      range: string
      savedAt: string
    }
    config: {
      title: string
      description: string
      loading: string
      addSubscription: string
      addManual: string
      subscriptionCount: string
      manualCount: string
      subscriptionsTitle: string
      subscriptionsDescription: string
      subscriptionsPlaceholder: string
      subscriptionListEmpty: string
      subscriptionItemFallback: string
      manualTitle: string
      manualDescription: string
      manualPlaceholder: string
      manualListEmpty: string
      manualItemFallback: string
      egressTitle: string
      egressDescription: string
      egressEnabled: string
      egressDisabled: string
      egressUrlLabel: string
      egressUrlPlaceholder: string
      egressUrlHint: string
      egressLockedHint: string
      egressRequiredError: string
      egressErrorTitle: string
      egressInvalidUrlError: string
      egressUnknownError: string
      egressSwitchLabel: string
      egressSwitchHint: string
      egressApply: string
      egressApplying: string
      subscriptionIntervalLabel: string
      subscriptionIntervalHint: string
      invalidInterval: string
      insertDirectLabel: string
      insertDirectHint: string
      subscriptionDialogTitle: string
      subscriptionDialogDescription: string
      subscriptionDialogInputLabel: string
      manualDialogTitle: string
      manualDialogDescription: string
      manualDialogInputLabel: string
      validate: string
      validating: string
      add: string
      addedToList: string
      importAvailable: string
      importInput: string
      cancel: string
      remove: string
      resultNode: string
      resultNetwork: string
      resultStatus: string
      resultLatency: string
      resultAction: string
      resultDetails: string
      closeDetails: string
      saveFailed: string
    }
    validation: {
      title: string
      description: string
      empty: string
      emptySubscriptions: string
      emptyManual: string
      ok: string
      failed: string
      proxyKind: string
      subscriptionKind: string
      discoveredNodes: string
      latency: string
      requestFailed: string
      timeout: string
      unreachable: string
      xrayMissing: string
      subscriptionInvalid: string
      subscriptionUnreachable: string
      subscriptionTimedOut: string
      subscriptionNoNodes: string
      subscriptionUnsupportedNodes: string
      cancelled: string
      validationFailed: string
    }
    progress: {
      titleValidate: string
      titleSave: string
      titleRevalidate: string
      badgeValidate: string
      badgeSave: string
      badgeRevalidate: string
      buttonValidatingSubscription: string
      buttonValidatingManual: string
      buttonAddingSubscription: string
      buttonAddingManual: string
      running: string
      waiting: string
      done: string
      failed: string
      stepCounter: string
      steps: Record<
        | 'save_settings'
        | 'validate_egress_socks5'
        | 'apply_egress_socks5'
        | 'refresh_subscription'
        | 'bootstrap_probe'
        | 'normalize_input'
        | 'parse_input'
        | 'fetch_subscription'
        | 'probe_nodes'
        | 'generate_result'
        | 'refresh_ui',
        string
      >
    }
    nodes: {
      title: string
      description: string
      loading: string
      empty: string
      table: {
        node: string
        source: string
        endpoint: string
        state: string
        assignments: string
        windows: string
        activity24h: string
        weight24h: string
      }
      weightLabel: string
      primary: string
      secondary: string
      successRateLabel: string
      latencyLabel: string
      successCountLabel: string
      failureCountLabel: string
      lastWeightLabel: string
      avgWeightLabel: string
      minMaxWeightLabel: string
    }
    states: {
      ready: string
      readyHint: string
      penalized: string
      penalizedHint: string
      direct: string
      timeout: string
      timeoutHint: string
      unreachable: string
      unreachableHint: string
      unavailable: string
      unavailableHint: string
      xrayMissing: string
      xrayMissingHint: string
    }
    sources: {
      manual: string
      subscription: string
      direct: string
      unknown: string
    }
    windows: {
      oneMinute: string
      fifteenMinutes: string
      oneHour: string
      oneDay: string
      sevenDays: string
    }
  }
  systemSettings: {
    title: string
    description: string
    helpLabel: string
    form: {
      title: string
      description: string
      countLabel: string
      countHint: string
      currentValue: string
      applyScopeHint: string
      invalidCount: string
      saveFailed: string
    }
    actions: {
      apply: string
      applying: string
    }
  }
  users: {
    title: string
    description: string
    registration: {
      title: string
      description: string
      enabled: string
      disabled: string
      unavailable: string
      saving: string
      loadFailed: string
      saveFailed: string
    }
    searchPlaceholder: string
    search: string
    clear: string
    pagination: string
    table: {
      user: string
      displayName: string
      username: string
      status: string
      tokenCount: string
      tags: string
      hourlyAny: string
      hourly: string
      daily: string
      monthly: string
      successDaily: string
      successMonthly: string
      lastActivity: string
      lastLogin: string
      actions: string
    }
    status: {
      active: string
      inactive: string
      enabled: string
      disabled: string
      unknown: string
    }
    actions: {
      view: string
    }
    usage: {
      title: string
      description: string
      open: string
      back: string
      table: {
        user: string
        status: string
        hourlyAny: string
        hourly: string
        daily: string
        monthly: string
        monthlyBroken: string
        dailySuccessRate: string
        monthlySuccessRate: string
        lastUsed: string
      }
    }
    empty: {
      loading: string
      none: string
      notFound: string
      noTokens: string
    }
    detail: {
      title: string
      subtitle: string
      back: string
      userId: string
      identityTitle: string
      identityDescription: string
      tokensTitle: string
      tokensDescription: string
    }
    brokenKeys: {
      limitTitle: string
      limitDescription: string
      limitField: string
      hint: string
      save: string
      saving: string
      savedAt: string
      invalid: string
      saveFailed: string
      openAction: string
      openDetails: string
      drawerTitle: string
      drawerDescription: string
      loading: string
      empty: string
      noReason: string
      noRelatedUsers: string
      breakerSystem: string
      breakerUnknown: string
      table: {
        key: string
        status: string
        reason: string
        latestBreakAt: string
        breaker: string
        relatedUsers: string
      }
      actions: {
        copyKeyId: string
        copied: string
      }
    }
    quota: {
      title: string
      description: string
      hourlyAny: string
      hourly: string
      daily: string
      monthly: string
      hint: string
      save: string
      saving: string
      savedAt: string
      invalid: string
      saveFailed: string
      inheritsDefaults: string
      customized: string
    }
    catalog: {
      title: string
      description: string
      summaryTitle: string
      summaryDescription: string
      summaryEmpty: string
      summaryAccounts: string
      loading: string
      empty: string
      invalid: string
      loadFailed: string
      saveFailed: string
      deleteFailed: string
      formCreateTitle: string
      formEditTitle: string
      formDescription: string
      systemReadonly: string
      iconPlaceholder: string
      iconHint: string
      scopeSystem: string
      scopeSystemShort: string
      scopeCustom: string
      blockShort: string
      blockDescription: string
      deleteConfirm: string
      deleteDialogTitle: string
      deleteDialogCancel: string
      deleteDialogConfirm: string
      backToUsers: string
      backToList: string
      tagNotFound: string
      columns: {
        tag: string
        scope: string
        effect: string
        delta: string
        users: string
        actions: string
      }
      fields: {
        name: string
        displayName: string
        icon: string
        effect: string
        hourlyAny: string
        hourly: string
        daily: string
        monthly: string
      }
      effectKinds: {
        quotaDelta: string
        blockAll: string
      }
      actions: {
        create: string
        save: string
        saving: string
        cancelEdit: string
        edit: string
        delete: string
      }
    }
    userTags: {
      title: string
      description: string
      empty: string
      bindPlaceholder: string
      bindAction: string
      binding: string
      unbindAction: string
      bindFailed: string
      unbindFailed: string
      readOnly: string
      sourceSystem: string
      sourceManual: string
      manageCatalog: string
    }
    effectiveQuota: {
      title: string
      description: string
      blockAllNotice: string
      baseLabel: string
      effectiveLabel: string
      columns: {
        item: string
        source: string
        effect: string
      }
    }
    tokens: {
      table: {
        id: string
        note: string
        status: string
        hourlyAny: string
        hourly: string
        daily: string
        monthly: string
        successDaily: string
        successMonthly: string
        lastUsed: string
        actions: string
      }
      actions: {
        view: string
      }
    }
  }
  accessibility: {
    skipToContent: string
  }
  tokens: {
    title: string
    description: string
    notePlaceholder: string
    newToken: string
    creating: string
    batchCreate: string
    pagination: {
      prev: string
      next: string
      page: string
    }
    table: {
      id: string
      note: string
      owner: string
      usage: string
      quota: string
      lastUsed: string
      actions: string
    }
    empty: {
      loading: string
      none: string
    }
    owner: {
      label: string
      unbound: string
    }
    actions: {
      copy: string
      share: string
      disable: string
      enable: string
      edit: string
      delete: string
      viewLeaderboard: string
    }
    statusBadges: {
      disabled: string
    }
    quotaStates: Record<'normal' | 'hour' | 'day' | 'month', string>
    dialogs: {
      delete: {
        title: string
        description: string
        cancel: string
        confirm: string
      }
      note: {
        title: string
        placeholder: string
        cancel: string
        confirm: string
        saving: string
      }
    }
    batchDialog: {
      title: string
      groupPlaceholder: string
      confirm: string
      creating: string
      cancel: string
      done: string
      createdN: string
      copyAll: string
    }
    groups: {
      label: string
      all: string
      ungrouped: string
      moreShow: string
      moreHide: string
    }
  }
  unboundTokenUsage: {
    title: string
    description: string
    searchPlaceholder: string
    error: string
    table: {
      identity: string
      status: string
      hourlyAny: string
      hourly: string
      daily: string
      monthly: string
      monthlyBroken: string
      dailySuccessRate: string
      monthlySuccessRate: string
      lastUsed: string
    }
    empty: {
      loading: string
      none: string
    }
    back: string
  }
    metrics: {
      labels: {
        total: string
        success: string
        failure: string
        errors: string
        quota: string
        unknownCalls: string
        newKeys: string
        newQuarantines: string
        keys: string
        quarantined: string
        exhausted: string
        remaining: string
        proxyAvailable: string
        proxyTotal: string
      }
      subtitles: {
        keysAll: string
        keysExhausted: string
        keysAvailability: string
      }
    loading: string
  }
  keys: {
    title: string
    description: string
    placeholder: string
    addButton: string
    adding: string
    batch: {
      placeholder: string
      groupPlaceholder: string
      hint: string
      count: string
      report: {
        title: string
        close: string
        summary: {
          inputLines: string
          validLines: string
          uniqueInInput: string
          created: string
          undeleted: string
          existed: string
          duplicateInInput: string
          failed: string
        }
        failures: {
          title: string
          none: string
          table: {
            apiKey: string
            error: string
          }
        }
      }
    }
    validation: {
      title: string
      hint: string
      registrationIpBadge: string
      registrationIpTooltip: string
      actions: {
        close: string
        retry: string
        retryFailed: string
        import: string
        importValid: string
        imported: string
      }
      import: {
        title: string
        exhaustedMarkFailed: string
      }
      summary: {
        group: string
        inputLines: string
        validLines: string
        uniqueInInput: string
        duplicateInInput: string
        checked: string
        ok: string
        exhausted: string
        exhaustedNote: string
        invalid: string
        error: string
      }
      emptyFiltered: string
      table: {
        apiKey: string
        result: string
        quota: string
        actions: string
      }
      statuses: {
        pending: string
        duplicate_in_input: string
        ok: string
        ok_exhausted: string
        unauthorized: string
        forbidden: string
        invalid: string
        error: string
      }
    }
    groups: {
      label: string
      all: string
      ungrouped: string
      moreShow: string
      moreHide: string
    }
    filters: {
      status: string
      region: string
      registrationIp: string
      registrationIpPlaceholder: string
      clearGroups: string
      clearStatuses: string
      clearRegistrationIp: string
      clearRegions: string
      selectedSuffix: string
    }
    selection: {
      selectRow: string
      selectAll: string
      selectCurrentPage: string
      clear: string
      selectedCount: string
    }
    pagination: {
      page: string
      perPage: string
    }
    table: {
      keyId: string
      status: string
      total: string
      success: string
      errors: string
      quota: string
      successRate: string
      remainingPct: string
      quotaLeft: string
      registration: string
      registrationIp: string
      registrationRegion: string
      assignedProxy: string
      syncedAt: string
      lastUsed: string
      statusChanged: string
      actions: string
    }
    empty: {
      loading: string
      none: string
      filtered: string
    }
    actions: {
      copy: string
      enable: string
      disable: string
      clearQuarantine: string
      delete: string
      details: string
    }
    bulkActions: {
      syncUsage: string
      clearQuarantine: string
      delete: string
      running: string
      summary: string
    }
    bulkSyncProgress: {
      title: string
      running: string
      finished: string
      refreshingList: string
      steps: {
        prepareRequest: string
        syncUsage: string
        refreshUi: string
      }
      status: {
        waiting: string
        running: string
        done: string
        failed: string
      }
      counters: {
        progress: string
        success: string
        skipped: string
        failed: string
      }
      lastResultLabel: string
      result: {
        success: string
        skipped: string
        failed: string
        noDetail: string
      }
    }
    quarantine: {
      badge: string
      sourcePrefix: string
      noReason: string
    }
    dialogs: {
      disable: {
        title: string
        description: string
        cancel: string
        confirm: string
      }
      delete: {
        title: string
        description: string
        cancel: string
        confirm: string
      }
      bulkDelete: {
        title: string
        description: string
        cancel: string
        confirm: string
      }
    }
  }
  jobs: {
    title: string
    description: string
    filters: {
      all: string
      quota: string
      usage: string
      logs: string
      geo: string
      linuxdo: string
    }
    empty: {
      loading: string
      none: string
    }
    table: {
      id: string
      type: string
      key: string
      status: string
      attempt: string
      started: string
      message: string
    }
    toggles: {
      show: string
      hide: string
    }
    types?: Record<string, string>
  }
  logs: {
    title: string
    description: string
    descriptionFallback: string
    descriptionWithRetention: string
    filters: {
      all: string
      success: string
      error: string
      quota: string
      requestType: string
      requestTypeAll: string
      requestTypeEmpty: string
      billingGroup: string
      protocolGroup: string
      resultOrEffect: string
      resultOrEffectAll: string
      resultGroup: string
      keyEffectGroup: string
      tokenAll: string
      keyAll: string
      noFacetOptions: string
    }
    empty: {
      loading: string
      none: string
    }
    table: {
      time: string
      key: string
      token: string
      requestType: string
      status: string
      chargedCredits: string
      httpStatus: string
      mcpStatus: string
      result: string
      keyEffect: string
      error: string
    }
    toggles: {
      show: string
      hide: string
    }
    pagination: {
      summary: string
      summaryWithRetention: string
      newer: string
      older: string
    }
    errors: {
      quotaExhausted: string
      quotaExhaustedHttp: string
      requestFailedHttpMcp: string
      requestFailedHttp: string
      requestFailedMcp: string
      requestFailedGeneric: string
      httpStatus: string
      none: string
    }
    keyEffects: {
      none: string
      quarantined: string
      markedExhausted: string
      restoredActive: string
      clearedQuarantine: string
      unknown: string
    }
  }
  statuses: Record<string, string>
  logDetails: {
    request: string
    response: string
    outcome: string
    keyEffect: string
    requestTypeDetail: string
    solution: string
    requestBody: string
    responseBody: string
    noBody: string
    loadingBody: string
    loadBodyFailed: string
    retryLoadBody: string
    noKeyEffect: string
    forwardedHeaders: string
    droppedHeaders: string
  }
  keyDetails: {
    title: string
    descriptionPrefix: string
    back: string
    syncAction: string
    syncing: string
    syncSuccess: string
    usageTitle: string
    usageDescription: string
    periodOptions: {
      day: string
      week: string
      month: string
    }
    apply: string
    loading: string
    metrics: {
      total: string
      success: string
      errors: string
      quota: string
      lastActivityPrefix: string
      noActivity: string
    }
    quarantine: {
      title: string
      description: string
      source: string
      reason: string
      detail: string
      showDetail: string
      hideDetail: string
      createdAt: string
      clearAction: string
      clearing: string
    }
    metadata: {
      title: string
      description: string
      group: string
      registrationIp: string
      registrationRegion: string
    }
    stickyUsers: {
      title: string
      description: string
      empty: string
      user: string
      yesterday: string
      today: string
      month: string
      trend: string
      lastSuccess: string
      success: string
      failure: string
      inactive: string
    }
    stickyNodes: {
      title: string
      description: string
      empty: string
      role: string
      node: string
      activity: string
      weight: string
      primary: string
      secondary: string
      assignmentSummary: string
      primaryAssignments: string
      secondaryAssignments: string
      window: string
    }
    logsTitle: string
    logsDescription: string
    logsEmpty: string
  }
  errors: {
      copyKey: string
      addKey: string
      addKeysBatch: string
      createToken: string
      copyToken: string
      toggleToken: string
      deleteToken: string
      updateTokenNote: string
      deleteKey: string
      toggleKey: string
      clearQuarantine: string
      loadKeyDetails: string
      syncUsage: string
    }
  footer: {
    title: string
    githubAria: string
    githubLabel: string
    loadingVersion: string
    tagPrefix: string
  }
}

interface TranslationShape {
  common: {
    languageLabel: string
    englishLabel: string
    chineseLabel: string
  }
  public: PublicTranslations
  admin: AdminTranslationsShape
}

const LanguageContext = createContext<LanguageContextValue | undefined>(undefined)

function readStoredLanguage(): Language | null {
  if (typeof window === 'undefined') return null
  const stored = window.localStorage.getItem(LANGUAGE_STORAGE_KEY)
  if (stored === 'en' || stored === 'zh') return stored
  return null
}

function detectBrowserLanguage(): Language | null {
  if (typeof navigator === 'undefined') return null
  const preferred = Array.isArray(navigator.languages) ? navigator.languages : []
  const fallbacks = typeof navigator.language === 'string' ? [navigator.language] : []
  const candidates = [...preferred, ...fallbacks]

  for (const locale of candidates) {
    const normalized = locale?.toLowerCase()
    if (!normalized) continue
    if (normalized.startsWith('zh')) return 'zh'
    if (normalized.startsWith('en')) return 'en'
  }

  return null
}

function persistLanguage(language: Language): void {
  if (typeof window === 'undefined') return
  window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language)
}

export const translations: Record<Language, TranslationShape> = {
  en: {
    common: {
      languageLabel: 'Language',
      englishLabel: 'English',
      chineseLabel: '中文',
    },
    public: {
      updateBanner: {
        title: 'New update available',
        description: (current, latest) => `Current ${current} → Latest ${latest}`,
        refresh: 'Reload now',
        dismiss: 'Remind me later',
      },
      heroTitle: 'Tavily Hikari Proxy',
      heroTagline: 'Transparent request visibility for your Tavily integration.',
      heroDescription:
        'Tavily Hikari pools multiple Tavily API Keys into a single endpoint, balances usage across them, and ships with request auditing, rate monitoring, and shareable access tokens.',
      metrics: {
        monthly: {
          title: 'Monthly Success (UTC)',
          subtitle: 'Tavily quotas reset at the start of every UTC month',
        },
        daily: {
          title: 'Today',
          subtitle: 'Successful requests in today’s browser-local window',
        },
        pool: {
          title: 'Key Pool Status',
          subtitle: 'Active Tavily keys / total keys (including exhausted)',
        },
      },
      adminButton: 'Open Admin Dashboard',
      adminLoginButton: 'Admin Login',
      linuxDoLogin: {
        button: 'Sign in with Linux DO',
        logoAlt: 'Linux DO logo',
      },
      registrationPaused: {
        badge: 'Registration paused',
        title: 'New registrations are temporarily paused',
        description:
          'This service is currently accepting sign-ins from already registered users only. New Linux DO accounts cannot be created right now.',
        returnHome: 'Return to home',
        continueHint: 'If you already have an account, go back to the home page and continue signing in there.',
      },
      registrationPausedNotice: {
        title: 'New registration is paused',
        description: 'Existing users can still sign in with Linux DO. New accounts are temporarily blocked.',
      },
      adminLogin: {
        title: 'Admin Login',
        description: 'Sign in to manage Tavily keys and access tokens.',
        password: {
          label: 'Admin Password',
          placeholder: 'Enter admin password',
        },
        submit: {
          label: 'Sign in',
          loading: 'Signing in…',
        },
        backHome: 'Back to home',
        hints: {
          checking: 'Checking session…',
          disabled: 'Built-in admin login is disabled on this server.',
        },
        errors: {
          invalid: 'Invalid password.',
          disabled: 'Built-in admin login is disabled on this server.',
          generic: 'Login failed.',
        },
      },
      accessPanel: {
        title: 'Token Usage',
        stats: {
          dailySuccess: 'Daily Success',
          dailyFailure: 'Daily Failure',
          monthlySuccess: 'Monthly Success (UTC)',
          hourlyLimit: 'Hourly Limit',
          dailyLimit: 'Daily Limit',
          monthlyLimit: 'Monthly Limit',
        },
      },
      accessToken: {
        label: 'Access Token',
        placeholder: 'th-xxxx-xxxxxxxxxxxx',
        toggle: {
          show: 'Show access token',
          hide: 'Hide access token',
          iconAlt: 'Toggle token visibility',
        },
      },
      copyToken: {
        iconAlt: 'Copy token',
        copy: 'Copy Token',
        copied: 'Copied',
        error: 'Copy failed',
      },
      tokenAccess: {
        button: 'Use Access Token',
        dialog: {
          title: 'Use Access Token',
          description: 'Enter an access token to view usage and recent requests.',
          actions: {
            cancel: 'Cancel',
            confirm: 'Continue',
          },
          loginHint: 'Tip: Sign in via linux.do to bind your account.',
        },
      },
      guide: {
        title: 'Connect Tavily Hikari to common clients',
        dataSourceLabel: 'Reference: ',
        tokenVisibility: {
          show: 'Show token',
          hide: 'Hide token',
        },
        tabs: {
          codex: 'Codex CLI',
          claude: 'Claude Code CLI',
          vscode: 'VS Code / Copilot',
          claudeDesktop: 'Claude Desktop',
          cursor: 'Cursor',
          windsurf: 'Windsurf',
          cherryStudio: 'Cherry Studio',
          other: 'Other',
        },
      },
      footer: {
        version: 'Current version: ',
      },
      errors: {
        metrics: 'Unable to load metrics right now',
        summary: 'Unable to load summary data',
      },
      logs: {
        title: 'Recent Requests (last 20)',
        description: 'Requires a valid access token to view token-specific activity.',
        empty: {
          noToken: 'Provide a valid access token to view the latest 20 requests for this token.',
          hint: 'Use a link with the full token in the hash, or enter a valid token above.',
          loading: 'Loading recent requests…',
          none: 'No recent requests for this token.',
        },
        table: {
          time: 'Time',
          httpStatus: 'HTTP',
          mcpStatus: 'Tavily',
          result: 'Result',
        },
        toggles: {
          show: 'Show details',
          hide: 'Hide details',
        },
      },
      cherryMock: {
        title: 'Cherry Studio settings preview',
        windowTitle: 'Settings',
        sidebar: {
          modelService: 'Model services',
          defaultModel: 'Default model',
          generalSettings: 'General',
          displaySettings: 'Display',
          dataSettings: 'Data',
          mcp: 'MCP',
          notes: 'Notes',
          webSearch: 'Web Search',
          memory: 'Global memory',
          apiServer: 'API server',
          docProcessing: 'Document processing',
          quickPhrases: 'Quick phrases',
          shortcuts: 'Shortcuts',
        },
        providerCard: {
          title: 'Web Search',
          subtitle: 'Search provider',
          providerValue: 'Tavily (API key)',
        },
        tavilyCard: {
          title: 'Tavily',
          apiKeyLabel: 'API key',
          apiKeyPlaceholder: 'th-xxxx-xxxxxxxxxxxx',
          apiKeyHint: 'Use your Tavily Hikari access token as the API key here.',
          testButtonLabel: 'Test',
          apiUrlLabel: 'API URL',
          apiUrlHint: 'Use this as the API URL in Cherry Studio.',
        },
        generalCard: {
          title: 'General settings',
          includeDateLabel: 'Include date in search',
          resultsCountLabel: 'Number of results',
        },
      },
    },
    admin: {
      header: {
        title: 'Tavily Hikari Overview',
        subtitle: 'Monitor API key allocation, quota health, and recent proxy activity.',
        updatedPrefix: 'Updated',
        refreshNow: 'Refresh Now',
        refreshing: 'Refreshing…',
        returnToConsole: 'Back to User Console',
      },
      loadingStates: {
        switching: 'Updating results…',
        refreshing: 'Refreshing current results…',
        error: 'Failed to load the current results.',
      },
      nav: {
        dashboard: 'Dashboard',
        usage: 'Usage',
        tokens: 'Tokens',
        keys: 'API Keys',
        requests: 'Requests',
        jobs: 'Jobs',
        users: 'Users',
        alerts: 'Alerts',
        systemSettings: 'System Settings',
        proxySettings: 'Proxy Settings',
      },
      dashboard: {
        title: 'Operations Dashboard',
        description: 'Global health, risk signals, and actionable activity in one place.',
        loading: 'Loading dashboard data…',
        summaryUnavailable: 'Unable to load the summary windows right now.',
        statusUnavailable: 'Unable to load the current site status right now.',
        todayTitle: 'Today',
        todayDescription: 'Core request signals up to now, compared with the same time yesterday.',
        monthTitle: 'This Month',
        monthDescription: 'Month-to-date request totals in one compact view.',
        currentStatusTitle: 'Current Site Status',
        currentStatusDescription: 'Live quota, active keys, and pool health right now.',
        deltaFromYesterday: 'vs same time yesterday',
        deltaNoBaseline: 'No yesterday baseline',
        percentagePointUnit: 'pp',
        asOfNow: 'Up to now',
        currentSnapshot: 'Current snapshot',
        todayShare: 'Today share',
        todayAdded: 'Added today',
        monthToDate: 'Month to date',
        monthAdded: 'Added this month',
        monthShare: 'Month share',
        quotaChargeTitle: 'Quota Charges',
        quotaChargeLocalEstimate: 'Local estimate',
        quotaChargeUpstreamActual: 'Upstream actual',
        quotaChargeDelta: 'Delta',
        quotaChargeSampledKeys: 'Sampled',
        quotaChargeStaleKeys: 'Stale',
        quotaChargeLatestSync: 'Latest sync',
        quotaChargeNoSync: 'No sync data',
        valuableTag: 'Primary',
        otherTag: 'Secondary',
        unknownTag: 'Unknown',
        upstreamExhaustedLabel: 'Upstream Keys Exhausted',
        trendsTitle: 'Traffic Trends',
        trendsDescription: 'The window still uses the latest 25 closed UTC hour buckets, while the x-axis is shown in local time.',
        requestTrend: 'Request volume',
        errorTrend: 'Error volume',
        chartModeResults: 'Results',
        chartModeTypes: 'Types',
        chartModeResultsDelta: 'Δ Results',
        chartModeTypesDelta: 'Δ Types',
        chartVisibleSeries: 'Visible series',
        chartDeltaSeries: 'Compared series',
        chartSelectionAll: 'All',
        chartEmpty: 'No visible chart series for the current selection.',
        chartUtcWindow: 'Local time axis · Last {count} UTC hour buckets',
        chartResultSecondarySuccess: 'Secondary success',
        chartResultPrimarySuccess: 'Primary success',
        chartResultSecondaryFailure: 'Secondary failure',
        chartResultPrimaryFailure429: 'Primary failure · 429',
        chartResultPrimaryFailureOther: 'Primary failure · other',
        chartResultUnknown: 'Unknown',
        chartTypeMcpNonBillable: 'MCP non-billable',
        chartTypeMcpBillable: 'MCP billable',
        chartTypeApiNonBillable: 'API non-billable',
        chartTypeApiBillable: 'API billable',
        riskTitle: 'Risk Watchlist',
        riskDescription: 'Items that may require operator action soon.',
        riskEmpty: 'No active risk signals detected.',
        actionsTitle: 'Action Center',
        actionsDescription: 'Recent events you can jump into quickly.',
        recentRequests: 'Recent requests',
        recentJobs: 'Recent jobs',
        openModule: 'Open',
        openToken: 'Open token',
        openKey: 'Open key',
        disabledTokenRisk: 'Token {id} is disabled',
        exhaustedKeyRisk: 'API key {id} is exhausted',
        failedJobRisk: 'Job #{id} status: {status}',
        tokenCoverageTruncated: 'Token risk scope is truncated. Open Tokens for complete coverage.',
        tokenCoverageError: 'Token risk scope could not be loaded. Open Tokens to retry.',
      },
      modules: {
        comingSoon: 'Coming soon',
        users: {
          title: 'User Management',
          description: 'Skeleton module reserved for user and role administration.',
          sections: {
            list: 'User directory',
            roles: 'Roles & permissions',
            status: 'Account status',
          },
        },
        alerts: {
          title: 'Alerts',
          description: 'Skeleton module reserved for alert rules and notifications.',
          sections: {
            rules: 'Alert rules',
            thresholds: 'Threshold policy',
            channels: 'Notification channels',
          },
        },
        proxySettings: {
          title: 'Proxy Settings',
          description: 'Skeleton module reserved for proxy and upstream controls.',
          sections: {
            upstream: 'Upstream targets',
            routing: 'Forwarding strategy',
            rateLimit: 'Rate limit policy',
          },
        },
      },
      proxySettings: {
        title: 'Forward Proxy Settings',
        description: 'Manage subscription-first proxy pools, validate candidates, and inspect live node affinity.',
        actions: {
          refresh: 'Refresh stats',
          save: 'Save settings',
          saving: 'Saving…',
          validateSubscriptions: 'Revalidate all',
          validatingSubscriptions: 'Revalidating all…',
          validateManual: 'Validate manual proxies',
          validatingManual: 'Validating manual proxies…',
        },
        summary: {
          configuredNodes: 'Configured nodes',
          configuredNodesHint: 'Current selectable pool across manual, subscription, and Direct fallback.',
          readyNodes: 'Ready nodes',
          readyNodesHint: 'Nodes that are both selectable and not currently penalized.',
          penalizedNodes: 'Penalized nodes',
          penalizedNodesHint: 'Nodes under temporary recovery or failure pressure.',
          subscriptions: 'Subscriptions',
          subscriptionsHint: 'Remote subscription sources currently persisted.',
          manualNodes: 'Manual proxies',
          manualNodesHint: 'Manually pinned proxy candidates kept outside subscriptions.',
          assignmentSpread: 'Primary / secondary',
          assignmentSpreadHint: 'How many upstream keys currently bind to each node tier.',
          range: 'Stats range',
          savedAt: 'Saved at {time}',
        },
        config: {
          title: 'Configuration',
          description: 'Manage saved subscriptions and manual nodes here. New entries live in dialogs so the page stays easy to scan.',
          loading: 'Loading proxy settings…',
          addSubscription: 'Add subscription',
          addManual: 'Add proxy nodes',
          subscriptionCount: '{count} subscription(s)',
          manualCount: '{count} manual node(s)',
          subscriptionsTitle: 'Subscription URLs',
          subscriptionsDescription: 'Persisted subscription sources. Add new feeds from the dialog or remove stale ones here.',
          subscriptionsPlaceholder: 'https://example.com/subscription.base64',
          subscriptionListEmpty: 'No subscription URLs yet.',
          subscriptionItemFallback: 'Subscription {index}',
          manualTitle: 'Manual proxy URLs',
          manualDescription: 'Pinned manual nodes kept outside subscriptions for recovery, overrides, or staging.',
          manualPlaceholder: 'http://127.0.0.1:8080\nsocks5h://127.0.0.1:1080\nvmess://…',
          manualListEmpty: 'No manual proxy URLs yet.',
          manualItemFallback: 'Manual node {index}',
          egressTitle: 'Global SOCKS5 egress proxy',
          egressDescription: 'Use one upstream SOCKS5 proxy for every non-direct forward-proxy request so outbound routing stays consistent.',
          egressEnabled: 'Enabled',
          egressDisabled: 'Disabled',
          egressUrlLabel: 'SOCKS5 proxy URL',
          egressUrlPlaceholder: 'socks5h://user:pass@127.0.0.1:1080',
          egressUrlHint: 'Accepts socks5:// or socks5h://. The URL remains editable while the proxy is disabled.',
          egressLockedHint: 'Disable the egress proxy first if you need to edit the URL.',
          egressRequiredError: 'Enter a SOCKS5 proxy URL before enabling the egress proxy.',
          egressErrorTitle: 'Unable to enable the SOCKS5 egress proxy',
          egressInvalidUrlError: 'Use a valid socks5:// or socks5h:// proxy URL.',
          egressUnknownError: 'The SOCKS5 egress proxy could not be enabled. Check the address and try again.',
          egressSwitchLabel: 'Enable SOCKS5 egress proxy',
          egressSwitchHint: 'New non-direct requests switch over only after validation and apply steps finish.',
          egressApply: 'Apply egress proxy',
          egressApplying: 'Applying egress proxy…',
          subscriptionIntervalLabel: 'Subscription refresh interval (seconds)',
          subscriptionIntervalHint: 'Used by the backend refresh scheduler. Keep it high enough to avoid noisy churn.',
          invalidInterval: 'Subscription refresh interval must be a positive integer.',
          insertDirectLabel: 'Insert Direct fallback',
          insertDirectHint: 'Keep Direct as a secondary or last-resort route when proxy nodes become unavailable.',
          subscriptionDialogTitle: 'Add subscription URL',
          subscriptionDialogDescription: 'Paste one subscription URL, validate it if you want a preview, or add it directly.',
          subscriptionDialogInputLabel: 'Subscription URL',
          manualDialogTitle: 'Import proxy nodes',
          manualDialogDescription: 'Paste one or more manual nodes, optionally validate them first, or import the pasted entries directly.',
          manualDialogInputLabel: 'Proxy node lines',
          validate: 'Validate',
          validating: 'Validating candidates…',
          add: 'Add',
          addedToList: 'Added to the list.',
          importAvailable: 'Import {count} node(s)',
          importInput: 'Import pasted nodes',
          cancel: 'Cancel',
          remove: 'Remove',
          resultNode: 'Node',
          resultNetwork: 'IP / Location',
          resultStatus: 'Status',
          resultLatency: 'Latency',
          resultAction: 'Action',
          resultDetails: 'View details',
          closeDetails: 'Close details',
          saveFailed: 'Failed to save forward proxy settings.',
        },
        validation: {
          title: 'Validation results',
          description: 'Run candidate probes before saving to understand parsing, Xray, and reachability failures.',
          empty: 'No validation results yet. Trigger a validation action to inspect candidate health.',
          emptySubscriptions: 'Add at least one subscription URL before validating subscriptions.',
          emptyManual: 'Add at least one manual proxy URL before validating manual candidates.',
          ok: 'Reachable',
          failed: 'Failed',
          proxyKind: 'Proxy candidate',
          subscriptionKind: 'Subscription candidate',
          discoveredNodes: 'Discovered nodes',
          latency: 'Latency',
          requestFailed: 'Validation request failed.',
          timeout: 'Timed out',
          unreachable: 'Unreachable',
          xrayMissing: 'Xray unavailable',
          subscriptionInvalid: 'Subscription format unsupported',
          subscriptionUnreachable: 'Subscription unavailable',
          subscriptionTimedOut: 'Validation timed out and found no usable nodes',
          subscriptionNoNodes: 'The subscription did not resolve to any nodes',
          subscriptionUnsupportedNodes: 'The subscription did not contain supported nodes',
          cancelled: 'Validation cancelled.',
          validationFailed: 'Validation failed',
        },
        progress: {
          titleValidate: 'Validation progress',
          titleSave: 'Save progress',
          titleRevalidate: 'Revalidation progress',
          badgeValidate: 'Validate',
          badgeSave: 'Save',
          badgeRevalidate: 'Revalidate',
          buttonValidatingSubscription: 'Validating subscription…',
          buttonValidatingManual: 'Validating nodes…',
          buttonAddingSubscription: 'Adding subscription…',
          buttonAddingManual: 'Importing nodes…',
          running: 'In progress…',
          waiting: 'Waiting…',
          done: 'Done',
          failed: 'Failed',
          stepCounter: '{current}/{total}',
          steps: {
            save_settings: 'Save settings',
            validate_egress_socks5: 'Validate SOCKS5 egress proxy',
            apply_egress_socks5: 'Apply SOCKS5 egress proxy',
            refresh_subscription: 'Refresh subscription',
            bootstrap_probe: 'Run bootstrap probes',
            normalize_input: 'Normalize input',
            parse_input: 'Parse input',
            fetch_subscription: 'Fetch subscription',
            probe_nodes: 'Probe nodes',
            generate_result: 'Prepare result',
            refresh_ui: 'Refresh settings and stats',
          },
        },
        nodes: {
          title: 'Node pool & live stats',
          description: 'Observe current node state, window metrics, 24-hour activity, and key affinity spread.',
          loading: 'Loading forward proxy stats…',
          empty: 'No forward proxy nodes are available yet.',
          table: {
            node: 'Node',
            source: 'Source',
            endpoint: 'Endpoint',
            state: 'State',
            assignments: 'Assignments',
            windows: 'Window stats',
            activity24h: '24h activity',
            weight24h: '24h weight',
          },
          weightLabel: 'Current weight',
          primary: 'Primary',
          secondary: 'Secondary',
          successRateLabel: 'Success',
          latencyLabel: 'Latency',
          successCountLabel: 'Successes',
          failureCountLabel: 'Failures',
          lastWeightLabel: 'Last',
          avgWeightLabel: 'Average',
          minMaxWeightLabel: 'Min / max',
        },
        states: {
          ready: 'Ready',
          readyHint: 'Eligible for new traffic within existing key affinity.',
          penalized: 'Penalized',
          penalizedHint: 'Temporarily deprioritized until recovery probes succeed.',
          direct: 'Direct',
          timeout: 'Timeout',
          timeoutHint: 'Recent probes timed out before the node could answer.',
          unreachable: 'Unreachable',
          unreachableHint: 'The node is currently failing connection attempts or transport setup.',
          unavailable: 'Unavailable',
          unavailableHint: 'The node is not selectable right now.',
          xrayMissing: 'Xray missing',
          xrayMissingHint: 'Share-link parsing succeeded, but the local Xray runtime is unavailable.',
        },
        sources: {
          manual: 'Manual',
          subscription: 'Subscription',
          direct: 'Direct',
          unknown: 'Unknown',
        },
        windows: {
          oneMinute: '1m',
          fifteenMinutes: '15m',
          oneHour: '1h',
          oneDay: '1d',
          sevenDays: '7d',
        },
      },
      systemSettings: {
        title: 'System Settings',
        description: 'Adjust system-level MCP session affinity behavior and apply it without restarting.',
        helpLabel: 'Show system settings help',
        form: {
          title: 'MCP session affinity pool',
          description: 'New MCP initialize requests pick from a stable per-user upstream key pool, then balance sessions inside that pool.',
          countLabel: 'Affinity key count',
          countHint: 'Each user stays sticky to up to this many upstream keys. A stable top-N ranking keeps pool changes minimal when the count or available keys change.',
          currentValue: 'Current value: {count}',
          applyScopeHint: 'Applies immediately to newly created MCP sessions only. Existing sessions keep their current upstream key.',
          invalidCount: 'Enter an integer between 1 and 1000.',
          saveFailed: 'Failed to save system settings.',
        },
        actions: {
          apply: 'Apply settings',
          applying: 'Applying…',
        },
      },
      users: {
        title: 'User Management',
        description: 'Account-level metrics, tag overlays, and shared quota controls.',
        registration: {
          title: 'Allow registration',
          description: 'Applies to first-time Linux DO sign-ins.',
          enabled: 'New sign-ins enabled.',
          disabled: 'New sign-ins paused.',
          unavailable: 'Registration policy unavailable.',
          saving: 'Saving…',
          loadFailed: 'Failed to load registration policy.',
          saveFailed: 'Failed to save registration policy.',
        },
        searchPlaceholder: 'Search by user ID, display name, username, or tag',
        search: 'Search',
        clear: 'Clear',
        pagination: 'Page {page} of {total}',
        table: {
          user: 'User',
          displayName: 'Display Name',
          username: 'Username',
          status: 'Status',
          tokenCount: 'Tokens',
          tags: 'Tags',
          hourlyAny: '1h (any)',
          hourly: '1h',
          daily: '24h',
          monthly: 'Month',
          successDaily: 'Daily S/F',
          successMonthly: 'Monthly Rate/F',
          lastActivity: 'Last Activity',
          lastLogin: 'Last Login',
          actions: 'Actions',
        },
        status: {
          active: 'Active',
          inactive: 'Inactive',
          enabled: 'Enabled',
          disabled: 'Disabled',
          unknown: 'Unknown',
        },
        actions: {
          view: 'Open details',
        },
        usage: {
          title: 'User Usage',
          description: 'Usage-first view for rolling quotas, success rates, token totals, and latest activity.',
          open: 'Open usage view',
          back: 'Back to users',
          table: {
            user: 'User',
            status: 'Status',
            hourlyAny: '1h (any)',
            hourly: '1h',
            daily: '24h',
            monthly: 'Month',
            monthlyBroken: 'Monthly Breakage',
            dailySuccessRate: 'Daily Success',
            monthlySuccessRate: 'Monthly Success',
            lastUsed: 'Last Used',
          },
        },
        empty: {
          loading: 'Loading users…',
          none: 'No users found.',
          notFound: 'User not found.',
          noTokens: 'This user has no bound tokens.',
        },
        detail: {
          title: 'User Detail',
          subtitle: 'Account {id}',
          back: 'Back to users',
          userId: 'User ID',
          identityTitle: 'Identity',
          identityDescription: 'Stable account identifiers and login status.',
          tokensTitle: 'Tokens',
          tokensDescription: 'All tokens under this account share the effective quota below.',
        },
        brokenKeys: {
          limitTitle: 'Monthly Breakage Limit',
          limitDescription: 'Only counts upstream keys broken this month and still unavailable right now.',
          limitField: 'Monthly breakage limit',
          hint: 'This limit only applies to monthly breakage statistics and does not affect business quota.',
          save: 'Save monthly breakage limit',
          saving: 'Saving…',
          savedAt: 'Saved at {time}',
          invalid: 'Monthly breakage limit must be a non-negative integer.',
          saveFailed: 'Failed to save monthly breakage limit.',
          openAction: 'Open monthly breakage details',
          openDetails: 'Open monthly breakage details for {label}',
          drawerTitle: 'Monthly Broken Keys',
          drawerDescription: 'Keys currently counted under {label}.',
          loading: 'Loading monthly broken keys…',
          empty: 'No monthly broken keys are currently counted here.',
          noReason: 'No reason recorded.',
          noRelatedUsers: 'No related users',
          breakerSystem: 'System',
          breakerUnknown: 'Unknown',
          table: {
            key: 'Key',
            status: 'Status',
            reason: 'Reason',
            latestBreakAt: 'Latest Break',
            breaker: 'Last Breaker',
            relatedUsers: 'Related Users',
          },
          actions: {
            copyKeyId: 'Copy key ID',
            copied: 'Copied',
          },
        },
        quota: {
          title: 'Base Quota',
          description: 'Edit the account baseline. Tags are applied on top of this baseline.',
          hourlyAny: 'Hourly-any limit',
          hourly: 'Hourly business limit',
          daily: 'Daily business limit',
          monthly: 'Monthly business limit',
          hint: 'Base quota accepts non-negative integers only.',
          save: 'Save base quota',
          saving: 'Saving…',
          savedAt: 'Saved at {time}',
          invalid: 'All base quota fields must be non-negative integers.',
          saveFailed: 'Failed to update base quota.',
          inheritsDefaults: 'Following defaults',
          customized: 'Customized baseline',
        },
        catalog: {
          title: 'Tag Catalog',
          description: 'Review reusable tags, account coverage, and effect settings.',
          summaryTitle: 'Tag Coverage',
          summaryDescription: 'See how many accounts are bound to each tag, then open the tag manager for full maintenance.',
          summaryEmpty: 'No user tags available yet.',
          summaryAccounts: 'accounts',
          loading: 'Loading tag catalog…',
          empty: 'No tags available yet.',
          invalid: 'Please complete the required fields and use integer deltas.',
          loadFailed: 'Failed to load tag catalog.',
          saveFailed: 'Failed to save tag settings.',
          deleteFailed: 'Failed to delete this tag.',
          formCreateTitle: 'Create tag',
          formEditTitle: 'Edit tag',
          formDescription: 'Custom tags can group users, add quota, subtract quota, or block all access.',
          systemReadonly: 'System tags keep their name and icon locked. Only quota effect fields are editable.',
          iconPlaceholder: 'linuxdo or leave empty',
          iconHint: 'Use `linuxdo` to render the local LinuxDo logo asset.',
          scopeSystem: 'System',
          scopeSystemShort: 'SYS',
          scopeCustom: 'Custom',
          blockShort: 'BLOCK',
          blockDescription: 'block_all forces every effective quota dimension to display and enforce as 0.',
          deleteConfirm: 'Delete tag “{name}”? Existing bindings will be removed too.',
          deleteDialogTitle: 'Delete tag',
          deleteDialogCancel: 'Cancel',
          deleteDialogConfirm: 'Delete',
          backToUsers: 'Back to users',
          backToList: 'Back to tag list',
          tagNotFound: 'Tag not found.',
          columns: {
            tag: 'Tag',
            scope: 'Scope',
            effect: 'Effect',
            delta: 'Quota Delta',
            users: 'Users',
            actions: 'Actions',
          },
          fields: {
            name: 'Tag name',
            displayName: 'Display name',
            icon: 'Icon key',
            effect: 'Effect kind',
            hourlyAny: 'Hourly-any delta',
            hourly: 'Hourly delta',
            daily: 'Daily delta',
            monthly: 'Monthly delta',
          },
          effectKinds: {
            quotaDelta: 'Quota delta',
            blockAll: 'Block all',
          },
          actions: {
            create: 'New tag',
            save: 'Save tag',
            saving: 'Saving…',
            cancelEdit: 'Cancel edit',
            edit: 'Edit tag',
            delete: 'Delete tag',
          },
        },
        userTags: {
          title: 'User Tags',
          description: 'Bind reusable tags to this user. System bindings stay read-only.',
          empty: 'No tags are currently bound.',
          bindPlaceholder: 'Select a custom tag to bind',
          bindAction: 'Bind tag',
          binding: 'Binding…',
          unbindAction: 'Unbind',
          bindFailed: 'Failed to bind this tag.',
          unbindFailed: 'Failed to unbind this tag.',
          readOnly: 'Read-only',
          sourceSystem: 'System sync',
          sourceManual: 'Manual bind',
          manageCatalog: 'Manage catalog',
        },
        effectiveQuota: {
          title: 'Effective Quota Breakdown',
          description: 'Effective quota = base quota + all tag deltas, then clamped to zero for display and enforcement.',
          blockAllNotice: 'A block_all tag is active. Effective quota is fully clamped to 0 for this user.',
          baseLabel: 'Base quota',
          effectiveLabel: 'Effective quota',
          columns: {
            item: 'Item',
            source: 'Source',
            effect: 'Effect',
          },
        },
        tokens: {
          table: {
            id: 'Token ID',
            note: 'Note',
            status: 'Status',
            hourlyAny: '1h (any)',
            hourly: '1h',
            daily: '24h',
            monthly: 'Month',
            successDaily: 'Daily S/F',
            successMonthly: 'Monthly S',
            lastUsed: 'Last Used',
            actions: 'Actions',
          },
          actions: {
            view: 'Open token detail',
          },
        },
      },
      accessibility: {
        skipToContent: 'Skip to main content',
      },
      tokens: {
        title: 'Access Tokens',
        description: 'Auth for /mcp. Format th-xxxx-xxxxxxxxxxxx',
        notePlaceholder: 'Note (optional)',
        newToken: 'New Token',
        creating: 'Creating…',
        batchCreate: 'Batch Create',
        pagination: {
          prev: 'Prev',
          next: 'Next',
          page: 'Page {page} of {total}',
        },
        table: {
          id: 'ID',
          note: 'Note',
          owner: 'Owner',
          usage: 'Usage',
          quota: 'Rate Limit',
          lastUsed: 'Last Used',
          actions: 'Actions',
        },
        empty: {
          loading: 'Loading tokens…',
          none: 'No tokens yet.',
        },
        owner: {
          label: 'Linked User',
          unbound: 'Unbound',
        },
        actions: {
          copy: 'Copy full token',
          share: 'Copy share link',
          disable: 'Disable token',
          enable: 'Enable token',
          edit: 'Edit note',
          delete: 'Delete token',
          viewLeaderboard: 'View unbound token usage',
        },
        statusBadges: {
          disabled: 'Disabled token',
        },
        quotaStates: {
          normal: 'Normal',
          hour: '1 hour limit',
          day: '24 hour limit',
          month: 'Monthly limit',
        },
        dialogs: {
          delete: {
            title: 'Delete Token',
            description: 'This will permanently remove the access token. Clients using it will receive 401.',
            cancel: 'Cancel',
            confirm: 'Delete',
          },
          note: {
            title: 'Edit Token Note',
            placeholder: 'Note',
            cancel: 'Cancel',
            confirm: 'Save',
            saving: 'Saving…',
          },
        },
        batchDialog: {
          title: 'Batch Create Tokens',
          groupPlaceholder: 'Group (required)',
          confirm: 'Create',
          creating: 'Creating…',
          cancel: 'Cancel',
          done: 'Done',
          createdN: 'Created {n} tokens',
          copyAll: 'Copy all links',
        },
        groups: {
          label: 'Groups',
          all: 'All',
          ungrouped: 'Ungrouped',
          moreShow: 'Show all groups',
          moreHide: 'Collapse groups',
        },
      },
      unboundTokenUsage: {
        title: 'Unbound Token Usage',
        description: 'Only tokens without linked users are listed here, using the same usage fields as the admin user usage page.',
        searchPlaceholder: 'Search by token ID, note, or group',
        error: 'Unable to load unbound token usage',
        table: {
          identity: 'Identity',
          status: 'Status',
          hourlyAny: '1h (any)',
          hourly: '1h',
          daily: '24h',
          monthly: 'Month',
          monthlyBroken: 'Monthly Breakage',
          dailySuccessRate: 'Daily Success',
          monthlySuccessRate: 'Monthly Success',
          lastUsed: 'Last Used',
        },
        empty: {
          loading: 'Loading unbound token usage…',
          none: 'No unbound token activity recorded yet.',
        },
        back: 'Back to tokens',
      },
      metrics: {
        labels: {
          total: 'Total Requests',
          success: 'Success',
          failure: 'Failure',
          errors: 'Errors',
          quota: 'Quota Exhausted',
          unknownCalls: 'Unknown Calls',
          newKeys: 'New Keys',
          newQuarantines: 'New Quarantines',
          keys: 'Active Keys',
          quarantined: 'Quarantined',
          exhausted: 'Exhausted',
          remaining: 'Remaining',
          proxyAvailable: 'Available Proxy Nodes',
          proxyTotal: 'Proxy Nodes Total',
        },
        subtitles: {
          keysAll: 'All keys available',
          keysExhausted: '{count} exhausted',
          keysAvailability: '{active} active · {quarantined} quarantined · {exhausted} exhausted',
        },
        loading: 'Loading latest metrics…',
      },
      keys: {
        title: 'API Keys',
        description: 'Status, usage, and recent success rates per Tavily API key.',
        placeholder: 'New Tavily API Key',
        addButton: 'Add Key',
        adding: 'Adding…',
        batch: {
          placeholder: 'Paste text (extract first tvly-dev-* key and public IP per line)',
          groupPlaceholder: 'Group (optional)',
          hint: 'Each line extracts the first tvly-dev-* key and first public IP; geo lookup uses the configured country.is-compatible service.',
          count: 'Extracted keys {count}',
          report: {
            title: 'Batch Import Report',
            close: 'Done',
            summary: {
              inputLines: 'Input lines',
              validLines: 'Valid lines',
              uniqueInInput: 'Unique in input',
              created: 'Created',
              undeleted: 'Restored',
              existed: 'Already existed',
              duplicateInInput: 'Duplicates in input',
              failed: 'Failed',
            },
            failures: {
              title: 'Failures',
              none: 'No failures.',
              table: {
                apiKey: 'API Key',
                error: 'Error',
              },
            },
          },
        },
        validation: {
          title: 'Verify API Keys',
          hint: 'Support text paste: extract the first tvly-dev-* key and first public IP from each line; geo lookup uses the configured country.is-compatible service.',
          registrationIpBadge: 'IP',
          registrationIpTooltip: 'Registration IP: {ip}',
          actions: {
            close: 'Close',
            retry: 'Retry',
            retryFailed: 'Retry failed',
            import: 'Import',
            importValid: 'Import {count} valid keys',
            imported: 'Imported',
          },
          import: {
            title: 'Import Result',
            exhaustedMarkFailed: '{count} exhausted keys could not be marked as exhausted',
          },
          summary: {
            group: 'Group: {group}',
            inputLines: 'Input lines',
            validLines: 'Valid lines',
            uniqueInInput: 'Unique in input',
            duplicateInInput: 'Duplicates in input',
            checked: 'Checked {checked} / {total}',
            ok: 'Valid',
            exhausted: 'Exhausted',
            exhaustedNote: '{count} keys will be imported as exhausted',
            invalid: 'Invalid',
            error: 'Error',
          },
          emptyFiltered: 'No rows match the selected status.',
          table: {
            apiKey: 'API Key',
            result: 'Result',
            quota: 'Quota',
            actions: 'Actions',
          },
          statuses: {
            pending: 'Pending',
            duplicate_in_input: 'Duplicate',
            ok: 'Valid',
            ok_exhausted: 'Valid (exhausted)',
            unauthorized: 'Unauthorized',
            forbidden: 'Forbidden',
            invalid: 'Invalid',
            error: 'Error',
          },
        },
        groups: {
          label: 'Groups',
          all: 'All',
          ungrouped: 'Ungrouped',
          moreShow: 'Show all groups',
          moreHide: 'Collapse groups',
        },
        filters: {
          status: 'Status',
          region: 'Region',
          registrationIp: 'Registration IP',
          registrationIpPlaceholder: 'Filter by exact IP',
          clearGroups: 'Show all groups',
          clearStatuses: 'Show all statuses',
          clearRegistrationIp: 'Clear IP',
          clearRegions: 'Show all regions',
          selectedSuffix: 'selected',
        },
        selection: {
          selectRow: 'Select key',
          selectAll: 'Select all keys on this page',
          selectCurrentPage: 'Select current page',
          clear: 'Clear selection',
          selectedCount: '{count} selected',
        },
        pagination: {
          page: 'Page {page} of {total}',
          perPage: 'Per page',
        },
        table: {
          keyId: 'Key ID',
          registration: 'Registration',
          registrationIp: 'Registration IP',
          registrationRegion: 'Region',
          assignedProxy: 'Assigned Proxy',
          status: 'Status',
          total: 'Total',
        success: 'Success',
        errors: 'Errors',
        quota: 'Quota Exhausted',
        successRate: 'Success Rate',
        remainingPct: 'Remaining %',
        quotaLeft: 'Remaining',
        syncedAt: 'Synced',
        lastUsed: 'Last Used',
        statusChanged: 'Status Changed',
        actions: 'Actions',
      },
        empty: {
          loading: 'Loading key statistics…',
          none: 'No key data recorded yet.',
          filtered: 'No keys match the current filters.',
        },
        actions: {
          copy: 'Copy original API key',
          enable: 'Enable key',
          disable: 'Disable key',
          clearQuarantine: 'Clear quarantine',
          delete: 'Remove key',
          details: 'Details',
        },
        bulkActions: {
          syncUsage: 'Sync usage',
          clearQuarantine: 'Clear quarantine',
          delete: 'Remove selected',
          running: 'Running…',
          summary: '{action}: {succeeded} succeeded, {skipped} skipped, {failed} failed (requested {requested})',
        },
        bulkSyncProgress: {
          title: 'Sync usage progress',
          running: 'Streaming live key-by-key results from the current request.',
          finished: 'Manual sync finished. Review the latest result below.',
          refreshingList: 'Refreshing the current keys list with the latest quota snapshot…',
          steps: {
            prepareRequest: 'Prepare request',
            syncUsage: 'Sync selected keys',
            refreshUi: 'Refresh current list',
          },
          status: {
            waiting: 'Waiting…',
            running: 'Running…',
            done: 'Done',
            failed: 'Failed',
          },
          counters: {
            progress: '{current}/{total} processed',
            success: 'Success {count}',
            skipped: 'Skipped {count}',
            failed: 'Failed {count}',
          },
          lastResultLabel: 'Latest key result',
          result: {
            success: 'Success',
            skipped: 'Skipped',
            failed: 'Failed',
            noDetail: 'No additional detail from the server.',
          },
        },
        quarantine: {
          badge: 'Quarantined',
          sourcePrefix: 'Source: {source}',
          noReason: 'No quarantine reason recorded.',
        },
        dialogs: {
          disable: {
            title: 'Disable API Key',
            description: 'This will stop using the key until you enable it again. No data will be removed.',
            cancel: 'Cancel',
            confirm: 'Disable',
          },
          delete: {
            title: 'Remove API Key',
            description: 'This will mark the key as Deleted. You can restore it later by re-adding the same secret.',
            cancel: 'Cancel',
            confirm: 'Remove',
          },
          bulkDelete: {
            title: 'Remove selected API Keys',
            description: 'Selected keys will be marked as Deleted. You can restore them later by re-adding the same secrets.',
            cancel: 'Cancel',
            confirm: 'Remove selected',
          },
        },
      },
      logs: {
        title: 'Recent Requests',
        description: 'Browse recent requests in reverse chronological order.',
        descriptionFallback: 'Browse recent requests in reverse chronological order.',
        descriptionWithRetention: 'Browse recent requests in reverse chronological order. Logs are retained for {days} days.',
        filters: {
          all: 'All',
          success: 'Success',
          error: 'Errors',
          quota: 'Quota exhausted',
          requestType: 'Request Type',
          requestTypeAll: 'All',
          requestTypeEmpty: 'No request types in this window',
          billingGroup: 'Billing',
          protocolGroup: 'Protocol',
          resultOrEffect: 'Result / Key Effect',
          resultOrEffectAll: 'All',
          resultGroup: 'Result',
          keyEffectGroup: 'Key Effect',
          tokenAll: 'All tokens',
          keyAll: 'All',
          noFacetOptions: 'No options in this window',
        },
        empty: {
          loading: 'Collecting recent requests…',
          none: 'No request logs captured yet.',
        },
        table: {
          key: 'Key',
          token: 'Token',
          time: 'Time',
          requestType: 'Request Type',
          status: 'Status',
          chargedCredits: 'Credits',
          httpStatus: 'HTTP Status',
          mcpStatus: 'Tavily Status',
          result: 'Result',
          keyEffect: 'Key Effect',
          error: 'Error',
        },
        toggles: {
          show: 'Show request details',
          hide: 'Hide request details',
        },
        pagination: {
          summary: 'Use newer/older navigation to browse recent requests.',
          summaryWithRetention: 'Use newer/older navigation to browse recent requests kept for {days} days.',
          newer: 'Newer',
          older: 'Older',
        },
        errors: {
          quotaExhausted: 'Quota exhausted',
          quotaExhaustedHttp: 'Quota exhausted (HTTP {http})',
          requestFailedHttpMcp: 'Request failed (HTTP {http}, Tavily {mcp})',
          requestFailedHttp: 'Request failed (HTTP {http})',
          requestFailedMcp: 'Request failed (Tavily {mcp})',
          requestFailedGeneric: 'Request failed',
          httpStatus: 'HTTP {http}',
          none: '—',
        },
        keyEffects: {
          none: 'No Change',
          quarantined: 'Quarantined',
          markedExhausted: 'Exhausted',
          restoredActive: 'Restored',
          clearedQuarantine: 'Cleared',
          unknown: 'Updated',
        },
      },
      jobs: {
        title: 'Scheduled Jobs',
        description: 'Recent background job executions.',
        filters: {
          all: 'All',
          quota: 'Sync quota',
          usage: 'Usage rollups',
          logs: 'Clean access logs',
          geo: 'Node IP/GEO refresh',
          linuxdo: 'LinuxDo user sync',
        },
        empty: {
          loading: 'Loading jobs…',
          none: 'No jobs yet.',
        },
        table: {
          id: 'ID',
          type: 'Type',
          key: 'Key',
          status: 'Status',
          attempt: 'Attempt',
          started: 'Started',
          message: 'Message',
        },
        toggles: {
          show: 'Show job details',
          hide: 'Hide job details',
        },
        types: {
          quota_sync: 'Sync quota',
          'quota_sync/hot': 'Hot quota sync',
          'quota_sync/manual': 'Manual sync',
          token_usage_rollup: 'Usage rollups',
          usage_aggregation: 'Usage rollups',
          auth_token_logs_gc: 'Log cleanup',
          request_logs_gc: 'Log cleanup',
          log_cleanup: 'Log cleanup',
          forward_proxy_geo_refresh: 'Node IP/GEO refresh',
          linuxdo_user_status_sync: 'LinuxDo user sync',
        },
      },
      statuses: {
        active: 'Active',
        quarantined: 'Quarantined',
        exhausted: 'Exhausted',
        success: 'Success',
        running: 'Running',
        pending: 'Pending',
        queued: 'Queued',
        completed: 'Completed',
        error: 'Error',
        failed: 'Failed',
        retry_exhausted: 'Retry Exhausted',
        neutral: 'Neutral',
        quota_exhausted: 'Quota Exhausted',
        timeout: 'Timed Out',
        cancelled: 'Canceled',
        deleted: 'Deleted',
        unknown: 'Unknown',
      },
      logDetails: {
        request: 'Request',
        response: 'Response',
        outcome: 'Outcome',
        keyEffect: 'Key Effect',
        requestTypeDetail: 'Request Type Detail',
        solution: 'Suggested Handling',
        requestBody: 'Request Body',
        responseBody: 'Response Body',
        noBody: 'No body captured.',
        loadingBody: 'Loading request details…',
        loadBodyFailed: 'Failed to load request details.',
        retryLoadBody: 'Retry',
        noKeyEffect: 'No automatic key state change',
        forwardedHeaders: 'Forwarded Headers',
        droppedHeaders: 'Dropped Headers',
      },
      keyDetails: {
        title: 'Key Details',
        descriptionPrefix: 'Inspect usage and recent requests for key:',
        back: 'Back',
        syncAction: 'Sync Usage',
        syncing: 'Syncing…',
        syncSuccess: 'Synced',
        usageTitle: 'Usage',
        usageDescription: 'Aggregated counts for selected period.',
        periodOptions: {
          day: 'Day',
          week: 'Week',
          month: 'Month',
        },
        apply: 'Apply',
        loading: 'Loading…',
        metrics: {
          total: 'Total',
          success: 'Successful',
          errors: 'Errors',
          quota: 'Quota Exhausted',
          lastActivityPrefix: 'Last activity',
          noActivity: 'No activity',
        },
        quarantine: {
          title: 'System Quarantine',
          description: 'This key is excluded from rotation until an admin clears the quarantine.',
          source: 'Source',
          reason: 'Reason',
          detail: 'Detail',
          showDetail: 'Show raw detail',
          hideDetail: 'Hide raw detail',
          createdAt: 'Quarantined at',
          clearAction: 'Clear quarantine',
          clearing: 'Clearing…',
        },
        metadata: {
          title: 'Registration Metadata',
          description: 'Import-time registration metadata extracted from the original account row.',
          group: 'Group',
          registrationIp: 'Registration IP',
          registrationRegion: 'Region',
        },
        stickyUsers: {
          title: 'Sticky Users',
          description: 'Current user bindings on this key, charged credits only.',
          empty: 'No users are currently sticky to this key.',
          user: 'User',
          yesterday: 'Yesterday',
          today: 'Today',
          month: 'Month',
          trend: '7d trend',
          lastSuccess: 'Last success',
          success: 'Success',
          failure: 'Failure',
          inactive: 'Inactive',
        },
        stickyNodes: {
          title: 'Sticky Nodes',
          description: 'Current primary and secondary proxy nodes for this key.',
          empty: 'No sticky nodes are currently assigned.',
          role: 'Role',
          node: 'Node',
          activity: '24h activity',
          weight: '24h weight',
          primary: 'Primary',
          secondary: 'Secondary',
          assignmentSummary: 'P{primary} · S{secondary}',
          primaryAssignments: 'Primary for {count} keys',
          secondaryAssignments: 'Secondary for {count} keys',
          window: 'Window',
        },
        logsTitle: 'Recent Requests',
        logsDescription: 'Browse recent requests for this key in reverse chronological order.',
        logsEmpty: 'No request logs for this period.',
      },
      errors: {
        copyKey: 'Failed to copy API key',
        addKey: 'Failed to add API key',
        addKeysBatch: 'Failed to add API keys',
        createToken: 'Failed to create token',
        copyToken: 'Failed to copy token',
        toggleToken: 'Failed to update token status',
        deleteToken: 'Failed to delete token',
        updateTokenNote: 'Failed to update token note',
        deleteKey: 'Failed to delete API key',
        toggleKey: 'Failed to update key status',
        clearQuarantine: 'Failed to clear key quarantine',
        loadKeyDetails: 'Failed to load details',
        syncUsage: 'Failed to sync usage',
      },
      footer: {
        title: 'Tavily Hikari Proxy Dashboard',
        githubAria: 'Open GitHub repository',
        githubLabel: 'GitHub',
        loadingVersion: '· Loading version…',
        tagPrefix: '· ',
      },
    },
  },
  zh: {
    common: {
      languageLabel: '语言',
      englishLabel: 'English',
      chineseLabel: '中文',
    },
    public: {
      updateBanner: {
        title: '有新版本上线',
        description: (current, latest) => `当前 ${current} → 可用 ${latest}`,
        refresh: '刷新以更新',
        dismiss: '暂不提醒',
      },
      heroTitle: 'Tavily Hikari Proxy',
      heroTagline: 'Transparent request visibility for your Tavily integration.',
      heroDescription:
        'Tavily Hikari 将多组 Tavily API Key 聚合为统一入口，自动均衡密钥用量，并提供请求审计、速率监控与访问令牌管理。',
      metrics: {
        monthly: {
          title: '本月成功请求（UTC）',
          subtitle: 'Tavily 月额度按 UTC 月初自动重置',
        },
        daily: {
          title: '今日',
          subtitle: '按当前浏览器本地今日窗口统计的成功请求',
        },
        pool: {
          title: '号池可用数',
          subtitle: '活跃 Tavily API Key / 总密钥（含本月耗尽）',
        },
      },
      adminButton: '打开管理员面板',
      adminLoginButton: '管理员登录',
      linuxDoLogin: {
        button: '使用 Linux DO 登录',
        logoAlt: 'Linux DO 标志',
      },
      registrationPaused: {
        badge: '暂停注册',
        title: '新用户注册暂时关闭',
        description: '当前服务仅允许已注册用户继续登录，暂不接受新的 Linux DO 账户创建本地身份。',
        returnHome: '返回首页',
        continueHint: '如果你已经有账号，请返回首页继续登录。',
      },
      registrationPausedNotice: {
        title: '新注册已暂停',
        description: '已注册用户仍可继续使用 Linux DO 登录；新的账户暂时无法创建。',
      },
      adminLogin: {
        title: '管理员登录',
        description: '登录后可管理 Tavily key 与访问令牌。',
        password: {
          label: '管理员口令',
          placeholder: '请输入管理员口令',
        },
        submit: {
          label: '登录',
          loading: '登录中…',
        },
        backHome: '返回首页',
        hints: {
          checking: '正在检查登录状态…',
          disabled: '当前服务未启用内置管理员登录。',
        },
        errors: {
          invalid: '口令不正确。',
          disabled: '当前服务未启用内置管理员登录。',
          generic: '登录失败。',
        },
      },
      accessPanel: {
        title: '令牌使用统计',
        stats: {
          dailySuccess: '今日成功',
          dailyFailure: '今日失败',
          monthlySuccess: '本月成功（UTC）',
          hourlyLimit: '1 小时限额',
          dailyLimit: '日限额',
          monthlyLimit: '月度限额',
        },
      },
      accessToken: {
        label: 'Access Token',
        placeholder: 'th-xxxx-xxxxxxxxxxxx',
        toggle: {
          show: '显示 Access Token',
          hide: '隐藏 Access Token',
          iconAlt: '切换 Access Token 可见性',
        },
      },
      copyToken: {
        iconAlt: '复制 Access Token',
        copy: '复制令牌',
        copied: '已复制',
        error: '复制失败',
      },
      tokenAccess: {
        button: '使用令牌访问',
        dialog: {
          title: '使用令牌访问',
          description: '输入 Access Token 后即可查看用量与近期请求。',
          actions: {
            cancel: '取消',
            confirm: '开始使用',
          },
          loginHint: '提示：建议使用 linux.do 登录，以绑定账号。',
        },
      },
      guide: {
        title: '如何在常见客户端接入 Tavily Hikari',
        dataSourceLabel: '数据来源：',
        tokenVisibility: {
          show: '显示密钥',
          hide: '隐藏密钥',
        },
        tabs: {
          codex: 'Codex CLI',
          claude: 'Claude Code CLI',
          vscode: 'VS Code / Copilot',
          claudeDesktop: 'Claude Desktop',
          cursor: 'Cursor',
          windsurf: 'Windsurf',
          cherryStudio: 'Cherry Studio',
          other: '其他',
        },
      },
      footer: {
        version: '当前版本：',
      },
      errors: {
        metrics: '暂时无法加载指标',
        summary: '暂时无法加载摘要数据',
      },
      logs: {
        title: '近期请求（最近 20 条）',
        description: '需要有效 Access Token 才可查看令牌关联的请求活动。',
        empty: {
          noToken: '需要有效 Access Token 才可查看最近 20 条请求。',
          hint: '请使用携带完整 token 的链接，或在上方手动填写有效 token。',
          loading: '正在加载近期请求…',
          none: '该令牌暂无近期请求。',
        },
        table: {
          time: '时间',
          httpStatus: 'HTTP',
          mcpStatus: 'Tavily',
          result: '结果',
        },
        toggles: {
          show: '展开详情',
          hide: '收起详情',
        },
      },
      cherryMock: {
        title: 'Cherry Studio 设置示意',
        windowTitle: '设置',
        sidebar: {
          modelService: '模型服务',
          defaultModel: '默认模型',
          generalSettings: '常规设置',
          displaySettings: '显示设置',
          dataSettings: '数据设置',
          mcp: 'MCP',
          notes: '笔记',
          webSearch: '网络搜索',
          memory: '全局记忆',
          apiServer: 'API 服务器',
          docProcessing: '文档处理',
          quickPhrases: '快捷短语',
          shortcuts: '快捷键',
        },
        providerCard: {
          title: '网络搜索',
          subtitle: '搜索服务商',
          providerValue: 'Tavily (API 密钥)',
        },
        tavilyCard: {
          title: 'Tavily',
          apiKeyLabel: 'API 密钥',
          apiKeyPlaceholder: 'th-xxxx-xxxxxxxxxxxx',
          apiKeyHint: '请将 Tavily Hikari 的访问令牌填入上方 API 密钥。',
          testButtonLabel: '检测',
          apiUrlLabel: 'API 地址',
          apiUrlHint: '在 Cherry Studio 中将此地址填入 “API 地址 / API URL”。',
        },
        generalCard: {
          title: '常规设置',
          includeDateLabel: '搜索包含日期',
          resultsCountLabel: '搜索结果个数',
        },
      },
    },
    admin: {
      header: {
        title: 'Tavily Hikari 总览',
        subtitle: '监控 API Key 分配、额度健康度与最新代理请求活动。',
        updatedPrefix: '更新于',
        refreshNow: '立即刷新',
        refreshing: '刷新中…',
        returnToConsole: '返回用户控制台',
      },
      loadingStates: {
        switching: '正在切换当前结果…',
        refreshing: '正在刷新当前结果…',
        error: '当前结果加载失败。',
      },
      nav: {
        dashboard: '仪表盘',
        usage: '用量',
        tokens: '访问令牌',
        keys: 'API Keys',
        requests: '请求日志',
        jobs: '任务作业',
        users: '用户管理',
        alerts: '告警中心',
        systemSettings: '系统设置',
        proxySettings: '代理设置',
      },
      dashboard: {
        title: '运营仪表盘',
        description: '在一个页面查看全局健康度、风险与可执行动作。',
        loading: '正在加载仪表盘数据…',
        summaryUnavailable: '暂时无法加载期间摘要。',
        statusUnavailable: '暂时无法加载站点当前状态。',
        todayTitle: '今日',
        todayDescription: '截至当前的核心请求指标，对比昨日同一时刻。',
        monthTitle: '本月',
        monthDescription: '用紧凑视图查看本月累计请求表现。',
        currentStatusTitle: '站点当前状态',
        currentStatusDescription: '查看此刻的额度、活跃密钥与池状态。',
        deltaFromYesterday: '较昨日同刻',
        deltaNoBaseline: '昨日无基线',
        percentagePointUnit: '个百分点',
        asOfNow: '截至当前',
        currentSnapshot: '当前快照',
        todayShare: '今日占比',
        todayAdded: '今日新增',
        monthToDate: '本月累计',
        monthAdded: '本月新增',
        monthShare: '本月占比',
        quotaChargeTitle: '额度扣减',
        quotaChargeLocalEstimate: '本地估算',
        quotaChargeUpstreamActual: '上游 Key 实扣',
        quotaChargeDelta: '差值',
        quotaChargeSampledKeys: '已采样',
        quotaChargeStaleKeys: '滞后',
        quotaChargeLatestSync: '最新同步',
        quotaChargeNoSync: '暂无同步数据',
        valuableTag: '主要',
        otherTag: '次要',
        unknownTag: '未知',
        upstreamExhaustedLabel: '上游 Key 耗尽',
        trendsTitle: '流量趋势',
        trendsDescription: '统计窗口仍取最近 25 个已封口 UTC 小时桶，横轴按本地时间显示。',
        requestTrend: '请求量趋势',
        errorTrend: '错误量趋势',
        chartModeResults: '调用结果',
        chartModeTypes: '调用类型',
        chartModeResultsDelta: '较昨日 · 调用结果',
        chartModeTypesDelta: '较昨日 · 调用类型',
        chartVisibleSeries: '显示系列',
        chartDeltaSeries: '对比系列',
        chartSelectionAll: '全部',
        chartEmpty: '当前选择下没有可显示的图表系列。',
        chartUtcWindow: '本地时间横轴 · 最近 {count} 个 UTC 小时桶',
        chartResultSecondarySuccess: '次要成功',
        chartResultPrimarySuccess: '主要成功',
        chartResultSecondaryFailure: '次要失败',
        chartResultPrimaryFailure429: '主要失败 · 429',
        chartResultPrimaryFailureOther: '主要失败 · 其他',
        chartResultUnknown: '未知',
        chartTypeMcpNonBillable: 'MCP 非计费',
        chartTypeMcpBillable: 'MCP 计费',
        chartTypeApiNonBillable: 'API 非计费',
        chartTypeApiBillable: 'API 计费',
        riskTitle: '风险看板',
        riskDescription: '即将需要处理的异常项。',
        riskEmpty: '当前未发现明显风险信号。',
        actionsTitle: '行动中心',
        actionsDescription: '快速查看最近活动并跳转到对应模块。',
        recentRequests: '近期请求',
        recentJobs: '近期任务',
        openModule: '查看',
        openToken: '查看令牌',
        openKey: '查看密钥',
        disabledTokenRisk: '令牌 {id} 已被禁用',
        exhaustedKeyRisk: 'API Key {id} 已耗尽',
        failedJobRisk: '任务 #{id} 状态：{status}',
        tokenCoverageTruncated: '令牌风险范围已截断，请进入“访问令牌”查看完整数据。',
        tokenCoverageError: '令牌风险数据加载失败，请进入“访问令牌”重试。',
      },
      modules: {
        comingSoon: '即将支持',
        users: {
          title: '用户管理',
          description: '预留骨架页：后续接入用户、角色与状态管理能力。',
          sections: {
            list: '用户列表',
            roles: '角色与权限',
            status: '账号状态',
          },
        },
        alerts: {
          title: '告警中心',
          description: '预留骨架页：后续接入规则告警与通知策略。',
          sections: {
            rules: '告警规则',
            thresholds: '阈值策略',
            channels: '通知渠道',
          },
        },
        proxySettings: {
          title: '代理设置',
          description: '预留骨架页：后续接入上游、转发与限流设置。',
          sections: {
            upstream: '上游目标',
            routing: '转发策略',
            rateLimit: '限流策略',
          },
        },
      },
      proxySettings: {
        title: '正向代理设置',
        description: '以订阅为主配置代理节点，查看实时调度状态，并观察上游 Key 的主备节点亲和。',
        actions: {
          refresh: '刷新统计',
          save: '保存设置',
          saving: '保存中…',
          validateSubscriptions: '全量验证',
          validatingSubscriptions: '正在全量验证…',
          validateManual: '验证手工节点',
          validatingManual: '正在验证手工节点…',
        },
        summary: {
          configuredNodes: '当前节点数',
          configuredNodesHint: '包含手工节点、订阅节点与 Direct 兜底节点。',
          readyNodes: '可用节点',
          readyNodesHint: '当前既可选又未处于惩罚状态的节点数量。',
          penalizedNodes: '惩罚中节点',
          penalizedNodesHint: '等待恢复探测或刚发生失败的节点。',
          subscriptions: '订阅源',
          subscriptionsHint: '当前已保存的订阅 URL 数量。',
          manualNodes: '手工节点',
          manualNodesHint: '补充的固定代理 URL 数量。',
          assignmentSpread: '主 / 备绑定',
          assignmentSpreadHint: '所有上游 Key 当前分配到主节点与备用节点的总数。',
          range: '统计范围',
          savedAt: '已于 {time} 保存',
        },
        config: {
          title: '配置',
          description: '这里用于管理已保存的订阅和手工节点；新增动作放进弹窗里，避免把设置页本身挤成大表单。',
          loading: '正在加载代理设置…',
          addSubscription: '添加订阅',
          addManual: '添加节点',
          subscriptionCount: '{count} 个订阅',
          manualCount: '{count} 个手工节点',
          subscriptionsTitle: '订阅 URL',
          subscriptionsDescription: '当前已保存的订阅源。新增走弹窗，旧链接也可以在这里直接删除。',
          subscriptionsPlaceholder: 'https://example.com/subscription.base64',
          subscriptionListEmpty: '还没有订阅链接。',
          subscriptionItemFallback: '订阅 {index}',
          manualTitle: '手工代理 URL',
          manualDescription: '保存在订阅之外的固定节点，适合作为兜底、灰度或临时补位。',
          manualPlaceholder: 'http://127.0.0.1:8080\nsocks5h://127.0.0.1:1080\nvmess://…',
          manualListEmpty: '还没有手工节点。',
          manualItemFallback: '手工节点 {index}',
          egressTitle: '全局 SOCKS5 出口代理',
          egressDescription: '为所有非直连 forward proxy 请求指定统一的上游 SOCKS5 出口代理，便于集中管理出口网络。',
          egressEnabled: '已开启',
          egressDisabled: '已关闭',
          egressUrlLabel: 'SOCKS5 代理地址',
          egressUrlPlaceholder: 'socks5h://user:pass@127.0.0.1:1080',
          egressUrlHint: '支持 socks5:// 与 socks5h://。关闭时可编辑，启用后自动锁定。',
          egressLockedHint: '如需修改地址，请先关闭 SOCKS5 出口代理。',
          egressRequiredError: '请先填写 SOCKS5 代理地址，再开启出口代理。',
          egressErrorTitle: '无法启用 SOCKS5 出口代理',
          egressInvalidUrlError: '请输入有效的 socks5:// 或 socks5h:// 代理地址。',
          egressUnknownError: 'SOCKS5 出口代理启用失败，请检查地址后重试。',
          egressSwitchLabel: '启用 SOCKS5 出口代理',
          egressSwitchHint: '只有在可用性检测和应用步骤完成后，新请求才会切换到新的出口路径。',
          egressApply: '应用出口代理',
          egressApplying: '正在应用出口代理…',
          subscriptionIntervalLabel: '订阅刷新周期（秒）',
          subscriptionIntervalHint: '由后端定时任务使用。周期过短会让节点列表更容易抖动。',
          invalidInterval: '订阅刷新周期必须是大于 0 的整数。',
          insertDirectLabel: '插入 Direct 兜底节点',
          insertDirectHint: '当代理节点全部不可用时，保留 Direct 作为备用或最终回退路径。',
          subscriptionDialogTitle: '添加订阅链接',
          subscriptionDialogDescription: '先粘贴一个订阅 URL，可先验证预览，也可以直接加入已保存列表。',
          subscriptionDialogInputLabel: '订阅 URL',
          manualDialogTitle: '批量导入节点',
          manualDialogDescription: '粘贴一个或多个手工节点，可先验证，也可以直接导入输入内容。',
          manualDialogInputLabel: '节点信息（每行一个）',
          validate: '验证可用性',
          validating: '正在验证候选项…',
          add: '添加',
          addedToList: '已加入列表。',
          importAvailable: '导入 {count} 个节点',
          importInput: '导入输入内容',
          cancel: '取消',
          remove: '删除',
          resultNode: '节点',
          resultNetwork: 'IP / 地理位置',
          resultStatus: '结果',
          resultLatency: '延迟',
          resultAction: '操作',
          resultDetails: '查看详情',
          closeDetails: '关闭详情',
          saveFailed: '保存正向代理设置失败。',
        },
        validation: {
          title: '验证结果',
          description: '在保存前探测候选连通性，提前暴露解析、Xray 或网络可达性问题。',
          empty: '还没有验证结果，先执行一次订阅或手工节点验证吧。',
          emptySubscriptions: '请至少填写一个订阅 URL 再执行验证。',
          emptyManual: '请至少填写一个手工代理 URL 再执行验证。',
          ok: '可达',
          failed: '失败',
          proxyKind: '代理候选',
          subscriptionKind: '订阅候选',
          discoveredNodes: '发现节点',
          latency: '延迟',
          requestFailed: '验证请求失败。',
          timeout: '超时',
          unreachable: '不可达',
          xrayMissing: 'Xray 不可用',
          subscriptionInvalid: '订阅无法解析',
          subscriptionUnreachable: '订阅不可达',
          subscriptionTimedOut: '订阅验证超时，未发现可用节点',
          subscriptionNoNodes: '订阅中没有解析出任何节点',
          subscriptionUnsupportedNodes: '订阅中的节点格式暂不支持',
          cancelled: '已取消验证。',
          validationFailed: '验证失败',
        },
        progress: {
          titleValidate: '验证进度',
          titleSave: '保存进度',
          titleRevalidate: '全量验证进度',
          badgeValidate: '验证',
          badgeSave: '保存',
          badgeRevalidate: '全量验证',
          buttonValidatingSubscription: '正在验证订阅…',
          buttonValidatingManual: '正在验证节点…',
          buttonAddingSubscription: '正在添加订阅…',
          buttonAddingManual: '正在导入节点…',
          running: '进行中…',
          waiting: '等待中…',
          done: '已完成',
          failed: '失败',
          stepCounter: '{current}/{total}',
          steps: {
            save_settings: '保存配置',
            validate_egress_socks5: '校验 SOCKS5 出口代理',
            apply_egress_socks5: '应用 SOCKS5 出口代理',
            refresh_subscription: '刷新订阅',
            bootstrap_probe: '引导探测节点',
            normalize_input: '规范化输入',
            parse_input: '解析输入',
            fetch_subscription: '拉取订阅',
            probe_nodes: '探测节点',
            generate_result: '生成结果',
            refresh_ui: '刷新列表与统计',
          },
        },
        nodes: {
          title: '节点池与实时统计',
          description: '查看当前节点状态、窗口统计、24 小时活动，以及主备绑定分布。',
          loading: '正在加载正向代理统计…',
          empty: '当前还没有可展示的正向代理节点。',
          table: {
            node: '节点',
            source: '来源',
            endpoint: '出口',
            state: '状态',
            assignments: '绑定数',
            windows: '窗口统计',
            activity24h: '24 小时活动',
            weight24h: '24 小时权重',
          },
          weightLabel: '当前权重',
          primary: '主节点',
          secondary: '备用节点',
          successRateLabel: '成功率',
          latencyLabel: '平均延迟',
          successCountLabel: '成功数',
          failureCountLabel: '失败数',
          lastWeightLabel: '最新',
          avgWeightLabel: '平均',
          minMaxWeightLabel: '最小 / 最大',
        },
        states: {
          ready: '可用',
          readyHint: '可继续承接已绑定上游 Key 的流量。',
          penalized: '惩罚中',
          penalizedHint: '暂时降权，等待恢复探测通过后再提升。',
          direct: 'Direct',
          timeout: '超时',
          timeoutHint: '最近探测在超时前没有收到节点响应。',
          unreachable: '不可达',
          unreachableHint: '当前连接建立或传输层探测失败。',
          unavailable: '不可用',
          unavailableHint: '节点当前不在可选集合内。',
          xrayMissing: '缺少 Xray',
          xrayMissingHint: '分享链接解析成功，但本地 Xray 运行时不可用。',
        },
        sources: {
          manual: '手工',
          subscription: '订阅',
          direct: '直连',
          unknown: '未知',
        },
        windows: {
          oneMinute: '1 分钟',
          fifteenMinutes: '15 分钟',
          oneHour: '1 小时',
          oneDay: '1 天',
          sevenDays: '7 天',
        },
      },
      systemSettings: {
        title: '系统设置',
        description: '调整系统级 MCP session 亲和策略，并且无需重启即可生效。',
        helpLabel: '显示系统设置说明',
        form: {
          title: 'MCP session 亲和池',
          description: '新的 MCP initialize 请求会先命中每个用户稳定的上游 key 池，再在池内按活跃 session 数做均衡。',
          countLabel: '亲和 key 数量',
          countHint: '每个用户最多亲和这么多个上游 key。数量或可用 key 变化时，会通过稳定 top-N 排序尽量减少迁移。',
          currentValue: '当前值：{count}',
          applyScopeHint: '设置会立即作用于新创建的 MCP session；已有 session 会继续保持当前上游 key。',
          invalidCount: '请输入 1 到 1000 之间的整数。',
          saveFailed: '保存系统设置失败。',
        },
        actions: {
          apply: '应用设置',
          applying: '应用中…',
        },
      },
      users: {
        title: '用户管理',
        description: '查看账户层统计、用户标签叠加与共享额度设置。',
        registration: {
          title: '允许注册',
          description: '作用于首次 Linux DO 登录。',
          enabled: '已允许新用户登录。',
          disabled: '已暂停新用户登录。',
          unavailable: '注册策略不可用。',
          saving: '保存中…',
          loadFailed: '加载注册策略失败。',
          saveFailed: '保存注册策略失败。',
        },
        searchPlaceholder: '按用户 ID、显示名、用户名或标签搜索',
        search: '搜索',
        clear: '清空',
        pagination: '第 {page}/{total} 页',
        table: {
          user: '用户',
          displayName: '显示名',
          username: '用户名',
          status: '状态',
          tokenCount: '令牌数',
          tags: '标签',
          hourlyAny: '1h（任意）',
          hourly: '1h（业务）',
          daily: '24h',
          monthly: '月度',
          successDaily: '日成功/失败',
          successMonthly: '月成功/失败',
          lastActivity: '最近活动',
          lastLogin: '最近登录',
          actions: '操作',
        },
        status: {
          active: '活跃',
          inactive: '未激活',
          enabled: '启用',
          disabled: '禁用',
          unknown: '未知',
        },
        actions: {
          view: '查看详情',
        },
        usage: {
          title: '用户用量',
          description: '专门查看各用户的滚动额度、成功率、令牌数量与最近使用时间。',
          open: '查看用量界面',
          back: '返回用户管理',
          table: {
            user: '用户',
            status: '状态',
            hourlyAny: '1h（任意）',
            hourly: '1h（业务）',
            daily: '24h',
            monthly: '月度',
            monthlyBroken: '月蹬坏',
            dailySuccessRate: '日成功率',
            monthlySuccessRate: '月成功率',
            lastUsed: '上次使用',
          },
        },
        empty: {
          loading: '正在加载用户…',
          none: '暂无匹配用户。',
          notFound: '未找到该用户。',
          noTokens: '该用户暂无绑定令牌。',
        },
        detail: {
          title: '用户详情',
          subtitle: '账户 {id}',
          back: '返回用户列表',
          userId: '用户 ID',
          identityTitle: '身份信息',
          identityDescription: '查看该账户的稳定标识、登录状态与基础归属。',
          tokensTitle: '令牌列表',
          tokensDescription: '该账户下所有令牌共享同一份有效额度。',
        },
        brokenKeys: {
          limitTitle: '月蹬坏限额',
          limitDescription: '只统计本月蹬坏且当前仍不可用的上游 Key。',
          limitField: '月蹬坏限额',
          hint: '这个限额只作用于月蹬坏统计，不影响业务额度。',
          save: '保存月蹬坏限额',
          saving: '保存中…',
          savedAt: '已于 {time} 保存',
          invalid: '月蹬坏限额必须是非负整数。',
          saveFailed: '保存月蹬坏限额失败。',
          openAction: '查看月蹬坏明细',
          openDetails: '查看 {label} 的月蹬坏明细',
          drawerTitle: '月蹬坏明细',
          drawerDescription: '当前计入 {label} 的上游 Key。',
          loading: '正在加载月蹬坏明细…',
          empty: '当前没有计入的月蹬坏上游 Key。',
          noReason: '没有记录原因。',
          noRelatedUsers: '没有关联用户',
          breakerSystem: '系统',
          breakerUnknown: '未知',
          table: {
            key: '上游 Key',
            status: '当前状态',
            reason: '不可用原因',
            latestBreakAt: '最后蹬坏时间',
            breaker: '最后蹬坏者',
            relatedUsers: '关联用户',
          },
          actions: {
            copyKeyId: '复制 Key ID',
            copied: '已复制',
          },
        },
        quota: {
          title: '基础额度',
          description: '这里只编辑用户基线额度，标签增减会在下方有效额度里叠加显示。',
          hourlyAny: '每小时任意请求限额',
          hourly: '每小时业务请求限额',
          daily: '每日业务请求限额',
          monthly: '每月业务请求限额',
          hint: '基础额度只接受非负整数。',
          save: '保存基础额度',
          saving: '保存中…',
          savedAt: '已于 {time} 保存',
          invalid: '所有基础额度字段必须为非负整数。',
          saveFailed: '保存基础额度失败。',
          inheritsDefaults: '跟随默认值',
          customized: '已自定义基线',
        },
        catalog: {
          title: '标签目录',
          description: '查看可复用标签、绑定账户数，以及对应的额度效果。',
          summaryTitle: '标签账户统计',
          summaryDescription: '这里只展示每个标签当前绑定了多少个账户；创建、编辑、删除都在独立标签管理页完成。',
          summaryEmpty: '当前还没有任何用户标签。',
          summaryAccounts: '个账户',
          loading: '正在加载标签目录…',
          empty: '当前还没有可用标签。',
          invalid: '请补全必填字段，并确保额度增减填写为整数。',
          loadFailed: '加载标签目录失败。',
          saveFailed: '保存标签失败。',
          deleteFailed: '删除标签失败。',
          formCreateTitle: '创建标签',
          formEditTitle: '编辑标签',
          formDescription: '自定义标签可用于分组、叠加额度、扣减额度，或直接封禁全部额度。',
          systemReadonly: '系统标签的名称与图标保持锁定，只允许调整额度效果。',
          iconPlaceholder: '填 linuxdo 或留空',
          iconHint: '填入 `linuxdo` 时会渲染本地 LinuxDo 图标。',
          scopeSystem: '系统',
          scopeSystemShort: '系统',
          scopeCustom: '自定义',
          blockShort: '拉黑',
          blockDescription: 'block_all 会把所有有效额度维度统一钳制到 0，并参与真实限流。',
          deleteConfirm: '确认删除标签“{name}”？现有绑定也会一起移除。',
          deleteDialogTitle: '删除标签',
          deleteDialogCancel: '取消',
          deleteDialogConfirm: '删除',
          backToUsers: '返回用户管理',
          backToList: '返回标签列表',
          tagNotFound: '未找到该标签。',
          columns: {
            tag: '标签',
            scope: '范围',
            effect: '效果',
            delta: '额度增减',
            users: '绑定用户',
            actions: '操作',
          },
          fields: {
            name: '标签名',
            displayName: '显示名称',
            icon: '图标键',
            effect: '效果类型',
            hourlyAny: '任意请求小时增减',
            hourly: '业务小时增减',
            daily: '日额度增减',
            monthly: '月额度增减',
          },
          effectKinds: {
            quotaDelta: '额度增减',
            blockAll: '全部封禁',
          },
          actions: {
            create: '新建标签',
            save: '保存标签',
            saving: '保存中…',
            cancelEdit: '取消编辑',
            edit: '编辑标签',
            delete: '删除标签',
          },
        },
        userTags: {
          title: '用户标签',
          description: '把可复用标签绑定到当前用户；系统同步标签保持只读。',
          empty: '当前用户还没有绑定任何标签。',
          bindPlaceholder: '选择一个自定义标签后绑定',
          bindAction: '绑定标签',
          binding: '绑定中…',
          unbindAction: '解绑',
          bindFailed: '绑定标签失败。',
          unbindFailed: '解绑标签失败。',
          readOnly: '只读',
          sourceSystem: '系统同步',
          sourceManual: '手动绑定',
          manageCatalog: '管理标签目录',
        },
        effectiveQuota: {
          title: '有效额度拆解',
          description: '有效额度 = 基础额度 + 所有标签增减，然后统一按 0 做下限钳制。',
          blockAllNotice: '当前存在 block_all 标签，这个用户的所有有效额度都会被钳制为 0。',
          baseLabel: '基础额度',
          effectiveLabel: '最终有效额度',
          columns: {
            item: '项目',
            source: '来源',
            effect: '效果',
          },
        },
        tokens: {
          table: {
            id: '令牌 ID',
            note: '备注',
            status: '状态',
            hourlyAny: '1h（任意）',
            hourly: '1h（业务）',
            daily: '24h',
            monthly: '月度',
            successDaily: '日成功/失败',
            successMonthly: '月成功',
            lastUsed: '最近使用',
            actions: '操作',
          },
          actions: {
            view: '查看令牌详情',
          },
        },
      },
      accessibility: {
        skipToContent: '跳转到主内容',
      },
      tokens: {
        title: '访问令牌',
        description: '用于 /mcp 的认证，格式 th-xxxx-xxxxxxxxxxxx',
        notePlaceholder: '备注（可选）',
        newToken: '新建令牌',
        creating: '创建中…',
        batchCreate: '批量创建',
        pagination: {
          prev: '上一页',
          next: '下一页',
          page: '第 {page}/{total} 页',
        },
        table: {
          id: 'ID',
          note: '备注',
          owner: '关联用户',
          usage: '用量',
          quota: '限额状态',
          lastUsed: '最近使用',
          actions: '操作',
        },
        empty: {
          loading: '正在加载令牌…',
          none: '暂时没有令牌。',
        },
        owner: {
          label: '关联用户',
          unbound: '未关联用户',
        },
        actions: {
          copy: '复制完整令牌',
          share: '复制分享链接',
          disable: '禁用令牌',
          enable: '启用令牌',
          edit: '修改备注',
          delete: '删除令牌',
          viewLeaderboard: '查看未关联令牌用量',
        },
        statusBadges: {
          disabled: '已禁用的令牌',
        },
        quotaStates: {
          normal: '正常',
          hour: '一小时受限',
          day: '24 小时受限',
          month: '本月受限',
        },
        dialogs: {
          delete: {
            title: '删除令牌',
            description: '此操作将永久移除该访问令牌，正在使用它的客户端会收到 401。',
            cancel: '取消',
            confirm: '删除',
          },
          note: {
            title: '编辑令牌备注',
            placeholder: '备注',
            cancel: '取消',
            confirm: '保存',
            saving: '保存中…',
          },
        },
        batchDialog: {
          title: '批量创建令牌',
          groupPlaceholder: '分组名（必填）',
          confirm: '创建',
          creating: '创建中…',
          cancel: '取消',
          done: '完成',
          createdN: '已创建 {n} 个令牌',
          copyAll: '复制全部链接',
        },
        groups: {
          label: '分组',
          all: '全部',
          ungrouped: '未分组',
          moreShow: '展开全部分组',
          moreHide: '收起分组',
        },
      },
      unboundTokenUsage: {
        title: '未关联 Token 用量',
        description: '这里只显示没有关联用户的 token，字段和管理员用户用量列表保持一致。',
        searchPlaceholder: '按令牌 ID、备注或分组搜索',
        error: '无法加载未关联 Token 用量',
        table: {
          identity: '身份',
          status: '状态',
          hourlyAny: '1h（任意）',
          hourly: '1h（业务）',
          daily: '24h',
          monthly: '月度',
          monthlyBroken: '月蹬坏',
          dailySuccessRate: '日成功率',
          monthlySuccessRate: '月成功率',
          lastUsed: '上次使用',
        },
        empty: {
          loading: '正在加载未关联 Token 用量…',
          none: '目前还没有未关联 token 的活动记录。',
        },
        back: '返回令牌列表',
      },
      metrics: {
        labels: {
          total: '总请求数',
          success: '成功',
          failure: '失败',
          errors: '错误',
          quota: '额度耗尽',
          unknownCalls: '未知调用',
          newKeys: '新增密钥',
          newQuarantines: '新增隔离密钥',
          keys: '活跃密钥',
          quarantined: '隔离中',
          exhausted: '已耗尽',
          remaining: '剩余可用',
          proxyAvailable: '可用代理节点',
          proxyTotal: '代理节点总数',
        },
        subtitles: {
          keysAll: '全部可用',
          keysExhausted: '{count} 个耗尽',
          keysAvailability: '{active} 个可用 · {quarantined} 个隔离中 · {exhausted} 个耗尽',
        },
        loading: '正在加载最新指标…',
      },
      keys: {
        title: 'API Keys',
        description: '查看每个 Tavily API Key 的状态、用量和成功率。',
        placeholder: '输入新的 Tavily API Key',
        addButton: '添加密钥',
        adding: '添加中…',
        batch: {
          placeholder: '粘贴文本（每行提取首个 tvly-dev-* key 和公网 IP）',
          groupPlaceholder: '分组名（可选）',
          hint: '每行提取首个 tvly-dev-* key 和首个公网 IP；地区解析会访问已配置的兼容 country.is 服务。',
          count: '可提取 key {count}',
          report: {
            title: '批量导入结果',
            close: '完成',
            summary: {
              inputLines: '输入行数',
              validLines: '有效行数',
              uniqueInInput: '输入去重后',
              created: '新增',
              undeleted: '恢复',
              existed: '已存在',
              duplicateInInput: '输入重复',
              failed: '失败',
            },
            failures: {
              title: '失败明细',
              none: '没有失败项。',
              table: {
                apiKey: 'API Key',
                error: '原因',
              },
            },
          },
        },
        validation: {
          title: '检测 API Keys',
          hint: '支持粘贴文本：每行先提取首个 tvly-dev-* key 和首个公网 IP，再检测并入库；地区解析会访问已配置的兼容 country.is 服务。',
          registrationIpBadge: 'IP',
          registrationIpTooltip: '注册 IP：{ip}',
          actions: {
            close: '关闭',
            retry: '重试',
            retryFailed: '重试失败项',
            import: '导入',
            importValid: '入库 {count} 个可用 key',
            imported: '已入库',
          },
          import: {
            title: '入库结果',
            exhaustedMarkFailed: '{count} 个已耗尽 key 未能标记为已耗尽',
          },
          summary: {
            group: '分组：{group}',
            inputLines: '输入行数',
            validLines: '有效行数',
            uniqueInInput: '输入去重后',
            duplicateInInput: '输入重复',
            checked: '已检测 {checked} / {total}',
            ok: '可用',
            exhausted: '已耗尽',
            exhaustedNote: '{count} 个 key 将以已耗尽状态入库',
            invalid: '不可用',
            error: '错误',
          },
          emptyFiltered: '没有符合当前筛选状态的记录。',
          table: {
            apiKey: 'API Key',
            result: '结果',
            quota: '额度',
            actions: '操作',
          },
          statuses: {
            pending: '检测中',
            duplicate_in_input: '输入重复',
            ok: '可用',
            ok_exhausted: '可用（已耗尽）',
            unauthorized: '未授权',
            forbidden: '禁止',
            invalid: '不可用',
            error: '错误',
          },
        },
        groups: {
          label: '分组',
          all: '全部',
          ungrouped: '未分组',
          moreShow: '展开全部分组',
          moreHide: '收起分组',
        },
        filters: {
          status: '状态',
          region: '地区',
          registrationIp: '注册 IP',
          registrationIpPlaceholder: '按完整 IP 筛选',
          clearGroups: '显示全部分组',
          clearStatuses: '显示全部状态',
          clearRegistrationIp: '清空 IP',
          clearRegions: '显示全部地区',
          selectedSuffix: '项已选',
        },
        selection: {
          selectRow: '选择该密钥',
          selectAll: '勾选当前页全部密钥',
          selectCurrentPage: '勾选当前页',
          clear: '清空选择',
          selectedCount: '已选 {count} 项',
        },
        pagination: {
          page: '第 {page}/{total} 页',
          perPage: '每页',
        },
        table: {
          keyId: 'Key ID',
          registration: '注册信息',
          registrationIp: '注册 IP',
          registrationRegion: '地区',
          assignedProxy: '分配代理',
          status: '状态',
          total: '总请求',
          success: '成功',
          errors: '错误',
          quota: '额度耗尽',
          successRate: '成功率',
          remainingPct: '剩余比例',
          quotaLeft: '剩余',
          syncedAt: '同步时间',
          lastUsed: '最近使用',
          statusChanged: '状态更新',
          actions: '操作',
        },
        empty: {
          loading: '正在加载密钥统计…',
          none: '暂时没有密钥数据。',
          filtered: '当前筛选条件下暂无密钥。',
        },
        actions: {
          copy: '复制原始 API Key',
          enable: '启用密钥',
          disable: '禁用密钥',
          clearQuarantine: '解除隔离',
          delete: '移除密钥',
          details: '查看详情',
        },
        bulkActions: {
          syncUsage: '同步额度',
          clearQuarantine: '解除隔离',
          delete: '批量移除',
          running: '执行中…',
          summary: '{action}：成功 {succeeded}，跳过 {skipped}，失败 {failed}（共 {requested} 项）',
        },
        bulkSyncProgress: {
          title: '同步额度进度',
          running: '正在按真实执行顺序回传每个密钥的同步结果。',
          finished: '手动同步已完成，可以查看下面的最新结果。',
          refreshingList: '正在刷新当前密钥列表，带回最新额度快照…',
          steps: {
            prepareRequest: '准备请求',
            syncUsage: '逐个同步密钥',
            refreshUi: '刷新当前列表',
          },
          status: {
            waiting: '等待中…',
            running: '进行中…',
            done: '已完成',
            failed: '失败',
          },
          counters: {
            progress: '已处理 {current}/{total}',
            success: '成功 {count}',
            skipped: '跳过 {count}',
            failed: '失败 {count}',
          },
          lastResultLabel: '最近结果',
          result: {
            success: '成功',
            skipped: '跳过',
            failed: '失败',
            noDetail: '服务端没有返回额外说明。',
          },
        },
        quarantine: {
          badge: '隔离中',
          sourcePrefix: '来源：{source}',
          noReason: '没有记录隔离原因。',
        },
        dialogs: {
          disable: {
            title: '禁用 API Key',
            description: '禁用后不会再使用该密钥，稍后可以重新启用，数据不会被删除。',
            cancel: '取消',
            confirm: '禁用',
          },
          delete: {
            title: '移除 API Key',
            description: '该密钥会被标记为 Deleted，稍后可以通过重新添加同一个密钥来恢复。',
            cancel: '取消',
            confirm: '移除',
          },
          bulkDelete: {
            title: '批量移除 API Keys',
            description: '已选密钥会被标记为 Deleted，稍后可以通过重新添加相同密钥恢复。',
            cancel: '取消',
            confirm: '批量移除',
          },
        },
      },
      logs: {
        title: '近期请求',
        description: '按时间倒序浏览近期请求。',
        descriptionFallback: '按时间倒序浏览近期请求。',
        descriptionWithRetention: '按时间倒序浏览近期请求。日志保留 {days} 天。',
        filters: {
          all: '全部',
          success: '成功',
          error: '错误',
          quota: '额度耗尽',
          requestType: '请求类型',
          requestTypeAll: 'All',
          requestTypeEmpty: '当前时间窗没有请求类型',
          billingGroup: '计费',
          protocolGroup: '协议',
          resultOrEffect: '结果与影响',
          resultOrEffectAll: 'All',
          resultGroup: '结果',
          keyEffectGroup: 'Key 影响',
          tokenAll: '全部 Token',
          keyAll: 'All',
          noFacetOptions: '当前没有可选项',
        },
        empty: {
          loading: '正在收集最新请求…',
          none: '尚未捕获请求日志。',
        },
        table: {
          key: 'Key',
          token: 'Token',
          time: '时间',
          requestType: '请求类型',
          status: '状态',
          chargedCredits: 'Credits',
          httpStatus: 'HTTP 状态码',
          mcpStatus: 'Tavily 状态',
          result: '结果',
          keyEffect: 'Key 影响',
          error: '错误',
        },
        toggles: {
          show: '展开请求详情',
          hide: '收起请求详情',
        },
        pagination: {
          summary: '使用较新 / 较旧翻页浏览近期请求。',
          summaryWithRetention: '使用较新 / 较旧翻页浏览近 {days} 天内保留的请求。',
          newer: '较新',
          older: '较旧',
        },
        errors: {
          quotaExhausted: '额度耗尽',
          quotaExhaustedHttp: '额度耗尽（HTTP {http}）',
          requestFailedHttpMcp: '请求失败（HTTP {http}，Tavily {mcp}）',
          requestFailedHttp: '请求失败（HTTP {http}）',
          requestFailedMcp: '请求失败（Tavily {mcp}）',
          requestFailedGeneric: '请求失败',
          httpStatus: 'HTTP {http}',
          none: '—',
        },
        keyEffects: {
          none: '无变更',
          quarantined: '已隔离',
          markedExhausted: '已耗尽',
          restoredActive: '已恢复',
          clearedQuarantine: '已清除',
          unknown: '已更新',
        },
      },
      jobs: {
        title: '计划任务',
        description: '后台计划任务与清理任务的最新执行记录。',
        filters: {
          all: '全部',
          quota: '同步额度',
          usage: '用量聚合',
          logs: '清理日志',
          geo: '节点 IP/GEO 刷新',
          linuxdo: 'LinuxDo 用户同步',
        },
        empty: {
          loading: '正在加载任务…',
          none: '暂无任务记录。',
        },
        table: {
          id: 'ID',
          type: '类型',
          key: 'Key',
          status: '状态',
          attempt: '重试次数',
          started: '开始时间',
          message: '消息',
        },
        toggles: {
          show: '展开任务详情',
          hide: '收起任务详情',
        },
        types: {
          quota_sync: '同步额度',
          'quota_sync/hot': '热 Key 同步',
          'quota_sync/manual': '手动同步',
          token_usage_rollup: '用量聚合',
          usage_aggregation: '用量聚合',
          auth_token_logs_gc: '日志清理',
          request_logs_gc: '日志清理',
          log_cleanup: '日志清理',
          forward_proxy_geo_refresh: '节点 IP/GEO 刷新',
          linuxdo_user_status_sync: 'LinuxDo 用户同步',
        },
      },
      statuses: {
        active: '活跃',
        quarantined: '隔离中',
        exhausted: '耗尽',
        success: '成功',
        running: '运行中',
        pending: '待处理',
        queued: '排队中',
        completed: '已完成',
        error: '错误',
        failed: '失败',
        retry_exhausted: '重试耗尽',
        neutral: '中性',
        quota_exhausted: '额度耗尽',
        timeout: '超时',
        cancelled: '已取消',
        deleted: '已删除',
        unknown: '未知',
      },
      logDetails: {
        request: '请求',
        response: '响应',
        outcome: '结果',
        keyEffect: 'Key 影响',
        requestTypeDetail: '请求类型详情',
        solution: '建议处理',
        requestBody: '请求体',
        responseBody: '响应体',
        noBody: '未捕获内容。',
        loadingBody: '正在加载请求详情…',
        loadBodyFailed: '加载请求详情失败。',
        retryLoadBody: '重试',
        noKeyEffect: '无自动状态变更',
        forwardedHeaders: '转发的 Header',
        droppedHeaders: '被丢弃的 Header',
      },
      keyDetails: {
        title: '密钥详情',
        descriptionPrefix: '查看该密钥的用量与近期请求：',
        back: '返回',
        syncAction: '同步额度',
        syncing: '同步中…',
        syncSuccess: '已同步',
        usageTitle: '用量',
        usageDescription: '按选择的时间范围聚合总数。',
        periodOptions: {
          day: '按天',
          week: '按周',
          month: '按月',
        },
        apply: '应用',
        loading: '加载中…',
        metrics: {
          total: '总请求',
          success: '成功',
          errors: '错误',
          quota: '额度耗尽',
          lastActivityPrefix: '最近活跃时间',
          noActivity: '暂无活跃记录',
        },
        quarantine: {
          title: '系统隔离',
          description: '该密钥当前已被移出轮转池，只有管理员手动解除后才会重新参与调度。',
          source: '来源',
          reason: '原因摘要',
          detail: '原始详情',
          showDetail: '展开原始详情',
          hideDetail: '收起原始详情',
          createdAt: '隔离时间',
          clearAction: '解除隔离',
          clearing: '解除中…',
        },
        metadata: {
          title: '注册信息',
          description: '这里展示导入时从原始账号行提取出的注册元数据。',
          group: '分组',
          registrationIp: '注册 IP',
          registrationRegion: '地区',
        },
        stickyUsers: {
          title: 'Sticky 用户',
          description: '当前仍绑定到这把密钥的用户，以及已记账 credits 视图。',
          empty: '当前没有用户 sticky 到这把密钥。',
          user: '用户',
          yesterday: '昨日',
          today: '今日',
          month: '本月',
          trend: '近 7 日',
          lastSuccess: '最近成功',
          success: '成功',
          failure: '失败',
          inactive: '停用',
        },
        stickyNodes: {
          title: 'Sticky 节点',
          description: '这把密钥当前的主备代理节点，与实时统计保持同款图表。',
          empty: '当前没有 sticky 节点。',
          role: '角色',
          node: '节点',
          activity: '24h 活动',
          weight: '24h 权重',
          primary: '主节点',
          secondary: '备节点',
          assignmentSummary: '主{primary} · 备{secondary}',
          primaryAssignments: '作为 {count} 个 key 的主节点',
          secondaryAssignments: '作为 {count} 个 key 的备节点',
          window: '窗口',
        },
        logsTitle: '近期请求',
        logsDescription: '按时间倒序浏览该密钥的近期请求。',
        logsEmpty: '该时间段内没有请求。',
      },
      errors: {
        copyKey: '复制 API Key 失败',
        addKey: '新增 API Key 失败',
        addKeysBatch: '批量添加 API Key 失败',
        createToken: '创建令牌失败',
        copyToken: '复制令牌失败',
        toggleToken: '更新令牌状态失败',
        deleteToken: '删除令牌失败',
        updateTokenNote: '更新令牌备注失败',
        deleteKey: '删除 API Key 失败',
        toggleKey: '更新密钥状态失败',
        clearQuarantine: '解除密钥隔离失败',
        loadKeyDetails: '加载详情失败',
        syncUsage: '同步额度失败',
      },
      footer: {
        title: 'Tavily Hikari 控制台',
        githubAria: '打开 GitHub 仓库',
        githubLabel: 'GitHub',
        loadingVersion: '· 正在读取版本…',
        tagPrefix: '· ',
      },
    },
  },
}

type LanguageOptionKey = 'englishLabel' | 'chineseLabel'

export const languageOptions: Array<{ value: Language; labelKey: LanguageOptionKey }> = [
  { value: 'en', labelKey: 'englishLabel' },
  { value: 'zh', labelKey: 'chineseLabel' },
]

export type Translations = TranslationShape
export type AdminTranslations = TranslationShape['admin']

export function LanguageProvider({
  children,
  initialLanguage,
}: {
  children: ReactNode
  initialLanguage?: Language
}): JSX.Element {
  const [language, setLanguageState] = useState<Language>(
    () => initialLanguage ?? readStoredLanguage() ?? detectBrowserLanguage() ?? DEFAULT_LANGUAGE,
  )

  const setLanguage = (next: Language) => {
    setLanguageState(next)
    persistLanguage(next)
  }

  const value = useMemo(
    () => ({
      language,
      setLanguage,
    }),
    [language],
  )

  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>
}

export function useLanguage(): LanguageContextValue {
  const context = useContext(LanguageContext)
  if (!context) {
    throw new Error('LanguageProvider is missing. Wrap your app with LanguageProvider.')
  }
  return context
}

export function useTranslate(): Translations {
  const { language } = useLanguage()
  return translations[language]
}

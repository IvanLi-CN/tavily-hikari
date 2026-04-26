export type Language = 'en' | 'zh'


export interface LanguageContextValue {
  language: Language
  setLanguage: (language: Language) => void
}

export interface PublicTranslations {
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

export interface AdminTranslationsShape {
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
    recentAlertsTitle: string
    recentAlertsDescription: string
    recentAlertsEvents: string
    recentAlertsGroups: string
    recentAlertsEmpty: string
    recentAlertsOpen: string
    recentAlertsTypeLabels: {
      upstream_rate_limited_429: string
      upstream_usage_limit_432: string
      upstream_key_blocked: string
      user_request_rate_limited: string
      user_quota_exhausted: string
    }
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
      requestRateLimitLabel: string
      requestRateLimitHint: string
      currentRequestRateLimitValue: string
      countLabel: string
      countHint: string
      currentValue: string
      rebalanceLabel: string
      rebalanceHint: string
      percentLabel: string
      percentHint: string
      percentDisabledHint: string
      currentPercentValue: string
      blockedKeyBaseLimitLabel: string
      blockedKeyBaseLimitHint: string
      currentBlockedKeyBaseLimitValue: string
      applyScopeHint: string
      invalidRequestRateLimit: string
      invalidCount: string
      invalidPercent: string
      invalidBlockedKeyBaseLimit: string
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
      sharedUsageTitle: string
      sharedUsageDescription: string
      sharedUsageLoading: string
      sharedUsageLoadFailed: string
      sharedUsageRetryAction: string
      sharedUsageEmpty: string
      sharedUsagePartialHint: string
      sharedUsageLegendUsed: string
      sharedUsageLegendLimit: string
      sharedUsageTabs: {
        oneHour: string
        fiveMinute: string
        daily: string
        monthly: string
      }
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
        totalRequests: string
        createdAt: string
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
      bindingEffectGroup: string
      selectionEffectGroup: string
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
      effects: string
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
      mcpSessionInitBackoffSet: string
      mcpSessionRetryWaited: string
      mcpSessionRetryScheduled: string
      unknown: string
    }
    bindingEffects: {
      none: string
      bound: string
      reused: string
      rebound: string
      unknown: string
    }
    selectionEffects: {
      none: string
      mcpSessionInitCooldownAvoided: string
      mcpSessionInitRateLimitAvoided: string
      mcpSessionInitPressureAvoided: string
      httpProjectAffinityCooldownAvoided: string
      httpProjectAffinityRateLimitAvoided: string
      httpProjectAffinityPressureAvoided: string
      unknown: string
    }
  }
  statuses: Record<string, string>
  logDetails: {
    request: string
    response: string
    outcome: string
    keyEffect: string
    bindingEffect: string
    selectionEffect: string
    gatewayMode: string
    experimentVariant: string
    proxySessionId: string
    routingSubjectHash: string
    upstreamOperation: string
    fallbackReason: string
    requestTypeDetail: string
    solution: string
    requestBody: string
    responseBody: string
    noBody: string
    loadingBody: string
    loadBodyFailed: string
    retryLoadBody: string
    noKeyEffect: string
    noBindingEffect: string
    noSelectionEffect: string
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

export interface TranslationShape {
  common: {
    languageLabel: string
    englishLabel: string
    chineseLabel: string
  }
  public: PublicTranslations
  admin: AdminTranslationsShape
}

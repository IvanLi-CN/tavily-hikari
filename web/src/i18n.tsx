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
  accessPanel: {
    title: string
    stats: {
      dailySuccess: string
      dailyFailure: string
      monthlySuccess: string
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
  guide: {
    title: string
    dataSourceLabel: string
    tabs: Record<string, string>
  }
  footer: {
    version: string
  }
  errors: {
    metrics: string
    summary: string
  }
}

interface AdminTranslationsShape {
  header: {
    title: string
    subtitle: string
    updatedPrefix: string
    refreshNow: string
    refreshing: string
  }
  tokens: {
    title: string
    description: string
    notePlaceholder: string
    newToken: string
    creating: string
    table: {
      id: string
      note: string
      usage: string
      lastUsed: string
      actions: string
    }
    empty: {
      loading: string
      none: string
    }
    actions: {
      copy: string
      share: string
      disable: string
      enable: string
      edit: string
      delete: string
    }
    statusBadges: {
      disabled: string
    }
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
  }
  metrics: {
    labels: {
      total: string
      success: string
      errors: string
      quota: string
      keys: string
    }
    subtitles: {
      keysAll: string
      keysExhausted: string
    }
    loading: string
  }
  keys: {
    title: string
    description: string
    placeholder: string
    addButton: string
    adding: string
    table: {
      keyId: string
      status: string
      total: string
      success: string
      errors: string
      quota: string
      successRate: string
      lastUsed: string
      statusChanged: string
      actions: string
    }
    empty: {
      loading: string
      none: string
    }
    actions: {
      copy: string
      enable: string
      disable: string
      delete: string
      details: string
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
    }
  }
  logs: {
    title: string
    description: string
    empty: {
      loading: string
      none: string
    }
    table: {
      time: string
      key: string
      httpStatus: string
      mcpStatus: string
      result: string
      error: string
    }
    toggles: {
      show: string
      hide: string
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
  }
  statuses: Record<string, string>
  logDetails: {
    request: string
    response: string
    outcome: string
    requestBody: string
    responseBody: string
    noBody: string
    forwardedHeaders: string
    droppedHeaders: string
  }
  keyDetails: {
    title: string
    descriptionPrefix: string
    back: string
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
    logsTitle: string
    logsDescription: string
    logsEmpty: string
  }
  errors: {
    copyKey: string
    addKey: string
    createToken: string
    copyToken: string
    toggleToken: string
    deleteToken: string
    updateTokenNote: string
    deleteKey: string
    toggleKey: string
    loadKeyDetails: string
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
          title: 'Today (server timezone)',
          subtitle: 'Successful requests since the server midnight',
        },
        pool: {
          title: 'Key Pool Status',
          subtitle: 'Active Tavily keys / total keys (including exhausted)',
        },
      },
      adminButton: 'Open Admin Dashboard',
      accessPanel: {
        title: 'Token Usage',
        stats: {
          dailySuccess: 'Daily Success',
          dailyFailure: 'Daily Failure',
          monthlySuccess: 'Monthly Success',
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
      guide: {
        title: 'Connect Tavily Hikari to common MCP clients',
        dataSourceLabel: 'Reference: ',
        tabs: {
          codex: 'Codex CLI',
          claude: 'Claude Code CLI',
          vscode: 'VS Code / Copilot',
          claudeDesktop: 'Claude Desktop',
          cursor: 'Cursor',
          windsurf: 'Windsurf',
          other: 'Other clients',
        },
      },
      footer: {
        version: 'Current version: ',
      },
      errors: {
        metrics: 'Unable to load metrics right now',
        summary: 'Unable to load summary data',
      },
    },
    admin: {
      header: {
        title: 'Tavily Hikari Overview',
        subtitle: 'Monitor API key allocation, quota health, and recent proxy activity.',
        updatedPrefix: 'Updated',
        refreshNow: 'Refresh Now',
        refreshing: 'Refreshing…',
      },
      tokens: {
        title: 'Access Tokens',
        description: 'Auth for /mcp. Format th-xxxx-xxxxxxxxxxxx',
        notePlaceholder: 'Note (optional)',
        newToken: 'New Token',
        creating: 'Creating…',
        table: {
          id: 'ID',
          note: 'Note',
          usage: 'Usage',
          lastUsed: 'Last Used',
          actions: 'Actions',
        },
        empty: {
          loading: 'Loading tokens…',
          none: 'No tokens yet.',
        },
        actions: {
          copy: 'Copy full token',
          share: 'Copy share link',
          disable: 'Disable token',
          enable: 'Enable token',
          edit: 'Edit note',
          delete: 'Delete token',
        },
        statusBadges: {
          disabled: 'Disabled token',
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
      },
      metrics: {
        labels: {
          total: 'Total Requests',
          success: 'Successful',
          errors: 'Errors',
          quota: 'Quota Exhausted',
          keys: 'Active Keys',
        },
        subtitles: {
          keysAll: 'All keys available',
          keysExhausted: '{count} exhausted',
        },
        loading: 'Loading latest metrics…',
      },
      keys: {
        title: 'API Keys',
        description: 'Status, usage, and recent success rates per Tavily API key.',
        placeholder: 'New Tavily API Key',
        addButton: 'Add Key',
        adding: 'Adding…',
        table: {
          keyId: 'Key ID',
          status: 'Status',
          total: 'Total',
          success: 'Success',
          errors: 'Errors',
          quota: 'Quota Exhausted',
          successRate: 'Success Rate',
          lastUsed: 'Last Used',
          statusChanged: 'Status Changed',
          actions: 'Actions',
        },
        empty: {
          loading: 'Loading key statistics…',
          none: 'No key data recorded yet.',
        },
        actions: {
          copy: 'Copy original API key',
          enable: 'Enable key',
          disable: 'Disable key',
          delete: 'Remove key',
          details: 'Details',
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
        },
      },
      logs: {
        title: 'Recent Requests',
        description: 'Up to the latest 50 invocations handled by the proxy.',
        empty: {
          loading: 'Collecting recent requests…',
          none: 'No request logs captured yet.',
        },
        table: {
          time: 'Time',
          key: 'Key',
          httpStatus: 'HTTP Status',
          mcpStatus: 'MCP Status',
          result: 'Result',
          error: 'Error',
        },
        toggles: {
          show: 'Show request details',
          hide: 'Hide request details',
        },
        errors: {
          quotaExhausted: 'Quota exhausted',
          quotaExhaustedHttp: 'Quota exhausted (HTTP {http})',
          requestFailedHttpMcp: 'Request failed (HTTP {http}, MCP {mcp})',
          requestFailedHttp: 'Request failed (HTTP {http})',
          requestFailedMcp: 'Request failed (MCP {mcp})',
          requestFailedGeneric: 'Request failed',
          httpStatus: 'HTTP {http}',
          none: '—',
        },
      },
      statuses: {
        active: 'Active',
        exhausted: 'Exhausted',
        success: 'Success',
        error: 'Error',
        quota_exhausted: 'Quota Exhausted',
        deleted: 'Deleted',
        unknown: 'Unknown',
      },
      logDetails: {
        request: 'Request',
        response: 'Response',
        outcome: 'Outcome',
        requestBody: 'Request Body',
        responseBody: 'Response Body',
        noBody: 'No body captured.',
        forwardedHeaders: 'Forwarded Headers',
        droppedHeaders: 'Dropped Headers',
      },
      keyDetails: {
        title: 'Key Details',
        descriptionPrefix: 'Inspect usage and recent requests for key:',
        back: 'Back',
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
        logsTitle: 'Recent Requests',
        logsDescription: 'Up to the latest 50 for this key.',
        logsEmpty: 'No request logs for this period.',
      },
      errors: {
        copyKey: 'Failed to copy API key',
        addKey: 'Failed to add API key',
        createToken: 'Failed to create token',
        copyToken: 'Failed to copy token',
        toggleToken: 'Failed to update token status',
        deleteToken: 'Failed to delete token',
        updateTokenNote: 'Failed to update token note',
        deleteKey: 'Failed to delete API key',
        toggleKey: 'Failed to update key status',
        loadKeyDetails: 'Failed to load details',
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
          title: '今日（服务器时区）',
          subtitle: '从服务器午夜起累计的成功请求',
        },
        pool: {
          title: '号池可用数',
          subtitle: '活跃 Tavily API Key / 总密钥（含本月耗尽）',
        },
      },
      adminButton: '打开管理员面板',
      accessPanel: {
        title: '令牌使用统计',
        stats: {
          dailySuccess: '今日成功',
          dailyFailure: '今日失败',
          monthlySuccess: '本月成功',
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
      guide: {
        title: '如何在常见 MCP 客户端接入 Tavily Hikari',
        dataSourceLabel: '数据来源：',
        tabs: {
          codex: 'Codex CLI',
          claude: 'Claude Code CLI',
          vscode: 'VS Code / Copilot',
          claudeDesktop: 'Claude Desktop',
          cursor: 'Cursor',
          windsurf: 'Windsurf',
          other: '其他客户端',
        },
      },
      footer: {
        version: '当前版本：',
      },
      errors: {
        metrics: '暂时无法加载指标',
        summary: '暂时无法加载摘要数据',
      },
    },
    admin: {
      header: {
        title: 'Tavily Hikari 总览',
        subtitle: '监控 API Key 分配、额度健康度与最新代理请求活动。',
        updatedPrefix: '更新于',
        refreshNow: '立即刷新',
        refreshing: '刷新中…',
      },
      tokens: {
        title: '访问令牌',
        description: '用于 /mcp 的认证，格式 th-xxxx-xxxxxxxxxxxx',
        notePlaceholder: '备注（可选）',
        newToken: '新建令牌',
        creating: '创建中…',
        table: {
          id: 'ID',
          note: '备注',
          usage: '用量',
          lastUsed: '最近使用',
          actions: '操作',
        },
        empty: {
          loading: '正在加载令牌…',
          none: '暂时没有令牌。',
        },
        actions: {
          copy: '复制完整令牌',
          share: '复制分享链接',
          disable: '禁用令牌',
          enable: '启用令牌',
          edit: '修改备注',
          delete: '删除令牌',
        },
        statusBadges: {
          disabled: '已禁用的令牌',
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
      },
      metrics: {
        labels: {
          total: '总请求数',
          success: '成功',
          errors: '错误',
          quota: '额度耗尽',
          keys: '活跃密钥',
        },
        subtitles: {
          keysAll: '全部可用',
          keysExhausted: '{count} 个耗尽',
        },
        loading: '正在加载最新指标…',
      },
      keys: {
        title: 'API Keys',
        description: '查看每个 Tavily API Key 的状态、用量和成功率。',
        placeholder: '输入新的 Tavily API Key',
        addButton: '添加密钥',
        adding: '添加中…',
        table: {
          keyId: 'Key ID',
          status: '状态',
          total: '总请求',
          success: '成功',
          errors: '错误',
          quota: '额度耗尽',
          successRate: '成功率',
          lastUsed: '最近使用',
          statusChanged: '状态更新',
          actions: '操作',
        },
        empty: {
          loading: '正在加载密钥统计…',
          none: '暂时没有密钥数据。',
        },
        actions: {
          copy: '复制原始 API Key',
          enable: '启用密钥',
          disable: '禁用密钥',
          delete: '移除密钥',
          details: '查看详情',
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
        },
      },
      logs: {
        title: '近期请求',
        description: '展示代理最近处理的最多 50 条调用记录。',
        empty: {
          loading: '正在收集最新请求…',
          none: '尚未捕获请求日志。',
        },
        table: {
          time: '时间',
          key: 'Key',
          httpStatus: 'HTTP 状态码',
          mcpStatus: 'MCP 状态码',
          result: '结果',
          error: '错误',
        },
        toggles: {
          show: '展开请求详情',
          hide: '收起请求详情',
        },
        errors: {
          quotaExhausted: '额度耗尽',
          quotaExhaustedHttp: '额度耗尽（HTTP {http}）',
          requestFailedHttpMcp: '请求失败（HTTP {http}，MCP {mcp}）',
          requestFailedHttp: '请求失败（HTTP {http}）',
          requestFailedMcp: '请求失败（MCP {mcp}）',
          requestFailedGeneric: '请求失败',
          httpStatus: 'HTTP {http}',
          none: '—',
        },
      },
      statuses: {
        active: '活跃',
        exhausted: '耗尽',
        success: '成功',
        error: '错误',
        quota_exhausted: '额度耗尽',
        deleted: '已删除',
        unknown: '未知',
      },
      logDetails: {
        request: '请求',
        response: '响应',
        outcome: '结果',
        requestBody: '请求体',
        responseBody: '响应体',
        noBody: '未捕获内容。',
        forwardedHeaders: '转发的 Header',
        droppedHeaders: '被丢弃的 Header',
      },
      keyDetails: {
        title: '密钥详情',
        descriptionPrefix: '查看该密钥的用量与近期请求：',
        back: '返回',
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
        logsTitle: '近期请求',
        logsDescription: '最多展示该密钥的 50 条请求。',
        logsEmpty: '该时间段内没有请求。',
      },
      errors: {
        copyKey: '复制 API Key 失败',
        addKey: '新增 API Key 失败',
        createToken: '创建令牌失败',
        copyToken: '复制令牌失败',
        toggleToken: '更新令牌状态失败',
        deleteToken: '删除令牌失败',
        updateTokenNote: '更新令牌备注失败',
        deleteKey: '删除 API Key 失败',
        toggleKey: '更新密钥状态失败',
        loadKeyDetails: '加载详情失败',
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

export function LanguageProvider({ children }: { children: ReactNode }): JSX.Element {
  const [language, setLanguageState] = useState<Language>(() => readStoredLanguage() ?? DEFAULT_LANGUAGE)

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

import { describe, expect, it } from 'bun:test'

import { ZH } from '../i18n/translations/zh'
import { adminJobTypeLabel, countAdminJobGroups, jobMatchesGroup, MANUAL_JOB_ACTIONS } from './jobFilters'
import type { JobLogView } from '../api'

describe('admin job filters', () => {
  it('groups LinuxDo tag refresh jobs with LinuxDo maintenance', () => {
    expect(jobMatchesGroup('linuxdo_user_tag_binding_refresh', 'linuxdo')).toBe(true)

    const jobs: JobLogView[] = [
      {
        id: 1,
        job_type: 'linuxdo_user_tag_binding_refresh',
        trigger_source: 'manual',
        key_id: null,
        key_group: null,
        status: 'success',
        attempt: 1,
        message: null,
        started_at: 1,
        finished_at: 2,
      },
    ]

    expect(countAdminJobGroups(jobs).linuxdo).toBe(1)
  })

  it('groups MCP cleanup jobs with log maintenance', () => {
    expect(jobMatchesGroup('mcp_sessions_gc', 'logs')).toBe(true)
    expect(jobMatchesGroup('mcp_session_init_backoffs_gc', 'logs')).toBe(true)

    const jobs: JobLogView[] = [
      {
        id: 1,
        job_type: 'mcp_sessions_gc',
        trigger_source: 'scheduler',
        key_id: null,
        key_group: null,
        status: 'success',
        attempt: 1,
        message: null,
        started_at: 1,
        finished_at: 2,
      },
    ]

    expect(countAdminJobGroups(jobs).logs).toBe(1)
  })

  it('labels manual jobs distinctly in Chinese without leaking raw job types', () => {
    const labels = MANUAL_JOB_ACTIONS.map((jobType) => adminJobTypeLabel(jobType, ZH.admin.jobs))

    expect(labels).toContain('访问令牌日志清理')
    expect(labels).toContain('请求日志清理')
    expect(labels).toContain('MCP 会话清理')
    expect(labels).toContain('MCP 会话初始化退避清理')
    expect(new Set(labels).size).toBe(labels.length)
    expect(labels.some((label) => label.includes('_gc'))).toBe(false)
  })

  it('keeps the legacy log cleanup label generic', () => {
    expect(adminJobTypeLabel('log_cleanup', ZH.admin.jobs)).toBe('日志清理')
  })
})

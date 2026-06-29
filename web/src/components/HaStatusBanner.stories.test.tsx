import '../../test/happydom'

import { describe, expect, it } from 'bun:test'
import { act } from 'react'
import { createElement } from 'react'
import { createRoot } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'

import meta, * as stories from './HaStatusBanner.stories'
import { LanguageProvider, translations } from '../i18n'
import { ThemeProvider } from '../theme'

async function renderIntoDom(element: JSX.Element): Promise<string> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  await act(async () => {
    root.render(createElement(LanguageProvider, { initialLanguage: 'zh' }, createElement(ThemeProvider, null, element)))
  })
  const text = container.textContent ?? ''
  await act(async () => {
    root.unmount()
  })
  container.remove()
  return text
}

async function renderNodeOrigins(element: JSX.Element): Promise<string[]> {
  const container = document.createElement('div')
  document.body.appendChild(container)
  const root = createRoot(container)
  await act(async () => {
    root.render(createElement(LanguageProvider, { initialLanguage: 'zh' }, createElement(ThemeProvider, null, element)))
  })
  const origins = Array.from(
    container.querySelectorAll('.ha-node-cell--origin code'),
    (node) => node.textContent ?? '',
  )
  await act(async () => {
    root.unmount()
  })
  container.remove()
  return origins
}

describe('HaStatusBanner Storybook proofs', () => {
  it('keeps the HA node list story and local promote entry available', () => {
    expect(meta).toMatchObject({
      title: 'Components/HaStatusBanner',
    })
    expect(stories.NodeListGallery).toBeDefined()
    expect(stories.StandbyAdmin).toBeDefined()
  })

  it('renders service node rows with the master switch action', () => {
    const renderStory = meta.render as ((args: typeof stories.StandbyAdmin.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.StandbyAdmin.args ?? {}),
            onOpenNodeDetails: () => undefined,
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.panelTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.nodeInventoryTitle)
    expect(markup).toContain('node-b')
    expect(markup).toContain('node-a')
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.promoteToMaster)
  })

  it('renders planned cutover inventory and timeline affordances for the ready story', () => {
    const renderStory = meta.render as ((args: typeof stories.PlannedCutoverReady.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.PlannedCutoverReady.args ?? {}),
            onOpenNodeDetails: () => undefined,
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.actionPlannedCutover)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.timelineTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.timelineLoadMore)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.healthStale)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.timelineStatusSuccess)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.healthReadyStandby)
  })

  it('keeps the source configuration entry on the main HA panel', () => {
    const renderStory = meta.render as ((args: typeof stories.FullMasterAdmin.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.FullMasterAdmin.args ?? {}),
            onConfigureSource: () => undefined,
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.configureSource)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.summaryCurrentOrigin)
  })

  it('keeps the local node row non-clickable while peer rows still open detail', () => {
    const renderStory = meta.render as ((args: typeof stories.PlannedCutoverReady.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.PlannedCutoverReady.args ?? {}),
            onOpenNodeDetails: () => undefined,
          }),
        ),
      ),
    )

    expect(markup).toContain('<strong>node-a</strong><span>当前管理节点</span>')
    expect(markup).toContain('class="ha-node-link"><strong>node-b</strong></button>')
    expect(markup).not.toContain('class="ha-node-link"><strong>node-a</strong></button>')
  })

  it('keeps lag-blocked standby reasons visible instead of falling back to configured', () => {
    const renderStory = meta.render as ((args: typeof stories.PlannedCutoverReady.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.PlannedCutoverReady.args ?? {}),
            onOpenNodeDetails: () => undefined,
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.messageSyncLagExceeded)
    expect(markup).not.toContain('>已配置<')
  })

  it('renders compact admin attention without node inventory actions', () => {
    const renderStory = meta.render as ((args: typeof stories.StandbyAdmin.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        { initialLanguage: 'zh' },
        createElement(
          ThemeProvider,
          null,
          renderStory?.({
            ...(meta.args ?? {}),
            ...(stories.StandbyAdmin.args ?? {}),
            adminVariant: 'compact',
            compactHref: '/admin/system-settings/ha',
            compactTitle: translations.zh.admin.systemSettings.ha.compactTitle,
            compactDescription: translations.zh.admin.systemSettings.ha.compactDescription,
            compactActionLabel: translations.zh.admin.systemSettings.ha.viewSettings,
          }),
        ),
      ),
    )

    expect(markup).toContain(translations.zh.admin.systemSettings.ha.compactTitle)
    expect(markup).toContain(translations.zh.admin.systemSettings.ha.viewSettings)
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.nodeInventoryTitle)
    expect(markup).not.toContain(translations.zh.admin.systemSettings.ha.promoteToMaster)
  })

  it('keeps the origin group source settings dialog story available', () => {
    expect(stories.OriginGroupSourceDialog).toBeDefined()
    expect(stories.OriginGroupSourceDialog.render).toBeDefined()
  })

  it('renders the origin group source settings dialog story in the browser runtime', async () => {
    const renderStory = stories.OriginGroupSourceDialog.render as (() => JSX.Element) | undefined
    const text = await renderIntoDom(renderStory?.() ?? <></>)

    expect(text).toContain(translations.zh.admin.systemSettings.ha.sourceKindOriginGroup)
    expect(text).toContain('eo-origin-group-ha-demo')
  })

  it('keeps the direct source settings dialog story available', () => {
    expect(stories.DirectSourceDialog).toBeDefined()
    expect(stories.DirectSourceDialog.render).toBeDefined()
  })

  it('renders the direct source settings dialog story in the browser runtime', async () => {
    const renderStory = stories.DirectSourceDialog.render as (() => JSX.Element) | undefined
    const text = await renderIntoDom(renderStory?.() ?? <></>)

    expect(text).toContain(translations.zh.admin.systemSettings.ha.sourceKindDirect)
    expect(text).toContain('203.0.113.9:58087')
  })

  it('renders node inventory origins from node source configuration instead of live route or peer public origin', async () => {
    const renderStory = meta.render as ((args: typeof stories.FullMasterAdmin.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const status = {
      ...(stories.FullMasterAdmin.args?.status ?? {}),
      nodeId: 'node-config-local',
      edgeoneCurrentTarget: 'edgeone-live-route',
      haSourceEffective: {
        sourceKind: 'direct',
        directOriginScheme: 'https',
        directOriginHost: 'local-source-config',
        directOriginPort: 1443,
        originGroupId: null,
        target: 'local-source-config:1443',
      },
      peerNodes: [
        {
          nodeId: 'node-config-peer',
          publicOrigin: 'peer-public-origin:443',
          sourceConfigTarget: 'peer-source-config:53844',
          role: 'standby',
          allowsBasicBusiness: false,
          allowsFullWrites: false,
          lastSyncAt: 1_700_000_018,
          syncLagSeconds: 4,
          recoveryStatus: null,
          message: 'standby is synchronized and ready',
          lastSeenAt: 1_700_000_020,
          stale: false,
          roleHint: 'standby_candidate',
          plannedCutoverEligible: true,
        },
      ],
    }

    const origins = await renderNodeOrigins(
      renderStory?.({
        ...(meta.args ?? {}),
        ...(stories.FullMasterAdmin.args ?? {}),
        status,
        onOpenNodeDetails: () => undefined,
      }) ?? <></>,
    )

    expect(origins).toEqual(['local-source-config:1443', 'peer-source-config:53844'])
  })

  it('keeps the dedicated node source configuration proof story available', () => {
    expect(stories.NodeSourceConfigProof).toBeDefined()
  })

  it('renders the dedicated proof story with distinct route and node-config targets', async () => {
    const renderStory = meta.render as ((args: typeof stories.NodeSourceConfigProof.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const text = await renderIntoDom(
      renderStory?.({
        ...(meta.args ?? {}),
        ...(stories.NodeSourceConfigProof.args ?? {}),
        onOpenNodeDetails: () => undefined,
      }) ?? <></>,
    )

    expect(text).toContain('gz.ivanli.cc:1443')
    expect(text).toContain('hinet-ep.707979.xyz:53844')
    expect(text).toContain('edgeone-live-route.example.com:443')
    expect(text).not.toContain('tavily-alt.ivanli.cc:443')
    expect(text).not.toContain('tavily-tw.ivanli.cc:443')
  })

  it('keeps the submit-failure source dialog story available', () => {
    expect(stories.SourceDialogSubmitFailure).toBeDefined()
    expect(stories.SourceDialogSubmitFailure.render).toBeDefined()
  })

  it('keeps the direct source selection summary in the story play proof', async () => {
    expect(stories.DirectSourceDialog.play).toBeDefined()

    await stories.DirectSourceDialog.play?.({
      canvasElement: {
        textContent: [
          translations.zh.admin.systemSettings.ha.configureSource,
          translations.zh.admin.systemSettings.ha.sourceKindDirect,
          translations.zh.admin.systemSettings.ha.sourceSelectedDirectLabel,
          'HTTPS · 203.0.113.9:58087',
        ].join(' '),
      } as HTMLElement,
    })
  })

  it('keeps the standby source dialog save-only contract in the story play proof', async () => {
    expect(stories.StandbySourceDialog.play).toBeDefined()

    await stories.StandbySourceDialog.play?.({
      canvasElement: {
        textContent: [
          translations.zh.admin.systemSettings.ha.configureSource,
          translations.zh.admin.systemSettings.ha.roleStandby,
          translations.zh.admin.systemSettings.ha.sourceSave,
        ].join(' '),
      } as HTMLElement,
    })

    await expect(
      stories.StandbySourceDialog.play?.({
        canvasElement: {
          textContent: [
            translations.zh.admin.systemSettings.ha.configureSource,
            translations.zh.admin.systemSettings.ha.roleStandby,
            translations.zh.admin.systemSettings.ha.sourceSave,
            translations.zh.admin.systemSettings.ha.sourceSaveAndApply,
          ].join(' '),
        } as HTMLElement,
      }),
    ).rejects.toThrow('Expected standby HA source dialog to omit the EdgeOne switch action.')
  })
})

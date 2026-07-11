import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { EN } from '../i18n/translations/en'
import { ZH } from '../i18n/translations/zh'
import meta, * as stories from './UpdateAvailableBanner.stories'

describe('UpdateAvailableBanner Storybook proofs', () => {
  it('keeps the update state gallery available', () => {
    expect(meta).toMatchObject({
      title: 'Support/Status/UpdateAvailableBanner',
      tags: ['autodocs'],
    })
    expect(stories.Ready).toBeDefined()
    expect(stories.Installing.args).toMatchObject({ status: 'installing', loading: true })
    expect(stories.Activating.args).toMatchObject({ status: 'activating', loading: true })
    expect(stories.ActivationFailed.args).toMatchObject({ status: 'activation-failed', loading: false })
    expect(stories.ChineseActivationFailed.args).toMatchObject({ strings: ZH.public.updateBanner })
    expect(stories.DarkActivationFailed.decorators).toHaveLength(1)
    expect(stories.ChineseReady.args).toMatchObject({ strings: ZH.public.updateBanner })
    expect(stories.DarkReady.decorators).toHaveLength(1)
  })

  it('renders ready and loading state copy from the shared translations', () => {
    const renderStory = meta.render as ((args: typeof meta.args) => JSX.Element) | undefined
    expect(renderStory).toBeDefined()

    const readyMarkup = renderToStaticMarkup(createElement(renderStory!, meta.args))
    const installingMarkup = renderToStaticMarkup(
      createElement(renderStory!, { ...meta.args, ...(stories.Installing.args ?? {}) }),
    )
    const activatingMarkup = renderToStaticMarkup(
      createElement(renderStory!, { ...meta.args, ...(stories.Activating.args ?? {}) }),
    )
    const failedMarkup = renderToStaticMarkup(
      createElement(renderStory!, { ...meta.args, ...(stories.ActivationFailed.args ?? {}) }),
    )

    expect(readyMarkup).toContain(EN.public.updateBanner.title)
    expect(readyMarkup).toContain('Current 0.2.0')
    expect(readyMarkup).toContain('Ready 0.2.1')
    expect(readyMarkup).toContain(EN.public.updateBanner.refresh)
    expect(installingMarkup).toContain(EN.public.updateBanner.preparing)
    expect(installingMarkup).toContain(EN.public.updateBanner.refreshing)
    expect(activatingMarkup).toContain(EN.public.updateBanner.activating)
    expect(failedMarkup).toContain(EN.public.updateBanner.failureTitle)
    expect(failedMarkup).toContain(EN.public.updateBanner.failureDescription)
    expect(failedMarkup).toContain(EN.public.updateBanner.retry)
    expect(failedMarkup).not.toContain('aria-busy="true"')
  })
})

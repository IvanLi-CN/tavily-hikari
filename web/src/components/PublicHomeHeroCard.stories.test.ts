import { describe, expect, it } from 'bun:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { LanguageProvider } from '../i18n'
import { ThemeProvider } from '../theme'
import meta, * as heroStories from './PublicHomeHeroCard.stories'

const extractPathPoints = (path: string) => {
  const tokens = path.match(/[A-Za-z]|-?\d+(?:\.\d+)?/g) ?? []
  let index = 0
  let command = ''
  let current: [string, string] = ['0', '0']
  let start: [string, string] | null = null

  while (index < tokens.length) {
    if (/^[A-Za-z]$/.test(tokens[index])) {
      command = tokens[index]
      index += 1
    }

    if (command === 'M' || command === 'L') {
      current = [tokens[index], tokens[index + 1]]
      start ??= current
      index += 2
      if (command === 'M') {
        command = 'L'
      }
      continue
    }

    if (command === 'H') {
      current = [tokens[index], current[1]]
      index += 1
      continue
    }

    if (command === 'V') {
      current = [current[0], tokens[index]]
      index += 1
      continue
    }

    if (command === 'C') {
      current = [tokens[index + 4], tokens[index + 5]]
      index += 6
      continue
    }

    throw new Error(`Unsupported path command: ${command}`)
  }

  return { start, end: current }
}

describe('PublicHomeHeroCard Storybook proofs', () => {
  it('exports a stable authentication checking state for slow statistics', () => {
    expect(meta).toMatchObject({
      title: 'Public/PublicHomeHeroCard',
      tags: ['autodocs'],
    })

    expect(heroStories.AuthStatusCheckingSlowStats.args).toMatchObject({
      metricsLoading: true,
      summaryLoading: true,
      showAuthStatusLoading: true,
    })
    expect(heroStories.LoadBalancerVisualProof.parameters).toMatchObject({
      viewport: { defaultViewport: '1440-device-desktop' },
    })
    expect(heroStories.LoadBalancerVisualProofMobile.parameters).toMatchObject({
      viewport: { defaultViewport: '0390-device-iphone-14' },
    })
  })

  it('renders explicit authentication checking copy without resolved metrics', () => {
    const renderStory = meta.render as
      | ((args: typeof heroStories.AuthStatusCheckingSlowStats.args) => JSX.Element)
      | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        null,
        createElement(
          ThemeProvider,
          null,
          renderStory?.(heroStories.AuthStatusCheckingSlowStats.args ?? {}),
        ),
      ),
    )

    expect(markup).toContain('Checking sign-in and registration status')
    expect(markup).toContain('Checking sign-in')
    expect(markup).toContain('/assets/relay-mesh-lockup-light.png')
    expect(markup).toContain('/assets/relay-mesh-lockup-dark.png')
    expect(markup).toContain('/assets/public-hero-load-balancer.png')
    expect(markup).not.toContain('/assets/public-hero-load-balancer-dark.png')
    expect(markup).toContain('public-home-load-balancer-motion')
    expect(markup).toContain('public-home-load-balancer-debug')
    expect(markup).toContain('hero-flow-in-1')
    expect(markup).toContain('hero-flow-in-7')
    expect(markup).toContain('hero-flow-out-purple')
    expect(markup).toContain('animateMotion')
    expect(markup).toContain('hero-flow-orbs')
    expect(markup).toContain('sr-only')
    expect([...markup.matchAll(/<animateMotion/g)]).toHaveLength(37)
    expect([...markup.matchAll(/hero-flow-particle-ingress/g)]).toHaveLength(21)
    expect([...markup.matchAll(/hero-flow-particle-(?:purple|blue|green|amber)/g)]).toHaveLength(16)
    const inputPaths = [...markup.matchAll(/id="hero-flow-in-\d+"\sd="([^"]+)"/g)]
    const inputPathPoints = inputPaths.map((match) => extractPathPoints(match[1]))
    expect(inputPaths).toHaveLength(7)
    expect(new Set(inputPathPoints.map(({ start }) => start?.join(',')))).toHaveLength(7)
    expect(new Set(inputPathPoints.map(({ end }) => end.join(',')))).toHaveLength(7)
    expect(markup).not.toContain('Requests paths')
    expect(markup).not.toContain('Key pool routes')
    expect(markup).not.toContain('hero-debug-paths')
    expect(markup).not.toContain('hero-debug-points')
    expect(markup).not.toContain('hero-title')
    expect(markup).not.toContain('hero-flow-glint')
    expect(markup).not.toContain('Sign in with Linux DO')
  })

  it('renders the Linux DO login action with the primary button treatment', () => {
    const renderStory = meta.render as
      | ((args: typeof heroStories.LoggedOutNoToken.args) => JSX.Element)
      | undefined
    expect(renderStory).toBeDefined()

    const markup = renderToStaticMarkup(
      createElement(
        LanguageProvider,
        null,
        createElement(
          ThemeProvider,
          null,
          renderStory?.(heroStories.LoggedOutNoToken.args ?? {}),
        ),
      ),
    )

    expect(markup).toContain('linuxdo-login-button')
    expect(markup).toContain('from-[#A78BFA] to-[#7C3AED]')
    expect(markup).toContain('text-primary-foreground')
    expect(markup).not.toContain('linuxdo-login-button h-auto rounded-full border-foreground/20')
  })
})

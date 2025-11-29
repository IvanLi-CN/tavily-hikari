import React from 'react'
import { useLanguage, useTranslate } from '../i18n'

function TavilyLogo({ className = 'h-5 w-5' }: { className?: string }): JSX.Element {
  return (
    <svg
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <title>Tavily</title>
      <path
        d="M9.1.503l2.824 4.47a1.078 1.078 0 01-.911 1.655H9.858v6.692h-1.67V0c.35 0 .7.168.912.503z"
        fill="#8FBCFA"
      />
      <path
        d="M4.453 4.974L7.277.503A1.07 1.07 0 018.189 0v13.32a2.633 2.633 0 00-1.67.48V6.628H5.364c-.85 0-1.366-.936-.912-1.654z"
        fill="#468BFF"
      />
      <path
        d="M17.041 17.74h-7.028c.423-.457.67-1.049.7-1.67h12.956c0 .35-.168.7-.502.912l-4.472 2.823a1.078 1.078 0 01-1.654-.911v-1.155z"
        fill="#FDBB11"
      />
      <path
        d="M18.695 12.334l4.47 2.824c.336.212.503.562.503.912H10.713a2.65 2.65 0 00-.493-1.67h6.822v-1.154c0-.85.935-1.366 1.653-.912z"
        fill="#F6D785"
      />
      <path
        d="M4.394 19.605L.316 23.683a1.07 1.07 0 001 .29l5.158-1.165A1.078 1.078 0 007 20.994l-.816-.816 3.073-3.074a1.61 1.61 0 000-2.276l-.042-.043-4.82 4.82z"
        fill="#FF9A9D"
      />
      <path
        d="M3.822 17.817l3.073-3.074a1.61 1.61 0 012.277 0l.042.043-4.818 4.819-4.08 4.079a1.07 1.07 0 01-.289-1l1.165-5.158A1.078 1.078 0 013.006 17l.816.817z"
        fill="#FE363B"
      />
    </svg>
  )
}

function CherryStudioMock(): JSX.Element {
  const { language } = useLanguage()
  const strings = useTranslate()
  const t = strings.public.cherryMock

  const baseUrl =
    typeof window !== 'undefined' && window.location.origin ? window.location.origin : 'https://your-hikari.example.com'
  const apiUrl = `${baseUrl}/api/tavily`

  // Keep controls visually static only
  const disabledProps = {
    disabled: true,
  } as const

  const NavPlaceholder = () => (
    <li className="rounded px-2 py-1">
      <span
        aria-hidden="true"
        className="block h-4 rounded-md border border-base-300/70 bg-base-200"
      />
    </li>
  )

  return (
    <div className="mt-6">
      {/* Keep a semantic title for screen readers without adding a second visible header */}
      <p className="sr-only">{t.title}</p>
      <div className="rounded-3xl border border-base-200 bg-base-200 shadow-lg">
        {/* Window chrome (single, custom) */}
        <div className="flex items-center gap-2 border-b border-base-300 px-4 py-2 rounded-t-3xl">
          <div className="flex gap-1.5">
            <span className="h-2.5 w-2.5 rounded-full bg-red-500" />
            <span className="h-2.5 w-2.5 rounded-full bg-amber-400" />
            <span className="h-2.5 w-2.5 rounded-full bg-emerald-500" />
          </div>
          <div className="mx-auto text-xs font-medium text-base-content/70">{t.windowTitle}</div>
        </div>

        {/* Main content */}
        <div className="bg-base-100 px-4 pb-4 pt-3 md:px-6 md:pb-6 md:pt-4">
          <div className="flex flex-col gap-4 md:flex-row">
            {/* Left navigation */}
            <nav className="w-full shrink-0 border border-base-200 bg-base-100/60 p-2 text-xs md:w-52 md:text-[0.78rem]">
              <ul className="space-y-1.5">
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                {/* Selected item */}
                <li className="rounded-lg border border-primary/70 bg-primary/10 px-2 py-1 text-primary">
                  {t.sidebar.webSearch}
                </li>
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
                <NavPlaceholder />
              </ul>
            </nav>

            {/* Right content area */}
            <div className="flex-1 space-y-3 md:space-y-4">
              {/* Provider card */}
              <section className="rounded-xl border border-primary/40 bg-base-100 px-4 py-3 shadow-sm">
                <div className="flex flex-col items-start justify-between gap-3 md:flex-row md:items-center">
                  <div className="space-y-0.5">
                    <p className="text-xs font-semibold text-base-content/80">{t.providerCard.title}</p>
                    <p className="text-[0.72rem] text-base-content/60">{t.providerCard.subtitle}</p>
                  </div>
                  <button
                    type="button"
                    className="btn btn-sm min-h-0 h-8 rounded-md border-primary/60 bg-primary/10 px-3 text-xs normal-case text-primary pointer-events-none"
                    {...disabledProps}
                  >
                    <span className="flex items-center gap-2">
                      <TavilyLogo className="h-4 w-4" />
                      <span>{t.providerCard.providerValue}</span>
                    </span>
                    <span className="ml-2 h-3 w-3 rounded-full border border-base-300 bg-base-200" />
                  </button>
                </div>
              </section>

              {/* Tavily config card */}
              <section className="space-y-3 rounded-xl border border-base-200 bg-base-100 px-4 py-3 shadow-sm">
                {/* Header row */}
                <div className="mb-1 flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <TavilyLogo className="h-5 w-5" />
                    <span className="text-xs font-semibold text-base-content/90">{t.tavilyCard.title}</span>
                  </div>
                  <div className="h-5 w-5 rounded border border-base-300 bg-base-200" aria-hidden="true" />
                </div>

                {/* API key block */}
                <div className="space-y-1.5">
                  <label className="block text-xs font-medium text-base-content/80">
                    {t.tavilyCard.apiKeyLabel}
                  </label>
                  <div className="flex items-stretch gap-1.5">
                    <input
                      type="text"
                      className="input input-xs md:input-sm input-bordered flex-1 text-xs"
                      value={t.tavilyCard.apiKeyPlaceholder}
                      readOnly
                      onClick={(e) => e.currentTarget.select()}
                      onFocus={(e) => e.currentTarget.select()}
                    />
                    <div
                      className="flex h-8 w-8 items-center justify-center rounded-md bg-base-200 text-[0.65rem] text-base-content/40"
                      aria-hidden="true"
                    >
                      ...
                    </div>
                    <button
                      type="button"
                      className="btn btn-xs h-8 min-h-0 rounded-md border-base-300 bg-base-200 px-3 text-[0.7rem] font-medium text-base-content/80 normal-case pointer-events-none"
                      {...disabledProps}
                    >
                      {t.tavilyCard.testButtonLabel}
                    </button>
                  </div>
                  <p className="text-[0.7rem] text-primary">
                    {t.tavilyCard.apiKeyHint}
                  </p>
                </div>

                {/* API URL block */}
                <div className="mt-3 space-y-1.5">
                  <label className="block text-xs font-medium text-base-content/80">
                    {t.tavilyCard.apiUrlLabel}
                  </label>
                  <input
                    type="text"
                    className="input input-xs md:input-sm input-bordered w-full text-xs text-base-content/80"
                    value={apiUrl}
                    readOnly
                    onClick={(e) => e.currentTarget.select()}
                    onFocus={(e) => e.currentTarget.select()}
                  />
                  <p className="text-[0.7rem] text-base-content/60">
                    {t.tavilyCard.apiUrlHint}
                  </p>
                </div>
              </section>

              {/* General settings card */}
              <section className="space-y-3 rounded-xl border border-dashed border-base-200 bg-base-100/90 px-4 py-3 shadow-sm text-base-content/70">
                <h5 className="text-xs font-semibold text-base-content/70">{t.generalCard.title}</h5>
                <div className="space-y-2">
                  {/* Include date row */}
                  <div className="flex items-center justify-between gap-4">
                    <span className="text-xs">{t.generalCard.includeDateLabel}</span>
                    <label className="flex items-center">
                      <input
                        type="checkbox"
                        className="toggle toggle-xs pointer-events-none"
                        checked
                        {...disabledProps}
                      />
                    </label>
                  </div>

                  {/* Results count row */}
                  <div className="space-y-1.5">
                    <div className="flex items-center justify-between gap-4">
                      <span className="text-xs">{t.generalCard.resultsCountLabel}</span>
                      <input
                        type="range"
                        min={1}
                        max={100}
                        defaultValue={20}
                        className="range range-xs pointer-events-none"
                        {...disabledProps}
                      />
                    </div>
                    <div className="flex items-center justify-between text-[0.65rem] text-base-content/50">
                      <span>1</span>
                      <span>5</span>
                      <span>20</span>
                      <span>50</span>
                      <span>100</span>
                    </div>
                  </div>
                </div>
              </section>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

export default CherryStudioMock

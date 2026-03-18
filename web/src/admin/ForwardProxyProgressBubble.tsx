import { Icon } from '@iconify/react'
import type { CSSProperties } from 'react'

import type { AdminTranslations } from '../i18n'
import type { ForwardProxyDialogProgressState } from './forwardProxyDialogProgress'

interface ForwardProxyProgressBubbleProps {
  strings: AdminTranslations['proxySettings']
  progress: ForwardProxyDialogProgressState
  className?: string
  style?: CSSProperties
}

export default function ForwardProxyProgressBubble({
  strings,
  progress,
  className,
  style,
}: ForwardProxyProgressBubbleProps): JSX.Element {
  const title =
    progress.action === 'validate'
      ? strings.progress.titleValidate
      : progress.action === 'revalidate'
        ? strings.progress.titleRevalidate
        : strings.progress.titleSave

  return (
    <div
      className={`rounded-2xl border border-primary/25 bg-primary/5 px-4 py-3 shadow-[0_16px_40px_-28px_hsl(var(--primary)/0.8)] ${className ?? ''}`}
      style={style}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <p className="text-sm font-semibold text-foreground">{title}</p>
          <p className="text-xs text-muted-foreground">
            {progress.message ?? strings.progress.running}
          </p>
        </div>
      </div>

      <div className="space-y-2">
        {progress.steps.map((step) => {
          const icon =
            step.status === 'done'
              ? 'mdi:check-circle'
              : step.status === 'error'
                ? 'mdi:alert-circle'
                : step.status === 'running'
                  ? 'mdi:loading'
                  : 'mdi:circle-outline'
          const toneClass =
            step.status === 'done'
              ? 'text-success'
              : step.status === 'error'
                ? 'text-destructive'
                : step.status === 'running'
                  ? 'text-primary'
                  : 'text-muted-foreground'

          return (
            <div
              key={step.key}
              className={`flex items-start gap-3 rounded-2xl border px-3 py-2 transition-colors ${
                step.status === 'running'
                  ? 'border-primary/35 bg-background/88'
                  : step.status === 'error'
                    ? 'border-destructive/30 bg-destructive/5'
                    : step.status === 'done'
                      ? 'border-success/25 bg-success/5'
                      : 'border-border/60 bg-background/70'
              }`}
            >
              <Icon
                icon={icon}
                className={`${toneClass} mt-0.5 text-base ${step.status === 'running' ? 'animate-spin' : ''}`}
              />
              <div className="min-w-0">
                <p className="text-sm font-medium text-foreground">{step.label}</p>
                <p className="text-xs text-muted-foreground">
                  {step.detail
                    ?? (step.status === 'done'
                      ? strings.progress.done
                      : step.status === 'error'
                        ? strings.progress.failed
                        : step.status === 'running'
                          ? strings.progress.running
                          : strings.progress.waiting)}
                </p>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

import React from 'react'

import { Badge } from './ui/badge'

export type StatusTone = 'success' | 'warning' | 'error' | 'info' | 'neutral'

const toneVariantMap: Record<StatusTone, 'success' | 'warning' | 'destructive' | 'info' | 'neutral'> = {
  success: 'success',
  warning: 'warning',
  error: 'destructive',
  info: 'info',
  neutral: 'neutral',
}

export interface StatusBadgeProps {
  tone: StatusTone
  children: React.ReactNode
  className?: string
}

export function StatusBadge({ tone, children, className = '' }: StatusBadgeProps): JSX.Element {
  return (
    <Badge variant={toneVariantMap[tone]} className={`status-badge status-pill ${className}`}>
      {children}
    </Badge>
  )
}

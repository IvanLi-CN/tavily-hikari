import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '../../lib/utils'

const badgeVariants = cva(
  'inline-flex items-center rounded-full border px-3 py-1 text-[11px] font-bold tracking-[0.01em] shadow-clayCard transition-colors focus:outline-none focus:ring-4 focus:ring-ring/25 focus:ring-offset-2',
  {
    variants: {
      variant: {
        default: 'border-primary/20 bg-primary/15 text-primary',
        secondary: 'border-secondary/20 bg-secondary/15 text-secondary',
        destructive: 'border-destructive/20 bg-destructive/15 text-destructive',
        outline: 'border-border/70 bg-card/70 text-foreground',
        success: 'border-success/35 bg-success/15 text-[hsl(var(--success-readable))]',
        warning: 'border-warning/40 bg-warning/20 text-[hsl(var(--warning-readable))]',
        info: 'border-info/35 bg-info/15 text-[hsl(var(--info-readable))]',
        neutral: 'border-border/80 bg-muted/70 text-muted-foreground',
      },
    },
    defaultVariants: {
      variant: 'default',
    },
  },
)

export interface BadgeProps extends React.HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps): JSX.Element {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />
}

export { Badge, badgeVariants }

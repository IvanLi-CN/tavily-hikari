import * as React from 'react'
import { Slot } from '@radix-ui/react-slot'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '../../lib/utils'

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-sm font-semibold ring-offset-background transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50',
  {
    variants: {
      variant: {
        default:
          'bg-primary text-primary-foreground shadow-[0_12px_24px_-12px_hsl(var(--primary)/0.85)] hover:bg-primary/92 hover:-translate-y-[1px]',
        destructive:
          'bg-destructive text-destructive-foreground shadow-[0_12px_24px_-12px_hsl(var(--destructive)/0.9)] hover:bg-destructive/92 hover:-translate-y-[1px]',
        outline: 'border border-border/80 bg-card/70 hover:bg-accent/40 hover:text-accent-foreground',
        secondary: 'border border-secondary/30 bg-secondary/18 text-foreground hover:bg-secondary/26',
        ghost: 'text-foreground/85 hover:bg-accent/38 hover:text-foreground',
        link: 'text-primary underline-offset-4 hover:underline',
        warning:
          'bg-warning text-warning-foreground shadow-[0_12px_24px_-12px_hsl(var(--warning)/0.8)] hover:bg-warning/92 hover:-translate-y-[1px]',
        success:
          'bg-success text-success-foreground shadow-[0_12px_24px_-12px_hsl(var(--success)/0.85)] hover:bg-success/92 hover:-translate-y-[1px]',
      },
      size: {
        default: 'h-10 px-4 py-2',
        sm: 'h-9 rounded-lg px-3',
        lg: 'h-11 rounded-lg px-8',
        icon: 'h-10 w-10',
        xs: 'h-7 rounded-md px-2.5 text-xs',
      },
    },
    defaultVariants: {
      variant: 'default',
      size: 'default',
    },
  },
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : 'button'
    return <Comp className={cn(buttonVariants({ variant, size, className }))} ref={ref} {...props} />
  },
)
Button.displayName = 'Button'

export { Button, buttonVariants }

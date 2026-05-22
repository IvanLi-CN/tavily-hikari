import * as React from 'react'
import { Slot } from '@radix-ui/react-slot'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '../../lib/utils'

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[20px] text-sm font-bold tracking-wide ring-offset-background transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-4 focus-visible:ring-ring/30 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 disabled:shadow-none active:scale-[0.94] active:shadow-clayPressed',
  {
    variants: {
      variant: {
        default:
          'border border-primary/10 bg-gradient-to-br from-[#A78BFA] to-[#7C3AED] text-primary-foreground shadow-clayButton hover:-translate-y-1 hover:shadow-clayButtonHover',
        destructive:
          'border border-destructive/10 bg-gradient-to-br from-red-400 to-destructive text-destructive-foreground shadow-clayButton hover:-translate-y-1 hover:shadow-clayButtonHover',
        outline: 'border border-primary/25 bg-card/80 text-primary shadow-clayButton hover:-translate-y-1 hover:border-primary/45 hover:bg-primary/10',
        secondary: 'border border-border/50 bg-card/80 text-foreground shadow-clayButton hover:-translate-y-1 hover:bg-secondary/10',
        ghost: 'text-foreground/85 shadow-none hover:-translate-y-1 hover:bg-primary/10 hover:text-primary hover:shadow-clayButton',
        link: 'text-primary underline-offset-4 hover:underline',
        warning:
          'border border-warning/10 bg-gradient-to-br from-amber-300 to-warning text-warning-foreground shadow-clayButton hover:-translate-y-1 hover:shadow-clayButtonHover',
        success:
          'border border-success/10 bg-gradient-to-br from-emerald-300 to-success text-success-foreground shadow-clayButton hover:-translate-y-1 hover:shadow-clayButtonHover',
      },
      size: {
        default: 'h-12 px-5 py-2',
        sm: 'h-11 px-4',
        lg: 'h-14 px-8 text-base',
        icon: 'h-12 w-12',
        xs: 'h-11 px-3 text-xs',
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

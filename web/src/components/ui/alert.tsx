import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'

import { cn } from '../../lib/utils'

const alertVariants = cva(
  'alert relative w-full text-left transition-all duration-200 ease-out',
  {
    variants: {
      variant: {
        default: '',
        destructive: 'alert-error',
        warning: 'alert-warning',
      },
      emphasis: {
        default: '',
        prominent: '',
      },
    },
    compoundVariants: [
      {
        variant: 'destructive',
        emphasis: 'prominent',
        className: 'shadow-[0_0_0_4px_hsl(var(--destructive)/0.16),var(--shadow-clay-card)]',
      },
    ],
    defaultVariants: {
      variant: 'default',
      emphasis: 'default',
    },
  },
)

const Alert = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & VariantProps<typeof alertVariants>
>(({ className, variant, ...props }, ref) => (
  <div ref={ref} role="alert" className={cn(alertVariants({ variant }), className)} {...props} />
))
Alert.displayName = 'Alert'

const AlertTitle = React.forwardRef<HTMLHeadingElement, React.HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => (
    <h5 ref={ref} className={cn('font-semibold leading-6 tracking-normal', className)} {...props} />
  ),
)
AlertTitle.displayName = 'AlertTitle'

const AlertDescription = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn('mt-1.5 text-sm leading-6 [&_p]:leading-6', className)} {...props} />
  ),
)
AlertDescription.displayName = 'AlertDescription'

export { Alert, AlertDescription, AlertTitle }

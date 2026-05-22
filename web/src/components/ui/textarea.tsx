import * as React from 'react'

import { cn } from '../../lib/utils'

const Textarea = React.forwardRef<HTMLTextAreaElement, React.ComponentProps<'textarea'>>(({ className, ...props }, ref) => {
  return (
    <textarea
      className={cn(
        'flex min-h-[96px] w-full rounded-[20px] border border-input/35 bg-muted/60 px-4 py-3 text-sm font-medium shadow-clayPressed ring-offset-background transition-all duration-200 placeholder:text-muted-foreground focus-visible:bg-card/90 focus-visible:outline-none focus-visible:ring-4 focus-visible:ring-ring/20 focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50',
        className,
      )}
      ref={ref}
      {...props}
    />
  )
})
Textarea.displayName = 'Textarea'

export { Textarea }

import * as React from 'react'
import * as TooltipPrimitive from '@radix-ui/react-tooltip'

import { cn } from '../../lib/utils'

const TooltipProvider = TooltipPrimitive.Provider
const Tooltip = TooltipPrimitive.Root
const TooltipTrigger = TooltipPrimitive.Trigger
const TooltipPortal = TooltipPrimitive.Portal

const TooltipContent = React.forwardRef<
  React.ElementRef<typeof TooltipPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof TooltipPrimitive.Content>
>(({ className, sideOffset = 8, collisionPadding = 12, children, ...props }, ref) => (
  <TooltipPortal>
    <TooltipPrimitive.Content
      ref={ref}
      sideOffset={sideOffset}
      collisionPadding={collisionPadding}
      className={cn(
        'layer-tooltip max-w-[min(20rem,calc(100vw-1rem))] overflow-hidden rounded-xl border border-border/90 bg-popover/98 px-3 py-1.5 text-xs font-medium leading-relaxed text-popover-foreground shadow-xl backdrop-blur',
        className,
      )}
      {...props}
    >
      {children}
      <TooltipPrimitive.Arrow className="fill-popover/98" width={12} height={6} />
    </TooltipPrimitive.Content>
  </TooltipPortal>
))
TooltipContent.displayName = TooltipPrimitive.Content.displayName

export {
  Tooltip,
  TooltipContent,
  TooltipPortal,
  TooltipProvider,
  TooltipTrigger,
}

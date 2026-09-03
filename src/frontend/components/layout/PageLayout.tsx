import type { ReactNode } from 'react'

import { cn } from '../../lib/utils'

interface PageFrameProps {
  children: ReactNode
  className?: string
}

export function PageFrame({ children, className }: PageFrameProps) {
  return (
    <div
      data-page-frame="true"
      className={cn('w-full px-4 pb-20 pt-20 sm:px-6 md:px-8 md:pt-8 xl:px-10', className)}
    >
      {children}
    </div>
  )
}

interface PageHeaderProps {
  title: string
  description: string | null
  actions: ReactNode | null
}

export function PageHeader({ title, description, actions }: PageHeaderProps) {
  return (
    <header className="mb-8 flex flex-col gap-4 lg:flex-row lg:items-center lg:justify-between">
      <div className="min-w-0">
        <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">
          {title}
        </h1>
        {description && <p className="mt-1 text-sm text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="w-full shrink-0 lg:ml-auto lg:w-auto">{actions}</div>}
    </header>
  )
}

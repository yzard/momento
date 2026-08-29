import { useId, type ReactNode } from 'react'
import type { LucideIcon } from 'lucide-react'

import { cn } from '../../lib/utils'

export interface AdminMetric {
  label: string
  value: number | string | null
  emphasis?: boolean
}

export function AdminPanel({
  icon: Icon,
  title,
  description,
  children,
}: {
  icon: LucideIcon
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
      <div className="flex items-center gap-4 border-b border-border bg-muted/30 px-5 py-5 sm:px-8 sm:py-6">
        <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
          <Icon aria-hidden="true" className="h-5 w-5" />
        </div>
        <div>
          <h2 className="font-display text-xl font-semibold text-foreground">{title}</h2>
          <p className="mt-1 text-sm text-muted-foreground">{description}</p>
        </div>
      </div>
      <div className="p-5 sm:p-8">{children}</div>
    </section>
  )
}

export function AdminStatusMetrics({ metrics }: { metrics: readonly AdminMetric[] }) {
  return (
    <dl className="grid grid-cols-2 gap-3 text-sm sm:grid-cols-4" aria-live="polite">
      {metrics.map((metric) => (
        <div key={metric.label} className="rounded-lg border border-border/50 bg-muted/20 p-3">
          <dt className="mb-1 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            {metric.label}
          </dt>
          <dd
            className={cn(
              'font-mono text-lg font-bold tabular-nums text-foreground',
              metric.emphasis && 'text-destructive'
            )}
          >
            {metric.value ?? '—'}
          </dd>
        </div>
      ))}
    </dl>
  )
}

export function AdminFailureLog({ title, entries }: { title: string; entries: readonly string[] }) {
  const inputId = useId()
  const value = entries.length > 0 ? entries.join('\n') : 'No failures.'

  return (
    <div className="mt-6">
      <label
        htmlFor={inputId}
        className="mb-2 block text-xs font-bold uppercase tracking-wider text-muted-foreground"
      >
        {title}
      </label>
      <textarea
        id={inputId}
        aria-label={title}
        value={value}
        readOnly
        spellCheck={false}
        className="min-h-32 w-full resize-y select-text rounded-lg border border-border bg-muted/20 p-4 font-mono text-xs leading-5 text-foreground outline-none focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/20"
      />
    </div>
  )
}

import type { ReactNode } from 'react'

interface PageStateProps {
  icon: ReactNode
  title: string
  description: string
  action?: ReactNode
}

export default function PageState({ icon, title, description, action }: PageStateProps) {
  return (
    <div className="flex min-h-[360px] flex-col items-center justify-center rounded-xl border border-dashed border-border bg-muted/10 px-6 py-16 text-center">
      <div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-border bg-background shadow-sm">
        {icon}
      </div>
      <h2 className="font-display text-xl font-semibold text-foreground">{title}</h2>
      <p className="mt-2 max-w-md text-sm text-muted-foreground">{description}</p>
      {action ? <div className="mt-6">{action}</div> : null}
    </div>
  )
}

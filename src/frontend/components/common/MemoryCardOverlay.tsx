import type { ElementType } from 'react'

interface MemoryCardOverlayProps {
  title: string
  subtitle: string | null
  badge: string
  headingLevel: 'h2' | 'h3'
}

export default function MemoryCardOverlay({
  title,
  subtitle,
  badge,
  headingLevel,
}: MemoryCardOverlayProps) {
  const Heading: ElementType = headingLevel

  return (
    <div className="absolute inset-x-0 bottom-0 bg-gradient-to-t from-black/90 via-black/60 to-transparent px-4 pb-4 pt-14 text-white">
      <div className="flex items-end justify-between gap-3">
        <div className="min-w-0">
          <Heading className="truncate font-display text-xl font-semibold leading-tight">
            {title}
          </Heading>
          {subtitle ? <p className="mt-0.5 truncate text-sm text-white/80">{subtitle}</p> : null}
        </div>
        <span className="shrink-0 rounded-full bg-black/45 px-2.5 py-1 text-xs font-bold text-white backdrop-blur-sm">
          {badge}
        </span>
      </div>
    </div>
  )
}

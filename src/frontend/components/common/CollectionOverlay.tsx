import { useEffect, useRef, type ReactNode } from 'react'

export default function CollectionOverlay({
  children,
  close,
}: {
  children: ReactNode
  close: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    ref.current?.focus()
    const keydown = (event: KeyboardEvent) => {
      if (document.querySelector('[data-media-viewer]')) return
      const target = event.target as HTMLElement
      if (target.closest('input, textarea, select, [contenteditable="true"]')) return
      if (event.key === 'Escape' || event.key === 'Backspace') {
        event.preventDefault()
        event.stopImmediatePropagation()
        close()
      }
      if (event.key === 'Tab') {
        const items = [
          ...(ref.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled), a[href], input, [tabindex="0"]'
          ) ?? []),
        ]
        const first = items[0],
          last = items[items.length - 1]
        if (!first) {
          event.preventDefault()
          return
        }
        if (
          event.shiftKey &&
          (document.activeElement === first || document.activeElement === ref.current)
        ) {
          event.preventDefault()
          last?.focus()
        }
        if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault()
          first.focus()
        }
      }
    }
    window.addEventListener('keydown', keydown)
    return () => {
      window.removeEventListener('keydown', keydown)
      previous?.focus()
    }
  }, [close])
  return (
    <div
      ref={ref}
      tabIndex={-1}
      role="dialog"
      aria-modal="true"
      aria-label="Collection details"
      className="absolute inset-0 z-40 flex min-h-0 bg-background"
    >
      {children}
    </div>
  )
}

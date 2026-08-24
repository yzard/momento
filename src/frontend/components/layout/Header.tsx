import { Menu } from 'lucide-react'

export default function Header({ onMenuClick }: { onMenuClick: () => void }) {
  return (
    <header className="absolute left-4 top-4 z-20 md:hidden">
      <button
        type="button"
        aria-label="Open navigation"
        onClick={onMenuClick}
        className="flex h-11 w-11 items-center justify-center rounded-lg border border-border/60 bg-background/90 text-muted-foreground shadow-sm backdrop-blur-sm transition-colors hover:bg-muted hover:text-foreground"
      >
        <Menu className="h-5 w-5" />
      </button>
    </header>
  )
}

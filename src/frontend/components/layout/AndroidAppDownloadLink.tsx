import { Download, Smartphone } from 'lucide-react'
import { cn } from '../../lib/utils'

interface AndroidAppDownloadLinkProps {
  compact: boolean
}

export default function AndroidAppDownloadLink({ compact }: AndroidAppDownloadLinkProps) {
  return (
    <a
      href="/momento-android.apk"
      download="momento-android.apk"
      aria-label="Download Android app"
      title="Download Android app"
      className={cn(
        'inline-flex min-h-11 items-center justify-center rounded-xl border border-primary/20 bg-primary/10 font-semibold text-primary transition-colors hover:border-primary/40 hover:bg-primary/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2',
        compact ? 'min-w-11 p-2.5' : 'w-full gap-3 px-4 py-3 sm:w-auto'
      )}
    >
      <Smartphone className="h-5 w-5 shrink-0" aria-hidden="true" />
      {!compact && <span>Download Android app</span>}
      {!compact && <Download className="h-4 w-4 shrink-0" aria-hidden="true" />}
    </a>
  )
}

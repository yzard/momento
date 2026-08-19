import { useEffect, useState } from 'react'
import { Loader2, RefreshCw } from 'lucide-react'

import { metadataApi, type MetadataStatus } from '../../api/metadata'

export default function MetadataPanel() {
  const [status, setStatus] = useState<MetadataStatus | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const loadStatus = async () => {
    try {
      setStatus(await metadataApi.getStatus())
      setErrorMessage(null)
    } catch {
      setErrorMessage('Could not load metadata status.')
    }
  }

  useEffect(() => {
    void loadStatus()
    const timer = setInterval(() => void loadStatus(), 2000)
    return () => clearInterval(timer)
  }, [])

  const runAction = async (action: () => Promise<unknown>) => {
    setIsSubmitting(true)
    try {
      await action()
      await loadStatus()
    } catch {
      setErrorMessage('Could not complete the metadata action.')
    } finally {
      setIsSubmitting(false)
    }
  }

  return <div>
    <div>
      <button type="button" onClick={() => void runAction(metadataApi.generate)} disabled={isSubmitting} className="w-full sm:w-auto px-8 py-2.5 bg-primary text-primary-foreground font-semibold text-sm rounded-lg inline-flex items-center justify-center gap-2 transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-50">
        {isSubmitting ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />} Generate
      </button>
    </div>
    {errorMessage && <p role="alert" className="mt-4 text-sm text-destructive">{errorMessage}</p>}
    {status && <div className="mt-6 grid grid-cols-2 sm:grid-cols-4 gap-3 text-sm" aria-live="polite">
      {[['Queued', status.queuedJobs], ['Processing', status.processingJobs], ['Completed', status.completedJobs], ['Failed', status.failedJobs]].map(([label, count]) => <div key={String(label)} className="bg-muted/10 p-3 rounded-lg border border-border/30"><span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">{label}</span><span className="font-bold text-lg text-foreground">{count}</span></div>)}
    </div>}
  </div>
}

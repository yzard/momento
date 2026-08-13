import { useEffect, useState } from 'react'
import { Loader2, RefreshCw, Trash2 } from 'lucide-react'

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

  return <div className="bg-card/30 rounded-xl border border-border/50 p-6 backdrop-blur-sm">
    <h3 className="text-lg font-medium mb-4 text-foreground">Metadata</h3>
    <p className="text-muted-foreground mb-6 font-light">Generate metadata, thumbnails, and prepared AI inputs. OCR and image tagging submit automatically after metadata completes.</p>
    <div className="flex flex-col sm:flex-row gap-4">
      <button type="button" onClick={() => void runAction(metadataApi.generate)} disabled={isSubmitting} className="flex-1 px-6 py-4 bg-primary/5 border border-primary/20 text-primary font-bold text-sm uppercase tracking-wider rounded-lg flex items-center justify-center gap-3 transition-colors hover:bg-primary/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-50">
        {isSubmitting ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />} Generate
      </button>
      <button type="button" onClick={() => void runAction(metadataApi.reset)} disabled={isSubmitting} className="flex-1 px-6 py-4 bg-destructive/5 border border-destructive/20 text-destructive font-bold text-sm uppercase tracking-wider rounded-lg flex items-center justify-center gap-3 transition-colors hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-destructive disabled:cursor-not-allowed disabled:opacity-50">
        <Trash2 className="w-4 h-4" /> Reset & Generate All
      </button>
    </div>
    {errorMessage && <p role="alert" className="mt-4 text-sm text-destructive">{errorMessage}</p>}
    {status && <div className="mt-8 grid grid-cols-2 sm:grid-cols-4 gap-4 text-sm">
      {[['Queued', status.queuedJobs], ['Processing', status.processingJobs], ['Completed', status.completedJobs], ['Failed', status.failedJobs]].map(([label, count]) => <div key={String(label)} className="bg-muted/10 p-3 rounded-lg border border-border/30"><span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">{label}</span><span className="font-bold text-lg text-foreground">{count}</span></div>)}
    </div>}
  </div>
}

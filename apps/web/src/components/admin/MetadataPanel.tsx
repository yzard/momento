import { useState, useEffect } from 'react'
import { adminApi } from '../../api/admin'
import { Loader2, RefreshCw, Trash2, X } from 'lucide-react'
import { cn } from '../../lib/utils'

interface RegenerationStatus {
  status: string
  totalMedia: number
  processedMedia: number
  updatedMetadata: number
  generatedThumbnails: number
  updatedTags: number
  startedAt: string | null
  completedAt: string | null
  errors: string[]
}

export default function MetadataPanel() {
  const [status, setStatus] = useState<RegenerationStatus | null>(null)
  const [isTriggering, setIsTriggering] = useState(false)

  const loadStatus = async () => {
    try {
      const data = await adminApi.getRegenerationStatus()
      setStatus(data)
    } catch {
      console.error('Failed to load regeneration status')
    }
  }

  useEffect(() => {
    loadStatus()
    const interval = setInterval(loadStatus, 2000)
    return () => clearInterval(interval)
  }, [])

  const handleGenerateMetadata = async () => {
    setIsTriggering(true)
    try {
      await adminApi.regenerateMedia(true)
      loadStatus()
    } catch {
      alert('Failed to start metadata generation. A job may already be running.')
    } finally {
      setIsTriggering(false)
    }
  }

  const handleCleanAndRegenerate = async () => {
    setIsTriggering(true)
    try {
      await adminApi.resetLibrary()
      loadStatus()
    } catch {
      alert('Failed to start clean and regenerate. A job may already be running.')
    } finally {
      setIsTriggering(false)
    }
  }

  const handleCancel = async () => {
    try {
      await adminApi.cancelRegeneration()
      loadStatus()
    } catch {
      alert('Failed to cancel regeneration.')
    }
  }

  const isRunning = status?.status === 'running'
  const progress = status && status.totalMedia > 0
    ? Math.round((status.processedMedia / status.totalMedia) * 100)
    : 0

  return (
    <div className="bg-card/30 rounded-xl border border-border/50 p-6 backdrop-blur-sm">
      <h3 className="text-lg font-medium mb-4 text-foreground">Regenerate Metadata</h3>

      <p className="text-muted-foreground mb-6 font-light">
        Generate or regenerate metadata and thumbnails for your media library.
      </p>

      <div className="flex flex-col sm:flex-row gap-4">
        <button
          onClick={handleGenerateMetadata}
          disabled={isTriggering || isRunning}
          className={cn(
            "flex-1 px-6 py-4 bg-primary/5 border border-primary/20 hover:bg-primary/10 hover:border-primary/30 text-primary font-bold text-sm uppercase tracking-wider transition-all rounded-lg flex items-center justify-center gap-3 disabled:opacity-50 disabled:cursor-not-allowed",
            isTriggering && "opacity-70"
          )}
        >
          {isTriggering ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />}
          Generate Metadata Info
        </button>

        <button
          onClick={handleCleanAndRegenerate}
          disabled={isTriggering || isRunning}
          className={cn(
            "flex-1 px-6 py-4 bg-destructive/5 border border-destructive/20 hover:bg-destructive/10 hover:border-destructive/30 text-destructive font-bold text-sm uppercase tracking-wider transition-all rounded-lg flex items-center justify-center gap-3 disabled:opacity-50 disabled:cursor-not-allowed",
            isTriggering && "opacity-70"
          )}
        >
          {isTriggering ? <Loader2 className="w-4 h-4 animate-spin" /> : <Trash2 className="w-4 h-4" />}
          Clean & Regenerate All
        </button>
      </div>

      <p className="text-xs text-muted-foreground text-center mt-4">
        "Generate Metadata Info" generates metadata and thumbnails for files that don't have them. "Clean & Regenerate All" clears all metadata and thumbnails first, then regenerates everything.
      </p>

      {status && status.status !== 'idle' && (
        <div className="mt-8 border-t border-border/50 pt-6">
          <div className="flex justify-between text-sm mb-3">
            <span className="text-muted-foreground">
              Status: <span className="font-medium text-foreground uppercase tracking-wide">{status.status}</span>
            </span>
            {status.totalMedia > 0 && (
              <span className="text-muted-foreground font-mono">{status.processedMedia} / {status.totalMedia} files</span>
            )}
          </div>

          {isRunning && status.totalMedia > 0 && (
            <div className="flex items-center gap-3 mb-6">
              <div className="flex-1 bg-muted/50 rounded-full h-2 overflow-hidden">
                <div
                  className="bg-primary h-2 rounded-full transition-all duration-300 shadow-[0_0_10px_hsl(var(--primary))]"
                  style={{ width: `${progress}%` }}
                />
              </div>
              <button
                onClick={handleCancel}
                className="p-1.5 rounded-full bg-muted/50 hover:bg-destructive/10 text-muted-foreground hover:text-destructive transition-colors"
                title="Cancel regeneration"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
          )}

          <div className="grid grid-cols-3 gap-4 text-sm">
            <div className="bg-muted/10 p-3 rounded-lg border border-border/30">
              <span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">Metadata</span>
              <span className="font-bold text-lg text-foreground">{status.updatedMetadata}</span>
            </div>
            <div className="bg-muted/10 p-3 rounded-lg border border-border/30">
              <span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">Thumbnails</span>
              <span className="font-bold text-lg text-foreground">{status.generatedThumbnails}</span>
            </div>
            <div className="bg-muted/10 p-3 rounded-lg border border-border/30">
              <span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">Tags</span>
              <span className="font-bold text-lg text-foreground">{status.updatedTags}</span>
            </div>
          </div>

          {status.errors.length > 0 && (
            <div className="mt-6">
              <h4 className="text-sm font-bold text-destructive mb-2 uppercase tracking-wide flex items-center gap-2">
                Errors
              </h4>
              <ul className="text-xs text-destructive/80 max-h-32 overflow-y-auto bg-destructive/5 p-4 rounded-lg border border-destructive/10 font-mono">
                {status.errors.slice(0, 10).map((err, i) => (
                  <li key={i} className="truncate mb-1 last:mb-0 border-b border-destructive/10 pb-1 last:border-0 last:pb-0">{err}</li>
                ))}
                {status.errors.length > 10 && (
                  <li className="mt-2 italic opacity-70">... and {status.errors.length - 10} more</li>
                )}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

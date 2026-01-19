import { useState, useEffect } from 'react'
import { adminApi } from '../../api/admin'

interface ImportStatus {
  status: string
  total_files: number
  processed_files: number
  successful_imports: number
  failed_imports: number
  started_at: string | null
  completed_at: string | null
  errors: string[]
}

export default function ImportPanel() {
  const [status, setStatus] = useState<ImportStatus | null>(null)
  const [isTriggering, setIsTriggering] = useState(false)
  const [isWebdavTriggering, setIsWebdavTriggering] = useState(false)

  const loadStatus = async () => {
    try {
      const data = await adminApi.getImportStatus()
      setStatus(data)
    } catch {
      console.error('Failed to load import status')
    }
  }

  useEffect(() => {
    loadStatus()
    const interval = setInterval(loadStatus, 2000)
    return () => clearInterval(interval)
  }, [])

  const handleTriggerImport = async () => {
    setIsTriggering(true)
    try {
      await adminApi.triggerImport()
      loadStatus()
    } catch {
      alert('Failed to start import. An import may already be running.')
    } finally {
      setIsTriggering(false)
    }
  }

  const handleTriggerWebdavImport = async () => {
    setIsWebdavTriggering(true)
    try {
      await adminApi.triggerWebdavImport()
      loadStatus()
    } catch {
      alert('Failed to start WebDAV import. Check configuration and ensure no import is running.')
    } finally {
      setIsWebdavTriggering(false)
    }
  }

  const isRunning = status?.status === 'running'
  const progress = status && status.total_files > 0
    ? Math.round((status.processed_files / status.total_files) * 100)
    : 0

  return (
    <div className="bg-card/30 rounded-xl border border-border/50 p-6 backdrop-blur-sm">
      <h3 className="text-lg font-medium mb-4 text-foreground">Import Photos</h3>

      <p className="text-muted-foreground mb-6 font-light">
        Place photos in the <code className="bg-muted px-1.5 py-0.5 rounded text-foreground font-mono text-sm">/data/imports/</code> directory for local import or configure WebDAV in <code className="bg-muted px-1.5 py-0.5 rounded text-foreground font-mono text-sm">/data/config.yaml</code>.
      </p>

      <div className="flex flex-wrap gap-3">
        <button
          onClick={handleTriggerImport}
          disabled={isTriggering || isRunning}
          className="bg-primary text-primary-foreground px-6 py-2 rounded-lg hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed font-medium shadow-lg shadow-primary/20 transition-all"
        >
          {isRunning ? 'Importing...' : isTriggering ? 'Starting...' : 'Start Local Import'}
        </button>
        <button
          onClick={handleTriggerWebdavImport}
          disabled={isWebdavTriggering || isRunning}
          className="bg-secondary text-secondary-foreground px-6 py-2 rounded-lg hover:bg-secondary/90 disabled:opacity-50 disabled:cursor-not-allowed font-medium shadow-lg shadow-secondary/20 transition-all"
        >
          {isRunning ? 'Importing...' : isWebdavTriggering ? 'Starting...' : 'Start WebDAV Import'}
        </button>
      </div>

      {status && status.status !== 'idle' && (
        <div className="mt-8 border-t border-border/50 pt-6">
          <div className="flex justify-between text-sm mb-3">
            <span className="text-muted-foreground">Status: <span className="font-medium text-foreground uppercase tracking-wide">{status.status}</span></span>
            {status.total_files > 0 && (
              <span className="text-muted-foreground font-mono">{status.processed_files} / {status.total_files} files</span>
            )}
          </div>

          {isRunning && status.total_files > 0 && (
            <div className="w-full bg-muted/50 rounded-full h-2 mb-6 overflow-hidden">
              <div
                className="bg-primary h-2 rounded-full transition-all duration-300 shadow-[0_0_10px_hsl(var(--primary))]"
                style={{ width: `${progress}%` }}
              />
            </div>
          )}

          <div className="grid grid-cols-2 gap-4 text-sm">
            <div className="bg-muted/10 p-3 rounded-lg border border-border/30">
              <span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">Successful</span>
              <span className="font-bold text-lg text-green-500">{status.successful_imports}</span>
            </div>
            <div className="bg-muted/10 p-3 rounded-lg border border-border/30">
              <span className="text-muted-foreground block text-xs uppercase tracking-wider mb-1">Failed</span>
              <span className="font-bold text-lg text-destructive">{status.failed_imports}</span>
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

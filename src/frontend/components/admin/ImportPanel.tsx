import { useState } from 'react'
import { importApi, type ImportStatus } from '../../api/import'
import { usePollingStatus } from '../../hooks/usePollingStatus'

export default function ImportPanel() {
  const [isTriggering, setIsTriggering] = useState(false)
  const { status, errorMessage, setErrorMessage, refresh } = usePollingStatus<ImportStatus>(
    importApi.getStatus,
    'Could not load import status.',
    2000,
  )

  const handleTriggerImport = async () => {
    setIsTriggering(true)
    try {
      await importApi.triggerLocal()
      await refresh()
    } catch {
      setErrorMessage('Could not start import. An import may already be running.')
    } finally {
      setIsTriggering(false)
    }
  }

  const isRunning = status?.status === 'running'
  const progress = status && status.totalFiles > 0
    ? Math.round((status.processedFiles / status.totalFiles) * 100)
    : 0

  return (
    <div>
      <button
        onClick={handleTriggerImport}
        type="button"
        disabled={isTriggering || isRunning}
        className="w-full bg-primary text-primary-foreground px-5 py-2.5 rounded-lg hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed font-semibold transition-colors"
      >
        {isRunning ? 'Importing...' : isTriggering ? 'Starting...' : 'Import Media'}
      </button>

      {status && status.status !== 'idle' && (
        <div className="mt-6 border-t border-border/50 pt-5" aria-live="polite">
          <div className="flex justify-between text-sm mb-3">
            <span className="text-muted-foreground">Status: <span className="font-medium text-foreground uppercase tracking-wide">{status.status}</span></span>
            {status.totalFiles > 0 && (
              <span className="text-muted-foreground font-mono">{status.processedFiles} / {status.totalFiles} files</span>
            )}
          </div>

          {isRunning && status.totalFiles > 0 && (
            <div className="w-full bg-muted/50 rounded-full h-2 mb-6 overflow-hidden">
              <div
                role="progressbar"
                aria-label="Import progress"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={progress}
                className="bg-primary h-2 rounded-full transition-all duration-300 shadow-[0_0_10px_hsl(var(--primary))]"
                style={{ width: `${progress}%` }}
              />
            </div>
          )}

          <div className="flex items-center gap-4 text-xs font-medium text-muted-foreground">
            <span>Imported <strong className="text-foreground">{status.successfulImports}</strong></span>
            <span>Failed <strong className={status.failedImports > 0 ? 'text-destructive' : 'text-foreground'}>{status.failedImports}</strong></span>
            <span>Total Media <strong className="text-foreground">{status.totalMedia}</strong></span>
          </div>

          {status.errors.length > 0 && (
            <div className="mt-6">
              <h4 className="text-sm font-bold text-destructive mb-2 uppercase tracking-wide flex items-center gap-2">
                Errors
              </h4>
              <ul className="text-xs text-destructive/80 max-h-32 overflow-y-auto bg-destructive/5 p-4 rounded-lg border border-destructive/10 font-mono">
                {status.errors.slice(0, 10).map((error, errorIndex) => (
                  <li key={errorIndex} className="truncate mb-1 last:mb-0 border-b border-destructive/10 pb-1 last:border-0 last:pb-0">{error}</li>
                ))}
                {status.errors.length > 10 && (
                  <li className="mt-2 italic opacity-70">... and {status.errors.length - 10} more</li>
                )}
              </ul>
            </div>
          )}
        </div>
      )}
      {errorMessage && <p role="alert" className="mt-4 text-sm text-destructive">{errorMessage}</p>}
    </div>
  )
}

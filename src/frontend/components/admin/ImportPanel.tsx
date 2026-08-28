import { useState } from 'react'
import { importApi, type ImportStatus } from '../../api/import'
import { usePollingStatus } from '../../hooks/usePollingStatus'

function importButtonLabel(isRunning: boolean, isTriggering: boolean): string {
  if (isRunning) return 'Importing...'
  if (isTriggering) return 'Starting...'
  return 'Import Media'
}

function ImportErrors({ errors }: { errors: string[] }) {
  if (errors.length === 0) return null

  return (
    <div className="mt-6">
      <h4 className="mb-2 flex items-center gap-2 text-sm font-bold uppercase tracking-wide text-destructive">
        Errors
      </h4>
      <ul className="max-h-32 overflow-y-auto rounded-lg border border-destructive/10 bg-destructive/5 p-4 font-mono text-xs text-destructive/80">
        {errors.slice(0, 10).map((error, errorIndex) => (
          <li
            key={`${errorIndex}-${error}`}
            className="mb-1 truncate border-b border-destructive/10 pb-1 last:mb-0 last:border-0 last:pb-0"
          >
            {error}
          </li>
        ))}
        {errors.length > 10 && (
          <li className="mt-2 italic opacity-70">... and {errors.length - 10} more</li>
        )}
      </ul>
    </div>
  )
}

function ImportProgress({ status }: { status: ImportStatus }) {
  if (status.status === 'idle') return null

  const isRunning = status.status === 'running'
  const hasFiles = status.totalFiles > 0
  const progress = hasFiles ? Math.round((status.processedFiles / status.totalFiles) * 100) : 0

  return (
    <div className="mt-6 border-t border-border/50 pt-5" aria-live="polite">
      <div className="mb-3 flex justify-between text-sm">
        <span className="text-muted-foreground">
          Status:{' '}
          <span className="font-medium uppercase tracking-wide text-foreground">
            {status.status}
          </span>
        </span>
        {hasFiles && (
          <span className="font-mono text-muted-foreground">
            {status.processedFiles} / {status.totalFiles} files
          </span>
        )}
      </div>

      {isRunning && hasFiles && (
        <div className="mb-6 h-2 w-full overflow-hidden rounded-full bg-muted/50">
          <div
            role="progressbar"
            aria-label="Import progress"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={progress}
            className="h-2 rounded-full bg-primary shadow-[0_0_10px_hsl(var(--primary))] transition-all duration-300"
            style={{ width: `${progress}%` }}
          />
        </div>
      )}

      <div className="flex items-center gap-4 text-xs font-medium text-muted-foreground">
        <span>
          Imported <strong className="text-foreground">{status.successfulImports}</strong>
        </span>
        <span>
          Failed{' '}
          <strong className={status.failedImports > 0 ? 'text-destructive' : 'text-foreground'}>
            {status.failedImports}
          </strong>
        </span>
        <span>
          Total Media <strong className="text-foreground">{status.totalMedia}</strong>
        </span>
      </div>

      <ImportErrors errors={status.errors} />
    </div>
  )
}

export default function ImportPanel() {
  const [isTriggering, setIsTriggering] = useState(false)
  const { status, errorMessage, setErrorMessage, refresh } = usePollingStatus<ImportStatus>(
    importApi.getStatus,
    'Could not load import status.',
    2000
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

  return (
    <div>
      <button
        onClick={handleTriggerImport}
        type="button"
        disabled={isTriggering || isRunning}
        className="w-full bg-primary text-primary-foreground px-5 py-2.5 rounded-lg hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:opacity-50 disabled:cursor-not-allowed font-semibold transition-colors"
      >
        {importButtonLabel(isRunning, isTriggering)}
      </button>

      {status && <ImportProgress status={status} />}
      {errorMessage && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {errorMessage}
        </p>
      )}
    </div>
  )
}

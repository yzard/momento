import { useState } from 'react'
import { importApi, type ImportStatus } from '../../api/import'
import { usePollingStatus } from '../../hooks/usePollingStatus'
import { AdminFailureLog, AdminStatusMetrics } from './AdminComponents'

function importButtonLabel(isRunning: boolean, isTriggering: boolean): string {
  if (isRunning) return 'Importing...'
  if (isTriggering) return 'Starting...'
  return 'Start import'
}

function ImportProgress({ status }: { status: ImportStatus }) {
  const isRunning = status.status === 'running'
  const hasFiles = status.totalFiles > 0
  const progress = hasFiles ? Math.round((status.processedFiles / status.totalFiles) * 100) : 0

  if (!isRunning || !hasFiles) return null

  return (
    <div className="mt-5" aria-live="polite">
      <div className="mb-3 flex justify-end text-sm">
        <span className="font-mono text-muted-foreground">
          {status.processedFiles} / {status.totalFiles} files
        </span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-full bg-muted/50">
        <div
          role="progressbar"
          aria-label="Import progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress}
          className="h-2 rounded-full bg-primary transition-[width] duration-300 motion-reduce:transition-none"
          style={{ width: `${progress}%` }}
        />
      </div>
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
  const metrics = [
    { label: 'Status', value: status?.status ?? null },
    { label: 'Imported', value: status?.successfulImports ?? null },
    {
      label: 'Failed',
      value: status?.failedImports ?? null,
      emphasis: Boolean(status?.failedImports),
    },
    { label: 'Total Media', value: status?.totalMedia ?? null },
  ]

  return (
    <div>
      <AdminStatusMetrics metrics={metrics} />
      {status && <ImportProgress status={status} />}
      <button
        onClick={handleTriggerImport}
        type="button"
        disabled={isTriggering || isRunning}
        className="mt-6 min-h-11 w-full cursor-pointer rounded-lg bg-primary px-5 py-2.5 font-semibold text-primary-foreground transition-colors duration-200 hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-50 sm:w-auto"
      >
        {importButtonLabel(isRunning, isTriggering)}
      </button>

      {errorMessage && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {errorMessage}
        </p>
      )}
      <AdminFailureLog title="Import failure log" entries={status?.errors ?? []} />
    </div>
  )
}

import { useState } from 'react'
import { Loader2, RefreshCw } from 'lucide-react'

import { metadataApi, type MetadataStatus } from '../../api/metadata'
import { usePollingStatus } from '../../hooks/usePollingStatus'
import ConfirmationDialog from '../common/ConfirmationDialog'
import { AdminFailureLog, AdminStatusMetrics } from './AdminComponents'

type MetadataAction = 'generate' | 'cancel' | 'clean'

function isMetadataGenerationActive(status: MetadataStatus | null): boolean {
  return status?.status === 'queued' || status?.status === 'processing'
}

export default function MetadataPanel() {
  const [busyAction, setBusyAction] = useState<MetadataAction | null>(null)
  const [pendingClean, setPendingClean] = useState(false)
  const { status, errorMessage, setErrorMessage, refresh } = usePollingStatus<MetadataStatus>(
    metadataApi.getStatus,
    'Could not load metadata status.',
    2000
  )

  const runAction = async (actionName: MetadataAction, action: () => Promise<unknown>) => {
    setBusyAction(actionName)
    try {
      await action()
      await refresh()
    } catch {
      setErrorMessage('Could not complete the metadata action.')
    } finally {
      setBusyAction(null)
    }
  }

  const generationIsActive = isMetadataGenerationActive(status)
  const cancellationIsSettling = status?.status === 'cancelling'
  const cleaningIsActive = status?.status === 'cleaning'
  const primaryAction: MetadataAction = generationIsActive ? 'cancel' : 'generate'
  const primaryLabel = cancellationIsSettling
    ? 'Cancelling…'
    : generationIsActive
      ? 'Cancel'
      : 'Generate'

  return (
    <div>
      <AdminStatusMetrics
        metrics={[
          { label: 'Queued', value: status?.queuedJobs ?? null },
          { label: 'Processing', value: status?.processingJobs ?? null },
          { label: 'Completed', value: status?.completedJobs ?? null },
          {
            label: 'Failed',
            value: status?.failedJobs ?? null,
            emphasis: Boolean(status?.failedJobs),
          },
        ]}
      />
      <div className="mt-6 flex flex-col gap-3 sm:flex-row">
        <button
          type="button"
          onClick={() =>
            void runAction(
              primaryAction,
              generationIsActive ? metadataApi.cancel : metadataApi.generate
            )
          }
          disabled={busyAction !== null || cancellationIsSettling || cleaningIsActive}
          className={`inline-flex min-h-11 w-full cursor-pointer items-center justify-center gap-2 rounded-lg px-8 py-2.5 text-sm font-semibold transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50 sm:w-auto ${
            generationIsActive
              ? 'border border-destructive/40 text-destructive hover:bg-destructive/10 focus-visible:ring-destructive'
              : 'bg-primary text-primary-foreground hover:bg-primary/90 focus-visible:ring-primary'
          }`}
        >
          {busyAction === primaryAction ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <RefreshCw className="w-4 h-4" />
          )}{' '}
          {primaryLabel}
        </button>
        <button
          type="button"
          onClick={() => setPendingClean(true)}
          disabled={
            busyAction !== null || generationIsActive || cancellationIsSettling || cleaningIsActive
          }
          className="inline-flex min-h-11 w-full cursor-pointer items-center justify-center rounded-lg border border-destructive/40 px-8 py-2.5 text-sm font-semibold text-destructive transition-colors duration-200 hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-destructive disabled:cursor-not-allowed disabled:opacity-50 sm:w-auto"
        >
          Clean Data
        </button>
      </div>
      {errorMessage && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {errorMessage}
        </p>
      )}
      <AdminFailureLog title="Metadata failure log" entries={status?.errors ?? []} />
      {pendingClean && (
        <ConfirmationDialog
          title="Clean metadata and AI data?"
          description="This removes generated metadata, thumbnails, and related AI data without deleting original media. Use Generate afterwards to rebuild metadata."
          confirmLabel="Clean Data"
          isProcessing={busyAction === 'clean'}
          destructive
          onConfirm={() => {
            setPendingClean(false)
            void runAction('clean', metadataApi.clean)
          }}
          onCancel={() => setPendingClean(false)}
        />
      )}
    </div>
  )
}

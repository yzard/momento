import { useState } from 'react'
import { Loader2, RefreshCw } from 'lucide-react'

import { metadataApi, type MetadataStatus } from '../../api/metadata'
import { usePollingStatus } from '../../hooks/usePollingStatus'
import ConfirmationDialog from '../common/ConfirmationDialog'
import { AdminFailureLog, AdminStatusMetrics } from './AdminComponents'

export default function MetadataPanel() {
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [pendingReset, setPendingReset] = useState(false)
  const { status, errorMessage, setErrorMessage, refresh } = usePollingStatus<MetadataStatus>(
    metadataApi.getStatus,
    'Could not load metadata status.',
    2000
  )

  const runAction = async (action: () => Promise<unknown>) => {
    setIsSubmitting(true)
    try {
      await action()
      await refresh()
    } catch {
      setErrorMessage('Could not complete the metadata action.')
    } finally {
      setIsSubmitting(false)
    }
  }

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
          onClick={() => void runAction(metadataApi.generate)}
          disabled={isSubmitting}
          className="inline-flex min-h-11 w-full cursor-pointer items-center justify-center gap-2 rounded-lg bg-primary px-8 py-2.5 text-sm font-semibold text-primary-foreground transition-colors duration-200 hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary disabled:cursor-not-allowed disabled:opacity-50 sm:w-auto"
        >
          {isSubmitting ? (
            <Loader2 className="w-4 h-4 animate-spin" />
          ) : (
            <RefreshCw className="w-4 h-4" />
          )}{' '}
          Generate
        </button>
        <button
          type="button"
          onClick={() => setPendingReset(true)}
          disabled={isSubmitting}
          className="inline-flex min-h-11 w-full cursor-pointer items-center justify-center rounded-lg border border-destructive/40 px-8 py-2.5 text-sm font-semibold text-destructive transition-colors duration-200 hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-destructive disabled:cursor-not-allowed disabled:opacity-50 sm:w-auto"
        >
          Reset &amp; regenerate
        </button>
      </div>
      {errorMessage && (
        <p role="alert" className="mt-4 text-sm text-destructive">
          {errorMessage}
        </p>
      )}
      <AdminFailureLog title="Metadata failure log" entries={status?.errors ?? []} />
      {pendingReset && (
        <ConfirmationDialog
          title="Reset metadata and AI data?"
          description="This removes generated metadata and related AI data, then queues metadata generation again. Existing original media is preserved."
          confirmLabel="Reset & regenerate"
          isProcessing={isSubmitting}
          destructive
          onConfirm={() => {
            setPendingReset(false)
            void runAction(metadataApi.reset)
          }}
          onCancel={() => setPendingReset(false)}
        />
      )}
    </div>
  )
}

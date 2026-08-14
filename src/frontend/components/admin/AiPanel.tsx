import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Bot, Eraser, Loader2, Play, RefreshCw, X } from 'lucide-react'

import { aiApi } from '../../api/ai'
import { deduplicateApi } from '../../api/deduplicate'
import { cn } from '../../lib/utils'

export default function AiPanel() {
  const queryClient = useQueryClient()
  const ocrQuery = useQuery({ queryKey: ['ai', 'ocr', 'status'], queryFn: aiApi.getOcrStatus, refetchInterval: 2000 })
  const taggingQuery = useQuery({ queryKey: ['ai', 'image-tagging', 'status'], queryFn: aiApi.getImageTaggingStatus, refetchInterval: 2000 })
  const deduplicationQuery = useQuery({ queryKey: ['deduplicate', 'status'], queryFn: deduplicateApi.status, refetchInterval: 2000 })
  const isRefreshing = ocrQuery.isFetching || taggingQuery.isFetching || deduplicationQuery.isFetching
  const ocrRunning = isActive(ocrQuery.data?.status)
  const taggingRunning = isActive(taggingQuery.data?.status)
  const clusteringRunning = isActive(deduplicationQuery.data?.status)
  const allRunning = ocrRunning || taggingRunning || clusteringRunning
  const actionMutation = useMutation({
    mutationFn: (action: () => Promise<{ message: string; queuedJobs: number }>) => action(),
    onSuccess: () => void Promise.all([
      queryClient.invalidateQueries({ queryKey: ['ai'] }),
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'status'] }),
    ]),
  })

  return (
    <section className="overflow-hidden rounded-xl border border-border bg-white shadow-sm">
      <div className="flex flex-col gap-4 border-b border-border bg-muted/30 px-8 py-6 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center gap-4">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-white text-primary shadow-sm">
            <Bot className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-xl font-display font-semibold text-foreground">AI</h2>
            <p className="text-sm text-muted-foreground">Library processing and similarity-index coverage.</p>
          </div>
        </div>
        <button type="button" onClick={() => void Promise.all([ocrQuery.refetch(), taggingQuery.refetch(), deduplicationQuery.refetch()])} disabled={isRefreshing} className="flex min-h-10 items-center justify-center gap-2 self-start rounded-lg border border-border bg-background px-4 text-sm font-bold text-foreground transition-colors hover:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-50 sm:self-auto">
          <RefreshCw className={cn('h-4 w-4', isRefreshing && 'animate-spin')} />
          Refresh metrics
        </button>
      </div>
      <div className="grid grid-cols-1 divide-y divide-border sm:grid-cols-2 sm:divide-x sm:divide-y-0 lg:grid-cols-5">
        <Metric label="Processed OCR" value={ocrQuery.data?.completedJobs} loading={ocrQuery.isLoading} />
        <Metric label="Processed image tagging" value={taggingQuery.data?.completedJobs} loading={taggingQuery.isLoading} />
        <Metric label="Ensembled media" value={deduplicationQuery.data?.ensembledMedia} loading={deduplicationQuery.isLoading} />
        <Metric label="Deduplication comparisons" value={deduplicationQuery.data?.candidateComparisons} loading={deduplicationQuery.isLoading} />
        <Metric label="Duplicate groups" value={deduplicationQuery.data?.clustersCreated} loading={deduplicationQuery.isLoading} />
      </div>
      <div className="border-t border-border px-8 py-6">
        <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-muted-foreground">AI job controls</h3>
        <div className="space-y-3">
          <ControlRow label="All AI Jobs" running={allRunning} start={() => aiApi.trigger()} cancel={() => aiApi.cancel()} clean={() => aiApi.clean()} mutation={actionMutation} />
          <ControlRow label="OCR" running={ocrRunning} start={() => aiApi.triggerOcr()} cancel={() => aiApi.cancelOcr()} clean={() => aiApi.cleanOcr()} mutation={actionMutation} />
          <ControlRow label="Image Tagging" running={taggingRunning} start={() => aiApi.triggerImageTagging()} cancel={() => aiApi.cancelImageTagging()} clean={() => aiApi.cleanImageTagging()} mutation={actionMutation} />
          <ControlRow label="Image Clustering" running={clusteringRunning} start={() => aiApi.triggerImageClustering()} cancel={() => aiApi.cancelImageClustering()} clean={() => aiApi.cleanImageClustering()} mutation={actionMutation} />
        </div>
        {actionMutation.isError && (
          <p role="alert" className="mt-4 flex items-center gap-2 text-sm text-destructive">
            <AlertCircle className="h-4 w-4" />
            The AI job action failed. Refresh the metrics and try again.
          </p>
        )}
      </div>
    </section>
  )
}

function isActive(status: string | undefined): boolean {
  return status === 'queued' || status === 'processing' || status === 'running' || status === 'cancelling'
}

function ControlRow({ label, running, start, cancel, clean, mutation }: { label: string; running: boolean; start: () => Promise<{ message: string; queuedJobs: number }>; cancel: () => Promise<{ message: string; queuedJobs: number }>; clean: () => Promise<{ message: string; queuedJobs: number }>; mutation: ReturnType<typeof useMutation<{ message: string; queuedJobs: number }, Error, () => Promise<{ message: string; queuedJobs: number }>>> }) {
  return (
    <div className="flex flex-col gap-3 border-b border-border pb-3 last:border-b-0 last:pb-0 sm:flex-row">
      <button type="button" onClick={() => mutation.mutate(running ? cancel : start)} disabled={mutation.isPending} className={cn('flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-bold transition-colors disabled:cursor-not-allowed disabled:opacity-50', running ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90' : 'bg-primary text-primary-foreground hover:bg-primary/90')}>
        {mutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : running ? <X className="h-4 w-4" /> : <Play className="h-4 w-4" />}
        {label}
      </button>
      <button type="button" onClick={() => mutation.mutate(clean)} disabled={running || mutation.isPending} className="flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg border border-destructive/20 bg-destructive/5 px-4 py-2 text-sm font-bold text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-50">
        <Eraser className="h-4 w-4" />
        Clean {label === 'All AI Jobs' ? 'All AI Data' : `${label} Data`}
      </button>
    </div>
  )
}

function Metric({ label, value, loading }: { label: string; value: number | undefined; loading: boolean }) {
  return (
    <div className="min-h-28 px-6 py-5">
      <span className="block text-[10px] font-bold uppercase tracking-wider text-muted-foreground">{label}</span>
      {loading ? <Loader2 className="mt-4 h-5 w-5 animate-spin text-muted-foreground" /> : <span className="mt-3 block font-mono text-2xl font-bold tracking-tight text-foreground">{(value ?? 0).toLocaleString()}</span>}
    </div>
  )
}

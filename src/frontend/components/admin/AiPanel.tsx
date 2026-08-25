import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Bot, Eraser, Loader2, Play, X } from 'lucide-react'

import { aiApi } from '../../api/ai'
import { deduplicateApi } from '../../api/deduplicate'
import { cn } from '../../lib/utils'

export default function AiPanel() {
  const ocrQuery = useQuery({ queryKey: ['ai', 'ocr', 'status'], queryFn: aiApi.getOcrStatus, refetchInterval: 1000 })
  const taggingQuery = useQuery({ queryKey: ['ai', 'image-tagging', 'status'], queryFn: aiApi.getImageTaggingStatus, refetchInterval: 1000 })
  const screenshotDetectionQuery = useQuery({ queryKey: ['ai', 'screenshot-detection', 'status'], queryFn: aiApi.getScreenshotDetectionStatus, refetchInterval: 1000 })
  const documentDetectionQuery = useQuery({ queryKey: ['ai', 'document-detection', 'status'], queryFn: aiApi.getDocumentDetectionStatus, refetchInterval: 1000 })
  const aestheticsQuery = useQuery({ queryKey: ['ai', 'image-aesthetics', 'status'], queryFn: aiApi.getImageAestheticsStatus, refetchInterval: 1000 })
  const deduplicationQuery = useQuery({ queryKey: ['deduplicate', 'status'], queryFn: deduplicateApi.status, refetchInterval: 1000 })
  const facesQuery = useQuery({ queryKey: ['ai', 'faces', 'status'], queryFn: aiApi.getFacesStatus, refetchInterval: 1000 })
  const ocrRunning = isActive(ocrQuery.data?.status)
  const taggingRunning = isActive(taggingQuery.data?.status)
  const screenshotDetectionRunning = isActive(screenshotDetectionQuery.data?.status)
  const documentDetectionRunning = isActive(documentDetectionQuery.data?.status)
  const aestheticsRunning = isActive(aestheticsQuery.data?.status)
  const clusteringRunning = isActive(deduplicationQuery.data?.status)
  const facesRunning = isActive(facesQuery.data?.status)
  const allRunning = ocrRunning || taggingRunning || screenshotDetectionRunning || documentDetectionRunning || aestheticsRunning || clusteringRunning || facesRunning
  return (
    <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm">
      <div className="border-b border-border bg-muted/30 px-8 py-6">
        <div className="flex items-center gap-4">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-border bg-card text-primary shadow-sm">
            <Bot className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-xl font-display font-semibold text-foreground">AI</h2>
            <p className="text-sm text-muted-foreground">Library processing and similarity-index coverage.</p>
          </div>
        </div>
      </div>
      <div className="grid grid-cols-1 divide-y divide-border sm:grid-cols-2 sm:divide-x sm:divide-y-0 lg:grid-cols-5 xl:grid-cols-10">
        <Metric label="Processed OCR" value={ocrQuery.data?.completedJobs} loading={ocrQuery.isLoading} />
        <Metric label="Processed image tagging" value={taggingQuery.data?.completedJobs} loading={taggingQuery.isLoading} />
        <Metric label="Processed screenshot detection" value={screenshotDetectionQuery.data?.completedJobs} loading={screenshotDetectionQuery.isLoading} />
        <Metric label="Processed document detection" value={documentDetectionQuery.data?.completedJobs} loading={documentDetectionQuery.isLoading} />
        <Metric label="Scored image aesthetics" value={aestheticsQuery.data?.completedJobs} loading={aestheticsQuery.isLoading} />
        <Metric label="Image clustering embeddings" value={deduplicationQuery.data?.ensembledMedia} loading={deduplicationQuery.isLoading} />
        <Metric label="Deduplication comparisons" value={deduplicationQuery.data?.candidateComparisons} loading={deduplicationQuery.isLoading} />
        <Metric label="Duplicate groups" value={deduplicationQuery.data?.clustersCreated} loading={deduplicationQuery.isLoading} />
        <Metric label="Processed face detection" value={facesQuery.data?.completedJobs} loading={facesQuery.isLoading} />
        <Metric label="Face groups" value={facesQuery.data?.faceGroups} loading={facesQuery.isLoading} />
      </div>
      <div className="border-t border-border px-8 py-6">
        <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-muted-foreground">AI job controls</h3>
        <div className="space-y-3">
          <ControlRow label="All AI Jobs" running={allRunning} start={() => aiApi.trigger()} cancel={() => aiApi.cancel()} clean={() => aiApi.clean()} />
          <ControlRow label="OCR" running={ocrRunning} start={() => aiApi.triggerOcr()} cancel={() => aiApi.cancelOcr()} clean={() => aiApi.cleanOcr()} />
          <ControlRow label="Image Tagging" running={taggingRunning} start={() => aiApi.triggerImageTagging()} cancel={() => aiApi.cancelImageTagging()} clean={() => aiApi.cleanImageTagging()} />
          <ControlRow label="Screenshot Detection" running={screenshotDetectionRunning} start={() => aiApi.triggerScreenshotDetection()} cancel={() => aiApi.cancelScreenshotDetection()} clean={() => aiApi.cleanScreenshotDetection()} />
          <ControlRow label="Document Detection" running={documentDetectionRunning} start={() => aiApi.triggerDocumentDetection()} cancel={() => aiApi.cancelDocumentDetection()} clean={() => aiApi.cleanDocumentDetection()} />
          <ControlRow label="Image Aesthetics" running={aestheticsRunning} start={() => aiApi.triggerImageAesthetics()} cancel={() => aiApi.cancelImageAesthetics()} clean={() => aiApi.cleanImageAesthetics()} />
          <ControlRow label="Image Clustering (Duplicate)" running={clusteringRunning} start={() => aiApi.triggerImageClustering()} cancel={() => aiApi.cancelImageClustering()} clean={() => aiApi.cleanImageClustering()} />
          <ControlRow label="Face Detection" running={facesRunning} start={() => aiApi.startFaces()} cancel={() => aiApi.cancelFaces()} clean={() => aiApi.cleanFaces()} />
        </div>
      </div>
    </section>
  )
}

function isActive(status: string | undefined): boolean {
  return status === 'queued' || status === 'processing' || status === 'running' || status === 'cancelling'
}

function ControlRow({ label, running, start, cancel, clean }: { label: string; running: boolean; start: () => Promise<{ message: string; queuedJobs: number }>; cancel: () => Promise<{ message: string; queuedJobs: number }>; clean: () => Promise<{ message: string; queuedJobs: number }> }) {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: (action: () => Promise<{ message: string; queuedJobs: number }>) => action(),
    onSuccess: () => void Promise.all([
      queryClient.invalidateQueries({ queryKey: ['ai'] }),
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'status'] }),
      queryClient.invalidateQueries({ queryKey: ['faces'] }),
    ]),
  })
  return (
    <div className="border-b border-border pb-3 last:border-b-0 last:pb-0">
      <div className="flex flex-col gap-3 sm:flex-row">
        <button type="button" onClick={() => mutation.mutate(running ? cancel : start)} disabled={mutation.isPending} className={cn('flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-bold transition-colors disabled:cursor-not-allowed disabled:opacity-50', running ? 'bg-destructive text-destructive-foreground hover:bg-destructive/90' : 'bg-primary text-primary-foreground hover:bg-primary/90')}>
          {mutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : running ? <X className="h-4 w-4" /> : <Play className="h-4 w-4" />}
          {label}
        </button>
        <button type="button" onClick={() => mutation.mutate(clean)} disabled={running || mutation.isPending} className="flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg border border-destructive/20 bg-destructive/5 px-4 py-2 text-sm font-bold text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-50">
          <Eraser className="h-4 w-4" />
          Clean {label === 'All AI Jobs' ? 'All AI Data' : `${label} Data`}
        </button>
      </div>
      {mutation.isError && <p role="alert" className="mt-2 flex items-center gap-2 text-sm text-destructive"><AlertCircle className="h-4 w-4" />The {label} action failed. Metrics update automatically; try again.</p>}
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

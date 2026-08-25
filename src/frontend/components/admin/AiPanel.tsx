import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Bot, Eraser, Loader2, Play, X } from 'lucide-react'

import { aiApi, type AiActionResponse, type AiFeature } from '../../api/ai'
import { cn } from '../../lib/utils'

export default function AiPanel() {
  const statusQuery = useQuery({ queryKey: ['ai', 'status'], queryFn: aiApi.status, refetchInterval: 1000 })
  const taskStatus = (task: Exclude<AiFeature, 'deduplicate'>) => statusQuery.data?.tasks.find((status) => status.task === task)
  const ocrStatus = taskStatus('ocr')
  const taggingStatus = taskStatus('image_tagging')
  const screenshotDetectionStatus = taskStatus('screenshot_detection')
  const documentDetectionStatus = taskStatus('document_detection')
  const aestheticsStatus = taskStatus('image_aesthetics')
  const faceDetectionStatus = taskStatus('face_detection')
  const deduplicateStatus = statusQuery.data?.deduplicate
  const ocrRunning = isActive(ocrStatus?.state)
  const taggingRunning = isActive(taggingStatus?.state)
  const screenshotDetectionRunning = isActive(screenshotDetectionStatus?.state)
  const documentDetectionRunning = isActive(documentDetectionStatus?.state)
  const aestheticsRunning = isActive(aestheticsStatus?.state)
  const clusteringRunning = isActive(deduplicateStatus?.status)
  const facesRunning = isActive(faceDetectionStatus?.state)
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
        <Metric label="Processed OCR" value={ocrStatus?.jobs.completed} loading={statusQuery.isLoading} />
        <Metric label="Processed image tagging" value={taggingStatus?.jobs.completed} loading={statusQuery.isLoading} />
        <Metric label="Processed screenshot detection" value={screenshotDetectionStatus?.jobs.completed} loading={statusQuery.isLoading} />
        <Metric label="Processed document detection" value={documentDetectionStatus?.jobs.completed} loading={statusQuery.isLoading} />
        <Metric label="Scored image aesthetics" value={aestheticsStatus?.jobs.completed} loading={statusQuery.isLoading} />
        <Metric label="Image clustering embeddings" value={deduplicateStatus?.ensembledMedia} loading={statusQuery.isLoading} />
        <Metric label="Deduplication comparisons" value={deduplicateStatus?.candidateComparisons} loading={statusQuery.isLoading} />
        <Metric label="Duplicate groups" value={deduplicateStatus?.clustersCreated} loading={statusQuery.isLoading} />
        <Metric label="Processed face detection" value={faceDetectionStatus?.jobs.completed} loading={statusQuery.isLoading} />
        <Metric label="Face groups" value={statusQuery.data?.faceGroups} loading={statusQuery.isLoading} />
      </div>
      <div className="border-t border-border px-8 py-6">
        <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-muted-foreground">AI job controls</h3>
        <div className="space-y-3">
          <ControlRow label="All AI Jobs" running={allRunning} start={() => aiApi.start()} cancel={() => aiApi.cancel()} clean={() => aiApi.clean()} />
          <ControlRow label="OCR" running={ocrRunning} start={() => aiApi.startFeature('ocr')} cancel={() => aiApi.cancelFeature('ocr')} clean={() => aiApi.cleanFeature('ocr')} />
          <ControlRow label="Image Tagging" running={taggingRunning} start={() => aiApi.startFeature('image_tagging')} cancel={() => aiApi.cancelFeature('image_tagging')} clean={() => aiApi.cleanFeature('image_tagging')} />
          <ControlRow label="Screenshot Detection" running={screenshotDetectionRunning} start={() => aiApi.startFeature('screenshot_detection')} cancel={() => aiApi.cancelFeature('screenshot_detection')} clean={() => aiApi.cleanFeature('screenshot_detection')} />
          <ControlRow label="Document Detection" running={documentDetectionRunning} start={() => aiApi.startFeature('document_detection')} cancel={() => aiApi.cancelFeature('document_detection')} clean={() => aiApi.cleanFeature('document_detection')} />
          <ControlRow label="Image Aesthetics" running={aestheticsRunning} start={() => aiApi.startFeature('image_aesthetics')} cancel={() => aiApi.cancelFeature('image_aesthetics')} clean={() => aiApi.cleanFeature('image_aesthetics')} />
          <ControlRow label="Deduplicate" running={clusteringRunning} start={() => aiApi.startFeature('deduplicate')} cancel={() => aiApi.cancelFeature('deduplicate')} clean={() => aiApi.cleanFeature('deduplicate')} />
          <ControlRow label="Face Detection" running={facesRunning} start={() => aiApi.startFeature('face_detection')} cancel={() => aiApi.cancelFeature('face_detection')} clean={() => aiApi.cleanFeature('face_detection')} />
        </div>
      </div>
    </section>
  )
}

function isActive(status: string | undefined): boolean {
  return status === 'queued' || status === 'submitting' || status === 'submitted' || status === 'running' || status === 'cancelling'
}

function ControlRow({ label, running, start, cancel, clean }: { label: string; running: boolean; start: () => Promise<AiActionResponse>; cancel: () => Promise<AiActionResponse>; clean: () => Promise<AiActionResponse> }) {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: (action: () => Promise<AiActionResponse>) => action(),
    onSuccess: () => void Promise.all([
      queryClient.invalidateQueries({ queryKey: ['ai'] }),
      queryClient.invalidateQueries({ queryKey: ['duplicates'] }),
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

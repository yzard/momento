import { useQuery } from '@tanstack/react-query'
import { Bot, Loader2, RefreshCw } from 'lucide-react'

import { aiApi } from '../../api/ai'
import { deduplicateApi } from '../../api/deduplicate'
import { cn } from '../../lib/utils'
import DeduplicatePanel from './DeduplicatePanel'

export default function AiPanel() {
  const ocrQuery = useQuery({ queryKey: ['ai', 'ocr', 'status'], queryFn: aiApi.getOcrStatus, refetchInterval: 2000 })
  const taggingQuery = useQuery({ queryKey: ['ai', 'image-tagging', 'status'], queryFn: aiApi.getImageTaggingStatus, refetchInterval: 2000 })
  const deduplicationQuery = useQuery({ queryKey: ['deduplicate', 'status'], queryFn: deduplicateApi.status, refetchInterval: 2000 })
  const isRefreshing = ocrQuery.isFetching || taggingQuery.isFetching || deduplicationQuery.isFetching

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
        <Metric label="Indexed deduplication" value={deduplicationQuery.data?.indexedMedia} loading={deduplicationQuery.isLoading} />
        <Metric label="Deduplication comparisons" value={deduplicationQuery.data?.candidateComparisons} loading={deduplicationQuery.isLoading} />
        <Metric label="Duplicate groups" value={deduplicationQuery.data?.clustersCreated} loading={deduplicationQuery.isLoading} />
      </div>
      <div className="border-t border-border px-8 py-6">
        <h3 className="mb-4 text-sm font-bold uppercase tracking-wider text-muted-foreground">Deduplication controls</h3>
        <DeduplicatePanel />
      </div>
    </section>
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

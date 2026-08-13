import { useEffect, useRef } from 'react'
import { useMutation, useQuery, useQueryClient, type UseQueryResult } from '@tanstack/react-query'
import { AlertCircle, Eraser, Loader2, RefreshCw, ScanSearch, X } from 'lucide-react'

import { deduplicateApi } from '../../api/deduplicate'
import { cn } from '../../lib/utils'

function isJobRunning(status: string | undefined): boolean {
  return status === 'running' || status === 'cancelling'
}

export default function DeduplicatePanel({ statusQuery: sharedStatusQuery }: { statusQuery?: UseQueryResult<Awaited<ReturnType<typeof deduplicateApi.status>>> }) {
  const queryClient = useQueryClient()
  const previousStatus = useRef<string | null>(null)
  const localStatusQuery = useQuery({
    queryKey: ['deduplicate', 'status'],
    queryFn: deduplicateApi.status,
    enabled: sharedStatusQuery === undefined,
    refetchInterval: (query) => isJobRunning(query.state.data?.status) ? 2000 : false,
  })
  const statusQuery = sharedStatusQuery ?? localStatusQuery

  useEffect(() => {
    const currentStatus = statusQuery.data?.status
    if (!currentStatus) return
    if (isJobRunning(previousStatus.current ?? undefined) && !isJobRunning(currentStatus)) {
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'groups'] })
    }
    previousStatus.current = currentStatus
  }, [queryClient, statusQuery.data?.status])

  const startMutation = useMutation({
    mutationFn: deduplicateApi.start,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['deduplicate', 'status'] }),
  })
  const cancelMutation = useMutation({
    mutationFn: deduplicateApi.cancel,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['deduplicate', 'status'] }),
  })
  const cleanMutation = useMutation({
    mutationFn: deduplicateApi.clean,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'status'] })
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'groups'] })
    },
  })

  const handleClean = () => {
    if (!window.confirm('Clear all deduplication indexes and scan results? Your media will not be deleted.')) return
    cleanMutation.mutate()
  }
  const running = isJobRunning(statusQuery.data?.status)
  const actionError = startMutation.isError || cancelMutation.isError || cleanMutation.isError

  return (
    <div>
      <div className="mb-5 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <p className="text-sm text-muted-foreground">
          Scan the global library for near-duplicates and bursts. Cleaning removes generated indexes and groups only.
        </p>
        <div className="flex items-center gap-2 self-start rounded-full border border-border bg-background px-3 py-1.5 text-xs font-bold uppercase tracking-wider text-muted-foreground sm:self-auto">
          {running && <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />}
          {statusQuery.isError ? 'Status unavailable' : statusQuery.data?.status ?? 'Loading status'}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <button
          type="button"
          onClick={() => startMutation.mutate()}
          disabled={running || startMutation.isPending || cleanMutation.isPending}
          className="flex min-h-11 items-center justify-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-bold text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {startMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <ScanSearch className="h-4 w-4" />}
          {startMutation.isPending ? 'Starting...' : 'Start scan'}
        </button>
        <button
          type="button"
          onClick={() => statusQuery.refetch()}
          disabled={statusQuery.isFetching}
          className="flex min-h-11 items-center justify-center gap-2 rounded-lg border border-border bg-background px-4 py-2 text-sm font-bold text-foreground transition-colors hover:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <RefreshCw className={cn('h-4 w-4', statusQuery.isFetching && 'animate-spin')} />
          Refresh status
        </button>
        <button
          type="button"
          onClick={() => cancelMutation.mutate()}
          disabled={!running || cancelMutation.isPending}
          className="flex min-h-11 items-center justify-center gap-2 rounded-lg border border-border bg-background px-4 py-2 text-sm font-bold text-foreground transition-colors hover:bg-muted/50 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {cancelMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <X className="h-4 w-4" />}
          {cancelMutation.isPending ? 'Cancelling...' : 'Cancel scan'}
        </button>
        <button
          type="button"
          onClick={handleClean}
          disabled={running || cleanMutation.isPending}
          className="flex min-h-11 items-center justify-center gap-2 rounded-lg border border-destructive/20 bg-destructive/5 px-4 py-2 text-sm font-bold text-destructive transition-colors hover:bg-destructive/10 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {cleanMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Eraser className="h-4 w-4" />}
          {cleanMutation.isPending ? 'Cleaning...' : 'Clean indexes'}
        </button>
      </div>

      {actionError && (
        <p role="alert" className="mt-4 flex items-center gap-2 text-sm text-destructive">
          <AlertCircle className="h-4 w-4" />
          The administrative action failed. Refresh the status and try again.
        </p>
      )}
      {statusQuery.data?.error && (
        <p role="alert" className="mt-4 rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-sm text-destructive">
          {statusQuery.data.error}
        </p>
      )}
    </div>
  )
}

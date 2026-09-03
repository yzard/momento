import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { trashApi, type TrashMedia } from '../api/trash'
import { AlertCircle, AlertTriangle, Loader2, RotateCcw, Trash2 } from 'lucide-react'
import { cn } from '../lib/utils'
import { trashThumbnailUrlLoader } from '../utils/assetUrlLoader'
import { invalidateMediaConsumers, queryKeys } from '../lib/queryKeys'
import { useLazyImage } from '../hooks/useLazyImage'
import ConfirmationDialog from '../components/common/ConfirmationDialog'
import PageState from '../components/common/PageState'
import { PageFrame, PageHeader } from '../components/layout/PageLayout'

type TrashConfirmation = 'selected' | 'all' | null

function formatDaysRemaining(deletedAt: string): string {
  const expiry = new Date(new Date(deletedAt).getTime() + 30 * 24 * 60 * 60 * 1000)
  const daysLeft = Math.ceil((expiry.getTime() - Date.now()) / (24 * 60 * 60 * 1000))
  return daysLeft > 0 ? `${daysLeft} days left` : 'Expiring soon'
}

function useTrashActions(items: TrashMedia[]) {
  const queryClient = useQueryClient()
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  const mutationOptions = {
    onSuccess: () => {
      void invalidateMediaConsumers(queryClient)
      setSelectedIds(new Set())
    },
  }
  const restoreMutation = useMutation({ mutationFn: trashApi.restore, ...mutationOptions })
  const deleteMutation = useMutation({ mutationFn: trashApi.permanentlyDelete, ...mutationOptions })
  const emptyMutation = useMutation({ mutationFn: trashApi.emptyTrash, ...mutationOptions })
  const toggle = (id: number) =>
    setSelectedIds((current) => {
      const next = new Set(current)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  const restore = () => {
    if (selectedIds.size > 0) restoreMutation.mutate(Array.from(selectedIds))
  }
  const permanentlyDelete = async () => {
    if (selectedIds.size > 0) await deleteMutation.mutateAsync(Array.from(selectedIds))
  }
  const empty = async () => {
    await emptyMutation.mutateAsync()
  }
  return {
    selectedIds,
    toggle,
    selectAll: () => setSelectedIds(new Set(items.map((item) => item.id))),
    deselectAll: () => setSelectedIds(new Set()),
    restore,
    permanentlyDelete,
    empty,
    isProcessing: restoreMutation.isPending || deleteMutation.isPending || emptyMutation.isPending,
    hasError: restoreMutation.isError || deleteMutation.isError || emptyMutation.isError,
  }
}

interface TrashToolbarProps {
  actions: ReturnType<typeof useTrashActions>
  onDeleteRequest: () => void
  onEmptyRequest: () => void
}

function TrashToolbar({ actions, onDeleteRequest, onEmptyRequest }: TrashToolbarProps) {
  if (actions.selectedIds.size > 0) {
    return (
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={actions.deselectAll}
          className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground"
        >
          Deselect ({actions.selectedIds.size})
        </button>
        <button
          type="button"
          onClick={actions.restore}
          disabled={actions.isProcessing}
          className="flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-bold uppercase tracking-wider text-primary-foreground disabled:opacity-50"
        >
          <RotateCcw className="h-4 w-4" />
          Restore
        </button>
        <button
          type="button"
          onClick={onDeleteRequest}
          disabled={actions.isProcessing}
          className="flex items-center gap-2 rounded-lg bg-destructive px-4 py-2 text-sm font-bold uppercase tracking-wider text-destructive-foreground disabled:opacity-50"
        >
          <Trash2 className="h-4 w-4" />
          Delete Forever
        </button>
      </div>
    )
  }
  return (
    <div className="flex flex-wrap gap-2">
      <button
        type="button"
        onClick={actions.selectAll}
        className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground"
      >
        Select All
      </button>
      <button
        type="button"
        onClick={onEmptyRequest}
        disabled={actions.isProcessing}
        className="flex items-center gap-2 rounded-lg bg-destructive/10 px-4 py-2 text-sm font-bold uppercase tracking-wider text-destructive disabled:opacity-50"
      >
        <Trash2 className="h-4 w-4" />
        Empty Trash
      </button>
    </div>
  )
}

export default function Trash() {
  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.trash.all,
    queryFn: trashApi.list,
  })

  const items = data?.items ?? []
  const actions = useTrashActions(items)
  const [confirmation, setConfirmation] = useState<TrashConfirmation>(null)

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <PageFrame className="animate-fade-in">
        <PageHeader
          title="Trash"
          description="Items are automatically deleted after 30 days."
          actions={
            !isLoading && !error && items.length > 0 ? (
              <TrashToolbar
                actions={actions}
                onDeleteRequest={() => setConfirmation('selected')}
                onEmptyRequest={() => setConfirmation('all')}
              />
            ) : null
          }
        />

        {isLoading ? (
          <PageState
            icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />}
            title="Loading Trash"
            description="Retrieving recently deleted media..."
          />
        ) : error ? (
          <PageState
            icon={<AlertCircle className="h-9 w-9 text-destructive" />}
            title="Unable to load Trash"
            description="Try the request again. Deleted media has not changed."
          />
        ) : items.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <Trash2 className="w-16 h-16 text-muted-foreground/30 mb-4" />
            <h2 className="text-xl font-semibold text-foreground mb-2">Trash is empty</h2>
            <p className="text-muted-foreground">Deleted items will appear here</p>
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-8">
            {items.map((item) => (
              <TrashItem
                key={item.id}
                item={item}
                selected={actions.selectedIds.has(item.id)}
                onToggle={() => actions.toggle(item.id)}
                daysRemaining={formatDaysRemaining(item.deletedAt)}
              />
            ))}
          </div>
        )}
        {actions.hasError && (
          <p
            role="alert"
            className="mt-6 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive"
          >
            The Trash operation failed. No unconfirmed changes were applied.
          </p>
        )}
      </PageFrame>
      {confirmation && (
        <ConfirmationDialog
          title={
            confirmation === 'all'
              ? 'Permanently empty Trash?'
              : `Permanently delete ${actions.selectedIds.size} selected item${actions.selectedIds.size === 1 ? '' : 's'}?`
          }
          description="This operation cannot be undone. The original files will be permanently removed."
          confirmLabel={confirmation === 'all' ? 'Empty Trash' : 'Delete forever'}
          isProcessing={actions.isProcessing}
          destructive
          onConfirm={() => {
            const operation = confirmation === 'all' ? actions.empty() : actions.permanentlyDelete()
            void operation.finally(() => setConfirmation(null))
          }}
          onCancel={() => setConfirmation(null)}
        />
      )}
    </div>
  )
}

interface TrashItemProps {
  item: TrashMedia
  selected: boolean
  onToggle: () => void
  daysRemaining: string
}

function TrashItem({ item, selected, onToggle, daysRemaining }: TrashItemProps) {
  const { targetRef: containerRef, imageUrl: thumbnailUrl } = useLazyImage<HTMLDivElement, number>({
    resourceId: item.id,
    loader: trashThumbnailUrlLoader,
    getCachedUrl: null,
    rootMargin: '400px',
  })

  return (
    <div
      ref={containerRef}
      className={cn(
        'relative aspect-square rounded-lg overflow-hidden cursor-pointer group transition-all',
        selected ? 'ring-4 ring-primary' : 'hover:ring-2 hover:ring-primary/50'
      )}
      onClick={onToggle}
    >
      {thumbnailUrl ? (
        <img
          src={thumbnailUrl}
          alt={item.originalFilename}
          className="w-full h-full object-cover"
        />
      ) : (
        <div className="w-full h-full bg-muted animate-pulse" />
      )}

      <div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity" />

      <div className="absolute top-2 left-2">
        <div
          className={cn(
            'w-6 h-6 rounded-full border-2 flex items-center justify-center transition-colors',
            selected
              ? 'bg-primary border-primary text-primary-foreground'
              : 'bg-black/50 border-white/50 group-hover:border-white'
          )}
        >
          {selected && <span className="text-xs font-bold">✓</span>}
        </div>
      </div>

      <div className="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/80 to-transparent p-2">
        <p className="text-white text-xs font-medium truncate">{item.originalFilename}</p>
        <p className="text-white/70 text-[10px] flex items-center gap-1">
          <AlertTriangle className="w-3 h-3" />
          {daysRemaining}
        </p>
      </div>
    </div>
  )
}

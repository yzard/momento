import { useState, useEffect, useRef } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { trashApi, type TrashMedia } from '../api/trash'
import { Trash2, RotateCcw, AlertTriangle, Loader2 } from 'lucide-react'
import { cn } from '../lib/utils'
import { batchLoader } from '../utils/batcher'

export default function Trash() {
  const queryClient = useQueryClient()
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())

  const { data, isLoading, error } = useQuery({
    queryKey: ['trash'],
    queryFn: trashApi.list,
  })

  const restoreMutation = useMutation({
    mutationFn: trashApi.restore,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['trash'] })
      queryClient.invalidateQueries({ queryKey: ['timeline'] })
      queryClient.invalidateQueries({ queryKey: ['media'] })
      setSelectedIds(new Set())
    },
  })

  const deleteMutation = useMutation({
    mutationFn: trashApi.permanentlyDelete,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['trash'] })
      setSelectedIds(new Set())
    },
  })

  const emptyMutation = useMutation({
    mutationFn: trashApi.emptyTrash,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['trash'] })
      setSelectedIds(new Set())
    },
  })

  const toggleSelect = (id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const selectAll = () => {
    if (data?.items) {
      setSelectedIds(new Set(data.items.map((item) => item.id)))
    }
  }

  const deselectAll = () => {
    setSelectedIds(new Set())
  }

  const handleRestore = () => {
    if (selectedIds.size > 0) {
      restoreMutation.mutate(Array.from(selectedIds))
    }
  }

  const handleDelete = () => {
    if (selectedIds.size > 0 && confirm('Permanently delete selected items? This cannot be undone.')) {
      deleteMutation.mutate(Array.from(selectedIds))
    }
  }

  const handleEmptyTrash = () => {
    if (confirm('Permanently delete ALL items in trash? This cannot be undone.')) {
      emptyMutation.mutate()
    }
  }

  const formatDaysRemaining = (deletedAt: string): string => {
    const deleted = new Date(deletedAt)
    const expiry = new Date(deleted.getTime() + 30 * 24 * 60 * 60 * 1000)
    const now = new Date()
    const daysLeft = Math.ceil((expiry.getTime() - now.getTime()) / (24 * 60 * 60 * 1000))
    return daysLeft > 0 ? `${daysLeft} days left` : 'Expiring soon'
  }

  const isProcessing = restoreMutation.isPending || deleteMutation.isPending || emptyMutation.isPending

  if (isLoading) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <p className="text-destructive">Failed to load trash</p>
      </div>
    )
  }

  const items = data?.items || []

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="max-w-7xl mx-auto animate-fade-in px-6 md:px-10 py-6 md:py-10">
        <div className="mb-8 flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div>
            <h1 className="text-3xl font-display font-bold text-foreground tracking-tight flex items-center gap-3">
              <Trash2 className="w-8 h-8" />
              Trash
            </h1>
            <p className="mt-1 text-muted-foreground font-medium">
              Items are automatically deleted after 30 days.
            </p>
          </div>

          {items.length > 0 && (
            <div className="flex flex-wrap gap-2">
              {selectedIds.size > 0 ? (
                <>
                  <button
                    onClick={deselectAll}
                    className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
                  >
                    Deselect ({selectedIds.size})
                  </button>
                  <button
                    onClick={handleRestore}
                    disabled={isProcessing}
                    className="px-4 py-2 bg-primary text-primary-foreground text-sm font-bold uppercase tracking-wider rounded-lg hover:bg-primary/90 disabled:opacity-50 flex items-center gap-2"
                  >
                    <RotateCcw className="w-4 h-4" />
                    Restore
                  </button>
                  <button
                    onClick={handleDelete}
                    disabled={isProcessing}
                    className="px-4 py-2 bg-destructive text-destructive-foreground text-sm font-bold uppercase tracking-wider rounded-lg hover:bg-destructive/90 disabled:opacity-50 flex items-center gap-2"
                  >
                    <Trash2 className="w-4 h-4" />
                    Delete Forever
                  </button>
                </>
              ) : (
                <>
                  <button
                    onClick={selectAll}
                    className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
                  >
                    Select All
                  </button>
                  <button
                    onClick={handleEmptyTrash}
                    disabled={isProcessing}
                    className="px-4 py-2 bg-destructive/10 text-destructive text-sm font-bold uppercase tracking-wider rounded-lg hover:bg-destructive/20 disabled:opacity-50 flex items-center gap-2"
                  >
                    <Trash2 className="w-4 h-4" />
                    Empty Trash
                  </button>
                </>
              )}
            </div>
          )}
        </div>

        {items.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-20 text-center">
            <Trash2 className="w-16 h-16 text-muted-foreground/30 mb-4" />
            <h2 className="text-xl font-semibold text-foreground mb-2">Trash is empty</h2>
            <p className="text-muted-foreground">Deleted items will appear here</p>
          </div>
        ) : (
          <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
            {items.map((item) => (
              <TrashItem
                key={item.id}
                item={item}
                selected={selectedIds.has(item.id)}
                onToggle={() => toggleSelect(item.id)}
                daysRemaining={formatDaysRemaining(item.deletedAt)}
              />
            ))}
          </div>
        )}
      </div>
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
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!containerRef.current) return
    let cancelled = false

    const loadThumbnail = async () => {
      try {
        const url = await batchLoader.load(item.id)
        if (!cancelled && url) setThumbnailUrl(url)
      } catch (err) {
        console.error('Failed to load thumbnail:', err)
      }
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          loadThumbnail()
          observer.disconnect()
        }
      },
      { rootMargin: '100px' }
    )
    observer.observe(containerRef.current)

    return () => {
      cancelled = true
      observer.disconnect()
    }
  }, [item.id])

  return (
    <div
      ref={containerRef}
      className={cn(
        "relative aspect-square rounded-lg overflow-hidden cursor-pointer group transition-all",
        selected ? "ring-4 ring-primary" : "hover:ring-2 hover:ring-primary/50"
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
            "w-6 h-6 rounded-full border-2 flex items-center justify-center transition-colors",
            selected
              ? "bg-primary border-primary text-primary-foreground"
              : "bg-black/50 border-white/50 group-hover:border-white"
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

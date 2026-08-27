import { useState, useEffect, useRef } from 'react'
import { useAlbum, useRemoveAlbumMedia, useReorderAlbum } from '../../hooks/useAlbums'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'
import { ArrowLeft, Check, Circle, Loader2 } from 'lucide-react'
import { cn } from '../../lib/utils'
import { batchLoader } from '../../utils/batcher'
import { useMediaSelection } from '../../hooks/useMediaSelection'
import ConfirmationDialog from '../common/ConfirmationDialog'
import MediaSelectionToolbar, { MediaSelectButton } from '../media/MediaSelectionToolbar'

interface AlbumViewProps {
  albumId: number
  onBack: () => void
  onPhotoClick: (media: Media, allMedia: Media[]) => void
}

export default function AlbumView({ albumId, onBack, onPhotoClick }: AlbumViewProps) {
  const { data: album, isLoading, error } = useAlbum(albumId)
  const reorderAlbum = useReorderAlbum()
  const removeAlbumMedia = useRemoveAlbumMedia()

  const [items, setItems] = useState<Media[]>([])
  const [draggedId, setDraggedId] = useState<number | null>(null)
  const [reorderError, setReorderError] = useState<string | null>(null)
  const [removeError, setRemoveError] = useState<string | null>(null)
  const [showRemoveConfirmation, setShowRemoveConfirmation] = useState(false)
  const itemsRef = useRef<Media[]>([])
  const confirmedItemsRef = useRef<Media[]>([])
  const reorderInFlightRef = useRef(false)
  const {
    selectionMode,
    selectedMediaIds,
    startSelection,
    clearSelection,
    cancelSelection,
    toggleSelection,
  } = useMediaSelection()

  useEffect(() => {
    cancelSelection()
    setShowRemoveConfirmation(false)
    setRemoveError(null)
  }, [albumId, cancelSelection])

  useEffect(() => {
    if (!album || draggedId !== null || reorderInFlightRef.current) return
    itemsRef.current = album.media
    confirmedItemsRef.current = album.media
    setItems(album.media)
  }, [album, draggedId])

  const handleDragStart = (e: React.DragEvent, id: number) => {
    if (selectionMode) return
    setDraggedId(id)
    e.dataTransfer.effectAllowed = 'move'
  }

  const handleDragOver = (e: React.DragEvent, targetId: number) => {
    e.preventDefault()
    if (selectionMode || draggedId === null || draggedId === targetId) return
    const nextItems = moveAlbumMedia(itemsRef.current, draggedId, targetId)
    itemsRef.current = nextItems
    setItems(nextItems)
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    if (selectionMode || draggedId === null || itemsRef.current.length === 0 || reorderInFlightRef.current) return
    const desiredItems = itemsRef.current
    const confirmedItems = confirmedItemsRef.current
    setDraggedId(null)
    if (desiredItems.every((item, index) => item.id === confirmedItems[index]?.id)) return

    reorderInFlightRef.current = true
    setReorderError(null)
    void reorderAlbum.mutateAsync({
      albumId,
      mediaIds: desiredItems.map((item) => item.id),
    }).then(() => {
      confirmedItemsRef.current = desiredItems
    }).catch(() => {
      itemsRef.current = confirmedItems
      setItems(confirmedItems)
      setReorderError('Could not save the album order.')
    }).finally(() => {
      reorderInFlightRef.current = false
    })
  }

  const handleRemoveSelected = () => {
    if (selectedMediaIds.size === 0 || removeAlbumMedia.isPending) return
    const mediaIds = Array.from(selectedMediaIds)
    const removedMediaIds = new Set(selectedMediaIds)
    setRemoveError(null)
    removeAlbumMedia.mutate(
      { albumId, mediaIds },
      {
        onSuccess: () => {
          const remainingItems = itemsRef.current.filter((item) => !removedMediaIds.has(item.id))
          itemsRef.current = remainingItems
          confirmedItemsRef.current = remainingItems
          setItems(remainingItems)
          setShowRemoveConfirmation(false)
          cancelSelection()
        },
        onError: () => {
          setShowRemoveConfirmation(false)
          setRemoveError('Could not remove the selected media from this album. Nothing was removed.')
        },
      },
    )
  }

  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-muted-foreground gap-3">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
        <p className="text-sm font-medium">Loading album...</p>
      </div>
    )
  }

  if (error || !album) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-destructive gap-3">
        <p className="text-lg font-semibold">Unable to load album</p>
        <button onClick={onBack} className="text-sm underline hover:text-destructive/80">
          Go back
        </button>
      </div>
    )
  }

  return (
    <div className="animate-fade-in">
      <div className="flex flex-col gap-6 mb-8">
        <button 
          onClick={onBack} 
          className="flex items-center gap-2 text-muted-foreground hover:text-foreground transition-colors w-fit group"
        >
          <div className="p-2 rounded-full bg-muted/50 group-hover:bg-muted transition-colors">
            <ArrowLeft className="w-4 h-4" />
          </div>
          <span className="font-medium text-sm">Back to Albums</span>
        </button>
        
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h2 className="text-4xl font-display font-bold text-foreground tracking-tight">{album.name}</h2>
            {!selectionMode && <MediaSelectButton onClick={startSelection} />}
          </div>
          <div className="flex items-center gap-4 text-sm text-muted-foreground">
            <span className="font-medium bg-secondary px-2.5 py-0.5 rounded-md text-secondary-foreground">{items.length} items</span>
            {album.description && <span>{album.description}</span>}
          </div>
          {selectionMode && (
            <MediaSelectionToolbar
              selectedCount={selectedMediaIds.size}
              isProcessing={removeAlbumMedia.isPending}
              onClear={clearSelection}
              onDone={cancelSelection}
              onAddToAlbum={null}
              onRemoveFromAlbum={() => setShowRemoveConfirmation(true)}
              onMoveToTrash={null}
            />
          )}
        </div>
      </div>

      {items.length === 0 ? (
        <div className="text-muted-foreground text-center py-20 bg-muted/20 rounded-2xl border-2 border-dashed border-border">
          <p>No photos in this album yet.</p>
        </div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8 gap-2">
          {items.map((item) => (
            <AlbumMediaItem
              key={item.id}
              item={item}
              isDragged={draggedId === item.id}
              selectionMode={selectionMode}
              selected={selectedMediaIds.has(item.id)}
              onDragStart={(e) => handleDragStart(e, item.id)}
              onDragOver={(e) => handleDragOver(e, item.id)}
              onDrop={handleDrop}
              onClick={() => {
                if (selectionMode) {
                  toggleSelection(item.id)
                  return
                }
                onPhotoClick(item, items)
              }}
            />
          ))}
        </div>
      )}
      {reorderError && <p role="alert" className="mt-4 text-sm text-destructive">{reorderError}</p>}
      {removeError && <p role="alert" className="mt-4 text-sm text-destructive">{removeError}</p>}
      {showRemoveConfirmation && (
        <ConfirmationDialog
          title={`Remove ${selectedMediaIds.size} selected item${selectedMediaIds.size === 1 ? '' : 's'}?`}
          description="The selected media will remain in your library and other albums."
          confirmLabel="Remove from album"
          isProcessing={removeAlbumMedia.isPending}
          destructive={false}
          onConfirm={handleRemoveSelected}
          onCancel={() => setShowRemoveConfirmation(false)}
        />
      )}
    </div>
  )
}

function moveAlbumMedia(items: Media[], draggedId: number, targetId: number): Media[] {
  const draggedIndex = items.findIndex((item) => item.id === draggedId)
  const targetIndex = items.findIndex((item) => item.id === targetId)
  if (draggedIndex === -1 || targetIndex === -1 || draggedIndex === targetIndex) return items

  const nextItems = [...items]
  const [draggedItem] = nextItems.splice(draggedIndex, 1)
  if (!draggedItem) return items
  nextItems.splice(targetIndex, 0, draggedItem)
  return nextItems
}

interface AlbumMediaItemProps {
  item: Media
  isDragged: boolean
  selectionMode: boolean
  selected: boolean
  onDragStart: (e: React.DragEvent) => void
  onDragOver: (e: React.DragEvent) => void
  onDrop: (e: React.DragEvent) => void
  onClick: () => void
}

function AlbumMediaItem({ item, isDragged, selectionMode, selected, onDragStart, onDragOver, onDrop, onClick }: AlbumMediaItemProps) {
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(() => 
    mediaApi.getCachedThumbnailUrl(item.id) || null
  )
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (thumbnailUrl || !containerRef.current) return
    let cancelled = false

    const loadThumbnail = async () => {
      try {
        const url = await batchLoader.load(item.id)
        if (!cancelled) setThumbnailUrl(url)
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
  }, [item.id, thumbnailUrl])

  return (
    <div
      ref={containerRef}
      draggable={!selectionMode}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDrop={onDrop}
      className={cn(
        'aspect-square relative cursor-pointer group overflow-hidden rounded-xl bg-muted shadow-sm transition-[box-shadow,opacity,transform] duration-200 motion-reduce:transition-none',
        isDragged && 'cursor-move opacity-25 ring-2 ring-primary',
        !isDragged && selected && 'ring-4 ring-primary ring-offset-2 ring-offset-background shadow-lg',
        !isDragged && !selected && 'opacity-100 hover:shadow-md hover:ring-2 hover:ring-primary/30 active:scale-[0.98]'
      )}
      role="button"
      tabIndex={0}
      aria-pressed={selectionMode ? selected : undefined}
      aria-label={selectionMode ? `${selected ? 'Deselect' : 'Select'} ${item.originalFilename}` : `Open ${item.originalFilename}`}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.key !== 'Enter' && event.key !== ' ') return
        event.preventDefault()
        onClick()
      }}
    >
      {thumbnailUrl ? (
        <img
          src={thumbnailUrl}
          alt={item.originalFilename}
          className="w-full h-full object-cover pointer-events-none select-none"
        />
      ) : (
        <div className="w-full h-full animate-pulse" />
      )}
      {selectionMode && (
        <div className={`absolute inset-0 transition-colors duration-150 motion-reduce:transition-none ${selected ? 'bg-primary/25' : 'bg-black/10'}`}>
          <span className={`absolute left-2 top-2 flex h-8 w-8 items-center justify-center rounded-full border-2 shadow-sm transition-colors duration-150 ${
            selected ? 'border-primary bg-primary text-primary-foreground' : 'border-white/90 bg-black/35 text-white'
          }`}>
            {selected ? <Check className="h-4 w-4" strokeWidth={3} /> : <Circle className="h-4 w-4" />}
          </span>
        </div>
      )}
    </div>
  )
}

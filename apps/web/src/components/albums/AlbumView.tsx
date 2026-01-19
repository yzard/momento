import { useState, useEffect } from 'react'
import { useAlbum, useReorderAlbum } from '../../hooks/useAlbums'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'
import { ArrowLeft, Loader2 } from 'lucide-react'
import { cn } from '../../lib/utils'

interface AlbumViewProps {
  albumId: number
  onBack: () => void
  onPhotoClick: (media: Media, allMedia: Media[]) => void
}

export default function AlbumView({ albumId, onBack, onPhotoClick }: AlbumViewProps) {
  const { data: album, isLoading, error } = useAlbum(albumId)
  const reorderAlbum = useReorderAlbum()

  const [items, setItems] = useState<Media[]>([])
  const [draggedId, setDraggedId] = useState<number | null>(null)

  useEffect(() => {
    if (album) {
      setItems(album.media)
    }
  }, [album])

  const handleDragStart = (e: React.DragEvent, id: number) => {
    setDraggedId(id)
    e.dataTransfer.effectAllowed = 'move'
  }

  const handleDragOver = (e: React.DragEvent, targetId: number) => {
    e.preventDefault()
    if (!draggedId || draggedId === targetId) return

    const draggedIndex = items.findIndex((item) => item.id === draggedId)
    const targetIndex = items.findIndex((item) => item.id === targetId)

    if (draggedIndex === -1 || targetIndex === -1) return

    const newItems = [...items]
    const [draggedItem] = newItems.splice(draggedIndex, 1)
    if (!draggedItem) {
      return
    }
    newItems.splice(targetIndex, 0, draggedItem)

    setItems(newItems)
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    if (draggedId && items.length > 0) {
      reorderAlbum.mutate({
        albumId,
        mediaIds: items.map((item) => item.id),
      })
    }
    setDraggedId(null)
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
        
        <div className="flex flex-col gap-2">
          <h2 className="text-4xl font-display font-bold text-foreground tracking-tight">{album.name}</h2>
          <div className="flex items-center gap-4 text-sm text-muted-foreground">
            <span className="font-medium bg-secondary px-2.5 py-0.5 rounded-md text-secondary-foreground">{items.length} items</span>
            {album.description && <span>{album.description}</span>}
          </div>
        </div>
      </div>

      {items.length === 0 ? (
        <div className="text-muted-foreground text-center py-20 bg-muted/20 rounded-2xl border-2 border-dashed border-border">
          <p>No photos in this album yet.</p>
        </div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8 gap-2">
          {items.map((item) => (
            <div
              key={item.id}
              draggable
              onDragStart={(e) => handleDragStart(e, item.id)}
              onDragOver={(e) => handleDragOver(e, item.id)}
              onDrop={handleDrop}
              className={cn(
                "aspect-square relative cursor-move group overflow-hidden rounded-xl bg-muted shadow-sm transition-all duration-300",
                draggedId === item.id ? 'opacity-25 ring-2 ring-primary' : 'opacity-100 hover:shadow-md hover:ring-2 hover:ring-primary/20 hover:scale-[1.02]'
              )}
              onClick={() => onPhotoClick(item, items)}
            >
              <img
                src={mediaApi.getThumbnailUrl(item.id)}
                alt={item.originalFilename}
                className="w-full h-full object-cover pointer-events-none select-none transition-transform duration-500 group-hover:scale-110"
                loading="lazy"
              />
            </div>
          ))}
        </div>
      )}
    </div>
  )
}


import { useState, useEffect } from 'react'
import { mediaApi } from '../../api/media'
import type { Album } from '../../api/types'
import { Folder, X } from 'lucide-react'

interface AlbumCardProps {
  album: Album
  onClick: () => void
  onDelete: () => void
}

export default function AlbumCard({ album, onClick, onDelete }: AlbumCardProps) {
  const [coverUrl, setCoverUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!album.coverMediaId) return
    let cancelled = false

    const loadCover = async () => {
      try {
        const url = await mediaApi.getThumbnailUrl(album.coverMediaId!)
        if (!cancelled) setCoverUrl(url)
      } catch (err) {
        console.error('Failed to load album cover:', err)
      }
    }
    loadCover()

    return () => {
      cancelled = true
    }
  }, [album.coverMediaId])

  return (
    <div
      className="relative group cursor-pointer bg-white rounded-xl border border-border overflow-hidden transition-all duration-300 hover:shadow-lg hover:border-primary/30"
      onClick={onClick}
    >
      <div className="aspect-[4/5] bg-muted relative overflow-hidden">
        {album.coverMediaId ? (
          coverUrl ? (
            <img
              src={coverUrl}
              alt={album.name}
              className="w-full h-full object-cover transition-transform duration-700 group-hover:scale-105"
            />
          ) : (
            <div className="w-full h-full animate-pulse" />
          )
        ) : (
          <div className="w-full h-full flex items-center justify-center text-muted-foreground/30 bg-muted/10">
            <Folder className="w-16 h-16 text-muted-foreground/20" strokeWidth={1} />
          </div>
        )}

        <div className="absolute inset-0 bg-black/0 group-hover:bg-black/10 transition-colors duration-300" />
      </div>
      
      <div className="p-4 bg-white border-t border-border/50">
        <h3 className="font-display font-semibold text-foreground truncate text-lg tracking-tight group-hover:text-primary transition-colors">{album.name}</h3>
        <p className="text-xs text-muted-foreground font-medium uppercase tracking-wider mt-1">{album.mediaCount} memories</p>
      </div>

      <button
        onClick={(e) => {
          e.stopPropagation()
          onDelete()
        }}
        className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 bg-white/90 backdrop-blur-sm text-destructive w-8 h-8 flex items-center justify-center rounded-full border border-destructive/20 hover:bg-destructive hover:text-white transition-all duration-200 shadow-sm z-10 hover:scale-110"
        title="Delete album"
      >
        <X className="w-4 h-4" strokeWidth={2} />
      </button>
    </div>
  )
}


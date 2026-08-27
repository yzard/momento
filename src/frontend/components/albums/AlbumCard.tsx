import type { Album } from '../../api/types'
import { Folder, X } from 'lucide-react'
import MemoryCardOverlay from '../common/MemoryCardOverlay'

interface AlbumCardProps {
  album: Album
  thumbnailUrls: Array<string | null>
  onClick: () => void
  onDelete: () => void
}

function collageGridClass(thumbnailCount: number): string {
  if (thumbnailCount === 1) return 'grid-cols-1 grid-rows-1'
  return 'grid-cols-2 grid-rows-2'
}

function collageCellClass(index: number, thumbnailCount: number): string {
  if (thumbnailCount === 2) return 'row-span-2'
  if (thumbnailCount === 3 && index === 0) return 'row-span-2'
  return ''
}

function memoryCountLabel(mediaCount: number): string {
  return `${mediaCount} ${mediaCount === 1 ? 'memory' : 'memories'}`
}

export default function AlbumCard({ album, thumbnailUrls, onClick, onDelete }: AlbumCardProps) {
  const thumbnailCount = thumbnailUrls.length

  return (
    <div className="group relative">
      <button
        type="button"
        aria-label={`${album.name}, ${memoryCountLabel(album.mediaCount)}`}
        className="relative aspect-square w-full overflow-hidden rounded-xl border border-border bg-card text-left shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
        onClick={onClick}
      >
        {thumbnailCount > 0 ? (
          <div className={`grid h-full w-full gap-px bg-black/20 ${collageGridClass(thumbnailCount)}`}>
            {thumbnailUrls.map((thumbnailUrl, index) => (
              <div
                key={album.thumbnailMediaIds[index]}
                className={`min-h-0 min-w-0 overflow-hidden bg-muted ${collageCellClass(index, thumbnailCount)}`}
              >
                {thumbnailUrl ? (
                  <img
                    src={thumbnailUrl}
                    alt=""
                    loading="lazy"
                    className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105"
                  />
                ) : null}
              </div>
            ))}
          </div>
        ) : (
          <div className="flex h-full w-full items-center justify-center bg-muted">
            <Folder className="h-12 w-12 text-muted-foreground/40" aria-hidden="true" strokeWidth={1.25} />
          </div>
        )}
        <MemoryCardOverlay
          title={album.name}
          subtitle={null}
          badge={memoryCountLabel(album.mediaCount)}
          headingLevel="h3"
        />
      </button>

      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation()
          onDelete()
        }}
        className="absolute right-2 top-2 z-10 flex h-9 w-9 items-center justify-center rounded-full border border-white/20 bg-black/55 text-white shadow-sm backdrop-blur-sm transition-all hover:bg-destructive focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white sm:opacity-0 sm:group-hover:opacity-100 sm:focus-visible:opacity-100"
        aria-label={`Delete ${album.name}`}
        title="Delete album"
      >
        <X className="h-4 w-4" aria-hidden="true" strokeWidth={2} />
      </button>
    </div>
  )
}

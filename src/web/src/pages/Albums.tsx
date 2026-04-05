import { useState } from 'react'
import AlbumList from '../components/albums/AlbumList'
import AlbumView from '../components/albums/AlbumView'
import Lightbox from '../components/viewer/Lightbox'
import type { Album, Media } from '../api/types'

export default function Albums() {
  const [selectedAlbumId, setSelectedAlbumId] = useState<number | null>(null)
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const [initialIndex, setInitialIndex] = useState(0)
  const [mediaIds, setMediaIds] = useState<number[]>([])

  const handleAlbumClick = (album: Album) => {
    setSelectedAlbumId(album.id)
  }

  const handlePhotoClick = (media: Media, allMedia: Media[]) => {
    const index = allMedia.findIndex((m) => m.id === media.id)
    setMediaIds(allMedia.map((item) => item.id))
    setInitialIndex(index >= 0 ? index : 0)
    setLightboxOpen(true)
  }

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="container max-w-[1800px] mx-auto p-6 md:p-10 animate-fade-in pb-20">
        {selectedAlbumId ? (
          <AlbumView
            albumId={selectedAlbumId}
            onBack={() => setSelectedAlbumId(null)}
            onPhotoClick={handlePhotoClick}
          />
        ) : (
          <AlbumList onAlbumClick={handleAlbumClick} />
        )}
      </div>
      {lightboxOpen && (
        <Lightbox
          mediaIds={mediaIds}
          currentIndex={initialIndex}
          onClose={() => setLightboxOpen(false)}
          onIndexChange={setInitialIndex}
        />
      )}
    </div>
  )
}

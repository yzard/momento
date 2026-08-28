import { useState } from 'react'
import AlbumList from '../components/albums/AlbumList'
import AlbumView from '../components/albums/AlbumView'
import ManagedLightbox from '../components/viewer/ManagedLightbox'
import type { Album, Media } from '../api/types'
import { useLightbox } from '../hooks/useLightbox'

export default function Albums() {
  const [selectedAlbumId, setSelectedAlbumId] = useState<number | null>(null)
  const lightbox = useLightbox()

  const handleAlbumClick = (album: Album) => {
    setSelectedAlbumId(album.id)
  }

  const handlePhotoClick = (media: Media, allMedia: Media[]) => {
    lightbox.open(
      media.id,
      allMedia.map((item) => item.id)
    )
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
      <ManagedLightbox controller={lightbox} />
    </div>
  )
}

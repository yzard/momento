import { useCallback, useState } from 'react'
import MapView from '../components/map/MapView'
import Lightbox from '../components/viewer/Lightbox'
import type { Media } from '../api/types'

export default function Map() {
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const [mediaList, setMediaList] = useState<Media[]>([])
  const [currentIndex, setCurrentIndex] = useState(0)
  const [geoMediaIds, setGeoMediaIds] = useState<number[]>([])

  const handleMediaChange = useCallback((items: Media[]) => {
    setMediaList(items)
    setGeoMediaIds(items.filter((m) => m.gpsLatitude !== null && m.gpsLongitude !== null).map((m) => m.id))
  }, [])

  const handlePhotoClick = (mediaId: number) => {
    const index = geoMediaIds.findIndex((id) => id === mediaId)
    if (index === -1) return

    setCurrentIndex(index)
    setLightboxOpen(true)
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <MapView onPhotoClick={handlePhotoClick} onMediaChange={handleMediaChange} />

      {lightboxOpen && (
        <Lightbox
          media={mediaList.filter((m) => m.gpsLatitude !== null && m.gpsLongitude !== null)}
          currentIndex={currentIndex}
          onClose={() => setLightboxOpen(false)}
          onIndexChange={setCurrentIndex}
        />
      )}
    </div>
  )
}

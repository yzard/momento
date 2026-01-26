import { useCallback, useState } from 'react'
import MapView from '../components/map/MapView'
import Lightbox from '../components/viewer/Lightbox'
import { mapApi, type MapMediaRequest } from '../api/map'

export default function MapPage() {
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const [mediaIds, setMediaIds] = useState<number[]>([])
  const [currentIndex, setCurrentIndex] = useState(0)

  const handlePhotoClick = useCallback((mediaId: number) => {
    setMediaIds([mediaId])
    setCurrentIndex(0)
    setLightboxOpen(true)
  }, [])

  const handleClusterClick = useCallback(async (payload: MapMediaRequest & { representativeId?: number | null }) => {
    try {
      const { representativeId, ...request } = payload
      const response = await mapApi.getMedia(request)
      if (response.items.length === 0) return
      const ids = response.items.map((item) => item.id)
      const targetIndex = representativeId
        ? ids.findIndex((id) => id === representativeId)
        : -1
      setMediaIds(ids)
      setCurrentIndex(targetIndex >= 0 ? targetIndex : 0)
      setLightboxOpen(true)
    } catch {
      console.error('Failed to load cluster media')
    }
  }, [])

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <MapView onPhotoClick={handlePhotoClick} onClusterClick={handleClusterClick} />

      {lightboxOpen && (
        <Lightbox
          mediaIds={mediaIds}
          currentIndex={currentIndex}
          onClose={() => setLightboxOpen(false)}
          onIndexChange={setCurrentIndex}
        />
      )}
    </div>
  )
}

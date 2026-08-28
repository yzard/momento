import { useCallback } from 'react'
import MapView from '../components/map/MapView'
import ManagedLightbox from '../components/viewer/ManagedLightbox'
import { mapApi, type MapMediaRequest } from '../api/map'
import { useLightbox } from '../hooks/useLightbox'

export default function MapPage() {
  const lightbox = useLightbox()

  const handlePhotoClick = useCallback(
    (mediaId: number) => {
      lightbox.openAtIndex([mediaId], 0)
    },
    [lightbox]
  )

  const handleClusterClick = useCallback(
    async (payload: MapMediaRequest & { representativeId?: number | null }) => {
      try {
        const { representativeId, ...request } = payload
        const response = await mapApi.getMedia(request)
        if (response.items.length === 0) return
        const ids = response.items.map((item) => item.id)
        const targetIndex = representativeId ? ids.findIndex((id) => id === representativeId) : -1
        lightbox.openAtIndex(ids, targetIndex >= 0 ? targetIndex : 0)
      } catch {
        console.error('Failed to load cluster media')
      }
    },
    [lightbox]
  )

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <MapView onPhotoClick={handlePhotoClick} onClusterClick={handleClusterClick} />

      <ManagedLightbox controller={lightbox} />
    </div>
  )
}

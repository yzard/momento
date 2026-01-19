import { useCallback, useState } from 'react'
import MapView from '../components/map/MapView'
import Lightbox from '../components/viewer/Lightbox'
import type { Media } from '../api/types'
import type { GeoMedia } from '../components/map/PhotoMarker'

export default function Map() {
  const [lightboxOpen, setLightboxOpen] = useState(false)
  const [mediaList, setMediaList] = useState<Media[]>([])
  const [currentIndex, setCurrentIndex] = useState(0)
  const [geoMedia, setGeoMedia] = useState<GeoMedia[]>([])

  const handleGeoMediaChange = useCallback((items: GeoMedia[]) => {
    setGeoMedia(items)
  }, [])

  const handlePhotoClick = (mediaId: number) => {
    const index = geoMedia.findIndex((m) => m.id === mediaId)
    if (index === -1) return

    const fullList = geoMedia.map((m) => ({
      id: m.id,
      mediaType: m.mediaType,
      mimeType: m.mimeType || '',
      originalFilename: m.originalFilename || 'Map Photo',
      filename: m.originalFilename || '',
      width: null,
      height: null,
      fileSize: 0,
      durationSeconds: null,
      dateTaken: m.dateTaken,
      gpsLatitude: m.latitude,
      gpsLongitude: m.longitude,
      cameraMake: null,
      cameraModel: null,
      iso: null,
      exposureTime: null,
      fNumber: null,
      focalLength: null,
      gpsAltitude: null,
      locationState: null,
      locationCountry: null,
      keywords: null,
      createdAt: '',
    })) satisfies Media[]

    setMediaList(fullList)
    setCurrentIndex(index)
    setLightboxOpen(true)
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <MapView onPhotoClick={handlePhotoClick} onMediaChange={handleGeoMediaChange} />

      {lightboxOpen && (
        <Lightbox
          media={mediaList}
          currentIndex={currentIndex}
          onClose={() => setLightboxOpen(false)}
          onIndexChange={setCurrentIndex}
        />
      )}
    </div>
  )
}

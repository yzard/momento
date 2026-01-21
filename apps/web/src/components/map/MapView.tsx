import { useEffect, useRef, useState } from 'react'
import { MapContainer, TileLayer, useMap, useMapEvents } from 'react-leaflet'
import { LatLngBounds, type LatLngTuple } from 'leaflet'
import 'leaflet/dist/leaflet.css'
import { mediaApi } from '../../api/media'
import PhotoMarker, { type GeoMedia } from './PhotoMarker'
import type { Media } from '../../api/types'
import { Loader2, Map as MapIcon } from 'lucide-react'

const VIEWPORT_STORAGE_KEY = 'map_viewport'

interface SavedViewport {
  center: LatLngTuple
  zoom: number
}

function getSavedViewport(): SavedViewport | null {
  const saved = sessionStorage.getItem(VIEWPORT_STORAGE_KEY)
  if (!saved) return null
  try {
    return JSON.parse(saved) as SavedViewport
  } catch {
    return null
  }
}

function MapViewportPersistence() {
  const map = useMapEvents({
    moveend: () => {
      const center = map.getCenter()
      const zoom = map.getZoom()
      const viewport: SavedViewport = {
        center: [center.lat, center.lng],
        zoom,
      }
      sessionStorage.setItem(VIEWPORT_STORAGE_KEY, JSON.stringify(viewport))
    },
  })
  return null
}

function FitBoundsToMarkers({ geoMedia, skipIfSavedViewport }: { geoMedia: GeoMedia[]; skipIfSavedViewport: boolean }) {
  const map = useMap()
  const hasFittedRef = useRef(false)

  useEffect(() => {
    if (geoMedia.length === 0) return
    if (hasFittedRef.current) return
    if (skipIfSavedViewport) {
      hasFittedRef.current = true
      return
    }

    const bounds = new LatLngBounds(
      geoMedia.map((m) => [m.latitude, m.longitude] as [number, number])
    )
    map.fitBounds(bounds, { padding: [50, 50] })
    hasFittedRef.current = true
  }, [map, geoMedia, skipIfSavedViewport])

  return null
}

interface MapViewProps {
  onPhotoClick?: (mediaId: number) => void
  onMediaChange?: (items: Media[]) => void
}

export default function MapView({ onPhotoClick, onMediaChange }: MapViewProps) {
  const [geoMedia, setGeoMedia] = useState<GeoMedia[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const loadMedia = async () => {
      try {
        const data = await mediaApi.listMapMedia()
        const geotagged = data.filter((m) => m.gpsLatitude !== null && m.gpsLongitude !== null)
        const geoItems: GeoMedia[] = geotagged.map((m) => ({
          id: m.id,
          thumbnailPath: null,
          thumbnailData: null,
          latitude: m.gpsLatitude as number,
          longitude: m.gpsLongitude as number,
          dateTaken: m.dateTaken,
          mediaType: m.mediaType,
          mimeType: m.mimeType,
          originalFilename: m.originalFilename,
        }))
        setGeoMedia(geoItems)
        onMediaChange?.(data)
      } catch {
        setError('Failed to load map data')
      } finally {
        setIsLoading(false)
      }
    }
    loadMedia()
  }, [onMediaChange])

  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
        <p className="text-sm font-medium">Loading map data...</p>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-destructive gap-3">
        <p className="font-semibold">{error}</p>
        <button onClick={() => window.location.reload()} className="text-sm underline decoration-destructive/50 underline-offset-4 hover:decoration-destructive">
          Retry
        </button>
      </div>
    )
  }

  if (geoMedia.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-6 bg-muted/5 rounded-xl border border-border/50">
        <div className="p-6 bg-background rounded-full border border-border/50 shadow-lg">
          <MapIcon className="w-10 h-10 opacity-60 text-primary" strokeWidth={1.5} />
        </div>
        <div className="text-center">
          <h3 className="text-xl font-medium text-foreground font-display tracking-tight">No geotagged photos</h3>
          <p className="text-sm mt-2 max-w-xs mx-auto font-medium">
            Photos with GPS location data will appear on this map.
          </p>
        </div>
      </div>
    )
  }

  const savedViewport = getSavedViewport()
  const initialCenter: LatLngTuple = savedViewport?.center ?? [0, 0]
  const initialZoom = savedViewport?.zoom ?? 2

  return (
    <div className="flex-1 w-full overflow-hidden rounded-2xl border border-border/60 shadow-sm bg-card m-6">
      <MapContainer center={initialCenter} zoom={initialZoom} style={{ height: '100%', width: '100%' }}>
        <MapViewportPersistence />
        <FitBoundsToMarkers geoMedia={geoMedia} skipIfSavedViewport={!!savedViewport} />
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> &copy; <a href="https://carto.com/attributions">CARTO</a>'
          url="https://{s}.basemaps.cartocdn.com/light_all/{z}/{x}/{y}{r}.png"
        />
        {geoMedia.map((media) => (
          <PhotoMarker key={media.id} media={media} onClick={onPhotoClick} />
        ))}
      </MapContainer>
    </div>
  )
}


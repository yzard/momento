import { useCallback, useEffect, useRef, useState } from 'react'
import { MapContainer, TileLayer, useMap, useMapEvents } from 'react-leaflet'
import type { LatLngTuple } from 'leaflet'
import 'leaflet/dist/leaflet.css'
import ClusterMarker from './ClusterMarker'
import { useMapClusters, type MapCluster } from '../../hooks/useMapClusters'
import { normalizeMapBounds, type BoundingBox } from '../../api/map'
import { Loader2 } from 'lucide-react'

const VIEWPORT_STORAGE_KEY = 'map_viewport'
const OPENSTREETMAP_TILE_URL = 'https://tile.openstreetmap.org/{z}/{x}/{y}.png'
const OPENSTREETMAP_ATTRIBUTION =
  '&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> contributors'

interface SavedViewport {
  center: LatLngTuple
  zoom: number
}

function getSavedViewport(): SavedViewport | null {
  const storedViewport = sessionStorage.getItem(VIEWPORT_STORAGE_KEY)
  if (!storedViewport) return null
  try {
    return JSON.parse(storedViewport) as SavedViewport
  } catch (error) {
    if (error instanceof SyntaxError) return null
    throw error
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

interface MapViewProps {
  onPhotoClick?: (mediaId: number) => void
  onClusterClick?: (payload: {
    bounds: BoundingBox
    geohashPrefixes: string[]
    representativeId?: number | null
  }) => void
}

interface MapViewportUpdate {
  bounds: BoundingBox
  zoom: number
}

function MapViewportTracker({
  onViewportChange,
}: {
  onViewportChange: (update: MapViewportUpdate) => void
}) {
  const viewportUpdateTimeoutReference = useRef<number | null>(null)
  const map = useMap()
  const scheduleViewportUpdate = useCallback(() => {
    if (viewportUpdateTimeoutReference.current) {
      window.clearTimeout(viewportUpdateTimeoutReference.current)
    }
    viewportUpdateTimeoutReference.current = window.setTimeout(() => {
      const mapBounds = map.getBounds()
      onViewportChange({
        bounds: normalizeMapBounds({
          north: mapBounds.getNorth(),
          south: mapBounds.getSouth(),
          east: mapBounds.getEast(),
          west: mapBounds.getWest(),
        }),
        zoom: map.getZoom(),
      })
    }, 200)
  }, [map, onViewportChange])

  useMapEvents({ moveend: scheduleViewportUpdate, zoomend: scheduleViewportUpdate })

  useEffect(() => {
    scheduleViewportUpdate()

    return () => {
      if (viewportUpdateTimeoutReference.current) {
        window.clearTimeout(viewportUpdateTimeoutReference.current)
      }
    }
  }, [scheduleViewportUpdate])

  return null
}

interface MapClusterMarkersProps {
  clusters: MapCluster[]
  onClusterClick: (cluster: MapCluster) => void
}

function MapClusterMarkers({ clusters, onClusterClick }: MapClusterMarkersProps) {
  return (
    <>
      {clusters.map((cluster) => {
        const [longitude, latitude] = cluster.geometry.coordinates as [number, number]
        const { count, representativeId } = cluster.properties
        const clusterKey = cluster.properties.cellId

        return (
          <ClusterMarker
            key={clusterKey}
            latitude={latitude}
            longitude={longitude}
            count={count}
            representativeId={representativeId}
            onClick={() => onClusterClick(cluster)}
          />
        )
      })}
    </>
  )
}

export default function MapView({ onPhotoClick, onClusterClick }: MapViewProps) {
  const savedViewport = getSavedViewport()
  const initialCenter: LatLngTuple = savedViewport?.center ?? [0, 0]
  const initialZoom = savedViewport?.zoom ?? 2
  const [bounds, setBounds] = useState<BoundingBox | null>(null)
  const [clusterDataZoom, setClusterDataZoom] = useState(initialZoom)
  const { clusters, isLoading, error } = useMapClusters({
    bounds,
    zoom: clusterDataZoom,
  })

  const handleViewportChange = ({ bounds: nextBounds, zoom: nextZoom }: MapViewportUpdate) => {
    setBounds(nextBounds)
    setClusterDataZoom(nextZoom)
  }

  const handleClusterClick = (cluster: MapCluster) => {
    const { count, representativeId, cellId } = cluster.properties

    if (count > 1 && cellId && bounds) {
      onClusterClick?.({ bounds, geohashPrefixes: [cellId], representativeId })
      return
    }

    if (!representativeId) return
    onPhotoClick?.(representativeId)
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-destructive gap-3">
        <p className="font-semibold">Failed to load map data</p>
        <button
          type="button"
          onClick={() => window.location.reload()}
          className="text-sm underline decoration-destructive/50 underline-offset-4 hover:decoration-destructive"
        >
          Retry
        </button>
      </div>
    )
  }

  return (
    <div className="relative flex-1 w-full overflow-hidden rounded-2xl border border-border/60 shadow-sm bg-card m-6">
      <MapContainer
        center={initialCenter}
        zoom={initialZoom}
        zoomAnimation
        markerZoomAnimation
        fadeAnimation
        style={{ height: '100%', width: '100%' }}
      >
        <MapViewportPersistence />
        <MapViewportTracker onViewportChange={handleViewportChange} />
        <TileLayer
          attribution={OPENSTREETMAP_ATTRIBUTION}
          detectRetina
          maxZoom={19}
          referrerPolicy="origin"
          url={OPENSTREETMAP_TILE_URL}
        />
        <MapClusterMarkers clusters={clusters} onClusterClick={handleClusterClick} />
      </MapContainer>
      {isLoading && (
        <div className="absolute inset-0 flex items-center justify-center bg-background/60 backdrop-blur-sm">
          <div className="flex items-center gap-3 text-muted-foreground">
            <Loader2 className="w-5 h-5 animate-spin text-primary" />
            <p className="text-sm font-medium">Loading map data...</p>
          </div>
        </div>
      )}
    </div>
  )
}

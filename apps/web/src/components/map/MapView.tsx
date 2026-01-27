import { useEffect, useRef, useState } from 'react'
import { MapContainer, TileLayer, useMap, useMapEvents } from 'react-leaflet'
import type { LatLngTuple, Map as LeafletMap } from 'leaflet'
import 'leaflet/dist/leaflet.css'
import ClusterMarker from './ClusterMarker'
import { useMapClusters, type MapCluster } from '../../hooks/useMapClusters'
import type { BoundingBox } from '../../api/map'
import { Loader2 } from 'lucide-react'

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
  onClusterClick?: (payload: { bounds: BoundingBox; geohashPrefixes: string[]; representativeId?: number | null }) => void
}

interface MapViewportUpdate {
  bounds: BoundingBox
  zoom: number
}

function MapViewportTracker({ onViewportChange }: { onViewportChange: (update: MapViewportUpdate) => void }) {
  const timeoutRef = useRef<number | null>(null)
  const map = useMapEvents({
    moveend: () => {
      if (timeoutRef.current) window.clearTimeout(timeoutRef.current)
      timeoutRef.current = window.setTimeout(() => {
        const bounds = map.getBounds()
        onViewportChange({
          bounds: {
            north: bounds.getNorth(),
            south: bounds.getSouth(),
            east: bounds.getEast(),
            west: bounds.getWest(),
          },
          zoom: map.getZoom(),
        })
      }, 200)
    },
    zoomend: () => {
      if (timeoutRef.current) window.clearTimeout(timeoutRef.current)
      timeoutRef.current = window.setTimeout(() => {
        const bounds = map.getBounds()
        onViewportChange({
          bounds: {
            north: bounds.getNorth(),
            south: bounds.getSouth(),
            east: bounds.getEast(),
            west: bounds.getWest(),
          },
          zoom: map.getZoom(),
        })
      }, 200)
    },
  })

  useEffect(() => {
    if (timeoutRef.current) window.clearTimeout(timeoutRef.current)
    timeoutRef.current = window.setTimeout(() => {
      const bounds = map.getBounds()
      onViewportChange({
        bounds: {
          north: bounds.getNorth(),
          south: bounds.getSouth(),
          east: bounds.getEast(),
          west: bounds.getWest(),
        },
        zoom: map.getZoom(),
      })
    }, 200)

    return () => {
      if (timeoutRef.current) window.clearTimeout(timeoutRef.current)
    }
  }, [map, onViewportChange])

  return null
}

function MapZoomTracker({ onZoomChange }: { onZoomChange: (zoom: number) => void }) {
  const rafRef = useRef<number | null>(null)
  const map = useMapEvents({
    zoom: () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current)
      rafRef.current = requestAnimationFrame(() => {
        onZoomChange(map.getZoom())
      })
    },
  })

  useEffect(() => () => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current)
  }, [])

  return null
}

function MapRefSetter({ onReady }: { onReady: (map: LeafletMap) => void }) {
  const map = useMap()

  useEffect(() => {
    onReady(map)
  }, [map, onReady])

  return null
}

export default function MapView({ onPhotoClick, onClusterClick }: MapViewProps) {
  const savedViewport = getSavedViewport()
  const initialCenter: LatLngTuple = savedViewport?.center ?? [0, 0]
  const initialZoom = savedViewport?.zoom ?? 2
  const mapRef = useRef<LeafletMap | null>(null)
  const [bounds, setBounds] = useState<BoundingBox | null>(null)
  const [dataZoom, setDataZoom] = useState(initialZoom)
  const [renderZoom, setRenderZoom] = useState(initialZoom)
  const { clusters, isLoading, supercluster, error } = useMapClusters({ bounds, zoom: renderZoom, dataZoom })

  const handleViewportChange = ({ bounds: nextBounds, zoom: nextZoom }: MapViewportUpdate) => {
    setBounds(nextBounds)
    setDataZoom(nextZoom)
    setRenderZoom(nextZoom)
  }

  const handleClusterClick = (cluster: MapCluster, latitude: number, longitude: number) => {
    const { count, representativeId, cluster_id: clusterId, cellId } = cluster.properties

    if (count > 1 && renderZoom < 16) {
      const targetZoom = clusterId ? supercluster.getClusterExpansionZoom(clusterId) : Math.min(16, renderZoom + 2)
      mapRef.current?.setView([latitude, longitude], targetZoom, { animate: true })
      return
    }

    if (count > 1 && clusterId !== undefined && bounds) {
      const leafLimit = Math.min(count, 500)
      const leaves = supercluster.getLeaves(clusterId, leafLimit)
      const geohashPrefixes = Array.from(
        new Set(
          leaves
            .map((leaf) => leaf.properties.cellId)
            .filter((cellId): cellId is string => typeof cellId === 'string' && cellId.length > 0)
        )
      )

      if (geohashPrefixes.length > 0) {
        onClusterClick?.({ bounds, geohashPrefixes, representativeId })
        return
      }
    }

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
        <MapRefSetter onReady={(map) => {
          mapRef.current = map
        }} />
        <MapViewportPersistence />
        <MapViewportTracker onViewportChange={handleViewportChange} />
        <MapZoomTracker onZoomChange={setRenderZoom} />
        <TileLayer
          attribution='&copy; <a href="https://www.openstreetmap.org/copyright">OpenStreetMap</a> &copy; <a href="https://carto.com/attributions">CARTO</a>'
          url="https://{s}.basemaps.cartocdn.com/light_all/{z}/{x}/{y}{r}.png"
        />
        {clusters.map((cluster) => {
          const [lng, lat] = cluster.geometry.coordinates as [number, number]
          const { count, representativeId } = cluster.properties
          const fallbackKey = `${lat}-${lng}`
          const clusterKey = cluster.properties.cluster
            ? `cluster-${cluster.properties.cluster_id ?? fallbackKey}`
            : `cell-${cluster.properties.cellId ?? representativeId ?? fallbackKey}`

          return (
            <ClusterMarker
              key={clusterKey}
              latitude={lat}
              longitude={lng}
              count={count}
              representativeId={representativeId}
              onClick={() => handleClusterClick(cluster, lat, lng)}
            />
          )
        })}
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

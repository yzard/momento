import { useEffect, useMemo, useState } from 'react'
import { Marker } from 'react-leaflet'
import { DivIcon } from 'leaflet'
import { tinyBatchLoader } from '../../utils/batcher'
import { clusterIconSize, createClusterIconElement } from './clusterIcon'

interface ClusterMarkerProps {
  latitude: number
  longitude: number
  count: number
  representativeId: number | null
  onClick?: () => void
}

export default function ClusterMarker({
  latitude,
  longitude,
  count,
  representativeId,
  onClick,
}: ClusterMarkerProps) {
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null)

  useEffect(() => {
    if (!representativeId) {
      setThumbnailUrl(null)
      return
    }

    let cancelled = false
    const loadThumbnail = async () => {
      try {
        const url = await tinyBatchLoader.load(representativeId)
        if (!cancelled && url) setThumbnailUrl(url)
      } catch (error) {
        console.error('Failed to load cluster thumbnail:', error)
      }
    }
    loadThumbnail()

    return () => {
      cancelled = true
    }
  }, [representativeId])

  const icon = useMemo(
    () =>
      new DivIcon({
        className: '',
        iconSize: [clusterIconSize, clusterIconSize],
        iconAnchor: [clusterIconSize / 2, clusterIconSize / 2],
        popupAnchor: [0, -clusterIconSize / 2],
        html: createClusterIconElement(thumbnailUrl, count),
      }),
    [count, thumbnailUrl]
  )

  return (
    <Marker
      position={[latitude, longitude]}
      icon={icon}
      eventHandlers={{
        click: () => onClick?.(),
      }}
    />
  )
}

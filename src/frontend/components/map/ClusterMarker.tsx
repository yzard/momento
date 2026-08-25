import { useEffect, useMemo, useState } from 'react'
import { Marker } from 'react-leaflet'
import { DivIcon } from 'leaflet'
import { tinyBatchLoader } from '../../utils/batcher'

interface ClusterMarkerProps {
  latitude: number
  longitude: number
  count: number
  representativeId: number | null
  onClick?: () => void
}

const THUMB_SIZE = 52

export default function ClusterMarker({ latitude, longitude, count, representativeId, onClick }: ClusterMarkerProps) {
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

  const badgeText = `${count}`
  const showBadge = count > 1

  const icon = useMemo(() => new DivIcon({
    className: '',
    iconSize: [THUMB_SIZE, THUMB_SIZE],
    iconAnchor: [THUMB_SIZE / 2, THUMB_SIZE / 2],
    popupAnchor: [0, -THUMB_SIZE / 2],
    html: `<div class="map-marker" style="position:relative;">
      <div class="map-marker__bubble" style="width:${THUMB_SIZE}px;height:${THUMB_SIZE}px;">
        ${thumbnailUrl ? `<img src="${thumbnailUrl}" class="map-marker__image" />` : '<div class="map-marker__placeholder"></div>'}
      </div>
      ${showBadge ? `<span class="map-marker__badge">${badgeText}</span>` : ''}
    </div>`,
  }), [badgeText, showBadge, thumbnailUrl])

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

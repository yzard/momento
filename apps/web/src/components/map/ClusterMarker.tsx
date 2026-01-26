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
      <div class="map-marker__bubble" style="position:relative;width:${THUMB_SIZE}px;height:${THUMB_SIZE}px;border-radius:10px;overflow:hidden;border:2px solid #ffffff;box-shadow:0 6px 16px rgba(15, 23, 42, 0.2);background:#f1f5f9;display:flex;align-items:center;justify-content:center;animation:mapMarkerPop 180ms ease-out;transform-origin:center;">
        ${thumbnailUrl ? `<img src="${thumbnailUrl}" style="width:100%;height:100%;object-fit:cover;" />` : '<div style="width:60%;height:60%;border-radius:6px;background:#e2e8f0;"></div>'}
      </div>
      ${showBadge ? `<span style="position:absolute;top:-10px;right:-10px;min-width:26px;height:26px;padding:0 6px;border-radius:999px;background:#111827;color:#ffffff;font-size:12px;font-weight:600;display:flex;align-items:center;justify-content:center;border:2px solid #ffffff;">${badgeText}</span>` : ''}
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

import { useState, useEffect, useMemo } from 'react'
import { Marker } from 'react-leaflet'
import { DivIcon } from 'leaflet'
import { tinyBatchLoader } from '../../utils/batcher'

export interface GeoMedia {
  id: number
  latitude: number
  longitude: number
  dateTaken: string | null
  mediaType: 'image' | 'video'
  mimeType: string | null
  originalFilename: string | null
}

interface PhotoMarkerProps {
  media: GeoMedia
  onClick?: (mediaId: number) => void
}

const THUMB_SIZE = 48

export default function PhotoMarker({ media, onClick }: PhotoMarkerProps) {
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    const loadThumbnail = async () => {
      try {
        const url = await tinyBatchLoader.load(media.id)
        if (!cancelled && url) setThumbnailUrl(url)
      } catch (error) {
        console.error('Failed to load marker thumbnail:', error)
      }
    }
    loadThumbnail()

    return () => {
      cancelled = true
    }
  }, [media.id])

  const icon = useMemo(() => new DivIcon({
    className: '',
    iconSize: [THUMB_SIZE, THUMB_SIZE],
    iconAnchor: [THUMB_SIZE / 2, THUMB_SIZE / 2],
    popupAnchor: [0, -THUMB_SIZE / 2],
    html: `<div style="width:${THUMB_SIZE}px;height:${THUMB_SIZE}px;border-radius:6px;overflow:hidden;border:2px solid white;box-shadow:0 2px 8px rgba(0,0,0,0.3);background:#f0f0f0;display:flex;align-items:center;justify-content:center;">
      ${thumbnailUrl ? `<img src="${thumbnailUrl}" style="width:100%;height:100%;object-fit:cover;" />` : '<div style="width:60%;height:60%;border-radius:4px;background:#e5e7eb;"></div>'}
    </div>`,
  }), [thumbnailUrl])

  return (
    <Marker
      position={[media.latitude, media.longitude]}
      icon={icon}
      eventHandlers={{
        click: () => onClick?.(media.id),
      }}
    />
  )
}

import { useState, useEffect, useMemo } from 'react'
import { Marker } from 'react-leaflet'
import { DivIcon } from 'leaflet'
import { mediaApi } from '../../api/media'

export interface GeoMedia {
  id: number
  thumbnailPath: string | null
  thumbnailData: string | null
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
  // Use thumbnailData (base64) if available, otherwise load via API
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(media.thumbnailData)

  useEffect(() => {
    // If we already have base64 data, use it directly
    if (media.thumbnailData) {
      setThumbnailUrl(media.thumbnailData)
      return
    }

    // Otherwise, load via API with Authorization header
    let cancelled = false
    const loadThumbnail = async () => {
      try {
        const url = await mediaApi.getThumbnailUrl(media.id)
        if (!cancelled) setThumbnailUrl(url)
      } catch (err) {
        console.error('Failed to load marker thumbnail:', err)
      }
    }
    loadThumbnail()

    return () => {
      cancelled = true
    }
  }, [media.id, media.thumbnailData])

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

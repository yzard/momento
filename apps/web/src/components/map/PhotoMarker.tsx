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
  const thumbnailUrl = media.thumbnailData || (media.thumbnailPath ? mediaApi.getThumbnailUrl(media.id) : null)

  const icon = new DivIcon({
    className: '',
    iconSize: [THUMB_SIZE, THUMB_SIZE],
    iconAnchor: [THUMB_SIZE / 2, THUMB_SIZE / 2],
    popupAnchor: [0, -THUMB_SIZE / 2],
    html: `<div style="width:${THUMB_SIZE}px;height:${THUMB_SIZE}px;border-radius:6px;overflow:hidden;border:2px solid white;box-shadow:0 2px 8px rgba(0,0,0,0.3);background:#f0f0f0;display:flex;align-items:center;justify-content:center;">
      ${thumbnailUrl ? `<img src=\"${thumbnailUrl}\" style=\"width:100%;height:100%;object-fit:cover;\" loading=\"lazy\" />` : '<div style=\"width:60%;height:60%;border-radius:4px;background:#e5e7eb;\"></div>'}
    </div>`,
  })

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

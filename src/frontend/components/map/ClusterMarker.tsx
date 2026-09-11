import { useLayoutEffect, useMemo } from 'react'
import { Marker } from 'react-leaflet'
import { DivIcon } from 'leaflet'
import { mediaApi } from '../../api/media'
import { clusterIconSize, createClusterIconElement, updateClusterIconCount } from './clusterIcon'

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
  const thumbnailUrl = representativeId ? mediaApi.getThumbnailURL(representativeId, 'tiny') : null
  const element = useMemo(() => createClusterIconElement(thumbnailUrl, 1), [thumbnailUrl])
  const icon = useMemo(
    () =>
      new DivIcon({
        className: '',
        iconSize: [clusterIconSize, clusterIconSize],
        iconAnchor: [clusterIconSize / 2, clusterIconSize / 2],
        popupAnchor: [0, -clusterIconSize / 2],
        html: element,
      }),
    [element]
  )
  useLayoutEffect(() => updateClusterIconCount(element, count), [element, count])

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

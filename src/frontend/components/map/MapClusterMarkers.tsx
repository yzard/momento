import ClusterMarker from './ClusterMarker'
import type { MapCluster } from '../../hooks/useMapClusters'

interface MapClusterMarkersProps {
  clusters: MapCluster[]
  onClusterClick: (cluster: MapCluster) => void
}

export default function MapClusterMarkers({ clusters, onClusterClick }: MapClusterMarkersProps) {
  return (
    <>
      {clusters.map((cluster) => {
        const [longitude, latitude] = cluster.geometry.coordinates as [number, number]
        const { count, representativeId } = cluster.properties
        const clusterKey = representativeId ?? cluster.properties.cellId

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

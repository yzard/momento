import { useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import { mapApi, type BoundingBox } from '../api/map'
import { queryKeys } from '../lib/queryKeys'

interface UseMapClustersProps {
  bounds: BoundingBox | null
  zoom: number
}

export interface MapCluster {
  geometry: { coordinates: [number, number] }
  properties: {
    count: number
    representativeId: number
    cellId: string
  }
}

export function useMapClusters({ bounds, zoom }: UseMapClustersProps) {
  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.mapClusters.viewport(bounds, zoom),
    queryFn: () => {
      if (!bounds) return { clusters: [], totalCount: 0 }
      return mapApi.getClusters(bounds, zoom)
    },
    enabled: !!bounds,
    staleTime: 5000,
  })
  // Server centers are based on the user's entire library. Never recluster a viewport subset.
  const clusters = useMemo<MapCluster[]>(
    () =>
      data?.clusters.map((cluster) => ({
        geometry: { coordinates: [cluster.lng, cluster.lat] },
        properties: {
          count: cluster.count,
          representativeId: cluster.representativeId,
          cellId: cluster.id,
        },
      })) ?? [],
    [data]
  )
  return { clusters, isLoading, totalCount: data?.totalCount ?? 0, error }
}

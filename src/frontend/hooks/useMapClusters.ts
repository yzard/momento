import { useEffect, useState, useRef } from 'react'
import { useQuery } from '@tanstack/react-query'
import Supercluster from 'supercluster'
import { mapApi, type BoundingBox } from '../api/map'
import { queryKeys } from '../lib/queryKeys'

interface UseMapClustersProps {
  bounds: BoundingBox | null
  zoom: number
  dataZoom: number
}

export type ClusterProperties = {
  count: number
  representativeId: number | null
  cellId?: string | null
  cluster?: boolean
  cluster_id?: number
  point_count?: number
  point_count_abbreviated?: number
}

export type MapCluster =
  Supercluster.ClusterFeature<ClusterProperties> | Supercluster.PointFeature<ClusterProperties>

export function useMapClusters({ bounds, zoom, dataZoom }: UseMapClustersProps) {
  const [clusters, setClusters] = useState<MapCluster[]>([])
  const superclusterRef = useRef<Supercluster<ClusterProperties, ClusterProperties>>(
    new Supercluster({
      radius: 32,
      maxZoom: 20,
      minPoints: 2,
      map: (props) => ({
        count: props.count,
        representativeId: props.representativeId,
        cellId: props.cellId ?? null,
      }),
      reduce: (accumulated, props) => {
        accumulated.count += props.count
        if (!accumulated.representativeId && props.representativeId) {
          accumulated.representativeId = props.representativeId
        }
      },
    })
  )

  // Fetch cluster data from backend
  const {
    data: backendData,
    isLoading,
    error,
  } = useQuery({
    queryKey: queryKeys.mapClusters.viewport(bounds, dataZoom),
    queryFn: () => {
      if (!bounds) return { clusters: [], totalCount: 0 }
      // Only fetch from backend if zoom is low enough to need server-side aggregation
      // or if we need fresh data for the viewport
      return mapApi.getClusters(bounds, dataZoom)
    },
    enabled: !!bounds,
    staleTime: 5000, // Keep data fresh for 5s
  })

  // Update supercluster when backend data changes
  useEffect(() => {
    if (!backendData?.clusters) return

    const points: Supercluster.PointFeature<ClusterProperties>[] = backendData.clusters.map(
      (cluster) => ({
        type: 'Feature' as const,
        properties: {
          cluster: false,
          count: cluster.count,
          representativeId: cluster.representativeId,
          cellId: cluster.id,
        },
        geometry: {
          type: 'Point' as const,
          coordinates: [cluster.lng, cluster.lat],
        },
      })
    )

    superclusterRef.current.load(points)
  }, [backendData])

  // Get clusters for current viewport
  useEffect(() => {
    if (!bounds || !backendData) return

    const bbox: [number, number, number, number] = [
      bounds.west,
      bounds.south,
      bounds.east,
      bounds.north,
    ]

    const newClusters = superclusterRef.current.getClusters(bbox, zoom)
    setClusters(newClusters)
  }, [backendData, bounds, zoom])

  return {
    clusters,
    isLoading,
    totalCount: backendData?.totalCount ?? 0,
    supercluster: superclusterRef.current,
    error,
  }
}

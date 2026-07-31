import { apiClient } from './client'
import type { Media } from './types'

export interface BoundingBox {
  north: number
  south: number
  east: number
  west: number
}

export interface MapClusterRequest {
  bounds: BoundingBox
  zoom: number
  filters?: Record<string, unknown>
}

export interface MapMediaRequest {
  bounds: BoundingBox
  geohashPrefixes?: string[]
}

export interface Cluster {
  id: string
  lat: number
  lng: number
  count: number
  representativeId: number
}

export interface MapClustersResponse {
  clusters: Cluster[]
  totalCount: number
}

export interface MapMediaResponse {
  items: Media[]
}

export const mapApi = {
  getClusters: async (bounds: BoundingBox, zoom: number): Promise<MapClustersResponse> => {
    const response = await apiClient.post<MapClustersResponse>('/map/clusters', {
      bounds,
      zoom,
    })
    return response.data
  },
  getMedia: async (request: MapMediaRequest): Promise<MapMediaResponse> => {
    const response = await apiClient.post<MapMediaResponse>('/map/media', request)
    return response.data
  },
}

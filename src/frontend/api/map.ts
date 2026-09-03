import { apiClient } from './client'
import type { Media } from './types'

export interface BoundingBox {
  north: number
  south: number
  east: number
  west: number
}

const MIN_LATITUDE = -90
const MAX_LATITUDE = 90
const MIN_LONGITUDE = -180
const MAX_LONGITUDE = 180
const FULL_LONGITUDE_SPAN = 360

function wrapLongitude(longitude: number): number {
  const remainder = ((longitude % FULL_LONGITUDE_SPAN) + FULL_LONGITUDE_SPAN) % FULL_LONGITUDE_SPAN
  return remainder >= MAX_LONGITUDE ? remainder - FULL_LONGITUDE_SPAN : remainder
}

export function normalizeMapBounds(bounds: BoundingBox): BoundingBox {
  const coordinates = [bounds.north, bounds.south, bounds.east, bounds.west]
  if (!coordinates.every(Number.isFinite)) {
    throw new RangeError('Map bounds must contain finite coordinates')
  }
  if (bounds.south > bounds.north) {
    throw new RangeError('Map bounds south must not exceed north')
  }

  const normalized = {
    north: Math.min(Math.max(bounds.north, MIN_LATITUDE), MAX_LATITUDE),
    south: Math.min(Math.max(bounds.south, MIN_LATITUDE), MAX_LATITUDE),
    east: bounds.east,
    west: bounds.west,
  }
  const longitudesAreCanonical =
    bounds.west >= MIN_LONGITUDE &&
    bounds.west <= MAX_LONGITUDE &&
    bounds.east >= MIN_LONGITUDE &&
    bounds.east <= MAX_LONGITUDE
  if (longitudesAreCanonical) return normalized

  if (bounds.east >= bounds.west && bounds.east - bounds.west >= FULL_LONGITUDE_SPAN) {
    return { ...normalized, east: MAX_LONGITUDE, west: MIN_LONGITUDE }
  }

  return {
    ...normalized,
    east: wrapLongitude(bounds.east),
    west: wrapLongitude(bounds.west),
  }
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
      bounds: normalizeMapBounds(bounds),
      zoom,
    })
    return response.data
  },
  getMedia: async (request: MapMediaRequest): Promise<MapMediaResponse> => {
    const response = await apiClient.post<MapMediaResponse>('/map/media', {
      ...request,
      bounds: normalizeMapBounds(request.bounds),
    })
    return response.data
  },
}

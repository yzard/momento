import { apiClient } from './client'
import type { Media, TimelineGroup } from './types'

interface MediaListRequest {
  cursor?: string
  limit?: number
}

type GroupBy = 'year' | 'month' | 'week' | 'day'

interface TimelineListRequest {
  cursor?: string
  limit?: number
  groupBy?: GroupBy
}

interface TimelineListResponse {
  groups: TimelineGroup[]
  nextCursor: string | null
  hasMore: boolean
}

interface MediaListResponse {
  items: Media[]
  nextCursor: string | null
  hasMore: boolean
}

interface GeoMedia {
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

export type { GroupBy }

// Cache for blob URLs to avoid re-fetching
const blobUrlCache = new Map<string, string>()

// Helper to fetch media with Authorization header and return blob URL
async function fetchMediaAsBlob(url: string, cacheKey: string): Promise<string> {
  // Check cache first
  const cached = blobUrlCache.get(cacheKey)
  if (cached) {
    return cached
  }

  const token = localStorage.getItem('momento_access_token')
  const response = await fetch(url, {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  })

  if (!response.ok) {
    throw new Error(`Failed to fetch media: ${response.status}`)
  }

  const blob = await response.blob()
  const blobUrl = URL.createObjectURL(blob)

  // Cache the blob URL
  blobUrlCache.set(cacheKey, blobUrl)

  return blobUrl
}

export const mediaApi = {
  list: async (params: MediaListRequest = {}): Promise<MediaListResponse> => {
    const response = await apiClient.post<MediaListResponse>('/media/list', params)
    return response.data
  },

  get: async (mediaId: number): Promise<Media> => {
    const response = await apiClient.post<Media>('/media/get', { mediaId })
    return response.data
  },

  delete: async (mediaId: number): Promise<void> => {
    await apiClient.post('/media/delete', { mediaId })
  },

  // Fetch file with Authorization header and return blob URL
  getFileUrl: async (mediaId: number): Promise<string> => {
    return fetchMediaAsBlob(`/api/v1/media/file/${mediaId}`, `file-${mediaId}`)
  },

  // Fetch preview with Authorization header and return blob URL
  getPreviewUrl: async (mediaId: number): Promise<string> => {
    return fetchMediaAsBlob(`/api/v1/preview/${mediaId}`, `preview-${mediaId}`)
  },

  // Fetch thumbnail with Authorization header and return blob URL
  getThumbnailUrl: async (mediaId: number): Promise<string> => {
    return fetchMediaAsBlob(`/api/v1/thumbnail/${mediaId}`, `thumbnail-${mediaId}`)
  },

  // Clear cached blob URLs (call on logout or when needed)
  clearCache: () => {
    blobUrlCache.forEach((url) => URL.revokeObjectURL(url))
    blobUrlCache.clear()
  },

  timeline: async (params: TimelineListRequest = {}): Promise<TimelineListResponse> => {
    const response = await apiClient.post<TimelineListResponse>('/timeline/list', params)
    return response.data
  },

  mapMedia: async (): Promise<GeoMedia[]> => {
    const response = await apiClient.post<{ items: GeoMedia[] }>('/map/media')
    return response.data.items
  },
}

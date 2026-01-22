import { apiClient } from './client'
import type { Media, TimelineGroup } from './types'

interface MediaListRequest {
  cursor?: string
  limit?: number
  groupBy?: GroupBy
}

type GroupBy = 'year' | 'month' | 'week' | 'day'
type ThumbnailSize = 'normal' | 'tiny'

export type { ThumbnailSize }


interface MediaListResponse {
  items: Media[]
  nextCursor: string | null
  hasMore: boolean
}

interface TimelineListResponse {
  groups: TimelineGroup[]
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
// Cache for in-flight requests to avoid duplicate fetches
const pendingRequests = new Map<string, Promise<string>>()
const pendingThumbnailBatch = new Map<string, Promise<Map<number, string>>>()
const pendingPreviewBatch = new Map<string, Promise<Map<number, string>>>()

// Helper to fetch media with Authorization header and return blob URL
async function fetchMediaAsBlob(url: string, cacheKey: string): Promise<string> {
  const cached = blobUrlCache.get(cacheKey)
  if (cached) {
    return cached
  }

  const pending = pendingRequests.get(cacheKey)
  if (pending) {
    return pending
  }

  const fetchPromise = (async () => {
    try {
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

      blobUrlCache.set(cacheKey, blobUrl)
      return blobUrl
    } finally {
      pendingRequests.delete(cacheKey)
    }
  })()

  pendingRequests.set(cacheKey, fetchPromise)
  return fetchPromise
}


export const mediaApi = {
  isThumbnailCached: (mediaId: number, size: ThumbnailSize = 'normal'): boolean => {
    return blobUrlCache.has(`thumbnail-${size}-${mediaId}`)
  },

  getCachedThumbnailUrl: (mediaId: number, size: ThumbnailSize = 'normal'): string | undefined => {
    return blobUrlCache.get(`thumbnail-${size}-${mediaId}`)
  },

  getThumbnailBatch: async (mediaIds: number[], size: ThumbnailSize = 'normal'): Promise<Map<number, string>> => {
    const uniqueIds = Array.from(new Set(mediaIds)).filter((id) => id > 0)
    if (uniqueIds.length === 0) {
      return new Map()
    }

    const cacheKeys = uniqueIds.map((id) => `thumbnail-${size}-${id}`)
    const cached = new Map<number, string>()
    const missingIds: number[] = []

    uniqueIds.forEach((id, idx) => {
      const cachedUrl = blobUrlCache.get(cacheKeys[idx] as string)
      if (cachedUrl) {
        cached.set(id, cachedUrl)
      } else {
        missingIds.push(id)
      }
    })

    if (missingIds.length === 0) {
      return cached
    }

    const batchKey = `${size}:${missingIds.join(',')}`
    const pending = pendingThumbnailBatch.get(batchKey)
    if (pending) {
      const pendingResult = await pending
      pendingResult.forEach((value, id) => cached.set(id, value))
      return cached
    }

    const fetchPromise = (async () => {
      try {
        const response = await apiClient.post<{ thumbnails: Record<string, string | null> }>(
          '/thumbnail/get',
          { mediaIds: missingIds, size }
        )
        const result = new Map<number, string>()
        Object.entries(response.data.thumbnails).forEach(([id, data]) => {
          const numericId = Number(id)
          if (!Number.isNaN(numericId) && data) {
            blobUrlCache.set(`thumbnail-${size}-${numericId}`, data)
            result.set(numericId, data)
          }
        })
        return result
      } finally {
        pendingThumbnailBatch.delete(batchKey)
      }
    })()

    pendingThumbnailBatch.set(batchKey, fetchPromise)

    const batchResult = await fetchPromise
    batchResult.forEach((value, id) => cached.set(id, value))
    return cached
  },

  getPreviewBatch: async (mediaIds: number[]): Promise<Map<number, string>> => {
    const uniqueIds = Array.from(new Set(mediaIds)).filter((id) => id > 0)
    if (uniqueIds.length === 0) {
      return new Map()
    }

    const cacheKeys = uniqueIds.map((id) => `preview-${id}`)
    const cached = new Map<number, string>()
    const missingIds: number[] = []

    uniqueIds.forEach((id, idx) => {
      const cachedUrl = blobUrlCache.get(cacheKeys[idx] as string)
      if (cachedUrl) {
        cached.set(id, cachedUrl)
      } else {
        missingIds.push(id)
      }
    })

    if (missingIds.length === 0) {
      return cached
    }

    const batchKey = missingIds.join(',')
    const pending = pendingPreviewBatch.get(batchKey)
    if (pending) {
      const pendingResult = await pending
      pendingResult.forEach((value, id) => cached.set(id, value))
      return cached
    }

    const fetchPromise = (async () => {
      try {
        const response = await apiClient.post<{ previews: Record<string, string | null> }>(
          '/preview/get',
          { ids: missingIds }
        )
        const result = new Map<number, string>()
        Object.entries(response.data.previews).forEach(([id, data]) => {
          const numericId = Number(id)
          if (!Number.isNaN(numericId) && data) {
            blobUrlCache.set(`preview-${numericId}`, data)
            result.set(numericId, data)
          }
        })
        return result
      } finally {
        pendingPreviewBatch.delete(batchKey)
      }
    })()

    pendingPreviewBatch.set(batchKey, fetchPromise)

    const batchResult = await fetchPromise
    batchResult.forEach((value, id) => cached.set(id, value))
    return cached
  },

  list: async (params: MediaListRequest = {}): Promise<MediaListResponse> => {
    const response = await apiClient.post<MediaListResponse>('/media/list', params)
    return response.data
  },

  listTimeline: async (params: MediaListRequest = {}): Promise<TimelineListResponse> => {
    const response = await apiClient.post<TimelineListResponse>('/media/list', params)
    return response.data
  },

  listMapMedia: async (): Promise<Media[]> => {
    const response = await apiClient.post<MediaListResponse>('/media/list', {})
    return response.data.items
  },

  get: async (mediaId: number): Promise<Media> => {
    const response = await apiClient.post<Media>('/media/get', { mediaId })
    return response.data
  },

  delete: async (mediaId: number): Promise<void> => {
    await apiClient.post('/media/delete', { mediaId })
  },

  getFileUrl: async (mediaId: number): Promise<string> => {
    return fetchMediaAsBlob(`/api/v1/media/file/${mediaId}`, `file-${mediaId}`)
  },

  getFileStreamUrl: (mediaId: number): string => {
    const token = localStorage.getItem('momento_access_token')
    if (token) {
      return `/api/v1/media/file/${mediaId}?token=${encodeURIComponent(token)}`
    }
    return `/api/v1/media/file/${mediaId}`
  },


  // Clear cached blob URLs (call on logout or when needed)
  clearCache: () => {
    blobUrlCache.forEach((url) => URL.revokeObjectURL(url))
    blobUrlCache.clear()
  },


  mapMedia: async (): Promise<GeoMedia[]> => {
    const response = await apiClient.post<{ items: GeoMedia[] }>('/map/media')
    return response.data.items
  },
}

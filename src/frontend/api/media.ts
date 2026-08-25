import { apiClient } from './client'
import type { Media, TimelineGroup } from './types'

interface TimelineListRequest {
  cursor?: string
  limit: number
  groupBy: GroupBy
  search: string
  mediaType?: MediaTypeFilter
  classification: TimelineClassification | null
  direction: TimelineDirection
  anchorDate?: string
}

interface MediaBatchRequest {
  ids: number[]
}

type GroupBy = 'year' | 'month' | 'week' | 'day'
type MediaTypeFilter = 'image' | 'video'
type TimelineClassification = 'screenshot' | 'document'
type TimelineDirection = 'older' | 'newer'
type ThumbnailSize = 'normal' | 'tiny'

export type { MediaTypeFilter, ThumbnailSize, TimelineClassification }


interface MediaBatchResponse {
  items: Media[]
}

interface MediaAccessTicketResponse {
  url: string
  expiresAt: string
}

interface TimelineListResponse {
  groups: TimelineGroup[]
  nextCursor: string | null
  previousCursor: string | null
  hasOlder: boolean
  hasNewer: boolean
}

interface TimelineMarker {
  label: string
  anchorDate: string
}

interface TimelineMarkersResponse {
  markers: TimelineMarker[]
}

export type { GroupBy, TimelineDirection, TimelineListRequest, TimelineListResponse, TimelineMarker, TimelineMarkersResponse }

export function thumbnailResponseMap(thumbnails: Record<string, string | null>): Map<number, string> {
  const decoded = new Map<number, string>()
  Object.entries(thumbnails).forEach(([id, thumbnail]) => {
    const mediaId = Number(id)
    if (!Number.isNaN(mediaId) && thumbnail) decoded.set(mediaId, thumbnail)
  })
  return decoded
}

// Cache for blob URLs to avoid re-fetching
const blobUrlCache = new Map<string, string>()
const pendingThumbnailBatch = new Map<string, Promise<Map<number, string>>>()
const pendingPreviewBatch = new Map<string, Promise<Map<number, string>>>()


export const mediaApi = {
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
        const result = thumbnailResponseMap(response.data.thumbnails)
        result.forEach((thumbnail, mediaId) => {
          blobUrlCache.set(`thumbnail-${size}-${mediaId}`, thumbnail)
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

  listTimeline: async (params: TimelineListRequest): Promise<TimelineListResponse> => {
    const response = await apiClient.post<TimelineListResponse>('/timeline/list', params)
    return response.data
  },

  getTimelineMarkers: async (mediaType: MediaTypeFilter | null, classification: TimelineClassification | null, search: string): Promise<TimelineMarkersResponse> => {
    const response = await apiClient.post<TimelineMarkersResponse>('/timeline/markers', {
      mediaType: mediaType ?? undefined,
      classification,
      search,
    })
    return response.data
  },

  getBatch: async (mediaIds: number[]): Promise<Media[]> => {
    if (mediaIds.length === 0) return []
    const response = await apiClient.post<MediaBatchResponse>('/media/get-batch', { ids: mediaIds } as MediaBatchRequest)
    return response.data.items
  },

  delete: async (mediaIds: number[]): Promise<void> => {
    await apiClient.post('/media/delete', { mediaIds })
  },

  getFileStreamURL: async (mediaId: number): Promise<string> => {
    const response = await apiClient.post<MediaAccessTicketResponse>('/media/access-ticket', {
      mediaId,
      resource: 'original',
    })
    return response.data.url
  },


  // Clear cached blob URLs (call on logout or when needed)
  clearCache: () => {
    blobUrlCache.forEach((url) => URL.revokeObjectURL(url))
    blobUrlCache.clear()
  },
}

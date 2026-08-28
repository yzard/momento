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

export type {
  GroupBy,
  TimelineDirection,
  TimelineListRequest,
  TimelineListResponse,
  TimelineMarker,
  TimelineMarkersResponse,
}

export function thumbnailResponseMap(
  thumbnails: Record<string, string | null>
): Map<number, string> {
  const decoded = new Map<number, string>()
  Object.entries(thumbnails).forEach(([id, thumbnail]) => {
    const mediaId = Number(id)
    if (!Number.isNaN(mediaId) && thumbnail) decoded.set(mediaId, thumbnail)
  })
  return decoded
}

const assetURLCache = new Map<string, string>()
const pendingThumbnailBatches = new Map<string, Promise<Map<number, string>>>()
const pendingPreviewBatches = new Map<string, Promise<Map<number, string>>>()
let assetCacheGeneration = 0

interface AssetBatchOptions {
  mediaIds: number[]
  cacheKey: (mediaId: number) => string
  batchKey: (missingIds: number[]) => string
  pendingBatches: Map<string, Promise<Map<number, string>>>
  fetchMissing: (missingIds: number[]) => Promise<Map<number, string>>
  supersededMessage: string
}

async function loadAssetBatch({
  mediaIds,
  cacheKey,
  batchKey,
  pendingBatches,
  fetchMissing,
  supersededMessage,
}: AssetBatchOptions): Promise<Map<number, string>> {
  const uniqueIds = Array.from(new Set(mediaIds)).filter((mediaId) => mediaId > 0)
  if (uniqueIds.length === 0) return new Map()

  const cached = new Map<number, string>()
  const missingIds: number[] = []
  uniqueIds.forEach((mediaId) => {
    const cachedURL = assetURLCache.get(cacheKey(mediaId))
    if (cachedURL) {
      cached.set(mediaId, cachedURL)
    } else {
      missingIds.push(mediaId)
    }
  })
  if (missingIds.length === 0) return cached

  const requestKey = batchKey(missingIds)
  const pendingRequest = pendingBatches.get(requestKey)
  if (pendingRequest) {
    const pendingResult = await pendingRequest
    pendingResult.forEach((value, mediaId) => cached.set(mediaId, value))
    return cached
  }

  const requestGeneration = assetCacheGeneration
  const fetchPromise = fetchMissing(missingIds).then((result) => {
    if (requestGeneration !== assetCacheGeneration) throw new Error(supersededMessage)
    result.forEach((value, mediaId) => assetURLCache.set(cacheKey(mediaId), value))
    return result
  })
  pendingBatches.set(requestKey, fetchPromise)

  try {
    const result = await fetchPromise
    result.forEach((value, mediaId) => cached.set(mediaId, value))
    return cached
  } finally {
    if (pendingBatches.get(requestKey) === fetchPromise) pendingBatches.delete(requestKey)
  }
}

export const mediaApi = {
  getCachedThumbnailURL: (mediaId: number, size: ThumbnailSize): string | undefined => {
    return assetURLCache.get(`thumbnail-${size}-${mediaId}`)
  },

  getThumbnailBatch: async (
    mediaIds: number[],
    size: ThumbnailSize
  ): Promise<Map<number, string>> => {
    return loadAssetBatch({
      mediaIds,
      cacheKey: (mediaId) => `thumbnail-${size}-${mediaId}`,
      batchKey: (missingIds) => `${size}:${missingIds.join(',')}`,
      pendingBatches: pendingThumbnailBatches,
      fetchMissing: async (missingIds) => {
        const response = await apiClient.post<{ thumbnails: Record<string, string | null> }>(
          '/thumbnail/get',
          { mediaIds: missingIds, size }
        )
        return thumbnailResponseMap(response.data.thumbnails)
      },
      supersededMessage: 'Thumbnail request was superseded',
    })
  },

  getPreviewBatch: async (mediaIds: number[]): Promise<Map<number, string>> => {
    return loadAssetBatch({
      mediaIds,
      cacheKey: (mediaId) => `preview-${mediaId}`,
      batchKey: (missingIds) => missingIds.join(','),
      pendingBatches: pendingPreviewBatches,
      fetchMissing: async (missingIds) => {
        const response = await apiClient.post<{ previews: Record<string, string | null> }>(
          '/preview/get',
          { ids: missingIds }
        )
        return thumbnailResponseMap(response.data.previews)
      },
      supersededMessage: 'Preview request was superseded',
    })
  },

  listTimeline: async (params: TimelineListRequest): Promise<TimelineListResponse> => {
    const response = await apiClient.post<TimelineListResponse>('/timeline/list', params)
    return response.data
  },

  getTimelineMarkers: async (
    mediaType: MediaTypeFilter | null,
    classification: TimelineClassification | null,
    search: string
  ): Promise<TimelineMarkersResponse> => {
    const response = await apiClient.post<TimelineMarkersResponse>('/timeline/markers', {
      mediaType: mediaType ?? undefined,
      classification,
      search,
    })
    return response.data
  },

  getBatch: async (mediaIds: number[]): Promise<Media[]> => {
    if (mediaIds.length === 0) return []
    const response = await apiClient.post<MediaBatchResponse>('/media/get-batch', {
      ids: mediaIds,
    } as MediaBatchRequest)
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

  clearCache: () => {
    assetCacheGeneration += 1
    assetURLCache.forEach((url) => {
      if (url.startsWith('blob:')) URL.revokeObjectURL(url)
    })
    assetURLCache.clear()
    pendingThumbnailBatches.clear()
    pendingPreviewBatches.clear()
  },
}

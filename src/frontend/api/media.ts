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

function mediaAssetURL(mediaId: number, assetPath: string): string {
  if (!Number.isSafeInteger(mediaId) || mediaId <= 0) {
    throw new Error('mediaId must be a positive safe integer')
  }
  return `/api/v1/media/${mediaId}/${assetPath}`
}

export const mediaApi = {
  getThumbnailURL: (mediaId: number, size: ThumbnailSize): string => {
    const assetPath = size === 'tiny' ? 'thumbnail/tiny' : 'thumbnail'
    return mediaAssetURL(mediaId, assetPath)
  },

  getPreviewURL: (mediaId: number): string => mediaAssetURL(mediaId, 'preview'),

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
}

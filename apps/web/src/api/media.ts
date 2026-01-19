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
  group_by?: GroupBy
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

export const mediaApi = {
  list: async (params: MediaListRequest = {}): Promise<MediaListResponse> => {
    const response = await apiClient.post<MediaListResponse>('/media/list', params)
    return response.data
  },

  get: async (mediaId: number): Promise<Media> => {
    const response = await apiClient.post<Media>('/media/get', { media_id: mediaId })
    return response.data
  },

  delete: async (mediaId: number): Promise<void> => {
    await apiClient.post('/media/delete', { media_id: mediaId })
  },

  getFileUrl: (mediaId: number): string => {
    const token = localStorage.getItem('momento_access_token')
    return `/api/v1/media/file/${mediaId}${token ? `?token=${token}` : ''}`
  },

  getPreviewUrl: (mediaId: number): string => {
    const token = localStorage.getItem('momento_access_token')
    return `/api/v1/preview/${mediaId}${token ? `?token=${token}` : ''}`
  },

  getThumbnailUrl: (mediaId: number): string => {
    const token = localStorage.getItem('momento_access_token')
    return `/api/v1/thumbnail/${mediaId}${token ? `?token=${token}` : ''}`
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

import { apiClient } from './client'
import { thumbnailResponseMap, type ThumbnailSize } from './media'

export interface TrashMedia {
  id: number
  filename: string
  originalFilename: string
  mediaType: 'image' | 'video'
  mimeType: string | null
  width: number | null
  height: number | null
  fileSize: number | null
  durationSeconds: number | null
  dateTaken: string | null
  deletedAt: string
  createdAt: string
}

interface TrashListResponse {
  items: TrashMedia[]
  totalCount: number
}

interface TrashResponse {
  message: string
  affectedCount: number
}

export const trashApi = {
  list: async (): Promise<TrashListResponse> => {
    const response = await apiClient.post<TrashListResponse>('/trash/list')
    return response.data
  },

  getThumbnailBatch: async (mediaIds: number[], size: ThumbnailSize): Promise<Map<number, string>> => {
    const response = await apiClient.post<{ thumbnails: Record<string, string | null> }>(
      '/trash/thumbnails/get',
      { mediaIds, size },
    )
    return thumbnailResponseMap(response.data.thumbnails)
  },

  restore: async (mediaIds: number[]): Promise<TrashResponse> => {
    const response = await apiClient.post<TrashResponse>('/trash/restore', {
      mediaIds,
    })
    return response.data
  },

  permanentlyDelete: async (mediaIds: number[]): Promise<TrashResponse> => {
    const response = await apiClient.post<TrashResponse>('/trash/delete', {
      mediaIds,
    })
    return response.data
  },

  emptyTrash: async (): Promise<TrashResponse> => {
    const response = await apiClient.post<TrashResponse>('/trash/empty')
    return response.data
  },
}

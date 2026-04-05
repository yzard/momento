import { apiClient } from './client'

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

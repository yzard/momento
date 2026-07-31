import { apiClient } from './client'
import type { Album, Media } from './types'

interface AlbumDetail {
  id: number
  name: string
  description: string | null
  coverMediaId: number | null
  media: Media[]
  createdAt: string
}

interface AlbumCreateRequest {
  name: string
  description?: string
}

interface AlbumUpdateRequest {
  albumId: number
  name?: string
  description?: string
  coverMediaId?: number
}

export const albumsApi = {
  list: async (): Promise<Album[]> => {
    const response = await apiClient.post<{ albums: Album[] }>('/album/list')
    return response.data.albums
  },

  get: async (albumId: number): Promise<AlbumDetail> => {
    const response = await apiClient.post<AlbumDetail>('/album/get', { albumId })
    return response.data
  },

  create: async (data: AlbumCreateRequest): Promise<Album> => {
    const response = await apiClient.post<Album>('/album/create', data)
    return response.data
  },

  update: async (data: AlbumUpdateRequest): Promise<Album> => {
    const response = await apiClient.post<Album>('/album/update', data)
    return response.data
  },

  delete: async (albumId: number): Promise<void> => {
    await apiClient.post('/album/delete', { albumId })
  },

  addMedia: async (albumId: number, mediaIds: number[]): Promise<void> => {
    await apiClient.post('/album/add-media', { albumId, mediaIds })
  },

  removeMedia: async (albumId: number, mediaIds: number[]): Promise<void> => {
    await apiClient.post('/album/remove-media', { albumId, mediaIds })
  },

  reorder: async (albumId: number, mediaIds: number[]): Promise<void> => {
    await apiClient.post('/album/reorder', { albumId, mediaIds })
  },
}

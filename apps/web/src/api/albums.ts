import { apiClient } from './client'
import type { Album, Media } from './types'

interface AlbumDetail {
  id: number
  name: string
  description: string | null
  cover_media_id: number | null
  media: Media[]
  created_at: string
}

interface AlbumCreateRequest {
  name: string
  description?: string
}

interface AlbumUpdateRequest {
  album_id: number
  name?: string
  description?: string
  cover_media_id?: number
}

export const albumsApi = {
  list: async (): Promise<Album[]> => {
    const response = await apiClient.post<{ albums: Album[] }>('/album/list')
    return response.data.albums
  },

  get: async (albumId: number): Promise<AlbumDetail> => {
    const response = await apiClient.post<AlbumDetail>('/album/get', { album_id: albumId })
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
    await apiClient.post('/album/delete', { album_id: albumId })
  },

  addMedia: async (albumId: number, mediaIds: number[]): Promise<void> => {
    await apiClient.post('/album/add-media', { album_id: albumId, media_ids: mediaIds })
  },

  removeMedia: async (albumId: number, mediaIds: number[]): Promise<void> => {
    await apiClient.post('/album/remove-media', { album_id: albumId, media_ids: mediaIds })
  },

  reorder: async (albumId: number, mediaIds: number[]): Promise<void> => {
    await apiClient.post('/album/reorder', { album_id: albumId, media_ids: mediaIds })
  },
}

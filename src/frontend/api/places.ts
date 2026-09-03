import { apiClient } from './client'
import type { Media } from './types'

export interface PlaceSummary {
  placeId: string
  city: string
  state: string | null
  country: string
  mediaCount: number
}

export interface PlacesListRequest {
  cursor: string | null
  limit: number
}

export interface PlacesListResponse {
  places: PlaceSummary[]
  nextCursor: string | null
  hasMore: boolean
}

export interface PlaceGetRequest {
  placeId: string
  cursor: string | null
  limit: number
}

export interface PlaceGetResponse {
  place: PlaceSummary
  media: Media[]
  nextCursor: string | null
  hasMore: boolean
}

export const placesApi = {
  list: async (request: PlacesListRequest): Promise<PlacesListResponse> => {
    const response = await apiClient.post<PlacesListResponse>('/places/list', request)
    return response.data
  },

  get: async (request: PlaceGetRequest): Promise<PlaceGetResponse> => {
    const response = await apiClient.post<PlaceGetResponse>('/places/get', request)
    return response.data
  },

  getThumbnail: async (placeId: string): Promise<string> =>
    `/api/v1/places/${encodeURIComponent(placeId)}/thumbnail`,
}

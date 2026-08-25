import { apiClient } from './client'
import type { Media } from './types'

export interface DuplicateGroup {
  clusterId: number
  items: Media[]
}

export interface DuplicateGroupsRequest {
  cursor: string | null
  limit: number
}

export interface DuplicateGroupsResponse {
  groups: DuplicateGroup[]
  nextCursor: string | null
  hasMore: boolean
  totalGroups: number
  totalMedia: number
}

export const duplicatesApi = {
  list: async (request: DuplicateGroupsRequest): Promise<DuplicateGroupsResponse> => {
    const response = await apiClient.post<DuplicateGroupsResponse>('/duplicates/list', request)
    return response.data
  },
}

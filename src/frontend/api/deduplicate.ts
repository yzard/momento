import { apiClient } from './client'
import type { Media } from './types'

export interface DeduplicateGroup {
  clusterId: number
  items: Media[]
}

export interface DeduplicateGroupsRequest {
  cursor: string | null
  limit: number
}

export interface DeduplicateGroupsResponse {
  groups: DeduplicateGroup[]
  nextCursor: string | null
  hasMore: boolean
}

export interface DeduplicateActionResponse {
  message: string
  status: string
}

export interface DeduplicateStatusResponse {
  status: string
  runId: number | null
  trigger: string | null
  scheduledFor: string | null
  startedAt: string | null
  completedAt: string | null
  indexedMedia: number
  processedMedia: number
  candidateComparisons: number
  clustersCreated: number
  error: string | null
  nextScheduledAt: string | null
}

export const deduplicateApi = {
  groups: async (request: DeduplicateGroupsRequest): Promise<DeduplicateGroupsResponse> => {
    const response = await apiClient.post<DeduplicateGroupsResponse>('/deduplicate/groups', request)
    return response.data
  },

  start: async (): Promise<DeduplicateActionResponse> => {
    const response = await apiClient.post<DeduplicateActionResponse>('/deduplicate/start')
    return response.data
  },

  status: async (): Promise<DeduplicateStatusResponse> => {
    const response = await apiClient.post<DeduplicateStatusResponse>('/deduplicate/status')
    return response.data
  },

  cancel: async (): Promise<DeduplicateActionResponse> => {
    const response = await apiClient.post<DeduplicateActionResponse>('/deduplicate/cancel')
    return response.data
  },

  clean: async (): Promise<DeduplicateActionResponse> => {
    const response = await apiClient.post<DeduplicateActionResponse>('/deduplicate/clean')
    return response.data
  },
}

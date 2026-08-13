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
  totalGroups: number
  totalMedia: number
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
    const response = await apiClient.post<DeduplicateGroupsResponse>('/ai/deduplicate/groups', request)
    return response.data
  },

  start: async (): Promise<DeduplicateActionResponse> => {
    const response = await apiClient.post<DeduplicateActionResponse>('/ai/deduplicate/start', {})
    return response.data
  },

  status: async (): Promise<DeduplicateStatusResponse> => {
    const response = await apiClient.post<DeduplicateStatusResponse>('/ai/deduplicate/status', {})
    return response.data
  },

  cancel: async (): Promise<DeduplicateActionResponse> => {
    const response = await apiClient.post<DeduplicateActionResponse>('/ai/deduplicate/cancel', {})
    return response.data
  },

  clean: async (): Promise<DeduplicateActionResponse> => {
    const response = await apiClient.post<DeduplicateActionResponse>('/ai/deduplicate/clean', {})
    return response.data
  },
}

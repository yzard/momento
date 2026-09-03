import { apiClient } from './client'
import type { Media } from './types'

export interface FaceGroup {
  faceGroupId: number
  faceCount: number
  mediaCount: number
}

export interface FaceGroupsListResponse {
  groups: FaceGroup[]
  nextCursor: string | null
  hasMore: boolean
}

export interface FaceGroupsListRequest {
  cursor: string | null
  limit: number
}

export interface FaceGroupResponse {
  group: FaceGroup
  media: Media[]
}

export interface FaceGroupGetRequest {
  faceGroupId: number
}

export interface FaceThumbnailRequest {
  faceGroupId: number
}

export interface FaceGroupsMergeRequest {
  faceGroupIds: number[]
}

export interface FaceGroupsMergeResponse {
  group: FaceGroup
}

export const facesApi = {
  listGroups: async (request: FaceGroupsListRequest): Promise<FaceGroupsListResponse> => {
    const response = await apiClient.post<FaceGroupsListResponse>('/faces/groups/list', request)
    return response.data
  },

  getGroup: async (request: FaceGroupGetRequest): Promise<FaceGroupResponse> => {
    const response = await apiClient.post<FaceGroupResponse>('/faces/groups/get', request)
    return response.data
  },

  getThumbnailURL: (request: FaceThumbnailRequest): string => {
    if (!Number.isSafeInteger(request.faceGroupId) || request.faceGroupId <= 0) {
      throw new Error('faceGroupId must be a positive safe integer')
    }
    return `/api/v1/faces/groups/${request.faceGroupId}/thumbnail`
  },

  mergeGroups: async (request: FaceGroupsMergeRequest): Promise<FaceGroupsMergeResponse> => {
    const response = await apiClient.post<FaceGroupsMergeResponse>('/faces/groups/merge', request)
    return response.data
  },
}

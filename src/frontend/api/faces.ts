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

const thumbnailUrlCache = new Map<number, string>()

export const facesApi = {
  listGroups: async (request: FaceGroupsListRequest): Promise<FaceGroupsListResponse> => {
    const response = await apiClient.post<FaceGroupsListResponse>('/faces/groups/list', request)
    return response.data
  },

  getGroup: async (request: FaceGroupGetRequest): Promise<FaceGroupResponse> => {
    const response = await apiClient.post<FaceGroupResponse>('/faces/groups/get', request)
    return response.data
  },

  getThumbnailUrl: async (request: FaceThumbnailRequest): Promise<string> => {
    const cachedUrl = thumbnailUrlCache.get(request.faceGroupId)
    if (cachedUrl) return cachedUrl

    const response = await apiClient.post<Blob>('/faces/thumbnails/get', request, { responseType: 'blob' })
    const thumbnailUrl = URL.createObjectURL(response.data)
    thumbnailUrlCache.set(request.faceGroupId, thumbnailUrl)
    return thumbnailUrl
  },

  mergeGroups: async (request: FaceGroupsMergeRequest): Promise<FaceGroupsMergeResponse> => {
    const response = await apiClient.post<FaceGroupsMergeResponse>('/faces/groups/merge', request)
    return response.data
  },
}

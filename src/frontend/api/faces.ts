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

const thumbnailURLCache = new Map<number, string>()
const pendingThumbnailRequests = new Map<number, Promise<string>>()
let thumbnailCacheGeneration = 0

function clearThumbnailCache(): void {
  thumbnailCacheGeneration += 1
  thumbnailURLCache.forEach((thumbnailURL) => URL.revokeObjectURL(thumbnailURL))
  thumbnailURLCache.clear()
  pendingThumbnailRequests.clear()
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

  getThumbnailURL: async (request: FaceThumbnailRequest): Promise<string> => {
    const cachedURL = thumbnailURLCache.get(request.faceGroupId)
    if (cachedURL) return cachedURL

    const pendingRequest = pendingThumbnailRequests.get(request.faceGroupId)
    if (pendingRequest) return pendingRequest

    const requestGeneration = thumbnailCacheGeneration
    const thumbnailRequest = (async () => {
      const response = await apiClient.post<Blob>('/faces/thumbnails/get', request, {
        responseType: 'blob',
      })
      if (requestGeneration !== thumbnailCacheGeneration) {
        throw new Error('Face thumbnail request was superseded')
      }
      const thumbnailURL = URL.createObjectURL(response.data)
      thumbnailURLCache.set(request.faceGroupId, thumbnailURL)
      return thumbnailURL
    })()

    pendingThumbnailRequests.set(request.faceGroupId, thumbnailRequest)
    try {
      return await thumbnailRequest
    } finally {
      if (pendingThumbnailRequests.get(request.faceGroupId) === thumbnailRequest) {
        pendingThumbnailRequests.delete(request.faceGroupId)
      }
    }
  },

  mergeGroups: async (request: FaceGroupsMergeRequest): Promise<FaceGroupsMergeResponse> => {
    const response = await apiClient.post<FaceGroupsMergeResponse>('/faces/groups/merge', request)
    clearThumbnailCache()
    return response.data
  },

  clearThumbnailCache,
}

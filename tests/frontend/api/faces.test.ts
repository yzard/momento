import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { facesApi } from '../../../src/frontend/api/faces'

describe('facesApi', () => {
  beforeEach(() => {
    post.mockReset()
  })

  it('lists and loads face groups with typed requests', async () => {
    const groupsResponse = {
      groups: [{ faceGroupId: 7, faceCount: 3, mediaCount: 2 }],
      nextCursor: null,
      hasMore: false,
    }
    const groupResponse = {
      group: { faceGroupId: 7, faceCount: 3, mediaCount: 2 },
      media: [],
    }
    post
      .mockResolvedValueOnce({ data: groupsResponse })
      .mockResolvedValueOnce({ data: groupResponse })

    await expect(facesApi.listGroups({ cursor: null, limit: 100 })).resolves.toEqual(groupsResponse)
    await expect(facesApi.getGroup({ faceGroupId: 7 })).resolves.toEqual(groupResponse)
    expect(post).toHaveBeenNthCalledWith(1, '/faces/groups/list', {
      cursor: null,
      limit: 100,
    })
    expect(post).toHaveBeenNthCalledWith(2, '/faces/groups/get', {
      faceGroupId: 7,
    })
  })

  it('builds the representative thumbnail binary URL', () => {
    expect(facesApi.getThumbnailURL({ faceGroupId: 8 })).toBe('/api/v1/faces/groups/8/thumbnail')
    expect(post).not.toHaveBeenCalled()
  })

  it('merges selected groups', async () => {
    const response = { group: { faceGroupId: 7, faceCount: 5, mediaCount: 4 } }
    post.mockResolvedValue({ data: response })

    await expect(facesApi.mergeGroups({ faceGroupIds: [7, 8] })).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/faces/groups/merge', {
      faceGroupIds: [7, 8],
    })
  })
})

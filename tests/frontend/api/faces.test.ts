import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { facesApi } from '../../../src/frontend/api/faces'

describe('facesApi', () => {
  beforeEach(() => {
    post.mockReset()
    vi.stubGlobal('URL', {
      createObjectURL: vi.fn((blob: Blob) => `blob:${blob.size}`),
      revokeObjectURL: vi.fn(),
    })
    facesApi.clearThumbnailCache()
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

  it('requests representative thumbnails as blobs', async () => {
    vi.mocked(URL.createObjectURL).mockReturnValue('blob:face-group-8')
    post.mockResolvedValue({ data: new Blob(['thumbnail']) })

    await expect(facesApi.getThumbnailURL({ faceGroupId: 8 })).resolves.toBe('blob:face-group-8')
    expect(post).toHaveBeenCalledWith(
      '/faces/thumbnails/get',
      { faceGroupId: 8 },
      { responseType: 'blob' }
    )
  })

  it('rejects responses from a previous session', async () => {
    let resolveThumbnail!: (value: { data: Blob }) => void
    post.mockReturnValue(
      new Promise((resolve) => {
        resolveThumbnail = resolve
      })
    )
    const pendingThumbnail = facesApi.getThumbnailURL({ faceGroupId: 9 })

    facesApi.clearThumbnailCache()
    resolveThumbnail({ data: new Blob(['stale']) })

    await expect(pendingThumbnail).rejects.toThrow('Face thumbnail request was superseded')
    expect(URL.createObjectURL).not.toHaveBeenCalled()
  })

  it('revokes object URLs when clearing the cache', async () => {
    vi.mocked(URL.createObjectURL).mockReturnValue('blob:face-group-10')
    post.mockResolvedValue({ data: new Blob(['thumbnail']) })

    await facesApi.getThumbnailURL({ faceGroupId: 10 })
    facesApi.clearThumbnailCache()

    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:face-group-10')
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

import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { albumsApi } from '../../../src/frontend/api/albums'

describe('albumsApi', () => {
  beforeEach(() => {
    post.mockReset()
  })

  it('treats album creation as an album detail response', async () => {
    post.mockResolvedValue({
      data: {
        id: 7,
        name: 'Trip',
        description: null,
        coverMediaId: null,
        media: [],
        createdAt: '2026-01-01T00:00:00Z',
      },
    })

    const album = await albumsApi.create({ name: 'Trip', mediaIds: [] })

    expect(post).toHaveBeenCalledWith('/album/create', {
      name: 'Trip',
      mediaIds: [],
    })
    expect(album.media).toEqual([])
  })

  it('creates an album and associates media in one request', async () => {
    post.mockResolvedValueOnce({
      data: {
        id: 7,
        name: 'Trip',
        description: null,
        coverMediaId: null,
        media: [{ id: 10 }, { id: 11 }],
        createdAt: '2026-01-01T00:00:00Z',
      },
    })

    const album = await albumsApi.create({ name: 'Trip', mediaIds: [10, 11] })

    expect(post.mock.calls).toEqual([['/album/create', { name: 'Trip', mediaIds: [10, 11] }]])
    expect(album.media.map((media) => media.id)).toEqual([10, 11])
  })

  it('sends the complete ordered media list when reordering', async () => {
    post.mockResolvedValue({
      data: { message: 'Album reordered successfully' },
    })

    await albumsApi.reorder(7, [42, 17, 9])

    expect(post).toHaveBeenCalledWith('/album/reorder', {
      albumId: 7,
      mediaIds: [42, 17, 9],
    })
  })
})

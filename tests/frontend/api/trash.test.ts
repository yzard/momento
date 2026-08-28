import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { trashApi } from '../../../src/frontend/api/trash'

describe('trashApi thumbnails', () => {
  beforeEach(() => {
    post.mockReset()
  })

  it('loads deleted thumbnails through the trash access contract', async () => {
    post.mockResolvedValue({
      data: {
        thumbnails: { '7': 'data:image/jpeg;base64,dGlueQ==', '8': null },
      },
    })

    const thumbnails = await trashApi.getThumbnailBatch([7, 8], 'tiny')

    expect(post).toHaveBeenCalledWith('/trash/thumbnails/get', {
      mediaIds: [7, 8],
      size: 'tiny',
    })
    expect(thumbnails).toEqual(new Map([[7, 'data:image/jpeg;base64,dGlueQ==']]))
  })
})

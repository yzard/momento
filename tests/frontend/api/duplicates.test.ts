import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { duplicatesApi } from '../../../src/frontend/api/duplicates'

describe('duplicatesApi', () => {
  beforeEach(() => post.mockReset())

  it('lists visible groups with the cursor contract', async () => {
    const response = {
      groups: [],
      nextCursor: null,
      hasMore: false,
      totalGroups: 0,
      totalMedia: 0,
    }
    post.mockResolvedValue({ data: response })

    await expect(duplicatesApi.list({ cursor: '12', limit: 20 })).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/duplicates/list', {
      cursor: '12',
      limit: 20,
    })
  })
})

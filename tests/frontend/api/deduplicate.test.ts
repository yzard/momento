import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({
  apiClient: { post },
}))

import { deduplicateApi } from '../../../src/frontend/api/deduplicate'

describe('deduplicateApi', () => {
  beforeEach(() => post.mockReset())

  it('lists groups with the cursor contract', async () => {
    const response = {
      groups: [],
      nextCursor: null,
      hasMore: false,
      totalGroups: 0,
      totalMedia: 0,
    }
    post.mockResolvedValue({ data: response })

    await expect(deduplicateApi.groups({ cursor: '12', limit: 20 })).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/deduplicate/groups', { cursor: '12', limit: 20 })
  })

  it.each([
    ['start', '/ai/deduplicate/start'],
    ['status', '/ai/deduplicate/status'],
    ['cancel', '/ai/deduplicate/cancel'],
    ['clean', '/ai/deduplicate/clean'],
  ] as const)('calls the %s operation', async (operation, path) => {
    post.mockResolvedValue({ data: { message: 'ok', status: 'idle' } })

    await deduplicateApi[operation]()

    expect(post).toHaveBeenCalledWith(path, {})
  })
})

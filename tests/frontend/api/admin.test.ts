import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({
  apiClient: { post },
}))

import { aiApi } from '../../../src/frontend/api/ai'

describe('aiApi', () => {
  beforeEach(() => post.mockReset())

  it('triggers durable AI work', async () => {
    const response = { message: 'AI processing queued', queuedJobs: 2 }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.trigger()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/trigger', {})
  })
})

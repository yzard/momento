import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { aiApi } from '../../../src/frontend/api/ai'

describe('AI administrator API', () => {
  beforeEach(() => post.mockReset())

  it('returns per-feature outcomes for a global action', async () => {
    const response = {
      action: 'start',
      results: [{ feature: 'ocr', outcome: 'queued', affectedJobs: 2, error: null }],
    }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.start()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/start')
  })

  it('loads every task from one aggregate status endpoint', async () => {
    const response = { tasks: [], deduplicate: { status: 'idle' }, faceGroups: 8 }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.status()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/status')
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { metadataApi } from '../../../src/frontend/api/metadata'

describe('metadataApi', () => {
  beforeEach(() => {
    post.mockReset()
    post.mockResolvedValue({ data: { message: 'completed', affectedJobs: 0 } })
  })

  it('uses the generate, cancel, clean, and status lifecycle endpoints', async () => {
    await metadataApi.generate()
    await metadataApi.cancel()
    await metadataApi.clean()
    await metadataApi.getStatus()

    expect(post.mock.calls).toEqual([
      ['/metadata/generate', {}],
      ['/metadata/cancel', {}],
      ['/metadata/clean', {}],
      ['/metadata/status', {}],
    ])
  })
})

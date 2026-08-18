import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { aiApi } from '../../../src/frontend/api/ai'

describe('aiApi image aesthetics', () => {
  beforeEach(() => post.mockResolvedValue({ data: { message: 'ok', queuedJobs: 0 } }))

  it('uses the image aesthetics control and status endpoints', async () => {
    await aiApi.triggerImageAesthetics()
    await aiApi.cancelImageAesthetics()
    await aiApi.cleanImageAesthetics()
    await aiApi.getImageAestheticsStatus()

    expect(post.mock.calls).toEqual([
      ['/ai/image_aesthetics/trigger', {}],
      ['/ai/image_aesthetics/cancel', {}],
      ['/ai/image_aesthetics/clean', {}],
      ['/ai/image_aesthetics/status', {}],
    ])
  })
})

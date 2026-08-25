import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({
  apiClient: { post },
}))

import { aiApi } from '../../../src/frontend/api/ai'

describe('aiApi', () => {
  beforeEach(() => post.mockReset())

  it('starts durable AI work', async () => {
    const response = { message: 'AI processing queued', queuedJobs: 2 }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.start()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/start', {})
  })

  it('cancels durable AI work', async () => {
    const response = { message: 'AI jobs cancelled', queuedJobs: 2 }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.cancel()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/cancel', {})
  })

  it('starts face detection work', async () => {
    const response = { message: 'Face detection queued', queuedJobs: 2 }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.startFaces()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/faces/start', {})
  })

  it('loads face detection and group status', async () => {
    const response = { status: 'idle', queuedJobs: 0, processingJobs: 0, completedJobs: 20, failedJobs: 0, errors: [], faceGroups: 8 }
    post.mockResolvedValue({ data: response })

    await expect(aiApi.getFacesStatus()).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/ai/faces/status', {})
  })
})

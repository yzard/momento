import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getCachedThumbnailUrl: vi.fn(),
  getThumbnailBatch: vi.fn(),
  getPreviewBatch: vi.fn(),
}))

vi.mock('../../../src/frontend/api/media', () => ({ mediaApi: mocks }))

import { batchLoader } from '../../../src/frontend/utils/batcher'

describe('batchLoader', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mocks.getCachedThumbnailUrl.mockReset().mockReturnValue(undefined)
    mocks.getThumbnailBatch.mockReset().mockResolvedValue(new Map([[42, 'place-thumbnail']]))
    mocks.getPreviewBatch.mockReset()
  })

  afterEach(() => vi.useRealTimers())

  it('batches normal media thumbnails', async () => {
    const thumbnailPromise = batchLoader.load(42)
    await vi.runAllTimersAsync()

    await expect(thumbnailPromise).resolves.toBe('place-thumbnail')
    expect(mocks.getCachedThumbnailUrl).toHaveBeenCalledWith(42, 'normal')
    expect(mocks.getThumbnailBatch).toHaveBeenCalledWith([42], 'normal')
  })

  it('coalesces subscribers while a batch is in flight', async () => {
    let resolveBatch!: (value: Map<number, string>) => void
    mocks.getThumbnailBatch.mockReturnValue(new Promise((resolve) => {
      resolveBatch = resolve
    }))
    const first = batchLoader.load(42)
    await vi.runAllTimersAsync()
    const second = batchLoader.load(42)

    resolveBatch(new Map([[42, 'shared-thumbnail']]))

    await expect(first).resolves.toBe('shared-thumbnail')
    await expect(second).resolves.toBe('shared-thumbnail')
    expect(mocks.getThumbnailBatch).toHaveBeenCalledOnce()
  })

  it('retries an asset after a failed batch', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
    mocks.getThumbnailBatch
      .mockRejectedValueOnce(new Error('temporary failure'))
      .mockResolvedValueOnce(new Map([[42, 'retried-thumbnail']]))

    const failed = batchLoader.load(42)
    await vi.runAllTimersAsync()
    await expect(failed).resolves.toBeNull()
    const retried = batchLoader.load(42)
    await vi.runAllTimersAsync()

    await expect(retried).resolves.toBe('retried-thumbnail')
    expect(mocks.getThumbnailBatch).toHaveBeenCalledTimes(2)
  })
})

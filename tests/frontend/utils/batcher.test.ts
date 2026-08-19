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
})

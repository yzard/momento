import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getCachedThumbnailUrl: vi.fn(),
  getThumbnailBatch: vi.fn(),
  getPreviewBatch: vi.fn(),
}))

vi.mock('../../../src/frontend/api/media', () => ({ mediaApi: mocks }))

import { placeBatchLoader } from '../../../src/frontend/utils/batcher'

describe('placeBatchLoader', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mocks.getCachedThumbnailUrl.mockReset().mockReturnValue(undefined)
    mocks.getThumbnailBatch.mockReset().mockResolvedValue(new Map([[42, 'place-thumbnail']]))
    mocks.getPreviewBatch.mockReset()
  })

  afterEach(() => vi.useRealTimers())

  it('batches representative thumbnails using the place size', async () => {
    const thumbnailPromise = placeBatchLoader.load(42)
    await vi.runAllTimersAsync()

    await expect(thumbnailPromise).resolves.toBe('place-thumbnail')
    expect(mocks.getCachedThumbnailUrl).toHaveBeenCalledWith(42, 'place')
    expect(mocks.getThumbnailBatch).toHaveBeenCalledWith([42], 'place')
  })
})

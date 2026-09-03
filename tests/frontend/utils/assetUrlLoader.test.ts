import { describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getThumbnailURL: vi.fn(),
}))

vi.mock('../../../src/frontend/api/media', () => ({ mediaApi: mocks }))

import { thumbnailUrlLoader } from '../../../src/frontend/utils/assetUrlLoader'

describe('thumbnailUrlLoader', () => {
  it('resolves the direct binary URL without a batching delay', async () => {
    mocks.getThumbnailURL.mockReturnValue('/api/v1/media/42/thumbnail')

    await expect(thumbnailUrlLoader.load(42)).resolves.toBe('/api/v1/media/42/thumbnail')
    expect(mocks.getThumbnailURL).toHaveBeenCalledWith(42, 'normal')
  })
})

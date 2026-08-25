import { act, fireEvent, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Media } from '../../../../src/frontend/api/types'

const mocks = vi.hoisted(() => ({
  getCachedThumbnailUrl: vi.fn(),
  getFileStreamURL: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: {
    getCachedThumbnailUrl: mocks.getCachedThumbnailUrl,
    getFileStreamURL: mocks.getFileStreamURL,
  },
}))

vi.mock('../../../../src/frontend/utils/batcher', () => ({
  batchLoader: { load: vi.fn() },
}))

import PhotoGrid from '../../../../src/frontend/components/timeline/PhotoGrid'

const video = {
  id: 5,
  mediaType: 'video',
  mimeType: 'video/mp4',
  originalFilename: 'preview.mp4',
  durationSeconds: 60,
} as Media

describe('PhotoGrid', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    mocks.getCachedThumbnailUrl.mockReset()
    mocks.getFileStreamURL.mockReset()
    mocks.getCachedThumbnailUrl.mockReturnValue('/thumbnail/5')
    mocks.getFileStreamURL.mockResolvedValue('/stream/5?ticket=signed')
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('requests a stream ticket only after the hover preview delay', async () => {
    const view = render(<PhotoGrid media={[video]} onPhotoClick={vi.fn()} />)
    const thumbnail = view.getByRole('img', { name: 'preview.mp4' })

    fireEvent.mouseEnter(thumbnail.parentElement as HTMLElement)
    expect(mocks.getFileStreamURL).not.toHaveBeenCalled()
    await act(async () => {
      vi.advanceTimersByTime(500)
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(mocks.getFileStreamURL).toHaveBeenCalledWith(5)
    expect(view.container.querySelector('video')?.getAttribute('src')).toBe(
      '/stream/5?ticket=signed',
    )
  })
})

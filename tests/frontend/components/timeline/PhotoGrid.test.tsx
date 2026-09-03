import { act, fireEvent, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Media } from '../../../../src/frontend/api/types'

const mocks = vi.hoisted(() => ({
  getFileStreamURL: vi.fn(),
  loadThumbnail: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: {
    getFileStreamURL: mocks.getFileStreamURL,
  },
}))

vi.mock('../../../../src/frontend/utils/assetUrlLoader', () => ({
  thumbnailUrlLoader: { load: mocks.loadThumbnail },
}))

import PhotoGrid from '../../../../src/frontend/components/timeline/PhotoGrid'

class VisibleIntersectionObserver implements IntersectionObserver {
  readonly root = null
  readonly rootMargin = '0px'
  readonly thresholds = [0]

  constructor(callback: IntersectionObserverCallback) {
    queueMicrotask(() => callback([{ isIntersecting: true } as IntersectionObserverEntry], this))
  }

  disconnect(): void {}
  observe(): void {}
  takeRecords(): IntersectionObserverEntry[] {
    return []
  }
  unobserve(): void {}
}

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
    vi.stubGlobal('IntersectionObserver', VisibleIntersectionObserver)
    mocks.getFileStreamURL.mockReset()
    mocks.loadThumbnail.mockReset().mockResolvedValue('/api/v1/media/5/thumbnail')
    mocks.getFileStreamURL.mockResolvedValue('/stream/5?ticket=signed')
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('requests a stream ticket only after the hover preview delay', async () => {
    const view = render(<PhotoGrid media={[video]} onPhotoClick={vi.fn()} selection={null} />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
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
      '/stream/5?ticket=signed'
    )
  })

  it('toggles media instead of opening it while selection is active', () => {
    const openMedia = vi.fn()
    const toggleSelection = vi.fn()
    const view = render(
      <PhotoGrid
        media={[video]}
        onPhotoClick={openMedia}
        selection={{ selectedMediaIds: new Set([5]), toggleSelection }}
      />
    )

    fireEvent.click(view.getByRole('button', { name: 'Deselect preview.mp4' }))

    expect(toggleSelection).toHaveBeenCalledWith(5)
    expect(openMedia).not.toHaveBeenCalled()
  })
})

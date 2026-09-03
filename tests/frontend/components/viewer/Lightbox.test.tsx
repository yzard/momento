import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getBatch: vi.fn(),
  getPreviewURL: vi.fn(),
  getFileStreamURL: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: {
    getBatch: mocks.getBatch,
    getPreviewURL: mocks.getPreviewURL,
    getFileStreamURL: mocks.getFileStreamURL,
  },
}))

vi.mock('../../../../src/frontend/components/viewer/MediaDetails', () => ({
  MediaDetails: () => null,
}))

import Lightbox from '../../../../src/frontend/components/viewer/Lightbox'

describe('Lightbox', () => {
  beforeEach(() => {
    mocks.getBatch.mockReset()
    mocks.getPreviewURL.mockReset()
    mocks.getPreviewURL.mockImplementation((id: number) => `/api/v1/media/${id}/preview`)
    mocks.getFileStreamURL.mockReset()
    mocks.getFileStreamURL.mockImplementation(async (id: number) => `/stream/${id}`)
    mocks.getBatch.mockResolvedValue([
      { id: 1, mediaType: 'image', originalFilename: 'first.jpg' },
      { id: 2, mediaType: 'image', originalFilename: 'second.jpg' },
    ])
  })

  afterEach(cleanup)

  it('updates the binary preview URL when the selected media changes', async () => {
    const view = render(
      <MemoryRouter>
        <Lightbox mediaIds={[1, 2]} currentIndex={0} onClose={vi.fn()} onIndexChange={vi.fn()} />
      </MemoryRouter>
    )
    expect((await screen.findByRole('img', { name: 'first.jpg' })).getAttribute('src')).toBe(
      '/api/v1/media/1/preview'
    )

    view.rerender(
      <MemoryRouter>
        <Lightbox mediaIds={[1, 2]} currentIndex={1} onClose={vi.fn()} onIndexChange={vi.fn()} />
      </MemoryRouter>
    )
    expect((await screen.findByRole('img', { name: 'second.jpg' })).getAttribute('src')).toBe(
      '/api/v1/media/2/preview'
    )
  })

  it('loads video through an asynchronous media access ticket', async () => {
    mocks.getBatch.mockResolvedValueOnce([
      { id: 7, mediaType: 'video', originalFilename: 'video.mp4' },
    ])

    const view = render(
      <MemoryRouter>
        <Lightbox mediaIds={[7]} currentIndex={0} onClose={vi.fn()} onIndexChange={vi.fn()} />
      </MemoryRouter>
    )

    await waitFor(() => expect(mocks.getFileStreamURL).toHaveBeenCalledWith(7))
    await waitFor(() =>
      expect(view.container.querySelector('video')?.getAttribute('src')).toBe('/stream/7')
    )
  })

  it('restores video position after refreshing a failed stream ticket', async () => {
    mocks.getBatch.mockResolvedValueOnce([
      { id: 8, mediaType: 'video', originalFilename: 'long-video.mp4' },
    ])
    mocks.getFileStreamURL
      .mockResolvedValueOnce('/stream/8/first')
      .mockResolvedValueOnce('/stream/8/refreshed')
    const view = render(
      <MemoryRouter>
        <Lightbox mediaIds={[8]} currentIndex={0} onClose={vi.fn()} onIndexChange={vi.fn()} />
      </MemoryRouter>
    )
    await waitFor(() =>
      expect(view.container.querySelector('video')?.getAttribute('src')).toBe('/stream/8/first')
    )
    const firstVideo = view.container.querySelector('video') as HTMLVideoElement
    firstVideo.currentTime = 321
    fireEvent.error(firstVideo)
    await waitFor(() =>
      expect(view.container.querySelector('video')?.getAttribute('src')).toBe('/stream/8/refreshed')
    )
    const refreshedVideo = view.container.querySelector('video') as HTMLVideoElement
    fireEvent.loadedMetadata(refreshedVideo)

    expect(refreshedVideo.currentTime).toBe(321)
    expect(mocks.getFileStreamURL).toHaveBeenCalledTimes(2)
  })
})

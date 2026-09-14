import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserRouter, MemoryRouter, useLocation } from 'react-router-dom'
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
    getThumbnailURL: (id: number) => `/thumbnail/${id}`,
  },
}))

vi.mock('../../../../src/frontend/components/viewer/MediaDetails', () => ({
  MediaDetails: () => null,
}))

import Lightbox from '../../../../src/frontend/components/viewer/Lightbox'
import ManagedLightbox from '../../../../src/frontend/components/viewer/ManagedLightbox'
import { useCollectionLightbox } from '../../../../src/frontend/hooks/useCollectionLightbox'

function CollectionViewerHarness() {
  const controller = useCollectionLightbox()
  const location = useLocation()
  return (
    <>
      <div data-testid="route">{location.pathname}</div>
      <button onClick={() => controller.open(1, [1, 2])}>Open viewer</button>
      <ManagedLightbox controller={controller} />
    </>
  )
}

describe('Lightbox', () => {
  const photo = (id: number) => ({
    id,
    mediaType: 'image',
    originalFilename: `${id}.jpg`,
  })
  function viewer(ids: number[], index: number, onIndexChange = vi.fn()) {
    return (
      <MemoryRouter>
        <Lightbox
          manageHistory={false}
          mediaIds={ids}
          currentIndex={index}
          onClose={vi.fn()}
          onIndexChange={onIndexChange}
        />
      </MemoryRouter>
    )
  }

  it('loads only the visible item in a large group and preserves the global position', async () => {
    mocks.getBatch.mockImplementation(async (ids: number[]) => ids.map(photo))
    const ids = Array.from({ length: 1500 }, (_, i) => i + 1)
    const change = vi.fn()
    const view = render(viewer(ids, 900, change))
    await screen.findByRole('img', { name: '901.jpg' })
    expect(mocks.getBatch).toHaveBeenCalledWith([901], expect.any(AbortSignal))
    expect(screen.getByText('901 / 1500')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Next media' }))
    expect(change).toHaveBeenCalledWith(901)
    view.rerender(viewer(ids, 901, change))
    await screen.findByRole('img', { name: '902.jpg' })
    view.rerender(viewer([...ids], 1499, change))
    await screen.findByRole('img', { name: '1500.jpg' })
    expect(screen.queryByRole('button', { name: 'Next media' })).toBeNull()
    expect(mocks.getBatch.mock.calls.map(([batch]) => batch)).toEqual([[901], [902], [1500]])
    view.rerender(viewer([...ids], 1499, change))
    expect(mocks.getBatch).toHaveBeenCalledTimes(3)
  })

  it('cancels obsolete loads and ignores late responses after navigation and close', async () => {
    let finish!: (items: ReturnType<typeof photo>[]) => void
    mocks.getBatch
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve
          })
      )
      .mockImplementation(async (ids: number[]) => ids.map(photo))
    const view = render(viewer([1, 2], 0))
    const signal = mocks.getBatch.mock.calls[0][1] as AbortSignal
    view.rerender(viewer([1, 2], 1))
    await screen.findByRole('img', { name: '2.jpg' })
    expect(signal.aborted).toBe(true)
    finish([photo(1)])
    await waitFor(() => expect(screen.queryByRole('img', { name: '1.jpg' })).toBeNull())
    view.unmount()
    expect((mocks.getBatch.mock.calls[1][1] as AbortSignal).aborted).toBe(true)
  })

  it('keeps navigation and retry available for missing or failed media', async () => {
    mocks.getBatch
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce([photo(2)])
    const view = render(viewer([1, 2], 0))
    await screen.findByRole('alert')
    expect(screen.getByRole('button', { name: 'Close viewer' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Next media' })).toBeTruthy()
    view.rerender(viewer([1, 2], 1))
    await screen.findByRole('alert')
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    await screen.findByRole('img', { name: '2.jpg' })
    expect(screen.getByText('2 / 2')).toBeTruthy()
  })

  beforeEach(() => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        observe() {}
        disconnect() {}
      }
    )
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

  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it.each(['/faces/5', '/places/7'])(
    'closes %s viewer with X and browser back one layer at a time',
    async (path) => {
      window.history.replaceState({}, '', path)
      render(
        <BrowserRouter>
          <CollectionViewerHarness />
        </BrowserRouter>
      )
      const pushState = vi.spyOn(window.history, 'pushState')
      fireEvent.click(screen.getByRole('button', { name: 'Open viewer' }))
      const close = await screen.findByRole('button', { name: 'Close viewer' })
      expect(pushState).toHaveBeenCalledTimes(1)
      fireEvent.click(close)
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Close viewer' })).toBeNull())
      expect(screen.getByTestId('route').textContent).toBe(path)
      fireEvent.click(screen.getByRole('button', { name: 'Open viewer' }))
      await screen.findByRole('button', { name: 'Close viewer' })
      window.history.back()
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Close viewer' })).toBeNull())
      expect(screen.getByTestId('route').textContent).toBe(path)
      window.history.forward()
      fireEvent.click(await screen.findByRole('button', { name: 'Close viewer' }))
      await waitFor(() => expect(screen.queryByRole('button', { name: 'Close viewer' })).toBeNull())
    }
  )

  it.each(['photo.nef', 'photo.heic', 'video.mp4'])(
    'downloads original bytes for %s',
    async (filename) => {
      mocks.getBatch.mockResolvedValueOnce([
        {
          id: 1,
          mediaType: filename.endsWith('mp4') ? 'video' : 'image',
          originalFilename: filename,
        },
      ])
      const clicked: HTMLAnchorElement[] = []
      vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
        this: HTMLAnchorElement
      ) {
        clicked.push(this)
      })
      render(
        <MemoryRouter>
          <Lightbox
            manageHistory={true}
            mediaIds={[1]}
            currentIndex={0}
            onClose={vi.fn()}
            onIndexChange={vi.fn()}
          />
        </MemoryRouter>
      )
      fireEvent.click(await screen.findByRole('button', { name: 'Download original' }))
      await waitFor(() => expect(clicked).toHaveLength(1))
      expect(clicked[0].getAttribute('href')).toBe('/stream/1')
      expect(clicked[0].download).toBe(filename)
      expect(mocks.getFileStreamURL).toHaveBeenCalledWith(1, 'original')
      expect(document.body.contains(clicked[0])).toBe(false)
    }
  )

  it('shows download errors and permits retry', async () => {
    mocks.getFileStreamURL.mockRejectedValueOnce(new Error('unavailable'))
    render(
      <MemoryRouter>
        <Lightbox
          manageHistory={true}
          mediaIds={[1]}
          currentIndex={0}
          onClose={vi.fn()}
          onIndexChange={vi.fn()}
        />
      </MemoryRouter>
    )
    fireEvent.click(await screen.findByRole('button', { name: 'Download original' }))
    expect((await screen.findByRole('alert')).textContent).toContain('Unable to download original')
    expect(screen.getByRole('button', { name: 'Download original' }).hasAttribute('disabled')).toBe(
      false
    )
  })

  it('updates the binary preview URL when the selected media changes', async () => {
    const view = render(
      <MemoryRouter>
        <Lightbox
          manageHistory={true}
          mediaIds={[1, 2]}
          currentIndex={0}
          onClose={vi.fn()}
          onIndexChange={vi.fn()}
        />
      </MemoryRouter>
    )
    expect((await screen.findByRole('img', { name: 'first.jpg' })).getAttribute('src')).toBe(
      '/api/v1/media/1/preview'
    )

    view.rerender(
      <MemoryRouter>
        <Lightbox
          manageHistory={true}
          mediaIds={[1, 2]}
          currentIndex={1}
          onClose={vi.fn()}
          onIndexChange={vi.fn()}
        />
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
        <Lightbox
          manageHistory={true}
          mediaIds={[7]}
          currentIndex={0}
          onClose={vi.fn()}
          onIndexChange={vi.fn()}
        />
      </MemoryRouter>
    )

    await waitFor(() => expect(mocks.getFileStreamURL).toHaveBeenCalledWith(7, 'preview'))
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
        <Lightbox
          manageHistory={true}
          mediaIds={[8]}
          currentIndex={0}
          onClose={vi.fn()}
          onIndexChange={vi.fn()}
        />
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

import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  batchLoad: vi.fn(),
  getCachedThumbnailURL: vi.fn(),
  reorder: vi.fn(),
  removeMedia: vi.fn(),
  album: {
    id: 7,
    name: 'Trip',
    description: null,
    coverMediaId: 42,
    createdAt: '2026-01-01',
    media: [
      { id: 42, originalFilename: 'first.jpg' },
      { id: 43, originalFilename: 'second.jpg' },
      { id: 44, originalFilename: 'third.jpg' },
    ],
  },
}))

vi.mock('../../../../src/frontend/utils/batcher', () => ({
  batchLoader: { load: mocks.batchLoad },
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: { getCachedThumbnailURL: mocks.getCachedThumbnailURL },
}))

vi.mock('../../../../src/frontend/hooks/useAlbums', () => ({
  useAlbum: () => ({
    data: mocks.album,
    isLoading: false,
    error: null,
  }),
  useReorderAlbum: () => ({ mutateAsync: mocks.reorder, isPending: false }),
  useRemoveAlbumMedia: () => ({
    mutate: (
      variables: { albumId: number; mediaIds: number[] },
      callbacks: { onSuccess: () => void }
    ) => {
      mocks.removeMedia(variables)
      callbacks.onSuccess()
    },
    isPending: false,
  }),
}))

import AlbumView from '../../../../src/frontend/components/albums/AlbumView'

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

describe('AlbumView', () => {
  beforeEach(() => {
    vi.stubGlobal('IntersectionObserver', VisibleIntersectionObserver)
    mocks.getCachedThumbnailURL.mockReturnValue(null)
    mocks.batchLoad.mockResolvedValue('batched-thumbnail')
    mocks.reorder.mockResolvedValue(undefined)
    mocks.removeMedia.mockResolvedValue(undefined)
  })

  it('selects multiple media and removes only those items from the album', async () => {
    render(<AlbumView albumId={7} onBack={vi.fn()} onPhotoClick={vi.fn()} />)

    fireEvent.click(screen.getByRole('button', { name: 'Select' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Select first.jpg' }))
    fireEvent.click(screen.getByRole('button', { name: 'Select third.jpg' }))
    fireEvent.click(screen.getByRole('button', { name: 'Remove from album' }))
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'Remove from album',
      })
    )

    await waitFor(() =>
      expect(mocks.removeMedia).toHaveBeenCalledWith({
        albumId: 7,
        mediaIds: [42, 44],
      })
    )
    expect(screen.queryByRole('img', { name: 'first.jpg' })).toBeNull()
    expect(screen.getByRole('img', { name: 'second.jpg' })).toBeTruthy()
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    vi.clearAllMocks()
  })

  it('loads visible album media through the shared thumbnail batcher', async () => {
    render(<AlbumView albumId={7} onBack={vi.fn()} onPhotoClick={vi.fn()} />)

    await waitFor(() => expect(mocks.batchLoad).toHaveBeenCalledWith(42))
    expect((await screen.findByRole('img', { name: 'first.jpg' })).getAttribute('src')).toBe(
      'batched-thumbnail'
    )
  })

  it('submits the complete latest order after a drag', async () => {
    render(<AlbumView albumId={7} onBack={vi.fn()} onPhotoClick={vi.fn()} />)
    const first = (await screen.findByRole('img', { name: 'first.jpg' })).parentElement
    const third = (await screen.findByRole('img', { name: 'third.jpg' })).parentElement
    if (!first || !third) throw new Error('album media containers are missing')

    fireEvent.dragStart(first, { dataTransfer: { effectAllowed: 'move' } })
    fireEvent.dragOver(third)
    fireEvent.drop(third)

    await waitFor(() =>
      expect(mocks.reorder).toHaveBeenCalledWith({
        albumId: 7,
        mediaIds: [43, 44, 42],
      })
    )
  })

  it('rolls back the local order when persistence fails', async () => {
    mocks.reorder.mockRejectedValue(new Error('failed'))
    render(<AlbumView albumId={7} onBack={vi.fn()} onPhotoClick={vi.fn()} />)
    const first = (await screen.findByRole('img', { name: 'first.jpg' })).parentElement
    const third = (await screen.findByRole('img', { name: 'third.jpg' })).parentElement
    if (!first || !third) throw new Error('album media containers are missing')

    fireEvent.dragStart(first, { dataTransfer: { effectAllowed: 'move' } })
    fireEvent.dragOver(third)
    fireEvent.drop(third)

    expect((await screen.findByRole('alert')).textContent).toBe('Could not save the album order.')
    expect(screen.getAllByRole('img').map((image) => image.getAttribute('alt'))).toEqual([
      'first.jpg',
      'second.jpg',
      'third.jpg',
    ])
  })
})

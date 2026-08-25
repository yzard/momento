import { cleanup, render, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getThumbnailBatch: vi.fn(),
  albumCard: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: { getThumbnailBatch: mocks.getThumbnailBatch },
}))

vi.mock('../../../../src/frontend/hooks/useAlbums', () => ({
  useAlbums: () => ({
    data: [
      { id: 1, name: 'First', description: null, coverMediaId: 10, mediaCount: 1, createdAt: '2026-01-01' },
      { id: 2, name: 'Second', description: null, coverMediaId: 20, mediaCount: 1, createdAt: '2026-01-02' },
      { id: 3, name: 'Empty', description: null, coverMediaId: null, mediaCount: 0, createdAt: '2026-01-03' },
    ],
    isLoading: false,
    error: null,
  }),
  useCreateAlbum: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useDeleteAlbum: () => ({ mutateAsync: vi.fn() }),
}))

vi.mock('../../../../src/frontend/components/albums/AlbumCard', () => ({
  default: (props: unknown) => {
    mocks.albumCard(props)
    return null
  },
}))

import AlbumList from '../../../../src/frontend/components/albums/AlbumList'

describe('AlbumList', () => {
  beforeEach(() => {
    mocks.getThumbnailBatch.mockReset()
    mocks.albumCard.mockReset()
    mocks.getThumbnailBatch.mockResolvedValue(new Map([
      [10, 'first-cover'],
      [20, 'second-cover'],
    ]))
  })

  afterEach(cleanup)

  it('loads all album covers in one batch and passes them to cards', async () => {
    render(<AlbumList onAlbumClick={vi.fn()} />)

    await waitFor(() => expect(mocks.getThumbnailBatch).toHaveBeenCalledOnce())
    expect(mocks.getThumbnailBatch).toHaveBeenCalledWith([10, 20])
    await waitFor(() => {
      expect(mocks.albumCard).toHaveBeenCalledWith(expect.objectContaining({
        album: expect.objectContaining({ id: 1 }),
        coverUrl: 'first-cover',
      }))
      expect(mocks.albumCard).toHaveBeenCalledWith(expect.objectContaining({
        album: expect.objectContaining({ id: 2 }),
        coverUrl: 'second-cover',
      }))
    })
  })
})

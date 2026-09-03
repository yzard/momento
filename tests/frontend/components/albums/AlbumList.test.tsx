import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getThumbnailURL: vi.fn(),
  albumCard: vi.fn(),
  deleteAlbum: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: { getThumbnailURL: mocks.getThumbnailURL },
}))

vi.mock('../../../../src/frontend/hooks/useAlbums', () => ({
  useAlbums: () => ({
    data: [
      {
        id: 1,
        name: 'First',
        description: null,
        coverMediaId: 10,
        thumbnailMediaIds: [10, 11, 12, 13],
        mediaCount: 4,
        createdAt: '2026-01-01',
      },
      {
        id: 2,
        name: 'Second',
        description: null,
        coverMediaId: 20,
        thumbnailMediaIds: [20, 11],
        mediaCount: 2,
        createdAt: '2026-01-02',
      },
      {
        id: 3,
        name: 'Empty',
        description: null,
        coverMediaId: null,
        thumbnailMediaIds: [],
        mediaCount: 0,
        createdAt: '2026-01-03',
      },
    ],
    isLoading: false,
    error: null,
  }),
  useCreateAlbum: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useDeleteAlbum: () => ({ mutateAsync: mocks.deleteAlbum, isPending: false }),
}))

vi.mock('../../../../src/frontend/components/albums/AlbumCard', () => ({
  default: (props: { album: { name: string }; onDelete: () => void }) => {
    mocks.albumCard(props)
    return (
      <button type="button" onClick={props.onDelete}>
        Delete {props.album.name}
      </button>
    )
  },
}))

import AlbumList from '../../../../src/frontend/components/albums/AlbumList'

describe('AlbumList', () => {
  beforeEach(() => {
    mocks.getThumbnailURL.mockReset()
    mocks.albumCard.mockReset()
    mocks.deleteAlbum.mockReset()
    mocks.deleteAlbum.mockResolvedValue(undefined)
    mocks.getThumbnailURL.mockImplementation((mediaId: number) => `${mediaId}-cover`)
  })

  afterEach(cleanup)

  it('passes direct binary cover URLs to cards', async () => {
    render(<AlbumList onAlbumClick={vi.fn()} />)

    expect(screen.getByRole('heading', { name: 'Albums' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Create Album' })).toBeTruthy()
    expect(mocks.getThumbnailURL).toHaveBeenCalledWith(10, 'normal')
    expect(mocks.getThumbnailURL).toHaveBeenCalledWith(20, 'normal')
    await waitFor(() => {
      expect(mocks.albumCard).toHaveBeenCalledWith(
        expect.objectContaining({
          album: expect.objectContaining({ id: 1 }),
          thumbnailUrls: ['10-cover', '11-cover', '12-cover', '13-cover'],
        })
      )
      expect(mocks.albumCard).toHaveBeenCalledWith(
        expect.objectContaining({
          album: expect.objectContaining({ id: 2 }),
          thumbnailUrls: ['20-cover', '11-cover'],
        })
      )
    })
  })

  it('requires explicit confirmation before deleting an album', async () => {
    const user = userEvent.setup()
    render(<AlbumList onAlbumClick={vi.fn()} />)

    await user.click(await screen.findByRole('button', { name: 'Delete First' }))
    expect(mocks.deleteAlbum).not.toHaveBeenCalled()
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'Delete album',
      })
    )

    expect(mocks.deleteAlbum).toHaveBeenCalledWith(1)
  })
})

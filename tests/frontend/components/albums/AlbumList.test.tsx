import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getThumbnailBatch: vi.fn(),
  albumCard: vi.fn(),
  deleteAlbum: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: { getThumbnailBatch: mocks.getThumbnailBatch },
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
    mocks.getThumbnailBatch.mockReset()
    mocks.albumCard.mockReset()
    mocks.deleteAlbum.mockReset()
    mocks.deleteAlbum.mockResolvedValue(undefined)
    mocks.getThumbnailBatch.mockResolvedValue(
      new Map([
        [10, 'first-cover'],
        [11, 'shared-cover'],
        [12, 'third-cover'],
        [13, 'fourth-cover'],
        [20, 'second-cover'],
      ])
    )
  })

  afterEach(cleanup)

  it('loads all album covers in one batch and passes them to cards', async () => {
    render(<AlbumList onAlbumClick={vi.fn()} />)

    await waitFor(() => expect(mocks.getThumbnailBatch).toHaveBeenCalledOnce())
    expect(mocks.getThumbnailBatch).toHaveBeenCalledWith([10, 11, 12, 13, 20], 'normal')
    await waitFor(() => {
      expect(mocks.albumCard).toHaveBeenCalledWith(
        expect.objectContaining({
          album: expect.objectContaining({ id: 1 }),
          thumbnailUrls: ['first-cover', 'shared-cover', 'third-cover', 'fourth-cover'],
        })
      )
      expect(mocks.albumCard).toHaveBeenCalledWith(
        expect.objectContaining({
          album: expect.objectContaining({ id: 2 }),
          thumbnailUrls: ['second-cover', 'shared-cover'],
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

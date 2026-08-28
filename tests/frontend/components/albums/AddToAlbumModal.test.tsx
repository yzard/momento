import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  addMedia: vi.fn(),
  create: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/albums', () => ({
  albumsApi: mocks,
}))

import AddToAlbumModal from '../../../../src/frontend/components/albums/AddToAlbumModal'

describe('AddToAlbumModal', () => {
  beforeEach(() => {
    mocks.list.mockReset()
    mocks.addMedia.mockReset()
    mocks.create.mockReset()
  })

  afterEach(cleanup)

  it('adds every selected media identifier to the chosen album', async () => {
    mocks.list.mockResolvedValue([{ id: 7, name: 'Trip', mediaCount: 2 }])
    mocks.addMedia.mockResolvedValue(undefined)
    const close = vi.fn()
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })

    render(
      <QueryClientProvider client={queryClient}>
        <AddToAlbumModal mediaIds={[10, 11, 12]} onClose={close} />
      </QueryClientProvider>
    )

    expect(screen.getByText('3 selected items')).toBeTruthy()
    fireEvent.click(await screen.findByRole('button', { name: /Trip/ }))

    await waitFor(() => expect(mocks.addMedia).toHaveBeenCalledWith(7, [10, 11, 12]))
    expect(close).toHaveBeenCalledOnce()
  })

  it('creates a new album with the selected media in one request', async () => {
    mocks.list.mockResolvedValue([])
    mocks.create.mockResolvedValue({ id: 9, name: 'New trip', media: [] })
    const close = vi.fn()
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })

    render(
      <QueryClientProvider client={queryClient}>
        <AddToAlbumModal mediaIds={[10, 11]} onClose={close} />
      </QueryClientProvider>
    )

    fireEvent.click(await screen.findByRole('button', { name: 'Create new album' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Album name' }), {
      target: { value: '  New trip  ' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Create & Add' }))

    await waitFor(() =>
      expect(mocks.create).toHaveBeenCalledWith({
        name: 'New trip',
        mediaIds: [10, 11],
      })
    )
    expect(mocks.addMedia).not.toHaveBeenCalled()
    expect(close).toHaveBeenCalledOnce()
  })
})

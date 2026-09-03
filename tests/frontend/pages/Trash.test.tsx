import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  restore: vi.fn(),
  permanentlyDelete: vi.fn(),
  emptyTrash: vi.fn(),
  loadThumbnail: vi.fn(),
}))

vi.mock('../../../src/frontend/api/trash', () => ({
  trashApi: {
    list: mocks.list,
    restore: mocks.restore,
    permanentlyDelete: mocks.permanentlyDelete,
    emptyTrash: mocks.emptyTrash,
  },
}))

vi.mock('../../../src/frontend/utils/assetUrlLoader', () => ({
  trashThumbnailUrlLoader: { load: mocks.loadThumbnail },
}))

import Trash from '../../../src/frontend/pages/Trash'

function renderTrash() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <Trash />
    </QueryClientProvider>
  )
}

describe('Trash page', () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset()
    mocks.loadThumbnail.mockResolvedValue(null)
    mocks.list.mockResolvedValue({
      items: [
        {
          id: 4,
          filename: 'trash.jpg',
          originalFilename: 'trash.jpg',
          mediaType: 'image',
          mimeType: 'image/jpeg',
          width: 100,
          height: 100,
          fileSize: 100,
          durationSeconds: null,
          dateTaken: null,
          deletedAt: new Date().toISOString(),
          createdAt: new Date().toISOString(),
        },
      ],
      totalCount: 1,
    })
    class MockIntersectionObserver {
      constructor(_callback: IntersectionObserverCallback) {}
      observe() {}
      disconnect() {}
      unobserve() {}
      takeRecords(): IntersectionObserverEntry[] {
        return []
      }
      readonly root = null
      readonly rootMargin = '0px'
      readonly thresholds = [0]
    }
    vi.stubGlobal('IntersectionObserver', MockIntersectionObserver)
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('requires explicit confirmation before permanently deleting selected media', async () => {
    const user = userEvent.setup()
    mocks.permanentlyDelete.mockResolvedValue({
      message: 'deleted',
      affectedCount: 1,
    })
    renderTrash()

    const heading = screen.getByRole('heading', { name: 'Trash' })
    expect(heading.closest('[data-page-frame="true"]')?.className).toContain('w-full')
    await user.click(await screen.findByText('trash.jpg'))
    await user.click(screen.getByRole('button', { name: 'Delete Forever' }))
    expect(mocks.permanentlyDelete).not.toHaveBeenCalled()

    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'Delete forever',
      })
    )
    await waitFor(() =>
      expect(mocks.permanentlyDelete).toHaveBeenCalledWith([4], expect.anything())
    )
  })

  it('requires explicit confirmation before emptying all Trash', async () => {
    const user = userEvent.setup()
    mocks.emptyTrash.mockResolvedValue({
      message: 'deleted',
      affectedCount: 1,
    })
    renderTrash()

    await user.click(await screen.findByRole('button', { name: 'Empty Trash' }))
    expect(mocks.emptyTrash).not.toHaveBeenCalled()

    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', {
        name: 'Empty Trash',
      })
    )
    await waitFor(() => expect(mocks.emptyTrash).toHaveBeenCalledOnce())
  })
})

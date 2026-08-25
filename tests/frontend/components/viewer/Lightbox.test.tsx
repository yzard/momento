import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  getBatch: vi.fn(),
  getPreviewBatch: vi.fn(),
}))

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: {
    getBatch: mocks.getBatch,
    getPreviewBatch: mocks.getPreviewBatch,
    getFileStreamUrl: (id: number) => `/stream/${id}`,
  },
}))

vi.mock('../../../../src/frontend/components/viewer/MediaDetails', () => ({
  MediaDetails: () => null,
}))

import Lightbox from '../../../../src/frontend/components/viewer/Lightbox'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

describe('Lightbox', () => {
  beforeEach(() => {
    mocks.getBatch.mockReset()
    mocks.getPreviewBatch.mockReset()
    mocks.getBatch.mockResolvedValue([
      { id: 1, mediaType: 'image', originalFilename: 'first.jpg' },
      { id: 2, mediaType: 'image', originalFilename: 'second.jpg' },
    ])
  })

  afterEach(cleanup)

  it('does not let an old preview overwrite the newly selected media', async () => {
    const first = deferred<Map<number, string | null>>()
    const second = deferred<Map<number, string | null>>()
    mocks.getPreviewBatch.mockImplementation(([id]: number[]) => (
      id === 1 ? first.promise : second.promise
    ))
    const view = render(
      <MemoryRouter>
        <Lightbox mediaIds={[1, 2]} currentIndex={0} onClose={vi.fn()} onIndexChange={vi.fn()} />
      </MemoryRouter>,
    )
    await waitFor(() => expect(mocks.getPreviewBatch).toHaveBeenCalledWith([1]))

    view.rerender(
      <MemoryRouter>
        <Lightbox mediaIds={[1, 2]} currentIndex={1} onClose={vi.fn()} onIndexChange={vi.fn()} />
      </MemoryRouter>,
    )
    await waitFor(() => expect(mocks.getPreviewBatch).toHaveBeenCalledWith([2]))
    await act(async () => second.resolve(new Map([[2, 'second-preview']])) )
    expect((await screen.findByRole('img', { name: 'second.jpg' })).getAttribute('src')).toBe('second-preview')

    await act(async () => first.resolve(new Map([[1, 'first-preview']])) )
    expect(screen.getByRole('img', { name: 'second.jpg' }).getAttribute('src')).toBe('second-preview')
  })
})

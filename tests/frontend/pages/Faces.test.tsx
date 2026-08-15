import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from '../../../src/frontend/node_modules/react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({ listGroups: vi.fn(), getGroup: vi.fn(), getThumbnailUrl: vi.fn(), mergeGroups: vi.fn(), role: 'user' as 'admin' | 'user', lightbox: vi.fn() }))

vi.mock('../../../src/frontend/api/faces', () => ({ facesApi: { listGroups: mocks.listGroups, getGroup: mocks.getGroup, getThumbnailUrl: mocks.getThumbnailUrl, mergeGroups: mocks.mergeGroups } }))
vi.mock('../../../src/frontend/hooks/useAuth', () => ({ useAuth: () => ({ user: { id: 1, role: mocks.role } }) }))
vi.mock('../../../src/frontend/components/timeline/PhotoGrid', () => ({ default: ({ media, onPhotoClick }: { media: Array<{ id: number }>; onPhotoClick: (media: { id: number }) => void }) => <button type="button" onClick={() => onPhotoClick(media[1])}>Open second media</button> }))
vi.mock('../../../src/frontend/components/viewer/Lightbox', () => ({ default: (props: { mediaIds: number[]; currentIndex: number }) => { mocks.lightbox(props); return <div>Lightbox</div> } }))

import Faces from '../../../src/frontend/pages/Faces'

function renderFaces(path = '/faces') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(<QueryClientProvider client={queryClient}><MemoryRouter initialEntries={[path]}><Routes><Route path="/faces" element={<Faces />} /><Route path="/faces/:faceGroupId" element={<Faces />} /></Routes></MemoryRouter></QueryClientProvider>)
}

describe('Faces page', () => {
  let intersectionCallback: IntersectionObserverCallback | undefined

  beforeEach(() => {
    mocks.role = 'user'
    mocks.listGroups.mockReset()
    mocks.getGroup.mockReset()
    mocks.getThumbnailUrl.mockReset()
    mocks.mergeGroups.mockReset()
    mocks.lightbox.mockReset()
    mocks.getThumbnailUrl.mockResolvedValue('blob:face')
    mocks.listGroups.mockResolvedValue({ groups: [{ faceGroupId: 5, faceCount: 4, mediaCount: 3 }, { faceGroupId: 8, faceCount: 2, mediaCount: 2 }], nextCursor: null, hasMore: false })
    intersectionCallback = undefined
    vi.stubGlobal('IntersectionObserver', class {
      constructor(callback: IntersectionObserverCallback) {
        intersectionCallback = callback
      }

      observe() {}
      disconnect() {}
    })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('shows responsive face group cards with representative counts', async () => {
    renderFaces()

    expect(await screen.findByRole('link', { name: 'Face group 5, 3 media' })).toBeTruthy()
    expect(screen.getByRole('link', { name: 'Face group 8, 2 media' })).toBeTruthy()
    expect(screen.queryByText('Face group 5')).toBeNull()
    expect(screen.getByText('3')).toBeTruthy()
    expect(screen.getByText('2')).toBeTruthy()
    expect(mocks.getThumbnailUrl).toHaveBeenCalledWith({ faceGroupId: 5 })
  })

  it('lets administrators select and merge two groups', async () => {
    mocks.role = 'admin'
    mocks.mergeGroups.mockResolvedValue({ group: { faceGroupId: 5, faceCount: 6, mediaCount: 5 } })
    renderFaces()
    const user = userEvent.setup()

    await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    await user.click(screen.getByRole('button', { name: 'Select face group 5' }))
    await user.click(screen.getByRole('button', { name: 'Select face group 8' }))
    expect(screen.getByText('curated group', { exact: false })).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Merge groups' }))

    await waitFor(() => expect(mocks.mergeGroups).toHaveBeenCalledWith({ faceGroupIds: [5, 8] }, expect.anything()))
  })

  it('loads the next face-group page when the scroll sentinel approaches', async () => {
    mocks.listGroups
      .mockResolvedValueOnce({ groups: [{ faceGroupId: 5, faceCount: 4, mediaCount: 3 }], nextCursor: '100', hasMore: true })
      .mockResolvedValueOnce({ groups: [{ faceGroupId: 105, faceCount: 2, mediaCount: 2 }], nextCursor: null, hasMore: false })
    renderFaces()
    await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    await waitFor(() => expect(intersectionCallback).toBeDefined())

    act(() => {
      intersectionCallback?.([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver)
    })

    expect(await screen.findByRole('link', { name: 'Face group 105, 2 media' })).toBeTruthy()
    expect(mocks.listGroups).toHaveBeenNthCalledWith(2, { cursor: '100', limit: 100 })
  })

  it('shows associated media and opens it in the existing lightbox', async () => {
    mocks.getGroup.mockResolvedValue({ group: { faceGroupId: 5, faceCount: 2, mediaCount: 2 }, media: [{ id: 10 }, { id: 11 }] })
    renderFaces('/faces/5')

    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: 'Open second media' }))

    expect(mocks.getGroup).toHaveBeenCalledWith({ faceGroupId: 5 })
    expect(mocks.lightbox).toHaveBeenCalledWith(expect.objectContaining({ mediaIds: [10, 11], currentIndex: 1 }))
  })
})

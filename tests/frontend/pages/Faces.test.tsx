import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from '../../../src/frontend/node_modules/react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  listGroups: vi.fn(),
  getGroup: vi.fn(),
  getThumbnailURL: vi.fn(),
  mergeGroups: vi.fn(),
  reject: vi.fn(),
  role: 'user' as 'admin' | 'user',
  lightbox: vi.fn(),
}))

vi.mock('../../../src/frontend/api/faces', () => ({
  facesApi: {
    listGroups: mocks.listGroups,
    getGroup: mocks.getGroup,
    getThumbnailURL: mocks.getThumbnailURL,
    mergeGroups: mocks.mergeGroups,
    reject: mocks.reject,
  },
}))
vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { id: 1, role: mocks.role } }),
}))
vi.mock('../../../src/frontend/components/timeline/PhotoGrid', () => ({
  default: ({
    media,
    onPhotoClick,
    selection,
  }: {
    media: Array<{ id: number }>
    onPhotoClick: (media: { id: number }) => void
    selection: { toggleSelection: (id: number) => void } | null
  }) => (
    <>
      <button
        type="button"
        onClick={() =>
          selection ? selection.toggleSelection(media[1].id) : onPhotoClick(media[1])
        }
      >
        Open second media
      </button>
      {selection &&
        media.map((item) => (
          <button key={item.id} onClick={() => selection.toggleSelection(item.id)}>
            Select media {item.id}
          </button>
        ))}
    </>
  ),
}))
vi.mock('../../../src/frontend/components/viewer/Lightbox', () => ({
  default: (props: { mediaIds: number[]; currentIndex: number }) => {
    mocks.lightbox(props)
    return <div>Lightbox</div>
  },
}))

import Faces from '../../../src/frontend/pages/Faces'

function renderFaces(path = '/faces') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/faces/:faceGroupId?" element={<Faces />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

describe('Faces page', () => {
  let intersectionCallback: IntersectionObserverCallback | undefined

  beforeEach(() => {
    mocks.role = 'user'
    mocks.listGroups.mockReset()
    mocks.getGroup.mockReset()
    mocks.getThumbnailURL.mockReset()
    mocks.mergeGroups.mockReset()
    mocks.reject.mockReset()
    mocks.reject.mockResolvedValue({ rejectedCount: 1 })
    mocks.lightbox.mockReset()
    mocks.getThumbnailURL.mockReturnValue('blob:face')
    mocks.listGroups.mockResolvedValue({
      groups: [
        { faceGroupId: 5, faceCount: 4, mediaCount: 3 },
        { faceGroupId: 8, faceCount: 2, mediaCount: 2 },
      ],
      nextCursor: null,
      hasMore: false,
    })
    intersectionCallback = undefined
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        constructor(callback: IntersectionObserverCallback) {
          intersectionCallback = callback
        }

        observe() {}
        disconnect() {}
      }
    )
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
    expect(screen.queryByText(/recognized faces|4 faces|Person 5/)).toBeNull()
    expect(screen.getByText('3')).toBeTruthy()
    expect(screen.getByText('2')).toBeTruthy()
    expect(mocks.getThumbnailURL).toHaveBeenCalledWith({ faceGroupId: 5 })
    const heading = screen.getByRole('heading', { name: 'Faces' })
    expect(heading.closest('[data-page-frame="true"]')).toBeTruthy()
    expect(
      screen.getByRole('link', { name: 'Face group 5, 3 media' }).parentElement?.parentElement
        ?.className
    ).toContain('2xl:grid-cols-8')
  })

  it('offers retry after a thumbnail fails without opening the group', async () => {
    renderFaces()
    const link = await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    const image = link.querySelector('img')!
    const originalURL = image.src
    fireEvent.error(image)
    fireEvent.click(screen.getByRole('button', { name: 'Retry thumbnail for face group 5' }))
    expect(image.src).not.toBe(originalURL)
    expect(screen.queryByRole('button', { name: 'Retry thumbnail for face group 5' })).toBeNull()
    expect(mocks.getGroup).not.toHaveBeenCalled()
  })

  it('lets administrators select and merge two groups', async () => {
    mocks.role = 'admin'
    mocks.mergeGroups.mockResolvedValue({
      group: { faceGroupId: 5, faceCount: 6, mediaCount: 5 },
    })
    renderFaces()
    const user = userEvent.setup()

    await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    await user.click(screen.getByRole('button', { name: 'Select face group 5' }))
    await user.click(screen.getByRole('button', { name: 'Select face group 8' }))
    expect(screen.getByText('curated group', { exact: false })).toBeTruthy()
    const beforeMerge = screen
      .getByRole('link', { name: 'Face group 5, 3 media' })
      .querySelector('img')!.src
    await user.click(screen.getByRole('button', { name: 'Merge groups' }))

    await waitFor(() =>
      expect(mocks.mergeGroups).toHaveBeenCalledWith({ faceGroupIds: [5, 8] }, expect.anything())
    )
    await waitFor(() =>
      expect(
        screen.getByRole('link', { name: 'Face group 5, 3 media' }).querySelector('img')!.src
      ).not.toBe(beforeMerge)
    )
  })

  it('loads the next face-group page when the scroll sentinel approaches', async () => {
    mocks.listGroups
      .mockResolvedValueOnce({
        groups: [{ faceGroupId: 5, faceCount: 4, mediaCount: 3 }],
        nextCursor: '100',
        hasMore: true,
      })
      .mockResolvedValueOnce({
        groups: [{ faceGroupId: 105, faceCount: 2, mediaCount: 2 }],
        nextCursor: null,
        hasMore: false,
      })
    renderFaces()
    await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    await waitFor(() => expect(intersectionCallback).toBeDefined())
    const firstThumbnail = screen
      .getByRole('link', { name: 'Face group 5, 3 media' })
      .querySelector('img')!
    const originalURL = firstThumbnail.src
    expect(firstThumbnail.getAttribute('loading')).toBe('lazy')
    const now = vi.spyOn(Date, 'now').mockReturnValue(Date.now() + 1000)

    act(() => {
      intersectionCallback?.(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    expect(await screen.findByRole('link', { name: 'Face group 105, 2 media' })).toBeTruthy()
    expect(firstThumbnail.src).toBe(originalURL)
    now.mockRestore()
    expect(mocks.listGroups).toHaveBeenNthCalledWith(2, {
      cursor: '100',
      limit: 100,
    })
  })

  it('shows associated media and opens it in the existing lightbox', async () => {
    mocks.getGroup.mockResolvedValue({
      group: { faceGroupId: 5, faceCount: 2, mediaCount: 2 },
      media: [{ id: 10 }, { id: 11 }],
    })
    renderFaces('/faces/5')

    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: 'Open second media' }))

    expect(mocks.getGroup).toHaveBeenCalledWith({ faceGroupId: 5 })
    expect(mocks.lightbox).toHaveBeenCalledWith(
      expect.objectContaining({ mediaIds: [10, 11], currentIndex: 1 })
    )
  })
  it('keeps the list DOM, selection and scroll position behind details and returns with Backspace', async () => {
    mocks.role = 'admin'
    mocks.getGroup.mockResolvedValue({
      group: { faceGroupId: 5, faceCount: 2, mediaCount: 2 },
      media: [{ id: 10 }, { id: 11 }],
      faces: [],
    })
    renderFaces()
    const link = await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    const list = link.closest('.overflow-y-auto') as HTMLElement
    list.scrollTop = 450
    await userEvent.click(screen.getByRole('button', { name: 'Select face group 5' }))
    await userEvent.click(link)
    await screen.findByRole('heading', { name: 'Face Group #5' })
    expect(list.isConnected).toBe(true)
    expect(screen.getByRole('dialog')).toBeTruthy()
    await userEvent.keyboard('{Backspace}')
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(screen.getByRole('link', { name: 'Face group 5, 3 media' })).toBe(link)
    expect(list.scrollTop).toBe(450)
    expect(screen.getByText('1 groups selected')).toBeTruthy()
  })

  it('rejects one group without reloading surviving or deleted thumbnails', async () => {
    mocks.role = 'admin'
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    const groups = Array.from({ length: 40 }, (_, index) => ({
      faceGroupId: index + 1,
      faceCount: 1,
      mediaCount: 1,
    }))
    mocks.listGroups.mockResolvedValueOnce({ groups, nextCursor: null, hasMore: false })
    mocks.listGroups.mockResolvedValue({
      groups: groups.slice(1),
      nextCursor: null,
      hasMore: false,
    })
    renderFaces()
    await screen.findByRole('link', { name: 'Face group 40, 1 media' })
    const thumbnails = groups.map((group) =>
      screen
        .getByRole('link', { name: `Face group ${group.faceGroupId}, 1 media` })
        .querySelector('img')!
    )
    const urls = thumbnails.map((img) => img.src)
    const changes: MutationRecord[] = []
    const observer = new MutationObserver((records) => changes.push(...records))
    for (const image of thumbnails)
      observer.observe(image, { attributes: true, attributeFilter: ['src'] })
    await userEvent.click(screen.getByRole('button', { name: 'Select face group 1' }))
    await userEvent.click(screen.getByRole('button', { name: 'Not a face' }))
    await waitFor(() =>
      expect(screen.queryByRole('link', { name: 'Face group 1, 1 media' })).toBeNull()
    )
    await waitFor(() => expect(screen.queryByText('1 groups selected')).toBeNull())
    observer.disconnect()
    expect(changes).toHaveLength(0)
    expect(thumbnails.map((img) => img.src)).toEqual(urls)
    expect(screen.getByRole('link', { name: 'Face group 40, 1 media' }).querySelector('img')).toBe(
      thumbnails[39]
    )
  })

  it('shows selected face thumbnails and submits the admin global rejection', async () => {
    mocks.role = 'admin'
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderFaces()
    await screen.findByRole('link', { name: 'Face group 5, 3 media' })
    await userEvent.click(screen.getByRole('button', { name: 'Select face group 5' }))
    expect(screen.getByAltText('Face group 5')).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Not a face' }))
    await waitFor(() =>
      expect(mocks.reject.mock.calls[0]?.[0]).toEqual(
        expect.objectContaining({ groupIds: [5], faceGroupId: null, faceIds: [] })
      )
    )
  })
  it('defaults admins to browsing and clears selections when cancelling selection mode', async () => {
    mocks.role = 'admin'
    mocks.getGroup.mockResolvedValue({
      group: { faceGroupId: 5, faceCount: 2, mediaCount: 2 },
      media: [{ id: 10 }, { id: 11 }],
      faces: [
        { faceId: 100, mediaId: 10 },
        { faceId: 101, mediaId: 11 },
      ],
    })
    renderFaces('/faces/5')
    await screen.findByRole('heading', { name: 'Face Group #5' })
    expect(screen.queryByRole('button', { name: 'Select media 10' })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Select', exact: true }))
    await userEvent.click(screen.getByRole('button', { name: 'Open second media' }))
    expect(screen.getByRole('button', { name: 'Not a face (1)' })).toBeTruthy()
    expect(mocks.lightbox).not.toHaveBeenCalled()
    await userEvent.click(screen.getByRole('button', { name: 'Cancel selection' }))
    expect(screen.queryByRole('button', { name: /Not a face/ })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Select', exact: true }))
    expect(screen.queryByRole('button', { name: /Not a face/ })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: 'Cancel selection' }))
    await userEvent.click(screen.getByRole('button', { name: 'Open second media' }))
    expect(mocks.lightbox).toHaveBeenCalledWith(
      expect.objectContaining({ mediaIds: [10, 11], currentIndex: 1 })
    )
  })

  it('requires explicit choices when a selected media has multiple faces in the group', async () => {
    mocks.role = 'admin'
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    mocks.getGroup.mockResolvedValue({
      group: { faceGroupId: 5, faceCount: 2, mediaCount: 1 },
      media: [{ id: 10 }],
      faces: [
        { faceId: 100, mediaId: 10 },
        { faceId: 101, mediaId: 10 },
      ],
    })
    renderFaces('/faces/5')
    await screen.findByRole('heading', { name: 'Face Group #5' })
    await userEvent.click(screen.getByRole('button', { name: 'Select', exact: true }))
    await userEvent.click(screen.getByRole('button', { name: 'Select media 10' }))
    expect(screen.getByRole('button', { name: 'Not a face (0)' }).hasAttribute('disabled')).toBe(
      true
    )
    await userEvent.click(screen.getByRole('checkbox', { name: /Face #100/ }))
    await userEvent.click(screen.getByRole('button', { name: 'Not a face (1)' }))
    await waitFor(() =>
      expect(mocks.reject.mock.calls[0]?.[0]).toEqual(
        expect.objectContaining({ groupIds: [], faceGroupId: 5, faceIds: [100] })
      )
    )
  })
})

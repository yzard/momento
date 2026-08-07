import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { Media } from '../../../src/frontend/api/types'

const mocks = vi.hoisted(() => ({
  groups: vi.fn(),
  status: vi.fn(),
  lightbox: vi.fn(),
  loadThumbnail: vi.fn(),
  observers: [] as Array<{
    callback: IntersectionObserverCallback
    root: Element | Document | null
    target: Element | null
  }>,
  role: 'user' as 'admin' | 'user',
}))

vi.mock('../../../src/frontend/api/deduplicate', () => ({
  deduplicateApi: {
    groups: mocks.groups,
    status: mocks.status,
    start: vi.fn(),
    cancel: vi.fn(),
    clean: vi.fn(),
  },
}))

vi.mock('../../../src/frontend/hooks/useAuth', () => ({
  useAuth: () => ({ user: { id: 7, username: 'viewer', role: mocks.role } }),
}))

vi.mock('../../../src/frontend/utils/batcher', () => ({
  batchLoader: { load: mocks.loadThumbnail },
}))

vi.mock('../../../src/frontend/components/viewer/Lightbox', () => ({
  default: (props: { mediaIds: number[]; currentIndex: number }) => {
    mocks.lightbox(props)
    return <div data-testid="lightbox">{props.mediaIds.join(',')}:{props.currentIndex}</div>
  },
}))

import Deduplicate from '../../../src/frontend/pages/Deduplicate'

function createMedia(id: number, originalFilename: string): Media {
  return {
    id,
    filename: originalFilename,
    originalFilename,
    mediaType: 'image',
    mimeType: 'image/jpeg',
    width: 1920,
    height: 1080,
    fileSize: 1024,
    durationSeconds: null,
    dateTaken: null,
    gpsLatitude: null,
    gpsLongitude: null,
    cameraMake: null,
    cameraModel: null,
    lensMake: null,
    lensModel: null,
    iso: null,
    exposureTime: null,
    fNumber: null,
    focalLength: null,
    gpsAltitude: null,
    locationCity: null,
    locationState: null,
    locationCountry: null,
    videoCodec: null,
    focalLength35mm: null,
    keywords: null,
    contentHash: null,
    createdAt: '2024-01-01T00:00:00Z',
  }
}

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <Deduplicate />
    </QueryClientProvider>,
  )
}

describe('Deduplicate page', () => {
  beforeEach(() => {
    mocks.role = 'user'
    mocks.groups.mockReset()
    mocks.status.mockReset()
    mocks.lightbox.mockReset()
    mocks.loadThumbnail.mockReset()
    mocks.loadThumbnail.mockResolvedValue(null)
    mocks.observers.length = 0
    class MockIntersectionObserver {
      readonly root: Element | Document | null
      readonly rootMargin = '0px'
      readonly thresholds = [0]
      target: Element | null = null

      constructor(readonly callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
        this.root = options?.root ?? null
        mocks.observers.push(this)
      }

      observe(target: Element) {
        this.target = target
      }

      disconnect() {}
      unobserve() {}
      takeRecords(): IntersectionObserverEntry[] { return [] }
    }
    vi.stubGlobal('IntersectionObserver', MockIntersectionObserver)
    mocks.groups.mockResolvedValue({
      groups: [],
      nextCursor: null,
      hasMore: false,
      totalGroups: 0,
      totalMedia: 0,
    })
    mocks.status.mockResolvedValue({ status: 'idle', runId: null })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('shows user groups without administrator controls', async () => {
    renderPage()

    expect(await screen.findByText('No duplicate groups')).toBeTruthy()
    expect(screen.getByText('Total 0 Similar Groups')).toBeTruthy()
    expect(screen.getByText('Total 0 Media')).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Deduplicate' })).toBeNull()
    expect(screen.queryByText('Start scan')).toBeNull()
    expect(mocks.status).not.toHaveBeenCalled()
  })

  it('does not expose scan controls to administrators on the utility page', async () => {
    mocks.role = 'admin'
    renderPage()

    expect(await screen.findByText('No duplicate groups')).toBeTruthy()
    expect(screen.queryByText('Start scan')).toBeNull()
    expect(mocks.status).not.toHaveBeenCalled()
  })

  it('automatically loads the next page near the end of the scroll container', async () => {
    const firstGroup = { clusterId: 10, items: [createMedia(1, 'one.jpg'), createMedia(2, 'two.jpg')] }
    const secondGroup = { clusterId: 20, items: [createMedia(3, 'three.jpg'), createMedia(4, 'four.jpg')] }
    mocks.groups
      .mockResolvedValueOnce({
        groups: [firstGroup],
        nextCursor: '10',
        hasMore: true,
        totalGroups: 2,
        totalMedia: 4,
      })
      .mockResolvedValueOnce({
        groups: [secondGroup],
        nextCursor: null,
        hasMore: false,
        totalGroups: 2,
        totalMedia: 4,
      })
    renderPage()

    expect(await screen.findByText('Similar group 10')).toBeTruthy()
    await waitFor(() => expect(mocks.observers.some((observer) => observer.root !== null)).toBe(true))
    const paginationObserver = mocks.observers.find((observer) => observer.root !== null)
    expect(paginationObserver?.target).toBeTruthy()
    act(() => {
      paginationObserver?.callback([
        { isIntersecting: true, target: paginationObserver.target } as IntersectionObserverEntry,
      ], paginationObserver as unknown as IntersectionObserver)
    })

    expect(await screen.findByText('Similar group 20')).toBeTruthy()
    expect(mocks.groups).toHaveBeenNthCalledWith(2, { cursor: '10', limit: 20 })
    expect(screen.getByText('Total 2 Similar Groups')).toBeTruthy()
    expect(screen.getByText('Total 4 Media')).toBeTruthy()
  })

  it('opens media for inspection while selection remains a separate action', async () => {
    const user = userEvent.setup()
    mocks.groups.mockResolvedValue({
      groups: [{
        clusterId: 10,
        items: [createMedia(1, 'one.jpg'), createMedia(2, 'two.jpg')],
      }],
      nextCursor: null,
      hasMore: false,
      totalGroups: 1,
      totalMedia: 2,
    })
    renderPage()

    await user.click(await screen.findByRole('button', { name: 'Select one.jpg' }))
    expect(screen.getAllByText('1 selected')).toHaveLength(2)
    expect(mocks.lightbox).not.toHaveBeenCalled()

    await user.click(screen.getByRole('button', { name: 'Inspect two.jpg' }))
    expect(screen.getByTestId('lightbox').textContent).toBe('1,2:1')
    expect(mocks.lightbox).toHaveBeenCalledWith(expect.objectContaining({
      mediaIds: [1, 2],
      currentIndex: 1,
    }))
  })
})

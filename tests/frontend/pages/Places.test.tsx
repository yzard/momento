import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from '../../../src/frontend/node_modules/react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  list: vi.fn(),
  get: vi.fn(),
  loadThumbnail: vi.fn(),
  photoGrid: vi.fn(),
  lightbox: vi.fn(),
}))

vi.mock('../../../src/frontend/api/places', () => ({
  placesApi: {
    list: mocks.list,
    get: mocks.get,
    getThumbnail: mocks.loadThumbnail,
  },
}))
vi.mock('../../../src/frontend/components/timeline/PhotoGrid', () => ({
  default: ({
    media,
    onPhotoClick,
  }: {
    media: Array<{ id: number }>
    onPhotoClick: (media: { id: number }) => void
  }) => {
    mocks.photoGrid(media)
    const lastMedia = media[media.length - 1]
    return lastMedia ? (
      <button type="button" onClick={() => onPhotoClick(lastMedia)}>
        Open last media
      </button>
    ) : null
  },
}))
vi.mock('../../../src/frontend/components/viewer/Lightbox', () => ({
  default: (props: { mediaIds: number[]; currentIndex: number }) => {
    mocks.lightbox(props)
    return <div>Lightbox</div>
  },
}))

import Places from '../../../src/frontend/pages/Places'

interface ObservedElement {
  callback: IntersectionObserverCallback
  target: Element
}

function renderPlaces(path = '/places') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/places" element={<Places />} />
          <Route path="/places/:placeId" element={<Places />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  )
}

describe('Places page', () => {
  const observedElements: ObservedElement[] = []

  beforeEach(() => {
    observedElements.length = 0
    mocks.list.mockReset()
    mocks.get.mockReset()
    mocks.loadThumbnail.mockReset().mockResolvedValue('place-thumbnail')
    mocks.photoGrid.mockReset()
    mocks.lightbox.mockReset()
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        private callback: IntersectionObserverCallback

        constructor(callback: IntersectionObserverCallback) {
          this.callback = callback
        }

        observe(target: Element) {
          observedElements.push({ callback: this.callback, target })
        }

        disconnect() {}
      }
    )
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('shows responsive place cards with lazy representative thumbnails and accessible labels', async () => {
    mocks.list.mockResolvedValue({
      places: [
        {
          placeId: 'paris-france',
          city: 'Paris',
          state: 'Ile-de-France',
          country: 'France',
          mediaCount: 8,
        },
        {
          placeId: 'tokyo-japan',
          city: 'Tokyo',
          state: null,
          country: 'Japan',
          mediaCount: 3,
        },
      ],
      nextCursor: null,
      hasMore: false,
    })
    renderPlaces()

    const parisCard = await screen.findByRole('link', {
      name: 'Paris, Ile-de-France, France, 8 media',
    })
    expect(parisCard.className).toContain('aspect-[3/2]')
    expect(screen.getByText('Paris')).toBeTruthy()
    expect(screen.getByText('Ile-de-France, France')).toBeTruthy()
    expect(screen.getByText('8 media')).toBeTruthy()
    expect(screen.getByText('Japan')).toBeTruthy()
    expect(
      screen.getByRole('heading', { name: 'Places' }).closest('[data-page-frame="true"]')
    ).toBeTruthy()
    expect(parisCard.parentElement?.className).toContain('2xl:grid-cols-5')

    const cardObserver = observedElements.find(({ target }) => target === parisCard)
    expect(cardObserver).toBeDefined()
    act(() => {
      cardObserver?.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    await waitFor(() => expect(mocks.loadThumbnail).toHaveBeenCalledWith('paris-france'))
    await waitFor(() =>
      expect(parisCard.querySelector('img')?.getAttribute('loading')).toBe('lazy')
    )
  })

  it('loads subsequent place pages when the sentinel approaches', async () => {
    mocks.list
      .mockResolvedValueOnce({
        places: [
          {
            placeId: 'paris-france',
            city: 'Paris',
            state: null,
            country: 'France',
            mediaCount: 8,
          },
        ],
        nextCursor: 'place-100',
        hasMore: true,
      })
      .mockResolvedValueOnce({
        places: [
          {
            placeId: 'tokyo-japan',
            city: 'Tokyo',
            state: null,
            country: 'Japan',
            mediaCount: 3,
          },
        ],
        nextCursor: null,
        hasMore: false,
      })
    renderPlaces()
    await screen.findByRole('link', { name: 'Paris, France, 8 media' })
    await waitFor(() =>
      expect(observedElements.some(({ target }) => target.tagName === 'DIV')).toBe(true)
    )

    const sentinelObserver = observedElements.find(({ target }) => target.tagName === 'DIV')
    act(() => {
      sentinelObserver?.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    expect(await screen.findByRole('link', { name: 'Tokyo, Japan, 3 media' })).toBeTruthy()
    expect(mocks.list).toHaveBeenNthCalledWith(2, {
      cursor: 'place-100',
      limit: 100,
    })
  })

  it('accumulates detail media in API order and opens the existing lightbox at the selected index', async () => {
    const place = {
      placeId: 'paris-france',
      city: 'Paris',
      state: null,
      country: 'France',
      mediaCount: 3,
    }
    mocks.get
      .mockResolvedValueOnce({
        place,
        media: [{ id: 10 }, { id: 11 }],
        nextCursor: 'media-11',
        hasMore: true,
      })
      .mockResolvedValueOnce({
        place,
        media: [{ id: 12 }],
        nextCursor: null,
        hasMore: false,
      })
    renderPlaces('/places/paris-france')
    await screen.findByRole('heading', { name: 'Paris, France' })
    await waitFor(() =>
      expect(observedElements.some(({ target }) => target.tagName === 'DIV')).toBe(true)
    )

    const sentinelObserver = observedElements.find(({ target }) => target.tagName === 'DIV')
    act(() => {
      sentinelObserver?.callback(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    await waitFor(() =>
      expect(mocks.photoGrid).toHaveBeenLastCalledWith([{ id: 10 }, { id: 11 }, { id: 12 }])
    )
    await userEvent.click(screen.getByRole('button', { name: 'Open last media' }))

    expect(mocks.get).toHaveBeenNthCalledWith(2, {
      placeId: 'paris-france',
      cursor: 'media-11',
      limit: 100,
    })
    expect(mocks.lightbox).toHaveBeenLastCalledWith(
      expect.objectContaining({ mediaIds: [10, 11, 12], currentIndex: 2 })
    )
  })
})

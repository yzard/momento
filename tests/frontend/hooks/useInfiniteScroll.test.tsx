import { act, cleanup, render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { useRef } from 'react'
import { useInfiniteScroll } from '../../../src/frontend/hooks/useInfiniteScroll'

const observer = {
  callback: null as IntersectionObserverCallback | null,
  disconnect: vi.fn(),
  observe: vi.fn(),
  options: null as IntersectionObserverInit | null,
}

function Harness({
  fetchNextPage,
  hasNextPage,
}: {
  fetchNextPage: () => Promise<unknown>
  hasNextPage: boolean
}) {
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const loadMoreRef = useRef<HTMLDivElement>(null)
  useInfiniteScroll({
    scrollContainerRef,
    loadMoreRef,
    hasNextPage,
    isFetchingNextPage: false,
    isFetchNextPageError: false,
    fetchNextPage,
  })
  return (
    <div ref={scrollContainerRef}>
      <div ref={loadMoreRef}>Load more</div>
    </div>
  )
}

describe('useInfiniteScroll', () => {
  beforeEach(() => {
    observer.callback = null
    observer.options = null
    observer.disconnect.mockReset()
    observer.observe.mockReset()
    vi.stubGlobal(
      'IntersectionObserver',
      class {
        constructor(callback: IntersectionObserverCallback, options?: IntersectionObserverInit) {
          observer.callback = callback
          observer.options = options ?? null
        }

        observe(target: Element) {
          observer.observe(target)
        }

        disconnect() {
          observer.disconnect()
        }
      }
    )
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  it('loads the next page when the sentinel enters the scroll container', async () => {
    const fetchNextPage = vi.fn().mockResolvedValue(undefined)
    const { container } = render(<Harness fetchNextPage={fetchNextPage} hasNextPage />)
    const root = container.firstElementChild
    const target = root?.firstElementChild

    expect(observer.observe).toHaveBeenCalledWith(target)
    expect(observer.options).toMatchObject({
      root,
      rootMargin: '0px 0px 320px 0px',
    })

    await act(async () => {
      observer.callback?.(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    })

    expect(fetchNextPage).toHaveBeenCalledOnce()
  })

  it('does not create an observer when there is no next page', () => {
    render(<Harness fetchNextPage={vi.fn()} hasNextPage={false} />)

    expect(observer.observe).not.toHaveBeenCalled()
  })
})

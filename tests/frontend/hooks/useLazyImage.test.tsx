import { act, render, renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { useLazyImage } from '../../../src/frontend/hooks/useLazyImage'

let intersectionCallback: IntersectionObserverCallback | null = null

class ControlledIntersectionObserver implements IntersectionObserver {
  readonly root = null
  readonly rootMargin = '0px'
  readonly thresholds = [0]

  constructor(callback: IntersectionObserverCallback) {
    intersectionCallback = callback
  }

  disconnect(): void {}
  observe(): void {}
  takeRecords(): IntersectionObserverEntry[] {
    return []
  }
  unobserve(): void {}
}

describe('useLazyImage', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    intersectionCallback = null
  })

  it('loads only after the target intersects', async () => {
    vi.stubGlobal('IntersectionObserver', ControlledIntersectionObserver)
    const loader = { load: vi.fn().mockResolvedValue('/thumbnail/1') }
    function LazyImageHarness() {
      const { targetRef, imageUrl } = useLazyImage<HTMLDivElement, number>({
        resourceId: 1,
        loader,
        getCachedUrl: null,
        rootMargin: '400px',
      })
      return <div ref={targetRef} data-image-url={imageUrl ?? ''} />
    }
    const view = render(<LazyImageHarness />)
    expect(loader.load).not.toHaveBeenCalled()

    await act(async () =>
      intersectionCallback?.(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver
      )
    )

    await waitFor(() =>
      expect(view.container.firstElementChild?.getAttribute('data-image-url')).toBe('/thumbnail/1')
    )
  })

  it('uses a cached URL without observing or loading', () => {
    const observe = vi.fn()
    vi.stubGlobal(
      'IntersectionObserver',
      class implements IntersectionObserver {
        readonly root = null
        readonly rootMargin = '0px'
        readonly thresholds = [0]
        disconnect(): void {}
        observe = observe
        takeRecords(): IntersectionObserverEntry[] {
          return []
        }
        unobserve(): void {}
      }
    )
    const loader = { load: vi.fn() }
    const hook = renderHook(() =>
      useLazyImage<HTMLDivElement, number>({
        resourceId: 2,
        loader,
        getCachedUrl: () => '/cached/2',
        rootMargin: '400px',
      })
    )

    expect(hook.result.current.imageUrl).toBe('/cached/2')
    expect(observe).not.toHaveBeenCalled()
    expect(loader.load).not.toHaveBeenCalled()
  })
})

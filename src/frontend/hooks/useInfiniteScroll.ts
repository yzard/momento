import { useEffect, type RefObject } from 'react'

interface InfiniteScrollOptions {
  scrollContainerRef: RefObject<HTMLDivElement>
  loadMoreRef: RefObject<HTMLDivElement>
  hasNextPage: boolean
  isFetchingNextPage: boolean
  isFetchNextPageError: boolean
  fetchNextPage: () => Promise<unknown>
}

export function useInfiniteScroll({
  scrollContainerRef,
  loadMoreRef,
  hasNextPage,
  isFetchingNextPage,
  isFetchNextPageError,
  fetchNextPage,
}: InfiniteScrollOptions): void {
  useEffect(() => {
    const target = loadMoreRef.current
    const root = scrollContainerRef.current
    if (!target || !root || !hasNextPage || isFetchNextPageError) return

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries[0]?.isIntersecting || isFetchingNextPage) return
        void fetchNextPage()
      },
      { root, rootMargin: '0px 0px 320px 0px' }
    )

    observer.observe(target)
    return () => observer.disconnect()
  }, [
    fetchNextPage,
    hasNextPage,
    isFetchNextPageError,
    isFetchingNextPage,
    loadMoreRef,
    scrollContainerRef,
  ])
}

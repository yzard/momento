import { useLayoutEffect, useRef, type RefObject } from 'react'

/** Keep the same visible tile when a mutation changes counts and ordering. */
export function useCollectionScrollAnchor(container: RefObject<HTMLDivElement>, revision: number) {
  const saved = useRef<{ keys: string[]; index: number; offset: number } | null>(null)
  useLayoutEffect(() => {
    const root = container.current
    if (!root) return
    const tiles = () => [...root.querySelectorAll<HTMLElement>('[data-collection-key]')]
    const previous = saved.current
    if (previous) {
      const current = new Map(tiles().map((tile) => [tile.dataset.collectionKey, tile]))
      const candidates = [
        ...previous.keys.slice(previous.index),
        ...previous.keys.slice(0, previous.index).reverse(),
      ]
      const tile = candidates.map((key) => current.get(key)).find(Boolean)
      if (tile)
        root.scrollTop +=
          tile.getBoundingClientRect().top - root.getBoundingClientRect().top - previous.offset
    }
    const capture = () => {
      const all = tiles(),
        top = root.getBoundingClientRect().top
      const index = all.findIndex((tile) => tile.getBoundingClientRect().bottom > top)
      if (index >= 0)
        saved.current = {
          keys: all.map((tile) => tile.dataset.collectionKey!),
          index,
          offset: all[index]!.getBoundingClientRect().top - top,
        }
    }
    capture()
    root.addEventListener('scroll', capture, { passive: true })
    return () => root.removeEventListener('scroll', capture)
  }, [container, revision])
}

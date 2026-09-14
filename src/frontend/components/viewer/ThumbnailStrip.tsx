import { useEffect, useRef, useState } from 'react'
import { mediaApi } from '../../api/media'

const ITEM_WIDTH = 60
const OVERSCAN = 2

interface ThumbnailStripProps {
  mediaIds: number[]
  currentIndex: number
  onIndexChange: (index: number) => void
}

export function ThumbnailStrip({ mediaIds, currentIndex, onIndexChange }: ThumbnailStripProps) {
  const viewport = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const [left, setLeft] = useState(0)
  useEffect(() => {
    const element = viewport.current
    if (!element) return
    const measure = () => setWidth(element.clientWidth)
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [])
  useEffect(() => {
    const element = viewport.current
    if (!element) return
    const target = Math.max(
      0,
      Math.min(
        currentIndex * ITEM_WIDTH - (width - ITEM_WIDTH) / 2,
        mediaIds.length * ITEM_WIDTH - width
      )
    )
    element.scrollLeft = target
    setLeft(target)
  }, [currentIndex, width, mediaIds.length])
  const start = Math.max(0, Math.floor(left / ITEM_WIDTH) - OVERSCAN)
  const end = Math.min(mediaIds.length, Math.ceil((left + width) / ITEM_WIDTH) + OVERSCAN)
  return (
    <div
      ref={viewport}
      role="region"
      aria-label="Media thumbnails"
      className="shrink-0 overflow-x-auto overscroll-x-contain border-t border-border bg-background/80 py-2"
      onScroll={(event) => setLeft(event.currentTarget.scrollLeft)}
    >
      <div className="relative h-14" style={{ width: mediaIds.length * ITEM_WIDTH }}>
        {mediaIds.slice(start, end).map((id, offset) => {
          const index = start + offset
          return (
            <button
              key={`${index}-${id}`}
              type="button"
              aria-label={`View media ${index + 1}`}
              aria-current={index === currentIndex ? 'true' : undefined}
              onClick={() => onIndexChange(index)}
              className={`absolute top-0 h-14 w-14 overflow-hidden rounded border-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary ${index === currentIndex ? 'border-primary' : 'border-transparent opacity-70 hover:opacity-100'}`}
              style={{ left: index * ITEM_WIDTH + 2 }}
            >
              <img
                src={mediaApi.getThumbnailURL(id, 'tiny')}
                alt=""
                loading="lazy"
                draggable={false}
                className="h-full w-full object-cover"
              />
            </button>
          )
        })}
      </div>
    </div>
  )
}

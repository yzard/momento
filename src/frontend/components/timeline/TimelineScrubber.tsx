import { useCallback, useRef, useState, type PointerEvent, type WheelEvent } from 'react'
import type { TimelineMarker } from '../../api/media'

interface TimelineScrubberProps {
  markers: TimelineMarker[]
  activeMarkerIndex: number
  onMarkerSelect: (marker: TimelineMarker) => void
  onWheel: (event: WheelEvent<HTMLElement>) => void
}

interface ScrubberMarkersProps {
  markers: TimelineMarker[]
  activeMarkerIndex: number
  hoveredIndex: number | null
  onMarkerSelect: (marker: TimelineMarker) => void
}

function formatMarkerLabel(marker: TimelineMarker): string {
  const [year, month] = marker.label.split('-')
  if (!year || !month) return marker.label
  const monthName = new Intl.DateTimeFormat('en-US', { month: 'short' }).format(
    new Date(Number(year), Number(month) - 1, 1)
  )
  return `${monthName} ${year}`
}

function ScrubberMarkers({
  markers,
  activeMarkerIndex,
  hoveredIndex,
  onMarkerSelect,
}: ScrubberMarkersProps) {
  return (
    <>
      {markers.map((marker, index) => {
        const isYearStart =
          index === 0 || marker.label.slice(0, 4) !== markers[index - 1]?.label.slice(0, 4)
        const distance = hoveredIndex === null ? 0 : Math.abs(hoveredIndex - index)
        const scale = distance === 0 ? 1.75 : distance === 1 ? 1.3 : 1
        return (
          <button
            key={marker.label}
            type="button"
            aria-current={index === activeMarkerIndex ? 'true' : undefined}
            aria-label={`Jump to ${formatMarkerLabel(marker)}`}
            className="absolute right-0 flex -translate-y-1/2 origin-right items-center gap-1.5 transition-transform duration-200 ease-out motion-reduce:transition-none"
            style={{
              top: `${(index / Math.max(markers.length - 1, 1)) * 100}%`,
              transform: `translateY(-50%) scale(${scale})`,
            }}
            onClick={(event) => {
              event.stopPropagation()
              onMarkerSelect(marker)
            }}
          >
            {isYearStart && (
              <span className="rounded bg-foreground px-1.5 py-0.5 text-[10px] font-semibold text-background">
                {marker.label.slice(0, 4)}
              </span>
            )}
            <span
              className={
                index === activeMarkerIndex
                  ? 'h-1 w-7 rounded-full bg-primary'
                  : 'h-px w-4 bg-muted-foreground/70'
              }
            />
          </button>
        )
      })}
    </>
  )
}

export default function TimelineScrubber({
  markers,
  activeMarkerIndex,
  onMarkerSelect,
  onWheel,
}: TimelineScrubberProps) {
  const railRef = useRef<HTMLDivElement>(null)
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const [dragging, setDragging] = useState(false)

  const markerAtPosition = useCallback(
    (clientY: number) => {
      const rail = railRef.current
      if (!rail || markers.length === 0) return 0
      const bounds = rail.getBoundingClientRect()
      const fraction = Math.min(Math.max((clientY - bounds.top) / bounds.height, 0), 1)
      return Math.round(fraction * (markers.length - 1))
    },
    [markers.length]
  )

  const selectAtPosition = useCallback(
    (clientY: number) => {
      const marker = markers[markerAtPosition(clientY)]
      if (marker) onMarkerSelect(marker)
    },
    [markerAtPosition, markers, onMarkerSelect]
  )

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    setDragging(true)
    event.currentTarget.setPointerCapture(event.pointerId)
    selectAtPosition(event.clientY)
  }

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    setHoveredIndex(markerAtPosition(event.clientY))
    if (dragging) selectAtPosition(event.clientY)
  }

  const stopDragging = (event: PointerEvent<HTMLDivElement>) => {
    setDragging(false)
    setHoveredIndex(null)
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
  }

  const visibleIndex = hoveredIndex ?? activeMarkerIndex
  const visibleMarker = markers[visibleIndex]

  return (
    <aside
      className="group/scrubber absolute inset-y-0 right-0 z-20 hidden w-28 md:block"
      onWheel={onWheel}
    >
      <div
        ref={railRef}
        role="scrollbar"
        aria-label="Timeline index"
        aria-orientation="vertical"
        aria-valuemin={0}
        aria-valuemax={Math.max(markers.length - 1, 0)}
        aria-valuenow={activeMarkerIndex}
        tabIndex={0}
        className="absolute inset-y-8 right-3 left-3 cursor-pointer opacity-0 outline-none transition-opacity duration-200 group-hover/scrubber:opacity-100 focus-within:opacity-100 focus-visible:ring-2 focus-visible:ring-primary/40 motion-reduce:transition-none"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerLeave={() => setHoveredIndex(null)}
        onPointerUp={stopDragging}
        onPointerCancel={stopDragging}
        onKeyDown={(event) => {
          const delta = event.key === 'ArrowUp' ? -1 : event.key === 'ArrowDown' ? 1 : 0
          const target =
            event.key === 'Home'
              ? 0
              : event.key === 'End'
                ? markers.length - 1
                : activeMarkerIndex + delta
          if (delta === 0 && event.key !== 'Home' && event.key !== 'End') return
          event.preventDefault()
          const marker = markers[Math.min(Math.max(target, 0), markers.length - 1)]
          if (marker) onMarkerSelect(marker)
        }}
      >
        <div className="absolute inset-y-0 right-2 w-px bg-border" />
        <ScrubberMarkers
          markers={markers}
          activeMarkerIndex={activeMarkerIndex}
          hoveredIndex={hoveredIndex}
          onMarkerSelect={onMarkerSelect}
        />
        {visibleMarker && (
          <div
            className="absolute right-7 -translate-y-1/2 whitespace-nowrap rounded-md bg-primary px-2 py-1 text-[10px] font-semibold text-primary-foreground shadow-md"
            style={{ top: `${(visibleIndex / Math.max(markers.length - 1, 1)) * 100}%` }}
          >
            {formatMarkerLabel(visibleMarker)}
          </div>
        )}
      </div>
    </aside>
  )
}

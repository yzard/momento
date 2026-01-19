import { useCallback, useEffect, useRef, useState, type MouseEvent, type CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import { ChevronLeft, ChevronRight, X } from 'lucide-react'
import { MediaDetails } from './MediaDetails'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'

interface LightboxProps {
  media: Media[]
  currentIndex: number
  onClose: () => void
  onIndexChange: (index: number) => void
}

const ZOOM_SCALE = 2.0

export default function Lightbox({ media, currentIndex, onClose, onIndexChange }: LightboxProps) {
  const currentMedia = media[currentIndex]
  const isVideo = currentMedia?.mediaType === 'video'
  const [isZoomed, setIsZoomed] = useState(false)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const dragStart = useRef({ x: 0, y: 0 })
  const offsetStart = useRef({ x: 0, y: 0 })

  const goToPrev = useCallback(() => {
    if (currentIndex > 0) onIndexChange(currentIndex - 1)
  }, [currentIndex, onIndexChange])

  const goToNext = useCallback(() => {
    if (currentIndex < media.length - 1) onIndexChange(currentIndex + 1)
  }, [currentIndex, media.length, onIndexChange])

  const resetZoom = useCallback(() => {
    setIsZoomed(false)
    setOffset({ x: 0, y: 0 })
    setIsDragging(false)
  }, [])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (e.key === 'ArrowLeft') goToPrev()
      if (e.key === 'ArrowRight') goToNext()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onClose, goToPrev, goToNext])

  useEffect(() => {
    resetZoom()
  }, [currentIndex, resetZoom])

  if (!currentMedia) return null

  const handleDoubleClick = () => {
    setIsZoomed((prev) => {
      const next = !prev
      if (!next) {
        setOffset({ x: 0, y: 0 })
      }
      return next
    })
  }

  const handleMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (!isZoomed) return
    event.preventDefault()
    setIsDragging(true)
    dragStart.current = { x: event.clientX, y: event.clientY }
    offsetStart.current = { ...offset }
  }

  const handleMouseMove = (event: MouseEvent<HTMLDivElement>) => {
    if (!isDragging) return
    const dx = event.clientX - dragStart.current.x
    const dy = event.clientY - dragStart.current.y
    setOffset({
      x: offsetStart.current.x + dx,
      y: offsetStart.current.y + dy,
    })
  }

  const stopDragging = () => {
    if (!isDragging) return
    setIsDragging(false)
  }

  const imageStyle: CSSProperties = {
    transform: `translate(${offset.x}px, ${offset.y}px) scale(${isZoomed ? ZOOM_SCALE : 1})`,
    transition: isDragging ? 'none' : 'transform 200ms ease',
    cursor: isZoomed ? (isDragging ? 'grabbing' : 'grab') : 'zoom-in',
  }

  const lightboxContent = (
    <div className="absolute inset-0 z-[2000] flex bg-background/95 backdrop-blur-sm">
      <div className="flex-1 relative flex items-center justify-center p-4 min-w-0 min-h-0">
        <button
          onClick={onClose}
          className="absolute top-4 left-4 z-50 p-2 rounded-full bg-background/20 hover:bg-background/40 text-foreground transition-colors border border-border/10 backdrop-blur-md"
        >
          <X className="w-6 h-6" />
        </button>

        {currentIndex > 0 && (
          <button
            onClick={goToPrev}
            className="absolute left-4 top-1/2 -translate-y-1/2 z-10 p-3 rounded-full bg-background/20 hover:bg-background/40 text-foreground transition-colors border border-border/10 backdrop-blur-md"
          >
            <ChevronLeft className="w-8 h-8" />
          </button>
        )}

        {currentIndex < media.length - 1 && (
          <button
            onClick={goToNext}
            className="absolute right-4 top-1/2 -translate-y-1/2 z-10 p-3 rounded-full bg-background/20 hover:bg-background/40 text-foreground transition-colors border border-border/10 backdrop-blur-md"
          >
            <ChevronRight className="w-8 h-8" />
          </button>
        )}

        {isVideo ? (
          <video
            src={mediaApi.getPreviewUrl(currentMedia.id)}
            className="max-w-full max-h-full rounded-lg shadow-2xl"
            controls
            loop
            playsInline
            preload="metadata"
          />
        ) : (
          <div
            className="w-full h-full flex items-center justify-center overflow-hidden"
            onDoubleClick={handleDoubleClick}
            onMouseDown={handleMouseDown}
            onMouseMove={handleMouseMove}
            onMouseUp={stopDragging}
            onMouseLeave={stopDragging}
          >
            <img
              src={mediaApi.getPreviewUrl(currentMedia.id)}
              alt={currentMedia.originalFilename}
              className="max-w-full max-h-full object-contain rounded-lg shadow-2xl select-none"
              style={imageStyle}
              draggable={false}
            />
          </div>
        )}
      </div>

      <div className="w-[320px] shrink-0 bg-card border-l border-border p-6 overflow-y-auto h-full">
        <MediaDetails media={currentMedia} className="bg-transparent p-0 border-0 shadow-none" />

        <div className="mt-6 pt-6 border-t border-border">
          <p className="text-xs text-muted-foreground text-center">
            {currentIndex + 1} / {media.length}
          </p>
        </div>
      </div>
    </div>
  )

  const portalTarget = document.getElementById('app-main')
  
  if (portalTarget) {
    return createPortal(lightboxContent, portalTarget)
  }

  return lightboxContent
}

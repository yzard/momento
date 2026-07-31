import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type CSSProperties } from 'react'
import { useLocation } from 'react-router-dom'
import { createPortal } from 'react-dom'
import { ChevronLeft, ChevronRight, X, Loader2 } from 'lucide-react'
import { MediaDetails } from './MediaDetails'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'

interface LightboxProps {
  mediaIds: number[]
  currentIndex: number
  onClose: () => void
  onIndexChange: (index: number) => void
}

const ZOOM_SCALE = 2.0

export default function Lightbox({ mediaIds, currentIndex, onClose, onIndexChange }: LightboxProps) {
  const [mediaList, setMediaList] = useState<Media[]>([])
  const [isMetadataLoading, setIsMetadataLoading] = useState(true)
  const [metadataError, setMetadataError] = useState(false)
  const safeIndex = mediaList.length > 0
    ? Math.min(currentIndex, mediaList.length - 1)
    : 0
  const currentMedia = mediaList[safeIndex]
  const isVideo = currentMedia?.mediaType === 'video'
  const [isZoomed, setIsZoomed] = useState(false)
  const location = useLocation()
  const hasClosedRef = useRef(false)

  useEffect(() => {
    hasClosedRef.current = false
    window.history.pushState({ lightbox: true, path: location.pathname }, '')

    const handlePopState = () => {
      if (!hasClosedRef.current) {
        hasClosedRef.current = true
        onClose()
      }
    }

    window.addEventListener('popstate', handlePopState)
    return () => {
      window.removeEventListener('popstate', handlePopState)
    }
  }, [location.pathname, onClose])

  const handleClose = useCallback(() => {
    if (!hasClosedRef.current) {
      hasClosedRef.current = true
      window.history.back()
      onClose()
    }
  }, [onClose])
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const dragStart = useRef({ x: 0, y: 0 })
  const offsetStart = useRef({ x: 0, y: 0 })

  useEffect(() => {
    let cancelled = false
    if (mediaIds.length === 0) {
      setMediaList([])
      setIsMetadataLoading(false)
      return () => {
        cancelled = true
      }
    }

    setIsMetadataLoading(true)
    setMetadataError(false)
    mediaApi.getBatch(mediaIds)
      .then((items) => {
        if (cancelled) return
        setMediaList(items)
        setIsMetadataLoading(false)
      })
      .catch(() => {
        if (cancelled) return
        setMediaList([])
        setIsMetadataLoading(false)
        setMetadataError(true)
      })

    return () => {
      cancelled = true
    }
  }, [mediaIds])

  const goToPrev = useCallback(() => {
    if (safeIndex > 0) onIndexChange(safeIndex - 1)
  }, [safeIndex, onIndexChange])

  const goToNext = useCallback(() => {
    if (safeIndex < mediaList.length - 1) onIndexChange(safeIndex + 1)
  }, [safeIndex, mediaList.length, onIndexChange])

  const resetZoom = useCallback(() => {
    setIsZoomed(false)
    setOffset({ x: 0, y: 0 })
    setIsDragging(false)
  }, [])

  useEffect(() => {
    const handleKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') handleClose()
      if (e.key === 'ArrowLeft') goToPrev()
      if (e.key === 'ArrowRight') goToNext()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [handleClose, goToPrev, goToNext])

  useEffect(() => {
    if (!currentMedia) return
    resetZoom()
  }, [currentMedia, resetZoom])

  useEffect(() => {
    if (mediaList.length > 0 && currentIndex >= mediaList.length) {
      onIndexChange(0)
    }
  }, [currentIndex, mediaList.length, onIndexChange])

  // Load preview URL when media changes
  useEffect(() => {
    if (!currentMedia) return

    setIsLoading(true)
    setPreviewUrl(null)

    if (currentMedia.mediaType === 'video') {
      setPreviewUrl(mediaApi.getFileStreamUrl(currentMedia.id))
      setIsLoading(false)
      return
    }

    mediaApi.getPreviewBatch([currentMedia.id])
      .then((batch) => {
        const url = batch.get(currentMedia.id) ?? null
        setPreviewUrl(url)
        setIsLoading(false)
      })
      .catch(() => {
        console.error('Failed to load preview')
        setIsLoading(false)
      })
  }, [currentMedia])

  if (!currentMedia && isMetadataLoading) {
    return (
      <div className="absolute inset-0 z-[2000] flex items-center justify-center bg-background/95 backdrop-blur-sm">
        <Loader2 className="w-12 h-12 animate-spin text-muted-foreground" />
      </div>
    )
  }

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

  const handleImageKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      handleDoubleClick()
    }
  }

  const handleMouseDown = (event: ReactMouseEvent<HTMLButtonElement>) => {
    if (!isZoomed) return
    event.preventDefault()
    setIsDragging(true)
    dragStart.current = { x: event.clientX, y: event.clientY }
    offsetStart.current = { ...offset }
  }

  const handleMouseMove = (event: ReactMouseEvent<HTMLButtonElement>) => {
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
          type="button"
          onClick={handleClose}
          className="absolute top-4 right-4 z-50 p-2 rounded-full bg-background/20 hover:bg-background/40 text-foreground transition-colors border border-border/10 backdrop-blur-md"
        >
          <X className="w-6 h-6" />
        </button>

        {safeIndex > 0 && (
          <button
            type="button"
            onClick={goToPrev}
            className="absolute left-4 top-1/2 -translate-y-1/2 z-10 p-3 rounded-full bg-background/20 hover:bg-background/40 text-foreground transition-colors border border-border/10 backdrop-blur-md"
          >
            <ChevronLeft className="w-8 h-8" />
          </button>
        )}

        {safeIndex < mediaList.length - 1 && (
          <button
            type="button"
            onClick={goToNext}
            className="absolute right-4 top-1/2 -translate-y-1/2 z-10 p-3 rounded-full bg-background/20 hover:bg-background/40 text-foreground transition-colors border border-border/10 backdrop-blur-md"
          >
            <ChevronRight className="w-8 h-8" />
          </button>
        )}

        {isLoading || isMetadataLoading ? (
          <div className="flex items-center justify-center">
            <Loader2 className="w-12 h-12 animate-spin text-muted-foreground" />
          </div>
        ) : metadataError ? (
          <div className="text-sm text-muted-foreground">Unable to load media details.</div>
        ) : previewUrl ? (
          isVideo ? (
            <video
              src={previewUrl}
              className="max-w-full max-h-full rounded-lg shadow-2xl"
              controls
              loop
              playsInline
              preload="metadata"
            >
              <track kind="captions" />
            </video>
          ) : (
            <button
              type="button"
              className="w-full h-full flex items-center justify-center overflow-hidden"
              onDoubleClick={handleDoubleClick}
              onMouseDown={handleMouseDown}
              onMouseMove={handleMouseMove}
              onMouseUp={stopDragging}
              onMouseLeave={stopDragging}
              onKeyDown={handleImageKeyDown}
              aria-label="Toggle zoom"
            >
              <img
                src={previewUrl}
                alt={currentMedia.originalFilename}
                className="max-w-full max-h-full object-contain rounded-lg shadow-2xl select-none"
                style={imageStyle}
                draggable={false}
              />
            </button>
          )
        ) : (
          <div className="text-muted-foreground">Failed to load media</div>
        )}
      </div>

      <div className="w-[320px] shrink-0 bg-card border-l border-border p-6 overflow-y-auto h-full">
        <MediaDetails media={currentMedia} className="bg-transparent p-0 border-0 shadow-none" />

        <div className="mt-6 pt-6 border-t border-border">
          <p className="text-xs text-muted-foreground text-center">
            {safeIndex + 1} / {mediaList.length}
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

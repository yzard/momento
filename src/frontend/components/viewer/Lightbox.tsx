import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react'
import { createPortal } from 'react-dom'
import { ChevronLeft, ChevronRight, Loader2, X } from 'lucide-react'
import { useLocation } from 'react-router-dom'

import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'
import { useMediaStreamURL } from '../../hooks/useMediaStreamURL'
import { MediaDetails } from './MediaDetails'

interface LightboxProps {
  mediaIds: number[]
  currentIndex: number
  onClose: () => void
  onIndexChange: (index: number) => void
}

const ZOOM_SCALE = 2

function useLightboxHistory(onClose: () => void): () => void {
  const location = useLocation()
  const hasClosedRef = useRef(false)

  useEffect(() => {
    hasClosedRef.current = false
    window.history.pushState({ lightbox: true, path: location.pathname }, '')
    const handlePopState = () => {
      if (hasClosedRef.current) return
      hasClosedRef.current = true
      onClose()
    }
    window.addEventListener('popstate', handlePopState)
    return () => window.removeEventListener('popstate', handlePopState)
  }, [location.pathname, onClose])

  return useCallback(() => {
    if (hasClosedRef.current) return
    hasClosedRef.current = true
    window.history.back()
    onClose()
  }, [onClose])
}

function useLightboxMedia(
  mediaIds: number[],
  currentIndex: number,
  onIndexChange: (index: number) => void
) {
  const [mediaList, setMediaList] = useState<Media[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [hasError, setHasError] = useState(false)

  useEffect(() => {
    let active = true
    setIsLoading(mediaIds.length > 0)
    setHasError(false)
    if (mediaIds.length === 0) {
      setMediaList([])
      return () => {
        active = false
      }
    }
    void mediaApi
      .getBatch(mediaIds)
      .then((items) => {
        if (!active) return
        setMediaList(items)
        setIsLoading(false)
      })
      .catch(() => {
        if (!active) return
        setMediaList([])
        setIsLoading(false)
        setHasError(true)
      })
    return () => {
      active = false
    }
  }, [mediaIds])

  const safeIndex = mediaList.length > 0 ? Math.min(currentIndex, mediaList.length - 1) : 0
  useEffect(() => {
    if (mediaList.length > 0 && currentIndex >= mediaList.length) onIndexChange(0)
  }, [currentIndex, mediaList.length, onIndexChange])

  return { mediaList, currentMedia: mediaList[safeIndex], safeIndex, isLoading, hasError }
}

function useImageZoom(mediaId: number | undefined) {
  const [isZoomed, setIsZoomed] = useState(false)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const dragStart = useRef({ x: 0, y: 0 })
  const offsetStart = useRef({ x: 0, y: 0 })

  useEffect(() => {
    setIsZoomed(false)
    setOffset({ x: 0, y: 0 })
    setIsDragging(false)
  }, [mediaId])

  const toggleZoom = () =>
    setIsZoomed((current) => {
      if (current) setOffset({ x: 0, y: 0 })
      return !current
    })
  const handleKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    toggleZoom()
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
    setOffset({
      x: offsetStart.current.x + event.clientX - dragStart.current.x,
      y: offsetStart.current.y + event.clientY - dragStart.current.y,
    })
  }
  const stopDragging = () => setIsDragging(false)
  const imageStyle: CSSProperties = {
    transform: `translate(${offset.x}px, ${offset.y}px) scale(${isZoomed ? ZOOM_SCALE : 1})`,
    transition: isDragging ? 'none' : 'transform 200ms ease',
    cursor: isZoomed ? (isDragging ? 'grabbing' : 'grab') : 'zoom-in',
  }

  return { toggleZoom, handleKeyDown, handleMouseDown, handleMouseMove, stopDragging, imageStyle }
}

function useDisplayedMedia(currentMedia: Media | undefined) {
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [isPreviewLoading, setIsPreviewLoading] = useState(true)
  const videoRef = useRef<HTMLVideoElement>(null)
  const videoResumeTimeRef = useRef(0)
  const isVideo = currentMedia?.mediaType === 'video'
  const stream = useMediaStreamURL(isVideo ? currentMedia.id : null, Boolean(isVideo))

  useEffect(() => {
    let active = true
    setPreviewUrl(null)
    setIsPreviewLoading(Boolean(currentMedia && !isVideo))
    if (!currentMedia || isVideo)
      return () => {
        active = false
      }
    void mediaApi
      .getPreviewBatch([currentMedia.id])
      .then((batch) => {
        if (!active) return
        setPreviewUrl(batch.get(currentMedia.id) ?? null)
        setIsPreviewLoading(false)
      })
      .catch(() => {
        if (active) setIsPreviewLoading(false)
      })
    return () => {
      active = false
    }
  }, [currentMedia, isVideo])

  const handleVideoError = () => {
    videoResumeTimeRef.current = videoRef.current?.currentTime ?? 0
    stream.retryStreamOnce()
  }
  const restoreVideoTime = () => {
    if (!videoRef.current || videoResumeTimeRef.current <= 0) return
    videoRef.current.currentTime = videoResumeTimeRef.current
    videoResumeTimeRef.current = 0
  }

  return {
    isVideo,
    displayedUrl: isVideo ? stream.streamURL : previewUrl,
    isLoading: isVideo ? stream.isStreamLoading : isPreviewLoading,
    videoRef,
    handleVideoError,
    restoreVideoTime,
  }
}

interface LightboxStageProps {
  media: Media
  metadataLoading: boolean
  metadataError: boolean
}

function LightboxStage({ media, metadataLoading, metadataError }: LightboxStageProps) {
  const displayed = useDisplayedMedia(media)
  const zoom = useImageZoom(media.id)
  if (displayed.isLoading || metadataLoading) {
    return (
      <Loader2
        aria-label="Loading media"
        className="h-12 w-12 animate-spin text-muted-foreground"
      />
    )
  }
  if (metadataError)
    return <div className="text-sm text-muted-foreground">Unable to load media details.</div>
  if (!displayed.displayedUrl)
    return <div className="text-muted-foreground">Failed to load media</div>
  if (displayed.isVideo) {
    return (
      <video
        ref={displayed.videoRef}
        src={displayed.displayedUrl}
        className="max-h-full max-w-full rounded-lg shadow-2xl"
        controls
        loop
        playsInline
        preload="metadata"
        onError={displayed.handleVideoError}
        onLoadedMetadata={displayed.restoreVideoTime}
      >
        <track kind="captions" />
      </video>
    )
  }
  return (
    <button
      type="button"
      className="flex h-full w-full items-center justify-center overflow-hidden"
      onDoubleClick={zoom.toggleZoom}
      onMouseDown={zoom.handleMouseDown}
      onMouseMove={zoom.handleMouseMove}
      onMouseUp={zoom.stopDragging}
      onMouseLeave={zoom.stopDragging}
      onKeyDown={zoom.handleKeyDown}
      aria-label="Toggle zoom"
    >
      <img
        src={displayed.displayedUrl}
        alt={media.originalFilename}
        className="max-h-full max-w-full select-none rounded-lg object-contain shadow-2xl"
        style={zoom.imageStyle}
        draggable={false}
      />
    </button>
  )
}

interface NavigationProps {
  currentIndex: number
  mediaCount: number
  onPrevious: () => void
  onNext: () => void
}

function LightboxNavigation({ currentIndex, mediaCount, onPrevious, onNext }: NavigationProps) {
  return (
    <>
      {currentIndex > 0 && (
        <button
          type="button"
          aria-label="Previous media"
          onClick={onPrevious}
          className="absolute left-4 top-1/2 z-10 -translate-y-1/2 rounded-full border border-border/10 bg-background/20 p-3 text-foreground backdrop-blur-md transition-colors hover:bg-background/40"
        >
          <ChevronLeft className="h-8 w-8" />
        </button>
      )}
      {currentIndex < mediaCount - 1 && (
        <button
          type="button"
          aria-label="Next media"
          onClick={onNext}
          className="absolute right-4 top-1/2 z-10 -translate-y-1/2 rounded-full border border-border/10 bg-background/20 p-3 text-foreground backdrop-blur-md transition-colors hover:bg-background/40"
        >
          <ChevronRight className="h-8 w-8" />
        </button>
      )}
    </>
  )
}

export default function Lightbox({
  mediaIds,
  currentIndex,
  onClose,
  onIndexChange,
}: LightboxProps) {
  const handleClose = useLightboxHistory(onClose)
  const mediaState = useLightboxMedia(mediaIds, currentIndex, onIndexChange)
  const goToPrevious = useCallback(() => {
    if (mediaState.safeIndex > 0) onIndexChange(mediaState.safeIndex - 1)
  }, [mediaState.safeIndex, onIndexChange])
  const goToNext = useCallback(() => {
    if (mediaState.safeIndex < mediaState.mediaList.length - 1)
      onIndexChange(mediaState.safeIndex + 1)
  }, [mediaState.mediaList.length, mediaState.safeIndex, onIndexChange])

  useEffect(() => {
    const handleKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') handleClose()
      if (event.key === 'ArrowLeft') goToPrevious()
      if (event.key === 'ArrowRight') goToNext()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [goToNext, goToPrevious, handleClose])

  if (!mediaState.currentMedia) {
    if (!mediaState.isLoading && !mediaState.hasError) return null
    return (
      <div className="absolute inset-0 z-[2000] flex items-center justify-center bg-background/95 backdrop-blur-sm">
        {mediaState.hasError ? (
          <p className="text-destructive">Unable to load media.</p>
        ) : (
          <Loader2 className="h-12 w-12 animate-spin text-muted-foreground" />
        )}
      </div>
    )
  }

  const content = (
    <div className="absolute inset-0 z-[2000] flex bg-background/95 backdrop-blur-sm">
      <div className="relative flex min-h-0 min-w-0 flex-1 items-center justify-center p-4">
        <button
          type="button"
          aria-label="Close viewer"
          onClick={handleClose}
          className="absolute right-4 top-4 z-50 rounded-full border border-border/10 bg-background/20 p-2 text-foreground backdrop-blur-md transition-colors hover:bg-background/40"
        >
          <X className="h-6 w-6" />
        </button>
        <LightboxNavigation
          currentIndex={mediaState.safeIndex}
          mediaCount={mediaState.mediaList.length}
          onPrevious={goToPrevious}
          onNext={goToNext}
        />
        <LightboxStage
          media={mediaState.currentMedia}
          metadataLoading={mediaState.isLoading}
          metadataError={mediaState.hasError}
        />
      </div>
      <aside className="h-full w-[320px] shrink-0 overflow-y-auto border-l border-border bg-card p-6">
        <MediaDetails
          media={mediaState.currentMedia}
          className="border-0 bg-transparent p-0 shadow-none"
        />
        <div className="mt-6 border-t border-border pt-6">
          <p className="text-center text-xs text-muted-foreground">
            {mediaState.safeIndex + 1} / {mediaState.mediaList.length}
          </p>
        </div>
      </aside>
    </div>
  )
  const portalTarget = document.getElementById('app-main')
  return portalTarget ? createPortal(content, portalTarget) : content
}

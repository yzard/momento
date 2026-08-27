import { useState, useRef, useCallback, useEffect, type KeyboardEvent } from 'react'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'
import { Check, Circle, Play } from 'lucide-react'
import { batchLoader } from '../../utils/batcher'
import { useMediaStreamURL } from '../../hooks/useMediaStreamURL'

export interface PhotoGridSelection {
  selectedMediaIds: ReadonlySet<number>
  toggleSelection: (mediaId: number) => void
}

interface PhotoGridProps {
  media: Media[]
  onPhotoClick: (media: Media) => void
  selection: PhotoGridSelection | null
}

function getFormatBadge(mimeType: string | null, filename: string, mediaType: 'image' | 'video'): string | null {
  const lowerMime = mimeType ? mimeType.toLowerCase() : ''
  const ext = filename.split('.').pop()?.toLowerCase() || ''

  if (mediaType === 'video') {
    if (lowerMime.includes('mp4') || ext === 'mp4') return 'MP4'
    if (lowerMime.includes('quicktime') || lowerMime.includes('mov') || ext === 'mov') return 'MOV'
    if (lowerMime.includes('webm') || ext === 'webm') return 'WEBM'
    if (lowerMime.includes('avi') || ext === 'avi') return 'AVI'
    if (lowerMime.includes('mkv') || ext === 'mkv') return 'MKV'
    return null
  }

  // Image formats
  if (lowerMime.includes('jpeg') || lowerMime.includes('jpg') || ext === 'jpg' || ext === 'jpeg') return 'JPG'
  if (lowerMime.includes('png') || ext === 'png') return 'PNG'
  if (lowerMime.includes('gif') || ext === 'gif') return 'GIF'
  if (lowerMime.includes('webp') || ext === 'webp') return 'WEBP'
  if (lowerMime.includes('heic') || lowerMime.includes('heif') || ext === 'heic' || ext === 'heif') return 'HEIC'
  if (lowerMime.includes('tiff') || ext === 'tiff' || ext === 'tif') return 'TIFF'
  if (lowerMime.includes('bmp') || ext === 'bmp') return 'BMP'
  if (lowerMime.includes('apng')) return 'APNG'
  if (lowerMime.includes('dng') || ext === 'dng') return 'RAW'
  if (lowerMime.includes('cr2') || ext === 'cr2') return 'RAW'
  if (lowerMime.includes('arw') || ext === 'arw') return 'RAW'
  if (lowerMime.includes('nef') || ext === 'nef') return 'RAW'

  return null
}

export default function PhotoGrid({ media, onPhotoClick, selection }: PhotoGridProps) {
  return (
    <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8 gap-3 px-1 pb-8">
      {media.map((item) => (
        <MediaItem
          key={item.id}
          item={item}
          onPhotoClick={onPhotoClick}
          selection={selection}
        />
      ))}
    </div>
  )
}

interface MediaItemProps {
  item: Media
  onPhotoClick: (media: Media) => void
  selection: PhotoGridSelection | null
}

function MediaItem({ item, onPhotoClick, selection }: MediaItemProps) {
  const [isHovering, setIsHovering] = useState(false)
  const [showVideo, setShowVideo] = useState(false)
  const [isVideoPlaying, setIsVideoPlaying] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(() => 
    mediaApi.getCachedThumbnailUrl(item.id) || null
  )
  const videoRef = useRef<HTMLVideoElement>(null)
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)

  const isVideo = item.mediaType === 'video'
  const isGif = item.mimeType?.toLowerCase().includes('gif') || item.originalFilename.toLowerCase().endsWith('.gif')
  const shouldPreview = isVideo || isGif
  const selectionMode = selection !== null
  const selected = selection?.selectedMediaIds.has(item.id) ?? false
  const formatBadge = getFormatBadge(item.mimeType, item.originalFilename, item.mediaType)
  const {
    streamURL: previewUrl,
    retryStreamOnce,
  } = useMediaStreamURL(shouldPreview ? item.id : null, showVideo)

  // Load thumbnail with IntersectionObserver for lazy loading
  useEffect(() => {
    if (thumbnailUrl || !containerRef.current) return

    let cancelled = false

    const loadThumbnail = async () => {
      try {
        const url = await batchLoader.load(item.id)
        if (!cancelled && url) setThumbnailUrl(url)
      } catch {
        console.error('Failed to load thumbnail')
      }
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          loadThumbnail()
          observer.disconnect()
        }
      },
      { rootMargin: '400px' }
    )
    observer.observe(containerRef.current)

    return () => {
      cancelled = true
      observer.disconnect()
    }
  }, [item.id, thumbnailUrl])

  const handleMouseEnter = useCallback(() => {
    setIsHovering(true)
    if (shouldPreview) {
      hoverTimeoutRef.current = setTimeout(() => {
        setShowVideo(true)
      }, 500)
    }
  }, [shouldPreview])

  const handleMouseLeave = useCallback(() => {
    setIsHovering(false)
    setShowVideo(false)
    setIsVideoPlaying(false)
    setCurrentTime(0)
    if (hoverTimeoutRef.current) {
      clearTimeout(hoverTimeoutRef.current)
      hoverTimeoutRef.current = null
    }
    if (videoRef.current) {
      videoRef.current.pause()
      videoRef.current.currentTime = 0
    }
  }, [])

  const handleVideoTimeUpdate = useCallback(() => {
    if (videoRef.current) {
      setCurrentTime(videoRef.current.currentTime)
      if (videoRef.current.currentTime >= 10) {
        videoRef.current.currentTime = 0
      }
    }
  }, [])

  const activateMedia = useCallback(() => {
    if (selection) {
      selection.toggleSelection(item.id)
      return
    }
    onPhotoClick(item)
  }, [item, onPhotoClick, selection])

  const handleKeyDown = useCallback((event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    activateMedia()
  }, [activateMedia])

  return (
    <div
      ref={containerRef}
      role="button"
      tabIndex={0}
      aria-pressed={selectionMode ? selected : undefined}
      aria-label={selectionMode ? `${selected ? 'Deselect' : 'Select'} ${item.originalFilename}` : `Open ${item.originalFilename}`}
      className={`aspect-square relative cursor-pointer group overflow-hidden bg-muted rounded-lg outline-none transition-[box-shadow,transform] duration-200 motion-reduce:transition-none ${
        selected
          ? 'z-10 ring-4 ring-primary ring-offset-2 ring-offset-background shadow-lg'
          : selectionMode
            ? 'hover:ring-2 hover:ring-primary/50 active:scale-[0.98]'
            : 'hover:z-10 hover:ring-2 hover:ring-background hover:shadow-lg active:scale-[0.98]'
      }`}
      onClick={activateMedia}
      onKeyDown={handleKeyDown}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      {thumbnailUrl ? (
        <img
          src={thumbnailUrl}
          alt={item.originalFilename}
          className={`w-full h-full object-cover transition-opacity duration-200 motion-reduce:transition-none ${isVideoPlaying ? 'opacity-0' : 'opacity-100'}`}
        />
      ) : (
        <div className="h-full w-full animate-pulse bg-muted" aria-hidden="true" />
      )}

      {selectionMode && (
        <div className={`absolute inset-0 transition-colors duration-150 motion-reduce:transition-none ${selected ? 'bg-primary/25' : 'bg-black/10'}`}>
          <span className={`absolute left-2 top-2 flex h-8 w-8 items-center justify-center rounded-full border-2 shadow-sm transition-colors duration-150 ${
            selected
              ? 'border-primary bg-primary text-primary-foreground'
              : 'border-white/90 bg-black/35 text-white'
          }`}>
            {selected ? <Check className="h-4 w-4" strokeWidth={3} /> : <Circle className="h-4 w-4" />}
          </span>
        </div>
      )}

      {isHovering && showVideo && shouldPreview && previewUrl && (
        isGif ? (
          <img
            src={previewUrl}
            alt={item.originalFilename}
            className="absolute inset-0 w-full h-full object-cover"
            onError={retryStreamOnce}
          />
        ) : (
          <video
            ref={videoRef}
            src={previewUrl}
            className="absolute inset-0 w-full h-full object-cover"
            autoPlay
            muted
            loop={false}
            playsInline
            onPlay={() => setIsVideoPlaying(true)}
            onTimeUpdate={handleVideoTimeUpdate}
            onError={retryStreamOnce}
          />
        )
      )}

      {isVideo && (!isHovering || !showVideo) && (
        <div className="absolute top-1/2 left-1/2 transform -translate-x-1/2 -translate-y-1/2 bg-black/40 backdrop-blur-md text-white p-3 rounded-full opacity-0 group-hover:opacity-100 transition-opacity border border-white/20">
          <Play className="w-6 h-6 fill-white ml-0.5" strokeWidth={1.5} />
        </div>
      )}

      {/* Format badge - top right */}
      {formatBadge && !isVideo && (
        <div className="absolute top-2 right-2 bg-black/60 backdrop-blur-sm text-white text-[10px] font-bold px-2 py-1 rounded-md uppercase tracking-wider border border-white/10">
          {formatBadge}
        </div>
      )}

      {/* Video duration badge - top right for videos */}
      {isVideo && (
        <div className="absolute top-2 right-2 bg-black/60 backdrop-blur-sm text-white text-[10px] font-bold px-2 py-1 rounded-md uppercase tracking-wider border border-white/10">
          {(isHovering && showVideo)
            ? formatDuration(currentTime)
            : (item.durationSeconds
              ? formatDuration(item.durationSeconds)
              : '0:00')}
        </div>
      )}

    </div>
  )
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

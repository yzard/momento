import { useState, useRef, useCallback } from 'react'
import { mediaApi } from '../../api/media'
import type { Media } from '../../api/types'
import { Play, Plus, Trash2 } from 'lucide-react'

interface PhotoGridProps {
  media: Media[]
  onPhotoClick: (media: Media) => void
  onAddToAlbum?: (media: Media) => void
  onDelete?: (media: Media) => void
}

const ANIMATED_FORMATS = ['gif', 'apng', 'webp']

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

function isAnimatedFormat(mimeType: string | null): boolean {
  if (!mimeType) return false
  const lower = mimeType.toLowerCase()
  return ANIMATED_FORMATS.some(fmt => lower.includes(fmt))
}

export default function PhotoGrid({ media, onPhotoClick, onAddToAlbum, onDelete }: PhotoGridProps) {
  return (
    <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8 gap-3 px-1 pb-8">
      {media.map((item) => (
        <MediaItem
          key={item.id}
          item={item}
          onPhotoClick={onPhotoClick}
          onAddToAlbum={onAddToAlbum}
          onDelete={onDelete}
        />
      ))}
    </div>
  )
}

interface MediaItemProps {
  item: Media
  onPhotoClick: (media: Media) => void
  onAddToAlbum?: (media: Media) => void
  onDelete?: (media: Media) => void
}

function MediaItem({ item, onPhotoClick, onAddToAlbum, onDelete }: MediaItemProps) {
  const [isHovering, setIsHovering] = useState(false)
  const [showVideo, setShowVideo] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const videoRef = useRef<HTMLVideoElement>(null)
  const hoverTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const isVideo = item.mediaType === 'video'
  const isAnimated = isAnimatedFormat(item.mimeType)
  const shouldPreview = isVideo || isAnimated
  const formatBadge = getFormatBadge(item.mimeType, item.originalFilename, item.mediaType)

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

  const handleActionClick = useCallback((e: React.MouseEvent, action: () => void) => {
    e.stopPropagation()
    action()
  }, [])

  return (
    <div
      className="aspect-square relative cursor-pointer group overflow-hidden bg-muted rounded-lg transition-all duration-300 hover:z-10 hover:ring-4 hover:ring-background hover:shadow-xl hover:scale-105"
      onClick={() => onPhotoClick(item)}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      <img
        src={mediaApi.getThumbnailUrl(item.id)}
        alt={item.originalFilename}
        className={`w-full h-full object-cover transition-transform duration-700 group-hover:scale-105 ${showVideo ? 'opacity-0' : 'opacity-100'}`}
        loading="lazy"
      />

      {isHovering && showVideo && shouldPreview && (
        <video
          ref={videoRef}
          src={mediaApi.getPreviewUrl(item.id)}
          className="absolute inset-0 w-full h-full object-cover"
          autoPlay
          muted
          loop={false}
          playsInline
          onTimeUpdate={handleVideoTimeUpdate}
        />
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
            : (item.durationSeconds || (item as any).duration_seconds
              ? formatDuration(item.durationSeconds || (item as any).duration_seconds)
              : '0:00')}
        </div>
      )}

      {/* Hover action buttons - bottom */}
      {isHovering && (onAddToAlbum || onDelete) && (
        <div className="absolute bottom-2 left-1/2 transform -translate-x-1/2 flex gap-2 opacity-0 group-hover:opacity-100 transition-opacity">
          {onAddToAlbum && (
            <button
              onClick={(e) => handleActionClick(e, () => onAddToAlbum(item))}
              className="p-2 rounded-full bg-black/50 backdrop-blur-sm text-white hover:bg-black/70 transition-colors border border-white/20"
              title="Add to album"
            >
              <Plus className="w-4 h-4" />
            </button>
          )}
          {onDelete && (
            <button
              onClick={(e) => handleActionClick(e, () => onDelete(item))}
              className="p-2 rounded-full bg-black/50 backdrop-blur-sm text-white hover:bg-red-500/70 transition-colors border border-white/20"
              title="Move to trash"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          )}
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


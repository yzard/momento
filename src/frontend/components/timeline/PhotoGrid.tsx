import { useState, useRef, useCallback, type KeyboardEvent } from 'react'
import type { Media } from '../../api/types'
import { Play } from 'lucide-react'
import { thumbnailUrlLoader } from '../../utils/assetUrlLoader'
import { useMediaStreamURL } from '../../hooks/useMediaStreamURL'
import { useLazyImage } from '../../hooks/useLazyImage'
import { mediaFormatBadge } from '../../lib/mediaFormat'
import MediaSelectionOverlay from '../media/MediaSelectionOverlay'

export interface PhotoGridSelection {
  selectedMediaIds: ReadonlySet<number>
  toggleSelection: (mediaId: number) => void
}

interface PhotoGridProps {
  media: Media[]
  onPhotoClick: (media: Media) => void
  selection: PhotoGridSelection | null
}

export default function PhotoGrid({ media, onPhotoClick, selection }: PhotoGridProps) {
  return (
    <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8 gap-3 px-1 pb-8">
      {media.map((item) => (
        <MediaItem key={item.id} item={item} onPhotoClick={onPhotoClick} selection={selection} />
      ))}
    </div>
  )
}

interface MediaItemProps {
  item: Media
  onPhotoClick: (media: Media) => void
  selection: PhotoGridSelection | null
}

interface MediaItemContentProps {
  item: Media
  thumbnailUrl: string | null
  previewUrl: string | null
  videoReference: React.RefObject<HTMLVideoElement>
  selectionMode: boolean
  selected: boolean
  isHovering: boolean
  showPreview: boolean
  isVideo: boolean
  isGif: boolean
  isVideoPlaying: boolean
  formatBadge: string | null
  currentTime: number
  onVideoPlay: () => void
  onVideoTimeUpdate: () => void
  onPreviewError: () => void
}

function MediaThumbnail({
  filename,
  thumbnailUrl,
  hidden,
}: {
  filename: string
  thumbnailUrl: string | null
  hidden: boolean
}) {
  if (!thumbnailUrl) {
    return <div className="h-full w-full animate-pulse bg-muted" aria-hidden="true" />
  }

  return (
    <img
      src={thumbnailUrl}
      alt={filename}
      className={`h-full w-full object-cover transition-opacity duration-200 motion-reduce:transition-none ${hidden ? 'opacity-0' : 'opacity-100'}`}
    />
  )
}

function HoverPreview({
  filename,
  previewUrl,
  isGif,
  videoReference,
  onVideoPlay,
  onVideoTimeUpdate,
  onPreviewError,
}: Pick<
  MediaItemContentProps,
  'previewUrl' | 'isGif' | 'videoReference' | 'onVideoPlay' | 'onVideoTimeUpdate' | 'onPreviewError'
> & { filename: string }) {
  if (!previewUrl) return null
  if (isGif) {
    return (
      <img
        src={previewUrl}
        alt={filename}
        className="absolute inset-0 h-full w-full object-cover"
        onError={onPreviewError}
      />
    )
  }

  return (
    <video
      ref={videoReference}
      src={previewUrl}
      className="absolute inset-0 h-full w-full object-cover"
      autoPlay
      muted
      playsInline
      onPlay={onVideoPlay}
      onTimeUpdate={onVideoTimeUpdate}
      onError={onPreviewError}
    />
  )
}

function FormatBadge({
  item,
  formatBadge,
  isVideo,
  previewVisible,
  currentTime,
}: Pick<MediaItemContentProps, 'item' | 'formatBadge' | 'isVideo' | 'currentTime'> & {
  previewVisible: boolean
}) {
  if (isVideo) {
    const displayedDuration = previewVisible ? currentTime : (item.durationSeconds ?? 0)
    return (
      <div className="absolute right-2 top-2 rounded-md border border-white/10 bg-black/60 px-2 py-1 text-[10px] font-bold uppercase tracking-wider text-white backdrop-blur-sm">
        {formatDuration(displayedDuration)}
      </div>
    )
  }
  if (!formatBadge) return null

  return (
    <div className="absolute right-2 top-2 rounded-md border border-white/10 bg-black/60 px-2 py-1 text-[10px] font-bold uppercase tracking-wider text-white backdrop-blur-sm">
      {formatBadge}
    </div>
  )
}

function MediaItemContent(props: MediaItemContentProps) {
  const previewVisible = props.isHovering && props.showPreview

  return (
    <>
      <MediaThumbnail
        filename={props.item.originalFilename}
        thumbnailUrl={props.thumbnailUrl}
        hidden={props.isVideoPlaying}
      />
      {props.selectionMode && <MediaSelectionOverlay selected={props.selected} />}
      {previewVisible && (
        <HoverPreview
          filename={props.item.originalFilename}
          previewUrl={props.previewUrl}
          isGif={props.isGif}
          videoReference={props.videoReference}
          onVideoPlay={props.onVideoPlay}
          onVideoTimeUpdate={props.onVideoTimeUpdate}
          onPreviewError={props.onPreviewError}
        />
      )}
      {props.isVideo && !previewVisible && (
        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full border border-white/20 bg-black/40 p-3 text-white opacity-0 backdrop-blur-md transition-opacity group-hover:opacity-100">
          <Play className="ml-0.5 h-6 w-6 fill-white" strokeWidth={1.5} />
        </div>
      )}
      <FormatBadge
        item={props.item}
        formatBadge={props.formatBadge}
        isVideo={props.isVideo}
        previewVisible={previewVisible}
        currentTime={props.currentTime}
      />
    </>
  )
}

function useHoverPreview(item: Media, shouldPreview: boolean) {
  const [isHovering, setIsHovering] = useState(false)
  const [showPreview, setShowPreview] = useState(false)
  const [isVideoPlaying, setIsVideoPlaying] = useState(false)
  const [currentTime, setCurrentTime] = useState(0)
  const videoReference = useRef<HTMLVideoElement>(null)
  const hoverDelayReference = useRef<ReturnType<typeof setTimeout> | null>(null)
  const { streamURL: previewUrl, retryStreamOnce } = useMediaStreamURL(
    shouldPreview ? item.id : null,
    showPreview
  )

  const handleMouseEnter = useCallback(() => {
    setIsHovering(true)
    if (!shouldPreview) return
    hoverDelayReference.current = setTimeout(() => setShowPreview(true), 500)
  }, [shouldPreview])

  const handleMouseLeave = useCallback(() => {
    setIsHovering(false)
    setShowPreview(false)
    setIsVideoPlaying(false)
    setCurrentTime(0)
    if (hoverDelayReference.current) clearTimeout(hoverDelayReference.current)
    hoverDelayReference.current = null
    if (!videoReference.current) return
    videoReference.current.pause()
    videoReference.current.currentTime = 0
  }, [])

  const handleVideoTimeUpdate = useCallback(() => {
    if (!videoReference.current) return
    setCurrentTime(videoReference.current.currentTime)
    if (videoReference.current.currentTime >= 10) videoReference.current.currentTime = 0
  }, [])

  return {
    isHovering,
    showPreview,
    isVideoPlaying,
    currentTime,
    videoReference,
    previewUrl,
    retryStreamOnce,
    handleMouseEnter,
    handleMouseLeave,
    handleVideoTimeUpdate,
    markVideoPlaying: () => setIsVideoPlaying(true),
  }
}

function MediaItem({ item, onPhotoClick, selection }: MediaItemProps) {
  const { targetRef: containerRef, imageUrl: thumbnailUrl } = useLazyImage<HTMLDivElement, number>({
    resourceId: item.id,
    loader: thumbnailUrlLoader,
    getCachedUrl: null,
    rootMargin: '400px',
  })

  const isVideo = item.mediaType === 'video'
  const isGif =
    item.mimeType?.toLowerCase().includes('gif') ||
    item.originalFilename.toLowerCase().endsWith('.gif')
  const shouldPreview = isVideo || isGif
  const selectionMode = selection !== null
  const selected = selection?.selectedMediaIds.has(item.id) ?? false
  const formatBadge = mediaFormatBadge(item.mimeType, item.originalFilename, item.mediaType)
  const preview = useHoverPreview(item, shouldPreview)

  const activateMedia = useCallback(() => {
    if (selection) {
      selection.toggleSelection(item.id)
      return
    }
    onPhotoClick(item)
  }, [item, onPhotoClick, selection])

  const handleKeyDown = useCallback(
    (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key !== 'Enter' && event.key !== ' ') return
      event.preventDefault()
      activateMedia()
    },
    [activateMedia]
  )

  return (
    <div
      ref={containerRef}
      role="button"
      tabIndex={0}
      aria-pressed={selectionMode ? selected : undefined}
      aria-label={
        selectionMode
          ? `${selected ? 'Deselect' : 'Select'} ${item.originalFilename}`
          : `Open ${item.originalFilename}`
      }
      className={`aspect-square relative cursor-pointer group overflow-hidden bg-muted rounded-lg outline-none transition-[box-shadow,transform] duration-200 motion-reduce:transition-none ${
        selected
          ? 'z-10 ring-4 ring-primary ring-offset-2 ring-offset-background shadow-lg'
          : selectionMode
            ? 'hover:ring-2 hover:ring-primary/50 active:scale-[0.98]'
            : 'hover:z-10 hover:ring-2 hover:ring-background hover:shadow-lg active:scale-[0.98]'
      }`}
      onClick={activateMedia}
      onKeyDown={handleKeyDown}
      onMouseEnter={preview.handleMouseEnter}
      onMouseLeave={preview.handleMouseLeave}
    >
      <MediaItemContent
        item={item}
        thumbnailUrl={thumbnailUrl}
        previewUrl={preview.previewUrl}
        videoReference={preview.videoReference}
        selectionMode={selectionMode}
        selected={selected}
        isHovering={preview.isHovering}
        showPreview={preview.showPreview && shouldPreview}
        isVideo={isVideo}
        isGif={isGif}
        isVideoPlaying={preview.isVideoPlaying}
        formatBadge={formatBadge}
        currentTime={preview.currentTime}
        onVideoPlay={preview.markVideoPlaying}
        onVideoTimeUpdate={preview.handleVideoTimeUpdate}
        onPreviewError={preview.retryStreamOnce}
      />
    </div>
  )
}

function formatDuration(seconds: number): string {
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

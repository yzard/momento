import { useState, useEffect, useRef } from 'react'
import { mediaApi } from '../../api/media'

interface MediaThumbnailProps {
  mediaId: number
  alt: string
  className?: string
  loading?: 'lazy' | 'eager'
}

export function MediaThumbnail({ mediaId, alt, className = '', loading = 'lazy' }: MediaThumbnailProps) {
  const [url, setUrl] = useState<string | null>(null)
  const [error, setError] = useState(false)
  const imgRef = useRef<HTMLImageElement>(null)

  useEffect(() => {
    let cancelled = false

    const loadUrl = async () => {
      try {
        const blobUrl = await mediaApi.getThumbnailUrl(mediaId)
        if (!cancelled) {
          setUrl(blobUrl)
        }
      } catch (err) {
        if (!cancelled) {
          setError(true)
        }
      }
    }

    // Use IntersectionObserver for lazy loading
    if (loading === 'lazy' && imgRef.current) {
      const observer = new IntersectionObserver(
        (entries) => {
          if (entries[0]?.isIntersecting) {
            loadUrl()
            observer.disconnect()
          }
        },
        { rootMargin: '100px' }
      )
      observer.observe(imgRef.current)
      return () => {
        cancelled = true
        observer.disconnect()
      }
    } else {
      loadUrl()
      return () => {
        cancelled = true
      }
    }
  }, [mediaId, loading])

  if (error) {
    return (
      <div ref={imgRef} className={`bg-muted flex items-center justify-center ${className}`}>
        <span className="text-muted-foreground text-xs">Error</span>
      </div>
    )
  }

  if (!url) {
    return (
      <div ref={imgRef} className={`bg-muted animate-pulse ${className}`} />
    )
  }

  return (
    <img
      ref={imgRef}
      src={url}
      alt={alt}
      className={className}
    />
  )
}

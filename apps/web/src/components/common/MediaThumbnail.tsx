import { useState, useEffect, useRef } from 'react'
import { mediaApi } from '../../api/media'

interface MediaThumbnailProps {
  mediaId: number
  alt: string
  className?: string
  loading?: 'lazy' | 'eager'
}

export function MediaThumbnail({ mediaId, alt, className = '', loading = 'lazy' }: MediaThumbnailProps) {
  const [url, setUrl] = useState<string | null>(() => 
    mediaApi.getCachedThumbnailUrl(mediaId) || null
  )
  const [error, setError] = useState(false)
  const imgRef = useRef<HTMLImageElement>(null)

  useEffect(() => {
    if (url) return
    
    let cancelled = false

    const loadUrl = async () => {
      try {
        const batch = await mediaApi.getThumbnailBatch([mediaId])
        const blobUrl = batch.get(mediaId) ?? null
        if (!cancelled) {
          setUrl(blobUrl)
        }
      } catch {
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
  }, [mediaId, loading, url])

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

import { useState, useEffect } from 'react'
import { mediaApi } from '../api/media'

type MediaUrlType = 'thumbnail' | 'preview' | 'file'

export function useMediaUrl(mediaId: number, type: MediaUrlType): string | null {
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    const loadUrl = async () => {
      try {
        let blobUrl: string
        switch (type) {
          case 'thumbnail':
            blobUrl = await mediaApi.getThumbnailUrl(mediaId)
            break
          case 'preview':
            blobUrl = await mediaApi.getPreviewUrl(mediaId)
            break
          case 'file':
            blobUrl = await mediaApi.getFileUrl(mediaId)
            break
        }
        if (!cancelled) {
          setUrl(blobUrl)
        }
      } catch (err) {
        console.error(`Failed to load ${type} for media ${mediaId}:`, err)
      }
    }

    loadUrl()

    return () => {
      cancelled = true
    }
  }, [mediaId, type])

  return url
}

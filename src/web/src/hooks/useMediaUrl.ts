import { useState, useEffect } from 'react'
import { mediaApi } from '../api/media'

type MediaUrlType = 'thumbnail' | 'preview' | 'file'

export function useMediaUrl(mediaId: number, type: MediaUrlType): string | null {
  const [url, setUrl] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false

    const loadUrl = async () => {
      try {
        let blobUrl: string | null = null
        switch (type) {
          case 'thumbnail': {
            const batch = await mediaApi.getThumbnailBatch([mediaId])
            blobUrl = batch.get(mediaId) ?? null
            break
          }
          case 'preview': {
            const batch = await mediaApi.getPreviewBatch([mediaId])
            blobUrl = batch.get(mediaId) ?? null
            break
          }
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

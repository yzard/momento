import { useCallback, useEffect, useRef, useState } from 'react'

import { mediaApi } from '../api/media'

interface MediaStreamURLState {
  streamURL: string | null
  isStreamLoading: boolean
  retryStreamOnce: () => void
}

export function useMediaStreamURL(mediaId: number | null, shouldLoad: boolean): MediaStreamURLState {
  const [streamURL, setStreamURL] = useState<string | null>(null)
  const [isStreamLoading, setIsStreamLoading] = useState(false)
  const streamURLRef = useRef<string | null>(null)
  const requestGenerationRef = useRef(0)
  const automaticRetryUsedRef = useRef(false)

  const requestStreamURL = useCallback(async () => {
    if (mediaId === null) return

    const requestGeneration = ++requestGenerationRef.current
    setIsStreamLoading(true)
    try {
      const url = await mediaApi.getFileStreamURL(mediaId)
      if (requestGeneration !== requestGenerationRef.current) return
      streamURLRef.current = url
      setStreamURL(url)
    } catch {
      if (requestGeneration !== requestGenerationRef.current) return
      streamURLRef.current = null
      setStreamURL(null)
    } finally {
      if (requestGeneration === requestGenerationRef.current) setIsStreamLoading(false)
    }
  }, [mediaId])

  useEffect(() => {
    requestGenerationRef.current += 1
    automaticRetryUsedRef.current = false
    streamURLRef.current = null
    setStreamURL(null)
    setIsStreamLoading(false)
  }, [mediaId])

  useEffect(() => {
    if (!shouldLoad || mediaId === null || streamURLRef.current) return
    void requestStreamURL()
  }, [mediaId, requestStreamURL, shouldLoad])

  const retryStreamOnce = useCallback(() => {
    if (mediaId === null || automaticRetryUsedRef.current) return
    automaticRetryUsedRef.current = true
    streamURLRef.current = null
    setStreamURL(null)
    void requestStreamURL()
  }, [mediaId, requestStreamURL])

  return { streamURL, isStreamLoading, retryStreamOnce }
}

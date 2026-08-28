import { useCallback, useState } from 'react'

export interface LightboxState {
  mediaIds: number[]
  currentIndex: number
}

export interface LightboxController {
  state: LightboxState | null
  open: (mediaId: number, mediaIds: readonly number[]) => void
  openAtIndex: (mediaIds: readonly number[], currentIndex: number) => void
  close: () => void
  setCurrentIndex: (currentIndex: number) => void
}

export function useLightbox(): LightboxController {
  const [state, setState] = useState<LightboxState | null>(null)

  const openAtIndex = useCallback((mediaIds: readonly number[], currentIndex: number) => {
    if (mediaIds.length === 0) return
    const boundedIndex = Math.min(Math.max(currentIndex, 0), mediaIds.length - 1)
    setState({ mediaIds: [...mediaIds], currentIndex: boundedIndex })
  }, [])

  const open = useCallback(
    (mediaId: number, mediaIds: readonly number[]) => {
      const currentIndex = mediaIds.indexOf(mediaId)
      openAtIndex(mediaIds, currentIndex >= 0 ? currentIndex : 0)
    },
    [openAtIndex]
  )

  const close = useCallback(() => setState(null), [])
  const setCurrentIndex = useCallback((currentIndex: number) => {
    setState((current) => {
      if (!current) return null
      const boundedIndex = Math.min(Math.max(currentIndex, 0), current.mediaIds.length - 1)
      return { ...current, currentIndex: boundedIndex }
    })
  }, [])

  return { state, open, openAtIndex, close, setCurrentIndex }
}

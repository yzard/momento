import { useCallback, useState } from 'react'

export function toggleSelectedMediaId(selectedMediaIds: Set<number>, mediaId: number): Set<number> {
  const nextSelectedMediaIds = new Set(selectedMediaIds)
  if (nextSelectedMediaIds.has(mediaId)) {
    nextSelectedMediaIds.delete(mediaId)
  } else {
    nextSelectedMediaIds.add(mediaId)
  }
  return nextSelectedMediaIds
}

export function useMediaSelection() {
  const [selectionMode, setSelectionMode] = useState(false)
  const [selectedMediaIds, setSelectedMediaIds] = useState<Set<number>>(() => new Set())

  const startSelection = useCallback(() => {
    setSelectionMode(true)
  }, [])

  const clearSelection = useCallback(() => {
    setSelectedMediaIds(new Set())
  }, [])

  const cancelSelection = useCallback(() => {
    setSelectionMode(false)
    setSelectedMediaIds(new Set())
  }, [])

  const toggleSelection = useCallback((mediaId: number) => {
    setSelectedMediaIds((currentSelectedMediaIds) =>
      toggleSelectedMediaId(currentSelectedMediaIds, mediaId)
    )
  }, [])

  return {
    selectionMode,
    selectedMediaIds,
    startSelection,
    clearSelection,
    cancelSelection,
    toggleSelection,
  }
}

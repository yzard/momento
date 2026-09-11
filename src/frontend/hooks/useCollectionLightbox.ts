import { useCallback, useEffect } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { useLightbox } from './useLightbox'

export function useCollectionLightbox() {
  const controller = useLightbox()
  const { close: closeViewer, openAtIndex } = controller
  const location = useLocation()
  const navigate = useNavigate()
  const viewer = location.state?.collectionViewer as
    { mediaIds: number[]; currentIndex: number } | undefined
  const active = Boolean(viewer)
  const close = useCallback(() => {
    if (active) navigate(-1)
    else closeViewer()
  }, [active, navigate, closeViewer])
  useEffect(() => {
    if (!viewer) closeViewer()
    else openAtIndex(viewer.mediaIds, viewer.currentIndex)
  }, [viewer, closeViewer, openAtIndex])
  return {
    ...controller,
    manageHistory: false,
    close,
    open: (id: number, ids: readonly number[]) => {
      if (!active)
        navigate(location.pathname, {
          state: {
            ...location.state,
            collectionViewer: { mediaIds: [...ids], currentIndex: Math.max(0, ids.indexOf(id)) },
          },
        })
      controller.open(id, ids)
    },
  }
}

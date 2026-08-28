import { useEffect, useRef, useState, type RefObject } from 'react'

export interface LazyImageLoader<ResourceId> {
  load: (resourceId: ResourceId) => Promise<string | null | undefined>
}

interface LazyImageOptions<ResourceId> {
  resourceId: ResourceId
  loader: LazyImageLoader<ResourceId>
  getCachedUrl: ((resourceId: ResourceId) => string | null | undefined) | null
  rootMargin: string
}

interface LazyImageResult<ElementType extends HTMLElement> {
  targetRef: RefObject<ElementType>
  imageUrl: string | null
}

export function useLazyImage<ElementType extends HTMLElement, ResourceId>(
  options: LazyImageOptions<ResourceId>
): LazyImageResult<ElementType> {
  const { resourceId, loader, getCachedUrl, rootMargin } = options
  const targetRef = useRef<ElementType>(null)
  const [loadedImage, setLoadedImage] = useState<{ resourceId: ResourceId; url: string } | null>(
    null
  )
  const cachedUrl = getCachedUrl?.(resourceId) ?? null
  const imageUrl = cachedUrl ?? (loadedImage?.resourceId === resourceId ? loadedImage.url : null)

  useEffect(() => {
    if (cachedUrl || !targetRef.current) return
    let active = true
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries[0]?.isIntersecting) return
        observer.disconnect()
        void loader
          .load(resourceId)
          .then((url) => {
            if (active && url) setLoadedImage({ resourceId, url })
          })
          .catch(() => undefined)
      },
      { rootMargin }
    )

    observer.observe(targetRef.current)
    return () => {
      active = false
      observer.disconnect()
    }
  }, [cachedUrl, loader, resourceId, rootMargin])

  return { targetRef, imageUrl }
}

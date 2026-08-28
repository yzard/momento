const THUMBNAIL_SIZE = 52

export const clusterIconSize = THUMBNAIL_SIZE

export function createClusterIconElement(
  thumbnailUrl: string | null,
  count: number
): HTMLDivElement {
  const marker = document.createElement('div')
  marker.className = 'map-marker'
  marker.style.position = 'relative'

  const bubble = document.createElement('div')
  bubble.className = 'map-marker__bubble'
  bubble.style.width = `${THUMBNAIL_SIZE}px`
  bubble.style.height = `${THUMBNAIL_SIZE}px`
  if (thumbnailUrl) {
    const image = document.createElement('img')
    image.className = 'map-marker__image'
    image.alt = ''
    image.draggable = false
    image.src = thumbnailUrl
    bubble.append(image)
  } else {
    const placeholder = document.createElement('div')
    placeholder.className = 'map-marker__placeholder'
    bubble.append(placeholder)
  }
  marker.append(bubble)

  if (count > 1) {
    const badge = document.createElement('span')
    badge.className = 'map-marker__badge'
    badge.textContent = String(count)
    marker.append(badge)
  }
  return marker
}

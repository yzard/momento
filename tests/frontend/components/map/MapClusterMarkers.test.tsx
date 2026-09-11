import { useLayoutEffect, useRef } from 'react'
import { cleanup, fireEvent, render } from '@testing-library/react'
import type { DivIcon } from 'leaflet'
import { afterEach, describe, expect, it, vi } from 'vitest'
import MapClusterMarkers from '../../../../src/frontend/components/map/MapClusterMarkers'
import type { MapCluster } from '../../../../src/frontend/hooks/useMapClusters'

vi.mock('../../../../src/frontend/api/media', () => ({
  mediaApi: { getThumbnailURL: (id: number) => `/media/${id}/thumbnail/tiny` },
}))
vi.mock('../../../../src/frontend/node_modules/react-leaflet/lib/index.js', () => ({
  Marker: function Marker({
    icon,
    position,
    eventHandlers,
  }: {
    icon: DivIcon
    position: number[]
    eventHandlers: { click: () => void }
  }) {
    const ref = useRef<HTMLDivElement>(null)
    useLayoutEffect(() => {
      ref.current?.replaceChildren(icon.options.html as HTMLElement)
    }, [icon])
    return (
      <div
        ref={ref}
        data-testid="marker"
        data-position={position.join(',')}
        onClick={eventHandlers.click}
      />
    )
  },
}))
afterEach(cleanup)

describe('MapClusterMarkers', () => {
  it('reuses the marker and image across zoom levels while updating count, position and selection', () => {
    const first: MapCluster = {
      geometry: { coordinates: [5, 52] },
      properties: { representativeId: 10, cellId: 'u4', count: 8 },
    }
    const onClick = vi.fn()
    const { container, getByTestId, rerender } = render(
      <MapClusterMarkers clusters={[first]} onClusterClick={onClick} />
    )
    const marker = getByTestId('marker')
    const image = container.querySelector('img')
    expect(image?.getAttribute('src')).toBe('/media/10/thumbnail/tiny')
    expect(container.querySelector('.map-marker__placeholder')).toBeNull()
    const next: MapCluster = {
      geometry: { coordinates: [5.1, 52.1] },
      properties: { representativeId: 10, cellId: 'u40:2', count: 4 },
    }
    rerender(<MapClusterMarkers clusters={[next]} onClusterClick={onClick} />)
    expect(getByTestId('marker')).toBe(marker)
    expect(container.querySelector('img')).toBe(image)
    expect(container.querySelector('.map-marker__badge')?.textContent).toBe('4')
    expect(marker.getAttribute('data-position')).toBe('52.1,5.1')
    fireEvent.click(marker)
    expect(onClick).toHaveBeenLastCalledWith(next)
    rerender(
      <MapClusterMarkers
        clusters={[{ ...next, properties: { ...next.properties, count: 1 } }]}
        onClusterClick={onClick}
      />
    )
    expect(container.querySelector('img')).toBe(image)
    expect(container.querySelector('.map-marker__badge')).toBeNull()
    rerender(
      <MapClusterMarkers
        clusters={[{ ...next, properties: { ...next.properties, representativeId: 20 } }]}
        onClusterClick={onClick}
      />
    )
    expect(container.querySelector('img')).not.toBe(image)
    expect(container.querySelector('img')?.getAttribute('src')).toBe('/media/20/thumbnail/tiny')
  })
})

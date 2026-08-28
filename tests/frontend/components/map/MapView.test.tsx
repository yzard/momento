import { cleanup, render, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.hoisted(() => {
  Object.defineProperty(globalThis.SVGSVGElement.prototype, 'createSVGRect', {
    configurable: true,
    value: () => ({}),
  })
})

vi.mock('../../../../src/frontend/hooks/useMapClusters', () => ({
  useMapClusters: () => ({
    clusters: [],
    isLoading: false,
    error: null,
    supercluster: {},
  }),
}))

vi.mock('../../../../src/frontend/components/map/ClusterMarker', () => ({
  default: () => null,
}))

import MapView from '../../../../src/frontend/components/map/MapView'

describe('MapView', () => {
  afterEach(() => {
    cleanup()
    sessionStorage.clear()
  })

  it('renders detailed OpenStreetMap tiles with visible attribution', async () => {
    const { container } = render(<MapView />)

    await waitFor(() => {
      expect(container.querySelector('img.leaflet-tile')).not.toBeNull()
    })
    expect(container.querySelector('img.leaflet-tile')?.getAttribute('src')).toContain(
      'tile.openstreetmap.org'
    )
    expect(container.textContent).toContain('OpenStreetMap')
  })
})

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { BoundingBox } from '../../../src/frontend/api/map'
import { useMapClusters } from '../../../src/frontend/hooks/useMapClusters'

const { getClusters } = vi.hoisted(() => ({ getClusters: vi.fn() }))
vi.mock('../../../src/frontend/api/map', () => ({ mapApi: { getClusters } }))

function wrapper({ children }: { children: ReactNode }) {
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>
}
let client: QueryClient
const bounds = { north: 42, south: 40, west: -75, east: -73 }
const first = {
  id: 'dr5',
  lat: 40.2,
  lng: -74.2,
  count: 8,
  representativeId: 10,
}
const second = {
  id: 'dr6',
  lat: 40.2001,
  lng: -74.2001,
  count: 2,
  representativeId: 20,
}

beforeEach(() => {
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  getClusters.mockReset()
})
afterEach(() => {
  cleanup()
  client.clear()
})

describe('useMapClusters', () => {
  it('preserves nearby server centers and membership instead of reclustering the viewport', async () => {
    getClusters
      .mockResolvedValueOnce({ clusters: [first, second], totalCount: 10 })
      .mockResolvedValueOnce({ clusters: [first], totalCount: 8 })
    const { result, rerender } = renderHook(
      ({ bounds }: { bounds: BoundingBox }) => useMapClusters({ bounds, zoom: 8 }),
      { wrapper, initialProps: { bounds } }
    )
    await waitFor(() => expect(result.current.clusters).toHaveLength(2))
    const marker = result.current.clusters[0]
    expect(marker).toEqual({
      geometry: { coordinates: [-74.2, 40.2] },
      properties: { cellId: 'dr5', count: 8, representativeId: 10 },
    })
    rerender({ bounds: { ...bounds, west: -74.5 } })
    await waitFor(() => expect(result.current.clusters).toHaveLength(1))
    expect(result.current.clusters[0]).toEqual(marker)
  })

  it('waits for a viewport and reports request failures', async () => {
    getClusters.mockRejectedValue(new Error('Unavailable'))
    const { result, rerender } = renderHook(
      ({ bounds }: { bounds: BoundingBox | null }) => useMapClusters({ bounds, zoom: 8 }),
      { wrapper, initialProps: { bounds: null as BoundingBox | null } }
    )
    expect(getClusters).not.toHaveBeenCalled()
    rerender({ bounds })
    await waitFor(() => expect(result.current.error).toBeTruthy())
    expect(result.current.clusters).toEqual([])
  })
  it('reuses visited viewports and zoom levels and refreshes invalidated results', async () => {
    getClusters.mockResolvedValue({ clusters: [first], totalCount: 8 })
    const { result, rerender } = renderHook(
      ({ bounds, zoom }: { bounds: BoundingBox; zoom: number }) => useMapClusters({ bounds, zoom }),
      { wrapper, initialProps: { bounds, zoom: 8 } }
    )
    await waitFor(() => expect(result.current.clusters).toHaveLength(1))
    rerender({ bounds, zoom: 9 })
    await waitFor(() => expect(getClusters).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(result.current.clusters).toHaveLength(1))
    rerender({ bounds: { ...bounds, west: -74.5 }, zoom: 8 })
    await waitFor(() => expect(getClusters).toHaveBeenCalledTimes(3))
    await waitFor(() => expect(result.current.clusters).toHaveLength(1))
    rerender({ bounds, zoom: 8 })
    expect(result.current.clusters).toHaveLength(1)
    expect(getClusters).toHaveBeenCalledTimes(3)
    getClusters.mockResolvedValue({ clusters: [second], totalCount: 2 })
    await client.invalidateQueries({ queryKey: ['map-clusters'] })
    await waitFor(() => expect(result.current.totalCount).toBe(2))
    expect(getClusters).toHaveBeenCalledTimes(4)
  })
})

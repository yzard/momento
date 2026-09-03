import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { mapApi, normalizeMapBounds } from '../../../src/frontend/api/map'

describe('normalizeMapBounds', () => {
  it('converts a repeated-world viewport into the complete world', () => {
    expect(
      normalizeMapBounds({
        east: 428.5546875,
        north: 86.83673396186525,
        south: -86.83673396186525,
        west: -428.5546875,
      })
    ).toEqual({ east: 180, north: 86.83673396186525, south: -86.83673396186525, west: -180 })
  })

  it('wraps a narrow repeated viewport across the antimeridian', () => {
    expect(normalizeMapBounds({ east: 190, north: 20, south: -20, west: 170 })).toEqual({
      east: -170,
      north: 20,
      south: -20,
      west: 170,
    })
  })

  it('clamps latitude overscan while preserving canonical antimeridian bounds', () => {
    expect(normalizeMapBounds({ east: -170, north: 95, south: -95, west: 170 })).toEqual({
      east: -170,
      north: 90,
      south: -90,
      west: 170,
    })
  })

  it('collapses a viewport beyond a pole to the nearest geographic edge', () => {
    expect(normalizeMapBounds({ east: 10, north: 110, south: 100, west: -10 })).toEqual({
      east: 10,
      north: 90,
      south: 90,
      west: -10,
    })
  })

  it('rejects non-finite and inverted latitude bounds', () => {
    expect(() =>
      normalizeMapBounds({ east: 10, north: Number.NaN, south: -10, west: -10 })
    ).toThrow('Map bounds must contain finite coordinates')
    expect(() => normalizeMapBounds({ east: 10, north: -10, south: 10, west: -10 })).toThrow(
      'Map bounds south must not exceed north'
    )
  })
})

describe('mapApi', () => {
  beforeEach(() => {
    post.mockReset()
    post.mockResolvedValue({ data: { clusters: [], totalCount: 0 } })
  })

  it('sends normalized cluster bounds', async () => {
    await mapApi.getClusters({ east: 428, north: 100, south: -100, west: -428 }, 2)

    expect(post).toHaveBeenCalledWith('/map/clusters', {
      bounds: { east: 180, north: 90, south: -90, west: -180 },
      zoom: 2,
    })
  })

  it('sends normalized media bounds without changing the geohash filter', async () => {
    await mapApi.getMedia({
      bounds: { east: 190, north: 20, south: -20, west: 170 },
      geohashPrefixes: ['xb'],
    })

    expect(post).toHaveBeenCalledWith('/map/media', {
      bounds: { east: -170, north: 20, south: -20, west: 170 },
      geohashPrefixes: ['xb'],
    })
  })
})

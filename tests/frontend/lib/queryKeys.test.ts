import { describe, expect, it, vi } from 'vitest'

import { invalidateMediaConsumers, queryKeys } from '../../../src/frontend/lib/queryKeys'

describe('query keys', () => {
  it('uses the same duplicate-list prefix for definitions and invalidation', () => {
    expect(queryKeys.duplicates.list(7).slice(0, 2)).toEqual(queryKeys.duplicates.listRoot)
  })

  it('invalidates every view that consumes mutable media', async () => {
    const invalidateQueries = vi.fn().mockResolvedValue(undefined)

    await invalidateMediaConsumers({ invalidateQueries })

    expect(invalidateQueries.mock.calls.map(([filter]) => filter.queryKey)).toEqual([
      queryKeys.timeline.all,
      queryKeys.trash.all,
      queryKeys.duplicates.all,
      queryKeys.mapClusters.all,
      queryKeys.albums.all,
      queryKeys.albums.detailRoot,
      queryKeys.places.all,
      queryKeys.faces.all,
    ])
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { placesApi } from '../../../src/frontend/api/places'

describe('placesApi', () => {
  beforeEach(() => post.mockReset())

  it('lists places with cursor pagination', async () => {
    const response = {
      places: [
        {
          placeId: 'paris-france',
          city: 'Paris',
          state: null,
          country: 'France',
          mediaCount: 8,
        },
      ],
      nextCursor: 'paris-france',
      hasMore: true,
    }
    post.mockResolvedValue({ data: response })

    await expect(placesApi.list({ cursor: null, limit: 100 })).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/places/list', {
      cursor: null,
      limit: 100,
    })
  })

  it('loads a place media page with its place identifier and cursor', async () => {
    const response = {
      place: {
        placeId: 'paris-france',
        city: 'Paris',
        state: null,
        country: 'France',
        mediaCount: 8,
      },
      media: [],
      nextCursor: null,
      hasMore: false,
    }
    post.mockResolvedValue({ data: response })

    await expect(
      placesApi.get({
        placeId: 'paris-france',
        cursor: 'media-40',
        limit: 100,
      })
    ).resolves.toEqual(response)
    expect(post).toHaveBeenCalledWith('/places/get', {
      placeId: 'paris-france',
      cursor: 'media-40',
      limit: 100,
    })
  })

  it('builds a freshly selected place binary thumbnail URL', async () => {
    await expect(placesApi.getThumbnail('paris-france')).resolves.toBe(
      '/api/v1/places/paris-france/thumbnail'
    )
    expect(post).not.toHaveBeenCalled()
  })
})

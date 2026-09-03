import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { mediaApi } from '../../../src/frontend/api/media'

describe('mediaApi timeline classification', () => {
  beforeEach(() => {
    post.mockReset()
    post.mockResolvedValue({ data: { groups: [], markers: [] } })
  })

  it('builds binary thumbnail and preview URLs without requesting JSON payloads', () => {
    expect(mediaApi.getThumbnailURL(7, 'normal')).toBe('/api/v1/media/7/thumbnail')
    expect(mediaApi.getThumbnailURL(7, 'tiny')).toBe('/api/v1/media/7/thumbnail/tiny')
    expect(mediaApi.getPreviewURL(7)).toBe('/api/v1/media/7/preview')
    expect(post).not.toHaveBeenCalled()
  })

  it('posts classification with timeline page requests', async () => {
    const request = {
      groupBy: 'day' as const,
      search: '',
      mediaType: 'image' as const,
      classification: 'screenshot' as const,
      direction: 'older' as const,
    }

    await mediaApi.listTimeline(request)

    expect(post).toHaveBeenCalledWith('/timeline/list', request)
  })

  it('posts explicit null classification for unclassified marker requests', async () => {
    await mediaApi.getTimelineMarkers('image', null, 'holiday')

    expect(post).toHaveBeenCalledWith('/timeline/markers', {
      mediaType: 'image',
      classification: null,
      search: 'holiday',
    })
  })

  it('requests a resource-scoped URL for original media streaming', async () => {
    post.mockResolvedValueOnce({
      data: {
        url: '/api/v1/media/42/original?ticket=signed',
        expiresAt: '2026-08-26T00:00:00Z',
      },
    })

    await expect(mediaApi.getFileStreamURL(42)).resolves.toBe(
      '/api/v1/media/42/original?ticket=signed'
    )
    expect(post).toHaveBeenCalledWith('/media/access-ticket', {
      mediaId: 42,
      resource: 'original',
    })
  })

  it('rejects invalid media IDs before constructing an asset URL', () => {
    expect(() => mediaApi.getThumbnailURL(0, 'normal')).toThrow(
      'mediaId must be a positive safe integer'
    )
  })
})

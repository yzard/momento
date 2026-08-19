import { beforeEach, describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { mediaApi } from '../../../src/frontend/api/media'

describe('mediaApi timeline classification', () => {
  beforeEach(() => {
    post.mockReset()
    post.mockResolvedValue({ data: { groups: [], markers: [] } })
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
})

import { describe, expect, it, vi } from 'vitest'

const { post } = vi.hoisted(() => ({ post: vi.fn() }))

vi.mock('../../../src/frontend/api/client', () => ({ apiClient: { post } }))

import { trashApi } from '../../../src/frontend/api/trash'

describe('trashApi thumbnails', () => {
  it('builds a binary thumbnail URL without requesting a JSON payload', () => {
    expect(trashApi.getThumbnailURL(7)).toBe('/api/v1/trash/7/thumbnail/tiny')
    expect(post).not.toHaveBeenCalled()
  })
})

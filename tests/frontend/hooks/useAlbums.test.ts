import { describe, expect, it } from 'vitest'

import { reorderAlbumMedia } from '../../../src/frontend/hooks/useAlbums'

describe('reorderAlbumMedia', () => {
  it('maps a complete ID order to the existing media values', () => {
    const media = [
      { id: 1, name: 'first' },
      { id: 2, name: 'second' },
      { id: 3, name: 'third' },
    ]

    expect(reorderAlbumMedia(media, [3, 1, 2])).toEqual([
      { id: 3, name: 'third' },
      { id: 1, name: 'first' },
      { id: 2, name: 'second' },
    ])
  })
})

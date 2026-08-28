import { describe, expect, it } from 'vitest'
import type { MediaTypeFilter, TimelineClassification } from '../../../src/frontend/api/media'
import { timelinePresentation } from '../../../src/frontend/lib/timelinePresentation'

describe('timelinePresentation', () => {
  it.each<[TimelineClassification | null, MediaTypeFilter | null, string, string, string]>([
    ['screenshot', null, 'Screenshots', 'screenshots', 'Search screenshots...'],
    ['document', null, 'Documents', 'documents', 'Search documents...'],
    [null, 'image', 'Photos', 'photos', 'Search photos...'],
    [null, 'video', 'Videos', 'videos', 'Search videos...'],
    [null, null, 'Timeline', 'media', 'Search media...'],
  ])(
    'returns the shared copy for classification=%s and mediaType=%s',
    (classification, mediaType, title, mediaLabel, searchPlaceholder) => {
      const presentation = timelinePresentation(mediaType, classification)

      expect(presentation).toMatchObject({
        title,
        mediaLabel,
        searchPlaceholder,
      })
      expect(presentation.emptyDescription).not.toBe('')
    }
  )
})

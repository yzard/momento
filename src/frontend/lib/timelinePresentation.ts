import type { MediaTypeFilter, TimelineClassification } from '../api/media'

export interface TimelinePresentation {
  title: string
  mediaLabel: string
  searchPlaceholder: string
  emptyDescription: string
}

export function timelinePresentation(
  mediaType: MediaTypeFilter | null,
  classification: TimelineClassification | null
): TimelinePresentation {
  if (classification === 'screenshot') {
    return {
      title: 'Screenshots',
      mediaLabel: 'screenshots',
      searchPlaceholder: 'Search screenshots...',
      emptyDescription: 'Screenshots identified by Screenshot Detection will appear here.',
    }
  }

  if (classification === 'document') {
    return {
      title: 'Documents',
      mediaLabel: 'documents',
      searchPlaceholder: 'Search documents...',
      emptyDescription: 'Documents identified by Document Detection will appear here.',
    }
  }

  if (mediaType === 'image') {
    return {
      title: 'Photos',
      mediaLabel: 'photos',
      searchPlaceholder: 'Search photos...',
      emptyDescription: 'Import some photos to get started.',
    }
  }

  if (mediaType === 'video') {
    return {
      title: 'Videos',
      mediaLabel: 'videos',
      searchPlaceholder: 'Search videos...',
      emptyDescription: 'Import some videos to get started.',
    }
  }

  return {
    title: 'Timeline',
    mediaLabel: 'media',
    searchPlaceholder: 'Search media...',
    emptyDescription: 'Import some photos or videos to get started.',
  }
}

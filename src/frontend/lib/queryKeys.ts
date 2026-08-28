import type { QueryClient } from '@tanstack/react-query'
import type { BoundingBox } from '../api/map'
import type { MediaTypeFilter, TimelineClassification } from '../api/media'

type QueryInvalidator = Pick<QueryClient, 'invalidateQueries'>

export const queryKeys = {
  ai: {
    all: ['ai'] as const,
    status: ['ai', 'status'] as const,
  },
  albums: {
    all: ['albums'] as const,
    detailRoot: ['album'] as const,
    detail: (albumId: number) => ['album', albumId] as const,
  },
  duplicates: {
    all: ['duplicates'] as const,
    listRoot: ['duplicates', 'list'] as const,
    list: (userId: number | undefined) => ['duplicates', 'list', userId] as const,
  },
  faces: {
    all: ['faces'] as const,
    groups: ['faces', 'groups'] as const,
    group: (faceGroupId: number) => ['faces', 'groups', faceGroupId] as const,
  },
  mapClusters: {
    all: ['map-clusters'] as const,
    viewport: (bounds: BoundingBox | null, dataZoom: number) =>
      ['map-clusters', bounds, dataZoom] as const,
  },
  places: {
    all: ['places'] as const,
    detail: (placeId: string) => ['places', placeId] as const,
  },
  timeline: {
    all: ['timeline'] as const,
    markers: (
      mediaType: MediaTypeFilter | null,
      classification: TimelineClassification | null,
      search: string
    ) => ['timeline', 'markers', mediaType, classification, search] as const,
  },
  trash: {
    all: ['trash'] as const,
  },
}

export async function invalidateMediaConsumers(queryClient: QueryInvalidator): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.timeline.all }),
    queryClient.invalidateQueries({ queryKey: queryKeys.trash.all }),
    queryClient.invalidateQueries({ queryKey: queryKeys.duplicates.all }),
    queryClient.invalidateQueries({ queryKey: queryKeys.mapClusters.all }),
    queryClient.invalidateQueries({ queryKey: queryKeys.albums.all }),
    queryClient.invalidateQueries({ queryKey: queryKeys.albums.detailRoot }),
    queryClient.invalidateQueries({ queryKey: queryKeys.places.all }),
    queryClient.invalidateQueries({ queryKey: queryKeys.faces.all }),
  ])
}

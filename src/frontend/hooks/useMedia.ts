import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { mediaApi, type GroupBy } from '../api/media'

export function useTimeline(groupBy: GroupBy, limit: number, search: string) {
  const normalizedSearch = search.trim()

  return useInfiniteQuery({
    queryKey: ['timeline', groupBy, normalizedSearch],
    queryFn: ({ pageParam }) =>
      mediaApi.listTimeline({ cursor: pageParam, limit, groupBy, search: normalizedSearch }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) => (lastPage.hasMore ? lastPage.nextCursor : undefined),
  })
}

export function useMediaList(limit = 50) {
  return useInfiniteQuery({
    queryKey: ['media'],
    queryFn: ({ pageParam }) => mediaApi.list({ cursor: pageParam, limit }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) => (lastPage.hasMore ? lastPage.nextCursor : undefined),
  })
}

export function useMapMedia() {
  return useInfiniteQuery({
    queryKey: ['mapMedia'],
    queryFn: () => mediaApi.listMapMedia(),
    initialPageParam: undefined,
    getNextPageParam: () => undefined,
  })
}

export function useDeleteMedia() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (mediaId: number) => mediaApi.delete(mediaId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['timeline'] })
      queryClient.invalidateQueries({ queryKey: ['media'] })
      queryClient.invalidateQueries({ queryKey: ['mapMedia'] })
    },
  })
}

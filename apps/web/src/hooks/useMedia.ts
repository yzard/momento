import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { mediaApi, type GroupBy } from '../api/media'

export function useTimeline(groupBy: GroupBy = 'day', limit = 100) {
  return useInfiniteQuery({
    queryKey: ['timeline', groupBy],
    queryFn: ({ pageParam }) => mediaApi.timeline({ cursor: pageParam, limit, group_by: groupBy }),
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
    queryFn: () => mediaApi.mapMedia(),
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

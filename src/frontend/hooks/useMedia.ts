import { useMutation, useQueryClient } from '@tanstack/react-query'
import { mediaApi } from '../api/media'

export function useDeleteMedia() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (mediaIds: number[]) => mediaApi.delete(mediaIds),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['timeline'] })
      queryClient.invalidateQueries({ queryKey: ['trash'] })
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'groups'] })
    },
  })
}

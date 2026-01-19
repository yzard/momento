import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { albumsApi } from '../api/albums'

export function useAlbums() {
  return useQuery({
    queryKey: ['albums'],
    queryFn: () => albumsApi.list(),
  })
}

export function useAlbum(albumId: number) {
  return useQuery({
    queryKey: ['album', albumId],
    queryFn: () => albumsApi.get(albumId),
    enabled: albumId > 0,
  })
}

export function useCreateAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (data: { name: string; description?: string }) => albumsApi.create(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['albums'] })
    },
  })
}

export function useDeleteAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (albumId: number) => albumsApi.delete(albumId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['albums'] })
    },
  })
}

export function useAddMediaToAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ albumId, mediaIds }: { albumId: number; mediaIds: number[] }) =>
      albumsApi.addMedia(albumId, mediaIds),
    onSuccess: (_, { albumId }) => {
      queryClient.invalidateQueries({ queryKey: ['album', albumId] })
      queryClient.invalidateQueries({ queryKey: ['albums'] })
    },
  })
}

export function useRemoveMediaFromAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ albumId, mediaIds }: { albumId: number; mediaIds: number[] }) =>
      albumsApi.removeMedia(albumId, mediaIds),
    onSuccess: (_, { albumId }) => {
      queryClient.invalidateQueries({ queryKey: ['album', albumId] })
      queryClient.invalidateQueries({ queryKey: ['albums'] })
    },
  })
}

export function useReorderAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ albumId, mediaIds }: { albumId: number; mediaIds: number[] }) =>
      albumsApi.reorder(albumId, mediaIds),
    onSuccess: (_, { albumId }) => {
      queryClient.invalidateQueries({ queryKey: ['album', albumId] })
    },
  })
}

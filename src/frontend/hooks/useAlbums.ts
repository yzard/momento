import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { albumsApi } from '../api/albums'
import type { AlbumDetail } from '../api/albums'
import { queryKeys } from '../lib/queryKeys'

export function reorderAlbumMedia<T extends { id: number }>(media: T[], mediaIds: number[]): T[] {
  const mediaById = new Map(media.map((item) => [item.id, item]))
  return mediaIds.flatMap((mediaId) => {
    const item = mediaById.get(mediaId)
    return item ? [item] : []
  })
}

export function useAlbums() {
  return useQuery({
    queryKey: queryKeys.albums.all,
    queryFn: () => albumsApi.list(),
  })
}

export function useAlbum(albumId: number) {
  return useQuery({
    queryKey: queryKeys.albums.detail(albumId),
    queryFn: () => albumsApi.get(albumId),
    enabled: albumId > 0,
  })
}

export function useCreateAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (data: { name: string; description?: string; mediaIds: number[] }) =>
      albumsApi.create(data),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.albums.all })
    },
  })
}

export function useDeleteAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (albumId: number) => albumsApi.delete(albumId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.albums.all })
    },
  })
}

export function useReorderAlbum() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ albumId, mediaIds }: { albumId: number; mediaIds: number[] }) =>
      albumsApi.reorder(albumId, mediaIds),
    onSuccess: (_, { albumId, mediaIds }) => {
      queryClient.setQueryData<AlbumDetail>(queryKeys.albums.detail(albumId), (album) =>
        album ? { ...album, media: reorderAlbumMedia(album.media, mediaIds) } : album
      )
      return queryClient.invalidateQueries({ queryKey: queryKeys.albums.detail(albumId) })
    },
  })
}

export function useRemoveAlbumMedia() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ albumId, mediaIds }: { albumId: number; mediaIds: number[] }) =>
      albumsApi.removeMedia(albumId, mediaIds),
    onSuccess: (_, { albumId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.albums.all })
      return queryClient.invalidateQueries({ queryKey: queryKeys.albums.detail(albumId) })
    },
  })
}

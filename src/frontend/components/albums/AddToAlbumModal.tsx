import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { albumsApi } from '../../api/albums'
import { X, Folder, Plus, Loader2 } from 'lucide-react'
import { cn } from '../../lib/utils'
import { queryKeys } from '../../lib/queryKeys'
import type { Album } from '../../api/types'

interface AddToAlbumModalProps {
  mediaIds: number[]
  onClose: () => void
}

interface AlbumPickerContentProps {
  albums: Album[] | undefined
  isLoading: boolean
  isProcessing: boolean
  errorMessage: string | null
  showNewAlbum: boolean
  newAlbumName: string
  onAlbumSelect: (albumId: number) => void
  onShowNewAlbum: () => void
  onHideNewAlbum: () => void
  onNameChange: (name: string) => void
  onCreate: (event: React.FormEvent) => void
}

function AlbumPickerContent(props: AlbumPickerContentProps) {
  if (props.isLoading)
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    )
  return (
    <>
      {props.errorMessage && (
        <p role="alert" className="mb-3 text-sm text-destructive">
          {props.errorMessage}
        </p>
      )}
      {props.albums && props.albums.length > 0 && (
        <div className="mb-4 space-y-2">
          {props.albums.map((album) => (
            <button
              key={album.id}
              onClick={() => props.onAlbumSelect(album.id)}
              disabled={props.isProcessing}
              className={cn(
                'flex min-h-11 w-full cursor-pointer items-center gap-3 rounded-lg px-4 py-3 text-left transition-colors',
                'hover:bg-muted disabled:opacity-50'
              )}
            >
              <Folder className="h-5 w-5 text-primary" />
              <span className="flex-1 font-medium">{album.name}</span>
            </button>
          ))}
        </div>
      )}
      {!props.showNewAlbum ? (
        <button
          onClick={props.onShowNewAlbum}
          className="flex min-h-11 w-full cursor-pointer items-center gap-3 rounded-lg px-4 py-3 text-left font-medium text-primary transition-colors hover:bg-muted"
        >
          <Plus className="h-5 w-5" />
          Create new album
        </button>
      ) : (
        <form onSubmit={props.onCreate} className="space-y-3">
          <input
            type="text"
            value={props.newAlbumName}
            onChange={(event) => props.onNameChange(event.target.value)}
            placeholder="Album name"
            aria-label="Album name"
            autoFocus
            className="w-full rounded-lg border border-border bg-muted px-4 py-2 focus:outline-none focus:ring-2 focus:ring-primary"
          />
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={props.onHideNewAlbum}
              className="min-h-11 cursor-pointer px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!props.newAlbumName.trim() || props.isProcessing}
              className="min-h-11 cursor-pointer rounded-lg bg-primary px-4 py-2 text-sm font-semibold text-primary-foreground disabled:cursor-not-allowed disabled:opacity-50"
            >
              Create & Add
            </button>
          </div>
        </form>
      )}
    </>
  )
}

export default function AddToAlbumModal({ mediaIds, onClose }: AddToAlbumModalProps) {
  const queryClient = useQueryClient()
  const [newAlbumName, setNewAlbumName] = useState('')
  const [showNewAlbum, setShowNewAlbum] = useState(false)

  const { data: albums, isLoading } = useQuery({
    queryKey: queryKeys.albums.all,
    queryFn: albumsApi.list,
  })

  const addMutation = useMutation({
    mutationFn: (albumId: number) => albumsApi.addMedia(albumId, mediaIds),
    onSuccess: (_, albumId) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.albums.all })
      void queryClient.invalidateQueries({ queryKey: queryKeys.albums.detail(albumId) })
      onClose()
    },
  })

  const createMutation = useMutation({
    mutationFn: () => albumsApi.create({ name: newAlbumName.trim(), mediaIds }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.albums.all })
      onClose()
    },
  })

  const handleAddToAlbum = (albumId: number) => {
    addMutation.mutate(albumId)
  }

  const handleCreateAndAdd = (e: React.FormEvent) => {
    e.preventDefault()
    if (newAlbumName.trim()) {
      createMutation.mutate()
    }
  }

  const isProcessing = addMutation.isPending || createMutation.isPending

  const errorMessage = addMutation.isError
    ? 'Could not add the selected media to this album.'
    : createMutation.isError
      ? 'Could not create the album.'
      : null

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      onClick={() => {
        if (!isProcessing) onClose()
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-to-album-title"
        className="bg-background border border-border rounded-xl shadow-2xl w-full max-w-md overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between p-4 border-b border-border">
          <div>
            <h2 id="add-to-album-title" className="text-lg font-semibold">
              Add to album
            </h2>
            <p className="mt-0.5 text-xs text-muted-foreground">
              {mediaIds.length} selected item{mediaIds.length === 1 ? '' : 's'}
            </p>
          </div>
          <button
            onClick={onClose}
            disabled={isProcessing}
            aria-label="Close album picker"
            className="flex h-11 w-11 cursor-pointer items-center justify-center rounded-lg transition-colors duration-200 hover:bg-muted disabled:cursor-not-allowed disabled:opacity-40"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="max-h-80 overflow-y-auto p-4">
          <AlbumPickerContent
            albums={albums}
            isLoading={isLoading}
            isProcessing={isProcessing}
            errorMessage={errorMessage}
            showNewAlbum={showNewAlbum}
            newAlbumName={newAlbumName}
            onAlbumSelect={handleAddToAlbum}
            onShowNewAlbum={() => setShowNewAlbum(true)}
            onHideNewAlbum={() => setShowNewAlbum(false)}
            onNameChange={setNewAlbumName}
            onCreate={handleCreateAndAdd}
          />
        </div>
      </div>
    </div>
  )
}

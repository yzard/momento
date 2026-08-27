import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { albumsApi } from '../../api/albums'
import { X, Folder, Plus, Loader2 } from 'lucide-react'
import { cn } from '../../lib/utils'

interface AddToAlbumModalProps {
  mediaIds: number[]
  onClose: () => void
}

export default function AddToAlbumModal({ mediaIds, onClose }: AddToAlbumModalProps) {
  const queryClient = useQueryClient()
  const [newAlbumName, setNewAlbumName] = useState('')
  const [showNewAlbum, setShowNewAlbum] = useState(false)

  const { data: albums, isLoading } = useQuery({
    queryKey: ['albums'],
    queryFn: albumsApi.list,
  })

  const addMutation = useMutation({
    mutationFn: (albumId: number) => albumsApi.addMedia(albumId, mediaIds),
    onSuccess: (_, albumId) => {
      void queryClient.invalidateQueries({ queryKey: ['albums'] })
      void queryClient.invalidateQueries({ queryKey: ['album', albumId] })
      onClose()
    },
  })

  const createMutation = useMutation({
    mutationFn: async () => {
      const album = await albumsApi.create({ name: newAlbumName })
      await albumsApi.addMedia(album.id, mediaIds)
      return album
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['albums'] })
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
      onClick={() => { if (!isProcessing) onClose() }}
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
            <h2 id="add-to-album-title" className="text-lg font-semibold">Add to album</h2>
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

        <div className="p-4 max-h-80 overflow-y-auto">
          {errorMessage && <p role="alert" className="mb-3 text-sm text-destructive">{errorMessage}</p>}
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 className="w-6 h-6 animate-spin text-muted-foreground" />
            </div>
          ) : (
            <>
              {albums && albums.length > 0 && (
                <div className="space-y-2 mb-4">
                  {albums.map((album) => (
                    <button
                      key={album.id}
                      onClick={() => handleAddToAlbum(album.id)}
                      disabled={isProcessing}
                      className={cn(
                        "w-full min-h-11 cursor-pointer flex items-center gap-3 px-4 py-3 rounded-lg text-left transition-colors duration-200",
                        "hover:bg-muted disabled:opacity-50"
                      )}
                    >
                      <Folder className="w-5 h-5 text-primary" />
                      <span className="flex-1 font-medium">{album.name}</span>
                    </button>
                  ))}
                </div>
              )}

              {!showNewAlbum ? (
                <button
                  onClick={() => setShowNewAlbum(true)}
                  className="w-full min-h-11 cursor-pointer flex items-center gap-3 px-4 py-3 rounded-lg text-left transition-colors duration-200 hover:bg-muted text-primary"
                >
                  <Plus className="w-5 h-5" />
                  <span className="font-medium">Create new album</span>
                </button>
              ) : (
                <form onSubmit={handleCreateAndAdd} className="space-y-3">
                  <input
                    type="text"
                    value={newAlbumName}
                    onChange={(e) => setNewAlbumName(e.target.value)}
                    placeholder="Album name"
                    aria-label="Album name"
                    autoFocus
                    className="w-full px-4 py-2 bg-muted border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-primary"
                  />
                  <div className="flex gap-2 justify-end">
                    <button
                      type="button"
                      onClick={() => setShowNewAlbum(false)}
                      className="min-h-11 cursor-pointer px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors duration-200"
                    >
                      Cancel
                    </button>
                    <button
                      type="submit"
                      disabled={!newAlbumName.trim() || isProcessing}
                      className="min-h-11 cursor-pointer px-4 py-2 bg-primary text-primary-foreground text-sm font-semibold rounded-lg hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      Create & Add
                    </button>
                  </div>
                </form>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}

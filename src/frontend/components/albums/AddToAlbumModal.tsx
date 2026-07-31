import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { albumsApi } from '../../api/albums'
import { X, Folder, Plus, Loader2 } from 'lucide-react'
import { cn } from '../../lib/utils'

interface AddToAlbumModalProps {
  mediaId: number
  onClose: () => void
}

export default function AddToAlbumModal({ mediaId, onClose }: AddToAlbumModalProps) {
  const queryClient = useQueryClient()
  const [newAlbumName, setNewAlbumName] = useState('')
  const [showNewAlbum, setShowNewAlbum] = useState(false)

  const { data: albums, isLoading } = useQuery({
    queryKey: ['albums'],
    queryFn: albumsApi.list,
  })

  const addMutation = useMutation({
    mutationFn: (albumId: number) => albumsApi.addMedia(albumId, [mediaId]),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['albums'] })
      onClose()
    },
  })

  const createMutation = useMutation({
    mutationFn: async () => {
      const album = await albumsApi.create({ name: newAlbumName })
      await albumsApi.addMedia(album.id, [mediaId])
      return album
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['albums'] })
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

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={onClose}>
      <div
        className="bg-background border border-border rounded-xl shadow-2xl w-full max-w-md mx-4 overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between p-4 border-b border-border">
          <h2 className="text-lg font-semibold">Add to Album</h2>
          <button
            onClick={onClose}
            className="p-1 rounded-lg hover:bg-muted transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="p-4 max-h-80 overflow-y-auto">
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
                        "w-full flex items-center gap-3 px-4 py-3 rounded-lg text-left transition-colors",
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
                  className="w-full flex items-center gap-3 px-4 py-3 rounded-lg text-left transition-colors hover:bg-muted text-primary"
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
                    autoFocus
                    className="w-full px-4 py-2 bg-muted border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-primary"
                  />
                  <div className="flex gap-2 justify-end">
                    <button
                      type="button"
                      onClick={() => setShowNewAlbum(false)}
                      className="px-4 py-2 text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
                    >
                      Cancel
                    </button>
                    <button
                      type="submit"
                      disabled={!newAlbumName.trim() || isProcessing}
                      className="px-4 py-2 bg-primary text-primary-foreground text-sm font-bold uppercase tracking-wider rounded-lg hover:bg-primary/90 disabled:opacity-50"
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

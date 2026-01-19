import { useState } from 'react'
import { useAlbums, useCreateAlbum, useDeleteAlbum } from '../../hooks/useAlbums'
import AlbumCard from './AlbumCard'
import { Plus, FolderPlus, Loader2 } from 'lucide-react'
import type { Album } from '../../api/types'

interface AlbumListProps {
  onAlbumClick: (album: Album) => void
}

export default function AlbumList({ onAlbumClick }: AlbumListProps) {
  const { data: albums, isLoading, error } = useAlbums()
  const createAlbum = useCreateAlbum()
  const deleteAlbum = useDeleteAlbum()
  const [showCreateModal, setShowCreateModal] = useState(false)
  const [newAlbumName, setNewAlbumName] = useState('')

  const handleCreate = async () => {
    if (!newAlbumName.trim()) return
    await createAlbum.mutateAsync({ name: newAlbumName.trim() })
    setNewAlbumName('')
    setShowCreateModal(false)
  }

  const handleDelete = async (albumId: number) => {
    if (confirm('Delete this album? Photos will not be deleted.')) {
      await deleteAlbum.mutateAsync(albumId)
    }
  }

  if (isLoading) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-muted-foreground gap-3">
        <Loader2 className="w-8 h-8 animate-spin text-primary" />
        <p className="text-sm font-medium">Loading your albums...</p>
      </div>
    )
  }

  if (error) {
    return (
      <div className="flex flex-col items-center justify-center h-[50vh] text-destructive gap-3">
        <p className="text-lg font-semibold">Unable to load albums</p>
      </div>
    )
  }

  return (
    <div className="animate-fade-in py-8">
      <div className="flex justify-between items-center mb-10 pb-6 border-b border-border/50">
        <div>
          <h2 className="text-4xl font-display font-medium text-foreground tracking-tight">Albums</h2>
          <p className="text-muted-foreground mt-2 font-light text-lg">Organize your favorite moments</p>
        </div>
        <button
          onClick={() => setShowCreateModal(true)}
          className="bg-foreground text-background px-6 py-2.5 hover:bg-foreground/90 transition-all rounded-lg shadow-md hover:shadow-lg flex items-center gap-2 font-bold uppercase tracking-wider text-xs"
        >
          <Plus className="w-4 h-4" strokeWidth={3} />
          Create Album
        </button>
      </div>

      {albums?.length === 0 ? (
        <div className="flex flex-col items-center justify-center h-80 text-muted-foreground border border-dashed border-border rounded-xl bg-muted/10">
          <div className="p-6 bg-white rounded-full border border-border mb-4 shadow-sm">
            <FolderPlus className="w-10 h-10 text-primary/80" strokeWidth={1.5} />
          </div>
          <p className="text-xl font-display font-medium text-foreground mb-1">No albums yet</p>
          <p className="text-sm font-medium mb-8">Create one to organize your photos.</p>
          <button
            onClick={() => setShowCreateModal(true)}
            className="text-primary font-bold hover:underline uppercase tracking-wide text-xs"
          >
            Create your first album
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-6 sm:gap-8">
          {albums?.map((album) => (
            <AlbumCard
              key={album.id}
              album={album}
              onClick={() => onAlbumClick(album)}
              onDelete={() => handleDelete(album.id)}
            />
          ))}
        </div>
      )}

      {showCreateModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-xl flex items-center justify-center z-50 p-4">
          <div className="bg-card shadow-2xl border border-border/50 p-8 w-full max-w-md animate-scale-in rounded-2xl">
            <h3 className="text-2xl font-display font-medium mb-2">New Album</h3>
            <p className="text-sm text-muted-foreground mb-8 font-medium">Give your collection a meaningful name.</p>
            
            <input
              type="text"
              value={newAlbumName}
              onChange={(e) => setNewAlbumName(e.target.value)}
              placeholder="e.g. Summer Vacation 2024"
              className="w-full px-4 py-3 border border-input rounded-lg mb-8 focus:outline-none focus:border-primary focus:bg-background bg-muted/20 text-lg font-medium placeholder:text-muted-foreground/50 transition-all focus:ring-2 focus:ring-primary/20"
              autoFocus
              onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
            />
            
            <div className="flex justify-end gap-4">
              <button
                onClick={() => setShowCreateModal(false)}
                className="px-6 py-3 text-muted-foreground hover:text-foreground font-bold uppercase tracking-wider text-sm transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleCreate}
                disabled={!newAlbumName.trim() || createAlbum.isPending}
                className="px-8 py-3 bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 font-medium uppercase tracking-wider text-sm transition-all rounded-full shadow-lg hover:shadow-primary/30"
              >
                {createAlbum.isPending ? 'Creating...' : 'Create Album'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}


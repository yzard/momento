import { useEffect, useState } from 'react'
import { mediaApi } from '../../api/media'
import { useAlbums, useCreateAlbum, useDeleteAlbum } from '../../hooks/useAlbums'
import AlbumCard from './AlbumCard'
import { Plus, FolderPlus, Loader2 } from 'lucide-react'
import type { Album } from '../../api/types'
import ConfirmationDialog from '../common/ConfirmationDialog'

interface AlbumListProps {
  onAlbumClick: (album: Album) => void
}

function CreateAlbumDialog({
  name,
  isPending,
  onNameChange,
  onCreate,
  onClose,
}: {
  name: string
  isPending: boolean
  onNameChange: (name: string) => void
  onCreate: () => void
  onClose: () => void
}) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 p-4 backdrop-blur-xl">
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-album-title"
        className="w-full max-w-md rounded-2xl border border-border/50 bg-card p-8 shadow-2xl animate-scale-in"
      >
        <h3 id="new-album-title" className="mb-2 font-display text-2xl font-medium">
          New Album
        </h3>
        <p className="mb-8 text-sm font-medium text-muted-foreground">
          Give your collection a meaningful name.
        </p>
        <input
          type="text"
          value={name}
          onChange={(event) => onNameChange(event.target.value)}
          placeholder="e.g. Summer Vacation 2024"
          aria-label="Album name"
          className="mb-8 w-full rounded-lg border border-input bg-muted/20 px-4 py-3 text-lg font-medium outline-none transition-all focus:border-primary focus:ring-2 focus:ring-primary/20"
          autoFocus
          onKeyDown={(event) => {
            if (event.key === 'Enter') onCreate()
          }}
        />
        <div className="flex justify-end gap-4">
          <button
            onClick={onClose}
            className="px-6 py-3 text-sm font-bold uppercase tracking-wider text-muted-foreground hover:text-foreground"
          >
            Cancel
          </button>
          <button
            onClick={onCreate}
            disabled={!name.trim() || isPending}
            className="rounded-full bg-primary px-8 py-3 text-sm font-medium uppercase tracking-wider text-primary-foreground shadow-lg disabled:opacity-50"
          >
            {isPending ? 'Creating...' : 'Create Album'}
          </button>
        </div>
      </div>
    </div>
  )
}

function AlbumCollection({
  albums,
  coverUrls,
  onAlbumClick,
  onDelete,
  onCreate,
}: {
  albums: Album[]
  coverUrls: Map<number, string>
  onAlbumClick: (album: Album) => void
  onDelete: (albumId: number) => void
  onCreate: () => void
}) {
  if (albums.length === 0) {
    return (
      <div className="flex h-80 flex-col items-center justify-center rounded-xl border border-dashed border-border bg-muted/10 text-muted-foreground">
        <div className="mb-4 rounded-full border border-border bg-card p-6 shadow-sm">
          <FolderPlus className="h-10 w-10 text-primary/80" />
        </div>
        <p className="mb-1 font-display text-xl font-medium text-foreground">No albums yet</p>
        <p className="mb-8 text-sm font-medium">Create one to organize your photos.</p>
        <button
          onClick={onCreate}
          className="text-xs font-bold uppercase tracking-wide text-primary hover:underline"
        >
          Create your first album
        </button>
      </div>
    )
  }
  return (
    <div className="grid grid-cols-2 gap-6 sm:grid-cols-3 sm:gap-8 md:grid-cols-4 lg:grid-cols-5">
      {albums.map((album) => (
        <AlbumCard
          key={album.id}
          album={album}
          thumbnailUrls={album.thumbnailMediaIds.map((mediaId) => coverUrls.get(mediaId) ?? null)}
          onClick={() => onAlbumClick(album)}
          onDelete={() => onDelete(album.id)}
        />
      ))}
    </div>
  )
}

export default function AlbumList({ onAlbumClick }: AlbumListProps) {
  const { data: albums, isLoading, error } = useAlbums()
  const createAlbum = useCreateAlbum()
  const deleteAlbum = useDeleteAlbum()
  const [showCreateModal, setShowCreateModal] = useState(false)
  const [pendingDeleteAlbumId, setPendingDeleteAlbumId] = useState<number | null>(null)
  const [newAlbumName, setNewAlbumName] = useState('')
  const [coverUrls, setCoverUrls] = useState<Map<number, string>>(new Map())
  const coverIdsKey = [...new Set(albums?.flatMap((album) => album.thumbnailMediaIds) ?? [])].join(
    ','
  )

  useEffect(() => {
    const coverIds = coverIdsKey ? coverIdsKey.split(',').map(Number) : []
    if (coverIds.length === 0) {
      setCoverUrls(new Map())
      return
    }

    let cancelled = false
    mediaApi
      .getThumbnailBatch(coverIds, 'normal')
      .then((thumbnails) => {
        if (cancelled) return
        setCoverUrls(
          new Map([...thumbnails].filter((entry): entry is [number, string] => entry[1] !== null))
        )
      })
      .catch((error: unknown) => {
        console.error('Failed to load album covers:', error)
      })

    return () => {
      cancelled = true
    }
  }, [coverIdsKey])

  const handleCreate = async () => {
    if (!newAlbumName.trim()) return
    await createAlbum.mutateAsync({ name: newAlbumName.trim(), mediaIds: [] })
    setNewAlbumName('')
    setShowCreateModal(false)
  }

  const handleDelete = async (albumId: number) => {
    await deleteAlbum.mutateAsync(albumId)
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
          <p className="text-muted-foreground mt-2 font-light text-lg">
            Organize your favorite moments
          </p>
        </div>
        <button
          onClick={() => setShowCreateModal(true)}
          className="bg-foreground text-background px-6 py-2.5 hover:bg-foreground/90 transition-all rounded-lg shadow-md hover:shadow-lg flex items-center gap-2 font-bold uppercase tracking-wider text-xs"
        >
          <Plus className="w-4 h-4" strokeWidth={3} />
          Create Album
        </button>
      </div>

      <AlbumCollection
        albums={albums ?? []}
        coverUrls={coverUrls}
        onAlbumClick={onAlbumClick}
        onDelete={setPendingDeleteAlbumId}
        onCreate={() => setShowCreateModal(true)}
      />

      {showCreateModal && (
        <CreateAlbumDialog
          name={newAlbumName}
          isPending={createAlbum.isPending}
          onNameChange={setNewAlbumName}
          onCreate={() => void handleCreate()}
          onClose={() => setShowCreateModal(false)}
        />
      )}
      {pendingDeleteAlbumId !== null && (
        <ConfirmationDialog
          title="Delete this album?"
          description="The album will be permanently removed. Its photos will remain in your library."
          confirmLabel="Delete album"
          isProcessing={deleteAlbum.isPending}
          destructive
          onConfirm={() => {
            void handleDelete(pendingDeleteAlbumId).finally(() => setPendingDeleteAlbumId(null))
          }}
          onCancel={() => setPendingDeleteAlbumId(null)}
        />
      )}
    </div>
  )
}

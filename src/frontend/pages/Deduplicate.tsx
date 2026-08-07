import { useEffect, useRef, useState } from 'react'
import { useInfiniteQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  AlertCircle,
  Check,
  Copy,
  ImageOff,
  Loader2,
  Play,
  Square,
  Trash2,
} from 'lucide-react'
import { deduplicateApi, type DeduplicateGroup } from '../api/deduplicate'
import { mediaApi } from '../api/media'
import type { Media } from '../api/types'
import { useAuth } from '../hooks/useAuth'
import { cn } from '../lib/utils'
import { batchLoader } from '../utils/batcher'

const GROUP_PAGE_SIZE = 20

function formatFileSize(fileSize: number | null): string {
  if (fileSize === null) return 'Unknown size'
  if (fileSize < 1024) return `${fileSize} B`
  if (fileSize < 1024 * 1024) return `${Math.round(fileSize / 1024)} KB`
  return `${(fileSize / (1024 * 1024)).toFixed(1)} MB`
}

function formatDimensions(media: Media): string {
  if (media.width === null || media.height === null) return 'Unknown dimensions'
  return `${media.width} x ${media.height}`
}

export default function Deduplicate() {
  const { user } = useAuth()
  const queryClient = useQueryClient()
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())

  const groupsQuery = useInfiniteQuery({
    queryKey: ['deduplicate', 'groups', user?.id],
    queryFn: ({ pageParam }) => deduplicateApi.groups({ cursor: pageParam, limit: GROUP_PAGE_SIZE }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.hasMore && lastPage.nextCursor
      ? lastPage.nextCursor
      : undefined,
  })

  const trashMutation = useMutation({
    mutationFn: (mediaIds: number[]) => mediaApi.delete(mediaIds),
    onSuccess: () => {
      setSelectedIds(new Set())
      queryClient.invalidateQueries({ queryKey: ['deduplicate', 'groups'] })
      queryClient.invalidateQueries({ queryKey: ['timeline'] })
      queryClient.invalidateQueries({ queryKey: ['media'] })
      queryClient.invalidateQueries({ queryKey: ['mapMedia'] })
      queryClient.invalidateQueries({ queryKey: ['map-clusters'] })
      queryClient.invalidateQueries({ queryKey: ['trash'] })
      queryClient.invalidateQueries({ queryKey: ['albums'] })
      queryClient.invalidateQueries({ queryKey: ['album'] })
    },
  })

  const groups = groupsQuery.data?.pages.flatMap((page) => page.groups) ?? []
  const mediaCount = groups.reduce((count, group) => count + group.items.length, 0)

  const toggleSelection = (mediaId: number) => {
    setSelectedIds((currentIds) => {
      const nextIds = new Set(currentIds)
      if (nextIds.has(mediaId)) {
        nextIds.delete(mediaId)
      } else {
        nextIds.add(mediaId)
      }
      return nextIds
    })
  }

  const handleMoveToTrash = () => {
    if (selectedIds.size === 0) return
    const confirmed = window.confirm(
      `Move ${selectedIds.size} selected ${selectedIds.size === 1 ? 'item' : 'items'} to Trash?`,
    )
    if (!confirmed) return
    trashMutation.mutate(Array.from(selectedIds))
  }

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="container max-w-[1800px] mx-auto px-4 py-6 sm:px-6 md:px-10 md:py-10 animate-fade-in pb-28">
        <header className="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <div className="mb-2 flex items-center gap-3 text-primary">
              <Copy className="h-5 w-5" />
              <span className="text-xs font-bold uppercase tracking-[0.2em]">Utility</span>
            </div>
            <h1 className="text-3xl font-display font-bold tracking-tight text-foreground">Deduplicate</h1>
            <p className="mt-2 max-w-2xl text-sm font-medium text-muted-foreground">
              Compare similar media, choose what you no longer need, and move only your selections to Trash.
            </p>
          </div>
          {!groupsQuery.isLoading && !groupsQuery.isError && groups.length > 0 && (
            <div className="flex gap-3 text-sm text-muted-foreground">
              <span className="rounded-lg border border-border bg-card px-3 py-2 font-medium">
                {groups.length} {groups.length === 1 ? 'group' : 'groups'} loaded
              </span>
              <span className="rounded-lg border border-border bg-card px-3 py-2 font-medium">
                {mediaCount} items
              </span>
            </div>
          )}
        </header>

        {groupsQuery.isLoading ? (
          <PageState icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />} title="Loading duplicate groups" description="Checking your accessible media..." />
        ) : groupsQuery.isError ? (
          <PageState
            icon={<AlertCircle className="h-9 w-9 text-destructive" />}
            title="Unable to load duplicate groups"
            description="Try the request again. Existing media has not been changed."
            action={(
              <button type="button" onClick={() => groupsQuery.refetch()} className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90">
                Try again
              </button>
            )}
          />
        ) : groups.length === 0 ? (
          <PageState
            icon={<ImageOff className="h-10 w-10 text-muted-foreground/60" />}
            title="No duplicate groups"
            description="No similar media is currently available in your library."
          />
        ) : (
          <div className="space-y-8">
            {groups.map((group) => (
              <DuplicateGroupSection
                key={group.clusterId}
                group={group}
                selectedIds={selectedIds}
                onToggle={toggleSelection}
              />
            ))}

            {groupsQuery.hasNextPage && (
              <div className="flex justify-center pt-2">
                <button
                  type="button"
                  onClick={() => groupsQuery.fetchNextPage()}
                  disabled={groupsQuery.isFetchingNextPage}
                  className="flex min-h-11 items-center gap-2 rounded-lg border border-border bg-card px-6 py-2.5 text-sm font-bold text-foreground shadow-sm transition-colors hover:bg-muted/40 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {groupsQuery.isFetchingNextPage && <Loader2 className="h-4 w-4 animate-spin" />}
                  {groupsQuery.isFetchingNextPage ? 'Loading groups...' : 'Load more groups'}
                </button>
              </div>
            )}
          </div>
        )}

        {trashMutation.isError && (
          <div role="alert" className="mt-6 flex items-center gap-2 rounded-lg border border-destructive/20 bg-destructive/5 p-4 text-sm text-destructive">
            <AlertCircle className="h-4 w-4 shrink-0" />
            Failed to move the selected media to Trash. Nothing was removed from this view.
          </div>
        )}

        {selectedIds.size > 0 && !groupsQuery.isFetching && (
          <div className="sticky bottom-4 z-30 mt-8 flex flex-col gap-3 rounded-xl border border-border bg-background/95 p-3 shadow-xl backdrop-blur-md sm:flex-row sm:items-center sm:justify-between sm:p-4">
            <div className="px-1">
              <p className="font-bold text-foreground">{selectedIds.size} selected</p>
              <p className="text-xs text-muted-foreground">Only selected media will be moved.</p>
            </div>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setSelectedIds(new Set())}
                disabled={trashMutation.isPending}
                className="min-h-11 flex-1 rounded-lg px-4 py-2 text-sm font-bold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50 sm:flex-none"
              >
                Clear
              </button>
              <button
                type="button"
                onClick={handleMoveToTrash}
                disabled={trashMutation.isPending}
                className="flex min-h-11 flex-1 items-center justify-center gap-2 rounded-lg bg-destructive px-5 py-2 text-sm font-bold text-destructive-foreground transition-colors hover:bg-destructive/90 disabled:cursor-not-allowed disabled:opacity-50 sm:flex-none"
              >
                {trashMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Trash2 className="h-4 w-4" />}
                {trashMutation.isPending ? 'Moving...' : 'Move to Trash'}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

interface DuplicateGroupSectionProps {
  group: DeduplicateGroup
  selectedIds: Set<number>
  onToggle: (mediaId: number) => void
}

function DuplicateGroupSection({ group, selectedIds, onToggle }: DuplicateGroupSectionProps) {
  const selectedCount = group.items.filter((media) => selectedIds.has(media.id)).length

  return (
    <section className="overflow-hidden rounded-xl border border-border bg-card shadow-sm" aria-labelledby={`duplicate-group-${group.clusterId}`}>
      <div className="flex items-center justify-between gap-4 border-b border-border bg-muted/20 px-4 py-4 sm:px-6">
        <div>
          <h2 id={`duplicate-group-${group.clusterId}`} className="font-display text-lg font-semibold text-foreground">
            Similar group {group.clusterId}
          </h2>
          <p className="mt-0.5 text-xs font-medium text-muted-foreground">
            {group.items.length} items to compare
          </p>
        </div>
        {selectedCount > 0 && (
          <span className="rounded-full bg-primary/10 px-3 py-1 text-xs font-bold text-primary">
            {selectedCount} selected
          </span>
        )}
      </div>
      <div className="grid grid-cols-2 gap-3 p-3 sm:grid-cols-3 sm:p-4 md:grid-cols-4 xl:grid-cols-6">
        {group.items.map((media) => (
          <DuplicateMediaCard
            key={media.id}
            media={media}
            selected={selectedIds.has(media.id)}
            onToggle={() => onToggle(media.id)}
          />
        ))}
      </div>
    </section>
  )
}

interface DuplicateMediaCardProps {
  media: Media
  selected: boolean
  onToggle: () => void
}

function DuplicateMediaCard({ media, selected, onToggle }: DuplicateMediaCardProps) {
  const cardRef = useRef<HTMLButtonElement>(null)
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(() => mediaApi.getCachedThumbnailUrl(media.id) ?? null)

  useEffect(() => {
    if (thumbnailUrl || !cardRef.current) return
    let cancelled = false

    const observer = new IntersectionObserver((entries) => {
      if (!entries[0]?.isIntersecting) return
      observer.disconnect()
      batchLoader.load(media.id)
        .then((url) => {
          if (!cancelled && url) setThumbnailUrl(url)
        })
        .catch(() => {
          console.error(`Failed to load thumbnail for media ${media.id}`)
        })
    }, { rootMargin: '300px' })

    observer.observe(cardRef.current)
    return () => {
      cancelled = true
      observer.disconnect()
    }
  }, [media.id, thumbnailUrl])

  return (
    <button
      ref={cardRef}
      type="button"
      aria-pressed={selected}
      aria-label={`${selected ? 'Deselect' : 'Select'} ${media.originalFilename}`}
      onClick={onToggle}
      className={cn(
        'group relative min-h-11 overflow-hidden rounded-lg border bg-background text-left transition-all focus-visible:ring-offset-2',
        selected ? 'border-primary ring-2 ring-primary' : 'border-border hover:border-primary/50 hover:shadow-md',
      )}
    >
      <div className="relative aspect-square overflow-hidden bg-muted">
        {thumbnailUrl ? (
          <img src={thumbnailUrl} alt="" className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105" />
        ) : (
          <div className="h-full w-full animate-pulse bg-muted" aria-hidden="true" />
        )}
        <span
          aria-hidden="true"
          className={cn(
            'absolute left-2 top-2 flex h-7 w-7 items-center justify-center rounded-md border-2 shadow-sm transition-colors',
            selected ? 'border-primary bg-primary text-primary-foreground' : 'border-white/80 bg-black/40 text-white',
          )}
        >
          {selected ? <Check className="h-4 w-4" /> : <Square className="h-3.5 w-3.5" />}
        </span>
        {media.mediaType === 'video' && (
          <span className="absolute right-2 top-2 rounded-md border border-white/20 bg-black/60 p-1.5 text-white backdrop-blur-sm">
            <Play className="h-3.5 w-3.5 fill-current" />
          </span>
        )}
      </div>
      <div className="space-y-1 p-3">
        <p className="truncate text-xs font-bold text-foreground" title={media.originalFilename}>{media.originalFilename}</p>
        <p className="truncate text-[11px] font-medium text-muted-foreground">{formatFileSize(media.fileSize)}</p>
        <p className="truncate text-[11px] text-muted-foreground">{formatDimensions(media)}</p>
      </div>
    </button>
  )
}

interface PageStateProps {
  icon: React.ReactNode
  title: string
  description: string
  action?: React.ReactNode
}

function PageState({ icon, title, description, action }: PageStateProps) {
  return (
    <div className="flex min-h-[360px] flex-col items-center justify-center rounded-xl border border-dashed border-border bg-muted/10 px-6 py-16 text-center">
      <div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-border bg-background shadow-sm">
        {icon}
      </div>
      <h2 className="font-display text-xl font-semibold text-foreground">{title}</h2>
      <p className="mt-2 max-w-md text-sm text-muted-foreground">{description}</p>
      {action && <div className="mt-6">{action}</div>}
    </div>
  )
}

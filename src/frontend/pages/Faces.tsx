import { useRef, useState, type RefObject } from 'react'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Check, ChevronLeft, Loader2, UsersRound } from 'lucide-react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import { facesApi, type FaceGroup } from '../api/faces'
import type { Media } from '../api/types'
import PageState from '../components/common/PageState'
import { PageFrame, PageHeader } from '../components/layout/PageLayout'
import PhotoGrid from '../components/timeline/PhotoGrid'
import ManagedLightbox from '../components/viewer/ManagedLightbox'
import { useAuth } from '../hooks/useAuth'
import { useInfiniteScroll } from '../hooks/useInfiniteScroll'
import { cn } from '../lib/utils'
import { queryKeys } from '../lib/queryKeys'
import { useLightbox } from '../hooks/useLightbox'

export default function Faces() {
  const { faceGroupId } = useParams()
  if (faceGroupId) return <FaceGroupDetail faceGroupId={Number(faceGroupId)} />
  return <FaceGroupList />
}

function FaceGroupsStatus({
  loading,
  failed,
  groupCount,
  onRetry,
}: {
  loading: boolean
  failed: boolean
  groupCount: number
  onRetry: () => void
}) {
  if (loading) {
    return (
      <PageState
        icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />}
        title="Loading face groups"
        description="Finding recognized people in your library..."
      />
    )
  }
  if (failed) {
    return (
      <PageState
        icon={<AlertCircle className="h-9 w-9 text-destructive" />}
        title="Unable to load face groups"
        description="Try the request again. Existing face groups have not changed."
        action={
          <button
            type="button"
            onClick={onRetry}
            className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90"
          >
            Try again
          </button>
        }
      />
    )
  }
  if (groupCount > 0) return null

  return (
    <PageState
      icon={<UsersRound className="h-10 w-10 text-muted-foreground/60" />}
      title="No face groups"
      description="Run Face Detection from the admin AI panel to recognize people in your library."
    />
  )
}

function FaceGroupGrid({
  groups,
  selectedGroupIds,
  selectable,
  onToggle,
}: {
  groups: FaceGroup[]
  selectedGroupIds: ReadonlySet<number>
  selectable: boolean
  onToggle: (faceGroupId: number) => void
}) {
  if (groups.length === 0) return null

  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 2xl:grid-cols-8">
      {groups.map((group) => (
        <FaceGroupCard
          key={group.faceGroupId}
          group={group}
          selected={selectedGroupIds.has(group.faceGroupId)}
          selectable={selectable}
          onToggle={() => onToggle(group.faceGroupId)}
        />
      ))}
    </div>
  )
}

function FaceGroupPagination({
  loadMoreReference,
  loading,
  failed,
  onRetry,
}: {
  loadMoreReference: RefObject<HTMLDivElement>
  loading: boolean
  failed: boolean
  onRetry: () => void
}) {
  return (
    <>
      <div
        ref={loadMoreReference}
        className="flex min-h-16 items-center justify-center"
        aria-hidden={!loading}
      >
        {loading && (
          <Loader2
            className="h-5 w-5 animate-spin text-primary"
            aria-label="Loading more face groups"
          />
        )}
      </div>
      {failed && (
        <div className="flex justify-center">
          <button
            type="button"
            onClick={onRetry}
            className="min-h-11 rounded-lg border border-border bg-background px-5 py-2 text-sm font-bold text-foreground hover:bg-muted"
          >
            Retry loading face groups
          </button>
        </div>
      )}
    </>
  )
}

function FaceMergeToolbar({
  selectedCount,
  pending,
  onClear,
  onMerge,
}: {
  selectedCount: number
  pending: boolean
  onClear: () => void
  onMerge: () => void
}) {
  if (selectedCount === 0) return null

  return (
    <div className="sticky bottom-4 z-30 mt-8 flex flex-col gap-3 rounded-xl border border-border bg-background/95 p-3 shadow-xl backdrop-blur-md sm:flex-row sm:items-center sm:justify-between sm:p-4">
      <div className="px-1">
        <p className="font-bold text-foreground">{selectedCount} groups selected</p>
        <p className="text-xs text-muted-foreground">
          Merge combines the selected groups into one curated group.
        </p>
      </div>
      <div className="flex gap-2">
        <button
          type="button"
          onClick={onClear}
          disabled={pending}
          className="min-h-11 rounded-lg px-4 py-2 text-sm font-bold text-muted-foreground hover:bg-muted disabled:opacity-50"
        >
          Clear
        </button>
        <button
          type="button"
          onClick={onMerge}
          disabled={selectedCount < 2 || pending}
          className="flex min-h-11 items-center justify-center gap-2 rounded-lg bg-primary px-5 py-2 text-sm font-bold text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {pending && <Loader2 className="h-4 w-4 animate-spin" />}
          Merge groups
        </button>
      </div>
    </div>
  )
}

function FaceGroupList() {
  const { user } = useAuth()
  const queryClient = useQueryClient()
  const [selectedGroupIds, setSelectedGroupIds] = useState<Set<number>>(new Set())
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const loadMoreRef = useRef<HTMLDivElement>(null)
  const groupsQuery = useInfiniteQuery({
    queryKey: queryKeys.faces.groups,
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => facesApi.listGroups({ cursor: pageParam, limit: 100 }),
    getNextPageParam: (lastPage) =>
      lastPage.hasMore && lastPage.nextCursor ? lastPage.nextCursor : undefined,
  })
  const { fetchNextPage, hasNextPage, isFetchNextPageError, isFetchingNextPage } = groupsQuery
  const groups = groupsQuery.data?.pages.flatMap((page) => page.groups) ?? []
  const mergeMutation = useMutation({
    mutationFn: facesApi.mergeGroups,
    onSuccess: () => {
      setSelectedGroupIds(new Set())
      queryClient.invalidateQueries({ queryKey: queryKeys.faces.all })
    },
  })

  const toggleSelection = (faceGroupId: number) => {
    setSelectedGroupIds((currentGroupIds) => {
      const nextGroupIds = new Set(currentGroupIds)
      if (nextGroupIds.has(faceGroupId)) {
        nextGroupIds.delete(faceGroupId)
      } else {
        nextGroupIds.add(faceGroupId)
      }
      return nextGroupIds
    })
  }

  const handleMerge = () => {
    if (selectedGroupIds.size < 2) return
    mergeMutation.mutate({ faceGroupIds: Array.from(selectedGroupIds) })
  }

  useInfiniteScroll({
    scrollContainerRef,
    loadMoreRef,
    hasNextPage,
    isFetchingNextPage,
    isFetchNextPageError,
    fetchNextPage,
  })

  return (
    <div
      ref={scrollContainerRef}
      className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent"
    >
      <PageFrame className="animate-fade-in pb-28">
        <PageHeader
          title="Faces"
          description="People recognized across your library."
          actions={
            user?.role === 'admin' && groupsQuery.data ? (
              <span className="text-sm font-medium text-muted-foreground">
                {groups.length} groups loaded
              </span>
            ) : null
          }
        />

        <FaceGroupsStatus
          loading={groupsQuery.isLoading}
          failed={groupsQuery.isError}
          groupCount={groups.length}
          onRetry={() => void groupsQuery.refetch()}
        />
        <FaceGroupGrid
          groups={groups}
          selectedGroupIds={selectedGroupIds}
          selectable={user?.role === 'admin'}
          onToggle={toggleSelection}
        />
        <FaceGroupPagination
          loadMoreReference={loadMoreRef}
          loading={isFetchingNextPage}
          failed={isFetchNextPageError}
          onRetry={() => void fetchNextPage()}
        />

        {mergeMutation.isError && (
          <p role="alert" className="mt-6 flex items-center gap-2 text-sm text-destructive">
            <AlertCircle className="h-4 w-4" />
            Unable to merge the selected face groups.
          </p>
        )}
        {user?.role === 'admin' && (
          <FaceMergeToolbar
            selectedCount={selectedGroupIds.size}
            pending={mergeMutation.isPending}
            onClear={() => setSelectedGroupIds(new Set())}
            onMerge={handleMerge}
          />
        )}
      </PageFrame>
    </div>
  )
}

function FaceGroupCard({
  group,
  selected,
  selectable,
  onToggle,
}: {
  group: FaceGroup
  selected: boolean
  selectable: boolean
  onToggle: () => void
}) {
  const thumbnailUrl = facesApi.getThumbnailURL({ faceGroupId: group.faceGroupId })

  return (
    <div
      className={cn(
        'group relative overflow-hidden rounded-xl border bg-card shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md',
        selected ? 'border-primary ring-2 ring-primary' : 'border-border'
      )}
    >
      <Link
        to={`/faces/${group.faceGroupId}`}
        aria-label={`Face group ${group.faceGroupId}, ${group.mediaCount} media`}
        className="block focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset"
      >
        <div className="relative aspect-square overflow-hidden bg-muted">
          {thumbnailUrl ? (
            <img
              src={thumbnailUrl}
              alt=""
              className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105"
            />
          ) : (
            <div className="h-full w-full animate-pulse bg-muted" aria-hidden="true" />
          )}
          <span className="absolute bottom-2 right-2 rounded-full bg-black/65 px-2.5 py-1 text-xs font-bold text-white shadow-sm backdrop-blur-sm">
            {group.mediaCount}
          </span>
        </div>
      </Link>
      {selectable && (
        <button
          type="button"
          aria-label={`${selected ? 'Deselect' : 'Select'} face group ${group.faceGroupId}`}
          aria-pressed={selected}
          onClick={onToggle}
          className={cn(
            'absolute left-2 top-2 flex h-8 w-8 items-center justify-center rounded-md border-2 shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary',
            selected
              ? 'border-primary bg-primary text-primary-foreground'
              : 'border-white/80 bg-black/40 text-white hover:bg-black/60'
          )}
        >
          {selected ? (
            <Check className="h-4 w-4" />
          ) : (
            <span className="h-3.5 w-3.5 rounded-sm border border-current" />
          )}
        </button>
      )}
    </div>
  )
}

function FaceGroupDetail({ faceGroupId }: { faceGroupId: number }) {
  const navigate = useNavigate()
  const lightbox = useLightbox()
  const groupQuery = useQuery({
    queryKey: queryKeys.faces.group(faceGroupId),
    queryFn: () => facesApi.getGroup({ faceGroupId }),
  })
  const openMedia = (media: Media) => {
    const mediaIds = groupQuery.data?.media.map((groupMedia) => groupMedia.id) ?? []
    lightbox.open(media.id, mediaIds)
  }

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <PageFrame className="animate-fade-in">
        <button
          type="button"
          onClick={() => navigate('/faces')}
          className="mb-6 flex min-h-10 items-center gap-2 rounded-lg px-3 text-sm font-bold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <ChevronLeft className="h-4 w-4" />
          All faces
        </button>
        {groupQuery.isLoading ? (
          <PageState
            icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />}
            title="Loading face group"
            description="Retrieving associated media..."
          />
        ) : null}
        {groupQuery.isError ? (
          <PageState
            icon={<AlertCircle className="h-9 w-9 text-destructive" />}
            title="Unable to load face group"
            description="Try the request again."
            action={
              <button
                type="button"
                onClick={() => groupQuery.refetch()}
                className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90"
              >
                Try again
              </button>
            }
          />
        ) : null}
        {groupQuery.data ? (
          <>
            <PageHeader
              title="Face group"
              description={`${groupQuery.data.group.faceCount} recognized faces across ${groupQuery.data.media.length} media items.`}
              actions={null}
            />
            {groupQuery.data.media.length > 0 ? (
              <PhotoGrid media={groupQuery.data.media} onPhotoClick={openMedia} selection={null} />
            ) : (
              <PageState
                icon={<UsersRound className="h-10 w-10 text-muted-foreground/60" />}
                title="No associated media"
                description="This face group has no accessible media."
              />
            )}
          </>
        ) : null}
      </PageFrame>
      <ManagedLightbox controller={lightbox} />
    </div>
  )
}

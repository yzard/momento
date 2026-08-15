import { useEffect, useRef, useState } from 'react'
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { AlertCircle, Check, ChevronLeft, Loader2, UsersRound } from 'lucide-react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import { facesApi, type FaceGroup } from '../api/faces'
import type { Media } from '../api/types'
import PhotoGrid from '../components/timeline/PhotoGrid'
import Lightbox from '../components/viewer/Lightbox'
import { useAuth } from '../hooks/useAuth'
import { cn } from '../lib/utils'

export default function Faces() {
  const { faceGroupId } = useParams()
  if (faceGroupId) return <FaceGroupDetail faceGroupId={Number(faceGroupId)} />
  return <FaceGroupList />
}

function FaceGroupList() {
  const { user } = useAuth()
  const queryClient = useQueryClient()
  const [selectedGroupIds, setSelectedGroupIds] = useState<Set<number>>(new Set())
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const loadMoreRef = useRef<HTMLDivElement>(null)
  const groupsQuery = useInfiniteQuery({
    queryKey: ['faces', 'groups'],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => facesApi.listGroups({ cursor: pageParam, limit: 100 }),
    getNextPageParam: (lastPage) => lastPage.hasMore && lastPage.nextCursor
      ? lastPage.nextCursor
      : undefined,
  })
  const {
    fetchNextPage,
    hasNextPage,
    isFetchNextPageError,
    isFetchingNextPage,
  } = groupsQuery
  const groups = groupsQuery.data?.pages.flatMap((page) => page.groups) ?? []
  const mergeMutation = useMutation({
    mutationFn: facesApi.mergeGroups,
    onSuccess: () => {
      setSelectedGroupIds(new Set())
      queryClient.invalidateQueries({ queryKey: ['faces'] })
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

  useEffect(() => {
    const target = loadMoreRef.current
    const root = scrollContainerRef.current
    if (!target || !root || !hasNextPage || isFetchNextPageError) return

    const observer = new IntersectionObserver((entries) => {
      if (!entries[0]?.isIntersecting || isFetchingNextPage) return
      void fetchNextPage()
    }, { root, rootMargin: '0px 0px 320px 0px' })
    observer.observe(target)
    return () => observer.disconnect()
  }, [
    fetchNextPage,
    hasNextPage,
    isFetchNextPageError,
    isFetchingNextPage,
  ])

  return (
    <div ref={scrollContainerRef} className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="container mx-auto max-w-[1800px] px-4 py-6 pb-28 sm:px-6 md:px-10 md:py-10 animate-fade-in">
        <div className="mb-8 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">Faces</h1>
            <p className="mt-1 text-sm text-muted-foreground">People recognized across your library.</p>
          </div>
          {user?.role === 'admin' && groupsQuery.data && (
            <span className="text-sm font-medium text-muted-foreground">{groups.length} groups loaded</span>
          )}
        </div>

        {groupsQuery.isLoading ? <PageState icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />} title="Loading face groups" description="Finding recognized people in your library..." /> : null}
        {groupsQuery.isError ? <PageState icon={<AlertCircle className="h-9 w-9 text-destructive" />} title="Unable to load face groups" description="Try the request again. Existing face groups have not changed." action={<button type="button" onClick={() => groupsQuery.refetch()} className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90">Try again</button>} /> : null}
        {!groupsQuery.isLoading && !groupsQuery.isError && groups.length === 0 ? <PageState icon={<UsersRound className="h-10 w-10 text-muted-foreground/60" />} title="No face groups" description="Run Face Detection from the admin AI panel to recognize people in your library." /> : null}
        {groups.length > 0 ? (
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
            {groups.map((group) => <FaceGroupCard key={group.faceGroupId} group={group} selected={selectedGroupIds.has(group.faceGroupId)} selectable={user?.role === 'admin'} onToggle={() => toggleSelection(group.faceGroupId)} />)}
          </div>
        ) : null}
        <div ref={loadMoreRef} className="flex min-h-16 items-center justify-center" aria-hidden={!isFetchingNextPage}>{isFetchingNextPage ? <Loader2 className="h-5 w-5 animate-spin text-primary" aria-label="Loading more face groups" /> : null}</div>
        {isFetchNextPageError ? <div className="flex justify-center"><button type="button" onClick={() => void fetchNextPage()} className="min-h-11 rounded-lg border border-border bg-background px-5 py-2 text-sm font-bold text-foreground hover:bg-muted">Retry loading face groups</button></div> : null}

        {mergeMutation.isError && <p role="alert" className="mt-6 flex items-center gap-2 text-sm text-destructive"><AlertCircle className="h-4 w-4" />Unable to merge the selected face groups.</p>}
        {user?.role === 'admin' && selectedGroupIds.size > 0 && (
          <div className="sticky bottom-4 z-30 mt-8 flex flex-col gap-3 rounded-xl border border-border bg-background/95 p-3 shadow-xl backdrop-blur-md sm:flex-row sm:items-center sm:justify-between sm:p-4">
            <div className="px-1"><p className="font-bold text-foreground">{selectedGroupIds.size} groups selected</p><p className="text-xs text-muted-foreground">Merge combines the selected groups into one curated group.</p></div>
            <div className="flex gap-2"><button type="button" onClick={() => setSelectedGroupIds(new Set())} disabled={mergeMutation.isPending} className="min-h-11 rounded-lg px-4 py-2 text-sm font-bold text-muted-foreground hover:bg-muted disabled:opacity-50">Clear</button><button type="button" onClick={handleMerge} disabled={selectedGroupIds.size < 2 || mergeMutation.isPending} className="flex min-h-11 items-center justify-center gap-2 rounded-lg bg-primary px-5 py-2 text-sm font-bold text-primary-foreground hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-50">{mergeMutation.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : null}Merge groups</button></div>
          </div>
        )}
      </div>
    </div>
  )
}

function FaceGroupCard({ group, selected, selectable, onToggle }: { group: FaceGroup; selected: boolean; selectable: boolean; onToggle: () => void }) {
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    facesApi.getThumbnailUrl({ faceGroupId: group.faceGroupId }).then((url) => {
      if (!cancelled) setThumbnailUrl(url)
    }).catch(() => undefined)
    return () => { cancelled = true }
  }, [group.faceGroupId])

  return (
    <div className={cn('group relative overflow-hidden rounded-xl border bg-card shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md', selected ? 'border-primary ring-2 ring-primary' : 'border-border')}>
      <Link to={`/faces/${group.faceGroupId}`} aria-label={`Face group ${group.faceGroupId}, ${group.mediaCount} media`} className="block focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-inset">
        <div className="relative aspect-square overflow-hidden bg-muted">{thumbnailUrl ? <img src={thumbnailUrl} alt="" className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105" /> : <div className="h-full w-full animate-pulse bg-muted" aria-hidden="true" />}<span className="absolute bottom-2 right-2 rounded-full bg-black/65 px-2.5 py-1 text-xs font-bold text-white shadow-sm backdrop-blur-sm">{group.mediaCount}</span></div>
      </Link>
      {selectable && <button type="button" aria-label={`${selected ? 'Deselect' : 'Select'} face group ${group.faceGroupId}`} aria-pressed={selected} onClick={onToggle} className={cn('absolute left-2 top-2 flex h-8 w-8 items-center justify-center rounded-md border-2 shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary', selected ? 'border-primary bg-primary text-primary-foreground' : 'border-white/80 bg-black/40 text-white hover:bg-black/60')}>{selected ? <Check className="h-4 w-4" /> : <span className="h-3.5 w-3.5 rounded-sm border border-current" />}</button>}
    </div>
  )
}

function FaceGroupDetail({ faceGroupId }: { faceGroupId: number }) {
  const navigate = useNavigate()
  const [viewer, setViewer] = useState<{ mediaIds: number[]; currentIndex: number } | null>(null)
  const groupQuery = useQuery({ queryKey: ['faces', 'groups', faceGroupId], queryFn: () => facesApi.getGroup({ faceGroupId }) })
  const openMedia = (media: Media) => {
    const mediaIds = groupQuery.data?.media.map((groupMedia) => groupMedia.id) ?? []
    const currentIndex = mediaIds.indexOf(media.id)
    setViewer({ mediaIds, currentIndex: currentIndex >= 0 ? currentIndex : 0 })
  }

  return (
    <div className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="container mx-auto max-w-[1800px] px-4 py-6 pb-20 sm:px-6 md:px-10 md:py-10 animate-fade-in">
        <button type="button" onClick={() => navigate('/faces')} className="mb-6 flex min-h-10 items-center gap-2 rounded-lg px-3 text-sm font-bold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"><ChevronLeft className="h-4 w-4" />All faces</button>
        {groupQuery.isLoading ? <PageState icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />} title="Loading face group" description="Retrieving associated media..." /> : null}
        {groupQuery.isError ? <PageState icon={<AlertCircle className="h-9 w-9 text-destructive" />} title="Unable to load face group" description="Try the request again." action={<button type="button" onClick={() => groupQuery.refetch()} className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90">Try again</button>} /> : null}
        {groupQuery.data ? <><div className="mb-8"><h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">Face group</h1><p className="mt-1 text-sm text-muted-foreground">{groupQuery.data.group.faceCount} recognized faces across {groupQuery.data.media.length} media items.</p></div>{groupQuery.data.media.length > 0 ? <PhotoGrid media={groupQuery.data.media} onPhotoClick={openMedia} /> : <PageState icon={<UsersRound className="h-10 w-10 text-muted-foreground/60" />} title="No associated media" description="This face group has no accessible media." />}</> : null}
      </div>
      {viewer && <Lightbox mediaIds={viewer.mediaIds} currentIndex={viewer.currentIndex} onClose={() => setViewer(null)} onIndexChange={(currentIndex) => setViewer((currentViewer) => currentViewer ? { ...currentViewer, currentIndex } : null)} />}
    </div>
  )
}

function PageState({ icon, title, description, action }: { icon: React.ReactNode; title: string; description: string; action?: React.ReactNode }) {
  return <div className="flex min-h-[360px] flex-col items-center justify-center rounded-xl border border-dashed border-border bg-muted/10 px-6 py-16 text-center"><div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-border bg-background shadow-sm">{icon}</div><h2 className="font-display text-xl font-semibold text-foreground">{title}</h2><p className="mt-2 max-w-md text-sm text-muted-foreground">{description}</p>{action ? <div className="mt-6">{action}</div> : null}</div>
}

import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { useInfiniteQuery } from '@tanstack/react-query'
import { AlertCircle, ChevronLeft, ImageOff, Loader2, MapPinned } from 'lucide-react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import { placesApi, type PlaceSummary } from '../api/places'
import type { Media } from '../api/types'
import MemoryCardOverlay from '../components/common/MemoryCardOverlay'
import PhotoGrid from '../components/timeline/PhotoGrid'
import Lightbox from '../components/viewer/Lightbox'

const PAGE_LIMIT = 100

interface InfiniteScrollOptions {
  scrollContainerRef: RefObject<HTMLDivElement>
  loadMoreRef: RefObject<HTMLDivElement>
  hasNextPage: boolean
  isFetchingNextPage: boolean
  isFetchNextPageError: boolean
  fetchNextPage: () => Promise<unknown>
}

function useInfiniteScroll({
  scrollContainerRef,
  loadMoreRef,
  hasNextPage,
  isFetchingNextPage,
  isFetchNextPageError,
  fetchNextPage,
}: InfiniteScrollOptions) {
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
  }, [fetchNextPage, hasNextPage, isFetchNextPageError, isFetchingNextPage, loadMoreRef, scrollContainerRef])
}

export default function Places() {
  const { placeId } = useParams()
  if (placeId) return <PlaceDetail placeId={placeId} />
  return <PlaceList />
}

function PlaceList() {
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const loadMoreRef = useRef<HTMLDivElement>(null)
  const placesQuery = useInfiniteQuery({
    queryKey: ['places'],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => placesApi.list({ cursor: pageParam, limit: PAGE_LIMIT }),
    getNextPageParam: (lastPage) => lastPage.hasMore && lastPage.nextCursor
      ? lastPage.nextCursor
      : undefined,
  })
  const places = placesQuery.data?.pages.flatMap((page) => page.places) ?? []

  useInfiniteScroll({
    scrollContainerRef,
    loadMoreRef,
    hasNextPage: placesQuery.hasNextPage,
    isFetchingNextPage: placesQuery.isFetchingNextPage,
    isFetchNextPageError: placesQuery.isFetchNextPageError,
    fetchNextPage: placesQuery.fetchNextPage,
  })

  return (
    <div ref={scrollContainerRef} className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="container mx-auto max-w-[1800px] px-4 py-6 pb-20 sm:px-6 md:px-10 md:py-10 animate-fade-in">
        <div className="mb-8">
          <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">Places</h1>
          <p className="mt-1 text-sm text-muted-foreground">Explore your library by city.</p>
        </div>

        {placesQuery.isLoading ? <PageState icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />} title="Loading places" description="Finding locations across your library..." /> : null}
        {placesQuery.isError ? <PageState icon={<AlertCircle className="h-9 w-9 text-destructive" />} title="Unable to load places" description="Try the request again." action={<button type="button" onClick={() => placesQuery.refetch()} className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90">Try again</button>} /> : null}
        {!placesQuery.isLoading && !placesQuery.isError && places.length === 0 ? <PageState icon={<MapPinned className="h-10 w-10 text-muted-foreground/60" />} title="No places" description="Media with recognized city information will appear here." /> : null}

        {places.length > 0 ? (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {places.map((place) => <PlaceCard key={place.placeId} place={place} />)}
          </div>
        ) : null}

        <PaginationStatus
          loadMoreRef={loadMoreRef}
          isFetching={placesQuery.isFetchingNextPage}
          hasError={placesQuery.isFetchNextPageError}
          retry={() => void placesQuery.fetchNextPage()}
          label="places"
        />
      </div>
    </div>
  )
}

function PlaceCard({ place }: { place: PlaceSummary }) {
  const cardRef = useRef<HTMLAnchorElement>(null)
  const [thumbnailUrl, setThumbnailUrl] = useState<string | null>(null)
  const region = [place.state, place.country].filter(Boolean).join(', ')
  const accessibleLocation = [place.city, place.state, place.country].filter(Boolean).join(', ')

  useEffect(() => {
    const card = cardRef.current
    if (!card || thumbnailUrl) return

    let cancelled = false
    const observer = new IntersectionObserver((entries) => {
      if (!entries[0]?.isIntersecting) return
      observer.disconnect()
      placesApi.getThumbnail(place.placeId)
        .then((url) => {
          if (!cancelled) setThumbnailUrl(url)
        })
        .catch(() => undefined)
    }, { rootMargin: '400px' })

    observer.observe(card)
    return () => {
      cancelled = true
      observer.disconnect()
    }
  }, [place.placeId, thumbnailUrl])

  return (
    <Link
      ref={cardRef}
      to={`/places/${encodeURIComponent(place.placeId)}`}
      aria-label={`${accessibleLocation}, ${place.mediaCount} media`}
      className="group relative aspect-[3/2] overflow-hidden rounded-xl border border-border bg-card shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    >
      {thumbnailUrl ? <img src={thumbnailUrl} alt="" loading="lazy" className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105" /> : <div className="flex h-full w-full items-center justify-center bg-muted"><MapPinned className="h-9 w-9 text-muted-foreground/40" aria-hidden="true" /></div>}
      <MemoryCardOverlay
        title={place.city}
        subtitle={region}
        badge={`${place.mediaCount} media`}
        headingLevel="h2"
      />
    </Link>
  )
}

function PlaceDetail({ placeId }: { placeId: string }) {
  const navigate = useNavigate()
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const loadMoreRef = useRef<HTMLDivElement>(null)
  const [viewerIndex, setViewerIndex] = useState<number | null>(null)
  const placeQuery = useInfiniteQuery({
    queryKey: ['places', placeId],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => placesApi.get({ placeId, cursor: pageParam, limit: PAGE_LIMIT }),
    getNextPageParam: (lastPage) => lastPage.hasMore && lastPage.nextCursor
      ? lastPage.nextCursor
      : undefined,
  })
  const place = placeQuery.data?.pages[0]?.place
  const media = placeQuery.data?.pages.flatMap((page) => page.media) ?? []
  const mediaIds = media.map((placeMedia) => placeMedia.id)

  useInfiniteScroll({
    scrollContainerRef,
    loadMoreRef,
    hasNextPage: placeQuery.hasNextPage,
    isFetchingNextPage: placeQuery.isFetchingNextPage,
    isFetchNextPageError: placeQuery.isFetchNextPageError,
    fetchNextPage: placeQuery.fetchNextPage,
  })

  const openMedia = (selectedMedia: Media) => {
    const selectedIndex = media.findIndex((placeMedia) => placeMedia.id === selectedMedia.id)
    setViewerIndex(selectedIndex >= 0 ? selectedIndex : 0)
  }

  return (
    <div ref={scrollContainerRef} className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent">
      <div className="container mx-auto max-w-[1800px] px-4 py-6 pb-20 sm:px-6 md:px-10 md:py-10 animate-fade-in">
        <button type="button" onClick={() => navigate('/places')} className="mb-6 flex min-h-10 items-center gap-2 rounded-lg px-3 text-sm font-bold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"><ChevronLeft className="h-4 w-4" />All places</button>

        {placeQuery.isLoading ? <PageState icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />} title="Loading place" description="Retrieving associated media..." /> : null}
        {placeQuery.isError ? <PageState icon={<AlertCircle className="h-9 w-9 text-destructive" />} title="Unable to load place" description="Try the request again." action={<button type="button" onClick={() => placeQuery.refetch()} className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90">Try again</button>} /> : null}

        {place ? (
          <>
            <div className="mb-8">
              <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">{place.city}, {place.country}</h1>
              <p className="mt-1 text-sm text-muted-foreground">{place.mediaCount} media in this place.</p>
            </div>
            {media.length > 0 ? <PhotoGrid media={media} onPhotoClick={openMedia} /> : <PageState icon={<ImageOff className="h-10 w-10 text-muted-foreground/60" />} title="No associated media" description="This place has no accessible media." />}
          </>
        ) : null}

        <PaginationStatus
          loadMoreRef={loadMoreRef}
          isFetching={placeQuery.isFetchingNextPage}
          hasError={placeQuery.isFetchNextPageError}
          retry={() => void placeQuery.fetchNextPage()}
          label="media"
        />
      </div>

      {viewerIndex !== null ? <Lightbox mediaIds={mediaIds} currentIndex={viewerIndex} onClose={() => setViewerIndex(null)} onIndexChange={setViewerIndex} /> : null}
    </div>
  )
}

function PaginationStatus({ loadMoreRef, isFetching, hasError, retry, label }: { loadMoreRef: RefObject<HTMLDivElement>; isFetching: boolean; hasError: boolean; retry: () => void; label: string }) {
  return (
    <div ref={loadMoreRef} className="flex min-h-16 items-center justify-center" aria-live="polite">
      {isFetching ? <span className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="h-5 w-5 animate-spin text-primary" />Loading more {label}...</span> : null}
      {hasError ? <button type="button" onClick={retry} className="min-h-11 rounded-lg border border-border bg-background px-5 py-2 text-sm font-bold text-foreground hover:bg-muted">Retry loading {label}</button> : null}
    </div>
  )
}

function PageState({ icon, title, description, action }: { icon: ReactNode; title: string; description: string; action?: ReactNode }) {
  return <div className="flex min-h-[360px] flex-col items-center justify-center rounded-xl border border-dashed border-border bg-muted/10 px-6 py-16 text-center"><div className="mb-5 flex h-16 w-16 items-center justify-center rounded-2xl border border-border bg-background shadow-sm">{icon}</div><h2 className="font-display text-xl font-semibold text-foreground">{title}</h2><p className="mt-2 max-w-md text-sm text-muted-foreground">{description}</p>{action ? <div className="mt-6">{action}</div> : null}</div>
}

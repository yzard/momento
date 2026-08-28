import { useRef, type RefObject } from 'react'
import { useInfiniteQuery } from '@tanstack/react-query'
import { AlertCircle, ChevronLeft, ImageOff, Loader2, MapPinned } from 'lucide-react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import { placesApi, type PlaceSummary } from '../api/places'
import type { Media } from '../api/types'
import MemoryCardOverlay from '../components/common/MemoryCardOverlay'
import PageState from '../components/common/PageState'
import PhotoGrid from '../components/timeline/PhotoGrid'
import ManagedLightbox from '../components/viewer/ManagedLightbox'
import { useInfiniteScroll } from '../hooks/useInfiniteScroll'
import { queryKeys } from '../lib/queryKeys'
import { useLazyImage } from '../hooks/useLazyImage'
import { useLightbox } from '../hooks/useLightbox'

const PAGE_LIMIT = 100
const placeThumbnailLoader = { load: placesApi.getThumbnail }

export default function Places() {
  const { placeId } = useParams()
  if (placeId) return <PlaceDetail placeId={placeId} />
  return <PlaceList />
}

function PlaceList() {
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const loadMoreRef = useRef<HTMLDivElement>(null)
  const placesQuery = useInfiniteQuery({
    queryKey: queryKeys.places.all,
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => placesApi.list({ cursor: pageParam, limit: PAGE_LIMIT }),
    getNextPageParam: (lastPage) =>
      lastPage.hasMore && lastPage.nextCursor ? lastPage.nextCursor : undefined,
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
    <div
      ref={scrollContainerRef}
      className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent"
    >
      <div className="container mx-auto max-w-[1800px] px-4 py-6 pb-20 sm:px-6 md:px-10 md:py-10 animate-fade-in">
        <div className="mb-8">
          <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">
            Places
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">Explore your library by city.</p>
        </div>

        {placesQuery.isLoading ? (
          <PageState
            icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />}
            title="Loading places"
            description="Finding locations across your library..."
          />
        ) : null}
        {placesQuery.isError ? (
          <PageState
            icon={<AlertCircle className="h-9 w-9 text-destructive" />}
            title="Unable to load places"
            description="Try the request again."
            action={
              <button
                type="button"
                onClick={() => placesQuery.refetch()}
                className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90"
              >
                Try again
              </button>
            }
          />
        ) : null}
        {!placesQuery.isLoading && !placesQuery.isError && places.length === 0 ? (
          <PageState
            icon={<MapPinned className="h-10 w-10 text-muted-foreground/60" />}
            title="No places"
            description="Media with recognized city information will appear here."
          />
        ) : null}

        {places.length > 0 ? (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
            {places.map((place) => (
              <PlaceCard key={place.placeId} place={place} />
            ))}
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
  const { targetRef: cardRef, imageUrl: thumbnailUrl } = useLazyImage<HTMLAnchorElement, string>({
    resourceId: place.placeId,
    loader: placeThumbnailLoader,
    getCachedUrl: null,
    rootMargin: '400px',
  })
  const region = [place.state, place.country].filter(Boolean).join(', ')
  const accessibleLocation = [place.city, place.state, place.country].filter(Boolean).join(', ')

  return (
    <Link
      ref={cardRef}
      to={`/places/${encodeURIComponent(place.placeId)}`}
      aria-label={`${accessibleLocation}, ${place.mediaCount} media`}
      className="group relative aspect-[3/2] overflow-hidden rounded-xl border border-border bg-card shadow-sm transition-all hover:-translate-y-0.5 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
    >
      {thumbnailUrl ? (
        <img
          src={thumbnailUrl}
          alt=""
          loading="lazy"
          className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-105"
        />
      ) : (
        <div className="flex h-full w-full items-center justify-center bg-muted">
          <MapPinned className="h-9 w-9 text-muted-foreground/40" aria-hidden="true" />
        </div>
      )}
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
  const lightbox = useLightbox()
  const placeQuery = useInfiniteQuery({
    queryKey: queryKeys.places.detail(placeId),
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => placesApi.get({ placeId, cursor: pageParam, limit: PAGE_LIMIT }),
    getNextPageParam: (lastPage) =>
      lastPage.hasMore && lastPage.nextCursor ? lastPage.nextCursor : undefined,
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
    lightbox.open(selectedMedia.id, mediaIds)
  }

  return (
    <div
      ref={scrollContainerRef}
      className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-muted-foreground/20 scrollbar-track-transparent"
    >
      <div className="container mx-auto max-w-[1800px] px-4 py-6 pb-20 sm:px-6 md:px-10 md:py-10 animate-fade-in">
        <button
          type="button"
          onClick={() => navigate('/places')}
          className="mb-6 flex min-h-10 items-center gap-2 rounded-lg px-3 text-sm font-bold text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <ChevronLeft className="h-4 w-4" />
          All places
        </button>

        {placeQuery.isLoading ? (
          <PageState
            icon={<Loader2 className="h-9 w-9 animate-spin text-primary" />}
            title="Loading place"
            description="Retrieving associated media..."
          />
        ) : null}
        {placeQuery.isError ? (
          <PageState
            icon={<AlertCircle className="h-9 w-9 text-destructive" />}
            title="Unable to load place"
            description="Try the request again."
            action={
              <button
                type="button"
                onClick={() => placeQuery.refetch()}
                className="min-h-11 rounded-lg bg-foreground px-5 py-2 text-sm font-bold text-background hover:bg-foreground/90"
              >
                Try again
              </button>
            }
          />
        ) : null}

        {place ? (
          <>
            <div className="mb-8">
              <h1 className="font-display text-3xl font-semibold tracking-tight text-foreground">
                {place.city}, {place.country}
              </h1>
              <p className="mt-1 text-sm text-muted-foreground">
                {place.mediaCount} media in this place.
              </p>
            </div>
            {media.length > 0 ? (
              <PhotoGrid media={media} onPhotoClick={openMedia} selection={null} />
            ) : (
              <PageState
                icon={<ImageOff className="h-10 w-10 text-muted-foreground/60" />}
                title="No associated media"
                description="This place has no accessible media."
              />
            )}
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

      <ManagedLightbox controller={lightbox} />
    </div>
  )
}

function PaginationStatus({
  loadMoreRef,
  isFetching,
  hasError,
  retry,
  label,
}: {
  loadMoreRef: RefObject<HTMLDivElement>
  isFetching: boolean
  hasError: boolean
  retry: () => void
  label: string
}) {
  return (
    <div ref={loadMoreRef} className="flex min-h-16 items-center justify-center" aria-live="polite">
      {isFetching ? (
        <span className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="h-5 w-5 animate-spin text-primary" />
          Loading more {label}...
        </span>
      ) : null}
      {hasError ? (
        <button
          type="button"
          onClick={retry}
          className="min-h-11 rounded-lg border border-border bg-background px-5 py-2 text-sm font-bold text-foreground hover:bg-muted"
        >
          Retry loading {label}
        </button>
      ) : null}
    </div>
  )
}

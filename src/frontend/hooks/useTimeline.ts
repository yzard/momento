import { useCallback, useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  mediaApi,
  type GroupBy,
  type MediaTypeFilter,
  type TimelineClassification,
  type TimelineListRequest,
  type TimelineListResponse,
  type TimelineMarker,
} from '../api/media'
import type { Media, TimelineGroup } from '../api/types'
import { queryKeys } from '../lib/queryKeys'

interface TimelineWindowOptions {
  groupBy: GroupBy
  search: string
  mediaType: MediaTypeFilter | null
  classification: TimelineClassification | null
  marker: TimelineMarker | null
  refreshKey: number
}

interface TimelinePageEntry {
  response: TimelineListResponse
}

type CachedPage = TimelineListResponse | Promise<TimelineListResponse>
const timelinePageLimit = 100
const maximumTimelinePages = 6
const maximumCachedPages = 12

type TimelineDirection = 'older' | 'newer'

function boundedTimelineEntries(
  entries: TimelinePageEntry[],
  response: TimelineListResponse,
  direction: TimelineDirection
): TimelinePageEntry[] {
  const nextEntries =
    direction === 'older' ? [...entries, { response }] : [{ response }, ...entries]
  if (nextEntries.length <= maximumTimelinePages) return nextEntries
  return direction === 'older'
    ? nextEntries.slice(-maximumTimelinePages)
    : nextEntries.slice(0, maximumTimelinePages)
}

function timelineRequest(
  options: Pick<TimelineWindowOptions, 'groupBy' | 'mediaType' | 'classification'> & {
    search: string
  },
  direction: TimelineDirection,
  cursor?: string,
  anchorDate?: string
): TimelineListRequest {
  return {
    limit: timelinePageLimit,
    groupBy: options.groupBy,
    search: options.search,
    mediaType: options.mediaType ?? undefined,
    classification: options.classification,
    direction,
    cursor,
    anchorDate,
  }
}

function mergeTimelinePages(pages: TimelinePageEntry[]): TimelineGroup[] {
  const mediaById = new Map<number, Media>()
  const mediaByGroup = new Map<string, Media[]>()

  for (const page of pages) {
    for (const group of page.response.groups) {
      const groupMedia = mediaByGroup.get(group.date) ?? []
      for (const media of group.media) {
        if (mediaById.has(media.id)) continue
        mediaById.set(media.id, media)
        groupMedia.push(media)
      }
      mediaByGroup.set(group.date, groupMedia)
    }
  }

  return Array.from(mediaByGroup, ([date, media]) => ({
    date,
    media: media.sort(
      (left, right) =>
        (right.dateTaken ?? '').localeCompare(left.dateTaken ?? '') || right.id - left.id
    ),
  })).sort((left, right) => right.date.localeCompare(left.date))
}

function timelineError(error: unknown): Error {
  return error instanceof Error ? error : new Error('Timeline request failed')
}

function useTimelinePageCache(contextKey: string) {
  const pageCacheRef = useRef<Map<string, CachedPage>>(new Map())

  useEffect(() => {
    pageCacheRef.current.clear()
  }, [contextKey])

  return useCallback(
    async (request: TimelineListRequest) => {
      const cacheKey = JSON.stringify([contextKey, request])
      const cached = pageCacheRef.current.get(cacheKey)
      if (cached) {
        pageCacheRef.current.delete(cacheKey)
        pageCacheRef.current.set(cacheKey, cached)
        return cached instanceof Promise ? cached : Promise.resolve(cached)
      }

      const pending = mediaApi
        .listTimeline(request)
        .then((response) => {
          pageCacheRef.current.set(cacheKey, response)
          while (pageCacheRef.current.size > maximumCachedPages) {
            const oldestKey = pageCacheRef.current.keys().next().value
            if (oldestKey === undefined) break
            pageCacheRef.current.delete(oldestKey)
          }
          return response
        })
        .catch((requestError: unknown) => {
          pageCacheRef.current.delete(cacheKey)
          throw requestError
        })
      pageCacheRef.current.set(cacheKey, pending)
      return pending
    },
    [contextKey]
  )
}

interface InitialTimelineLoad {
  isCurrent: () => boolean
  request: TimelineListRequest
  fetchPage: (request: TimelineListRequest) => Promise<TimelineListResponse>
  appendPage: (response: TimelineListResponse, direction: TimelineDirection) => void
  setError: (error: Error) => void
  finishLoading: () => void
}

async function loadInitialTimelinePage(load: InitialTimelineLoad): Promise<void> {
  try {
    const response = await load.fetchPage(load.request)
    if (load.isCurrent()) load.appendPage(response, 'older')
  } catch (requestError: unknown) {
    if (load.isCurrent()) load.setError(timelineError(requestError))
  } finally {
    if (load.isCurrent()) load.finishLoading()
  }
}

function useTimelineEntries(contextKey: string) {
  const generationRef = useRef(0)
  const entriesRef = useRef<TimelinePageEntry[]>([])
  const loadingOlderRef = useRef(false)
  const loadingNewerRef = useRef(false)
  const [entries, setEntries] = useState<TimelinePageEntry[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [isLoadingOlder, setIsLoadingOlder] = useState(false)
  const [isLoadingNewer, setIsLoadingNewer] = useState(false)
  const [error, setError] = useState<Error | null>(null)
  const fetchPage = useTimelinePageCache(contextKey)
  const appendPage = useCallback((response: TimelineListResponse, direction: TimelineDirection) => {
    const nextEntries = boundedTimelineEntries(entriesRef.current, response, direction)
    entriesRef.current = nextEntries
    setEntries(nextEntries)
  }, [])
  const beginGeneration = useCallback(() => {
    const generation = generationRef.current + 1
    generationRef.current = generation
    entriesRef.current = []
    loadingOlderRef.current = false
    loadingNewerRef.current = false
    setEntries([])
    setIsLoadingOlder(false)
    setIsLoadingNewer(false)
    setError(null)
    return generation
  }, [])

  return {
    generationRef,
    entriesRef,
    loadingOlderRef,
    loadingNewerRef,
    entries,
    isLoading,
    setIsLoading,
    isLoadingOlder,
    setIsLoadingOlder,
    isLoadingNewer,
    setIsLoadingNewer,
    error,
    setError,
    fetchPage,
    appendPage,
    beginGeneration,
  }
}

export function useTimelineMarkers(
  mediaType: MediaTypeFilter | null,
  classification: TimelineClassification | null,
  search: string
) {
  const normalizedSearch = search.trim()

  return useQuery({
    queryKey: queryKeys.timeline.markers(mediaType, classification, normalizedSearch),
    queryFn: () => mediaApi.getTimelineMarkers(mediaType, classification, normalizedSearch),
    staleTime: Infinity,
    gcTime: 1000 * 60 * 10,
    refetchOnMount: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
}

interface TimelineDirectionState {
  loadingReference: { current: boolean }
  setLoading: (loading: boolean) => void
  boundary: TimelinePageEntry | undefined
}

type TimelineEntriesState = ReturnType<typeof useTimelineEntries>

interface TimelineDirectionSources {
  entriesReference: TimelineEntriesState['entriesRef']
  loadingOlderReference: TimelineEntriesState['loadingOlderRef']
  loadingNewerReference: TimelineEntriesState['loadingNewerRef']
  setLoadingOlder: TimelineEntriesState['setIsLoadingOlder']
  setLoadingNewer: TimelineEntriesState['setIsLoadingNewer']
}

function timelineDirectionState(
  direction: TimelineDirection,
  sources: TimelineDirectionSources
): TimelineDirectionState {
  if (direction === 'older') {
    return {
      loadingReference: sources.loadingOlderReference,
      setLoading: sources.setLoadingOlder,
      boundary: sources.entriesReference.current.at(-1),
    }
  }

  return {
    loadingReference: sources.loadingNewerReference,
    setLoading: sources.setLoadingNewer,
    boundary: sources.entriesReference.current[0],
  }
}

function directionCursor(
  direction: TimelineDirection,
  boundary: TimelinePageEntry | undefined
): string | null {
  if (!boundary) return null
  if (direction === 'older') {
    return boundary.response.hasOlder ? (boundary.response.nextCursor ?? null) : null
  }
  return boundary.response.hasNewer ? (boundary.response.previousCursor ?? null) : null
}

function useTimelinePageLoader({
  groupBy,
  normalizedSearch,
  mediaType,
  classification,
  timelineEntries,
}: Pick<TimelineWindowOptions, 'groupBy' | 'mediaType' | 'classification'> & {
  normalizedSearch: string
  timelineEntries: ReturnType<typeof useTimelineEntries>
}) {
  const {
    entriesRef,
    loadingOlderRef,
    loadingNewerRef,
    setIsLoadingOlder,
    setIsLoadingNewer,
    generationRef,
    fetchPage,
    appendPage,
    setError,
  } = timelineEntries
  const loadPage = useCallback(
    async (direction: TimelineDirection) => {
      const directionState = timelineDirectionState(direction, {
        entriesReference: entriesRef,
        loadingOlderReference: loadingOlderRef,
        loadingNewerReference: loadingNewerRef,
        setLoadingOlder: setIsLoadingOlder,
        setLoadingNewer: setIsLoadingNewer,
      })
      if (directionState.loadingReference.current) return
      const cursor = directionCursor(direction, directionState.boundary)
      if (!cursor) return

      const generation = generationRef.current
      directionState.loadingReference.current = true
      directionState.setLoading(true)
      const request = timelineRequest(
        { groupBy, search: normalizedSearch, mediaType, classification },
        direction,
        cursor
      )
      try {
        const response = await fetchPage(request)
        if (generation !== generationRef.current) return
        appendPage(response, direction)
      } catch (requestError: unknown) {
        if (generation !== generationRef.current) return
        setError(timelineError(requestError))
      } finally {
        if (generation === generationRef.current) {
          directionState.loadingReference.current = false
          directionState.setLoading(false)
        }
      }
    },
    [
      appendPage,
      classification,
      entriesRef,
      fetchPage,
      generationRef,
      groupBy,
      loadingNewerRef,
      loadingOlderRef,
      mediaType,
      normalizedSearch,
      setError,
      setIsLoadingNewer,
      setIsLoadingOlder,
    ]
  )

  return {
    loadOlder: useCallback(() => loadPage('older'), [loadPage]),
    loadNewer: useCallback(() => loadPage('newer'), [loadPage]),
  }
}

export function useTimelineWindow(options: TimelineWindowOptions) {
  const { groupBy, search, mediaType, classification, marker, refreshKey } = options
  const normalizedSearch = search.trim()
  const contextKey = JSON.stringify([
    groupBy,
    normalizedSearch,
    mediaType,
    classification,
    refreshKey,
  ])
  const timelineEntries = useTimelineEntries(contextKey)
  const {
    generationRef,
    entries,
    isLoading,
    setIsLoading,
    isLoadingOlder,
    isLoadingNewer,
    error,
    setError,
    fetchPage,
    appendPage,
    beginGeneration,
  } = timelineEntries

  useEffect(() => {
    const generation = beginGeneration()

    if (!marker) {
      setIsLoading(false)
      return
    }

    setIsLoading(true)
    const request = timelineRequest(
      { groupBy, search: normalizedSearch, mediaType, classification },
      'older',
      undefined,
      marker.anchorDate
    )
    void loadInitialTimelinePage({
      isCurrent: () => generation === generationRef.current,
      request,
      fetchPage,
      appendPage,
      setError,
      finishLoading: () => setIsLoading(false),
    })
  }, [
    appendPage,
    beginGeneration,
    classification,
    fetchPage,
    generationRef,
    groupBy,
    marker,
    mediaType,
    normalizedSearch,
    setError,
    setIsLoading,
  ])

  const { loadOlder, loadNewer } = useTimelinePageLoader({
    groupBy,
    normalizedSearch,
    mediaType,
    classification,
    timelineEntries,
  })

  const groups = mergeTimelinePages(entries)
  const hasNextPage = entries.at(-1)?.response.hasOlder ?? false
  const hasPreviousPage = entries[0]?.response.hasNewer ?? false

  return {
    groups,
    hasNextPage,
    hasPreviousPage,
    isLoading,
    isFetching: isLoading || isLoadingOlder || isLoadingNewer,
    isLoadingOlder,
    isLoadingNewer,
    error,
    loadOlder,
    loadNewer,
  }
}

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

interface TimelineWindowOptions {
  groupBy: GroupBy
  search: string
  mediaType: MediaTypeFilter | null
  classification: TimelineClassification | null
  marker: TimelineMarker | null
  preloadKey: number
  refreshKey: number
}

interface TimelinePageEntry {
  response: TimelineListResponse
}

type CachedPage = TimelineListResponse | Promise<TimelineListResponse>

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
    media: media.sort((left, right) => (
      (right.dateTaken ?? '').localeCompare(left.dateTaken ?? '') || right.id - left.id
    )),
  })).sort((left, right) => right.date.localeCompare(left.date))
}

export function useTimelineMarkers(mediaType: MediaTypeFilter | null, classification: TimelineClassification | null, search: string) {
  const normalizedSearch = search.trim()

  return useQuery({
    queryKey: ['timeline', 'markers', mediaType, classification, normalizedSearch],
    queryFn: () => mediaApi.getTimelineMarkers(mediaType, classification, normalizedSearch),
    staleTime: Infinity,
    gcTime: 1000 * 60 * 10,
    refetchOnMount: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
}

export function useTimelineWindow(options: TimelineWindowOptions) {
  const {
    groupBy,
    search,
    mediaType,
    classification,
    marker,
    preloadKey,
    refreshKey,
  } = options
  const normalizedSearch = search.trim()
  const contextKey = JSON.stringify([
    groupBy,
    normalizedSearch,
    mediaType,
    classification,
    refreshKey,
  ])
  const pageCacheRef = useRef<Map<string, CachedPage>>(new Map())
  const generationRef = useRef(0)
  const entriesRef = useRef<TimelinePageEntry[]>([])
  const loadingOlderRef = useRef(false)
  const loadingNewerRef = useRef(false)
  const [entries, setEntries] = useState<TimelinePageEntry[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [isLoadingOlder, setIsLoadingOlder] = useState(false)
  const [isLoadingNewer, setIsLoadingNewer] = useState(false)
  const [error, setError] = useState<Error | null>(null)

  const fetchPage = useCallback(async (request: TimelineListRequest) => {
    const cacheKey = JSON.stringify([contextKey, request])
    const cached = pageCacheRef.current.get(cacheKey)
    if (cached) return cached instanceof Promise ? cached : Promise.resolve(cached)

    const pending = mediaApi.listTimeline(request)
      .then((response) => {
        pageCacheRef.current.set(cacheKey, response)
        return response
      })
      .catch((requestError: unknown) => {
        pageCacheRef.current.delete(cacheKey)
        throw requestError
      })
    pageCacheRef.current.set(cacheKey, pending)
    return pending
  }, [contextKey])

  const appendPage = useCallback((response: TimelineListResponse, direction: 'older' | 'newer') => {
    const entry = { response }
    const nextEntries = direction === 'older'
      ? [...entriesRef.current, entry]
      : [entry, ...entriesRef.current]
    entriesRef.current = nextEntries
    setEntries(nextEntries)
  }, [])

  const preloadPeriods = useCallback(async (
    initialResponse: TimelineListResponse,
    direction: 'older' | 'newer',
    generation: number,
  ) => {
    const loadingRef = direction === 'older' ? loadingOlderRef : loadingNewerRef
    const setLoading = direction === 'older' ? setIsLoadingOlder : setIsLoadingNewer
    if (loadingRef.current) return
    loadingRef.current = true
    setLoading(true)
    let response = initialResponse
    try {
      for (let index = 0; index < 10; index += 1) {
        if (generation !== generationRef.current) return
        const cursor = direction === 'older' ? response.nextCursor : response.previousCursor
        const hasMore = direction === 'older' ? response.hasOlder : response.hasNewer
        if (!hasMore || !cursor) return
        const request: TimelineListRequest = {
          groupBy,
          search: normalizedSearch,
          mediaType: mediaType ?? undefined,
          classification,
          direction,
          cursor,
        }
        response = await fetchPage(request)
        if (generation !== generationRef.current) return
        appendPage(response, direction)
      }
    } catch (requestError: unknown) {
      if (generation === generationRef.current) {
        setError(requestError instanceof Error ? requestError : new Error('Timeline preload failed'))
      }
    } finally {
      loadingRef.current = false
      setLoading(false)
    }
  }, [appendPage, classification, fetchPage, groupBy, mediaType, normalizedSearch])

  useEffect(() => {
    pageCacheRef.current.clear()
  }, [contextKey])

  useEffect(() => {
    const generation = generationRef.current + 1
    generationRef.current = generation
    entriesRef.current = []
    setEntries([])
    setError(null)

    if (!marker) {
      setIsLoading(false)
      return
    }

    setIsLoading(true)
    const request: TimelineListRequest = {
      groupBy,
      search: normalizedSearch,
      mediaType: mediaType ?? undefined,
      classification,
      direction: 'older',
      anchorDate: marker.anchorDate,
    }
    void fetchPage(request)
      .then((response) => {
        if (generation !== generationRef.current) return
        appendPage(response, 'older')
        void preloadPeriods(response, 'older', generation)
        if (preloadKey > 0) void preloadPeriods(response, 'newer', generation)
      })
      .catch((requestError: unknown) => {
        if (generation !== generationRef.current) return
        setError(requestError instanceof Error ? requestError : new Error('Timeline request failed'))
      })
      .finally(() => {
        if (generation === generationRef.current) setIsLoading(false)
      })
  }, [appendPage, classification, fetchPage, groupBy, marker, mediaType, normalizedSearch, preloadKey, preloadPeriods])

  const loadOlder = useCallback(async () => {
    if (loadingOlderRef.current) return
    const oldest = entriesRef.current.at(-1)
    if (!oldest?.response.hasOlder || !oldest.response.nextCursor) return

    loadingOlderRef.current = true
    setIsLoadingOlder(true)
    const request: TimelineListRequest = {
      groupBy,
      search: normalizedSearch,
      mediaType: mediaType ?? undefined,
      classification,
      direction: 'older',
      cursor: oldest.response.nextCursor,
    }
    try {
      const response = await fetchPage(request)
      appendPage(response, 'older')
    } catch (requestError: unknown) {
      setError(requestError instanceof Error ? requestError : new Error('Timeline request failed'))
    } finally {
      loadingOlderRef.current = false
      setIsLoadingOlder(false)
    }
  }, [appendPage, classification, fetchPage, groupBy, mediaType, normalizedSearch])

  const loadNewer = useCallback(async () => {
    if (loadingNewerRef.current) return
    const newest = entriesRef.current[0]
    if (!newest?.response.hasNewer || !newest.response.previousCursor) return

    loadingNewerRef.current = true
    setIsLoadingNewer(true)
    const request: TimelineListRequest = {
      groupBy,
      search: normalizedSearch,
      mediaType: mediaType ?? undefined,
      classification,
      direction: 'newer',
      cursor: newest.response.previousCursor,
    }
    try {
      const response = await fetchPage(request)
      appendPage(response, 'newer')
    } catch (requestError: unknown) {
      setError(requestError instanceof Error ? requestError : new Error('Timeline request failed'))
    } finally {
      loadingNewerRef.current = false
      setIsLoadingNewer(false)
    }
  }, [appendPage, classification, fetchPage, groupBy, mediaType, normalizedSearch])

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

package io.github.yzard.momento.feature.timeline

import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TimelineScreenTest {
    @Test
    fun timelinePagesMapToIndependentBackendFilters() {
        assertEquals(null, TimelinePage.TIMELINE.mediaType)
        assertEquals("image", TimelinePage.PHOTOS.mediaType)
        assertEquals("video", TimelinePage.VIDEOS.mediaType)
        assertEquals("image", TimelinePage.SCREENSHOTS.mediaType)
        assertEquals("image", TimelinePage.DOCUMENTS.mediaType)
        assertEquals("screenshot", TimelinePage.SCREENSHOTS.classification)
        assertEquals("document", TimelinePage.DOCUMENTS.classification)
    }

    @Test
    fun timelinePeriodsMapToBackendGrouping() {
        assertEquals(listOf("day", "week", "month", "year"), TimelinePeriod.entries.map { it.groupBy })
    }

    @Test
    fun normalizesTimelineSearchWithoutChangingPageFilters() {
        assertEquals("receipt", normalizedTimelineSearchQuery(" receipt "))
        assertEquals("image", TimelinePage.PHOTOS.mediaType)
        assertEquals("video", TimelinePage.VIDEOS.mediaType)
        assertEquals("screenshot", TimelinePage.SCREENSHOTS.classification)
        assertEquals("document", TimelinePage.DOCUMENTS.classification)
    }

    @Test
    fun createsIndependentScrollKeysForFilters() {
        assertEquals("TIMELINE:DAY:", timelineScrollKey(TimelinePage.TIMELINE, TimelinePeriod.DAY, ""))
        assertEquals("PHOTOS:MONTH:receipt", timelineScrollKey(TimelinePage.PHOTOS, TimelinePeriod.MONTH, "receipt"))
    }

    @Test
    fun prefetchDistanceKeepsAtLeastThreeViewportsReady() {
        assertEquals(24, timelinePrefetchDistance(1))
        assertEquals(30, timelinePrefetchDistance(10))
        assertEquals(0, timelinePrefetchDistance(0))
    }

    @Test
    fun appendsBeforeTheLastThreeViewportsAreConsumed() {
        assertTrue(shouldAppendTimeline(69, 100, 10, hasOlder = true, loading = false))
        assertFalse(shouldAppendTimeline(68, 100, 10, hasOlder = true, loading = false))
        assertFalse(shouldAppendTimeline(69, 100, 10, hasOlder = true, loading = true))
        assertFalse(shouldAppendTimeline(69, 100, 10, hasOlder = false, loading = false))
        assertFalse(shouldAppendTimeline(-1, 0, 0, hasOlder = true, loading = false))
    }

    @Test
    fun prependsBeforeTheFirstThreeViewportsAreConsumed() {
        assertTrue(shouldPrependTimeline(30, 10, hasNewer = true, loading = false))
        assertFalse(shouldPrependTimeline(31, 10, hasNewer = true, loading = false))
        assertFalse(shouldPrependTimeline(30, 10, hasNewer = true, loading = true))
        assertFalse(shouldPrependTimeline(30, 10, hasNewer = false, loading = false))
        assertFalse(shouldPrependTimeline(-1, 0, hasNewer = true, loading = false))
    }

    @Test
    fun mergesCursorPagesWithoutRepeatingMedia() {
        val existing = listOf(TimelineGroup("Today", listOf(media(1))))
        val next = listOf(
            TimelineGroup("Today", listOf(media(1), media(2))),
            TimelineGroup("Yesterday", listOf(media(3))),
        )

        val merged = mergeTimelineGroups(existing, next)

        assertEquals(listOf("Today", "Yesterday"), merged.map { it.date })
        assertEquals(listOf(1L, 2L), merged.first().media.map { it.id })
    }

    @Test
    fun flattensPeriodsIntoOneContinuousMediaSequence() {
        val flattened = flattenTimelineGroups(
            listOf(
                TimelineGroup("Today", listOf(media(1), media(2))),
                TimelineGroup("Yesterday", listOf(media(3))),
            ),
        )

        assertEquals(listOf(1L, 2L, 3L), flattened.map { it.media.id })
        assertEquals(listOf("Today", "Today", "Yesterday"), flattened.map { it.period })
        assertEquals("Today", timelinePeriodAtIndex(flattened, 0))
        assertEquals("Yesterday", timelinePeriodAtIndex(flattened, 2))
        assertEquals("Today", timelinePeriodAtIndex(flattened, 1))
        assertEquals(null, timelinePeriodAtIndex(flattened, 3))
    }

    @Test
    fun removesSelectedMediaAndEmptyPeriods() {
        val groups = listOf(
            TimelineGroup("Today", listOf(media(1), media(2))),
            TimelineGroup("Yesterday", listOf(media(3))),
        )

        val remaining = removeTimelineMedia(groups, setOf(2L, 3L))

        assertEquals(listOf("Today"), remaining.map { it.date })
        assertEquals(listOf(1L), remaining.single().media.map { it.id })
    }

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "image",
        createdAt = "2026-01-01T00:00:00Z",
    )
}

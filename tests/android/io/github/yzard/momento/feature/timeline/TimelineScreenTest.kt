package io.github.yzard.momento.feature.timeline

import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.core.model.TimelineGroup
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TimelineScreenTest {
    @Test
    fun mapsScreenshotsIndependentOfMediaType() {
        assertEquals(null to "screenshot", timelineFilters("Screenshots"))
    }

    @Test
    fun appendsWhenScrollApproachesEnd() {
        assertTrue(shouldAppendTimeline(8, 10, hasOlder = true, appending = false))
        assertFalse(shouldAppendTimeline(8, 10, hasOlder = true, appending = true))
        assertFalse(shouldAppendTimeline(8, 10, hasOlder = false, appending = false))
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

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "image",
        createdAt = "2026-01-01T00:00:00Z",
    )
}

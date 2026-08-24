package io.github.yzard.momento.core.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MomentoRepositoryTest {
    @Test fun createsUnfilteredDailyTimelineRequest() {
        val request = timelineRequest(null, "day", null, null, "2026-01-01T00:00:00Z")
        assertEquals(100, request.limit)
        assertNull(request.cursor)
        assertEquals("day", request.groupBy)
        assertEquals("older", request.direction)
        assertNull(request.mediaType)
        assertNull(request.classification)
    }

    @Test fun preservesTimelineCursorPeriodFiltersAndAnchorAcrossPages() {
        val request = timelineRequest(
            "cursor",
            "month",
            "image",
            "screenshot",
            "2026-01-01T00:00:00Z",
        )
        assertEquals("cursor", request.cursor)
        assertEquals("month", request.groupBy)
        assertEquals("image", request.mediaType)
        assertEquals("screenshot", request.classification)
        assertEquals("2026-01-01T00:00:00Z", request.anchorDate)
    }
}

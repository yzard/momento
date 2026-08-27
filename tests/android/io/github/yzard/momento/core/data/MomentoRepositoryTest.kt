package io.github.yzard.momento.core.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MomentoRepositoryTest {
    @Test fun createsUnfilteredDailyTimelineRequest() {
        val request = timelineRequest(null, "day", "", null, null, "older", "2026-01-01T00:00:00Z")
        assertEquals(100, request.limit)
        assertNull(request.cursor)
        assertEquals("day", request.groupBy)
        assertEquals("", request.search)
        assertEquals("older", request.direction)
        assertNull(request.mediaType)
        assertNull(request.classification)
    }

    @Test fun preservesTimelineCursorPeriodFiltersAndAnchorAcrossPages() {
        val request = timelineRequest(
            "cursor",
            "month",
            "receipt",
            "image",
            "screenshot",
            "newer",
            "2026-01-01T00:00:00Z",
        )
        assertEquals("cursor", request.cursor)
        assertEquals("month", request.groupBy)
        assertEquals("receipt", request.search)
        assertEquals("image", request.mediaType)
        assertEquals("screenshot", request.classification)
        assertEquals("newer", request.direction)
        assertEquals("2026-01-01T00:00:00Z", request.anchorDate)
    }

    @Test fun preservesCursorForPagedCollectionRequests() {
        assertNull(pagedListRequest(null).cursor)
        assertEquals("next-page", pagedListRequest("next-page").cursor)
        assertEquals(100, pagedListRequest("next-page").limit)
    }
}

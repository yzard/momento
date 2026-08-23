package io.github.yzard.momento.core.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MomentoRepositoryTest {
    @Test fun mapsAllTimelineFilterWithoutMediaType() {
        val request = timelineRequest("day", null, null, "2026-01-01T00:00:00Z")
        assertEquals(100, request.limit)
        assertEquals("older", request.direction)
        assertNull(request.mediaType)
        assertNull(request.classification)
    }

    @Test fun mapsClassificationIndependentlyFromMediaType() {
        val request = timelineRequest("day", "image", "screenshot", "2026-01-01T00:00:00Z")
        assertEquals("image", request.mediaType)
        assertEquals("screenshot", request.classification)
    }
}

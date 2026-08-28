package io.github.yzard.momento.feature.timeline

import io.github.yzard.momento.core.model.TimelineResponse
import io.github.yzard.momento.feature.media.PageState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TimelinePagingStateTest {
    @Test fun initialFailureIsExplicitAndRetryable() {
        val loading = requireNotNull(TimelinePagingState.initial().begin(true, TimelineDirection.OLDER))

        val failed = loading.fail(TimelineDirection.OLDER, "Offline")

        assertEquals(PageState.Failed("Offline"), failed.page)
        assertEquals(TimelineDirection.OLDER, failed.failedDirection)
    }

    @Test fun refreshFailurePreservesExistingTimeline() {
        val ready = TimelinePagingState.initial().copy(
            page = PageState.Ready(emptyList(), refreshing = false),
        )
        val refreshing = requireNotNull(ready.begin(true, TimelineDirection.OLDER))

        val failed = refreshing.fail(TimelineDirection.OLDER, "Offline")

        assertTrue(failed.page is PageState.Ready)
        assertEquals(false, (failed.page as PageState.Ready).refreshing)
    }

    @Test fun completedPageStoresBothCursorDirections() {
        val response = TimelineResponse(
            groups = emptyList(),
            nextCursor = "older",
            previousCursor = "newer",
            hasOlder = true,
            hasNewer = true,
        )

        val completed = TimelinePagingState.initial().complete(response, true, TimelineDirection.OLDER)

        assertEquals("older", completed.olderCursor)
        assertEquals("newer", completed.newerCursor)
        assertTrue(completed.hasOlder)
        assertTrue(completed.hasNewer)
    }
}

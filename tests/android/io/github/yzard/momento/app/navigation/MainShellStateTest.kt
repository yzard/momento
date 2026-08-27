package io.github.yzard.momento.app.navigation

import io.github.yzard.momento.core.model.Media
import io.github.yzard.momento.feature.timeline.TimelinePeriod
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MainShellStateTest {
    @Test fun navigationAndTimelineChoicesStayInOneStateOwner() {
        val state = MainShellState()

        state.navigate(Destination.ALBUMS)
        state.selectTimelinePeriod(TimelinePeriod.MONTH)
        state.updateTimelineSearchQuery("lake")

        assertEquals(Destination.ALBUMS, state.destination)
        assertEquals(TimelinePeriod.MONTH, state.timelinePeriod)
        assertEquals("lake", state.timelineSearchQuery)
    }

    @Test fun closingChangedViewerRefreshesItsParentOnce() {
        val state = MainShellState()
        val media = listOf(media(1), media(2))

        state.openViewer(media, 1)
        assertEquals(media, state.viewerMedia)
        state.updateViewerIndex(0)
        state.markViewerChanged()
        state.closeViewer()
        state.closeViewer()

        assertNull(state.viewerMedia)
        assertEquals(1, state.contentRevision)
        assertEquals(0, state.viewerIndex)
    }

    @Test(expected = IllegalArgumentException::class)
    fun viewerRejectsAnInvalidStartingIndex() {
        MainShellState().openViewer(listOf(media(1)), 2)
    }

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "photo",
        createdAt = "2026-01-01T00:00:00Z",
    )
}

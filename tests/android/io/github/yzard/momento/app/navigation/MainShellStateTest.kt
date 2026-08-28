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

    @Test fun viewerUsesTheRouteStackAndPublishesExplicitLibraryChanges() {
        val state = MainShellState()
        val media = listOf(media(1), media(2))

        state.openViewer(media, 1)
        assertEquals(MainRoute.Viewer(media, 1), state.viewer)
        state.updateViewerIndex(0)
        state.markMediaChanged(2)
        state.closeViewer()
        state.closeViewer()

        assertNull(state.viewer)
        assertEquals(LibraryChange(1, 2), state.libraryChange)
        assertEquals(listOf(MainRoute.Collection(Destination.TIMELINE)), state.routeStack)
    }

    @Test(expected = IllegalArgumentException::class)
    fun viewerRejectsAnInvalidStartingIndex() {
        MainShellState().openViewer(listOf(media(1)), 2)
    }

    @Test fun detailsAndViewerFormOneOrderedBackStack() {
        val state = MainShellState()
        state.navigate(Destination.ALBUMS)
        state.openAlbum(42)
        state.openViewer(listOf(media(1)), 0)

        assertEquals(MainRoute.Viewer(listOf(media(1)), 0), state.currentRoute)
        state.closeViewer()
        assertEquals(MainRoute.AlbumDetail(42), state.currentRoute)
        state.closeDetail()
        assertEquals(MainRoute.Collection(Destination.ALBUMS), state.currentRoute)
    }

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "photo",
        createdAt = "2026-01-01T00:00:00Z",
    )
}

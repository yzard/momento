package io.github.yzard.momento.app.navigation

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class RoutesTest {
    @Test fun timelineIsTheMainDestination() { assertEquals("Timeline", Destination.TIMELINE.label) }

    @Test fun timelinePagesShareTimelineNavigationBehavior() {
        val timelinePages = listOf(Destination.TIMELINE) + timelineSubpageDestinations

        assertTrue(timelinePages.all { it.isTimelinePage() })
        assertTrue(Destination.ALBUMS.isTimelinePage().not())
        assertTrue(Destination.entries.none { it.name == "SEARCH" })
        assertTrue(Destination.entries.none { it.name == "CREATE" })
    }

    @Test fun drawerMatchesWebNavigationOrder() {
        assertEquals(
            listOf(
                Destination.TIMELINE,
                Destination.PHOTOS,
                Destination.VIDEOS,
                Destination.SCREENSHOTS,
                Destination.DOCUMENTS,
                Destination.ALBUMS,
                Destination.MAP,
                Destination.PLACES,
                Destination.FACES,
                Destination.DEDUPLICATE,
                Destination.TRASH,
            ),
            webDrawerDestinations,
        )
    }

    @Test fun collectionsDestinationsArePresent() { assertTrue(Destination.entries.containsAll(listOf(Destination.ALBUMS, Destination.MAP, Destination.PLACES, Destination.FACES, Destination.DEDUPLICATE, Destination.TRASH))) }

    @Test fun utilitySectionContainsDeduplication() {
        assertEquals(listOf(Destination.DEDUPLICATE), utilityDrawerDestinations)
    }

    @Test fun settingsAndAdminOwnTheirPageTitles() {
        assertTrue(Destination.SETTINGS.hasFloatingTitle().not())
        assertTrue(Destination.ADMIN.hasFloatingTitle().not())
        assertTrue(Destination.PLACES.hasFloatingTitle())
    }
}

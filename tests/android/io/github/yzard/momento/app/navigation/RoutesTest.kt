package io.github.yzard.momento.app.navigation

import io.github.yzard.momento.core.model.BackupCapabilities
import io.github.yzard.momento.core.model.Capabilities
import io.github.yzard.momento.core.model.FeatureFlags
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

    @Test fun serverCapabilitiesHideUnavailableGeneratedCollections() {
        val capabilities = Capabilities(
            appVersion = "1.0.0",
            apiVersion = 1,
            supportedMediaExtensions = emptyList(),
            features = FeatureFlags(
                llm = true,
                imageTagging = true,
                deduplicate = false,
                faceDetection = false,
                imageAesthetics = true,
                screenshotDetection = false,
                documentDetection = true,
            ),
            backup = BackupCapabilities(true, 1, 1, 1, 1),
        )

        assertTrue(Destination.DOCUMENTS.isAvailable(capabilities))
        assertTrue(!Destination.SCREENSHOTS.isAvailable(capabilities))
        assertTrue(!Destination.FACES.isAvailable(capabilities))
        assertTrue(!Destination.DEDUPLICATE.isAvailable(capabilities))
        assertTrue(Destination.ALBUMS.isAvailable(capabilities))
    }

    @Test fun settingsAndAdminOwnTheirPageTitles() {
        assertTrue(Destination.SETTINGS.hasFloatingTitle().not())
        assertTrue(Destination.ADMIN.hasFloatingTitle().not())
        assertTrue(Destination.PLACES.hasFloatingTitle())
    }
}

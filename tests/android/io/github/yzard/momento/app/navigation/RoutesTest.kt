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

    @Test fun adminSectionContainsFourRealSubpagesInDisplayOrder() {
        assertEquals(
            listOf(
                Destination.ADMIN_IMPORT,
                Destination.ADMIN_METADATA,
                Destination.ADMIN_AI,
                Destination.ADMIN_USERS,
            ),
            adminDrawerDestinations,
        )
        assertTrue(adminDrawerDestinations.all { it.isAdminPage() })
        assertTrue(!Destination.SETTINGS.isAdminPage())
        assertEquals(adminDrawerDestinations, adminDrawerDestinationsForRole("admin"))
        assertTrue(adminDrawerDestinationsForRole("user").isEmpty())
        assertTrue(adminDrawerDestinationsForRole(null).isEmpty())
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
            backup = BackupCapabilities(true, 2, 1, 1, 1),
        )

        val available = CapabilityState.Available(capabilities)
        assertTrue(Destination.DOCUMENTS.isAvailable(available))
        assertTrue(!Destination.SCREENSHOTS.isAvailable(available))
        assertTrue(!Destination.FACES.isAvailable(available))
        assertTrue(!Destination.DEDUPLICATE.isAvailable(available))
        assertTrue(Destination.ALBUMS.isAvailable(available))
    }

    @Test fun unknownCapabilitiesDoNotExposeServerGeneratedCollectionsOrBackup() {
        assertTrue(!Destination.SCREENSHOTS.isAvailable(CapabilityState.Loading))
        assertTrue(!Destination.FACES.isAvailable(CapabilityState.Failed("Offline")))
        assertTrue(Destination.ALBUMS.isAvailable(CapabilityState.Loading))
        assertTrue(!CapabilityState.Loading.backupAvailable())
    }

}

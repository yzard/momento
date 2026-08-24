package io.github.yzard.momento.feature.viewer

import io.github.yzard.momento.core.model.Media
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ViewerScreenTest {
    @Test
    fun clampsNavigationAtBothEndsAndHandlesAnEmptyViewer() {
        assertEquals(0, viewerIndex(0, -1, 2))
        assertEquals(1, viewerIndex(1, 1, 2))
        assertEquals(0, viewerIndex(0, 1, 0))
    }

    @Test
    fun removalKeepsTheNearestRemainingMediaSelected() {
        val media = listOf(media(1), media(2), media(3))

        val (remaining, index) = removeViewedMedia(media, 2)

        assertEquals(listOf(1L, 2L), remaining.map { it.id })
        assertEquals(1, index)
    }

    @Test
    fun formatsTheFloatingViewerTimestamp() {
        val timestamp = viewerTimestamp(media(1).copy(dateTaken = "2026-08-23T21:45:00"))

        assertEquals("Aug 23", timestamp.date)
        assertEquals("09:45 PM", timestamp.time)
    }

    @Test
    fun exposesAvailableMetadataWithoutPlaceholderRows() {
        val rows = media(1).copy(width = 1200, height = 800, fileSize = 2048).let(::mediaMetadataRows)

        assertTrue("Dimensions" to "1200 x 800" in rows)
        assertTrue("File size" to "2.0 KiB" in rows)
    }

    @Test
    fun createsAProviderSafeShareFilename() {
        assertEquals("9-my_photo_.jpg", shareCacheFilename(media(9).copy(originalFilename = "my photo?.jpg")))
    }

    @Test
    fun onlyStationarySinglePointerGesturesToggleViewerChrome() {
        assertTrue(shouldToggleViewerChrome(maxPointerCount = 1, movedBeyondTouchSlop = false))
        assertFalse(shouldToggleViewerChrome(maxPointerCount = 1, movedBeyondTouchSlop = true))
        assertFalse(shouldToggleViewerChrome(maxPointerCount = 2, movedBeyondTouchSlop = false))
    }

    @Test
    fun seekPreviewIsClampedBeforeCommittingToThePlayer() {
        assertEquals(0L, boundedPlaybackPosition(-20f, 1_000))
        assertEquals(500L, boundedPlaybackPosition(500f, 1_000))
        assertEquals(1_000L, boundedPlaybackPosition(2_000f, 1_000))
        assertEquals(0L, boundedPlaybackPosition(500f, 0))
    }

    @Test
    fun filmstripChoosesTheThumbnailNearestTheViewportCenter() {
        val centered = centeredFilmstripIndex(
            viewportStartOffset = 0,
            viewportEndOffset = 300,
            items = listOf(
                FilmstripItemBounds(index = 3, offset = 70, size = 40),
                FilmstripItemBounds(index = 4, offset = 130, size = 40),
                FilmstripItemBounds(index = 5, offset = 190, size = 40),
            ),
        )

        assertEquals(4, centered)
        assertEquals(null, centeredFilmstripIndex(0, 300, emptyList()))
    }

    private fun media(id: Long) = Media(
        id = id,
        filename = "$id.jpg",
        originalFilename = "$id.jpg",
        mediaType = "image",
        mimeType = "image/jpeg",
        width = null,
        height = null,
        fileSize = null,
        durationSeconds = null,
        dateTaken = null,
        gpsLatitude = null,
        gpsLongitude = null,
        locationCity = null,
        locationState = null,
        locationCountry = null,
        createdAt = "2026-08-23T21:45:00",
    )
}

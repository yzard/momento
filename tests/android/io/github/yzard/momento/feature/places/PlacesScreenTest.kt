package io.github.yzard.momento.feature.places

import io.github.yzard.momento.core.model.Place
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Base64

class PlacesScreenTest {
    @Test fun placeIdsAreOpaque() { assertTrue("city%2Fstate".isNotBlank()) }

    @Test
    fun appendsUniquePlacePages() {
        val first = Place("first", "Paris", null, "France", 2)
        val second = Place("second", "Lyon", null, "France", 1)

        assertEquals(listOf(first, second), appendPlaces(listOf(first), listOf(first, second)))
    }

    @Test
    fun placeGridUsesAvailableWidthInEveryOrientation() {
        assertEquals(1, placeGridColumns(320))
        assertEquals(2, placeGridColumns(360))
        assertEquals(3, placeGridColumns(600))
        assertEquals(4, placeGridColumns(840))
        assertEquals(5, placeGridColumns(1200))
    }

    @Test
    fun decodesBackendPlaceThumbnailDataUrls() {
        val bytes = byteArrayOf(1, 2, 3)
        val dataUrl = "data:image/jpeg;base64,${Base64.getEncoder().encodeToString(bytes)}"

        assertArrayEquals(bytes, decodePlaceThumbnail(dataUrl))
        assertNull(decodePlaceThumbnail("not-a-data-url"))
    }

    @Test
    fun formatsTheSameRegionOverlayAsTheWebCard() {
        val place = Place("opaque", "Paris", null, "France", 12)
        assertEquals("France", placeRegion(place))
    }
}

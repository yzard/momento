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

    @Test fun appendsUniquePageItems() { assertTrue(appendPlaceMedia(emptyList(), emptyList()).isEmpty()) }

    @Test
    fun portraitAlwaysUsesTwoLargeTiles() {
        assertEquals(2, placeGridColumns(isPortrait = true, widthDp = 320))
        assertEquals(2, placeGridColumns(isPortrait = true, widthDp = 900))
        assertEquals(3, placeGridColumns(isPortrait = false, widthDp = 700))
        assertEquals(4, placeGridColumns(isPortrait = false, widthDp = 900))
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

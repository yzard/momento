package io.github.yzard.momento.feature.places

import io.github.yzard.momento.core.model.Place
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PlacesScreenTest {
    @Test fun placeIdsAreOpaque() { assertTrue("city%2Fstate".isNotBlank()) }

    @Test
    fun placeGridUsesAvailableWidthInEveryOrientation() {
        assertEquals(1, placeGridColumns(320))
        assertEquals(2, placeGridColumns(360))
        assertEquals(3, placeGridColumns(600))
        assertEquals(4, placeGridColumns(840))
        assertEquals(5, placeGridColumns(1200))
    }

    @Test
    fun formatsTheSameRegionOverlayAsTheWebCard() {
        val place = Place("opaque", "Paris", null, "France", 12)
        assertEquals("France", placeRegion(place))
        assertEquals("France · 12 media", placeDetailSubtitle(place))
    }
}

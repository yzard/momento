package io.github.yzard.momento.feature.map

import io.github.yzard.momento.core.model.BoundingBox
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NativeMapScreenTest {
    @Test fun boundingBoxKeepsVisibleMapEdgesInServerOrder() {
        val bounds = BoundingBox(north = 52.0, south = 51.0, east = 5.0, west = 4.0)
        assertEquals(52.0, bounds.north, 0.0)
        assertEquals(4.0, bounds.west, 0.0)
    }
    @Test fun sendsTappedClusterIdAsPrefix() { assertEquals(listOf("u4pr"), clusterPrefixes("u4pr")) }

    @Test fun createsBoundsForAValidViewport() {
        assertEquals(
            BoundingBox(52.0, 51.0, 5.0, 4.0),
            visibleMapBounds(52.0, 51.0, 5.0, 4.0),
        )
    }

    @Test fun rejectsMapViewportBeforeLayout() {
        assertNull(visibleMapBounds(0.0, 0.0, 0.0, 0.0))
        assertNull(visibleMapBounds(Double.NaN, 0.0, 0.0, 0.0))
    }

    @Test fun capturesBoundsAndZoomAtTheTimeOfTheMapEvent() {
        assertEquals(
            MapViewport(BoundingBox(52.0, 51.0, 5.0, 4.0), 12),
            mapViewport(52.0, 51.0, 5.0, 4.0, 12),
        )
        assertNull(mapViewport(0.0, 0.0, 0.0, 0.0, 2))
    }

    @Test fun onlyTheNewestViewportRequestCanUpdateMarkers() {
        val tracker = MapViewportRequestTracker()
        val first = tracker.createRequest(
            MapViewport(BoundingBox(52.0, 51.0, 5.0, 4.0), 8),
        )
        val second = tracker.createRequest(
            MapViewport(BoundingBox(53.0, 52.0, 6.0, 5.0), 9),
        )

        assertFalse(tracker.isCurrent(first))
        assertTrue(tracker.isCurrent(second))
    }

    @Test fun calculatesStableClusterOverlayChanges() {
        assertEquals(
            MapClusterChanges(
                removedIds = setOf("old"),
                addedIds = setOf("new"),
                retainedIds = setOf("stable"),
            ),
            mapClusterChanges(
                currentIds = setOf("old", "stable"),
                incomingIds = setOf("stable", "new"),
            ),
        )
    }

    @Test fun roundsAnimatedZoomAndKeepsItInsideTheServerRange() {
        assertEquals(9, normalizedMapZoom(8.6))
        assertEquals(2, normalizedMapZoom(1.4))
        assertEquals(20, normalizedMapZoom(24.0))
        assertNull(normalizedMapZoom(Double.NaN))
    }

    @Test fun savesAndValidatesTheLastMapPosition() {
        val position = MapPosition(40.7128, -74.006, 12)
        assertEquals(position, parseMapPosition(serializeMapPosition(position)))
        assertNull(parseMapPosition("91.0,0.0,12"))
        assertNull(parseMapPosition("0.0,181.0,12"))
        assertNull(parseMapPosition("0.0,0.0,99"))
        assertNull(parseMapPosition("broken"))
    }

    @Test fun cropsClusterThumbnailsFromTheCenter() {
        assertEquals(CropBounds(50, 0, 150, 100), centerCropBounds(200, 100))
        assertEquals(CropBounds(0, 50, 100, 150), centerCropBounds(100, 200))
        assertEquals(CropBounds(0, 0, 100, 100), centerCropBounds(100, 100))
    }
}

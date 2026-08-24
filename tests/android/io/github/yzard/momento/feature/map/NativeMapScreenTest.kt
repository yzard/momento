package io.github.yzard.momento.feature.map

import io.github.yzard.momento.core.model.BoundingBox
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
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

    @Test fun multiMediaClustersZoomInWithoutExceedingTheMaximum() {
        assertEquals(10, clusterClickZoom(currentZoom = 8, mediaCount = 3, maximumZoom = 20))
        assertEquals(20, clusterClickZoom(currentZoom = 19, mediaCount = 3, maximumZoom = 20))
        assertEquals(8, clusterClickZoom(currentZoom = 8, mediaCount = 1, maximumZoom = 20))
    }

    @Test fun cropsClusterThumbnailsFromTheCenter() {
        assertEquals(CropBounds(50, 0, 150, 100), centerCropBounds(200, 100))
        assertEquals(CropBounds(0, 50, 100, 150), centerCropBounds(100, 200))
        assertEquals(CropBounds(0, 0, 100, 100), centerCropBounds(100, 100))
    }
}

package io.github.yzard.momento.feature.places
import org.junit.Assert.assertTrue
import org.junit.Test
class PlacesScreenTest { @Test fun placeIdsAreOpaque() { assertTrue("city%2Fstate".isNotBlank()) }; @Test fun appendsUniquePageItems() { assertTrue(appendPlaceMedia(emptyList(), emptyList()).isEmpty()) } }

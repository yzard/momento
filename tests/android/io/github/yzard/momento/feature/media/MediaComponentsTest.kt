package io.github.yzard.momento.feature.media

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaComponentsTest {
    @Test fun phoneGridHasThreeColumns() { assertEquals(3, adaptiveGridColumns(360)) }
    @Test fun togglesMediaSelectionInBothDirections() {
        assertEquals(setOf(2L), toggleMediaSelection(emptySet(), 2L))
        assertEquals(emptySet<Long>(), toggleMediaSelection(setOf(2L), 2L))
    }
}

package io.github.yzard.momento.feature.media

import org.junit.Assert.assertEquals
import org.junit.Test

class MediaComponentsTest {
    @Test fun phoneGridHasThreeColumns() { assertEquals(3, adaptiveGridColumns(360)) }
    @Test fun chunksRowsWithoutDroppingTail() { assertEquals(listOf(listOf(1L, 2L, 3L), listOf(4L)), mediaRows(listOf(1, 2, 3, 4), 3)) }
    @Test fun cellWidthAccountsForGridGaps() { assertEquals(99.333336f, mediaCellWidth(300f, 3, 1f)) }
    @Test fun togglesMediaSelectionInBothDirections() {
        assertEquals(setOf(2L), toggleMediaSelection(emptySet(), 2L))
        assertEquals(emptySet<Long>(), toggleMediaSelection(setOf(2L), 2L))
    }
}

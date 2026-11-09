package io.github.yzard.momento.app.designsystem

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class MomentoSelectionDockTest {
    @Test
    fun selectionCountLabelIsConcise() {
        assertEquals("4 selected", momentoSelectionCountLabel(4))
        assertEquals("0 selected", momentoSelectionCountLabel(0))
    }

    @Test
    fun rejectsNegativeSelectionCount() {
        assertThrows(IllegalArgumentException::class.java) { momentoSelectionCountLabel(-1) }
    }
}

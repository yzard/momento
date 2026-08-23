package io.github.yzard.momento.feature.media

import org.junit.Assert.assertEquals
import org.junit.Test

class AdaptiveGridTest {
    @Test fun usesFiveColumnsAtTabletBreakpoint() {
        assertEquals(3, adaptiveGridColumns(599))
        assertEquals(5, adaptiveGridColumns(600))
    }
}

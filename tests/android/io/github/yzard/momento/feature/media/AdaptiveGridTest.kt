package io.github.yzard.momento.feature.media

import org.junit.Assert.assertEquals
import org.junit.Test

class AdaptiveGridTest {
    @Test fun gridDensityTracksAvailableWidth() {
        assertEquals(2, adaptiveGridColumns(359))
        assertEquals(3, adaptiveGridColumns(360))
        assertEquals(4, adaptiveGridColumns(480))
        assertEquals(5, adaptiveGridColumns(600))
        assertEquals(6, adaptiveGridColumns(840))
        assertEquals(8, adaptiveGridColumns(1200))
    }

    @Test fun windowClassesUseMaterialBreakpoints() {
        assertEquals(MomentoWindowClass.COMPACT, momentoWindowClass(599))
        assertEquals(MomentoWindowClass.MEDIUM, momentoWindowClass(600))
        assertEquals(MomentoWindowClass.EXPANDED, momentoWindowClass(840))
    }

    @Test fun contentPaddingGrowsWithWindowClass() {
        assertEquals(12, adaptiveContentPadding(360))
        assertEquals(20, adaptiveContentPadding(720))
        assertEquals(28, adaptiveContentPadding(1024))
    }

    @Test fun paginationStartsBeforeTheLastVisibleCard() {
        assertEquals(true, shouldLoadMoreMedia(7, 10, hasMore = true, loading = false))
        assertEquals(false, shouldLoadMoreMedia(7, 10, hasMore = true, loading = true))
        assertEquals(false, shouldLoadMoreMedia(7, 10, hasMore = false, loading = false))
    }
}

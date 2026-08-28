package io.github.yzard.momento.app.designsystem

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class MomentoPageScaffoldTest {
    @Test
    fun compactPageIncludesInsetsAndBottomControls() {
        assertEquals(
            MomentoPageLayout(horizontalPadding = 12, topPadding = 104, bottomPadding = 116),
            momentoPageLayout(
                widthDp = 390,
                statusBarInsetDp = 24,
                navigationBarInsetDp = 24,
                hasBottomControls = true,
            ),
        )
    }

    @Test
    fun expandedPageUsesExpandedHorizontalPaddingWithoutControlClearance() {
        assertEquals(
            MomentoPageLayout(horizontalPadding = 28, topPadding = 80, bottomPadding = 20),
            momentoPageLayout(
                widthDp = 1_024,
                statusBarInsetDp = 0,
                navigationBarInsetDp = 0,
                hasBottomControls = false,
            ),
        )
    }

    @Test
    fun invalidInsetsAreRejected() {
        assertThrows(IllegalArgumentException::class.java) {
            momentoPageLayout(
                widthDp = 390,
                statusBarInsetDp = -1,
                navigationBarInsetDp = 0,
                hasBottomControls = false,
            )
        }
    }
}

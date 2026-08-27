package io.github.yzard.momento.app.designsystem

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MomentoChromeTest {
    private val colors = FloatingControlColors(
        container = Color(0xF01C1C1E),
        content = Color.White,
        selected = Color.White.copy(alpha = 0.16f),
        outline = Color.White.copy(alpha = 0.12f),
    )

    @Test
    fun idleActionChipUsesTheDockMaterial() {
        assertEquals(colors.container, momentoActionChipContainerColor(colors, pressed = false, enabled = true))
    }

    @Test
    fun pressedActionChipUsesAVisibleMonochromeHighlight() {
        val pressed = momentoActionChipContainerColor(colors, pressed = true, enabled = true)

        assertNotEquals(colors.container, pressed)
        assertTrue(pressed.alpha > colors.container.alpha)
    }

    @Test
    fun disabledActionChipDimsTheDockMaterial() {
        val disabled = momentoActionChipContainerColor(colors, pressed = false, enabled = false)

        assertEquals(colors.container.alpha * 0.55f, disabled.alpha, 0.01f)
    }

    @Test
    fun mediaViewerReservesBlackTopChromeAndBottomControls() {
        assertEquals(104.dp, momentoMediaViewerContentPadding.calculateTopPadding())
        assertEquals(104.dp, momentoMediaViewerContentPadding.calculateBottomPadding())
        assertEquals(0.dp, momentoMediaViewerContentPadding.calculateLeftPadding(LayoutDirection.Ltr))
        assertEquals(0.dp, momentoMediaViewerContentPadding.calculateRightPadding(LayoutDirection.Ltr))
    }
}

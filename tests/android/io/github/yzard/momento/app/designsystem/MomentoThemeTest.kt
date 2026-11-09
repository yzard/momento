package io.github.yzard.momento.app.designsystem

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.compositeOver
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import kotlin.math.pow
import org.junit.Test

class MomentoThemeTest {
    @Test
    fun colorSchemesUseNeutralBlackAndWhiteFoundations() {
        val light = momentoColorScheme(darkTheme = false)
        val dark = momentoColorScheme(darkTheme = true)

        assertEquals(Color.White, light.background)
        assertEquals(Color.Black, light.onBackground)
        assertEquals(Color.Black, light.primary)
        assertEquals(Color.Black, dark.background)
        assertEquals(Color.White, dark.onBackground)
        assertEquals(Color.White, dark.primary)
        assertEquals(Color.Transparent, light.surfaceTint)
        assertEquals(Color.Transparent, dark.surfaceTint)
    }


    @Test
    fun floatingControlsStayReadableAndSeparateFromDarkCanvas() {
        fun luminance(color: Color): Double {
            fun channel(value: Float): Double = if (value <= 0.04045f) value / 12.92 else ((value + 0.055) / 1.055).pow(2.4)
            return 0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
        }
        for (dark in listOf(false, true)) {
            val colors = momentoFloatingControlColors(dark)
            val first = luminance(colors.container)
            val second = luminance(colors.content)
            assertTrue((maxOf(first, second) + 0.05) / (minOf(first, second) + 0.05) >= 4.5)
            if (dark) {
                val boundary = luminance(colors.outline.compositeOver(colors.container))
                assertTrue((boundary + 0.05) / 0.05 >= 1.5)
                assertTrue("Floating controls use the deeper charcoal fill", first < 0.02)
            }
        }
    }

    @Test
    fun floatingControlsUseMonochromeContrastAndSelection() {
        val light = momentoFloatingControlColors(darkTheme = false)
        val dark = momentoFloatingControlColors(darkTheme = true)

        assertEquals(Color.Black, light.content)
        assertEquals(Color.White, dark.content)
        assertEquals(Color.Black, light.selected.copy(alpha = 1f))
        assertEquals(Color.White, dark.selected.copy(alpha = 1f))
        assertEquals(Color.Black, light.outline.copy(alpha = 1f))
        assertEquals(Color.White, dark.outline.copy(alpha = 1f))
        assertEquals(0.98f, light.container.alpha, 0.01f)
        assertEquals(0.97f, dark.container.alpha, 0.01f)
    }
}

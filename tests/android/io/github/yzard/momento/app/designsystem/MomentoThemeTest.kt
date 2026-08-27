package io.github.yzard.momento.app.designsystem

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
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
    fun floatingControlsUseMonochromeContrastAndSelection() {
        val light = momentoFloatingControlColors(darkTheme = false)
        val dark = momentoFloatingControlColors(darkTheme = true)

        assertEquals(Color.Black, light.content)
        assertEquals(Color.White, dark.content)
        assertEquals(Color.Black, light.selected.copy(alpha = 1f))
        assertEquals(Color.White, dark.selected.copy(alpha = 1f))
        assertEquals(Color.Black, light.outline.copy(alpha = 1f))
        assertEquals(Color.White, dark.outline.copy(alpha = 1f))
        assertEquals(0.96f, light.container.alpha, 0.01f)
        assertEquals(0.94f, dark.container.alpha, 0.01f)
    }
}

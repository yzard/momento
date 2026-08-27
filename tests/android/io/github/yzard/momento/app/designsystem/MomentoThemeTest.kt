package io.github.yzard.momento.app.designsystem

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Test

class MomentoThemeTest {
    @Test
    fun floatingControlsUseThemeContrastOverStableMaterials() {
        val light = momentoFloatingControlColors(darkTheme = false)
        val dark = momentoFloatingControlColors(darkTheme = true)

        assertEquals(Color(0xFF202420), light.content)
        assertEquals(Color(0xFFF4F6F1), dark.content)
        assertEquals(0.94f, light.container.alpha, 0.01f)
        assertEquals(0.94f, dark.container.alpha, 0.01f)
    }
}

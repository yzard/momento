package io.github.yzard.momento.app.designsystem

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertEquals
import org.junit.Test

class MomentoThemeTest {
    @Test
    fun floatingControlsUseThemeContrastOverTransparentGrey() {
        val light = momentoFloatingControlColors(darkTheme = false)
        val dark = momentoFloatingControlColors(darkTheme = true)

        assertEquals(Color.Black, light.content)
        assertEquals(Color.White, dark.content)
        assertEquals(0.76f, light.container.alpha, 0.01f)
        assertEquals(0.76f, dark.container.alpha, 0.01f)
    }
}

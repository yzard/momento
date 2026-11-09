package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.SemanticsNodeInteraction
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class SettingsControlRowsInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()

    @Test fun compactBackupActionsStayOnTheTitleRow() = verifyRows(360.dp)
    @Test fun expandedBackupActionsStayOnTheTitleRow() = verifyRows(840.dp)

    private fun verifyRows(width: Dp) {
        var copies = 0
        var shares = 0
        var repairs = 0
        var clears = 0
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                val density = if (width > 400.dp) Density(1f) else LocalDensity.current
                CompositionLocalProvider(LocalDensity provides density) {
                    Column(Modifier.width(width)) {
                        BackupDiagnosticsRow(false, { copies++ }, { shares++ })
                        BackupHistoryRow(true, true, false, false, { repairs++ }, { clears++ })
                    }
                }
            }
        }
        val diagnostics = composeRule.onNodeWithText("Backup diagnostics").fetchSemanticsNode().boundsInRoot
        val copy = composeRule.onNodeWithText("Copy").fetchSemanticsNode().boundsInRoot
        val share = composeRule.onNodeWithContentDescription("Share logs").fetchSemanticsNode().boundsInRoot
        val history = composeRule.onNodeWithText("Backup history").fetchSemanticsNode().boundsInRoot
        val repair = composeRule.onNodeWithText("Reverify").fetchSemanticsNode().boundsInRoot
        val clear = composeRule.onNodeWithText("Clear").fetchSemanticsNode().boundsInRoot
        assertTrue(copy.left >= diagnostics.right)
        assertTrue(share.left >= copy.right)
        assertEquals(diagnostics.center.y, copy.center.y, 2f)
        assertEquals(copy.center.y, share.center.y, 2f)
        assertTrue(repair.left >= history.right)
        assertTrue(clear.left >= repair.right)
        assertEquals(history.center.y, repair.center.y, 2f)
        assertEquals(repair.center.y, clear.center.y, 2f)
        assertHasVisibleInk(composeRule.onNodeWithText("Backup diagnostics"))
        assertHasVisibleInk(composeRule.onNodeWithText("Backup history"))
        assertHasVisibleInk(composeRule.onNodeWithContentDescription("Share logs"))
        composeRule.onNodeWithText("Copy").performClick()
        composeRule.onNodeWithContentDescription("Share logs").performClick()
        composeRule.onNodeWithText("Reverify").performClick()
        composeRule.onNodeWithText("Clear").performClick()
        assertEquals(listOf(1, 1, 1, 1), listOf(copies, shares, repairs, clears))
    }

    private fun assertHasVisibleInk(node: SemanticsNodeInteraction) {
        val pixels = node.captureToImage().toPixelMap()
        var visible = 0
        for (y in 0 until pixels.height) for (x in 0 until pixels.width) {
            if (pixels[x, y].luminance() > 0.5f) visible++
        }
        assertTrue("Dark-theme text and icons must remain visible", visible > 10)
    }

    @Test fun activeWorkDisablesConflictingBackupActions() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                Column {
                    BackupDiagnosticsRow(true, {}, {})
                    BackupHistoryRow(true, false, true, false, {}, {})
                }
            }
        }
        composeRule.onNodeWithText("Copy").assertIsNotEnabled()
        composeRule.onNodeWithContentDescription("Share logs").assertIsNotEnabled()
        composeRule.onNodeWithText("Reverifying…").assertIsNotEnabled()
        composeRule.onNodeWithText("Clear").assertIsNotEnabled()
    }
}

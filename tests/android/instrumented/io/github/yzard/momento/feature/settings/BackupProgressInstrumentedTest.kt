package io.github.yzard.momento.feature.settings

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.width
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.graphics.toPixelMap
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.feature.backup.backupFileStatus
import io.github.yzard.momento.feature.backup.backupUploadFailure
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class BackupProgressInstrumentedTest {
    @get:Rule val rule = createComposeRule()
    @Test fun currentFileAndMediaCountStaySeparateAndFailuresAreRedBelowProgress() {
        val failure = backupUploadFailure("Original.MOV", "Cannot Open")
        rule.setContent { MomentoTheme(ThemePreference.DARK) {
            BackupProgressContent(backupFileStatus("Current.MOV", 12340000, 2000000000), true, 3, 10, listOf(failure))
        } }
        rule.onNodeWithText("Backing up Current.MOV 12.34 MB/2 GB").assertIsDisplayed()
        rule.onNodeWithText("3/10").assertIsDisplayed()
        rule.onNodeWithText(failure).assertIsDisplayed()
        val progress = rule.onNodeWithTag("backupMediaProgress").fetchSemanticsNode().boundsInRoot
        val error = rule.onNodeWithText(failure)
        assertTrue(error.fetchSemanticsNode().boundsInRoot.top >= progress.bottom)
        val pixels = error.captureToImage().toPixelMap()
        assertTrue((0 until pixels.width).any { x -> (0 until pixels.height).any { y ->
            pixels[x, y].red > 0.7f && pixels[x, y].green < 0.4f && pixels[x, y].blue < 0.4f
        } })
    }
    @Test fun progressAndLogsFillTheExpandedWidthInsideTheSettingsGutters() {
        rule.setContent { MomentoTheme(ThemePreference.DARK) {
            CompositionLocalProvider(LocalDensity provides Density(1f)) {
                Box(Modifier.width(840.dp).testTag("backupViewport")) {
                    BackupProgressContent("Backing up Original.MOV 12.34 MB/2 GB", true, 3, 10,
                        listOf("Original.MOV: Upload failed — Cannot Open"))
                }
            }
        } }
        val viewport = rule.onNodeWithTag("backupViewport").fetchSemanticsNode().boundsInRoot
        for (tag in listOf("backupProgressContent", "backupStatus", "backupMediaRow", "backupUploadError")) {
            val bounds = rule.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
            assertTrue(kotlin.math.abs(bounds.left - viewport.left - 16) < 1)
            assertTrue(kotlin.math.abs(bounds.right - viewport.right + 16) < 1)
        }
    }

    @Test fun failedFileRemainsVisibleAfterTheRunStops() {
        rule.setContent { MomentoTheme(ThemePreference.LIGHT) {
            BackupProgressContent("Backup waiting — retry available", false, 3, 10,
                listOf(backupUploadFailure("Original.MOV", "Network unavailable")))
        } }
        rule.onNodeWithText("Original.MOV: Upload failed — Network unavailable").assertIsDisplayed()
        rule.onNodeWithTag("backupMediaProgress").assertDoesNotExist()
    }
}

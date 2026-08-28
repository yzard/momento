package io.github.yzard.momento.app.designsystem

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Text
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.Density
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class MomentoPageScaffoldInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test fun lightCompactPageExposesOneTitleBackActionContentAndBottomControl() {
        var backClicks = 0
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                MomentoPageScaffold(
                    title = "Paris",
                    subtitle = "France · 12 media",
                    backContentDescription = "Back to places",
                    onBack = { backClicks += 1 },
                    trailingContent = null,
                    reserveBottomControls = true,
                    bottomContent = { Text("Selection actions") },
                    modifier = Modifier,
                ) {
                    Box(Modifier.fillMaxSize()) { Text("Media content") }
                }
            }
        }

        composeRule.onNodeWithText("Paris").assertIsDisplayed()
        composeRule.onNodeWithText("France · 12 media").assertIsDisplayed()
        composeRule.onNodeWithText("Media content").assertIsDisplayed()
        composeRule.onNodeWithText("Selection actions").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Back to places").assertHasClickAction().performClick()
        composeRule.runOnIdle { assertEquals(1, backClicks) }
    }

    @Test fun darkLargeFontPageKeepsTitleSubtitleAndContentVisible() {
        composeRule.setContent {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, fontScale = 1.6f)) {
                MomentoTheme(ThemePreference.DARK) {
                    MomentoPageScaffold(
                        title = "Albums",
                        subtitle = "24 memories",
                        backContentDescription = null,
                        onBack = null,
                        trailingContent = null,
                        reserveBottomControls = false,
                        bottomContent = null,
                        modifier = Modifier,
                    ) {
                        Box(Modifier.fillMaxSize()) { Text("Album grid") }
                    }
                }
            }
        }

        composeRule.onNodeWithText("Albums").assertIsDisplayed()
        composeRule.onNodeWithText("24 memories").assertIsDisplayed()
        composeRule.onNodeWithText("Album grid").assertIsDisplayed()
    }

    @Test fun systemThemePageKeepsTheSamePageContract() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.SYSTEM) {
                MomentoPageScaffold(
                    title = "Timeline",
                    subtitle = null,
                    backContentDescription = null,
                    onBack = null,
                    trailingContent = null,
                    reserveBottomControls = false,
                    bottomContent = null,
                    modifier = Modifier,
                ) {
                    Box(Modifier.fillMaxSize()) { Text("Timeline content") }
                }
            }
        }

        composeRule.onNodeWithText("Timeline").assertIsDisplayed()
        composeRule.onNodeWithText("Timeline content").assertIsDisplayed()
    }
}

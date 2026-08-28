package io.github.yzard.momento.feature.media

import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class UiStatesInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test fun errorStateRemainsReadableAndActionable() {
        var retries = 0
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                ErrorState(
                    message = "Could not load memories",
                    retry = { retries += 1 },
                    modifier = Modifier,
                )
            }
        }

        composeRule.onNodeWithText("Could not load memories").assertIsDisplayed()
        composeRule.onNodeWithText("Try again").assertHasClickAction().performClick()
        composeRule.runOnIdle { assertEquals(1, retries) }
    }

    @Test fun emptyStateExplainsWhatHappensNext() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.SYSTEM) {
                EmptyState(
                    title = "No media",
                    explanation = "Imported memories will appear here.",
                    modifier = Modifier,
                )
            }
        }
        composeRule.onNodeWithText("No media").assertIsDisplayed()
        composeRule.onNodeWithText("Imported memories will appear here.").assertIsDisplayed()
    }

    @Test fun loadingStateHasAVisibleDescription() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                LoadingState("Loading timeline", Modifier)
            }
        }
        composeRule.onNodeWithText("Loading timeline").assertIsDisplayed()
    }
}

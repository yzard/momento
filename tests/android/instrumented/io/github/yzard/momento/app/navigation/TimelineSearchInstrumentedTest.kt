package io.github.yzard.momento.app.navigation

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performImeAction
import androidx.compose.ui.test.performTextClearance
import androidx.compose.ui.test.performTextInput
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.feature.timeline.TimelinePeriod
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class TimelineSearchInstrumentedTest {
    @get:Rule val composeRule = createComposeRule()
    private var query by mutableStateOf("")
    private var shown by mutableStateOf(true)
    private var period by mutableStateOf(TimelinePeriod.MONTH)

    private fun render() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                if (shown) ShellOverlay(
                    destination = Destination.PHOTOS,
                    timelinePeriod = period,
                    searchQuery = query,
                    selectTimelinePeriod = { period = it },
                    openMenu = {},
                    search = { query = it },
                )
            }
        }
    }

    private fun startSearch() {
        composeRule.onNodeWithContentDescription("Open search").performClick()
        composeRule.onNodeWithContentDescription("Search photos").performTextInput(" lake ")
        composeRule.onNodeWithContentDescription("Search photos").performImeAction()
        composeRule.waitForIdle()
    }

    @Test fun submittedSearchStaysBetweenMenuAndClearUntilClearIsPressed() {
        render()
        startSearch()
        composeRule.runOnIdle { assertEquals("lake", query) }
        composeRule.onNodeWithTag("timeline-period-dock").assertDoesNotExist()
        composeRule.onNodeWithContentDescription("Search photos").assertTextEquals("lake")
        composeRule.onNodeWithContentDescription("Clear search").assertIsDisplayed()
        val bar = composeRule.onNodeWithTag("timeline-search-bar").fetchSemanticsNode().boundsInRoot
        val menu = composeRule.onNodeWithContentDescription("Open navigation menu").fetchSemanticsNode().boundsInRoot
        val clear = composeRule.onNodeWithContentDescription("Clear search").fetchSemanticsNode().boundsInRoot
        assertTrue(bar.left > menu.right)
        assertTrue(bar.right < clear.left)
        assertTrue(kotlin.math.abs(bar.center.x - (menu.center.x + clear.center.x) / 2) < 2f)
        composeRule.onNodeWithContentDescription("Clear search").performClick()
        composeRule.runOnIdle { assertEquals("", query); assertEquals(TimelinePeriod.MONTH, period) }
        composeRule.onNodeWithTag("timeline-search-bar").assertDoesNotExist()
        composeRule.onNodeWithTag("timeline-period-dock").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Open search").assertIsDisplayed()
    }

    @Test fun emptyTextOnlyExitsAfterImeSubmission() {
        render()
        startSearch()
        composeRule.onNodeWithContentDescription("Search photos").performClick().performTextClearance()
        composeRule.runOnIdle { assertEquals("lake", query) }
        composeRule.onNodeWithTag("timeline-period-dock").assertDoesNotExist()
        composeRule.onNodeWithContentDescription("Clear search").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Search photos").performTextInput("   ")
        composeRule.onNodeWithContentDescription("Search photos").performImeAction()
        composeRule.runOnIdle { assertEquals("", query) }
        composeRule.onNodeWithTag("timeline-period-dock").assertIsDisplayed()
    }

    @Test fun dismissingKeyboardAndRecreatingOverlayPreserveActiveSearch() {
        query = "lake"
        render()
        composeRule.onNodeWithContentDescription("Search photos").performClick().performTextClearance()
        composeRule.onNodeWithContentDescription("Search photos").performTextInput("draft")
        composeRule.onNodeWithTag("dismiss-search-keyboard").performClick()
        composeRule.runOnIdle { assertEquals("lake", query) }
        composeRule.onNodeWithContentDescription("Search photos").assertTextEquals("lake")
        composeRule.onNodeWithTag("timeline-period-dock").assertDoesNotExist()
        composeRule.runOnIdle { shown = false }
        composeRule.runOnIdle { shown = true }
        composeRule.onNodeWithContentDescription("Clear search").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Search photos").assertTextEquals("lake")
    }
}

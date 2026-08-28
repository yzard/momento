package io.github.yzard.momento.app.designsystem

import androidx.compose.foundation.layout.Column
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class MomentoChromeInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun darkPageHeaderAndActionChipExposeClearRoles() {
        var clicks = 0
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                Column {
                    MomentoPageHeader(
                        title = "Albums",
                        subtitle = null,
                        modifier = Modifier,
                        leadingContent = null,
                        trailingContent = null,
                    )
                    MomentoActionChip(
                        label = "Create album",
                        icon = Icons.Default.Add,
                        enabled = true,
                        onClick = { clicks += 1 },
                        modifier = Modifier,
                    )
                }
            }
        }

        composeRule.onNodeWithText("Albums").assertIsDisplayed()
        composeRule.onNodeWithText("Create album").assertHasClickAction().performClick()
        composeRule.runOnIdle { assertEquals(1, clicks) }
    }

    @Test
    fun detailHeaderExposesItsTitleAndBackActionWithoutAnotherContainer() {
        var backClicks = 0
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                MomentoDetailPageHeader(
                    title = "Paris",
                    subtitle = "France · 12 media",
                    backContentDescription = "Back to places",
                    enabled = true,
                    onBack = { backClicks += 1 },
                    modifier = Modifier,
                )
            }
        }

        composeRule.onNodeWithText("Paris").assertIsDisplayed()
        composeRule.onNodeWithText("France · 12 media").assertIsDisplayed()
        composeRule.onNodeWithContentDescription("Back to places").assertHasClickAction().performClick()
        composeRule.runOnIdle { assertEquals(1, backClicks) }
    }

}

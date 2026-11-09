package io.github.yzard.momento.feature.settings

import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.performClick
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import org.junit.Assert.assertTrue
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test

class SettingsTrailingActionsInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun backupRowKeepsItsStatusWithoutExplanationsOrScheduleText() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                BackupSettingsSection(
                    settings = Settings(null, false, true, ThemePreference.DARK),
                    settingsStore = SettingsStore(context),
                    backupAvailable = true,
                )
            }
        }
        composeRule.onNodeWithText("No media backed up yet").assertIsDisplayed()
        composeRule.onNodeWithText("Back up now").assertIsDisplayed()
        composeRule.onAllNodesWithText("Cancel").assertCountEquals(0)
        composeRule.onAllNodesWithText("Metadata and AI processing", substring = true).assertCountEquals(0)
        composeRule.onAllNodesWithText("Daily backup is", substring = true).assertCountEquals(0)
        composeRule.onAllNodesWithText("Next daily backup:", substring = true).assertCountEquals(0)
    }

    @Test
    fun actionsStayToTheRightOfWrappedSettingDescriptions() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                ListItem(
                    headlineContent = { Text("Back up this device") },
                    supportingContent = {
                        Text(
                            "A deliberately long description that wraps without moving actions below it.",
                            Modifier.testTag("description"),
                        )
                    },
                    trailingContent = {
                        BackupRunButton(running = false, enabled = true, start = {}, cancel = {})
                    },
                )
            }
        }

        val description = composeRule.onNodeWithTag("description", useUnmergedTree = true)
            .fetchSemanticsNode().boundsInRoot
        val action = composeRule.onNodeWithText("Back up now")
            .fetchSemanticsNode().boundsInRoot

        assertTrue(action.left >= description.right)
    }

    @Test fun oneButtonStartsAnIdleRunAndCancelsOnlyTheRunningRun() {
        var started = 0
        var cancelled = 0
        composeRule.setContent {
            var running by remember { mutableStateOf(false) }
            MomentoTheme(ThemePreference.DARK) {
                BackupRunButton(
                    running = running,
                    enabled = true,
                    start = { started++; running = true },
                    cancel = { cancelled++; running = false },
                )
            }
        }
        composeRule.onAllNodesWithText("Cancel").assertCountEquals(0)
        composeRule.onNodeWithText("Back up now").performClick()
        composeRule.onAllNodesWithText("Back up now").assertCountEquals(0)
        composeRule.onNodeWithText("Cancel").performClick()
        composeRule.onAllNodesWithText("Cancel").assertCountEquals(0)
        composeRule.onNodeWithText("Back up now").assertIsDisplayed()
        assertEquals(1, started)
        assertEquals(1, cancelled)
    }
}

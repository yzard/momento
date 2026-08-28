package io.github.yzard.momento.feature.settings

import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class SettingsTrailingActionsInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

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
                        SettingsTrailingActions {
                            TextButton(onClick = {}, modifier = Modifier.testTag("first-action")) {
                                Text("Cancel")
                            }
                            TextButton(onClick = {}, modifier = Modifier.testTag("second-action")) {
                                Text("Back up now")
                            }
                        }
                    },
                )
            }
        }

        val description = composeRule.onNodeWithTag("description", useUnmergedTree = true)
            .fetchSemanticsNode().boundsInRoot
        val firstAction = composeRule.onNodeWithTag("first-action", useUnmergedTree = true)
            .fetchSemanticsNode().boundsInRoot
        val secondAction = composeRule.onNodeWithTag("second-action", useUnmergedTree = true)
            .fetchSemanticsNode().boundsInRoot

        assertTrue(firstAction.left >= description.right)
        assertTrue(secondAction.left >= description.right)
    }
}

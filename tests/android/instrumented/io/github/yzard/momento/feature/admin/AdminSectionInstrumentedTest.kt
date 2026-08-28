package io.github.yzard.momento.feature.admin

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.material3.Text
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Rule
import org.junit.Test

class AdminSectionInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun phoneTabsExposeEverySectionAndChangeSelection() {
        composeRule.setContent {
            var selectedSection by remember { mutableStateOf(AdminSection.USERS) }
            MomentoTheme(ThemePreference.LIGHT) {
                AdminSectionTabs(
                    selectedSection = selectedSection,
                    selectSection = { selectedSection = it },
                )
            }
        }

        composeRule.onNodeWithText("Users").assertIsSelected()
        composeRule.onNodeWithText("Import").assertIsDisplayed()
        composeRule.onNodeWithText("Metadata").assertIsDisplayed()
        composeRule.onNodeWithText("AI Processing").performClick().assertIsSelected()
    }

    @Test
    fun portraitTabLayoutKeepsEverySelectedSectionVisible() {
        composeRule.setContent {
            var selectedSection by remember { mutableStateOf(AdminSection.USERS) }
            MomentoTheme(ThemePreference.LIGHT) {
                AdminSectionLayout(
                    selectedSection = selectedSection,
                    selectSection = { selectedSection = it },
                    useNavigationRail = false,
                ) {
                    Text("${selectedSection.label} content")
                }
            }
        }

        AdminSection.entries.forEach { section ->
            composeRule.onNodeWithText(section.label).performClick().assertIsSelected()
            composeRule.onNodeWithText("${section.label} content").assertIsDisplayed()
        }
    }

    @Test
    fun landscapeRailLayoutKeepsEverySelectedSectionVisible() {
        composeRule.setContent {
            var selectedSection by remember { mutableStateOf(AdminSection.USERS) }
            MomentoTheme(ThemePreference.DARK) {
                AdminSectionLayout(
                    selectedSection = selectedSection,
                    selectSection = { selectedSection = it },
                    useNavigationRail = true,
                ) {
                    Text("${selectedSection.label} content")
                }
            }
        }

        AdminSection.entries.forEach { section ->
            composeRule.onNodeWithText(section.label).performClick().assertIsSelected()
            composeRule.onNodeWithText("${section.label} content").assertIsDisplayed()
        }
    }

    @Test
    fun expandedLandscapeAiControlsExposeTheWebTableColumns() {
        composeRule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                AiControlTable(
                    status = null,
                    busyControls = emptySet(),
                    primary = { _, _, _ -> },
                    clean = { _, _ -> },
                    save = { _, _, _ -> },
                )
            }
        }

        listOf("Feature", "Minute", "Hour", "Day", "Month", "Weekday", "Save", "Start / Cancel", "Clean")
            .forEach { label -> composeRule.onAllNodesWithText(label).assertCountEquals(1) }
        composeRule.onAllNodesWithText("OCR").assertCountEquals(1)
        composeRule.onAllNodesWithText("Face detection").assertCountEquals(1)
    }
}

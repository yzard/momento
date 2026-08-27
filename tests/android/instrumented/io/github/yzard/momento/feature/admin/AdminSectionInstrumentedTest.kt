package io.github.yzard.momento.feature.admin

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.junit4.createComposeRule
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
}

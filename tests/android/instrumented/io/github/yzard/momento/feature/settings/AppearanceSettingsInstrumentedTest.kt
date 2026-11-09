package io.github.yzard.momento.feature.settings

import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.platform.app.InstrumentationRegistry
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class AppearanceSettingsInstrumentedTest {
    @get:Rule val rule = createComposeRule()

    @Test fun dropdownShowsAndPersistsTheCurrentTheme() {
        val store = SettingsStore(InstrumentationRegistry.getInstrumentation().targetContext)
        val previous = runBlocking { store.settings.first().themePreference }
        try {
            runBlocking { store.setThemePreference(ThemePreference.SYSTEM) }
            rule.setContent {
                val settings by store.settings.collectAsState(Settings(null, false, true, ThemePreference.SYSTEM))
                MomentoTheme(settings.themePreference) { AppearanceSettingsSection(settings.themePreference, store) }
            }
            val title = rule.onNodeWithText("Appearance").fetchSemanticsNode().boundsInRoot
            val choice = rule.onNodeWithText("Follow system").fetchSemanticsNode().boundsInRoot
            assertTrue(choice.left > title.right)
            rule.onNodeWithText("Follow system").performClick()
            rule.onNodeWithText("Dark").performClick()
            rule.waitUntil { runBlocking { store.settings.first().themePreference == ThemePreference.DARK } }
            rule.onNodeWithText("Dark").assertIsDisplayed().performClick()
            rule.onNodeWithText("Follow system").performClick()
            rule.waitUntil { runBlocking { store.settings.first().themePreference == ThemePreference.SYSTEM } }
            rule.onNodeWithText("Follow system").assertIsDisplayed()
        } finally { runBlocking { store.setThemePreference(previous) } }
    }
}

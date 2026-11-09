package io.github.yzard.momento.feature.settings

import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.AccountRepository
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.model.MessageResponse
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test

class AccountSettingsInstrumentedTest {
    @get:Rule val rule = createComposeRule()
    private val repository = object : AccountRepository {
        override suspend fun changePassword(current: String, updated: String): MessageResponse =
            error("Layout verification must not change credentials")
    }

    @Test fun serverVersionSitsBelowTheOriginAndAccountActionsStackOnTheRight() {
        rule.setContent {
            MomentoTheme(ThemePreference.DARK) {
                AccountSettingsSection(repository, null, "https://fixture.invalid", "fixture-1", {})
            }
        }
        val origin = rule.onNodeWithText("https://fixture.invalid", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val version = rule.onNodeWithText("Server version fixture-1", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val password = rule.onNodeWithText("Change password", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val signOut = rule.onNodeWithText("Sign out", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertTrue("Server version $version must be below origin $origin", version.top >= origin.bottom)
        assertTrue(password.left >= origin.right)
        assertTrue(signOut.left >= version.right)
        assertTrue(signOut.top >= password.bottom)
        assertEquals(password.right, signOut.right, 2f)
    }

    @Test fun unavailableCapabilitiesDoNotInventAServerVersion() {
        rule.setContent {
            MomentoTheme(ThemePreference.LIGHT) {
                AccountSettingsSection(repository, null, null, null, {})
            }
        }
        rule.onNodeWithText("No server selected").assertIsDisplayed()
        rule.onAllNodesWithText("Server version", substring = true).assertCountEquals(0)
    }
}

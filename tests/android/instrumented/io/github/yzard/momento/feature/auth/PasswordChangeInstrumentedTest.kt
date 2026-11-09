package io.github.yzard.momento.feature.auth

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performScrollTo
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import io.github.yzard.momento.app.designsystem.MomentoTheme
import io.github.yzard.momento.core.data.ThemePreference
import org.junit.Rule
import org.junit.Test

class PasswordChangeInstrumentedTest {
    @get:Rule
    val composeRule = createComposeRule()

    @Test
    fun passwordChangeFieldsAcceptInputAndExposeValidation() {
        composeRule.setContent {
            var currentPassword by remember { mutableStateOf("") }
            var newPassword by remember { mutableStateOf("") }
            var confirmation by remember { mutableStateOf("") }
            MomentoTheme(ThemePreference.LIGHT) {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    PasswordChangeFields(
                        currentPassword = currentPassword,
                        newPassword = newPassword,
                        confirmation = confirmation,
                        changeCurrentPassword = { currentPassword = it },
                        changeNewPassword = { newPassword = it },
                        changeConfirmation = { confirmation = it },
                        enabled = true,
                        errorMessage = "Password needs attention",
                        modifier = Modifier,
                    )
                }
            }
        }

        composeRule.onNodeWithText("Current password").performTextInput("admin")
        androidx.test.espresso.Espresso.closeSoftKeyboard()
        composeRule.onNodeWithText("New password").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Confirm new password").performScrollTo().assertIsDisplayed()
        composeRule.onNodeWithText("Password needs attention").performScrollTo().assertIsDisplayed()
    }
}

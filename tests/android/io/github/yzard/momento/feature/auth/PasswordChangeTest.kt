package io.github.yzard.momento.feature.auth

import androidx.compose.ui.autofill.AutofillType
import androidx.compose.ui.ExperimentalComposeUiApi
import io.github.yzard.momento.core.model.User
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PasswordChangeTest {
    @Test
    fun requiresPasswordChangeBeforeCompletingTheSession() {
        assertEquals(LoginRequirement.CHANGE_PASSWORD, loginRequirement(user(mustChangePassword = true)))
        assertEquals(LoginRequirement.COMPLETE_SESSION, loginRequirement(user(mustChangePassword = false)))
    }

    @Test
    fun validatesPasswordLengthAndConfirmation() {
        assertEquals("New password must be at least 8 characters", validateNewPassword("short", "short"))
        assertEquals("New passwords do not match", validateNewPassword("longenough", "different"))
        assertNull(validateNewPassword("longenough", "longenough"))
    }

    @Suppress("DEPRECATION")
    @OptIn(ExperimentalComposeUiApi::class)
    @Test
    fun passwordFieldsExposeExistingAndNewCredentialRoles() {
        assertEquals(listOf(AutofillType.Password), passwordAutofillTypes(PasswordAutofillRole.EXISTING))
        assertEquals(listOf(AutofillType.NewPassword), passwordAutofillTypes(PasswordAutofillRole.NEW))
    }

    private fun user(mustChangePassword: Boolean): User = User(
        id = 1,
        username = "admin",
        email = "admin@example.com",
        role = "admin",
        mustChangePassword = mustChangePassword,
        isActive = true,
        createdAt = "2026-01-01T00:00:00Z",
    )
}

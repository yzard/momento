package io.github.yzard.momento.feature.auth

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardOptions
import io.github.yzard.momento.core.model.User

enum class LoginRequirement {
    COMPLETE_SESSION,
    CHANGE_PASSWORD,
}

fun loginRequirement(user: User): LoginRequirement =
    if (user.mustChangePassword) LoginRequirement.CHANGE_PASSWORD else LoginRequirement.COMPLETE_SESSION

fun validateNewPassword(newPassword: String, confirmation: String): String? = when {
    newPassword.length < 8 -> "New password must be at least 8 characters"
    newPassword != confirmation -> "New passwords do not match"
    else -> null
}

@Composable
fun PasswordChangeFields(
    currentPassword: String,
    newPassword: String,
    confirmation: String,
    changeCurrentPassword: (String) -> Unit,
    changeNewPassword: (String) -> Unit,
    changeConfirmation: (String) -> Unit,
    enabled: Boolean,
    errorMessage: String?,
    modifier: Modifier,
) {
    Column(modifier) {
        OutlinedTextField(
            value = currentPassword,
            onValueChange = changeCurrentPassword,
            label = { Text("Current password") },
            enabled = enabled,
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Next,
            ),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = newPassword,
            onValueChange = changeNewPassword,
            label = { Text("New password") },
            enabled = enabled,
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Next,
            ),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = confirmation,
            onValueChange = changeConfirmation,
            label = { Text("Confirm new password") },
            enabled = enabled,
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Password,
                imeAction = ImeAction.Done,
            ),
            modifier = Modifier.fillMaxWidth(),
        )
        errorMessage?.let { message -> Text(message) }
    }
}

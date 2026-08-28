package io.github.yzard.momento.feature.settings

import android.view.autofill.AutofillManager
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.AdminPanelSettings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import io.github.yzard.momento.core.data.AccountRepository
import io.github.yzard.momento.core.model.User
import io.github.yzard.momento.feature.auth.PasswordChangeFields
import io.github.yzard.momento.feature.auth.validateNewPassword
import kotlinx.coroutines.launch
import retrofit2.HttpException
import java.io.IOException

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AccountSettingsSection(
    repository: AccountRepository,
    user: User?,
    origin: String?,
    openAdmin: () -> Unit,
    logout: () -> Unit,
) {
    var passwordDialog by remember { mutableStateOf(false) }
    var logoutDialog by remember { mutableStateOf(false) }

    Column {
        ListItem(
            headlineContent = { Text(user?.username ?: "Account") },
            supportingContent = {
                Column {
                    Text(origin ?: "No server selected")
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        TextButton(onClick = { passwordDialog = true }) { Text("Change password") }
                        TextButton(onClick = { logoutDialog = true }) {
                            Icon(Icons.AutoMirrored.Filled.Logout, null)
                            Text("Log out")
                        }
                    }
                }
            },
            leadingContent = { Icon(Icons.Default.AccountCircle, null) },
        )
        if (user?.role == "admin") {
            ListItem(
                headlineContent = { Text("Admin") },
                leadingContent = { Icon(Icons.Default.AdminPanelSettings, null) },
                modifier = Modifier.clickable(onClick = openAdmin),
            )
        }
    }

    if (passwordDialog) PasswordDialog(repository) { passwordDialog = false }
    if (logoutDialog) {
        AlertDialog(
            onDismissRequest = { logoutDialog = false },
            title = { Text("Log out?") },
            text = { Text("Are you sure you want to log out?") },
            confirmButton = {
                TextButton(
                    onClick = {
                        logoutDialog = false
                        logout()
                    },
                ) { Text("Log out") }
            },
            dismissButton = { TextButton(onClick = { logoutDialog = false }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun PasswordDialog(repository: AccountRepository, dismiss: () -> Unit) {
    var currentPassword by remember { mutableStateOf("") }
    var newPassword by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var submitting by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val context = LocalContext.current

    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Change password") },
        text = {
            PasswordChangeFields(
                currentPassword = currentPassword,
                newPassword = newPassword,
                confirmation = confirmation,
                changeCurrentPassword = { currentPassword = it },
                changeNewPassword = { newPassword = it },
                changeConfirmation = { confirmation = it },
                enabled = !submitting,
                errorMessage = error,
                modifier = Modifier,
            )
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (submitting) return@TextButton
                    val validation = validateNewPassword(newPassword, confirmation)
                    if (validation != null) {
                        error = validation
                        return@TextButton
                    }
                    scope.launch {
                        submitting = true
                        try {
                            repository.changePassword(currentPassword, newPassword)
                            context.getSystemService(AutofillManager::class.java)?.commit()
                            dismiss()
                        } catch (_: HttpException) {
                            error = "Could not change password"
                        } catch (_: IOException) {
                            error = "Could not reach the server"
                        } finally {
                            submitting = false
                        }
                    }
                },
                enabled = !submitting,
            ) { Text(if (submitting) "Saving" else "Save") }
        },
        dismissButton = { TextButton(dismiss, enabled = !submitting) { Text("Cancel") } },
    )
}

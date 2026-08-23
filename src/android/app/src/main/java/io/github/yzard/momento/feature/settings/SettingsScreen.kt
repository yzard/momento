package io.github.yzard.momento.feature.settings

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.AdminPanelSettings
import androidx.compose.material.icons.filled.Backup
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Password
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.PasswordVisualTransformation
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.database.BackupDatabase
import io.github.yzard.momento.core.database.BackupQueueCount
import io.github.yzard.momento.core.model.User
import io.github.yzard.momento.feature.backup.backupReadPermissions
import io.github.yzard.momento.feature.backup.scheduleBackup
import kotlinx.coroutines.launch
import retrofit2.HttpException
import java.io.IOException

fun backupSummary(counts: List<BackupQueueCount>): String {
    if (counts.isEmpty()) return "No device media has been queued"
    return counts.sortedBy { it.state.ordinal }.joinToString(" · ") { count ->
        "${count.count} ${count.state.name.lowercase().replace('_', ' ')}"
    }
}

fun validateNewPassword(newPassword: String, confirmation: String): String? = when {
    newPassword.length < 8 -> "New password must be at least 8 characters"
    newPassword != confirmation -> "New passwords do not match"
    else -> null
}

fun themePreferenceLabel(themePreference: ThemePreference): String = when (themePreference) {
    ThemePreference.SYSTEM -> "Follow system"
    ThemePreference.LIGHT -> "Light"
    ThemePreference.DARK -> "Dark"
}

@Composable
fun SettingsScreen(
    repository: MomentoRepository,
    settingsStore: SettingsStore,
    user: User?,
    openAdmin: () -> Unit,
    logout: () -> Unit,
) {
    val context = LocalContext.current
    val settings by settingsStore.settings.collectAsState(initial = Settings(null, false, true, ThemePreference.SYSTEM))
    val database = remember { BackupDatabase.create(context.applicationContext) }
    val queueCounts by database.backupAssetDao().observeCounts().collectAsState(initial = emptyList())
    var passwordDialog by remember { mutableStateOf(false) }
    var themeDialog by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val permissionRequest = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { permissions ->
        if (permissions.values.all { it }) scheduleBackup(context, settings.mobileDataEnabled, true)
    }

    DisposableEffect(database) {
        onDispose { database.close() }
    }
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            ListItem(
                headlineContent = { Text(user?.username ?: "Account") },
                supportingContent = { Text(settings.origin ?: "No server selected") },
                leadingContent = { Icon(Icons.Default.AccountCircle, null) },
            )
            HorizontalDivider()
            ListItem(
                headlineContent = { Text("Back up this device") },
                supportingContent = { Text(backupSummary(queueCounts)) },
                leadingContent = { Icon(Icons.Default.Backup, null) },
                modifier = Modifier.clickable { permissionRequest.launch(backupReadPermissions()) },
            )
            SettingsSwitch("Camera folder only", settings.cameraOnly) { enabled ->
                scope.launch {
                    settingsStore.setCameraOnly(enabled)
                    scheduleBackup(context, settings.mobileDataEnabled, false)
                }
            }
            SettingsSwitch("Use mobile data", settings.mobileDataEnabled) { enabled ->
                scope.launch {
                    settingsStore.setMobileDataEnabled(enabled)
                    scheduleBackup(context, enabled, false)
                }
            }
            HorizontalDivider()
            ListItem(
                headlineContent = { Text("Appearance") },
                supportingContent = { Text(themePreferenceLabel(settings.themePreference)) },
                leadingContent = { Icon(Icons.Default.Palette, null) },
                modifier = Modifier.clickable { themeDialog = true },
            )
            ListItem(
                headlineContent = { Text("Change password") },
                leadingContent = { Icon(Icons.Default.Password, null) },
                modifier = Modifier.clickable { passwordDialog = true },
            )
            if (user?.role == "admin") {
                ListItem(
                    headlineContent = { Text("Admin") },
                    leadingContent = { Icon(Icons.Default.AdminPanelSettings, null) },
                    modifier = Modifier.clickable { openAdmin() },
                )
            }
            ListItem(
                headlineContent = { Text("Log out") },
                leadingContent = { Icon(Icons.AutoMirrored.Filled.Logout, null) },
                modifier = Modifier.clickable { logout() },
            )
        }
    }
    if (passwordDialog) PasswordDialog(repository) { passwordDialog = false }
    if (themeDialog) {
        ThemePreferenceDialog(
            selected = settings.themePreference,
            select = { themePreference ->
                scope.launch { settingsStore.setThemePreference(themePreference) }
                themeDialog = false
            },
            dismiss = { themeDialog = false },
        )
    }
}

@Composable
private fun SettingsSwitch(label: String, checked: Boolean, set: (Boolean) -> Unit) {
    ListItem(headlineContent = { Text(label) }, trailingContent = { Switch(checked, set) })
}

@Composable
private fun ThemePreferenceDialog(
    selected: ThemePreference,
    select: (ThemePreference) -> Unit,
    dismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Appearance") },
        text = {
            Column {
                ThemePreference.entries.forEach { themePreference ->
                    ListItem(
                        headlineContent = { Text(themePreferenceLabel(themePreference)) },
                        leadingContent = {
                            RadioButton(
                                selected = themePreference == selected,
                                onClick = { select(themePreference) },
                            )
                        },
                        modifier = Modifier.clickable { select(themePreference) },
                    )
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(dismiss) { Text("Cancel") } },
    )
}

@Composable
private fun PasswordDialog(repository: MomentoRepository, dismiss: () -> Unit) {
    var currentPassword by remember { mutableStateOf("") }
    var newPassword by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()

    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Change password") },
        text = {
            Column {
                OutlinedTextField(currentPassword, { currentPassword = it }, label = { Text("Current password") }, visualTransformation = PasswordVisualTransformation())
                OutlinedTextField(newPassword, { newPassword = it }, label = { Text("New password") }, visualTransformation = PasswordVisualTransformation())
                OutlinedTextField(confirmation, { confirmation = it }, label = { Text("Confirm new password") }, visualTransformation = PasswordVisualTransformation())
                error?.let { Text(it) }
            }
        },
        confirmButton = {
            TextButton({
                val validation = validateNewPassword(newPassword, confirmation)
                if (validation != null) {
                    error = validation
                    return@TextButton
                }
                scope.launch {
                    try {
                        repository.changePassword(currentPassword, newPassword)
                        dismiss()
                    } catch (_: HttpException) {
                        error = "Could not change password"
                    } catch (_: IOException) {
                        error = "Could not reach the server"
                    }
                }
            }) { Text("Save") }
        },
        dismissButton = { TextButton(dismiss) { Text("Cancel") } },
    )
}

package io.github.yzard.momento.feature.settings

import android.content.ActivityNotFoundException
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.os.Build
import android.provider.Settings as AndroidSettings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AccountCircle
import androidx.compose.material.icons.filled.AdminPanelSettings
import androidx.compose.material.icons.filled.Backup
import androidx.compose.material.icons.automirrored.filled.Logout
import androidx.compose.material.icons.filled.Palette
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.material3.MaterialTheme
import androidx.core.content.FileProvider
import androidx.core.net.toUri
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.data.ThemePreference
import io.github.yzard.momento.core.database.BackupDatabase
import io.github.yzard.momento.core.database.BackupQueueCount
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.User
import io.github.yzard.momento.feature.backup.backupReadPermissions
import io.github.yzard.momento.feature.backup.isBackupNetworkAllowed
import io.github.yzard.momento.feature.backup.observeBackupNetworkAllowed
import io.github.yzard.momento.feature.backup.requestBackupCancellation
import io.github.yzard.momento.feature.backup.scheduleBackup
import kotlinx.coroutines.launch
import retrofit2.HttpException
import java.io.IOException
import java.io.File

enum class AndroidUpdateDecision { INVALID_PACKAGE, UP_TO_DATE, UPDATE_AVAILABLE }

data class AndroidUpdateIdentity(val versionCode: Long, val buildTimeMillis: Long)

private const val ANDROID_BUILD_TIME_METADATA_KEY = "io.github.yzard.momento.BUILD_TIME"
private const val ANDROID_BUILD_TIME_PREFIX = "epochMillis:"

fun androidUpdateDecision(
    installedVersionCode: Long,
    installedBuildTimeMillis: Long,
    candidateVersionCode: Long,
    candidateBuildTimeMillis: Long?,
    packageMatches: Boolean,
): AndroidUpdateDecision = when {
    !packageMatches || candidateBuildTimeMillis == null -> AndroidUpdateDecision.INVALID_PACKAGE
    candidateVersionCode < installedVersionCode -> AndroidUpdateDecision.UP_TO_DATE
    candidateVersionCode > installedVersionCode -> AndroidUpdateDecision.UPDATE_AVAILABLE
    candidateBuildTimeMillis > installedBuildTimeMillis -> AndroidUpdateDecision.UPDATE_AVAILABLE
    else -> AndroidUpdateDecision.UP_TO_DATE
}

fun androidBuildTimeMillis(metadataValue: String?): Long? {
    if (metadataValue?.startsWith(ANDROID_BUILD_TIME_PREFIX) != true) return null
    return metadataValue.removePrefix(ANDROID_BUILD_TIME_PREFIX).toLongOrNull()?.takeIf { it > 0 }
}

fun androidUpdateCacheFilename(identity: AndroidUpdateIdentity): String =
    "momento-update-${identity.versionCode}-${identity.buildTimeMillis}.apk"

fun cachedAndroidUpdateIdentity(filename: String): AndroidUpdateIdentity? {
    val match = Regex("^momento-update-([0-9]+)-([0-9]+)\\.apk$").matchEntire(filename) ?: return null
    val versionCode = match.groupValues[1].toLongOrNull() ?: return null
    val buildTimeMillis = match.groupValues[2].toLongOrNull() ?: return null
    return AndroidUpdateIdentity(versionCode, buildTimeMillis)
}

fun backupSummary(counts: List<BackupQueueCount>, networkAllowed: Boolean): String {
    val total = counts.sumOf { it.count }
    val uploaded = counts.filter { it.state == BackupState.SERVER_PROCESSING || it.state == BackupState.COMPLETED }.sumOf { it.count }
    val failed = counts.filter { it.state == BackupState.TERMINAL_FAILED || it.state == BackupState.CANCELLED }.sumOf { it.count }
    val cancelling = counts.filter { it.state == BackupState.CANCELLING }.sumOf { it.count }
    if (cancelling > 0) return "$uploaded/$total media uploaded, $cancelling cancelling..."
    if (failed > 0) return "$uploaded/$total media uploaded, $failed failed."
    if (uploaded == total) return "$uploaded/$total media uploaded, all set."
    if (!networkAllowed) return "$uploaded/$total media uploaded, pausing"
    return "$uploaded/$total media uploaded, uploading..."
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
    val queueCounts by remember(database, settings.cameraOnly) {
        database.backupAssetDao().observeCounts(settings.cameraOnly)
    }.collectAsState(initial = emptyList())
    val networkAllowed by remember(context, settings.mobileDataEnabled) {
        observeBackupNetworkAllowed(context.applicationContext, settings.mobileDataEnabled)
    }.collectAsState(
        initial = isBackupNetworkAllowed(context.applicationContext, settings.mobileDataEnabled),
    )
    var passwordDialog by remember { mutableStateOf(false) }
    var themeDialog by remember { mutableStateOf(false) }
    var logoutDialog by remember { mutableStateOf(false) }
    var updateBusy by remember { mutableStateOf(false) }
    var updateStatus by remember { mutableStateOf("Check the signed-in host for a newer Android release") }
    var pendingUpdatePath by rememberSaveable { mutableStateOf<String?>(null) }
    val scope = rememberCoroutineScope()
    val canCancelBackup = queueCounts.any { (state, count) ->
        count > 0 && state in setOf(
            BackupState.QUEUED,
            BackupState.FAILED,
            BackupState.UPLOADING,
            BackupState.COMPLETING,
            BackupState.SERVER_PROCESSING,
            BackupState.CANCELLING,
        )
    }
    val permissionRequest = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { permissions ->
        if (permissions.values.all { it }) scheduleBackup(context, settings.mobileDataEnabled, true)
    }
    val installerLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        pendingUpdatePath?.let(::File)?.delete()
        pendingUpdatePath = null
        updateStatus = if (result.resultCode == Activity.RESULT_OK) {
            "Android finished the update installer."
        } else {
            "The update was not installed."
        }
    }
    val installPermissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        updateStatus = if (context.packageManager.canRequestPackageInstalls()) {
            "Install permission granted. Tap Update to continue."
        } else {
            "Allow Momento to install updates, then try again."
        }
    }

    suspend fun checkForUpdate() {
        if (updateBusy) return
        if (!context.packageManager.canRequestPackageInstalls()) {
            updateStatus = "Allow Momento to install updates from this host."
            installPermissionLauncher.launch(
                Intent(
                    AndroidSettings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    "package:${context.packageName}".toUri(),
                ),
            )
            return
        }

        updateBusy = true
        updateStatus = "Checking for updates..."
        val updateDirectory = File(context.cacheDir, "app_updates")
        updateDirectory.listFiles()?.forEach { it.delete() }
        val downloadFile = File(updateDirectory, "momento-update.download")
        try {
            repository.downloadAndroidApk(downloadFile)
            val packageInfo = context.packageManager.readArchivePackageInfo(downloadFile)
            if (packageInfo == null) {
                downloadFile.delete()
                updateStatus = "The host did not provide a valid Momento APK."
                return
            }
            val candidateVersionCode = packageInfo.compatibleLongVersionCode()
            val candidateBuildTimeMillis = packageInfo.androidBuildTimeMillis()
            when (
                androidUpdateDecision(
                    installedVersionCode = BuildConfig.VERSION_CODE.toLong(),
                    installedBuildTimeMillis = BuildConfig.BUILD_TIME_MILLIS,
                    candidateVersionCode = candidateVersionCode,
                    candidateBuildTimeMillis = candidateBuildTimeMillis,
                    packageMatches = packageInfo.packageName == context.packageName,
                )
            ) {
                AndroidUpdateDecision.INVALID_PACKAGE -> {
                    downloadFile.delete()
                    updateStatus = "The host APK is not a Momento Android package."
                }
                AndroidUpdateDecision.UP_TO_DATE -> {
                    downloadFile.delete()
                    updateStatus = "Momento ${BuildConfig.VERSION_NAME} is up to date."
                }
                AndroidUpdateDecision.UPDATE_AVAILABLE -> {
                    val candidateIdentity = AndroidUpdateIdentity(candidateVersionCode, requireNotNull(candidateBuildTimeMillis))
                    val updateFile = File(updateDirectory, androidUpdateCacheFilename(candidateIdentity))
                    if (!downloadFile.renameTo(updateFile)) throw IOException("Could not stage the Android update")
                    pendingUpdatePath = updateFile.absolutePath
                    updateStatus = if (candidateVersionCode == BuildConfig.VERSION_CODE.toLong()) {
                        "A newer build of Momento ${BuildConfig.VERSION_NAME} is ready to install."
                    } else {
                        "Version ${packageInfo.versionName ?: candidateVersionCode} is ready to install."
                    }
                    val uri = FileProvider.getUriForFile(
                        context,
                        "${context.packageName}.fileprovider",
                        updateFile,
                    )
                    installerLauncher.launch(
                        Intent(Intent.ACTION_VIEW).apply {
                            setDataAndType(uri, "application/vnd.android.package-archive")
                            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                        },
                    )
                }
            }
        } catch (_: IOException) {
            downloadFile.delete()
            pendingUpdatePath?.let(::File)?.delete()
            pendingUpdatePath = null
            updateStatus = "Could not download an Android update from this host."
        } catch (_: ActivityNotFoundException) {
            downloadFile.delete()
            pendingUpdatePath?.let(::File)?.delete()
            pendingUpdatePath = null
            updateStatus = "No package installer is available on this device."
        } catch (_: SecurityException) {
            downloadFile.delete()
            pendingUpdatePath?.let(::File)?.delete()
            pendingUpdatePath = null
            updateStatus = "Android blocked the update installer."
        } finally {
            updateBusy = false
        }
    }

    DisposableEffect(database) {
        onDispose { database.close() }
    }
    androidx.compose.runtime.LaunchedEffect(Unit) {
        val updateDirectory = File(context.cacheDir, "app_updates")
        File(updateDirectory, "momento-update.download").delete()
        updateDirectory.listFiles()?.forEach { file ->
            val identity = cachedAndroidUpdateIdentity(file.name) ?: return@forEach
            val decision = androidUpdateDecision(
                installedVersionCode = BuildConfig.VERSION_CODE.toLong(),
                installedBuildTimeMillis = BuildConfig.BUILD_TIME_MILLIS,
                candidateVersionCode = identity.versionCode,
                candidateBuildTimeMillis = identity.buildTimeMillis,
                packageMatches = true,
            )
            if (decision != AndroidUpdateDecision.UPDATE_AVAILABLE) file.delete()
        }
    }
    LazyColumn(
        modifier = Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars),
        contentPadding = PaddingValues(bottom = 88.dp),
    ) {
        item {
            Text(
                "Settings",
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.padding(horizontal = 20.dp, vertical = 20.dp),
            )
            ListItem(
                headlineContent = { Text(user?.username ?: "Account") },
                supportingContent = { Text(settings.origin ?: "No server selected") },
                leadingContent = { Icon(Icons.Default.AccountCircle, null) },
                trailingContent = {
                    Row {
                        TextButton(onClick = { passwordDialog = true }) {
                            Text("Change Password")
                        }
                        IconButton(onClick = { logoutDialog = true }) {
                            Icon(Icons.AutoMirrored.Filled.Logout, "Log out")
                        }
                    }
                },
            )
            ListItem(
                headlineContent = { Text("Momento ${BuildConfig.VERSION_NAME}") },
                supportingContent = { Text(updateStatus) },
                leadingContent = { Icon(Icons.Default.SystemUpdate, null) },
                trailingContent = {
                    TextButton(
                        onClick = { scope.launch { checkForUpdate() } },
                        enabled = !updateBusy,
                    ) { Text(if (updateBusy) "Checking" else "Update") }
                },
            )
            HorizontalDivider()
            Text(
                "Backup",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onBackground,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
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
            ListItem(
                headlineContent = { Text("Back up this device") },
                supportingContent = {
                    Column {
                        Text(backupSummary(queueCounts, networkAllowed))
                        Text("Metadata and AI processing run separately on the server schedule.")
                    }
                },
                leadingContent = { Icon(Icons.Default.Backup, null) },
                trailingContent = {
                    Row {
                        if (canCancelBackup) {
                            TextButton(
                                onClick = {
                                    scope.launch {
                                        requestBackupCancellation(
                                            context.applicationContext,
                                            database.backupAssetDao(),
                                        )
                                    }
                                },
                            ) {
                                Text("Cancel")
                            }
                        }
                        TextButton(onClick = { permissionRequest.launch(backupReadPermissions()) }) {
                            Text("Backup Now")
                        }
                    }
                },
            )
            HorizontalDivider()
            ListItem(
                headlineContent = { Text("Appearance") },
                supportingContent = { Text(themePreferenceLabel(settings.themePreference)) },
                leadingContent = { Icon(Icons.Default.Palette, null) },
                modifier = Modifier.clickable { themeDialog = true },
            )
            if (user?.role == "admin") {
                ListItem(
                    headlineContent = { Text("Admin") },
                    leadingContent = { Icon(Icons.Default.AdminPanelSettings, null) },
                    modifier = Modifier.clickable { openAdmin() },
                )
            }
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

@Suppress("DEPRECATION")
private fun PackageManager.readArchivePackageInfo(file: File): PackageInfo? =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getPackageArchiveInfo(
            file.absolutePath,
            PackageManager.PackageInfoFlags.of(PackageManager.GET_META_DATA.toLong()),
        )
    } else {
        getPackageArchiveInfo(file.absolutePath, PackageManager.GET_META_DATA)
    }

private fun PackageInfo.androidBuildTimeMillis(): Long? =
    androidBuildTimeMillis(applicationInfo?.metaData?.getString(ANDROID_BUILD_TIME_METADATA_KEY))

@Suppress("DEPRECATION")
private fun PackageInfo.compatibleLongVersionCode(): Long =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) longVersionCode else versionCode.toLong()

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

package io.github.yzard.momento.feature.settings

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.provider.Settings as AndroidSettings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.net.toUri
import io.github.yzard.momento.BuildConfig
import io.github.yzard.momento.core.data.AndroidUpdateRepository
import kotlinx.coroutines.launch
import java.io.File

@Composable
internal fun AndroidUpdateSettingsSection(repository: AndroidUpdateRepository) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val coordinator = remember(context, repository) {
        AndroidUpdateCoordinator(context.applicationContext, repository)
    }
    var busy by remember { mutableStateOf(false) }
    var status by remember { mutableStateOf("Check the signed-in host for a newer Android release") }
    var pendingUpdatePath by rememberSaveable { mutableStateOf<String?>(null) }

    val installerLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        pendingUpdatePath?.let(::File)?.delete()
        pendingUpdatePath = null
        status = if (result.resultCode == Activity.RESULT_OK) {
            "Android finished the update installer."
        } else {
            "The update was not installed."
        }
    }
    val installPermissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        status = if (context.packageManager.canRequestPackageInstalls()) {
            "Install permission granted. Tap Install update to continue."
        } else {
            "Allow Momento to install updates, then try again."
        }
    }

    fun launchPendingUpdate() {
        val updateFile = pendingUpdatePath?.let(::File) ?: return
        if (!context.packageManager.canRequestPackageInstalls()) {
            status = "The update is ready. Allow Momento to install it."
            installPermissionLauncher.launch(
                Intent(
                    AndroidSettings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                    "package:${context.packageName}".toUri(),
                ),
            )
            return
        }
        try {
            installerLauncher.launch(coordinator.installerIntent(updateFile))
        } catch (_: ActivityNotFoundException) {
            updateFile.delete()
            pendingUpdatePath = null
            status = "No package installer is available on this device."
        } catch (_: SecurityException) {
            updateFile.delete()
            pendingUpdatePath = null
            status = "Android blocked the update installer."
        }
    }

    suspend fun checkForUpdate() {
        if (busy) return
        busy = true
        status = "Checking for updates..."
        when (val result = coordinator.check()) {
            is AndroidUpdateCheckResult.Finished -> {
                pendingUpdatePath = null
                status = result.message
            }
            is AndroidUpdateCheckResult.Available -> {
                pendingUpdatePath = result.file.absolutePath
                status = result.message
                launchPendingUpdate()
            }
        }
        busy = false
    }

    LaunchedEffect(coordinator) { coordinator.clearObsoleteUpdates() }

    ListItem(
        headlineContent = { Text("Momento ${BuildConfig.VERSION_NAME}") },
        supportingContent = { Text(status) },
        trailingContent = {
            SettingsTrailingActions {
                TextButton(
                    onClick = {
                        if (pendingUpdatePath == null) {
                            scope.launch { checkForUpdate() }
                        } else {
                            launchPendingUpdate()
                        }
                    },
                    enabled = !busy,
                ) {
                    Text(
                        when {
                            busy -> "Checking"
                            pendingUpdatePath != null -> "Install update"
                            else -> "Check for update"
                        },
                    )
                }
            }
        },
        leadingContent = { Icon(Icons.Default.SystemUpdate, null) },
    )
}

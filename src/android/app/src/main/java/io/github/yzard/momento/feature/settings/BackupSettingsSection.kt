package io.github.yzard.momento.feature.settings

import android.database.sqlite.SQLiteException
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Backup
import androidx.compose.material.icons.filled.DeleteSweep
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.work.WorkInfo
import androidx.work.WorkManager
import io.github.yzard.momento.core.data.Settings
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.database.BackupDatabase
import io.github.yzard.momento.core.database.BackupIntegritySummary
import io.github.yzard.momento.core.database.BackupQueueCount
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.feature.backup.BackupHistoryClearResult
import io.github.yzard.momento.feature.backup.BackupHistoryRepairResult
import io.github.yzard.momento.feature.backup.PERIODIC_BACKUP_WORK_NAME
import io.github.yzard.momento.feature.backup.backupCanReadOriginalMedia
import io.github.yzard.momento.feature.backup.backupLocationMetadataAccessLabel
import io.github.yzard.momento.feature.backup.backupMediaAccessLabel
import io.github.yzard.momento.feature.backup.backupPermissions
import io.github.yzard.momento.feature.backup.clearBackupHistory
import io.github.yzard.momento.feature.backup.currentBackupLocationMetadataAccess
import io.github.yzard.momento.feature.backup.currentBackupMediaAccess
import io.github.yzard.momento.feature.backup.isBackupNetworkAllowed
import io.github.yzard.momento.feature.backup.observeBackupNetworkAllowed
import io.github.yzard.momento.feature.backup.repairUnverifiedBackupHistory
import io.github.yzard.momento.feature.backup.requestBackupCancellation
import io.github.yzard.momento.feature.backup.scheduleImmediateBackup
import io.github.yzard.momento.feature.backup.schedulePeriodicBackup
import kotlinx.coroutines.launch
import java.io.IOException
import java.text.DateFormat
import java.util.Date

fun backupSummary(counts: List<BackupQueueCount>, networkAllowed: Boolean): String {
    val total = counts.sumOf { it.count }
    val uploaded = counts
        .filter { it.state == BackupState.SERVER_PROCESSING || it.state == BackupState.COMPLETED }
        .sumOf { it.count }
    val failed = counts
        .filter { it.state == BackupState.TERMINAL_FAILED || it.state == BackupState.CANCELLED }
        .sumOf { it.count }
    val cancelling = counts.filter { it.state == BackupState.CANCELLING }.sumOf { it.count }
    if (cancelling > 0) return "$uploaded/$total media uploaded, $cancelling cancelling..."
    if (failed > 0) return "$uploaded/$total media uploaded, $failed failed."
    if (uploaded == total) return "$uploaded/$total media uploaded, all set."
    if (!networkAllowed) return "$uploaded/$total media uploaded, pausing"
    return "$uploaded/$total media uploaded, uploading..."
}

fun backupIntegritySummary(summary: BackupIntegritySummary): String = when {
    summary.completedRecords == 0L -> "No completed backups to verify"
    summary.unverifiedCompletedRecords == 0L ->
        "${summary.verifiedRecords}/${summary.completedRecords} completed backups fully verified against the server"
    else ->
        "${summary.verifiedRecords}/${summary.completedRecords} completed backups fully verified; ${summary.unverifiedCompletedRecords} older backups need re-verification"
}

enum class BackupScheduleStatus { NOT_SCHEDULED, WAITING, RUNNING }

private val ACTIVE_BACKUP_STATES = setOf(
    BackupState.QUEUED,
    BackupState.FAILED,
    BackupState.UPLOADING,
    BackupState.COMPLETING,
    BackupState.SERVER_PROCESSING,
    BackupState.CANCELLING,
)

fun backupHasActiveRecords(counts: List<BackupQueueCount>): Boolean =
    counts.any { (state, count) -> count > 0 && state in ACTIVE_BACKUP_STATES }

fun backupHistoryCanBeCleared(
    counts: List<BackupQueueCount>,
    scheduleStatus: BackupScheduleStatus,
): Boolean = counts.sumOf { it.count } > 0 &&
    !backupHasActiveRecords(counts) &&
    scheduleStatus != BackupScheduleStatus.RUNNING

fun backupScheduleSummary(status: BackupScheduleStatus, nextScheduledAt: String?): String = when (status) {
    BackupScheduleStatus.NOT_SCHEDULED -> "Daily backup is not scheduled"
    BackupScheduleStatus.RUNNING -> "Daily backup is running now"
    BackupScheduleStatus.WAITING -> nextScheduledAt?.let { "Next daily backup: $it" } ?: "Daily backup is scheduled"
}

@Composable
internal fun BackupSettingsSection(
    settings: Settings,
    settingsStore: SettingsStore,
    backupAvailable: Boolean,
) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val database = remember { BackupDatabase.create(context.applicationContext) }
    val queueCounts by remember(database, settings.cameraOnly) {
        database.backupAssetDao().observeCounts(settings.cameraOnly)
    }.collectAsState(initial = emptyList())
    val allQueueCounts by remember(database) {
        database.backupAssetDao().observeCounts(cameraOnly = false)
    }.collectAsState(initial = emptyList())
    val latestBackupError by remember(database) {
        database.backupAssetDao().observeLatestError()
    }.collectAsState(initial = null)
    val backupIntegrity by remember(database) {
        database.backupAssetDao().observeIntegritySummary()
    }.collectAsState(initial = BackupIntegritySummary(0, 0, 0, 0))
    val workManager = remember(context) { WorkManager.getInstance(context.applicationContext) }
    val periodicWorkInfos by remember(workManager) {
        workManager.getWorkInfosForUniqueWorkFlow(PERIODIC_BACKUP_WORK_NAME)
    }.collectAsState(initial = emptyList())
    val networkAllowed by remember(context, settings.mobileDataEnabled) {
        observeBackupNetworkAllowed(context.applicationContext, settings.mobileDataEnabled)
    }.collectAsState(
        initial = isBackupNetworkAllowed(context.applicationContext, settings.mobileDataEnabled),
    )
    var clearDialog by remember { mutableStateOf(false) }
    var repairDialog by remember { mutableStateOf(false) }
    var clearBusy by remember { mutableStateOf(false) }
    var repairBusy by remember { mutableStateOf(false) }
    var historyStatus by remember { mutableStateOf<String?>(null) }
    var mediaAccess by remember { mutableStateOf(currentBackupMediaAccess(context)) }
    var locationAccess by remember { mutableStateOf(currentBackupLocationMetadataAccess(context)) }
    val scope = rememberCoroutineScope()
    val hasRequiredAccess = backupCanReadOriginalMedia(mediaAccess, locationAccess)
    val activePeriodicWork = periodicWorkInfos.firstOrNull { !it.state.isFinished }
    val scheduleStatus = when (activePeriodicWork?.state) {
        WorkInfo.State.RUNNING -> BackupScheduleStatus.RUNNING
        null -> BackupScheduleStatus.NOT_SCHEDULED
        else -> BackupScheduleStatus.WAITING
    }
    val nextScheduledAt = activePeriodicWork?.nextScheduleTimeMillis
        ?.takeIf { it > 0 && it < Long.MAX_VALUE }
        ?.let { DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(it)) }
    val canCancel = backupHasActiveRecords(allQueueCounts)
    val recordCount = allQueueCounts.sumOf { it.count }
    val canClear = backupHistoryCanBeCleared(allQueueCounts, scheduleStatus)
    val canRepair = backupIntegrity.unverifiedCompletedRecords > 0 &&
        !backupHasActiveRecords(allQueueCounts) &&
        scheduleStatus != BackupScheduleStatus.RUNNING
    val historyDescription = historyStatus ?: when {
        recordCount == 0L -> "No local backup history"
        canClear -> "$recordCount local records. Clear them to back up the selected range again."
        else -> "$recordCount local records. Finish or cancel the current backup before clearing."
    }
    val permissionRequest = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        mediaAccess = currentBackupMediaAccess(context)
        locationAccess = currentBackupLocationMetadataAccess(context)
        if (backupCanReadOriginalMedia(mediaAccess, locationAccess)) {
            schedulePeriodicBackup(context.applicationContext, settings.mobileDataEnabled)
            scheduleImmediateBackup(context.applicationContext, settings.mobileDataEnabled)
        }
    }

    DisposableEffect(database) { onDispose { database.close() } }

    Column {
        Text(
            "Backup",
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onBackground,
            fontWeight = androidx.compose.ui.text.font.FontWeight.Bold,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        )
        if (!backupAvailable) {
            ListItem(
                headlineContent = { Text("Device backup unavailable") },
                supportingContent = { Text("This server has disabled Android device backup.") },
                leadingContent = { Icon(Icons.Default.Backup, null) },
            )
        } else {
            SettingsSwitch("Camera folder only", settings.cameraOnly) { enabled ->
                scope.launch {
                    settingsStore.setCameraOnly(enabled)
                    if (hasRequiredAccess) schedulePeriodicBackup(context.applicationContext, settings.mobileDataEnabled)
                }
            }
            SettingsSwitch("Use mobile data", settings.mobileDataEnabled) { enabled ->
                scope.launch {
                    settingsStore.setMobileDataEnabled(enabled)
                    if (hasRequiredAccess) schedulePeriodicBackup(context.applicationContext, enabled)
                }
            }
            ListItem(
                headlineContent = { Text("Back up this device") },
                supportingContent = {
                    Column {
                        Text(backupMediaAccessLabel(mediaAccess))
                        Text(backupLocationMetadataAccessLabel(locationAccess))
                        Text(backupSummary(queueCounts, networkAllowed))
                        Text(backupScheduleSummary(scheduleStatus, nextScheduledAt))
                        latestBackupError?.let { Text("Recent issue: $it") }
                        Text("Metadata and AI processing run separately on the server schedule.")
                    }
                },
                trailingContent = {
                    SettingsTrailingActions {
                        if (canCancel) {
                            TextButton(
                                enabled = !clearBusy,
                                onClick = {
                                    scope.launch {
                                        requestBackupCancellation(
                                            context.applicationContext,
                                            database.backupAssetDao(),
                                            settings.mobileDataEnabled,
                                        )
                                    }
                                },
                            ) { Text("Cancel") }
                        }
                        TextButton(
                            enabled = !clearBusy,
                            onClick = {
                                mediaAccess = currentBackupMediaAccess(context)
                                locationAccess = currentBackupLocationMetadataAccess(context)
                                if (!backupCanReadOriginalMedia(mediaAccess, locationAccess)) {
                                    permissionRequest.launch(backupPermissions(Build.VERSION.SDK_INT))
                                } else {
                                    schedulePeriodicBackup(context.applicationContext, settings.mobileDataEnabled)
                                    scheduleImmediateBackup(context.applicationContext, settings.mobileDataEnabled)
                                }
                            },
                        ) { Text("Back up now") }
                    }
                },
                leadingContent = { Icon(Icons.Default.Backup, null) },
            )
            ListItem(
                headlineContent = { Text("Backup history") },
                supportingContent = {
                    Column {
                        Text(historyDescription)
                        Text(backupIntegritySummary(backupIntegrity))
                    }
                },
                trailingContent = {
                    SettingsTrailingActions {
                        TextButton(
                            onClick = { repairDialog = true },
                            enabled = canRepair && !repairBusy && !clearBusy,
                        ) { Text(if (repairBusy) "Re-verifying" else "Re-verify older backups") }
                        TextButton(
                            onClick = { clearDialog = true },
                            enabled = canClear && !clearBusy,
                            colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                        ) { Text(if (clearBusy) "Clearing" else "Clear backup history") }
                    }
                },
                leadingContent = { Icon(Icons.Default.DeleteSweep, null) },
            )
        }
    }

    if (clearDialog) {
        BackupHistoryDialog(
            title = "Clear backup history?",
            explanation = "This clears every backup record stored on this device. Photos already stored on the server are not deleted. The next backup will upload every photo and video in the currently selected range again.",
            confirmLabel = if (clearBusy) "Clearing" else "Clear records",
            destructive = true,
            busy = clearBusy,
            dismiss = { clearDialog = false },
            confirm = {
                scope.launch {
                    clearBusy = true
                    try {
                        historyStatus = when (
                            val result = clearBackupHistory(
                                context.applicationContext,
                                database.backupAssetDao(),
                                settingsStore,
                                settings.mobileDataEnabled,
                            )
                        ) {
                            is BackupHistoryClearResult.Cleared ->
                                "Cleared ${result.recordCount} local records. Back up now will upload the selected range again."
                            BackupHistoryClearResult.ActiveBackup ->
                                "Backup history was not cleared because a backup is still active."
                        }
                    } catch (_: IOException) {
                        historyStatus = "Could not clear backup history. Try again."
                    } catch (_: SQLiteException) {
                        historyStatus = "Could not clear backup history. Try again."
                    } finally {
                        clearBusy = false
                        clearDialog = false
                    }
                }
            },
        )
    }
    if (repairDialog) {
        BackupHistoryDialog(
            title = "Re-verify older backups?",
            explanation = "Momento will read the original files again and upload only older completed records that lack full verification. Matching originals are deduplicated on the server.",
            confirmLabel = if (repairBusy) "Re-verifying" else "Re-verify",
            destructive = false,
            busy = repairBusy,
            dismiss = { repairDialog = false },
            confirm = {
                scope.launch {
                    repairBusy = true
                    try {
                        historyStatus = when (
                            val result = repairUnverifiedBackupHistory(
                                context.applicationContext,
                                database.backupAssetDao(),
                                settingsStore,
                                settings.mobileDataEnabled,
                            )
                        ) {
                            is BackupHistoryRepairResult.Requeued ->
                                "Queued ${result.recordCount} older backups for lossless re-verification."
                            BackupHistoryRepairResult.ActiveBackup ->
                                "Older backups were not requeued because a backup is still active."
                        }
                    } catch (_: IOException) {
                        historyStatus = "Could not re-verify backup history. Try again."
                    } catch (_: SQLiteException) {
                        historyStatus = "Could not re-verify backup history. Try again."
                    } finally {
                        repairBusy = false
                        repairDialog = false
                    }
                }
            },
        )
    }
}

@Composable
private fun SettingsSwitch(label: String, checked: Boolean, set: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(label) },
        trailingContent = { Switch(checked = checked, onCheckedChange = null) },
        modifier = Modifier.clickable(role = Role.Switch) { set(!checked) },
    )
}

@Composable
private fun BackupHistoryDialog(
    title: String,
    explanation: String,
    confirmLabel: String,
    destructive: Boolean,
    busy: Boolean,
    dismiss: () -> Unit,
    confirm: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = { if (!busy) dismiss() },
        title = { Text(title) },
        text = { Text(explanation) },
        confirmButton = {
            TextButton(
                onClick = confirm,
                enabled = !busy,
                colors = if (destructive) {
                    ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)
                } else {
                    ButtonDefaults.textButtonColors()
                },
            ) { Text(confirmLabel) }
        },
        dismissButton = { TextButton(onClick = dismiss, enabled = !busy) { Text("Cancel") } },
    )
}

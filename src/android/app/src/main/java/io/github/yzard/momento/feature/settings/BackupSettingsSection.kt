package io.github.yzard.momento.feature.settings

import android.content.Intent
import android.content.ActivityNotFoundException
import android.net.Uri
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import io.github.yzard.momento.feature.backup.BackupMediaAccess
import io.github.yzard.momento.feature.backup.BackupLocationMetadataAccess
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import io.github.yzard.momento.feature.backup.backupDiagnosticReport
import io.github.yzard.momento.feature.backup.conciseBackupIssue
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
import androidx.work.await
import io.github.yzard.momento.feature.backup.IMMEDIATE_BACKUP_WORK_NAME
import io.github.yzard.momento.feature.backup.BACKUP_PHASE_KEY
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
    if (total == 0L) return "No media backed up yet"
    if (cancelling > 0) return "$uploaded/$total media uploaded, $cancelling cancelling..."
    if (failed > 0) return "$uploaded/$total media uploaded, $failed failed."
    if (uploaded == total) return "$uploaded/$total media uploaded, all set."
    if (!networkAllowed) return "$uploaded/$total media uploaded, pausing"
    return "$uploaded/$total media uploaded, uploading..."
}

internal fun backupActivitySummary(
    starting: Boolean,
    workState: WorkInfo.State?,
    phase: String?,
    networkAllowed: Boolean,
    counts: List<BackupQueueCount>,
): String = when {
    starting -> "Starting backup…"
    workState == WorkInfo.State.ENQUEUED || workState == WorkInfo.State.BLOCKED ->
        if (networkAllowed) "Backup queued — waiting to start or retry" else "Backup waiting for an allowed network"
    workState == WorkInfo.State.RUNNING && phase == "scanning" -> "Scanning photos and videos…"
    workState == WorkInfo.State.RUNNING && phase != "uploading" -> "Preparing backup…"
    workState == WorkInfo.State.FAILED -> "Backup could not finish. Copy logs for details."
    workState == WorkInfo.State.CANCELLED -> "Backup cancelled"
    workState == WorkInfo.State.SUCCEEDED && counts.isEmpty() -> "Scan complete — no media to back up"
    else -> backupSummary(counts, networkAllowed)
}

internal fun backupPermissionSummary(
    media: BackupMediaAccess,
    location: BackupLocationMetadataAccess,
): String {
    val photos = when (media) {
        BackupMediaAccess.FULL -> "Photos and videos: granted"
        BackupMediaAccess.PARTIAL -> "Photos and videos: partially granted"
        BackupMediaAccess.DENIED -> "Photos and videos: not granted"
    }
    val metadata = if (location == BackupLocationMetadataAccess.PRESERVED) "granted" else "not granted"
    return "$photos · Photo location: $metadata"
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
    val immediateWorkInfos by remember(workManager) {
        workManager.getWorkInfosForUniqueWorkFlow(IMMEDIATE_BACKUP_WORK_NAME)
    }.collectAsState(initial = emptyList())
    var startingBackup by remember { mutableStateOf(false) }
    var startError by remember { mutableStateOf<String?>(null) }
    val periodicWorkInfos by remember(workManager) {
        workManager.getWorkInfosForUniqueWorkFlow(PERIODIC_BACKUP_WORK_NAME)
    }.collectAsState(initial = emptyList())
    val networkAllowed by remember(context, settings.mobileDataEnabled) {
        observeBackupNetworkAllowed(context.applicationContext, settings.mobileDataEnabled)
    }.collectAsState(
        initial = isBackupNetworkAllowed(context.applicationContext, settings.mobileDataEnabled),
    )
    var diagnosticParts by remember { mutableStateOf<List<String>>(emptyList()) }
    var diagnosticPartIndex by remember { mutableStateOf(0) }
    var copyingDiagnostics by remember { mutableStateOf(false) }
    var diagnosticStatus by remember { mutableStateOf<String?>(null) }
    var clearDialog by remember { mutableStateOf(false) }
    var repairDialog by remember { mutableStateOf(false) }
    var clearBusy by remember { mutableStateOf(false) }
    var repairBusy by remember { mutableStateOf(false) }
    var historyStatus by remember { mutableStateOf<String?>(null) }
    var mediaAccess by remember { mutableStateOf(currentBackupMediaAccess(context)) }
    var locationAccess by remember { mutableStateOf(currentBackupLocationMetadataAccess(context)) }
    var permissionSettingsError by remember { mutableStateOf<String?>(null) }
    val lifecycleOwner = LocalLifecycleOwner.current
    DisposableEffect(lifecycleOwner, context) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                mediaAccess = currentBackupMediaAccess(context)
                locationAccess = currentBackupLocationMetadataAccess(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }
    val scope = rememberCoroutineScope()
    val hasRequiredAccess = backupCanReadOriginalMedia(mediaAccess, locationAccess)
    val activePeriodicWork = periodicWorkInfos.firstOrNull { !it.state.isFinished }
    val scheduleStatus = when (activePeriodicWork?.state) {
        WorkInfo.State.RUNNING -> BackupScheduleStatus.RUNNING
        null -> BackupScheduleStatus.NOT_SCHEDULED
        else -> BackupScheduleStatus.WAITING
    }
    val immediateWork = immediateWorkInfos.firstOrNull { !it.state.isFinished } ?: immediateWorkInfos.firstOrNull()
    val visibleWork = activePeriodicWork?.takeIf { it.state == WorkInfo.State.RUNNING } ?: immediateWork
    val backupRunning = startingBackup || visibleWork?.state == WorkInfo.State.RUNNING
    val historyScheduleStatus = if (backupRunning) BackupScheduleStatus.RUNNING else scheduleStatus
    val nextScheduledAt = activePeriodicWork?.nextScheduleTimeMillis
        ?.takeIf { it > 0 && it < Long.MAX_VALUE }
        ?.let { DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(it)) }
    val canCancel = backupHasActiveRecords(allQueueCounts)
    val recordCount = allQueueCounts.sumOf { it.count }
    val canClear = backupHistoryCanBeCleared(allQueueCounts, historyScheduleStatus)
    val canRepair = backupIntegrity.unverifiedCompletedRecords > 0 &&
        !backupHasActiveRecords(allQueueCounts) &&
        historyScheduleStatus != BackupScheduleStatus.RUNNING
    val historyDescription = historyStatus ?: when {
        recordCount == 0L -> "No local backup history"
        canClear -> "$recordCount local records. Clear them to back up the selected range again."
        else -> "$recordCount local records. Finish or cancel the current backup before clearing."
    }
    fun startBackup() {
        startingBackup = true
        startError = null
        scope.launch {
            try {
                schedulePeriodicBackup(context.applicationContext, settings.mobileDataEnabled)
                scheduleImmediateBackup(context.applicationContext, settings.mobileDataEnabled).await()
            } catch (error: Exception) {
                if (error is kotlinx.coroutines.CancellationException) throw error
                startError = "Could not start backup. Try again."
            } finally {
                startingBackup = false
            }
        }
    }
    val permissionRequest = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        mediaAccess = currentBackupMediaAccess(context)
        locationAccess = currentBackupLocationMetadataAccess(context)
        if (backupCanReadOriginalMedia(mediaAccess, locationAccess)) {
            startBackup()
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
            ListItem(
                headlineContent = { Text("Backup permissions") },
                supportingContent = {
                    Column {
                        Text(backupPermissionSummary(mediaAccess, locationAccess))
                        permissionSettingsError?.let { Text(it) }
                    }
                },
                trailingContent = {
                    TextButton(onClick = {
                        permissionSettingsError = null
                        try {
                            context.startActivity(Intent(
                                android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                                Uri.fromParts("package", context.packageName, null),
                            ))
                        } catch (_: ActivityNotFoundException) {
                            permissionSettingsError = "Open system Settings → Apps → Momento → Permissions."
                        }
                    }) { Text("Settings") }
                },
            )
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
                        Text(startError ?: backupActivitySummary(
                            startingBackup, visibleWork?.state,
                            visibleWork?.progress?.getString(BACKUP_PHASE_KEY), networkAllowed, queueCounts,
                        ))
                        Text(backupScheduleSummary(scheduleStatus, nextScheduledAt))
                        latestBackupError?.let { Text("Recent issue: ${conciseBackupIssue(it)}") }
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
                            enabled = !clearBusy && !backupRunning,
                            onClick = {
                                mediaAccess = currentBackupMediaAccess(context)
                                locationAccess = currentBackupLocationMetadataAccess(context)
                                if (!backupCanReadOriginalMedia(mediaAccess, locationAccess)) {
                                    permissionRequest.launch(backupPermissions(Build.VERSION.SDK_INT))
                                } else {
                                    startBackup()
                                }
                            },
                        ) { Text(if (startingBackup) "Starting…" else if (backupRunning) "Backing up…" else "Back up now") }
                    }
                },
                leadingContent = { Icon(Icons.Default.Backup, null) },
            )
            ListItem(
                headlineContent = { Text("Backup diagnostics") },
                supportingContent = { Text(diagnosticStatus ?: "Copy failure details and recent logs for your developer. Includes file names.") },
                trailingContent = {
                    TextButton(enabled = !copyingDiagnostics, onClick = {
                        scope.launch {
                            copyingDiagnostics = true
                            try {
                                if (diagnosticPartIndex >= diagnosticParts.size) {
                                    val report = backupDiagnosticReport(context.applicationContext, settings, database.backupAssetDao())
                                    diagnosticParts = report.chunked(100_000)
                                    diagnosticPartIndex = 0
                                }
                                val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                                clipboard.setPrimaryClip(ClipData.newPlainText("Momento backup diagnostics", diagnosticParts[diagnosticPartIndex]))
                                diagnosticPartIndex += 1
                                diagnosticStatus = if (diagnosticParts.size == 1) "Diagnostic logs copied" else
                                    "Copied part $diagnosticPartIndex of ${diagnosticParts.size}. Paste it before copying the next part."
                            } catch (_: IOException) {
                                diagnosticStatus = "Could not read diagnostic logs. Try again."
                            } catch (_: SQLiteException) {
                                diagnosticStatus = "Could not read backup records. Try again."
                            } catch (_: IllegalStateException) {
                                diagnosticStatus = "Could not copy diagnostic logs. Try again."
                            } finally {
                                copyingDiagnostics = false
                            }
                        }
                    }) { Text(if (copyingDiagnostics) "Preparing…" else if (diagnosticPartIndex < diagnosticParts.size) "Copy next part" else "Copy logs") }
                },
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

package io.github.yzard.momento.feature.backup

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.await
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.database.BackupAssetDao
import java.util.concurrent.TimeUnit

private fun backupConstraints(allowMobileData: Boolean): Constraints = Constraints.Builder()
    .setRequiredNetworkType(if (allowMobileData) NetworkType.CONNECTED else NetworkType.UNMETERED)
    .build()

private fun ensureBackupNotificationChannel(context: Context) {
    context.getSystemService(NotificationManager::class.java).createNotificationChannel(
        NotificationChannel(BACKUP_CHANNEL, "Momento backup", NotificationManager.IMPORTANCE_LOW),
    )
}

fun scheduleImmediateBackup(context: Context, allowMobileData: Boolean) {
    ensureBackupNotificationChannel(context)
    WorkManager.getInstance(context).enqueueUniqueWork(
        IMMEDIATE_BACKUP_WORK_NAME,
        ExistingWorkPolicy.KEEP,
        OneTimeWorkRequestBuilder<BackupWorker>()
            .setConstraints(backupConstraints(allowMobileData))
            .build(),
    )
}

fun schedulePeriodicBackup(context: Context, allowMobileData: Boolean) {
    ensureBackupNotificationChannel(context)
    val workManager = WorkManager.getInstance(context)
    workManager.cancelUniqueWork(LEGACY_BACKUP_WORK_NAME)
    workManager.enqueueUniquePeriodicWork(
        PERIODIC_BACKUP_WORK_NAME,
        ExistingPeriodicWorkPolicy.UPDATE,
        PeriodicWorkRequestBuilder<BackupWorker>(24, TimeUnit.HOURS)
            .setInitialDelay(24, TimeUnit.HOURS)
            .setConstraints(backupConstraints(allowMobileData))
            .build(),
    )
}

sealed interface BackupHistoryClearResult {
    data class Cleared(val recordCount: Int) : BackupHistoryClearResult
    data object ActiveBackup : BackupHistoryClearResult
}

sealed interface BackupHistoryRepairResult {
    data class Requeued(val recordCount: Int) : BackupHistoryRepairResult
    data object ActiveBackup : BackupHistoryRepairResult
}

private suspend fun stopScheduledBackupWork(workManager: WorkManager) {
    workManager.cancelUniqueWork(IMMEDIATE_BACKUP_WORK_NAME).await()
    workManager.cancelUniqueWork(PERIODIC_BACKUP_WORK_NAME).await()
    workManager.cancelUniqueWork(LEGACY_BACKUP_WORK_NAME).await()
}

suspend fun clearBackupHistory(
    context: Context,
    assets: BackupAssetDao,
    settingsStore: SettingsStore,
    allowMobileData: Boolean,
): BackupHistoryClearResult {
    val workManager = WorkManager.getInstance(context)
    stopScheduledBackupWork(workManager)
    if (assets.activeRecordCount() > 0) {
        schedulePeriodicBackup(context, allowMobileData)
        return BackupHistoryClearResult.ActiveBackup
    }

    settingsStore.rotateBackupGeneration()
    val deletedRecords = assets.deleteAll()
    deleteAllBackupSnapshots(context)
    schedulePeriodicBackup(context, allowMobileData)
    return BackupHistoryClearResult.Cleared(deletedRecords)
}

suspend fun repairUnverifiedBackupHistory(
    context: Context,
    assets: BackupAssetDao,
    settingsStore: SettingsStore,
    allowMobileData: Boolean,
): BackupHistoryRepairResult {
    val workManager = WorkManager.getInstance(context)
    stopScheduledBackupWork(workManager)
    if (assets.activeRecordCount() > 0) {
        schedulePeriodicBackup(context, allowMobileData)
        return BackupHistoryRepairResult.ActiveBackup
    }

    val generation = settingsStore.rotateBackupGeneration()
    val requeuedRecords = assets.requeueUnverifiedCompleted(generation)
    schedulePeriodicBackup(context, allowMobileData)
    if (requeuedRecords > 0) {
        scheduleImmediateBackup(context, allowMobileData)
    }
    return BackupHistoryRepairResult.Requeued(requeuedRecords)
}

suspend fun requestBackupCancellation(
    context: Context,
    assets: BackupAssetDao,
    allowMobileData: Boolean,
) {
    assets.requestCancellation()
    val workManager = WorkManager.getInstance(context)
    stopScheduledBackupWork(workManager)
    schedulePeriodicBackup(context, allowMobileData)
    val constraints = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
    workManager.enqueueUniqueWork(
        BACKUP_CANCELLATION_WORK,
        ExistingWorkPolicy.REPLACE,
        OneTimeWorkRequestBuilder<BackupCancellationWorker>().setConstraints(constraints).build(),
    )
}

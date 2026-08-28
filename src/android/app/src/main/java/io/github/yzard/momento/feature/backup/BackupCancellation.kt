package io.github.yzard.momento.feature.backup

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import io.github.yzard.momento.core.data.EncryptedTokenStore
import io.github.yzard.momento.core.data.MomentoRepository
import io.github.yzard.momento.core.data.BackupRepository
import io.github.yzard.momento.core.data.SettingsStore
import io.github.yzard.momento.core.database.BackupAssetDao
import io.github.yzard.momento.core.database.BackupAssetEntity
import io.github.yzard.momento.core.database.BackupDatabase
import io.github.yzard.momento.core.model.BackupState
import io.github.yzard.momento.core.model.BackupUploadResponse
import io.github.yzard.momento.core.network.NetworkClient
import kotlinx.coroutines.flow.first
import retrofit2.HttpException
import java.io.IOException

internal suspend fun cancelBackupAsset(
    asset: BackupAssetEntity,
    assets: BackupAssetDao,
    repository: BackupRepository,
): BackupProgress {
    val uploadId = asset.uploadId
    if (uploadId == null) {
        assets.updateTransfer(asset.uri, BackupState.CANCELLED, 0, null, null, null, asset.protocolVersion, asset.contentHash)
        return BackupProgress.COMPLETED
    }
    return try {
        recordCancellationStatus(asset, repository.cancelBackupUpload(uploadId), assets)
    } catch (error: IOException) {
        assets.updateTransfer(asset.uri, BackupState.CANCELLING, asset.uploadedBytes, uploadId, asset.mediaId, error.message, asset.protocolVersion, asset.contentHash)
        BackupProgress.WAITING_FOR_SERVER
    } catch (error: HttpException) {
        if (!isCancellationRetryable(error.code())) {
            assets.updateTransfer(asset.uri, BackupState.TERMINAL_FAILED, asset.uploadedBytes, uploadId, asset.mediaId, "HTTP ${error.code()} while cancelling", asset.protocolVersion, asset.contentHash)
            return BackupProgress.COMPLETED
        }
        val current = try {
            repository.backupUploadStatus(uploadId)
        } catch (_: IOException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, asset.uploadedBytes, uploadId, asset.mediaId, error.message, asset.protocolVersion, asset.contentHash)
            return BackupProgress.WAITING_FOR_SERVER
        } catch (statusError: HttpException) {
            assets.updateTransfer(asset.uri, BackupState.CANCELLING, asset.uploadedBytes, uploadId, asset.mediaId, "HTTP ${statusError.code()} while checking cancellation", asset.protocolVersion, asset.contentHash)
            return BackupProgress.WAITING_FOR_SERVER
        }
        recordCancellationStatus(asset, current, assets)
    }
}

private suspend fun recordCancellationStatus(
    asset: BackupAssetEntity,
    response: BackupUploadResponse,
    assets: BackupAssetDao,
): BackupProgress {
    val reportedState = cancellationState(response.status)
    val completedHashMatches = reportedState != BackupState.COMPLETED || completedBackupHashMatches(
        asset.protocolVersion,
        asset.contentHash,
        response.contentHash,
    )
    val state = if (completedHashMatches) reportedState else BackupState.TERMINAL_FAILED
    val error = when {
        !completedHashMatches -> "Server completion hash does not match the original snapshot"
        response.error != null -> response.error
        state == BackupState.TERMINAL_FAILED -> "Server upload failed"
        else -> null
    }
    assets.updateTransfer(asset.uri, state, response.uploadedSize, response.uploadId, response.mediaId, error, asset.protocolVersion, asset.contentHash)
    if (state == BackupState.CANCELLING || state == BackupState.SERVER_PROCESSING) {
        return BackupProgress.WAITING_FOR_SERVER
    }
    return BackupProgress.COMPLETED
}

internal fun cancellationState(status: String): BackupState = when (status) {
    "cancelled" -> BackupState.CANCELLED
    "completed" -> BackupState.COMPLETED
    "failed" -> BackupState.TERMINAL_FAILED
    "processing" -> BackupState.SERVER_PROCESSING
    else -> BackupState.CANCELLING
}

internal fun isCancellationRetryable(statusCode: Int): Boolean =
    statusCode == 409 || isRetryable(statusCode)

class BackupCancellationWorker(context: Context, parameters: WorkerParameters) : CoroutineWorker(context, parameters) {
    override suspend fun doWork(): Result {
        val database = BackupDatabase.create(applicationContext)
        return try {
            val settingsStore = SettingsStore(applicationContext)
            val tokenStore = EncryptedTokenStore(applicationContext)
            if (!tokenStore.isAuthenticated.value || settingsStore.settings.first().origin == null) return Result.failure()
            val repository = MomentoRepository(settingsStore, tokenStore, NetworkClient(tokenStore))
            var waiting = false
            for (asset in database.backupAssetDao().cancellationPending()) {
                if (cancelBackupAsset(asset, database.backupAssetDao(), repository) == BackupProgress.WAITING_FOR_SERVER) {
                    waiting = true
                } else {
                    deleteBackupSnapshot(applicationContext, asset.operationId)
                }
            }
            if (waiting) Result.retry() else Result.success()
        } finally {
            database.close()
        }
    }
}

